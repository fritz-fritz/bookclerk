//! Plugin-style `[sources.*]` and `[integrations.*]` configuration.
//!
//! Layout mirrors a small plugin registry for content sources and optional
//! third-party integrations. **Diagnostics are not an integration** — they
//! stay under top-level `[diagnostics]`.
//!
//! ```toml
//! [sources.audible]
//! enabled = true
//! quality = "high"        # Audible-native: high | normal
//!
//! [sources.graphicaudio]
//! enabled = true
//! access = "web"          # source-specific knob
//! quality = "hi"          # GraphicAudio-native: hi | lo
//!
//! # [integrations.audiobookshelf]
//! # enabled = true
//!
//! [diagnostics]
//! share_reports = false
//! ```
//!
//! Each source owns its quality enum (when it has one). Sources without a
//! quality knob omit the field. CLI/daemon registries only register sources
//! with `enabled = true`.

use serde::{Deserialize, Serialize};

use crate::pipeline_opts::{GraphicAudioAccess, IngestQuality};
use crate::settings::AudioQuality;

fn default_true() -> bool {
    true
}

/// Per-content-source plugins under `[sources]`.
///
/// Each source is independently enableable. Source-specific knobs (including
/// native quality enums) live on that source's table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct SourcesConfig {
    pub audible: AudibleSourceConfig,
    pub libro: SourcePluginConfig,
    pub chirp: SourcePluginConfig,
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

    /// Resolved Audible license quality from `[sources.audible]`.
    #[must_use]
    pub fn audible_quality(&self) -> AudioQuality {
        self.audible.effective_quality()
    }

    /// Prefer GraphicAudio Hi encode from `[sources.graphicaudio]`.
    #[must_use]
    pub fn graphicaudio_prefers_hi(&self) -> bool {
        self.graphicaudio.effective_quality().prefers_hi()
    }

    /// Deprecated bridge: map plugin quality into the legacy shared ingest enum.
    #[must_use]
    pub fn ingest_override(&self, source: &str) -> Option<IngestQuality> {
        match source.trim().to_ascii_lowercase().as_str() {
            "audible" => Some(match self.audible.effective_quality() {
                AudioQuality::High => IngestQuality::High,
                AudioQuality::Normal => IngestQuality::Normal,
            }),
            "graphicaudio" | "graphic_audio" | "ga" => {
                Some(match self.graphicaudio.effective_quality() {
                    GraphicAudioQuality::Hi => IngestQuality::High,
                    GraphicAudioQuality::Lo => IngestQuality::Low,
                })
            }
            // Libro / Chirp have no quality knob.
            "libro" | "libro.fm" | "librofm" | "chirp" => None,
            _ => None,
        }
    }
}

/// Common knobs shared by content sources that have no source-specific fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SourcePluginConfig {
    /// When false, the source is not registered in CLI/daemon registries.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for SourcePluginConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Audible plugin (`[sources.audible]`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AudibleSourceConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Audible-native license quality (`high` | `normal`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<AudioQuality>,
    /// Deprecated alias for [`Self::quality`] (`highest`/`high`/`normal`/`low`).
    /// Still accepted so older TOML keeps working; prefer `quality`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ingest: Option<IngestQuality>,
}

impl Default for AudibleSourceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            quality: None,
            ingest: None,
        }
    }
}

impl AudibleSourceConfig {
    /// Prefer `quality`; fall back to deprecated `ingest`; else High.
    #[must_use]
    pub fn effective_quality(&self) -> AudioQuality {
        self.quality
            .or_else(|| self.ingest.map(IngestQuality::as_audible))
            .unwrap_or(AudioQuality::High)
    }
}

/// GraphicAudio Hi/Lo encode preference (`[sources.graphicaudio] quality`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum GraphicAudioQuality {
    /// Higher bitrate / Hi URL when available.
    #[default]
    Hi,
    /// Lower bitrate / Lo URL.
    Lo,
}

impl GraphicAudioQuality {
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "hi" | "high" | "highest" => Some(Self::Hi),
            "lo" | "low" | "lowest" => Some(Self::Lo),
            _ => None,
        }
    }

    #[must_use]
    pub fn prefers_hi(self) -> bool {
        matches!(self, Self::Hi)
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
    /// GraphicAudio-native quality (`hi` | `lo`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<GraphicAudioQuality>,
    /// Deprecated alias for [`Self::quality`]. Prefer `quality`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ingest: Option<IngestQuality>,
}

impl Default for GraphicAudioSourceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            access: GraphicAudioAccess::Web,
            quality: None,
            ingest: None,
        }
    }
}

impl GraphicAudioSourceConfig {
    #[must_use]
    pub fn effective_quality(&self) -> GraphicAudioQuality {
        self.quality
            .or_else(|| {
                self.ingest.map(|q| {
                    if q.prefers_graphicaudio_hi() {
                        GraphicAudioQuality::Hi
                    } else {
                        GraphicAudioQuality::Lo
                    }
                })
            })
            .unwrap_or(GraphicAudioQuality::Hi)
    }
}

/// Optional third-party integrations under `[integrations]`.
///
/// Holds connect-portal settings and known adapters (Audiobookshelf, …).
/// Crash / error-burst reporting is **not** an integration — use top-level
/// [`crate::DiagnosticsConfig`] / `[diagnostics]`.
///
/// Unknown keys are rejected so a mistaken `[integrations.diagnostics]` fails
/// loudly instead of being ignored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct IntegrationsConfig {
    /// Base path for the connect portal (reverse-proxy friendly).
    pub portal_base_path: String,
    /// Claim ticket lifetime in hours.
    pub claim_ticket_ttl_hours: u64,
    /// Public origin used when logging/printing ticket URLs (optional).
    pub public_origin: Option<String>,
    /// Portal session lifetime in hours after redeem or credential login.
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
    /// ABS base URL (scheme + host, no trailing slash).
    pub base_url: String,
    /// Admin/service API key or user token (prefer `LIBATION_ABS_API_KEY`).
    pub api_key: Option<String>,
    /// Library id for scan-on-liberate.
    pub library_id: Option<String>,
    /// Poll ABS users and mint claim tickets for new ones.
    pub watch_users: bool,
    /// Trigger `POST /api/libraries/{id}/scan` after liberate.
    pub notify_scan_on_liberate: bool,
    /// Allow portal “Sign in with Audiobookshelf” (`POST /login`).
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
