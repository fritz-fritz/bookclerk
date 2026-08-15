//! JobHandler helpers: stream-copy vertical slice (no media in scalars).

use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;

use super::roles::{Destination, JobHandler, JobHandlerContext, Source};
use super::types::{JobInvocation, JobOutcome, WriteOptions};
use crate::{PluginError, Result};

/// JSON body for a `stream_copy` [`JobInvocation`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamCopySpec {
    /// Source object key.
    pub from: String,
    /// Destination object key.
    pub to: String,
}

/// Copies `from` → `to` through granted source/destination streams.
///
/// # Errors
///
/// Returns a plugin error when open, put, or progress reporting fails.
pub async fn stream_copy_keys(
    input: &dyn Source,
    output: &dyn Destination,
    from: &str,
    to: &str,
    progress: &dyn super::roles::ProgressSink,
) -> Result<JobOutcome> {
    progress.report(0.0, "opening source").await?;
    let read = input.open(from).await?;
    let mut options = WriteOptions {
        content_type: read.meta.content_type.clone(),
        content_length: Some(read.meta.size),
        sha256: read.meta.sha256.clone(),
    };
    if options.content_length == Some(0) {
        options.content_length = None;
    }
    progress.report(10.0, "copying").await?;
    let put = output.put(to, read.body, options).await?;
    progress.report(100.0, "done").await?;
    Ok(JobOutcome::Completed {
        message: format!("copied {from} -> {to}"),
        bytes_copied: put.bytes_written,
    })
}

/// Default [`JobHandler`] for the stream-copy vertical slice.
pub struct StreamCopyHandler;

#[async_trait::async_trait(?Send)]
impl JobHandler for StreamCopyHandler {
    async fn handle(
        &self,
        invocation: JobInvocation,
        context: JobHandlerContext,
    ) -> Result<JobOutcome> {
        if context.cancel.poll().await.unwrap_or(false) {
            return Ok(JobOutcome::Cancelled {
                message: "cancelled before stream_copy".into(),
            });
        }
        if invocation.command_type != "stream_copy" {
            return Err(PluginError::unsupported(format!(
                "unsupported job command `{}`",
                invocation.command_type
            )));
        }
        let spec: StreamCopySpec = serde_json::from_str(&invocation.payload_json)
            .map_err(|err| PluginError::invalid_params(format!("stream_copy spec: {err}")))?;
        stream_copy_keys(
            context.input.as_ref(),
            context.output.as_ref(),
            &spec.from,
            &spec.to,
            context.progress.as_ref(),
        )
        .await
    }
}

/// Reads an entire transferred body into a vec (tests / small objects only).
///
/// # Errors
///
/// Returns an I/O error mapped as [`PluginError::internal`].
pub async fn read_all(
    mut body: std::pin::Pin<Box<dyn tokio::io::AsyncRead + Send>>,
) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    body.read_to_end(&mut buf)
        .await
        .map_err(|err| PluginError::internal(format!("read stream: {err}")))?;
    Ok(buf)
}
