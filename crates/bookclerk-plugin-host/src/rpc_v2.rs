//! Host session for ABI v2 Cap'n Proto guests (object-capability + streams).
//!
//! Cap'n Proto clients are `!Send`, so the vat runs on a dedicated current-thread
//! runtime. Host [`StorageBackend`] methods send work onto that thread.

#![allow(clippy::missing_docs_in_private_items)]
#![allow(clippy::arc_with_non_send_sync)]

use std::pin::Pin;
use std::sync::Arc;
use std::thread;

use anyhow::anyhow;
use async_trait::async_trait;
use bookclerk_config::Config;
use bookclerk_plugin_sdk::v2::{
    connect_plugin, ByteRange as AbiByteRange, Destination, DestinationContext, JobEvent,
    ListOptions, ObjectMetadata, PluginClient, PluginDescribe, PutResult, ReadResult, Source,
    StreamCopySpec, WorkerContext, WriteOptions, MAX_SCALAR_BYTES, MAX_STREAM_WINDOW_BYTES,
    PRODUCT_API_VERSION,
};
use bookclerk_storage::{
    ByteRange, ListPage, ObjectInfo, ObjectMeta, ObjectProbe, PutStreamResult, StorageBackend,
    StorageError,
};
use bytes::Bytes;
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::sync::{mpsc, oneshot};

use crate::discover::DiscoveredPlugin;
use crate::{PluginError, Result};

/// Work item executed on the v2 vat thread.
enum Work {
    /// `BookclerkPlugin.describe`.
    Describe {
        /// Reply channel.
        reply: oneshot::Sender<Result<PluginDescribe>>,
    },
    /// Ensure a destination stub exists for `ctx`.
    EnsureDest {
        /// Destination factory context.
        ctx: DestinationContext,
        /// Reply channel.
        reply: oneshot::Sender<Result<()>>,
    },
    /// `Destination.head`.
    Head {
        /// Object key.
        key: String,
        /// Reply channel.
        reply: oneshot::Sender<Result<Option<ObjectMetadata>>>,
    },
    /// `Destination.list`.
    List {
        /// List options.
        options: ListOptions,
        /// Reply channel.
        reply: oneshot::Sender<Result<bookclerk_plugin_sdk::v2::ListPage>>,
    },
    /// Streamed get.
    GetStream {
        /// Object key.
        key: String,
        /// Optional range.
        range: Option<AbiByteRange>,
        /// Reply channel.
        reply: oneshot::Sender<Result<ReadResult>>,
    },
    /// Streamed put.
    PutStream {
        /// Object key.
        key: String,
        /// Body stream.
        body: Pin<Box<dyn AsyncRead + Send>>,
        /// Write options.
        options: WriteOptions,
        /// Reply channel.
        reply: oneshot::Sender<Result<PutResult>>,
    },
    /// Server-side copy.
    Copy {
        /// Source key.
        from: String,
        /// Destination key.
        to: String,
        /// Reply channel.
        reply: oneshot::Sender<Result<u64>>,
    },
    /// Delete key.
    Delete {
        /// Object key.
        key: String,
        /// Reply channel.
        reply: oneshot::Sender<Result<()>>,
    },
    /// JobHandler stream-copy vertical slice.
    StreamCopy {
        /// Durable job id.
        job_id: String,
        /// Copy spec.
        spec: StreamCopySpec,
        /// Reply channel.
        reply: oneshot::Sender<Result<bookclerk_plugin_sdk::v2::JobOutcome>>,
    },
    /// Drop the vat.
    Shutdown,
}

/// Host-side v2 plugin session (one jailed child + one vat thread).
pub struct V2PluginSession {
    /// Work queue into the vat thread.
    tx: mpsc::UnboundedSender<Work>,
    /// Plugin id.
    id: String,
    /// Guest data directory.
    data: std::path::PathBuf,
}

impl V2PluginSession {
    /// Spawns a v2 guest and connects Cap'n Proto on stdio.
    ///
    /// # Errors
    ///
    /// Fails when the child cannot start, describe fails, or `apiVersion` is not 2.
    pub async fn spawn(
        plugin: &DiscoveredPlugin,
        config: &Config,
        config_table: Value,
    ) -> Result<Self> {
        if plugin.manifest.api_version != PRODUCT_API_VERSION {
            return Err(PluginError::message(format!(
                "plugin `{}` api_version {} is not v2",
                plugin.manifest.id, plugin.manifest.api_version
            )));
        }
        let spawned = crate::spawn_stdio::spawn_stdio_guest(plugin, config, config_table).await?;
        let id = spawned.id.clone();
        let data = spawned.data.clone();
        let (tx, rx) = mpsc::unbounded_channel();
        let (ready_tx, ready_rx) = oneshot::channel::<Result<PluginDescribe>>();
        thread::Builder::new()
            .name(format!("plugin-v2-{}", id))
            .spawn(move || vat_thread(spawned, rx, ready_tx))
            .map_err(|err| PluginError::message(format!("v2 vat thread: {err}")))?;
        let desc = ready_rx
            .await
            .map_err(|err| PluginError::message(format!("v2 vat dropped: {err}")))??;
        if desc.api_version != PRODUCT_API_VERSION {
            return Err(PluginError::message(format!(
                "plugin `{id}` describe apiVersion {} is not {PRODUCT_API_VERSION}",
                desc.api_version
            )));
        }
        Ok(Self { tx, id, data })
    }

