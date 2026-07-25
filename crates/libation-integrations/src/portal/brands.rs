//! Source brand colors + remote favicon URLs for the connect portal.
//!
//! Integration brands are owned by each integration plugin (see
//! [`crate::abs::brand`]); look them up via [`integration_brand`] or
//! [`crate::Integration::portal_brand`].

use libation_source::SourceKind;

pub use crate::brand::Brand;

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

/// Brand for an integration provider id, delegated to plugin modules.
#[must_use]
pub fn integration_brand(id: &str) -> Option<Brand> {
    crate::abs::brand_for_id(id)
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
