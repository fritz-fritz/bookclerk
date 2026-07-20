//! Rate-limited HTTP download helper.

use std::path::Path;
use std::time::{Duration, Instant};

use audible_rs::api::client::Client;
use audible_rs::downloader::{self, DownloadOutcome};
use tokio::io::AsyncWriteExt;

use crate::error::{AudibleError, Result};

/// Download a URL to `dest`, optionally throttling to `limit_kbps` (0 = unlimited).
#[allow(clippy::too_many_arguments)]
pub async fn download_to_file_limited(
    client: &Client,
    url: &str,
    dest: &Path,
    expected_size: Option<u64>,
    force: bool,
    limit_kbps: u32,
    expected_content_type: &[&str],
    ext_overrides: &[(&str, &str)],
    version_tag: Option<&str>,
) -> Result<(DownloadOutcome, std::path::PathBuf)> {
    if limit_kbps == 0 {
        return downloader::download_to_file(
            client,
            url,
            dest,
            expected_size,
            force,
            None,
            expected_content_type,
            ext_overrides,
            version_tag,
        )
        .await
        .map_err(|e| AudibleError::Download(e.to_string()));
    }

    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    if dest.exists() && !force {
        return Ok((DownloadOutcome::AlreadyComplete, dest.to_path_buf()));
    }

    let request = client.authed_get(url).await?;
    let response = request
        .send()
        .await
        .map_err(|e| AudibleError::Download(e.to_string()))?;
    let response = response
        .error_for_status()
        .map_err(|e| AudibleError::Download(e.to_string()))?;
    let mut file = tokio::fs::File::create(dest).await?;
    let limit_bps = u64::from(limit_kbps) * 1024;
    let mut downloaded = 0u64;
    let mut window_start = Instant::now();
    let mut stream = response.bytes_stream();
    use futures::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| AudibleError::Download(e.to_string()))?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        let elapsed = window_start.elapsed();
        let expected = Duration::from_secs_f64(downloaded as f64 / limit_bps as f64);
        if expected > elapsed {
            tokio::time::sleep(expected - elapsed).await;
        }
        if downloaded >= limit_bps {
            window_start = Instant::now();
            downloaded = 0;
        }
    }
    file.flush().await?;
    Ok((DownloadOutcome::Downloaded, dest.to_path_buf()))
}
