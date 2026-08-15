//! Cap'n Proto two-party RPC adapters for ABI v2 role classes.
//!
//! Public types remain [`crate::v2::Destination`] / [`crate::v2::ByteRange`] streams.
//! Capability table indexes stay inside capnp-rpc. Method results are typed
//! success/error unions; SDKs map `err` onto [`PluginError`].

#![allow(clippy::missing_docs_in_private_items)]
#![allow(clippy::arc_with_non_send_sync)] // capnp stubs are `!Send`; vat is LocalSet.

use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Poll};

use capnp_rpc::{rpc_twoparty_capnp, twoparty, RpcSystem};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, Mutex};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use super::limits::{ScalarLimits, MAX_LIST_PAGE, MAX_STREAM_WINDOW_BYTES};
use super::plugin_v2_capnp::{
    bookclerk_plugin, byte_source, cancellation, copy_reply, describe_reply,
    destination as dest_iface, destination_reply, empty_reply, get_reply, handle_reply, head_reply,
    job_handler, job_invocation, job_outcome, list_reply, object_metadata, open_reply,
    plugin_describe, plugin_error, progress_sink, pull_reply, put_reply, source as source_capnp,
    source_reply, worker_reply,
};
use super::roles::{
    ByteRange, Cancellation, Destination, JobHandler, JobHandlerContext, NeverCancel, PluginRoot,
    ProgressSink, ReadResult, Source,
};
use super::types::{
    CopyResult, DestinationContext, JobCheckpoint, JobInvocation, JobOutcome, ListOptions,
    ListPage, ObjectInfo, ObjectMetadata, PluginDescribe, PutResult, SourceContext, WorkerContext,
    WriteOptions, ENVELOPE_VERSION, MAX_CHECKPOINT_BYTES,
};
use crate::{PluginError, Result};

fn from_capnp(err: impl std::fmt::Display) -> PluginError {
    PluginError::unavailable(err.to_string())
}

fn text_of(r: capnp::text::Reader<'_>) -> String {
    r.to_string().unwrap_or_default()
}

fn write_error(mut b: plugin_error::Builder<'_>, err: &PluginError) {
    b.set_code(err.wire_str());
    b.set_message(&err.message);
}

