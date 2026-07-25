//! Liberated-audio output packaging and destination plugins.
//!
//! ```toml
//! [output]
//! format = "enriched_m4b"
//! naming_profile = "audiobookshelf"
//!
//! [output.local]
//! enabled = true
//! root = "/data/Audiobooks"
//!
//! [output.s3]
//! enabled = false
//! bucket = "my-library"
//! region = "us-east-1"
//! ```
//!
//! Each destination plugin has its own `enabled` flag; multiple may be on at
//! once (liberated files are written to every enabled destination).
//! Credentials for S3 stay env-only (`AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY`).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{ConfigError, Result};
use crate::extras::{FileTimestampMode, LameConfig, PathSanitizationMode, ReplacementRule};
use crate::naming_profile::{NamingProfile, ResolvedNamingTemplates};
use crate::path_limits::DEFAULT_MAX_FILENAME_LENGTH;
use crate::pipeline_opts::{ChapterJsonMode, OutputFormat};

fn default_true() -> bool {
    true
}

/// Liberate packaging + destination backends (`[output]`).
///
/// Store-specific ingest knobs live under [`crate::SourcesConfig`] / `[sources.*]`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct OutputConfig {
    /// Post-liberate packaging format (default: enriched M4B).
    pub format: OutputFormat,
    /// Prefer Widevine/CENC (also enables Adrm→Widevine fallback; auto-provisions L3 CDM).
    pub widevine: bool,
    /// Prefer xHE-AAC on the Widevine path when offered.
    pub xhe_aac: bool,
    /// Optional local Widevine `.wvd` path (absolute or relative to `LIBATION_FILES_DIR`).
    pub widevine_cdm: Option<PathBuf>,
    /// Remote L3 CDM provider. `None` uses classic Libation AudibleCdm; empty/`off` disables.
    pub widevine_cdm_provider: Option<String>,
    /// Named path-template preset (`audiobookshelf` default, or `classic`).
    pub naming_profile: NamingProfile,
    pub folder_template: Option<String>,
    pub file_template: Option<String>,
    /// Save cover JPEG alongside audio.
    pub download_cover: bool,
    /// Download companion PDF when available.
    pub download_pdf: bool,
    /// Write a `.cue` sidecar from API chapters.
    pub create_cue: bool,
    /// Embed tags, cover, and chapters natively.
    pub fixup_metadata: bool,
    /// Chapter JSON sidecars: `off` | `flat` | `tree` | `both`.
    pub chapter_json: ChapterJsonMode,
    /// Deprecated: use [`Self::chapter_json`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_chapter_json: Option<bool>,
    /// Persist raw catalog API JSON (`metadata.json`).
    pub save_metadata_json: bool,
    /// Cover image size for download/embed (`500`, `1215`, or `native`).
    pub cover_size: String,
    /// Preferred Audible chapter API layout when fetching (`tree` or `flat`).
    pub chapter_layout: String,
    /// Re-liberate when media already exists at the destination.
    pub overwrite_existing: bool,
    /// Scratch directory for in-progress work; relative to files_dir.
    pub in_progress: Option<PathBuf>,
    /// Action when a title fails to liberate.
    pub bad_book_action: BadBookAction,
    /// Max MP3 part size in MiB when format is `split_mp3_by_size`.
    pub split_mp3_max_mb: u32,
    pub chapter_file_template: Option<String>,
    pub chapter_title_template: Option<String>,
    pub minimum_file_duration_minutes: u32,
    pub combine_nested_chapter_titles: bool,
    pub merge_opening_and_end_credits: bool,
    pub strip_unabridged: bool,
    pub strip_audible_brand_audio: bool,
    pub download_clips_bookmarks: bool,
    /// Keep encrypted download in storage (`RetainAaxFile`).
    pub retain_aax_file: bool,
    /// Fetch speed cap in KB/s (`0` = unlimited).
    pub download_speed_limit_kbps: u32,
    pub lame: LameConfig,
    pub max_sample_rate: Option<u32>,
    pub creation_time: FileTimestampMode,
    pub last_write_time: FileTimestampMode,
    pub path_sanitization: PathSanitizationMode,
    pub replacement_characters: Vec<ReplacementRule>,
    /// Max length per path segment. Default 255; `0` disables truncation.
    pub max_filename_length: u32,
    /// Local filesystem destination plugin (`[output.local]`).
    pub local: OutputLocalConfig,
    /// S3 / MinIO destination plugin (`[output.s3]`).
    pub s3: OutputS3Config,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            format: OutputFormat::default(),
            widevine: false,
            xhe_aac: false,
            widevine_cdm: None,
            widevine_cdm_provider: None,
            naming_profile: NamingProfile::default(),
            folder_template: None,
            file_template: None,
            download_cover: true,
            download_pdf: true,
            create_cue: false,
            fixup_metadata: true,
            chapter_json: ChapterJsonMode::Off,
            save_chapter_json: None,
            save_metadata_json: false,
            cover_size: String::from("500"),
            chapter_layout: String::from("tree"),
            overwrite_existing: false,
            in_progress: None,
            bad_book_action: BadBookAction::Ask,
            split_mp3_max_mb: 200,
            chapter_file_template: None,
            chapter_title_template: None,
            minimum_file_duration_minutes: 0,
            combine_nested_chapter_titles: false,
            merge_opening_and_end_credits: false,
            strip_unabridged: false,
            strip_audible_brand_audio: false,
            download_clips_bookmarks: false,
            retain_aax_file: false,
            download_speed_limit_kbps: 0,
            lame: LameConfig::default(),
            max_sample_rate: None,
            creation_time: FileTimestampMode::Now,
            last_write_time: FileTimestampMode::Now,
            path_sanitization: PathSanitizationMode::Auto,
            replacement_characters: Vec::new(),
            max_filename_length: DEFAULT_MAX_FILENAME_LENGTH as u32,
            local: OutputLocalConfig::default(),
            s3: OutputS3Config::default(),
        }
    }
}

