//! Plugin kind labels shared by discovery and the wire protocol.

use serde::{Deserialize, Serialize};

/// Which Libation surface a plugin implements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginKind {
    Source,
    Integration,
    Output,
}

impl PluginKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Integration => "integration",
            Self::Output => "output",
        }
    }
}