fn read_error(r: plugin_error::Reader<'_>) -> PluginError {
    let code = r.get_code().ok().map(text_of).unwrap_or_default();
    let message = r.get_message().ok().map(text_of).unwrap_or_default();
    PluginError::from_wire(&code, message)
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

fn fill_job_outcome(b: job_outcome::Builder<'_>, outcome: &JobOutcome) -> capnp::Result<()> {
    match outcome {
        JobOutcome::Completed {
            message,
            bytes_copied,
        } => {
            let mut c = b.init_completed();
            c.set_message(message);
            c.set_bytes_copied(*bytes_copied);
        }
        JobOutcome::Retryable {
            message,
            retry_after_unix_ms,
        } => {
            let mut c = b.init_retryable();
            c.set_message(message);
            c.set_retry_after_unix_ms(retry_after_unix_ms.unwrap_or(0));
        }
        JobOutcome::Rejected { message } => {
            b.init_rejected().set_message(message);
        }
        JobOutcome::Cancelled { message } => {
            b.init_cancelled().set_message(message);
        }
        JobOutcome::Suspended {
            checkpoint,
            wake_at_unix_ms,
        } => {
            let mut c = b.init_suspended();
            c.set_checkpoint_json(&checkpoint.json);
            c.set_checkpoint_schema_version(checkpoint.schema_version);
            c.set_wake_at_unix_ms(*wake_at_unix_ms);
        }
    }
    Ok(())
}

fn read_job_outcome(r: job_outcome::Reader<'_>) -> Result<JobOutcome> {
    match r.which().map_err(from_capnp)? {
        job_outcome::Completed(c) => {
            let c = c.map_err(from_capnp)?;
            Ok(JobOutcome::Completed {
                message: text_of(c.get_message().map_err(from_capnp)?),
                bytes_copied: c.get_bytes_copied(),
            })
        }
        job_outcome::Retryable(c) => {
            let c = c.map_err(from_capnp)?;
            let retry = c.get_retry_after_unix_ms();
            Ok(JobOutcome::Retryable {
                message: text_of(c.get_message().map_err(from_capnp)?),
                retry_after_unix_ms: if retry == 0 { None } else { Some(retry) },
            })
        }
        job_outcome::Rejected(c) => {
            let c = c.map_err(from_capnp)?;
            Ok(JobOutcome::Rejected {
                message: text_of(c.get_message().map_err(from_capnp)?),
            })
        }
        job_outcome::Cancelled(c) => {
            let c = c.map_err(from_capnp)?;
            Ok(JobOutcome::Cancelled {
                message: text_of(c.get_message().map_err(from_capnp)?),
            })
        }
        job_outcome::Suspended(c) => {
            let c = c.map_err(from_capnp)?;
            Ok(JobOutcome::Suspended {
                checkpoint: JobCheckpoint {
                    schema_version: c.get_checkpoint_schema_version(),
                    json: text_of(c.get_checkpoint_json().map_err(from_capnp)?),
                },
                wake_at_unix_ms: c.get_wake_at_unix_ms(),
            })
        }
    }
}

fn fill_invocation(
    mut b: job_invocation::Builder<'_>,
    invocation: &JobInvocation,
) -> capnp::Result<()> {
    b.set_envelope_version(invocation.envelope_version);
    b.set_payload_schema_version(invocation.payload_schema_version);
    b.set_invocation_id(&invocation.invocation_id);
    b.set_command_type(&invocation.command_type);
    b.set_payload_json(&invocation.payload_json);
    b.set_idempotency_key(&invocation.idempotency_key);
    b.set_attempt(invocation.attempt);
    b.set_correlation_id(&invocation.correlation_id);
    if let Some(c) = &invocation.causation_id {
        b.set_causation_id(c);
    }
    b.set_deadline_unix_ms(invocation.deadline_unix_ms);
    if let Some(cp) = &invocation.checkpoint {
        b.set_checkpoint_json(&cp.json);
        b.set_checkpoint_schema_version(cp.schema_version);
    }
    Ok(())
}

fn read_invocation(r: job_invocation::Reader<'_>) -> Result<JobInvocation> {
    let envelope_version = r.get_envelope_version();
    if envelope_version != 0 && envelope_version != ENVELOPE_VERSION {
        return Err(PluginError::unsupported(format!(
            "unsupported job envelope version {envelope_version}"
        )));
    }
    let checkpoint_json = text_of(r.get_checkpoint_json().map_err(from_capnp)?);
    if checkpoint_json.len() > MAX_CHECKPOINT_BYTES as usize {
        return Err(PluginError::payload_too_large(format!(
            "checkpoint of {} bytes exceeds {MAX_CHECKPOINT_BYTES}",
            checkpoint_json.len()
        )));
    }
    let checkpoint = if checkpoint_json.is_empty() && r.get_checkpoint_schema_version() == 0 {
        None
    } else {
        Some(JobCheckpoint {
            schema_version: r.get_checkpoint_schema_version(),
            json: checkpoint_json,
        })
    };
    let causation = text_of(r.get_causation_id().map_err(from_capnp)?);
    Ok(JobInvocation {
        envelope_version: if envelope_version == 0 {
            ENVELOPE_VERSION
        } else {
            envelope_version
        },
        payload_schema_version: r.get_payload_schema_version().max(1),
        invocation_id: text_of(r.get_invocation_id().map_err(from_capnp)?),
        command_type: text_of(r.get_command_type().map_err(from_capnp)?),
        payload_json: text_of(r.get_payload_json().map_err(from_capnp)?),
        idempotency_key: text_of(r.get_idempotency_key().map_err(from_capnp)?),
        attempt: r.get_attempt().max(1),
        correlation_id: text_of(r.get_correlation_id().map_err(from_capnp)?),
        causation_id: if causation.is_empty() {
            None
        } else {
            Some(causation)
        },
        deadline_unix_ms: r.get_deadline_unix_ms(),
        checkpoint,
    })
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
        let result = results.get().init_result();
        match AsyncReadExt::read(&mut *guard, &mut buf).await {
            Ok(read) => {
                buf.truncate(read);
                let mut ok = result.init_ok();
                ok.set_chunk(&buf);
                ok.set_done(read == 0);
            }
            Err(err) => {
                write_error(
                    result.init_err(),
                    &PluginError::internal(format!("byte source read: {err}")),
                );
            }
        }
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
/// A failed `ByteSource.pull` is returned as [`PluginError`] — never as a clean
/// EOF. Destinations must abort rather than commit.
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
        let result = reply
            .get()
            .map_err(from_capnp)?
            .get_result()
            .map_err(from_capnp)?;
        match result.which().map_err(from_capnp)? {
            pull_reply::Ok(ok) => {
                let ok = ok.map_err(from_capnp)?;
                let chunk = ok.get_chunk().map_err(from_capnp)?;
                if !chunk.is_empty() {
                    writer.write_all(chunk).await.map_err(|err| {
                        PluginError::internal(format!("stream write failed: {err}"))
                    })?;
                    total += chunk.len() as u64;
                }
                if ok.get_done() || chunk.is_empty() {
                    break;
                }
            }
            pull_reply::Err(err) => {
                return Err(read_error(err.map_err(from_capnp)?));
            }
        }
    }
    writer
        .flush()
        .await
        .map_err(|err| PluginError::internal(format!("stream flush failed: {err}")))?;
    Ok(total)
}

enum ByteChunk {
    Data(Vec<u8>),
    Eof,
    Err(std::io::Error),
}

struct ByteSourceAsyncRead {
    rx: mpsc::Receiver<ByteChunk>,
    buf: Vec<u8>,
    pos: usize,
    done: bool,
}

impl AsyncRead for ByteSourceAsyncRead {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.done {
            return Poll::Ready(Ok(()));
        }
        if self.pos < self.buf.len() {
            let n = (self.buf.len() - self.pos).min(buf.remaining());
            buf.put_slice(&self.buf[self.pos..self.pos + n]);
            self.pos += n;
            if self.pos >= self.buf.len() {
                self.buf.clear();
                self.pos = 0;
            }
            return Poll::Ready(Ok(()));
        }
        match self.rx.poll_recv(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(None) | Poll::Ready(Some(ByteChunk::Eof)) => {
                self.done = true;
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Some(ByteChunk::Err(err))) => {
                self.done = true;
                Poll::Ready(Err(err))
            }
            Poll::Ready(Some(ByteChunk::Data(chunk))) => {
                self.buf = chunk;
                self.pos = 0;
                self.poll_read(cx, buf)
            }
        }
    }
}

