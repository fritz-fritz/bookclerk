//! Download / license options mapped from Libation config.

use libation_config::{AudioQuality, DownloadConfig, DownloadFormat};
use serde::{Deserialize, Serialize};

/// Options forwarded to audible-rs download / license calls.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DownloadOptions {
    pub quality: AudioQuality,
    pub format: DownloadFormat,
    pub widevine: bool,
    pub xhe_aac: bool,
}

impl From<&DownloadConfig> for DownloadOptions {
    fn from(cfg: &DownloadConfig) -> Self {
        if cfg.widevine {
            tracing::warn!(
                "download.widevine requested but Widevine/CENC liberate is not implemented"
            );
        }
        if cfg.xhe_aac {
            tracing::warn!("download.xhe_aac requested but codec preference is not implemented");
        }
        if matches!(cfg.format, DownloadFormat::Mp3) {
            tracing::warn!(
                "download.format=mp3 requested but re-encode is not implemented; storing Adrm output as-is"
            );
        }
        Self {
            quality: cfg.quality,
            format: cfg.format,
            widevine: cfg.widevine,
            xhe_aac: cfg.xhe_aac,
        }
    }
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self::from(&DownloadConfig::default())
    }
}