    /// Plugin id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Guest data directory.
    #[must_use]
    pub fn data_dir(&self) -> &std::path::Path {
        &self.data
    }

    /// Sends work to the vat thread.
    async fn call<T>(&self, build: impl FnOnce(oneshot::Sender<Result<T>>) -> Work) -> Result<T> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(build(reply))
            .map_err(|_| PluginError::unavailable("v2 vat thread closed"))?;
        rx.await
            .map_err(|_| PluginError::unavailable("v2 vat thread dropped reply"))?
    }

    /// Instantiates the destination stub with `ctx`.
    ///
    /// # Errors
    ///
    /// Returns a plugin error when the factory call fails.
    pub async fn ensure_destination(&self, ctx: DestinationContext) -> Result<()> {
        self.call(|reply| Work::EnsureDest { ctx, reply }).await
    }

    /// Calls `BookclerkPlugin.describe`.
    ///
    /// # Errors
    ///
    /// Returns a plugin error when the RPC fails.
    pub async fn describe(&self) -> Result<PluginDescribe> {
        self.call(|reply| Work::Describe { reply }).await
    }

    /// Runs the stream-copy job handler on the guest.
    ///
    /// # Errors
    ///
    /// Returns a plugin error when the handler fails.
    pub async fn stream_copy(
        &self,
        job_id: &str,
        from: &str,
        to: &str,
    ) -> Result<bookclerk_plugin_sdk::v2::JobOutcome> {
        self.call(|reply| Work::StreamCopy {
            job_id: job_id.into(),
            spec: StreamCopySpec {
                from: from.into(),
                to: to.into(),
            },
            reply,
        })
        .await
    }
}

impl Drop for V2PluginSession {
    fn drop(&mut self) {
        let _ = self.tx.send(Work::Shutdown);
    }
}

/// Maps ABI errors onto host [`PluginError`].
fn map_abi(err: bookclerk_plugin_sdk::PluginError) -> PluginError {
    PluginError::from_abi(Some(err.code.as_str()), err.message)
}

fn vat_thread(
    spawned: crate::spawn_stdio::SpawnedStdio,
    mut rx: mpsc::UnboundedReceiver<Work>,
    ready: oneshot::Sender<Result<PluginDescribe>>,
) {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(err) => {
            let _ = ready.send(Err(PluginError::message(format!("v2 runtime: {err}"))));
            return;
        }
    };
    rt.block_on(async move {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async move {
                let (client, rpc) =
                    connect_plugin(spawned.stdout, spawned.stdin, MAX_STREAM_WINDOW_BYTES);
                tokio::task::spawn_local(rpc);
                match client.describe().await {
                    Ok(desc) => {
                        let _ = ready.send(Ok(desc));
                    }
                    Err(err) => {
                        let _ = ready.send(Err(map_abi(err)));
                        return;
                    }
                }
                let mut dest: Option<bookclerk_plugin_sdk::v2::DestinationClient> = None;
                while let Some(work) = rx.recv().await {
                    match work {
                        Work::Shutdown => break,
                        Work::Describe { reply } => {
                            let _ = reply.send(client.describe().await.map_err(map_abi));
                        }
                        Work::EnsureDest { ctx, reply } => {
                            let out = client.destination(ctx).await.map_err(map_abi);
                            match out {
                                Ok(d) => {
                                    dest = Some(d);
                                    let _ = reply.send(Ok(()));
                                }
                                Err(err) => {
                                    let _ = reply.send(Err(err));
                                }
                            }
                        }
                        Work::Head { key, reply } => {
                            let out = match dest.as_ref() {
                                Some(d) => d.head(&key).await.map_err(map_abi),
                                None => Err(PluginError::message("v2 destination not created")),
                            };
                            let _ = reply.send(out);
                        }
                        Work::List { options, reply } => {
                            let out = match dest.as_ref() {
                                Some(d) => d.list(options).await.map_err(map_abi),
                                None => Err(PluginError::message("v2 destination not created")),
                            };
                            let _ = reply.send(out);
                        }
                        Work::GetStream { key, range, reply } => {
                            let out = match dest.as_ref() {
                                Some(d) => d.get(&key, range).await.map_err(map_abi),
                                None => Err(PluginError::message("v2 destination not created")),
                            };
                            let _ = reply.send(out);
                        }
                        Work::PutStream {
                            key,
                            body,
                            options,
                            reply,
                        } => {
                            let out = match dest.as_ref() {
                                Some(d) => d.put(&key, body, options).await.map_err(map_abi),
                                None => Err(PluginError::message("v2 destination not created")),
                            };
                            let _ = reply.send(out);
                        }
                        Work::Copy { from, to, reply } => {
                            let out = match dest.as_ref() {
                                Some(d) => d
                                    .copy(&from, &to)
                                    .await
                                    .map(|r| r.bytes_copied)
                                    .map_err(map_abi),
                                None => Err(PluginError::message("v2 destination not created")),
                            };
                            let _ = reply.send(out);
                        }
                        Work::Delete { key, reply } => {
                            let out = match dest.as_ref() {
                                Some(d) => d.delete(&key).await.map_err(map_abi),
                                None => Err(PluginError::message("v2 destination not created")),
                            };
                            let _ = reply.send(out);
                        }
                        Work::StreamCopy {
                            job_id,
                            spec,
                            reply,
                        } => {
                            let out = run_stream_copy(&client, dest.as_ref(), job_id, spec).await;
                            let _ = reply.send(out);
                        }
                    }
                }
                drop(spawned.child);
            })
            .await;
    });
}

