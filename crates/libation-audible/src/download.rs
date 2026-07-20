//! License request + encrypted download via audible-rs.

use std::path::{Path, PathBuf};

use audible_rs::api::client::Client;
use audible_rs::downloader::{self, Quality};
use audible_rs::models::content::DownloadLicense;
use libation_config::AudioQuality;

use crate::accounts::resolve_auth_file_async;
use crate::auth::load_authenticator;
use crate::error::{AudibleError, Result};

/// Authenticated Audible client bound to one account.
pub struct AccountClient {
    pub client: Client,
    pub account_id: String,
    pub marketplace: String,
    pub auth_file: PathBuf,
}

/// Public summary of a license grant (no secrets).
#[derive(Debug, Clone)]
pub struct LicenseSummary {
    pub asin: String,
    pub status_code: String,
    pub drm_type: Option<String>,
    pub content_format: Option<String>,
    pub content_size: Option<u64>,
    pub granted: bool,
    pub denial_message: Option<String>,
    pub has_voucher: bool,
    pub offline_url_present: bool,
}

/// Encrypted (or plain Mpeg) audio downloaded to cache, plus decrypt material.
#[derive(Debug, Clone)]
pub struct EncryptedDownload {
    pub path: PathBuf,
    pub drm_type: Option<String>,
    pub content_format: Option<String>,
    pub content_size: Option<u64>,
    /// Adrm aaxc content key (hex); absent for plain Mpeg media.
    pub key: Option<String>,
    /// Adrm aaxc IV (hex).
    pub iv: Option<String>,
    /// True when the file still needs aaxclean/ffmpeg decrypt.
    pub needs_decrypt: bool,
}

/// Open an audible-rs [`Client`] for `account` (auth file stem, label, or customer id).
pub async fn open_account_client(files_dir: &Path, account: &str) -> Result<AccountClient> {
    let auth_file = resolve_auth_file_async(files_dir, account).await?;
    let auth = load_authenticator(&auth_file, None).await?;
    let marketplace = auth.locale().country_code.to_string();
    let account_id = auth
        .customer_id()
        .map(str::to_string)
        .unwrap_or_else(|| {
            auth_file
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(account)
                .to_string()
        });
    let client = Client::new(auth).map_err(AudibleError::from)?;
    Ok(AccountClient {
        client,
        account_id,
        marketplace,
        auth_file,
    })
}

/// Request an Adrm/Mpeg download license for `asin`.
pub async fn request_content_license(
    client: &Client,
    marketplace: &str,
    asin: &str,
    quality: AudioQuality,
) -> Result<DownloadLicense> {
    let q = match quality {
        AudioQuality::High => Quality::High,
        AudioQuality::Normal => Quality::Normal,
    };
    downloader::request_license(client, marketplace, asin, q)
        .await
        .map_err(AudibleError::from)
}

/// Summarize a license without exposing voucher / URL query secrets.
#[must_use]
pub fn summarize_license(license: &DownloadLicense) -> LicenseSummary {
    LicenseSummary {
        asin: license.asin.clone(),
        status_code: license.status_code.clone(),
        drm_type: license.drm_type.clone(),
        content_format: license.content_format.clone(),
        content_size: license.content_size,
        granted: license.is_granted(),
        denial_message: license.denial_message.clone(),
        has_voucher: license.has_voucher,
        offline_url_present: license.offline_url.is_some(),
    }
}

