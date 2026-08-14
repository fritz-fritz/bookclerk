//! Global CLI output format helpers.

use clap::ValueEnum;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
/// Private `OutputFormat` enum used by this crate's implementation.
pub enum OutputFormat {
    #[default]
    /// `Text` variant of the enclosing enum.
    Text,
    /// `Json` variant of the enclosing enum.
    Json,
}

impl OutputFormat {
    #[must_use]
    /// Returns whether `json` holds for this value.
    pub fn is_json(self) -> bool {
        matches!(self, Self::Json)
    }
}

/// Print a serializable value as JSON, or run `text` for human output.
pub fn emit<T: Serialize>(
    format: OutputFormat,
    value: &T,
    text: impl FnOnce(),
) -> anyhow::Result<()> {
    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(value)?);
        }
        OutputFormat::Text => text(),
    }
    Ok(())
}
