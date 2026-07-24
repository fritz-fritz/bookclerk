//! Plugin-style `[sources.*]` and `[integrations.*]` configuration.
//!
//! Each content source owns a typed TOML table under `[sources.<id>]` with
//! that store’s knobs (bitrate / container / access as applicable). Integrations
//! live under `[integrations.*]`. **Diagnostics are not an integration.**
//!
//! ```toml
//! [sources.audible]
//! enabled = true
//! bitrate = "high"            # high | normal
//!
//! [sources.graphicaudio]
//! enabled = true
//! access = "web"              # web | zip | device
//! bitrate = "hi"              # hi | lo (device)
//! container = "auto"          # auto | m4b | mp3 | flac (zip)
//!
//! [sources.libro]
//! enabled = true
//! container = "m4b"           # m4b | zip
//!
//! [sources.chirp]
//! enabled = true
//! ```

use serde::{Deserialize, Serialize};

use crate::pipeline_opts::GraphicAudioAccess;
use crate::settings::AudioQuality;

fn default_true() -> bool {
    true
}

/// Per-content-source plugins under `[sources]`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct SourcesConfig {
    pub audible: AudibleSourceConfig,
    pub libro: LibroSourceConfig,
    pub chirp: ChirpSourceConfig,
    pub graphicaudio: GraphicAudioSourceConfig,
}

impl SourcesConfig {
    /// Whether a content source id should be registered / scanned.
    #[must_use]
    pub fn is_enabled(&self, source: &str) -> bool {
        match source.trim().to_ascii_lowercase().as_str() {
            "audible" => self.audible.enabled,
            "libro" | "libro.fm" | "librofm" => self.libro.enabled,
            "chirp" => self.chirp.enabled,
            "graphicaudio" | "graphic_audio" | "ga" => self.graphicaudio.enabled,
            _ => true,
        }
    }
}

/// Audible plugin (`[sources.audible]`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AudibleSourceConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// License bitrate tier requested from Audible (`high` | `normal`).
    #[serde(default)]
    pub bitrate: AudioQuality,
}

impl Default for AudibleSourceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            bitrate: AudioQuality::High,
        }
    }
}

/// Libro.fm plugin (`[sources.libro]`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LibroSourceConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Preferred download container (`m4b` | `zip`).
    #[serde(default)]
    pub container: LibroContainer,
}

impl Default for LibroSourceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            container: LibroContainer::M4b,
        }
    }
}

/// Preferred Libro.fm download packaging.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum LibroContainer {
    /// Single M4B when the store offers it (default).
    #[default]
    M4b,
    /// Multi-part ZIP of MP3s.
    Zip,
}

impl LibroContainer {
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "m4b" | "audiobook" => Some(Self::M4b),
            "zip" | "mp3" | "parts" => Some(Self::Zip),
            _ => None,
        }
    }
}

/// Chirp plugin (`[sources.chirp]`) — enable flag only today.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ChirpSourceConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for ChirpSourceConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// GraphicAudio device encode preference (`[sources.graphicaudio] bitrate`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum GraphicAudioBitrate {
    /// Higher bitrate / Hi URL when available.
    #[default]
    Hi,
    /// Lower bitrate / Lo URL.
    Lo,
}

impl GraphicAudioBitrate {
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "hi" | "high" => Some(Self::Hi),
            "lo" | "low" => Some(Self::Lo),
            _ => None,
        }
    }

    #[must_use]
    pub fn prefers_hi(self) -> bool {
        matches!(self, Self::Hi)
    }
}

/// Preferred GraphicAudio ZIP SKU when [`GraphicAudioAccess::Zip`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum GraphicAudioContainer {
    /// Prefer M4B, then MP3, then FLAC (default).
    #[default]
    Auto,
    M4b,
    Mp3,
    Flac,
}

impl GraphicAudioContainer {
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "auto" | "any" => Some(Self::Auto),
            "m4b" => Some(Self::M4b),
            "mp3" => Some(Self::Mp3),
            "flac" => Some(Self::Flac),
            _ => None,
        }
    }

    /// Rank used when sorting Magento downloadable options (lower = better).
    #[must_use]
    pub fn format_rank(self, option_label: &str) -> u8 {
        let label = option_label.to_ascii_lowercase();
        match self {
            Self::Auto => {
                if label.contains("m4b") {
                    0
                } else if label.contains("mp3") {
                    1
                } else if label.contains("flac") {
                    2
                } else {
                    3
                }
            }
            Self::M4b => {
                if label.contains("m4b") {
                    0
                } else {
                    10
                }
            }
            Self::Mp3 => {
                if label.contains("mp3") {
                    0
                } else {
                    10
                }
            }
            Self::Flac => {
                if label.contains("flac") {
                    0
                } else {
                    10
                }
            }
        }
    }
}

/// GraphicAudio plugin (`[sources.graphicaudio]`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct GraphicAudioSourceConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Fetch path: `web` (default) | `zip` | `device`.
    #[serde(default)]
    pub access: GraphicAudioAccess,
    /// Device encode bitrate (`hi` | `lo`); ignored for web/zip.
    #[serde(default)]
    pub bitrate: GraphicAudioBitrate,
    /// ZIP SKU container preference when [`Self::access`] is `zip`.
    #[serde(default)]
    pub container: GraphicAudioContainer,
}

impl Default for GraphicAudioSourceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            access: GraphicAudioAccess::Web,
            bitrate: GraphicAudioBitrate::Hi,
            container: GraphicAudioContainer::Auto,
        }
    }
}

/// Optional third-party integrations under `[integrations]`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct IntegrationsConfig {
    pub portal_base_path: String,
    pub claim_ticket_ttl_hours: u64,
    pub public_origin: Option<String>,
    pub portal_session_ttl_hours: u64,
    pub audiobookshelf: AudiobookshelfConfig,
}

impl Default for IntegrationsConfig {
    fn default() -> Self {
        Self {
            portal_base_path: "/connect".into(),
            claim_ticket_ttl_hours: 72,
            public_origin: None,
            portal_session_ttl_hours: 12,
            audiobookshelf: AudiobookshelfConfig::default(),
        }
    }
}

/// Audiobookshelf integration settings (`[integrations.audiobookshelf]`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AudiobookshelfConfig {
    pub enabled: bool,
    pub base_url: String,
    pub api_key: Option<String>,
    pub library_id: Option<String>,
    pub watch_users: bool,
    pub notify_scan_on_liberate: bool,
    pub allow_credential_login: bool,
}

impl Default for AudiobookshelfConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: String::new(),
            api_key: None,
            library_id: None,
            watch_users: true,
            notify_scan_on_liberate: true,
            allow_credential_login: true,
        }
    }
}
