//! Portal / UI brand metadata owned by content-source plugins.

use serde::Serialize;

/// Visual identity for a content source in the connect portal.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct SourceBrand {
    /// Identifier.
    pub id: &'static str,
    /// Name.
    pub name: &'static str,
    /// Bg.
    pub bg: &'static str,
    /// Fg.
    pub fg: &'static str,
    /// Accent.
    pub accent: &'static str,
    /// Remote favicon / app-icon URL (not stored in-repo).
    pub icon_url: &'static str,
}

impl SourceBrand {
    /// Logo href.
    #[must_use]
    pub fn logo_href(&self) -> &'static str {
        self.icon_url
    }
}
