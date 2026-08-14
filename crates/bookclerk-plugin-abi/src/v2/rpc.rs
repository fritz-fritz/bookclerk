//! Cap'n Proto two-party RPC adapters for ABI v2 role classes.
//!
//! Public types remain [`crate::v2::Destination`] / [`crate::v2::ByteRange`] streams.
//! Capability table indexes stay inside capnp-rpc.

#![allow(clippy::missing_docs_in_private_items)]
#![allow(clippy::arc_with_non_send_sync)] // capnp stubs are `!Send`; vat is LocalSet.

use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;

use capnp_rpc::{rpc_twoparty_capnp, twoparty, RpcSystem};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use super::limits::MAX_STREAM_WINDOW_BYTES;
use super::plugin_v2_capnp::{
    bookclerk_plugin, byte_source, destination as dest_capnp, job_handler, object_metadata,
    plugin_describe, progress_sink, source as source_capnp,
};
use super::roles::{
    ByteRange, Destination, JobHandler, JobHandlerContext, PluginRoot, ProgressSink, ReadResult,
    Source,
};
use super::types::{
    CopyResult, DestinationContext, JobEvent, JobOutcome, ListOptions, ListPage, ObjectInfo,
    ObjectMetadata, PluginDescribe, PutResult, SourceContext, WorkerContext, WriteOptions,
};
use crate::{PluginError, Result};

fn capnp_err(err: PluginError) -> capnp::Error {
    capnp::Error::failed(format!("{}: {}", err.code.as_str(), err.message))
}

fn from_capnp(err: capnp::Error) -> PluginError {
    let extra = err.to_string();
    if extra.contains("payload_too_large") {
        PluginError::payload_too_large(extra)
    } else if extra.contains("deadline_exceeded") {
        PluginError::deadline_exceeded(extra)
    } else if extra.contains("not_found") {
        PluginError::not_found(extra)
    } else if extra.contains("unsupported") {
        PluginError::unsupported(extra)
    } else if extra.contains("forbidden") {
        PluginError::forbidden(extra)
    } else {
        PluginError::internal(extra)
    }
}

fn text_of(r: capnp::text::Reader<'_>) -> String {
    r.to_string().unwrap_or_default()
}

/// Wraps an [`AsyncRead`] as a `ByteSource` capability (pull window = flow control).
pub struct ReadByteSource {
    reader: Arc<Mutex<Pin<Box<dyn AsyncRead + Send>>>>,
    window: u32,
}

impl ReadByteSource {
    /// Builds a source that yields at most `window` bytes per pull.
    #[must_use]
    pub fn new(reader: Pin<Box<dyn AsyncRead + Send>>, window: u32) -> Self {
        Self {
            reader: Arc::new(Mutex::new(reader)),
            window: window.clamp(1, MAX_STREAM_WINDOW_BYTES),
        }
    }
}

impl byte_source::Server for ReadByteSource {
    async fn pull(
        self: Rc<Self>,
        params: byte_source::PullParams,
        mut results: byte_source::PullResults,
    ) -> capnp::Result<()> {
        let max = params.get()?.get_max_bytes();
        let n = (max.min(self.window).max(1)) as usize;
        let mut buf = vec![0u8; n];
        let mut guard = self.reader.lock().await;
        let read = AsyncReadExt::read(&mut *guard, &mut buf)
            .await
            .map_err(|err| capnp::Error::failed(err.to_string()))?;
        buf.truncate(read);
        results.get().set_chunk(&buf);
        results.get().set_done(read == 0);
        Ok(())
    }
}

/// Returns a `ByteSource` client that pulls from `reader`.
#[must_use]
pub fn byte_source_from_async_read(
    reader: Pin<Box<dyn AsyncRead + Send>>,
    window: u32,
) -> byte_source::Client {
    capnp_rpc::new_client(ReadByteSource::new(reader, window))
}

/// Pulls `source` into `writer` using bounded windows.
///
/// # Errors
///
/// Returns a plugin error when the stream or writer fails.
pub async fn pull_byte_source_to_writer<W: AsyncWrite + Unpin>(
    source: byte_source::Client,
    writer: &mut W,
    window: u32,
) -> Result<u64> {
    let window = window.clamp(1, MAX_STREAM_WINDOW_BYTES);
    let mut total = 0u64;
    loop {
        let mut req = source.pull_request();
        req.get().set_max_bytes(window);
        let reply = req.send().promise.await.map_err(from_capnp)?;
        let reader = reply.get().map_err(from_capnp)?;
        let chunk = reader.get_chunk().map_err(from_capnp)?;
        if !chunk.is_empty() {
            writer
                .write_all(chunk)
                .await
                .map_err(|err| PluginError::internal(format!("stream write failed: {err}")))?;
            total += chunk.len() as u64;
        }
        if reader.get_done() || chunk.is_empty() {
            break;
        }
    }
    writer
        .flush()
        .await
        .map_err(|err| PluginError::internal(format!("stream flush failed: {err}")))?;
    Ok(total)
}

fn async_read_from_byte_source(
    client: byte_source::Client,
    window: u32,
) -> Pin<Box<dyn AsyncRead + Send>> {
    let (mut writer, reader) = tokio::io::duplex(window as usize);
    tokio::task::spawn_local(async move {
        let _ = pull_byte_source_to_writer(client, &mut writer, window).await;
    });
    Box::pin(reader)
}

fn fill_metadata(mut b: object_metadata::Builder<'_>, meta: &ObjectMetadata) {
    b.set_key(&meta.key);
    b.set_size(meta.size);
    if let Some(ct) = &meta.content_type {
        b.set_content_type(ct);
    }
    if let Some(etag) = &meta.etag {
        b.set_etag(etag);
    }
    if let Some(sum) = &meta.sha256 {
        b.set_sha256(sum);
    }
}