async fn run_stream_copy(
    client: &PluginClient,
    dest: Option<&bookclerk_plugin_sdk::v2::DestinationClient>,
    job_id: String,
    spec: StreamCopySpec,
) -> Result<bookclerk_plugin_sdk::v2::JobOutcome> {
    let Some(dest) = dest else {
        return Err(PluginError::message("v2 destination not created"));
    };
    let handler = client
        .worker(WorkerContext {
            job_id,
            plugin_data_dir: String::new(),
            json: String::new(),
        })
        .await
        .map_err(map_abi)?;
    let event = JobEvent {
        event_type: "stream_copy".into(),
        json: serde_json::to_string(&spec).map_err(|err| PluginError::message(err.to_string()))?,
    };
    let input: Arc<dyn Source> = Arc::new(DestAsSource { dest: dest.clone() });
    let output: Arc<dyn Destination> = Arc::new(dest.clone());
    let progress: Arc<dyn bookclerk_plugin_sdk::v2::ProgressSink> = Arc::new(NoopProgress);
    client
        .handle_job(handler, event, input, output, progress)
        .await
        .map_err(map_abi)
}

struct DestAsSource {
    dest: bookclerk_plugin_sdk::v2::DestinationClient,
}

#[async_trait(?Send)]
impl Source for DestAsSource {
    async fn open(
        &self,
        key: &str,
    ) -> std::result::Result<ReadResult, bookclerk_plugin_sdk::PluginError> {
        Destination::get(&self.dest, key, None).await
    }
}

struct NoopProgress;

#[async_trait(?Send)]
impl bookclerk_plugin_sdk::v2::ProgressSink for NoopProgress {
    async fn report(
        &self,
        _percent: f32,
        _message: &str,
    ) -> std::result::Result<(), bookclerk_plugin_sdk::PluginError> {
        Ok(())
    }
}

/// [`StorageBackend`] over a v2 destination capability (streams, fail-closed scalars).
#[derive(Clone)]
pub struct V2Storage {
    /// Vat session.
    session: Arc<V2PluginSession>,
}

impl V2Storage {
    /// Wraps a connected session after [`V2PluginSession::ensure_destination`].
    #[must_use]
    pub fn new(session: Arc<V2PluginSession>) -> Self {
        Self { session }
    }

    fn map_err(err: PluginError) -> StorageError {
        let msg = err.to_string();
        if msg.contains("not_found") || msg.contains("not found") {
            StorageError::NotFound(msg)
        } else if msg.contains("payload_too_large") || msg.contains("payload too large") {
            StorageError::PayloadTooLarge(msg)
        } else {
            StorageError::Other(anyhow!(msg))
        }
    }
}

