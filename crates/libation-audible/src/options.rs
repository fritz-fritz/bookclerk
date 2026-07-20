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