fn read_metadata(r: object_metadata::Reader<'_>) -> Result<ObjectMetadata> {
    Ok(ObjectMetadata {
        key: text_of(r.get_key().map_err(from_capnp)?),
        size: r.get_size(),
        content_type: {
            let t = text_of(r.get_content_type().map_err(from_capnp)?);
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        },
        etag: {
            let t = text_of(r.get_etag().map_err(from_capnp)?);
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        },
        sha256: {
            let d = r.get_sha256().map_err(from_capnp)?;
            if d.is_empty() {
                None
            } else {
                Some(d.to_vec())
            }
        },
    })
}

/// Cap'n Proto server wrapping a [`Destination`] trait object.
pub struct DestinationServer {
    inner: Arc<dyn Destination>,
    window: u32,
}

impl DestinationServer {
    /// Serves `inner` with the given stream window.
    #[must_use]
    pub fn new(inner: Arc<dyn Destination>, window: u32) -> Self {
        Self {
            inner,
            window: window.clamp(1, MAX_STREAM_WINDOW_BYTES),
        }
    }
}

impl dest_capnp::Server for DestinationServer {
    async fn head(
        self: Rc<Self>,
        params: dest_capnp::HeadParams,
        mut results: dest_capnp::HeadResults,
    ) -> capnp::Result<()> {
        let key = params.get()?.get_key()?.to_string().unwrap_or_default();
        match self.inner.head(&key).await {
            Ok(Some(meta)) => {
                results.get().set_found(true);
                fill_metadata(results.get().get_meta()?, &meta);
                Ok(())
            }
            Ok(None) => {
                results.get().set_found(false);
                Ok(())
            }
            Err(err) => Err(capnp_err(err)),
        }
    }

    async fn list(
        self: Rc<Self>,
        params: dest_capnp::ListParams,
        mut results: dest_capnp::ListResults,
    ) -> capnp::Result<()> {
        let options = {
            let o = params.get()?.get_options()?;
            ListOptions {
                prefix: o.get_prefix().ok().map(text_of).unwrap_or_default(),
                cursor: {
                    let c = o.get_cursor().ok().map(text_of).unwrap_or_default();
                    if c.is_empty() {
                        None
                    } else {
                        Some(c)
                    }
                },
                limit: o.get_limit(),
            }
        };
        let page = self.inner.list(options).await.map_err(capnp_err)?;
        let mut out = results.get().get_page()?;
        if let Some(c) = &page.next_cursor {
            out.set_next_cursor(c);
        }
        let mut list = out.init_objects(page.objects.len() as u32);
        for (i, obj) in page.objects.iter().enumerate() {
            let mut item = list.reborrow().get(i as u32);
            item.set_key(&obj.key);
            item.set_size(obj.size);
        }
        Ok(())
    }

    async fn get(
        self: Rc<Self>,
        params: dest_capnp::GetParams,
        mut results: dest_capnp::GetResults,
    ) -> capnp::Result<()> {
        let p = params.get()?;
        let key = p.get_key().ok().map(text_of).unwrap_or_default();
        let range = p.get_options().ok().and_then(|o| {
            o.get_range().ok().map(|r| ByteRange {
                offset: r.get_offset(),
                length: {
                    let n = r.get_length();
                    if n == 0 {
                        None
                    } else {
                        Some(n)
                    }
                },
            })
        });
        let read = self.inner.get(&key, range).await.map_err(capnp_err)?;
        fill_metadata(results.get().get_meta()?, &read.meta);
        results
            .get()
            .set_body(byte_source_from_async_read(read.body, self.window));
        Ok(())
    }

    async fn put(
        self: Rc<Self>,
        params: dest_capnp::PutParams,
        mut results: dest_capnp::PutResults,
    ) -> capnp::Result<()> {
        let p = params.get()?;
        let key = p.get_key().ok().map(text_of).unwrap_or_default();
        let body = p
            .get_body()
            .ok()
            .ok_or_else(|| capnp::Error::failed("put missing body stream".into()))?;
        let options = p
            .get_options()
            .ok()
            .map(|o| WriteOptions {
                content_type: {
                    let t = o.get_content_type().ok().map(text_of).unwrap_or_default();
                    if t.is_empty() {
                        None
                    } else {
                        Some(t)
                    }
                },
                content_length: {
                    let n = o.get_content_length();
                    if n == 0 {
                        None
                    } else {
                        Some(n)
                    }
                },
                sha256: o.get_sha256().ok().and_then(|d| {
                    if d.is_empty() {
                        None
                    } else {
                        Some(d.to_vec())
                    }
                }),
            })
            .unwrap_or_default();
        let reader = async_read_from_byte_source(body, self.window);
        let put = self
            .inner
            .put(&key, reader, options)
            .await
            .map_err(capnp_err)?;
        let mut out = results.get().get_result()?;
        out.set_key(&put.key);
        out.set_bytes_written(put.bytes_written);
        if let Some(etag) = &put.etag {
            out.set_etag(etag);
        }
        if let Some(sum) = &put.sha256 {
            out.set_sha256(sum);
        }
        Ok(())
    }

    async fn copy(
        self: Rc<Self>,
        params: dest_capnp::CopyParams,
        mut results: dest_capnp::CopyResults,
    ) -> capnp::Result<()> {
        let p = params.get()?;
        let from = p.get_from().ok().map(text_of).unwrap_or_default();
        let to = p.get_to().ok().map(text_of).unwrap_or_default();
        let copy = self.inner.copy(&from, &to).await.map_err(capnp_err)?;
        results
            .get()
            .get_result()?
            .set_bytes_copied(copy.bytes_copied);
        Ok(())
    }

    async fn delete(
        self: Rc<Self>,
        params: dest_capnp::DeleteParams,
        _results: dest_capnp::DeleteResults,
    ) -> capnp::Result<()> {
        let key = params.get()?.get_key()?.to_string().unwrap_or_default();
        self.inner.delete(&key).await.map_err(capnp_err)
    }
}

/// Host-side [`Destination`] over a capnp destination stub.
#[derive(Clone)]
pub struct DestinationClient {
    client: dest_capnp::Client,
    window: u32,
}

