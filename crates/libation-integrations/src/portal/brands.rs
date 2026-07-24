//! Brand colors + original mark SVGs for the connect portal.
//!
//! Marks are Libation-drawn identifiers (not scraped official logo files). Buttons
//! pair each mark with the store/integration name for recognition.

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
}

impl Brand {
    #[must_use]
    pub fn logo_href(&self, portal_base: &str) -> String {
        let base = portal_base.trim_end_matches('/');
        format!("{base}/assets/brands/{}.svg", self.id)
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
        },
        SourceKind::LibroFm => Brand {
            id: "libro",
            name: "Libro.fm",
            bg: "#1F4E3D",
            fg: "#F4F1EA",
            accent: "#2F6B53",
        },
        SourceKind::GraphicAudio => Brand {
            id: "graphicaudio",
            name: "GraphicAudio",
            bg: "#141414",
            fg: "#F5F5F5",
            accent: "#C41E3A",
        },
        SourceKind::Chirp => Brand {
            id: "chirp",
            name: "Chirp",
            bg: "#0F766E",
            fg: "#ECFEFF",
            accent: "#14B8A6",
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
        }),
        _ => None,
    }
}

/// SVG document for a brand id (mark only; wordmark is button text).
#[must_use]
pub fn brand_svg(id: &str) -> Option<&'static str> {
    match id.trim().to_ascii_lowercase().as_str() {
        "audible" => Some(AUDIBLE_SVG),
        "libro" => Some(LIBRO_SVG),
        "graphicaudio" => Some(GRAPHICAUDIO_SVG),
        "chirp" => Some(CHIRP_SVG),
        "audiobookshelf" | "abs" => Some(AUDIOBOOKSHELF_SVG),
        _ => None,
    }
}

const AUDIBLE_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" role="img" aria-label="Audible">
  <rect width="64" height="64" rx="14" fill="#F8991D"/>
  <path fill="#111" d="M18 40c6-10 14-16 22-18 1.2-.3 2.2.8 1.7 1.9-3.2 6.6-5.6 12.4-6.7 16.6-.3 1.1-1.6 1.5-2.5.8C28 38 22.8 39 18.8 41.2c-1 .6-2.2-.3-1.8-1.2z"/>
  <path fill="none" stroke="#111" stroke-width="3" stroke-linecap="round" d="M34 22c6 2 11 7 14 14M38 18c8 3 14 10 18 19"/>
</svg>"##;

const LIBRO_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" role="img" aria-label="Libro.fm">
  <rect width="64" height="64" rx="14" fill="#1F4E3D"/>
  <path fill="#F4F1EA" d="M16 14h22c6 0 10 4 10 10v26c0-5-4-8-9-8H16V14z"/>
  <path fill="#A7D7C5" d="M16 42h23c5 0 9 3 9 8H16V42z"/>
  <path fill="#2F6B53" d="M16 14v36" stroke="#A7D7C5" stroke-width="3"/>
</svg>"##;

const GRAPHICAUDIO_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" role="img" aria-label="GraphicAudio">
  <rect width="64" height="64" rx="14" fill="#141414"/>
  <path fill="#C41E3A" d="M14 18h36v6H30v22h-8V24H14V18z"/>
  <path fill="#F5F5F5" d="M34 28c6 0 12 4 12 11s-6 11-12 11-12-4-12-11 6-11 12-11zm0 6c-3.2 0-5.5 2.3-5.5 5s2.3 5 5.5 5 5.5-2.3 5.5-5-2.3-5-5.5-5z"/>
</svg>"##;

const CHIRP_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" role="img" aria-label="Chirp">
  <rect width="64" height="64" rx="14" fill="#0F766E"/>
  <path fill="#ECFEFF" d="M14 36c8-2 14-8 17-16 1.6 7 6 13 13 17-8 1-14 5-18 12-2-5-6-9-12-13z"/>
  <circle cx="44" cy="22" r="4" fill="#5EEAD4"/>
</svg>"##;

const AUDIOBOOKSHELF_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" role="img" aria-label="Audiobookshelf">
  <rect width="64" height="64" rx="14" fill="#1E293B"/>
  <rect x="12" y="14" width="8" height="36" rx="2" fill="#59BC89"/>
  <rect x="24" y="18" width="8" height="32" rx="2" fill="#86EFAC"/>
  <rect x="36" y="12" width="8" height="38" rx="2" fill="#59BC89"/>
  <rect x="48" y="20" width="6" height="30" rx="2" fill="#34D399"/>
  <path fill="#94A3B8" d="M10 50h44v4H10z"/>
</svg>"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_source_has_svg() {
        for kind in [
            SourceKind::Audible,
            SourceKind::LibroFm,
            SourceKind::GraphicAudio,
            SourceKind::Chirp,
        ] {
            let b = source_brand(kind);
            assert!(brand_svg(b.id).is_some(), "missing svg for {}", b.id);
        }
    }

    #[test]
    fn abs_brand_aliases() {
        assert_eq!(
            integration_brand("abs").map(|b| b.id),
            Some("audiobookshelf")
        );
        assert!(brand_svg("audiobookshelf").is_some());
    }
}
