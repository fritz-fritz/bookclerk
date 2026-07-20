//! Download / license options mapped from Libation config.

use std::path::PathBuf;

use libation_config::{AudioQuality, DownloadConfig, DownloadFormat};
use serde::{Deserialize, Serialize};

/// Options forwarded to audible-rs download / license calls.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DownloadOptions {
    pub quality: AudioQuality,
    pub format: DownloadFormat,
    /// Prefer / force Widevine/CENC when true; also used as Adrm→Widevine fallback with CDM.
    pub widevine: bool,
    /// Prefer xHE-AAC on the Widevine path when offered.
    pub xhe_aac: bool,
    /// Optional path to a `.wvd` CDM (relative to files_dir or absolute).
    pub widevine_cdm: Option<PathBuf>,
    /// Classic Libation-style folder template (e.g. `<author>/<title>`).
    pub folder_template: Option<String>,
    /// Classic Libation-style file template without extension (e.g. `<asin>`).
    pub file_template: Option<String>,
    pub download_cover: bool,
    pub download_pdf: bool,
    pub create_cue: bool,
    pub fixup_metadata: bool,
    pub save_chapter_json: bool,
    pub cover_size: String,
    pub chapter_layout: String,
    pub overwrite_existing: bool,
}

impl From<&DownloadConfig> for DownloadOptions {
    fn from(cfg: &DownloadConfig) -> Self {
        Self {
            quality: cfg.quality,
            format: cfg.format,
            widevine: cfg.widevine,
            xhe_aac: cfg.xhe_aac,
            widevine_cdm: cfg.widevine_cdm.clone(),
            folder_template: cfg.folder_template.clone(),
            file_template: cfg.file_template.clone(),
            download_cover: cfg.download_cover,
            download_pdf: cfg.download_pdf,
            create_cue: cfg.create_cue,
            fixup_metadata: cfg.fixup_metadata,
            save_chapter_json: cfg.save_chapter_json,
            cover_size: cfg.cover_size.clone(),
            chapter_layout: cfg.chapter_layout.clone(),
            overwrite_existing: cfg.overwrite_existing,
        }
    }
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self::from(&DownloadConfig::default())
    }
}