#[async_trait]
impl StorageBackend for V2Storage {
    fn name(&self) -> &'static str {
        "plugin-v2"
    }

    fn clone_box(&self) -> Box<dyn StorageBackend> {
        Box::new(self.clone())
    }

    async fn put(&self, key: &str, data: Bytes, meta: ObjectMeta) -> bookclerk_storage::Result<()> {
        if data.len() > MAX_SCALAR_BYTES as usize {
            return Err(StorageError::PayloadTooLarge(format!(
                "v2 scalar put of {} bytes exceeds {MAX_SCALAR_BYTES} (use put_stream)",
                data.len()
            )));
        }
        self.put_stream(key, Box::pin(std::io::Cursor::new(data)), meta)
            .await
            .map(|_| ())
    }

    async fn put_file(
        &self,
        key: &str,
        path: &std::path::Path,
        meta: ObjectMeta,
    ) -> bookclerk_storage::Result<()> {
        let file = tokio::fs::File::open(path).await?;
        self.put_stream(key, Box::pin(file), meta).await.map(|_| ())
    }

    async fn get(&self, key: &str) -> bookclerk_storage::Result<Bytes> {
        let probe = self.probe(key).await?;
        if probe.size > u64::from(MAX_SCALAR_BYTES) {
            return Err(StorageError::PayloadTooLarge(format!(
                "v2 scalar get of {} bytes exceeds {MAX_SCALAR_BYTES} (use get_stream)",
                probe.size
            )));
        }
        let (_probe, mut body) = self.get_stream(key, None).await?;
        let mut buf = Vec::new();
        body.read_to_end(&mut buf).await?;
        Ok(Bytes::from(buf))
    }

    async fn exists(&self, key: &str) -> bookclerk_storage::Result<bool> {
        Ok(self.head(key).await?.is_some())
    }

    async fn list(&self, prefix: &str) -> bookclerk_storage::Result<Vec<ObjectInfo>> {
        let mut out = Vec::new();
        let mut cursor = None;
        loop {
            let page = self.list_page(prefix, cursor.as_deref(), 0).await?;
            out.extend(page.objects);
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        Ok(out)
    }

    async fn probe(&self, key: &str) -> bookclerk_storage::Result<ObjectProbe> {
        match self.head(key).await? {
            Some(probe) => Ok(probe),
            None => Err(StorageError::NotFound(key.into())),
        }
    }

    async fn copy(&self, from: &str, to: &str) -> bookclerk_storage::Result<()> {
        self.session
            .call(|reply| Work::Copy {
                from: from.into(),
                to: to.into(),
                reply,
            })
            .await
            .map(|_| ())
            .map_err(Self::map_err)
    }

    async fn delete(&self, key: &str) -> bookclerk_storage::Result<()> {
        self.session
            .call(|reply| Work::Delete {
                key: key.into(),
                reply,
            })
            .await
            .map_err(Self::map_err)
    }

    async fn list_page(
        &self,
        prefix: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> bookclerk_storage::Result<ListPage> {
        let page = self
            .session
            .call(|reply| Work::List {
                options: ListOptions {
                    prefix: prefix.into(),
                    cursor: cursor.map(str::to_string),
                    limit,
                },
                reply,
            })
            .await
            .map_err(Self::map_err)?;
        Ok(ListPage {
            objects: page
                .objects
                .into_iter()
                .map(|o| ObjectInfo {
                    key: o.key,
                    size: o.size,
                })
                .collect(),
            next_cursor: page.next_cursor,
        })
    }

    async fn get_stream(
        &self,
        key: &str,
        range: Option<ByteRange>,
    ) -> bookclerk_storage::Result<(ObjectProbe, Pin<Box<dyn AsyncRead + Send>>)> {
        let abi_range = range.map(|r| AbiByteRange {
            offset: r.offset,
            length: r.length,
        });
        let read = self
            .session
            .call(|reply| Work::GetStream {
                key: key.into(),
                range: abi_range,
                reply,
            })
            .await
            .map_err(Self::map_err)?;
        Ok((meta_to_probe(read.meta), read.body))
    }

    async fn put_stream(
        &self,
        key: &str,
        body: Pin<Box<dyn AsyncRead + Send>>,
        meta: ObjectMeta,
    ) -> bookclerk_storage::Result<PutStreamResult> {
        let put = self
            .session
            .call(|reply| Work::PutStream {
                key: key.into(),
                body,
                options: WriteOptions {
                    content_type: meta.content_type,
                    content_length: meta.content_length,
                    sha256: None,
                },
                reply,
            })
            .await
            .map_err(Self::map_err)?;
        Ok(PutStreamResult {
            bytes_written: put.bytes_written,
            etag: put.etag,
        })
    }

    async fn head(&self, key: &str) -> bookclerk_storage::Result<Option<ObjectProbe>> {
        let meta = self
            .session
            .call(|reply| Work::Head {
                key: key.into(),
                reply,
            })
            .await
            .map_err(Self::map_err)?;
        Ok(meta.map(meta_to_probe))
    }
}

fn meta_to_probe(meta: ObjectMetadata) -> ObjectProbe {
    ObjectProbe {
        key: meta.key.clone(),
        size: meta.size,
        content_type: meta.content_type.clone(),
        meta: ObjectMeta {
            content_type: meta.content_type,
            content_length: Some(meta.size),
            ..Default::default()
        },
    }
}
