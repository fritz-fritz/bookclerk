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

/// Retry-stable destination commit token for one stream-copy step.
///
/// The token is derived from the durable idempotency key, the step id (or
/// `stream_copy`), and the destination object key. It does **not** include
/// `invocation_id` / attempt / generation: destination publish is at-least-once
/// (the library lease and object visibility are not one atomic commit), so a
/// reclaimed worker must restage and commit the same token. Local/S3 `commit`
/// treats an already-published dest with a missing stage as success.
#[must_use]
pub fn stream_copy_commit_token(invocation: &JobInvocation, dest_key: &str) -> String {
    use sha2::{Digest, Sha256};
    let step = invocation.step_id.as_deref().unwrap_or("stream_copy");
    let mut hasher = Sha256::new();
    hasher.update(invocation.idempotency_key.as_bytes());
    hasher.update([0u8]);
    hasher.update(step.as_bytes());
    hasher.update([0u8]);
    hasher.update(dest_key.as_bytes());
    let digest = hasher.finalize();
    format!("sc-{}", hex::encode(&digest[..16]))
}

/// Copies `from` → `to` through granted source/destination streams.
///
/// Bytes are staged under a retry-stable `commit_token` then committed after a
/// best-effort fence check. Publication is at-least-once and idempotent on that
/// token. Cancellation or put/commit failure aborts the stage.
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
    let token = stream_copy_commit_token(invocation, &spec.to);
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
        let token = stream_copy_commit_token(&invocation, "out/book.m4b");
        assert!(token.starts_with("sc-"));
        assert!(!token.contains('/'));
        assert!(!token.contains('\\'));
        assert!(!token.contains(".."));
    }

    #[test]
    fn commit_token_is_stable_across_attempts_and_generations() {
        let first = JobInvocation::stream_copy_from_lease(
            crate::v2::types::JobInvocationLease {
                job_id: "job-1".into(),
                attempt: 1,
                generation: 1,
                dedup_key: "dedup-stable".into(),
                deadline_unix_ms: 1,
                checkpoint: None,
                invocation_sequence: 1,
            },
            "{}",
        );
        let second = JobInvocation::stream_copy_from_lease(
            crate::v2::types::JobInvocationLease {
                job_id: "job-1".into(),
                attempt: 3,
                generation: 9,
                dedup_key: "dedup-stable".into(),
                deadline_unix_ms: 2,
                checkpoint: None,
                invocation_sequence: 7,
            },
            "{}",
        );
        assert_ne!(first.invocation_id, second.invocation_id);
        assert_eq!(
            stream_copy_commit_token(&first, "library/title.m4b"),
            stream_copy_commit_token(&second, "library/title.m4b")
        );
        assert_ne!(
            stream_copy_commit_token(&first, "library/title.m4b"),
            stream_copy_commit_token(&first, "library/other.m4b")
        );
    }
}