fn async_read_from_byte_source(
    client: byte_source::Client,
    window: u32,
) -> Pin<Box<dyn AsyncRead + Send>> {
    let (tx, rx) = mpsc::channel(4);
    tokio::task::spawn_local(async move {
        let window = window.clamp(1, MAX_STREAM_WINDOW_BYTES);
        loop {
            let mut req = client.pull_request();
            req.get().set_max_bytes(window);
            let chunk = match req.send().promise.await {
                Ok(reply) => match reply.get() {
                    Ok(r) => match r.get_result() {
                        Ok(result) => match result.which() {
                            Ok(pull_reply::Ok(ok)) => match ok {
                                Ok(ok) => match ok.get_chunk() {
                                    Ok(bytes) => {
                                        let done = ok.get_done() || bytes.is_empty();
                                        if bytes.is_empty() {
                                            ByteChunk::Eof
                                        } else {
                                            let data = bytes.to_vec();
                                            if done {
                                                if tx.send(ByteChunk::Data(data)).await.is_err() {
                                                    return;
                                                }
                                                ByteChunk::Eof
                                            } else {
                                                ByteChunk::Data(data)
                                            }
                                        }
                                    }
                                    Err(err) => ByteChunk::Err(std::io::Error::other(err)),
                                },
                                Err(err) => ByteChunk::Err(std::io::Error::other(err)),
                            },
                            Ok(pull_reply::Err(err)) => {
                                let pe = err
                                    .map(read_error)
                                    .unwrap_or_else(|e| PluginError::unavailable(e.to_string()));
                                ByteChunk::Err(std::io::Error::other(pe))
                            }
                            Err(err) => ByteChunk::Err(std::io::Error::other(err)),
                        },
                        Err(err) => ByteChunk::Err(std::io::Error::other(err)),
                    },
                    Err(err) => ByteChunk::Err(std::io::Error::other(err)),
                },
                Err(err) => ByteChunk::Err(std::io::Error::other(err)),
            };
            let eof = matches!(chunk, ByteChunk::Eof | ByteChunk::Err(_));
            if tx.send(chunk).await.is_err() {
                return;
            }
            if eof {
                return;
            }
        }
    });
    Box::pin(ByteSourceAsyncRead {
        rx,
        buf: Vec::new(),
        pos: 0,
        done: false,
    })
}

