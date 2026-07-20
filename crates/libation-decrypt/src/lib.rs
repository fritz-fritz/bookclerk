//! Decrypt pipeline. v1 shells out to `aaxclean-cli`; v2 will be pure Rust.

mod error;

pub use error::{DecryptError, Result};

use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::process::Command;

/// Input for a decrypt job.
#[derive(Debug, Clone)]
pub struct DecryptRequest {
    /// Encrypted AAX/AAXC file.
    pub input: PathBuf,
    /// Destination m4b/m4a path.
    pub output: PathBuf,
    /// Activation bytes (AAX) when required.
    pub activation_bytes: Option<String>,
    /// Path to `aaxclean-cli` binary (default: look up on `PATH`).
    pub aaxclean_bin: Option<PathBuf>,
}

/// Outcome of a successful decrypt.
#[derive(Debug, Clone)]
pub struct DecryptOutcome {
    pub output: PathBuf,
}

/// Decrypt using `aaxclean-cli` (v1 strategy).
pub async fn decrypt_with_aaxclean(req: DecryptRequest) -> Result<DecryptOutcome> {
    if !req.input.exists() {
        return Err(DecryptError::InputMissing(req.input));
    }
    if let Some(parent) = req.output.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let bin = req
        .aaxclean_bin
        .unwrap_or_else(|| PathBuf::from("aaxclean-cli"));

    let mut cmd = Command::new(&bin);
    cmd.arg(&req.input).arg(&req.output);
    if let Some(bytes) = &req.activation_bytes {
        cmd.arg("--activation-bytes").arg(bytes);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    tracing::info!(
        input = %req.input.display(),
        output = %req.output.display(),
        bin = %bin.display(),
        "running aaxclean-cli"
    );

    let output = cmd.output().await.map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            DecryptError::AaxcleanNotFound(bin.clone())
        } else {
            DecryptError::Io(err)
        }
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(DecryptError::AaxcleanFailed {
            status: output.status.code(),
            stderr: stderr.trim().to_string(),
        });
    }

    if !req.output.exists() {
        return Err(DecryptError::OutputMissing(req.output.clone()));
    }

    Ok(DecryptOutcome { output: req.output })
}

/// True when `aaxclean-cli` appears to be available.
pub async fn aaxclean_available(bin: Option<&Path>) -> bool {
    let bin = bin.unwrap_or_else(|| Path::new("aaxclean-cli"));
    Command::new(bin)
        .arg("--help")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}
