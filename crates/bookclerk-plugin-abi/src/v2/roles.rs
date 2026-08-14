//! Author-facing async traits for ABI v2 role classes.

use std::pin::Pin;

use tokio::io::AsyncRead;

use super::types::{
    CopyResult, DestinationContext, JobEvent, JobOutcome, ListOptions, ListPage, ObjectMetadata,
    PluginDescribe, PutResult, SourceContext, WorkerContext, WriteOptions,
};
use crate::Result;

/// Inclusive byte range for a streamed read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    /// Starting offset.
    pub offset: u64,
    /// Number of bytes; `None` means to end of object.
    pub length: Option<u64>,
}

/// Streamed read result. `body` ownership is transferred to the caller.
pub struct ReadResult {
    /// Object metadata (size, type, checksums).
    pub meta: ObjectMetadata,
    /// Byte stream; drop/cancel aborts the read.
    pub body: Pin<Box<dyn AsyncRead + Send>>,
}

/// Destination capability (storage).
///
/// Cap'n Proto stubs are `!Send`; call these traits from a `LocalSet`.
#[async_trait::async_trait(?Send)]
pub trait Destination {
    /// Metadata without a body; `Ok(None)` when the key is missing.
    async fn head(&self, key: &str) -> Result<Option<ObjectMetadata>>;

    /// One page of keys under `options.prefix`.
    async fn list(&self, options: ListOptions) -> Result<ListPage>;

    /// Streamed read. The body is a transferred stream, not a scalar.
    async fn get(&self, key: &str, range: Option<ByteRange>) -> Result<ReadResult>;

    /// Streamed write. `body` ownership is transferred to the destination.
    async fn put(
        &self,
        key: &str,
        body: Pin<Box<dyn AsyncRead + Send>>,
        options: WriteOptions,
    ) -> Result<PutResult>;

    /// Server-side copy when the backend supports it.
    async fn copy(&self, from: &str, to: &str) -> Result<CopyResult>;

    /// Delete a key (no-op if missing).
    async fn delete(&self, key: &str) -> Result<()>;
}

/// Source capability that can open a named object as a stream.
#[async_trait::async_trait(?Send)]
pub trait Source {
    /// Opens `key` for streamed reading.
    async fn open(&self, key: &str) -> Result<ReadResult>;
}

/// Progress reports for a job invocation (never carries media).
#[async_trait::async_trait(?Send)]
pub trait ProgressSink {
    /// Reports `percent` in `0..=100` and an operator-facing `message`.
    async fn report(&self, percent: f32, message: &str) -> Result<()>;
}

/// Granted stubs for one [`JobHandler::handle`] invocation.
pub struct JobHandlerContext {
    /// Input source capability.
    pub input: Box<dyn Source>,
    /// Output destination capability.
    pub output: Box<dyn Destination>,
    /// Progress sink (durable job row).
    pub progress: Box<dyn ProgressSink>,
}

/// Plugin worker that handles one durable job invocation.
#[async_trait::async_trait(?Send)]
pub trait JobHandler {
    /// Runs `event` using granted capabilities until completion or cancel.
    async fn handle(&self, event: JobEvent, context: JobHandlerContext) -> Result<JobOutcome>;
}

/// Root `BookclerkPlugin` capability (`describe` / role factories / shutdown).
#[async_trait::async_trait(?Send)]
pub trait PluginRoot: 'static {
    /// Advertises identity, features, and scalar limits.
    async fn describe(&self) -> Result<PluginDescribe>;

    /// Returns a destination capability for this invocation.
    async fn destination(&self, context: DestinationContext) -> Result<Box<dyn Destination>>;

    /// Returns a source capability for this invocation.
    async fn source(&self, context: SourceContext) -> Result<Box<dyn Source>>;

    /// Returns a job handler for this invocation.
    async fn worker(&self, context: WorkerContext) -> Result<Box<dyn JobHandler>>;

    /// Releases guest resources.
    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}