impl DestinationClient {
    /// Wraps a capnp destination client.
    #[must_use]
    pub fn new(client: dest_capnp::Client, window: u32) -> Self {
        Self {
            client,
            window: window.clamp(1, MAX_STREAM_WINDOW_BYTES),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Destination for DestinationClient {
    async fn head(&self, key: &str) -> Result<Option<ObjectMetadata>> {
        let mut req = self.client.head_request();
        req.get().set_key(key);
        let reply = req.send().promise.await.map_err(from_capnp)?;
        let r = reply.get().map_err(from_capnp)?;
        if !r.get_found() {
            return Ok(None);
        }
        Ok(Some(read_metadata(r.get_meta().map_err(from_capnp)?)?))
    }

    async fn list(&self, options: ListOptions) -> Result<ListPage> {
        let mut req = self.client.list_request();
        {
            let mut o = req.get().get_options().map_err(from_capnp)?;
            o.set_prefix(&options.prefix);
            if let Some(c) = &options.cursor {
                o.set_cursor(c);
            }
            o.set_limit(options.limit);
        }
        let reply = req.send().promise.await.map_err(from_capnp)?;
        let page = reply
            .get()
            .map_err(from_capnp)?
            .get_page()
            .map_err(from_capnp)?;
        let list = page.get_objects().map_err(from_capnp)?;
        let mut objects = Vec::with_capacity(list.len() as usize);
        for item in list.iter() {
            objects.push(ObjectInfo {
                key: text_of(item.get_key().map_err(from_capnp)?),
                size: item.get_size(),
            });
        }
        let cursor = text_of(page.get_next_cursor().map_err(from_capnp)?);
        Ok(ListPage {
            objects,
            next_cursor: if cursor.is_empty() {
                None
            } else {
                Some(cursor)
            },
        })
    }

    async fn get(&self, key: &str, range: Option<ByteRange>) -> Result<ReadResult> {
        let mut req = self.client.get_request();
        req.get().set_key(key);
        if let Some(range) = range {
            let o = req.get().get_options().map_err(from_capnp)?;
            let mut r = o.get_range().map_err(from_capnp)?;
            r.set_offset(range.offset);
            r.set_length(range.length.unwrap_or(0));
        }
        let reply = req.send().promise.await.map_err(from_capnp)?;
        let r = reply.get().map_err(from_capnp)?;
        let meta = read_metadata(r.get_meta().map_err(from_capnp)?)?;
        let body = r.get_body().map_err(from_capnp)?;
        Ok(ReadResult {
            meta,
            body: async_read_from_byte_source(body, self.window),
        })
    }

    async fn put(
        &self,
        key: &str,
        body: Pin<Box<dyn AsyncRead + Send>>,
        options: WriteOptions,
    ) -> Result<PutResult> {
        let mut req = self.client.put_request();
        req.get().set_key(key);
        req.get()
            .set_body(byte_source_from_async_read(body, self.window));
        {
            let mut o = req.get().get_options().map_err(from_capnp)?;
            if let Some(ct) = &options.content_type {
                o.set_content_type(ct);
            }
            o.set_content_length(options.content_length.unwrap_or(0));
            if let Some(sum) = &options.sha256 {
                o.set_sha256(sum);
            }
        }
        let reply = req.send().promise.await.map_err(from_capnp)?;
        let r = reply
            .get()
            .map_err(from_capnp)?
            .get_result()
            .map_err(from_capnp)?;
        Ok(PutResult {
            key: text_of(r.get_key().map_err(from_capnp)?),
            bytes_written: r.get_bytes_written(),
            etag: {
                let t = text_of(r.get_etag().map_err(from_capnp)?);
                if t.is_empty() {
                    None
                } else {
                    Some(t)
                }
            },
            sha256: {
                let d = r.get_sha256().map_err(from_capnp)?;
                if d.is_empty() {
                    None
                } else {
                    Some(d.to_vec())
                }
            },
        })
    }

    async fn copy(&self, from: &str, to: &str) -> Result<CopyResult> {
        let mut req = self.client.copy_request();
        req.get().set_from(from);
        req.get().set_to(to);
        let reply = req.send().promise.await.map_err(from_capnp)?;
        Ok(CopyResult {
            bytes_copied: reply
                .get()
                .map_err(from_capnp)?
                .get_result()
                .map_err(from_capnp)?
                .get_bytes_copied(),
        })
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let mut req = self.client.delete_request();
        req.get().set_key(key);
        req.send().promise.await.map_err(from_capnp)?;
        Ok(())
    }
}

/// Cap'n Proto server wrapping a [`Source`].
pub struct SourceServer {
    inner: Arc<dyn Source>,
    window: u32,
}

impl SourceServer {
    /// Serves `inner`.
    #[must_use]
    pub fn new(inner: Arc<dyn Source>, window: u32) -> Self {
        Self {
            inner,
            window: window.clamp(1, MAX_STREAM_WINDOW_BYTES),
        }
    }
}

impl source_capnp::Server for SourceServer {
    async fn open(
        self: Rc<Self>,
        params: source_capnp::OpenParams,
        mut results: source_capnp::OpenResults,
    ) -> capnp::Result<()> {
        let key = params.get()?.get_key()?.to_string().unwrap_or_default();
        let read = self.inner.open(&key).await.map_err(capnp_err)?;
        fill_metadata(results.get().get_meta()?, &read.meta);
        results
            .get()
            .set_body(byte_source_from_async_read(read.body, self.window));
        Ok(())
    }
}

/// Host-side [`Source`] over a capnp source stub.
#[derive(Clone)]
pub struct SourceClient {
    client: source_capnp::Client,
    window: u32,
}

impl SourceClient {
    /// Wraps a capnp source client.
    #[must_use]
    pub fn new(client: source_capnp::Client, window: u32) -> Self {
        Self {
            client,
            window: window.clamp(1, MAX_STREAM_WINDOW_BYTES),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Source for SourceClient {
    async fn open(&self, key: &str) -> Result<ReadResult> {
        let mut req = self.client.open_request();
        req.get().set_key(key);
        let reply = req.send().promise.await.map_err(from_capnp)?;
        let r = reply.get().map_err(from_capnp)?;
        Ok(ReadResult {
            meta: read_metadata(r.get_meta().map_err(from_capnp)?)?,
            body: async_read_from_byte_source(r.get_body().map_err(from_capnp)?, self.window),
        })
    }
}

struct ProgressServer {
    inner: Arc<dyn ProgressSink>,
}

impl progress_sink::Server for ProgressServer {
    async fn report(
        self: Rc<Self>,
        params: progress_sink::ReportParams,
        _results: progress_sink::ReportResults,
    ) -> capnp::Result<()> {
        let p = params.get()?;
        let percent = p.get_percent();
        let message = p.get_message().ok().map(text_of).unwrap_or_default();
        self.inner
            .report(percent, &message)
            .await
            .map_err(capnp_err)
    }
}

/// Cap'n Proto server wrapping [`PluginRoot`].
pub struct PluginServer {
    inner: Arc<dyn PluginRoot>,
    window: u32,
}

impl PluginServer {
    /// Serves `inner`.
    #[must_use]
    pub fn new(inner: Arc<dyn PluginRoot>, window: u32) -> Self {
        Self {
            inner,
            window: window.clamp(1, MAX_STREAM_WINDOW_BYTES),
        }
    }
}

fn fill_describe(mut b: plugin_describe::Builder<'_>, d: &PluginDescribe) -> capnp::Result<()> {
    b.set_api_version(d.api_version);
    b.set_id(&d.id);
    b.set_kind(&d.kind);
    if let Some(name) = &d.display_name {
        b.set_display_name(name);
    }
    {
        let mut feats = b.reborrow().init_rpc_features(d.rpc_features.len() as u32);
        for (i, f) in d.rpc_features.iter().enumerate() {
            feats.set(i as u32, f);
        }
    }
    let mut lim = b.get_scalar_limits()?;
    lim.set_max_scalar_bytes(d.scalar_limits.max_scalar_bytes);
    lim.set_max_stream_window_bytes(d.scalar_limits.max_stream_window_bytes);
    lim.set_max_list_page(d.scalar_limits.max_list_page);
    Ok(())
}

impl bookclerk_plugin::Server for PluginServer {
    async fn describe(
        self: Rc<Self>,
        _params: bookclerk_plugin::DescribeParams,
        mut results: bookclerk_plugin::DescribeResults,
    ) -> capnp::Result<()> {
        let d = self.inner.describe().await.map_err(capnp_err)?;
        fill_describe(results.get().get_manifest()?, &d)
    }

    async fn destination(
        self: Rc<Self>,
        params: bookclerk_plugin::DestinationParams,
        mut results: bookclerk_plugin::DestinationResults,
    ) -> capnp::Result<()> {
        let c = params.get()?.get_context()?;
        let ctx = DestinationContext {
            plugin_data_dir: c
                .get_plugin_data_dir()
                .ok()
                .map(text_of)
                .unwrap_or_default(),
            json: c.get_json().ok().map(text_of).unwrap_or_default(),
        };
        let dest = self.inner.destination(ctx).await.map_err(capnp_err)?;
        let client: dest_capnp::Client =
            capnp_rpc::new_client(DestinationServer::new(Arc::from(dest), self.window));
        results.get().set_dest(client);
        Ok(())
    }

    async fn source(
        self: Rc<Self>,
        params: bookclerk_plugin::SourceParams,
        mut results: bookclerk_plugin::SourceResults,
    ) -> capnp::Result<()> {
        let c = params.get()?.get_context()?;
        let ctx = SourceContext {
            plugin_data_dir: c
                .get_plugin_data_dir()
                .ok()
                .map(text_of)
                .unwrap_or_default(),
            json: c.get_json().ok().map(text_of).unwrap_or_default(),
        };
        let src = self.inner.source(ctx).await.map_err(capnp_err)?;
        let client: source_capnp::Client =
            capnp_rpc::new_client(SourceServer::new(Arc::from(src), self.window));
        results.get().set_src(client);
        Ok(())
    }

    async fn worker(
        self: Rc<Self>,
        params: bookclerk_plugin::WorkerParams,
        mut results: bookclerk_plugin::WorkerResults,
    ) -> capnp::Result<()> {
        let c = params.get()?.get_context()?;
        let ctx = WorkerContext {
            job_id: c.get_job_id().ok().map(text_of).unwrap_or_default(),
            plugin_data_dir: c
                .get_plugin_data_dir()
                .ok()
                .map(text_of)
                .unwrap_or_default(),
            json: c.get_json().ok().map(text_of).unwrap_or_default(),
        };
        let handler = self.inner.worker(ctx).await.map_err(capnp_err)?;
        let client: job_handler::Client =
            capnp_rpc::new_client(JobHandlerServer::new(Arc::from(handler), self.window));
        results.get().set_handler(client);
        Ok(())
    }

    async fn shutdown(
        self: Rc<Self>,
        _params: bookclerk_plugin::ShutdownParams,
        _results: bookclerk_plugin::ShutdownResults,
    ) -> capnp::Result<()> {
        self.inner.shutdown().await.map_err(capnp_err)
    }
}

struct JobHandlerServer {
    inner: Arc<dyn JobHandler>,
    window: u32,
}

impl JobHandlerServer {
    fn new(inner: Arc<dyn JobHandler>, window: u32) -> Self {
        Self { inner, window }
    }
}

impl job_handler::Server for JobHandlerServer {
    async fn handle(
        self: Rc<Self>,
        params: job_handler::HandleParams,
        mut results: job_handler::HandleResults,
    ) -> capnp::Result<()> {
        let p = params.get()?;
        let event = p
            .get_event()
            .ok()
            .map(|e| JobEvent {
                event_type: e.get_event_type().ok().map(text_of).unwrap_or_default(),
                json: e.get_json().ok().map(text_of).unwrap_or_default(),
            })
            .ok_or_else(|| capnp::Error::failed("missing job event".into()))?;
        let input = p
            .get_input()
            .ok()
            .ok_or_else(|| capnp::Error::failed("missing input source".into()))?;
        let output = p
            .get_output()
            .ok()
            .ok_or_else(|| capnp::Error::failed("missing output destination".into()))?;
        let progress: Arc<dyn ProgressSink> = match p.get_progress().ok() {
            Some(client) => Arc::new(ProgressClient { client }),
            None => Arc::new(NullProgress),
        };
        let ctx = JobHandlerContext {
            input: Box::new(SourceClient::new(input, self.window)),
            output: Box::new(DestinationClient::new(output, self.window)),
            progress: Box::new(ProgressArc(progress)),
        };
        let outcome = self.inner.handle(event, ctx).await.map_err(capnp_err)?;
        let mut out = results.get().get_outcome()?;
        out.set_ok(outcome.ok);
        out.set_message(&outcome.message);
        out.set_bytes_copied(outcome.bytes_copied);
        Ok(())
    }
}

struct NullProgress;

#[async_trait::async_trait(?Send)]
impl ProgressSink for NullProgress {
    async fn report(&self, _percent: f32, _message: &str) -> Result<()> {
        Ok(())
    }
}

struct ProgressClient {
    client: progress_sink::Client,
}

#[async_trait::async_trait(?Send)]
impl ProgressSink for ProgressClient {
    async fn report(&self, percent: f32, message: &str) -> Result<()> {
        let mut req = self.client.report_request();
        req.get().set_percent(percent);
        req.get().set_message(message);
        req.send().promise.await.map_err(from_capnp)?;
        Ok(())
    }
}

struct ProgressArc(Arc<dyn ProgressSink>);

#[async_trait::async_trait(?Send)]
impl ProgressSink for ProgressArc {
    async fn report(&self, percent: f32, message: &str) -> Result<()> {
        self.0.report(percent, message).await
    }
}

/// Host bootstrap client for a v2 plugin vat.
#[derive(Clone)]
pub struct PluginClient {
    client: bookclerk_plugin::Client,
    /// Negotiated stream window.
    pub window: u32,
}

impl PluginClient {
    /// Wraps a bootstrap client.
    #[must_use]
    pub fn new(client: bookclerk_plugin::Client, window: u32) -> Self {
        Self {
            client,
            window: window.clamp(1, MAX_STREAM_WINDOW_BYTES),
        }
    }

    /// Calls `describe`.
    ///
    /// # Errors
    ///
    /// Returns a plugin error when the RPC fails or the version is not 2.
    pub async fn describe(&self) -> Result<PluginDescribe> {
        let req = self.client.describe_request();
        let reply = req.send().promise.await.map_err(from_capnp)?;
        let m = reply
            .get()
            .map_err(from_capnp)?
            .get_manifest()
            .map_err(from_capnp)?;
        if m.get_api_version() != super::PRODUCT_API_VERSION {
            return Err(PluginError::unsupported(format!(
                "unsupported apiVersion {}",
                m.get_api_version()
            )));
        }
        let feats = m.get_rpc_features().map_err(from_capnp)?;
        let mut rpc_features = Vec::new();
        for f in feats.iter() {
            rpc_features.push(f.map_err(from_capnp)?.to_string().unwrap_or_default());
        }
        let lim = m.get_scalar_limits().map_err(from_capnp)?;
        Ok(PluginDescribe {
            api_version: m.get_api_version(),
            id: text_of(m.get_id().map_err(from_capnp)?),
            kind: text_of(m.get_kind().map_err(from_capnp)?),
            display_name: {
                let n = text_of(m.get_display_name().map_err(from_capnp)?);
                if n.is_empty() {
                    None
                } else {
                    Some(n)
                }
            },
            rpc_features,
            scalar_limits: super::types::ScalarLimitsDto {
                max_scalar_bytes: lim.get_max_scalar_bytes(),
                max_stream_window_bytes: lim.get_max_stream_window_bytes(),
                max_list_page: lim.get_max_list_page(),
            },
        })
    }

    /// Returns a destination capability.
    ///
    /// # Errors
    ///
    /// Returns a plugin error when the factory call fails.
    pub async fn destination(&self, ctx: DestinationContext) -> Result<DestinationClient> {
        let mut req = self.client.destination_request();
        {
            let mut c = req.get().get_context().map_err(from_capnp)?;
            c.set_plugin_data_dir(&ctx.plugin_data_dir);
            c.set_json(&ctx.json);
        }
        let reply = req.send().promise.await.map_err(from_capnp)?;
        Ok(DestinationClient::new(
            reply
                .get()
                .map_err(from_capnp)?
                .get_dest()
                .map_err(from_capnp)?,
            self.window,
        ))
    }

    /// Returns a source capability.
    ///
    /// # Errors
    ///
    /// Returns a plugin error when the factory call fails.
    pub async fn source(&self, ctx: SourceContext) -> Result<SourceClient> {
        let mut req = self.client.source_request();
        {
            let mut c = req.get().get_context().map_err(from_capnp)?;
            c.set_plugin_data_dir(&ctx.plugin_data_dir);
            c.set_json(&ctx.json);
        }
        let reply = req.send().promise.await.map_err(from_capnp)?;
        Ok(SourceClient::new(
            reply
                .get()
                .map_err(from_capnp)?
                .get_src()
                .map_err(from_capnp)?,
            self.window,
        ))
    }

    /// Returns a job handler capability.
    ///
    /// # Errors
    ///
    /// Returns a plugin error when the factory call fails.
    pub async fn worker(&self, ctx: WorkerContext) -> Result<job_handler::Client> {
        let mut req = self.client.worker_request();
        {
            let mut c = req.get().get_context().map_err(from_capnp)?;
            c.set_job_id(&ctx.job_id);
            c.set_plugin_data_dir(&ctx.plugin_data_dir);
            c.set_json(&ctx.json);
        }
        let reply = req.send().promise.await.map_err(from_capnp)?;
        reply
            .get()
            .map_err(from_capnp)?
            .get_handler()
            .map_err(from_capnp)
    }

    /// Invokes `JobHandler.handle` with host-granted source/destination stubs.
    ///
    /// # Errors
    ///
    /// Returns a plugin error when the handler fails.
    pub async fn handle_job(
        &self,
        handler: job_handler::Client,
        event: JobEvent,
        input: Arc<dyn Source>,
        output: Arc<dyn Destination>,
        progress: Arc<dyn ProgressSink>,
    ) -> Result<JobOutcome> {
        let mut req = handler.handle_request();
        {
            let mut e = req.get().get_event().map_err(from_capnp)?;
            e.set_event_type(&event.event_type);
            e.set_json(&event.json);
        }
        req.get()
            .set_input(capnp_rpc::new_client(SourceServer::new(input, self.window)));
        req.get()
            .set_output(capnp_rpc::new_client(DestinationServer::new(
                output,
                self.window,
            )));
        req.get()
            .set_progress(capnp_rpc::new_client(ProgressServer { inner: progress }));
        let reply = req.send().promise.await.map_err(from_capnp)?;
        let o = reply
            .get()
            .map_err(from_capnp)?
            .get_outcome()
            .map_err(from_capnp)?;
        Ok(JobOutcome {
            ok: o.get_ok(),
            message: text_of(o.get_message().map_err(from_capnp)?),
            bytes_copied: o.get_bytes_copied(),
        })
    }
}

/// Serves `plugin` as the bootstrap object on a two-party vat over `reader`/`writer`.
///
/// Must run inside a `tokio::task::LocalSet`.
///
/// # Errors
///
/// Returns a plugin error when the vat fails.
pub async fn serve_plugin<R, W>(
    plugin: Arc<dyn PluginRoot>,
    reader: R,
    writer: W,
    window: u32,
) -> Result<()>
where
    R: tokio::io::AsyncRead + Unpin + 'static,
    W: tokio::io::AsyncWrite + Unpin + 'static,
{
    let window = window.clamp(1, MAX_STREAM_WINDOW_BYTES);
    let client: bookclerk_plugin::Client = capnp_rpc::new_client(PluginServer::new(plugin, window));
    let network = twoparty::VatNetwork::new(
        reader.compat(),
        writer.compat_write(),
        rpc_twoparty_capnp::Side::Server,
        Default::default(),
    );
    let rpc_system = RpcSystem::new(Box::new(network), Some(client.client));
    rpc_system
        .await
        .map_err(|err| PluginError::internal(err.to_string()))
}

/// Serves `plugin` on stdin/stdout. Must run on a current-thread runtime + `LocalSet`.
///
/// # Errors
///
/// Returns a plugin error when the vat fails.
pub async fn serve_plugin_stdio(plugin: Arc<dyn PluginRoot>, window: u32) -> Result<()> {
    serve_plugin(plugin, tokio::io::stdin(), tokio::io::stdout(), window).await
}

/// Connects as the client side of a two-party vat.
///
/// Must run inside a `tokio::task::LocalSet`. The returned [`RpcSystem`] must be
/// spawned with `tokio::task::spawn_local`.
pub fn connect_plugin<R, W>(
    reader: R,
    writer: W,
    window: u32,
) -> (PluginClient, RpcSystem<rpc_twoparty_capnp::Side>)
where
    R: tokio::io::AsyncRead + Unpin + 'static,
    W: tokio::io::AsyncWrite + Unpin + 'static,
{
    let network = twoparty::VatNetwork::new(
        reader.compat(),
        writer.compat_write(),
        rpc_twoparty_capnp::Side::Client,
        Default::default(),
    );
    let mut rpc_system = RpcSystem::new(Box::new(network), None);
    let client: bookclerk_plugin::Client = rpc_system.bootstrap(rpc_twoparty_capnp::Side::Server);
    (
        PluginClient::new(client, window.clamp(1, MAX_STREAM_WINDOW_BYTES)),
        rpc_system,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::limits::ScalarLimits;
    use crate::v2::types::ScalarLimitsDto;
    use crate::v2::{FEATURE_SCALAR_LIMITS, FEATURE_STREAMS, PRODUCT_API_VERSION};
    use crate::PluginErrorCode;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::io::Cursor;
    use tokio::io::AsyncReadExt;

    struct MemDest {
        objects: Mutex<HashMap<String, Vec<u8>>>,
    }

    #[async_trait(?Send)]
    impl Destination for MemDest {
        async fn head(&self, key: &str) -> Result<Option<ObjectMetadata>> {
            Ok(self.objects.lock().await.get(key).map(|b| ObjectMetadata {
                key: key.into(),
                size: b.len() as u64,
                ..Default::default()
            }))
        }

        async fn list(&self, options: ListOptions) -> Result<ListPage> {
            let guard = self.objects.lock().await;
            let mut keys: Vec<_> = guard
                .iter()
                .filter(|(k, _)| k.starts_with(&options.prefix))
                .map(|(k, v)| ObjectInfo {
                    key: k.clone(),
                    size: v.len() as u64,
                })
                .collect();
            keys.sort_by(|a, b| a.key.cmp(&b.key));
            Ok(ListPage {
                objects: keys,
                next_cursor: None,
            })
        }

        async fn get(&self, key: &str, _range: Option<ByteRange>) -> Result<ReadResult> {
            let data = self
                .objects
                .lock()
                .await
                .get(key)
                .cloned()
                .ok_or_else(|| PluginError::not_found(key))?;
            Ok(ReadResult {
                meta: ObjectMetadata {
                    key: key.into(),
                    size: data.len() as u64,
                    ..Default::default()
                },
                body: Box::pin(Cursor::new(data)),
            })
        }

        async fn put(
            &self,
            key: &str,
            mut body: Pin<Box<dyn AsyncRead + Send>>,
            _options: WriteOptions,
        ) -> Result<PutResult> {
            let mut buf = Vec::new();
            body.read_to_end(&mut buf)
                .await
                .map_err(|err| PluginError::internal(format!("read body: {err}")))?;
            let n = buf.len() as u64;
            self.objects.lock().await.insert(key.to_string(), buf);
            Ok(PutResult {
                key: key.into(),
                bytes_written: n,
                ..Default::default()
            })
        }

        async fn copy(&self, from: &str, to: &str) -> Result<CopyResult> {
            let mut guard = self.objects.lock().await;
            let data = guard
                .get(from)
                .cloned()
                .ok_or_else(|| PluginError::not_found(from))?;
            let n = data.len() as u64;
            guard.insert(to.to_string(), data);
            Ok(CopyResult { bytes_copied: n })
        }

        async fn delete(&self, key: &str) -> Result<()> {
            self.objects.lock().await.remove(key);
            Ok(())
        }
    }

    struct MemPlugin {
        dest: Arc<MemDest>,
    }

    #[async_trait(?Send)]
    impl PluginRoot for MemPlugin {
        async fn describe(&self) -> Result<PluginDescribe> {
            Ok(PluginDescribe {
                api_version: PRODUCT_API_VERSION,
                id: "mem".into(),
                kind: "output".into(),
                display_name: Some("memory".into()),
                rpc_features: vec![FEATURE_SCALAR_LIMITS.into(), FEATURE_STREAMS.into()],
                scalar_limits: ScalarLimitsDto::from(ScalarLimits::default()),
            })
        }

        async fn destination(&self, _ctx: DestinationContext) -> Result<Box<dyn Destination>> {
            Ok(Box::new(MemDest {
                objects: Mutex::new(self.dest.objects.lock().await.clone()),
            }))
        }

        async fn source(&self, _ctx: SourceContext) -> Result<Box<dyn Source>> {
            Err(PluginError::unsupported("source"))
        }

        async fn worker(&self, _ctx: WorkerContext) -> Result<Box<dyn JobHandler>> {
            Ok(Box::new(crate::v2::StreamCopyHandler))
        }
    }

    #[async_trait(?Send)]
    impl Source for MemDest {
        async fn open(&self, key: &str) -> Result<ReadResult> {
            Destination::get(self, key, None).await
        }
    }

    struct TestProgress;

    #[async_trait(?Send)]
    impl ProgressSink for TestProgress {
        async fn report(&self, _percent: f32, _message: &str) -> Result<()> {
            Ok(())
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn capnp_destination_roundtrip_stream() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let (client_stream, server_stream) = tokio::io::duplex(64 * 1024);
                let (server_r, server_w) = tokio::io::split(server_stream);
                let (client_r, client_w) = tokio::io::split(client_stream);
                let plugin = Arc::new(MemPlugin {
                    dest: Arc::new(MemDest {
                        objects: Mutex::new(HashMap::new()),
                    }),
                });
                tokio::task::spawn_local(async move {
                    let _ = serve_plugin(plugin, server_r, server_w, 64 * 1024).await;
                });
                let (client, rpc) = connect_plugin(client_r, client_w, 64 * 1024);
                tokio::task::spawn_local(rpc);
                let desc = client.describe().await.unwrap();
                assert_eq!(desc.api_version, PRODUCT_API_VERSION);
                let dest = client
                    .destination(DestinationContext::default())
                    .await
                    .unwrap();
                let payload = vec![7u8; 300_000];
                dest.put(
                    "big",
                    Box::pin(Cursor::new(payload.clone())),
                    WriteOptions::default(),
                )
                .await
                .unwrap();
                let got = dest.get("big", None).await.unwrap();
                assert_eq!(got.meta.size, payload.len() as u64);
                let mut out = Vec::new();
                let mut body = got.body;
                body.read_to_end(&mut out).await.unwrap();
                assert_eq!(out, payload);
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn capnp_job_handler_stream_copy() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let (client_stream, server_stream) = tokio::io::duplex(64 * 1024);
                let (server_r, server_w) = tokio::io::split(server_stream);
                let (client_r, client_w) = tokio::io::split(client_stream);
                let store = Arc::new(MemDest {
                    objects: Mutex::new(HashMap::new()),
                });
                store
                    .put(
                        "from",
                        Box::pin(Cursor::new(b"stream-copy-payload".to_vec())),
                        WriteOptions::default(),
                    )
                    .await
                    .unwrap();
                let plugin = Arc::new(MemPlugin {
                    dest: Arc::clone(&store),
                });
                tokio::task::spawn_local(async move {
                    let _ = serve_plugin(plugin, server_r, server_w, 64 * 1024).await;
                });
                let (client, rpc) = connect_plugin(client_r, client_w, 64 * 1024);
                tokio::task::spawn_local(rpc);
                let handler = client.worker(WorkerContext::default()).await.unwrap();
                let event = JobEvent {
                    event_type: "stream_copy".into(),
                    json: serde_json::to_string(&crate::v2::StreamCopySpec {
                        from: "from".into(),
                        to: "to".into(),
                    })
                    .unwrap(),
                };
                let input: Arc<dyn Source> = Arc::clone(&store) as Arc<dyn Source>;
                let output: Arc<dyn Destination> = Arc::clone(&store) as Arc<dyn Destination>;
                let progress: Arc<dyn ProgressSink> = Arc::new(TestProgress);
                let outcome = client
                    .handle_job(handler, event, input, output, progress)
                    .await
                    .unwrap();
                assert!(outcome.ok);
                assert_eq!(outcome.bytes_copied, 19);
                let got = store.get("to", None).await.unwrap();
                let mut out = Vec::new();
                let mut body = got.body;
                body.read_to_end(&mut out).await.unwrap();
                assert_eq!(out, b"stream-copy-payload");
            })
            .await;
    }

    struct PatternReader {
        remaining: u64,
        pos: u64,
    }

    impl tokio::io::AsyncRead for PatternReader {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            let n = (self.remaining as usize)
                .min(buf.remaining())
                .min(64 * 1024);
            if n == 0 {
                return std::task::Poll::Ready(Ok(()));
            }
            for i in 0..n {
                buf.put_slice(&[((self.pos + i as u64) % 251) as u8]);
            }
            self.pos += n as u64;
            self.remaining -= n as u64;
            std::task::Poll::Ready(Ok(()))
        }
    }

    struct CountingDest {
        written: Mutex<u64>,
    }

    #[async_trait(?Send)]
    impl Destination for CountingDest {
        async fn head(&self, _key: &str) -> Result<Option<ObjectMetadata>> {
            Ok(None)
        }
        async fn list(&self, _options: ListOptions) -> Result<ListPage> {
            Ok(ListPage::default())
        }
        async fn get(&self, key: &str, _range: Option<ByteRange>) -> Result<ReadResult> {
            let size: u64 = key.parse().unwrap_or(0);
            Ok(ReadResult {
                meta: ObjectMetadata {
                    key: key.into(),
                    size,
                    ..Default::default()
                },
                body: Box::pin(PatternReader {
                    remaining: size,
                    pos: 0,
                }),
            })
        }
        async fn put(
            &self,
            key: &str,
            mut body: Pin<Box<dyn AsyncRead + Send>>,
            _options: WriteOptions,
        ) -> Result<PutResult> {
            let mut n = 0u64;
            let mut buf = [0u8; 65536];
            loop {
                let read = body
                    .read(&mut buf)
                    .await
                    .map_err(|err| PluginError::internal(format!("read body: {err}")))?;
                if read == 0 {
                    break;
                }
                n += read as u64;
            }
            *self.written.lock().await = n;
            Ok(PutResult {
                key: key.into(),
                bytes_written: n,
                ..Default::default()
            })
        }
        async fn copy(&self, _from: &str, _to: &str) -> Result<CopyResult> {
            Err(PluginError::unsupported("copy"))
        }
        async fn delete(&self, _key: &str) -> Result<()> {
            Ok(())
        }
    }

    struct CountPlugin;

    #[async_trait(?Send)]
    impl PluginRoot for CountPlugin {
        async fn describe(&self) -> Result<PluginDescribe> {
            Ok(PluginDescribe {
                api_version: PRODUCT_API_VERSION,
                id: "count".into(),
                kind: "output".into(),
                display_name: None,
                rpc_features: vec![FEATURE_STREAMS.into(), FEATURE_SCALAR_LIMITS.into()],
                scalar_limits: ScalarLimitsDto::from(ScalarLimits::default()),
            })
        }
        async fn destination(&self, _ctx: DestinationContext) -> Result<Box<dyn Destination>> {
            Ok(Box::new(CountingDest {
                written: Mutex::new(0),
            }))
        }
        async fn source(&self, _ctx: SourceContext) -> Result<Box<dyn Source>> {
            Err(PluginError::unsupported("source"))
        }
        async fn worker(&self, _ctx: WorkerContext) -> Result<Box<dyn JobHandler>> {
            Ok(Box::new(crate::v2::StreamCopyHandler))
        }
    }

    fn rss_kib() -> Option<u64> {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:") {
                return rest.split_whitespace().next().and_then(|s| s.parse().ok());
            }
        }
        None
    }

    #[tokio::test(flavor = "current_thread")]
    async fn capnp_lazy_multi_mib_stream_stays_bounded() {
        const PAYLOAD: u64 = 32 * 1024 * 1024;
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let (client_stream, server_stream) = tokio::io::duplex(64 * 1024);
                let (server_r, server_w) = tokio::io::split(server_stream);
                let (client_r, client_w) = tokio::io::split(client_stream);
                let plugin = Arc::new(CountPlugin);
                tokio::task::spawn_local(async move {
                    let _ = serve_plugin(plugin, server_r, server_w, 64 * 1024).await;
                });
                let (client, rpc) = connect_plugin(client_r, client_w, 64 * 1024);
                tokio::task::spawn_local(rpc);
                let dest = client
                    .destination(DestinationContext::default())
                    .await
                    .unwrap();
                let before = rss_kib();
                let put = dest
                    .put(
                        "out",
                        Box::pin(PatternReader {
                            remaining: PAYLOAD,
                            pos: 0,
                        }),
                        WriteOptions {
                            content_length: Some(PAYLOAD),
                            ..Default::default()
                        },
                    )
                    .await
                    .unwrap();
                assert_eq!(put.bytes_written, PAYLOAD);
                if let (Some(before), Some(after)) = (before, rss_kib()) {
                    let grew = after.saturating_sub(before);
                    assert!(
                        grew < 16 * 1024,
                        "RSS grew {grew} KiB during {PAYLOAD} byte stream (budget 16 MiB)"
                    );
                }
                let got = dest.get(&PAYLOAD.to_string(), None).await.unwrap();
                let mut n = 0u64;
                let mut body = got.body;
                let mut buf = [0u8; 65536];
                loop {
                    let r = body.read(&mut buf).await.unwrap();
                    if r == 0 {
                        break;
                    }
                    n += r as u64;
                }
                assert_eq!(n, PAYLOAD);
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn capnp_rejects_wrong_api_version_in_describe() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                struct Bad;
                #[async_trait(?Send)]
                impl PluginRoot for Bad {
                    async fn describe(&self) -> Result<PluginDescribe> {
                        Ok(PluginDescribe {
                            api_version: 1,
                            id: "bad".into(),
                            kind: "output".into(),
                            display_name: None,
                            rpc_features: vec![],
                            scalar_limits: ScalarLimitsDto::from(ScalarLimits::default()),
                        })
                    }
                    async fn destination(
                        &self,
                        _ctx: DestinationContext,
                    ) -> Result<Box<dyn Destination>> {
                        Err(PluginError::unsupported("destination"))
                    }
                    async fn source(&self, _ctx: SourceContext) -> Result<Box<dyn Source>> {
                        Err(PluginError::unsupported("source"))
                    }
                    async fn worker(&self, _ctx: WorkerContext) -> Result<Box<dyn JobHandler>> {
                        Err(PluginError::unsupported("worker"))
                    }
                }
                let (client_stream, server_stream) = tokio::io::duplex(16 * 1024);
                let (server_r, server_w) = tokio::io::split(server_stream);
                let (client_r, client_w) = tokio::io::split(client_stream);
                tokio::task::spawn_local(async move {
                    let _ = serve_plugin(Arc::new(Bad), server_r, server_w, 16 * 1024).await;
                });
                let (client, rpc) = connect_plugin(client_r, client_w, 16 * 1024);
                tokio::task::spawn_local(rpc);
                let err = client.describe().await.unwrap_err();
                assert_eq!(err.code, PluginErrorCode::Unsupported);
            })
            .await;
    }
}
