//! JobHandler helpers: stream-copy vertical slice (no media in scalars).

use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;

use super::roles::{Cancellation, Destination, JobHandler, JobHandlerContext, Source};
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

/// Stable destination commit token for one stream-copy invocation.
#[must_use]
pub fn stream_copy_commit_token(invocation: &JobInvocation) -> String {
    let mut out = String::from("sc-");
    for ch in invocation
        .idempotency_key
        .chars()
        .chain(std::iter::once('-'))
        .chain(invocation.invocation_id.chars())
    {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.len() > 128 {
        out.truncate(128);
    }
    out
}

/// Copies `from` → `to` through granted source/destination streams.
///
/// Bytes are staged under `commit_token` then committed after a fence check.
/// Cancellation or put/commit failure aborts the stage.
///
/// # Errors
///
/// Returns a plugin error when open, put, commit, or progress reporting fails.
pub async fn stream_copy_keys(
    input: &dyn Source,
    output: &dyn Destination,
    spec: &StreamCopySpec,
    invocation: &JobInvocation,
    progress: &dyn super::roles::ProgressSink,
    cancel: &dyn Cancellation,
) -> Result<JobOutcome> {
    let token = stream_copy_commit_token(invocation);
    if cancel.poll().await.unwrap_or(false) {
        return Ok(JobOutcome::Cancelled {
            message: "cancelled before stream_copy".into(),
        });
    }
    progress.report(0.0, "opening source").await?;
    let read = input.open(&spec.from).await?;
    let mut options = WriteOptions {
        content_type: read.meta.content_type.clone(),
        content_length: Some(read.meta.size),
        sha256: read.meta.sha256.clone(),
        commit_token: Some(token.clone()),
        stage_only: true,
    };
    if options.content_length == Some(0) {
        options.content_length = None;
    }
    progress.report(10.0, "staging").await?;
    let put = match output.put(&spec.to, read.body, options).await {
        Ok(put) => put,
        Err(err) => {
            let _ = output.abort_stage(&spec.to, &token).await;
            return Err(err);
        }
    };
    if cancel.poll().await.unwrap_or(false) {
        let _ = output.abort_stage(&spec.to, &token).await;
        return Ok(JobOutcome::Cancelled {
            message: "cancelled after stage".into(),
        });
    }
    if let Err(err) = progress.report(90.0, "committing").await {
        let _ = output.abort_stage(&spec.to, &token).await;
        return Err(err);
    }
    if let Err(err) = output.commit(&spec.to, &token).await {
        let _ = output.abort_stage(&spec.to, &token).await;
        return Err(err);
    }
    progress.report(100.0, "done").await?;
    Ok(JobOutcome::Completed {
        message: format!("copied {} -> {}", spec.from, spec.to),
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
            &spec,
            &invocation,
            context.progress.as_ref(),
            context.cancel.as_ref(),
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

#[cfg(test)]
#[allow(clippy::missing_panics_doc)]
mod tests {
    use super::*;
    use crate::v2::types::JobInvocation;

    #[test]
    fn commit_token_is_path_safe_identifier() {
        let invocation = JobInvocation::stream_copy("job/a:b", "{}");
        let token = stream_copy_commit_token(&invocation);
        assert!(token.starts_with("sc-"));
        assert!(!token.contains('/'));
        assert!(!token.contains('\\'));
        assert!(!token.contains(".."));
    }
}