impl OutputConfig {
    /// Resolved packaging format.
    #[must_use]
    pub fn effective_format(&self) -> OutputFormat {
        self.format
    }

    /// Resolved chapter JSON sidecar mode (default off).
    #[must_use]
    pub fn effective_chapter_json(&self) -> ChapterJsonMode {
        if let Some(true) = self.save_chapter_json {
            if self.chapter_json == ChapterJsonMode::Off {
                return match self.chapter_layout.to_ascii_lowercase().as_str() {
                    "flat" => ChapterJsonMode::Flat,
                    "both" => ChapterJsonMode::Both,
                    _ => ChapterJsonMode::Tree,
                };
            }
        }
        if let Some(false) = self.save_chapter_json {
            if self.chapter_json == ChapterJsonMode::Off {
                return ChapterJsonMode::Off;
            }
        }
        self.chapter_json
    }

    /// Resolve folder / file / chapter-file templates.
    #[must_use]
    pub fn resolve_naming_templates(&self) -> ResolvedNamingTemplates {
        ResolvedNamingTemplates::resolve(
            self.naming_profile,
            self.folder_template.as_deref(),
            self.file_template.as_deref(),
            self.chapter_file_template.as_deref(),
        )
    }

    /// Enabled destination plugins (may be more than one).
    #[must_use]
    pub fn enabled_backends(&self) -> Vec<OutputBackendKind> {
        let mut out = Vec::new();
        if self.local.enabled {
            out.push(OutputBackendKind::Local);
        }
        if self.s3.enabled {
            out.push(OutputBackendKind::S3);
        }
        out
    }

    /// True when at least one destination plugin is enabled.
    #[must_use]
    pub fn has_enabled_destination(&self) -> bool {
        !self.enabled_backends().is_empty()
    }

    /// Prefix used for path-length budgeting when generating storage keys.
    ///
    /// When S3 is among the enabled destinations, its prefix is used (stricter
    /// full-key budget). Otherwise the local prefix is used.
    #[must_use]
    pub fn path_limit_prefix(&self) -> String {
        if self.s3.enabled {
            normalize_storage_prefix(self.s3.prefix.trim())
        } else {
            normalize_storage_prefix(self.local.prefix.trim())
        }
    }

    /// Whether any enabled destination is S3 (for path sanitization / key budget).
    #[must_use]
    pub fn is_s3(&self) -> bool {
        self.s3.enabled
    }

    /// Human-readable list of enabled destination ids (`local`, `s3`, …).
    #[must_use]
    pub fn enabled_backend_names(&self) -> Vec<&'static str> {
        self.enabled_backends()
            .into_iter()
            .map(|kind| match kind {
                OutputBackendKind::Local => "local",
                OutputBackendKind::S3 => "s3",
            })
            .collect()
    }

    /// Validate destination plugin configuration.
    pub fn validate_destinations(&self) -> Result<()> {
        if !self.has_enabled_destination() {
            return Err(ConfigError::Invalid(
                "enable at least one of [output.local] or [output.s3]".into(),
            ));
        }
        if self.s3.enabled && self.s3.bucket.trim().is_empty() {
            return Err(ConfigError::Invalid(
                "output.s3.enabled=true requires output.s3.bucket".into(),
            ));
        }
        Ok(())
    }
}

/// How to handle liberate failures (`BadBook` setting).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum BadBookAction {
    #[default]
    Ask,
    Abort,
    Retry,
    Ignore,
}

/// Which output destination plugin is active.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum OutputBackendKind {
    #[default]
    Local,
    S3,
}

/// Local filesystem destination (`[output.local]`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct OutputLocalConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Root directory for liberated audiobooks.
    pub root: PathBuf,
    /// Optional key prefix under [`Self::root`] (trailing slash optional).
    pub prefix: String,
}

impl Default for OutputLocalConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            root: PathBuf::from("Audiobooks"),
            prefix: String::new(),
        }
    }
}

/// S3 / MinIO destination (`[output.s3]`).
///
/// Credentials remain env-only (`AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct OutputS3Config {
    pub enabled: bool,
    pub bucket: String,
    pub prefix: String,
    pub region: String,
    /// Optional custom endpoint (MinIO, LocalStack, etc.).
    pub endpoint: Option<String>,
    /// Force path-style addressing (typical for MinIO).
    pub force_path_style: bool,
}

impl Default for OutputS3Config {
    fn default() -> Self {
        Self {
            enabled: false,
            bucket: String::new(),
            prefix: String::from("library/"),
            region: String::from("us-east-1"),
            endpoint: None,
            force_path_style: false,
        }
    }
}

/// Normalize a storage key prefix: empty stays empty; otherwise ensure a trailing `/`.
#[must_use]
pub fn normalize_storage_prefix(prefix: &str) -> String {
    let trimmed = prefix.trim().trim_start_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.ends_with('/') {
        trimmed.to_string()
    } else {
        format!("{trimmed}/")
    }
}