/// Download the licensed audio file to `dest` (parent dirs created).
///
/// For Adrm grants, also decrypts the voucher into `key`/`iv`. Mpeg grants
/// (plain mp3/m4a) set `needs_decrypt = false`.
pub async fn download_licensed_audio(
    client: &Client,
    license: &DownloadLicense,
    dest: &Path,
) -> Result<EncryptedDownload> {
    if !license.is_granted() {
        return Err(AudibleError::License(format!(
            "license denied for {}: {}",
            license.asin,
            license
                .denial_message
                .as_deref()
                .unwrap_or("no reason given")
        )));
    }

    let url = license
        .offline_url
        .as_deref()
        .ok_or_else(|| AudibleError::License(format!("{}: missing offline_url", license.asin)))?;

    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let (_outcome, path) = downloader::download_to_file(
        client,
        url,
        dest,
        license.content_size,
        false,
        None,
        &[
            "audio/aax",
            "audio/vnd.audible.aax",
            "audio/mpeg",
            "audio/mp3",
            "audio/mp4",
            "audio/x-m4a",
            "audio/audible",
        ],
        &[
            ("audio/mpeg", "mp3"),
            ("audio/mp3", "mp3"),
            ("audio/mp4", "m4a"),
            ("audio/x-m4a", "m4a"),
        ],
        license.version_tag().as_deref(),
    )
    .await
    .map_err(|err| AudibleError::Download(err.to_string()))?;

    let drm = license.drm_type.as_deref().unwrap_or("Adrm");
    let is_mpeg = drm.eq_ignore_ascii_case("Mpeg");
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let plain_media = matches!(ext.as_str(), "mp3" | "m4a");

    let (key, iv, needs_decrypt) = if is_mpeg || plain_media {
        (None, None, false)
    } else if license.has_voucher {
        let (dt, ds, cid) = (
            client.device_type(),
            client.device_serial(),
            client.customer_id(),
        );
        let (Some(dt), Some(ds), Some(cid)) = (dt, ds, cid) else {
            return Err(AudibleError::License(
                "auth file lacks device/customer data needed to decrypt the aaxc voucher".into(),
            ));
        };
        let voucher = license
            .decrypt_voucher(dt, ds, cid)
            .map_err(|err| AudibleError::License(format!("voucher decrypt failed: {err}")))?;
        (Some(voucher.key), Some(voucher.iv), true)
    } else {
        return Err(AudibleError::License(format!(
            "{}: Adrm grant has no voucher (cannot decrypt)",
            license.asin
        )));
    };

    Ok(EncryptedDownload {
        path,
        drm_type: license.drm_type.clone(),
        content_format: license.content_format.clone(),
        content_size: license.content_size,
        key,
        iv,
        needs_decrypt,
    })
}

/// Fetch license + download encrypted audio into `cache_dir` for liberate.
pub async fn fetch_and_download(
    files_dir: &Path,
    account: &str,
    asin: &str,
    quality: AudioQuality,
    cache_dir: &Path,
) -> Result<(AccountClient, EncryptedDownload, LicenseSummary)> {
    let account_client = open_account_client(files_dir, account).await?;
    let license = request_content_license(
        &account_client.client,
        &account_client.marketplace,
        asin,
        quality,
    )
    .await?;
    let summary = summarize_license(&license);
    if !summary.granted {
        return Err(AudibleError::License(format!(
            "license denied for {asin}: {}",
            summary
                .denial_message
                .as_deref()
                .unwrap_or("no reason given")
        )));
    }

    let format = license
        .content_format
        .as_deref()
        .filter(|f| !f.is_empty())
        .unwrap_or("audio");
    let dest = cache_dir.join(asin).join(format!("{asin}.{format}.aaxc"));
    let download = download_licensed_audio(&account_client.client, &license, &dest).await?;
    Ok((account_client, download, summary))
}

#[cfg(test)]
mod tests {
    use super::*;
    use audible_rs::models::content::DownloadLicense;

    #[test]
    fn summarize_granted_license() {
        let license = DownloadLicense::from_response(serde_json::json!({
            "content_license": {
                "status_code": "Granted",
                "asin": "B00TEST",
                "drm_type": "Adrm",
                "license_response": "VOUCHER",
                "content_metadata": {
                    "content_url": {"offline_url": "https://cds.example/x.aaxc?Policy=abc"},
                    "content_reference": {
                        "content_format": "AAX_44_64",
                        "content_size_in_bytes": 100u64
                    }
                }
            }
        }))
        .unwrap();
        let summary = summarize_license(&license);
        assert!(summary.granted);
        assert!(summary.has_voucher);
        assert!(summary.offline_url_present);
        assert_eq!(summary.drm_type.as_deref(), Some("Adrm"));
        assert_eq!(summary.content_format.as_deref(), Some("AAX_44_64"));
    }
}
