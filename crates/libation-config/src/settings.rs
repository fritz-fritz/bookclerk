//! Application settings loaded from TOML with environment overrides.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{ConfigError, Result};
use crate::paths::{resolve_config_path, resolve_files_dir, Paths};

/// Top-level Libation configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Config {
    /// Filesystem layout (resolved at load time; not always present in TOML).
    #[serde(skip)]
    pub paths: Option<Paths>,

    pub library: LibraryConfig,
    pub download: DownloadConfig,
    pub storage: StorageConfig,
    pub daemon: DaemonConfig,
}

/// Library / scan related settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LibraryConfig {
    /// Automatically liberate newly scanned titles (daemon).
    pub auto_liberate: bool,
    /// Scan interval in minutes for `libationd` (0 = disabled).
    pub scan_interval_minutes: u64,
}

impl Default for LibraryConfig {
    fn default() -> Self {
        Self {
            auto_liberate: false,
            scan_interval_minutes: 60,
        }
    }
}

/// Download / audio format preferences (Libation parity).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct DownloadConfig {
    pub quality: AudioQuality,
    pub format: DownloadFormat,
    /// Request Widevine L3 licenses when available.
    pub widevine: bool,
    /// Prefer xHE-AAC when offered.
    pub xhe_aac: bool,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            quality: AudioQuality::High,
            format: DownloadFormat::M4b,
            widevine: true,
            xhe_aac: false,
        }
    }
}

/// Audible download quality.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum AudioQuality {
    #[default]
    High,
    Normal,
}

/// Container / codec preference for liberated files.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum DownloadFormat {
    #[default]
    M4b,
    Mp3,
}

/// Pluggable storage backend configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct StorageConfig {
    pub backend: StorageBackendKind,
    pub local: StorageLocalConfig,
    pub s3: StorageS3Config,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            backend: StorageBackendKind::Local,
            local: StorageLocalConfig::default(),
            s3: StorageS3Config::default(),
        }
    }
}

/// Which storage implementation to use.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum StorageBackendKind {
    #[default]
    Local,
    S3,
}

/// Local filesystem storage root.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct StorageLocalConfig {
    /// Root directory for liberated audiobooks.
    pub root: PathBuf,
}

impl Default for StorageLocalConfig {
    fn default() -> Self {
        Self {
            root: PathBuf::from("Audiobooks"),
        }
    }
}

/// S3 / MinIO storage settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct StorageS3Config {
    pub bucket: String,
    pub prefix: String,
    pub region: String,
    /// Optional custom endpoint (MinIO, LocalStack, etc.).
    pub endpoint: Option<String>,
    /// Force path-style addressing (typical for MinIO).
    pub force_path_style: bool,
}

impl Default for StorageS3Config {
    fn default() -> Self {
        Self {
            bucket: String::new(),
            prefix: String::from("library/"),
            region: String::from("us-east-1"),
            endpoint: None,
            force_path_style: false,
        }
    }
}

/// Daemon / HTTP control plane settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct DaemonConfig {
    /// Bind address for the HTTP control plane.
    pub listen: String,
    /// Emit JSON logs when true.
    pub json_logs: bool,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            listen: String::from("127.0.0.1:8787"),
            json_logs: true,
        }
    }
}

impl Config {
    /// Load config: defaults ← optional TOML file ← environment overrides.
    pub fn load(cli_files_dir: Option<PathBuf>, config_path: Option<PathBuf>) -> Result<Self> {
        let files_dir = resolve_files_dir(cli_files_dir);
        let paths = Paths::from_files_dir(files_dir);
        let path = config_path.unwrap_or_else(|| resolve_config_path(&paths.files_dir));

        let mut cfg = if path.exists() {
            Self::from_toml_file(&path)?
        } else {
            tracing::debug!(?path, "config file not found; using defaults");
            Self::default()
        };

        cfg.apply_env_overrides();
        cfg.paths = Some(paths);
        cfg.validate()?;
        Ok(cfg)
    }

    /// Parse a TOML config file.
    pub fn from_toml_file(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_toml_str(&text, &path.display().to_string())
    }

    /// Parse TOML from a string.
    pub fn from_toml_str(text: &str, origin: &str) -> Result<Self> {
        toml::from_str(text).map_err(|source| ConfigError::Parse {
            path: origin.to_string(),
            source,
        })
    }