fn decode_list_page(
    page: super::plugin_v2_capnp::list_page::Reader<'_>,
    max: u32,
) -> Result<ListPage> {
    let list = page.get_objects().map_err(from_capnp)?;
    if list.len() > max {
        return Err(PluginError::payload_too_large(format!(
            "list page of {} objects exceeds {max}",
            list.len()
        )));
    }
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

impl dest_iface::Server for DestinationServer {
    async fn head(
        self: Rc<Self>,
        params: dest_iface::HeadParams,
        mut results: dest_iface::HeadResults,
    ) -> capnp::Result<()> {
        let key = params.get()?.get_key()?.to_string().unwrap_or_default();
        let result = results.get().init_result();
        match self.inner.head(&key).await {
            Ok(Some(meta)) => {
                let mut ok = result.init_ok();
                ok.set_found(true);
                fill_metadata(ok.get_meta()?, &meta);
            }
            Ok(None) => {
                result.init_ok().set_found(false);
            }
            Err(err) => write_error(result.init_err(), &err),
        }
        Ok(())
    }

    async fn list(
        self: Rc<Self>,
        params: dest_iface::ListParams,
        mut results: dest_iface::ListResults,
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
        let result = results.get().init_result();
        match self.inner.list(options).await {
            Ok(page) => {
                let mut out = result.init_ok();
                if let Some(c) = &page.next_cursor {
                    out.set_next_cursor(c);
                }
                let mut list = out.init_objects(page.objects.len() as u32);
                for (i, obj) in page.objects.iter().enumerate() {
                    let mut item = list.reborrow().get(i as u32);
                    item.set_key(&obj.key);
                    item.set_size(obj.size);
                }
            }
            Err(err) => write_error(result.init_err(), &err),
        }
        Ok(())
    }

    async fn get(
        self: Rc<Self>,
        params: dest_iface::GetParams,
        mut results: dest_iface::GetResults,
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
        let result = results.get().init_result();
        match self.inner.get(&key, range).await {
            Ok(read) => {
                let mut ok = result.init_ok();
                fill_metadata(ok.reborrow().get_meta()?, &read.meta);
                ok.set_body(byte_source_from_async_read(read.body, self.window));
            }
            Err(err) => write_error(result.init_err(), &err),
        }
        Ok(())
    }

    async fn put(
        self: Rc<Self>,
        params: dest_iface::PutParams,
        mut results: dest_iface::PutResults,
    ) -> capnp::Result<()> {
        let p = params.get()?;
        let key = p.get_key().ok().map(text_of).unwrap_or_default();
        let body = p.get_body().ok();
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
        let result = results.get().init_result();
        let Some(body) = body else {
            write_error(
                result.init_err(),
                &PluginError::invalid_params("put missing body stream"),
            );
            return Ok(());
        };
        let reader = async_read_from_byte_source(body, self.window);
        match self.inner.put(&key, reader, options).await {
            Ok(put) => {
                let mut out = result.init_ok();
                out.set_key(&put.key);
                out.set_bytes_written(put.bytes_written);
                if let Some(etag) = &put.etag {
                    out.set_etag(etag);
                }
                if let Some(sum) = &put.sha256 {
                    out.set_sha256(sum);
                }
            }
            Err(err) => write_error(result.init_err(), &err),
        }
        Ok(())
    }

    async fn copy(
        self: Rc<Self>,
        params: dest_iface::CopyParams,
        mut results: dest_iface::CopyResults,
    ) -> capnp::Result<()> {
        let p = params.get()?;
        let from = p.get_from().ok().map(text_of).unwrap_or_default();
        let to = p.get_to().ok().map(text_of).unwrap_or_default();
        let result = results.get().init_result();
        match self.inner.copy(&from, &to).await {
            Ok(copy) => result.init_ok().set_bytes_copied(copy.bytes_copied),
            Err(err) => write_error(result.init_err(), &err),
        }
        Ok(())
    }

    async fn delete(
        self: Rc<Self>,
        params: dest_iface::DeleteParams,
        mut results: dest_iface::DeleteResults,
    ) -> capnp::Result<()> {
        let key = params.get()?.get_key()?.to_string().unwrap_or_default();
        let mut result = results.get().init_result();
        match self.inner.delete(&key).await {
            Ok(()) => {
                result.set_ok(());
            }
            Err(err) => write_error(result.init_err(), &err),
        }
        Ok(())
    }
}

/// Host-side [`Destination`] over a capnp destination stub.
#[derive(Clone)]
pub struct DestinationClient {
    client: dest_iface::Client,
    window: u32,
    max_list_page: u32,
}

impl DestinationClient {
    /// Wraps a capnp destination client.
    #[must_use]
    pub fn new(client: dest_iface::Client, window: u32) -> Self {
        Self {
            client,
            window: window.clamp(1, MAX_STREAM_WINDOW_BYTES),
            max_list_page: MAX_LIST_PAGE,
        }
    }

    /// Applies negotiated list-page cardinality.
    #[must_use]
    pub fn with_max_list_page(mut self, max_list_page: u32) -> Self {
        self.max_list_page = max_list_page.max(1);
        self
    }
}

#[async_trait::async_trait(?Send)]
impl Destination for DestinationClient {
    async fn head(&self, key: &str) -> Result<Option<ObjectMetadata>> {
        let mut req = self.client.head_request();
        req.get().set_key(key);
        let reply = req.send().promise.await.map_err(from_capnp)?;
        let result = reply
            .get()
            .map_err(from_capnp)?
            .get_result()
            .map_err(from_capnp)?;
        match result.which().map_err(from_capnp)? {
            head_reply::Ok(ok) => {
                let ok = ok.map_err(from_capnp)?;
                if !ok.get_found() {
                    return Ok(None);
                }
                Ok(Some(read_metadata(ok.get_meta().map_err(from_capnp)?)?))
            }
            head_reply::Err(err) => Err(read_error(err.map_err(from_capnp)?)),
        }
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
        let result = reply
            .get()
            .map_err(from_capnp)?
            .get_result()
            .map_err(from_capnp)?;
        match result.which().map_err(from_capnp)? {
            list_reply::Ok(page) => decode_list_page(page.map_err(from_capnp)?, self.max_list_page),
            list_reply::Err(err) => Err(read_error(err.map_err(from_capnp)?)),
        }
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
        let result = reply
            .get()
            .map_err(from_capnp)?
            .get_result()
            .map_err(from_capnp)?;
        match result.which().map_err(from_capnp)? {
            get_reply::Ok(ok) => {
                let ok = ok.map_err(from_capnp)?;
                Ok(ReadResult {
                    meta: read_metadata(ok.get_meta().map_err(from_capnp)?)?,
                    body: async_read_from_byte_source(
                        ok.get_body().map_err(from_capnp)?,
                        self.window,
                    ),
                })
            }
            get_reply::Err(err) => Err(read_error(err.map_err(from_capnp)?)),
        }
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
        let result = reply
            .get()
            .map_err(from_capnp)?
            .get_result()
            .map_err(from_capnp)?;
        match result.which().map_err(from_capnp)? {
            put_reply::Ok(r) => {
                let r = r.map_err(from_capnp)?;
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
            put_reply::Err(err) => Err(read_error(err.map_err(from_capnp)?)),
        }
    }

    async fn copy(&self, from: &str, to: &str) -> Result<CopyResult> {
        let mut req = self.client.copy_request();
        req.get().set_from(from);
        req.get().set_to(to);
        let reply = req.send().promise.await.map_err(from_capnp)?;
        let result = reply
            .get()
            .map_err(from_capnp)?
            .get_result()
            .map_err(from_capnp)?;
        match result.which().map_err(from_capnp)? {
            copy_reply::Ok(r) => Ok(CopyResult {
                bytes_copied: r.map_err(from_capnp)?.get_bytes_copied(),
            }),
            copy_reply::Err(err) => Err(read_error(err.map_err(from_capnp)?)),
        }
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let mut req = self.client.delete_request();
        req.get().set_key(key);
        let reply = req.send().promise.await.map_err(from_capnp)?;
        let result = reply
            .get()
            .map_err(from_capnp)?
            .get_result()
            .map_err(from_capnp)?;
        match result.which().map_err(from_capnp)? {
            empty_reply::Ok(()) => Ok(()),
            empty_reply::Err(err) => Err(read_error(err.map_err(from_capnp)?)),
        }
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
        let result = results.get().init_result();
        match self.inner.open(&key).await {
            Ok(read) => {
                let mut ok = result.init_ok();
                fill_metadata(ok.reborrow().get_meta()?, &read.meta);
                ok.set_body(byte_source_from_async_read(read.body, self.window));
            }
            Err(err) => write_error(result.init_err(), &err),
        }
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
        let result = reply
            .get()
            .map_err(from_capnp)?
            .get_result()
            .map_err(from_capnp)?;
        match result.which().map_err(from_capnp)? {
            open_reply::Ok(ok) => {
                let ok = ok.map_err(from_capnp)?;
                Ok(ReadResult {
                    meta: read_metadata(ok.get_meta().map_err(from_capnp)?)?,
                    body: async_read_from_byte_source(
                        ok.get_body().map_err(from_capnp)?,
                        self.window,
                    ),
                })
            }
            open_reply::Err(err) => Err(read_error(err.map_err(from_capnp)?)),
        }
    }
}

struct ProgressServer {
    inner: Arc<dyn ProgressSink>,
}

impl progress_sink::Server for ProgressServer {
    async fn report(
        self: Rc<Self>,
        params: progress_sink::ReportParams,
        mut results: progress_sink::ReportResults,
    ) -> capnp::Result<()> {
        let p = params.get()?;
        let percent = p.get_percent();
        let message = p.get_message().ok().map(text_of).unwrap_or_default();
        let mut result = results.get().init_result();
        match self.inner.report(percent, &message).await {
            Ok(()) => result.set_ok(()),
            Err(err) => write_error(result.init_err(), &err),
        }
        Ok(())
    }
}

struct CancellationServer {
    inner: Arc<dyn Cancellation>,
}

impl cancellation::Server for CancellationServer {
    async fn poll(
        self: Rc<Self>,
        _params: cancellation::PollParams,
        mut results: cancellation::PollResults,
    ) -> capnp::Result<()> {
        let cancelled = self.inner.poll().await.unwrap_or(false);
        results.get().set_cancelled(cancelled);
        Ok(())
    }
}

struct CancellationClient {
    client: cancellation::Client,
}

#[async_trait::async_trait(?Send)]
impl Cancellation for CancellationClient {
    async fn poll(&self) -> Result<bool> {
        let req = self.client.poll_request();
        let reply = req.send().promise.await.map_err(from_capnp)?;
        Ok(reply.get().map_err(from_capnp)?.get_cancelled())
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

impl bookclerk_plugin::Server for PluginServer {
    async fn describe(
        self: Rc<Self>,
        _params: bookclerk_plugin::DescribeParams,
        mut results: bookclerk_plugin::DescribeResults,
    ) -> capnp::Result<()> {
        let result = results.get().init_result();
        match self.inner.describe().await {
            Ok(d) => fill_describe(result.init_ok(), &d)?,
            Err(err) => write_error(result.init_err(), &err),
        }
        Ok(())
    }

    async fn destination(
        self: Rc<Self>,
        params: bookclerk_plugin::DestinationParams,
        mut results: bookclerk_plugin::DestinationResults,
    ) -> capnp::Result<()> {
        let c = params.get()?.get_context()?;
        let ctx = DestinationContext {
            json: c.get_json().ok().map(text_of).unwrap_or_default(),
        };
        let mut result = results.get().init_result();
        match self.inner.destination(ctx).await {
            Ok(dest) => {
                let client: dest_iface::Client =
                    capnp_rpc::new_client(DestinationServer::new(Arc::from(dest), self.window));
                result.set_ok(client);
            }
            Err(err) => write_error(result.init_err(), &err),
        }
        Ok(())
    }

    async fn source(
        self: Rc<Self>,
        params: bookclerk_plugin::SourceParams,
        mut results: bookclerk_plugin::SourceResults,
    ) -> capnp::Result<()> {
        let c = params.get()?.get_context()?;
        let ctx = SourceContext {
            json: c.get_json().ok().map(text_of).unwrap_or_default(),
        };
        let mut result = results.get().init_result();
        match self.inner.source(ctx).await {
            Ok(src) => {
                let client: source_capnp::Client =
                    capnp_rpc::new_client(SourceServer::new(Arc::from(src), self.window));
                result.set_ok(client);
            }
            Err(err) => write_error(result.init_err(), &err),
        }
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
            json: c.get_json().ok().map(text_of).unwrap_or_default(),
        };
        let mut result = results.get().init_result();
        match self.inner.worker(ctx).await {
            Ok(handler) => {
                let client: job_handler::Client =
                    capnp_rpc::new_client(JobHandlerServer::new(Arc::from(handler), self.window));
                result.set_ok(client);
            }
            Err(err) => write_error(result.init_err(), &err),
        }
        Ok(())
    }

    async fn shutdown(
        self: Rc<Self>,
        _params: bookclerk_plugin::ShutdownParams,
        mut results: bookclerk_plugin::ShutdownResults,
    ) -> capnp::Result<()> {
        let mut result = results.get().init_result();
        match self.inner.shutdown().await {
            Ok(()) => result.set_ok(()),
            Err(err) => write_error(result.init_err(), &err),
        }
        Ok(())
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
        let invocation = p
            .get_invocation()
            .map_err(|err| capnp::Error::failed(err.to_string()))
            .and_then(|r| {
                read_invocation(r).map_err(|err| capnp::Error::failed(err.to_string()))
            })?;
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
        let cancel: Box<dyn Cancellation> = match p.get_cancel().ok() {
            Some(client) => Box::new(CancellationClient { client }),
            None => Box::new(NeverCancel),
        };
        let ctx = JobHandlerContext {
            input: Box::new(SourceClient::new(input, self.window)),
            output: Box::new(DestinationClient::new(output, self.window)),
            progress: Box::new(ProgressArc(progress)),
            cancel,
        };
        let result = results.get().init_result();
        match self.inner.handle(invocation, ctx).await {
            Ok(outcome) => fill_job_outcome(result.init_ok(), &outcome)?,
            Err(err) => write_error(result.init_err(), &err),
        }
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
        let reply = req.send().promise.await.map_err(from_capnp)?;
        let result = reply
            .get()
            .map_err(from_capnp)?
            .get_result()
            .map_err(from_capnp)?;
        match result.which().map_err(from_capnp)? {
            empty_reply::Ok(()) => Ok(()),
            empty_reply::Err(err) => Err(read_error(err.map_err(from_capnp)?)),
        }
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
    /// Negotiated scalar / list limits.
    pub limits: ScalarLimits,
}

impl PluginClient {
    /// Wraps a bootstrap client.
    #[must_use]
    pub fn new(client: bookclerk_plugin::Client, window: u32) -> Self {
        let window = window.clamp(1, MAX_STREAM_WINDOW_BYTES);
        Self {
            client,
            window,
            limits: ScalarLimits {
                max_scalar_bytes: super::limits::MAX_SCALAR_BYTES,
                max_stream_window_bytes: window,
                max_list_page: MAX_LIST_PAGE,
            },
        }
    }

    /// Applies negotiated limits (stream window + list page).
    #[must_use]
    pub fn with_limits(mut self, limits: ScalarLimits) -> Self {
        self.limits = limits;
        self.window = limits
            .max_stream_window_bytes
            .clamp(1, MAX_STREAM_WINDOW_BYTES);
        self
    }

    /// Calls `describe`.
    ///
    /// # Errors
    ///
    /// Returns a plugin error when the RPC fails or the version is not 2.
    pub async fn describe(&self) -> Result<PluginDescribe> {
        let req = self.client.describe_request();
        let reply = req.send().promise.await.map_err(from_capnp)?;
        let result = reply
            .get()
            .map_err(from_capnp)?
            .get_result()
            .map_err(from_capnp)?;
        let m = match result.which().map_err(from_capnp)? {
            describe_reply::Ok(m) => m.map_err(from_capnp)?,
            describe_reply::Err(err) => return Err(read_error(err.map_err(from_capnp)?)),
        };
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
            c.set_json(&ctx.json);
        }
        let reply = req.send().promise.await.map_err(from_capnp)?;
        let result = reply
            .get()
            .map_err(from_capnp)?
            .get_result()
            .map_err(from_capnp)?;
        match result.which().map_err(from_capnp)? {
            destination_reply::Ok(dest) => Ok(DestinationClient::new(
                dest.map_err(from_capnp)?,
                self.window,
            )
            .with_max_list_page(self.limits.max_list_page)),
            destination_reply::Err(err) => Err(read_error(err.map_err(from_capnp)?)),
        }
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
            c.set_json(&ctx.json);
        }
        let reply = req.send().promise.await.map_err(from_capnp)?;
        let result = reply
            .get()
            .map_err(from_capnp)?
            .get_result()
            .map_err(from_capnp)?;
        match result.which().map_err(from_capnp)? {
            source_reply::Ok(src) => Ok(SourceClient::new(src.map_err(from_capnp)?, self.window)),
            source_reply::Err(err) => Err(read_error(err.map_err(from_capnp)?)),
        }
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
            c.set_json(&ctx.json);
        }
        let reply = req.send().promise.await.map_err(from_capnp)?;
        let result = reply
            .get()
            .map_err(from_capnp)?
            .get_result()
            .map_err(from_capnp)?;
        match result.which().map_err(from_capnp)? {
            worker_reply::Ok(handler) => handler.map_err(from_capnp),
            worker_reply::Err(err) => Err(read_error(err.map_err(from_capnp)?)),
        }
    }

    /// Invokes `JobHandler.handle` with host-granted source/destination stubs.
    ///
    /// # Errors
    ///
    /// Returns a plugin error when the handler fails.
    pub async fn handle_job(
        &self,
        handler: job_handler::Client,
        invocation: JobInvocation,
        input: Arc<dyn Source>,
        output: Arc<dyn Destination>,
        progress: Arc<dyn ProgressSink>,
    ) -> Result<JobOutcome> {
        self.handle_job_with_cancel(
            handler,
            invocation,
            input,
            output,
            progress,
            Arc::new(NeverCancel),
        )
        .await
    }

    /// [`Self::handle_job`] with an explicit cancellation capability.
    ///
    /// # Errors
    ///
    /// Returns a plugin error when the handler fails.
    pub async fn handle_job_with_cancel(
        &self,
        handler: job_handler::Client,
        invocation: JobInvocation,
        input: Arc<dyn Source>,
        output: Arc<dyn Destination>,
        progress: Arc<dyn ProgressSink>,
        cancel: Arc<dyn Cancellation>,
    ) -> Result<JobOutcome> {
        let mut req = handler.handle_request();
        fill_invocation(req.get().get_invocation().map_err(from_capnp)?, &invocation)
            .map_err(from_capnp)?;
        req.get()
            .set_input(capnp_rpc::new_client(SourceServer::new(input, self.window)));
        req.get()
            .set_output(capnp_rpc::new_client(DestinationServer::new(
                output,
                self.window,
            )));
        req.get()
            .set_progress(capnp_rpc::new_client(ProgressServer { inner: progress }));
        req.get()
            .set_cancel(capnp_rpc::new_client(CancellationServer { inner: cancel }));
        let reply = req.send().promise.await.map_err(from_capnp)?;
        let result = reply
            .get()
            .map_err(from_capnp)?
            .get_result()
            .map_err(from_capnp)?;
        match result.which().map_err(from_capnp)? {
            handle_reply::Ok(o) => read_job_outcome(o.map_err(from_capnp)?),
            handle_reply::Err(err) => Err(read_error(err.map_err(from_capnp)?)),
        }
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
    use crate::v2::{
        ByteRange, CopyResult, Destination, DestinationContext, JobHandler, ListOptions, ListPage,
        ObjectInfo, ObjectMetadata, PluginDescribe, PluginRoot, PutResult, ReadResult,
        ScalarLimits, Source, SourceContext, WorkerContext, WriteOptions, FEATURE_SCALAR_LIMITS,
        FEATURE_STREAMS, MAX_LIST_PAGE, PRODUCT_API_VERSION,
    };
    use crate::{PluginError, PluginErrorCode, Result};
    use std::collections::HashMap;
    use std::sync::Mutex;
    use tokio::io::duplex;

    struct MemDest {
        store: Mutex<HashMap<String, Vec<u8>>>,
    }

    #[async_trait::async_trait(?Send)]
    impl Destination for MemDest {
        async fn head(&self, key: &str) -> Result<Option<ObjectMetadata>> {
            if key == "internal-msg" {
                return Err(PluginError::internal("object not_found in cache"));
            }
            let store = self.store.lock().expect("lock");
            Ok(store.get(key).map(|v| ObjectMetadata {
                key: key.into(),
                size: v.len() as u64,
                ..Default::default()
            }))
        }

        async fn list(&self, _options: ListOptions) -> Result<ListPage> {
            Ok(ListPage {
                objects: (0..MAX_LIST_PAGE + 2)
                    .map(|i| ObjectInfo {
                        key: format!("k{i}"),
                        size: 1,
                    })
                    .collect(),
                next_cursor: None,
            })
        }

        async fn get(&self, key: &str, _range: Option<ByteRange>) -> Result<ReadResult> {
            if key == "fail-mid" {
                return Ok(ReadResult {
                    meta: ObjectMetadata {
                        key: key.into(),
                        size: 100,
                        ..Default::default()
                    },
                    body: Box::pin(FailAfter { remain: 8 }),
                });
            }
            let store = self.store.lock().expect("lock");
            let data = store
                .get(key)
                .cloned()
                .ok_or_else(|| PluginError::not_found(format!("missing {key}")))?;
            let size = data.len() as u64;
            Ok(ReadResult {
                meta: ObjectMetadata {
                    key: key.into(),
                    size,
                    ..Default::default()
                },
                body: Box::pin(std::io::Cursor::new(data)),
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
                .map_err(|err| PluginError::internal(err.to_string()))?;
            let n = buf.len() as u64;
            self.store.lock().expect("lock").insert(key.into(), buf);
            Ok(PutResult {
                key: key.into(),
                bytes_written: n,
                ..Default::default()
            })
        }

        async fn copy(&self, _from: &str, _to: &str) -> Result<CopyResult> {
            Err(PluginError::unsupported("copy"))
        }

        async fn delete(&self, key: &str) -> Result<()> {
            self.store.lock().expect("lock").remove(key);
            Ok(())
        }
    }

    struct FailAfter {
        remain: usize,
    }

    impl AsyncRead for FailAfter {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            if self.remain == 0 {
                return Poll::Ready(Err(std::io::Error::other("source exploded")));
            }
            let n = self.remain.min(buf.remaining()).min(4);
            buf.put_slice(&vec![1u8; n]);
            self.remain -= n;
            Poll::Ready(Ok(()))
        }
    }

    struct TestPlugin {
        dest: Arc<MemDest>,
    }

    #[async_trait::async_trait(?Send)]
    impl PluginRoot for TestPlugin {
        async fn describe(&self) -> Result<PluginDescribe> {
            Ok(PluginDescribe {
                api_version: PRODUCT_API_VERSION,
                id: "native_test".into(),
                kind: "output".into(),
                display_name: None,
                rpc_features: vec![FEATURE_SCALAR_LIMITS.into(), FEATURE_STREAMS.into()],
                scalar_limits: ScalarLimits::default().into(),
            })
        }

        async fn destination(&self, _context: DestinationContext) -> Result<Box<dyn Destination>> {
            Ok(Box::new(DestClone(Arc::clone(&self.dest))))
        }

        async fn source(&self, _context: SourceContext) -> Result<Box<dyn Source>> {
            Err(PluginError::unsupported("source"))
        }

        async fn worker(&self, _context: WorkerContext) -> Result<Box<dyn JobHandler>> {
            Err(PluginError::unsupported("worker"))
        }
    }

    struct DestClone(Arc<MemDest>);

    #[async_trait::async_trait(?Send)]
    impl Destination for DestClone {
        async fn head(&self, key: &str) -> Result<Option<ObjectMetadata>> {
            self.0.head(key).await
        }
        async fn list(&self, options: ListOptions) -> Result<ListPage> {
            self.0.list(options).await
        }
        async fn get(&self, key: &str, range: Option<ByteRange>) -> Result<ReadResult> {
            self.0.get(key, range).await
        }
        async fn put(
            &self,
            key: &str,
            body: Pin<Box<dyn AsyncRead + Send>>,
            options: WriteOptions,
        ) -> Result<PutResult> {
            self.0.put(key, body, options).await
        }
        async fn copy(&self, from: &str, to: &str) -> Result<CopyResult> {
            self.0.copy(from, to).await
        }
        async fn delete(&self, key: &str) -> Result<()> {
            self.0.delete(key).await
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn typed_error_preserves_internal_when_message_says_not_found() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let (client_end, server_end) = duplex(64 * 1024);
                let (server_r, server_w) = tokio::io::split(server_end);
                let (client_r, client_w) = tokio::io::split(client_end);
                let plugin = Arc::new(TestPlugin {
                    dest: Arc::new(MemDest {
                        store: Mutex::new(HashMap::new()),
                    }),
                });
                tokio::task::spawn_local(async move {
                    let _ = serve_plugin(plugin, server_r, server_w, 64 * 1024).await;
                });
                let (client, rpc) = connect_plugin(client_r, client_w, 64 * 1024);
                tokio::task::spawn_local(rpc);
                let dest = client
                    .destination(DestinationContext::default())
                    .await
                    .expect("dest");
                let err = dest.head("internal-msg").await.expect_err("must fail");
                assert_eq!(err.code, PluginErrorCode::Internal);
                assert!(err.message.contains("not_found"));
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn list_page_overflow_is_payload_too_large() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let (client_end, server_end) = duplex(256 * 1024);
                let (server_r, server_w) = tokio::io::split(server_end);
                let (client_r, client_w) = tokio::io::split(client_end);
                let plugin = Arc::new(TestPlugin {
                    dest: Arc::new(MemDest {
                        store: Mutex::new(HashMap::new()),
                    }),
                });
                tokio::task::spawn_local(async move {
                    let _ = serve_plugin(plugin, server_r, server_w, 64 * 1024).await;
                });
                let (client, rpc) = connect_plugin(client_r, client_w, 64 * 1024);
                tokio::task::spawn_local(rpc);
                let dest = client
                    .destination(DestinationContext::default())
                    .await
                    .expect("dest");
                let err = dest
                    .list(ListOptions::default())
                    .await
                    .expect_err("oversize page");
                assert_eq!(err.code, PluginErrorCode::PayloadTooLarge);
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mid_stream_source_failure_is_not_eof() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let (client_end, server_end) = duplex(64 * 1024);
                let (server_r, server_w) = tokio::io::split(server_end);
                let (client_r, client_w) = tokio::io::split(client_end);
                let plugin = Arc::new(TestPlugin {
                    dest: Arc::new(MemDest {
                        store: Mutex::new(HashMap::new()),
                    }),
                });
                tokio::task::spawn_local(async move {
                    let _ = serve_plugin(plugin, server_r, server_w, 64 * 1024).await;
                });
                let (client, rpc) = connect_plugin(client_r, client_w, 64 * 1024);
                tokio::task::spawn_local(rpc);
                let dest = client
                    .destination(DestinationContext::default())
                    .await
                    .expect("dest");
                dest.put(
                    "keep",
                    Box::pin(std::io::Cursor::new(b"original".to_vec())),
                    WriteOptions::default(),
                )
                .await
                .expect("seed");
                let got = dest.get("fail-mid", None).await.expect("open fail-mid");
                let mut body = got.body;
                let mut buf = Vec::new();
                let err = body.read_to_end(&mut buf).await.expect_err("must not eof");
                assert_ne!(err.kind(), std::io::ErrorKind::UnexpectedEof);
                let keep = dest.get("keep", None).await.expect("keep");
                let mut out = Vec::new();
                let mut body = keep.body;
                body.read_to_end(&mut out).await.unwrap();
                assert_eq!(out, b"original");
            })
            .await;
    }
}
