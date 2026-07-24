//! Brand colors + remote favicon URLs for the connect portal.
//!
//! Icons are loaded via `<img src>` from each store/integration’s public
//! favicon (or Google’s favicon mirror when the site only ships fingerprinted
//! asset paths). Nothing is vendored into this repository.

use libation_source::SourceKind;
use serde::Serialize;

/// Visual identity for a store or integration in the portal UI.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Brand {
    pub id: &'static str,
    pub name: &'static str,
    /// Button background.
    pub bg: &'static str,
    /// Button foreground / text.
    pub fg: &'static str,
    /// Optional accent (border / focus).
    pub accent: &'static str,
    /// Remote favicon / app-icon URL (not stored in-repo).
    pub icon_url: &'static str,
}

impl Brand {
    /// Favicon URL for `<img src>`.
    #[must_use]
    pub fn logo_href(&self) -> &'static str {
        self.icon_url
    }
}

/// Brand for a registered content source.
#[must_use]
pub fn source_brand(kind: SourceKind) -> Brand {
    match kind {
        SourceKind::Audible => Brand {
            id: "audible",
            name: "Audible",
            bg: "#F8991D",
            fg: "#111111",
            accent: "#D97706",
            // Audible’s published Google/SEO favicon (CDN).
            icon_url: "https://www.google.com/s2/favicons?domain=audible.com&sz=128",
        },
        SourceKind::LibroFm => Brand {
            id: "libro",
            name: "Libro.fm",
            bg: "#1F4E3D",
            fg: "#F4F1EA",
            accent: "#2F6B53",
            // Site icons are content-hashed; mirror keeps a stable href.
            icon_url: "https://www.google.com/s2/favicons?domain=libro.fm&sz=128",
        },
        SourceKind::GraphicAudio => Brand {
            id: "graphicaudio",
            name: "GraphicAudio",
            bg: "#141414",
            fg: "#F5F5F5",
            accent: "#C41E3A",
            icon_url: "https://www.google.com/s2/favicons?domain=graphicaudio.net&sz=128",
        },
        SourceKind::Chirp => Brand {
            id: "chirp",
            name: "Chirp",
            bg: "#0F766E",
            fg: "#ECFEFF",
            accent: "#14B8A6",
            icon_url: "https://www.google.com/s2/favicons?domain=chirpbooks.com&sz=128",
        },
    }
}

/// Brand for an integration provider id, if known.
#[must_use]
pub fn integration_brand(id: &str) -> Option<Brand> {
    match id.trim().to_ascii_lowercase().as_str() {
        "audiobookshelf" | "abs" => Some(Brand {
            id: "audiobookshelf",
            name: "Audiobookshelf",
            bg: "#1E293B",
            fg: "#F8FAFC",
            accent: "#59BC89",
            icon_url: "https://www.google.com/s2/favicons?domain=audiobookshelf.org&sz=128",
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_source_has_https_icon() {
        for kind in [
            SourceKind::Audible,
            SourceKind::LibroFm,
            SourceKind::GraphicAudio,
            SourceKind::Chirp,
        ] {
            let b = source_brand(kind);
            assert!(
                b.icon_url.starts_with("https://"),
                "{} icon must be https",
                b.id
            );
            assert_eq!(b.logo_href(), b.icon_url);
        }
    }

    #[test]
    fn abs_brand_aliases() {
        let b = integration_brand("abs").expect("abs alias");
        assert_eq!(b.id, "audiobookshelf");
        assert!(b.icon_url.starts_with("https://"));
    }
}
