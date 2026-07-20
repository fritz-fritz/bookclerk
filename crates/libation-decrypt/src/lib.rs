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
    /// Adrm aaxc content key (hex) — preferred modern path.
    pub audible_key: Option<String>,
    /// Adrm aaxc IV (hex).
    pub audible_iv: Option<String>,
    /// Legacy AAX activation bytes (unsupported by aaxclean-cli key/iv path).
    pub activation_bytes: Option<String>,
    /// Path to `aaxclean-cli` binary (default: `AUDIBLE_AAXCLEAN_CLI` or `PATH`).
    pub aaxclean_bin: Option<PathBuf>,
}

/// Outcome of a successful decrypt.
#[derive(Debug, Clone)]
pub struct DecryptOutcome {
    pub output: PathBuf,
}

/// Build argv for aaxclean-cli (without the binary). Exposed for tests.
pub fn aaxclean_args(req: &DecryptRequest) -> Result<Vec<String>> {
    match (&req.audible_key, &req.audible_iv) {
        (Some(key), Some(iv)) => Ok(vec![
            "-f".into(),
            req.input.display().to_string(),
            "--audible_key".into(),
            key.clone(),
            "--audible_iv".into(),
            iv.clone(),
            "--moov_faststart".into(),
            "-o".into(),
            req.output.display().to_string(),
        ]),
        _ if req.activation_bytes.is_some() => Err(DecryptError::UnsupportedActivationBytes),
        _ => Err(DecryptError::MissingCredentials),
    }
}

fn resolve_aaxclean_bin(explicit: Option<&Path>) -> PathBuf {
    if let Some(path) = explicit {
        return path.to_path_buf();
    }
    if let Ok(path) = std::env::var("AUDIBLE_AAXCLEAN_CLI") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    PathBuf::from("aaxclean-cli")
}

/// Decrypt using `aaxclean-cli` (v1 strategy).
pub async fn decrypt_with_aaxclean(req: DecryptRequest) -> Result<DecryptOutcome> {
    if !req.input.exists() {
        return Err(DecryptError::InputMissing(req.input));
    }
    if let Some(parent) = req.output.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let args = aaxclean_args(&req)?;
    let bin = resolve_aaxclean_bin(req.aaxclean_bin.as_deref());

    let mut cmd = Command::new(&bin);
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Never log key/iv — only paths and binary name.
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
        let _ = tokio::fs::remove_file(&req.output).await;
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
    let bin = resolve_aaxclean_bin(bin);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aaxclean_args_use_key_iv_shape() {
        let req = DecryptRequest {
            input: PathBuf::from("/cache/book.aaxc"),
            output: PathBuf::from("/cache/book.m4b"),
            audible_key: Some("aabb".into()),
            audible_iv: Some("ccdd".into()),
            activation_bytes: None,
            aaxclean_bin: None,
        };
        let args = aaxclean_args(&req).unwrap();
        assert_eq!(
            args,
            vec![
                "-f",
                "/cache/book.aaxc",
                "--audible_key",
                "aabb",
                "--audible_iv",
                "ccdd",
                "--moov_faststart",
                "-o",
                "/cache/book.m4b",
            ]
        );
    }

    #[test]
    fn activation_bytes_alone_rejected() {
        let req = DecryptRequest {
            input: PathBuf::from("in.aax"),
            output: PathBuf::from("out.m4b"),
            audible_key: None,
            audible_iv: None,
            activation_bytes: Some("deadbeef".into()),
            aaxclean_bin: None,
        };
        assert!(matches!(
            aaxclean_args(&req),
            Err(DecryptError::UnsupportedActivationBytes)
        ));
    }
}