    /// Apply `LIBATION_*` environment overrides.
    pub fn apply_env_overrides(&mut self) {
        if let Ok(v) = std::env::var("LIBATION_STORAGE_BACKEND") {
            match v.to_ascii_lowercase().as_str() {
                "local" => self.storage.backend = StorageBackendKind::Local,
                "s3" => self.storage.backend = StorageBackendKind::S3,
                other => tracing::warn!(%other, "unknown LIBATION_STORAGE_BACKEND; ignoring"),
            }
        }
        if let Ok(v) = std::env::var("LIBATION_STORAGE_LOCAL_ROOT") {
            self.storage.local.root = PathBuf::from(v);
        }
        if let Ok(v) = std::env::var("LIBATION_S3_BUCKET") {
            self.storage.s3.bucket = v;
        }
        if let Ok(v) = std::env::var("LIBATION_S3_PREFIX") {
            self.storage.s3.prefix = v;
        }
        if let Ok(v) = std::env::var("LIBATION_S3_REGION") {
            self.storage.s3.region = v;
        }
        if let Ok(v) = std::env::var("LIBATION_S3_ENDPOINT") {
            self.storage.s3.endpoint = Some(v);
        }
        if let Ok(v) = std::env::var("LIBATION_S3_FORCE_PATH_STYLE") {
            self.storage.s3.force_path_style =
                parse_bool(&v).unwrap_or(self.storage.s3.force_path_style);
        }
        if let Ok(v) = std::env::var("LIBATION_DAEMON_LISTEN") {
            self.daemon.listen = v;
        }
        if let Ok(v) = std::env::var("LIBATION_DAEMON_JSON_LOGS") {
            self.daemon.json_logs = parse_bool(&v).unwrap_or(self.daemon.json_logs);
        }
        if let Ok(v) = std::env::var("LIBATION_AUTO_LIBERATE") {
            self.library.auto_liberate = parse_bool(&v).unwrap_or(self.library.auto_liberate);
        }
        if let Ok(v) = std::env::var("LIBATION_SCAN_INTERVAL_MINUTES") {
            if let Ok(n) = v.parse() {
                self.library.scan_interval_minutes = n;
            }
        }
        if let Ok(v) = std::env::var("LIBATION_WIDEVINE") {
            self.download.widevine = parse_bool(&v).unwrap_or(self.download.widevine);
        }
        if let Ok(v) = std::env::var("LIBATION_XHE_AAC") {
            self.download.xhe_aac = parse_bool(&v).unwrap_or(self.download.xhe_aac);
        }
    }

    /// Soft validation of cross-field constraints.
    pub fn validate(&self) -> Result<()> {
        if self.storage.backend == StorageBackendKind::S3 && self.storage.s3.bucket.is_empty() {
            return Err(ConfigError::Invalid(
                "storage.backend=s3 requires storage.s3.bucket".into(),
            ));
        }
        Ok(())
    }

    /// Resolved paths (panic only if `load` was not used — callers should use `load`).
    #[must_use]
    pub fn paths(&self) -> &Paths {
        self.paths
            .as_ref()
            .expect("Config.paths populated by Config::load")
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_example_toml() {
        let text = r#"
[library]
auto_liberate = true
scan_interval_minutes = 30

[download]
quality = "high"
format = "m4b"
widevine = true
xhe_aac = false

[storage]
backend = "local"

[storage.local]
root = "/data/audiobooks"

[storage.s3]
bucket = "my-audiobooks"
prefix = "library/"
region = "us-east-1"

[daemon]
listen = "0.0.0.0:8787"
json_logs = true
"#;
        let cfg = Config::from_toml_str(text, "test").unwrap();
        assert!(cfg.library.auto_liberate);
        assert_eq!(cfg.library.scan_interval_minutes, 30);
        assert_eq!(cfg.storage.backend, StorageBackendKind::Local);
        assert_eq!(cfg.storage.local.root, PathBuf::from("/data/audiobooks"));
        assert_eq!(cfg.daemon.listen, "0.0.0.0:8787");
    }

    #[test]
    fn s3_requires_bucket() {
        let mut cfg = Config::default();
        cfg.storage.backend = StorageBackendKind::S3;
        assert!(cfg.validate().is_err());
        cfg.storage.s3.bucket = "books".into();
        assert!(cfg.validate().is_ok());
    }
}
