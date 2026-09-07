//! Companion `migrations.toml` declared by `plugin.toml` `migration_plan`.
//!
//! The host proves these statements as BookclerkSQL. This crate only parses
//! TOML; it does not typecheck SQL.

use std::path::{Component, Path};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Parsed companion migration plan (`[[steps]]`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginMigrationPlanFile {
    /// Ordered frozen steps (versions strictly increasing, starting at 1).
    #[serde(default)]
    pub steps: Vec<PluginMigrationStepToml>,
}

/// One frozen plugin schema revision in the companion file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginMigrationStepToml {
    /// Monotonic schema revision (`>= 1`).
    pub version: i64,
    /// First package/semver that shipped this step.
    pub introduced_in: String,
    /// Ordered `up` ops (`{ schema = "..." }` or `{ data = "..." }`).
    pub up: Vec<PluginMigrationOpToml>,
    /// Optional reverse ops; omit when the step is irreversible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub down: Option<Vec<PluginMigrationOpToml>>,
}

/// One admitted BookclerkSQL statement in a plugin migration step.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum PluginMigrationOpToml {
    /// Schema mutation (`CREATE` / `DROP` / index).
    Schema {
        /// Canonical BookclerkSQL.
        schema: String,
    },
    /// Data backfill (`INSERT` / `UPDATE` / `DELETE`).
    Data {
        /// Canonical BookclerkSQL.
        data: String,
    },
}

impl PluginMigrationOpToml {
    /// Canonical SQL text.
    #[must_use]
    pub fn sql(&self) -> &str {
        match self {
            Self::Schema { schema } => schema,
            Self::Data { data } => data,
        }
    }

    /// True when this op is schema DDL.
    #[must_use]
    pub fn is_schema(&self) -> bool {
        matches!(self, Self::Schema { .. })
    }
}

impl PluginMigrationPlanFile {
    /// Parses a companion `migrations.toml` document.
    ///
    /// # Errors
    ///
    /// Returns when TOML is malformed or a step has no `up` ops / invalid version.
    pub fn parse(text: &str) -> Result<Self> {
        let file: Self = toml::from_str(text)?;
        file.validate()?;
        Ok(file)
    }

    /// Semantic checks that do not typecheck SQL.
    ///
    /// # Errors
    ///
    /// Returns when versions are not strictly increasing positive integers or
    /// an `up` list is empty.
    pub fn validate(&self) -> Result<()> {
        let mut prev = 0i64;
        for (index, step) in self.steps.iter().enumerate() {
            if step.version < 1 {
                return Err(Error::message(format!(
                    "migrations.toml step {index} version {} must be >= 1",
                    step.version
                )));
            }
            if step.version <= prev {
                return Err(Error::message(format!(
                    "migrations.toml versions must be strictly increasing (saw {} after {prev})",
                    step.version
                )));
            }
            if step.up.is_empty() {
                return Err(Error::message(format!(
                    "migrations.toml version {} has no up ops",
                    step.version
                )));
            }
            if step.introduced_in.trim().is_empty() {
                return Err(Error::message(format!(
                    "migrations.toml version {} introduced_in must not be empty",
                    step.version
                )));
            }
            prev = step.version;
        }
        Ok(())
    }
}

/// True when `raw` is a relative companion path with no `..` components.
///
/// # Errors
///
/// Returns when the path is empty, absolute, or contains `.` / `..` escapes.
pub fn validate_migration_plan_path(raw: &str) -> Result<()> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(Error::message(
            "plugin.toml: migration_plan must be a non-empty relative path",
        ));
    }
    if trimmed != raw {
        return Err(Error::message(
            "plugin.toml: migration_plan must not have leading or trailing whitespace",
        ));
    }
    let path = Path::new(raw);
    if path.is_absolute() {
        return Err(Error::message(format!(
            "plugin.toml: migration_plan `{raw}` must be relative to the plugin root"
        )));
    }
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir => {
                return Err(Error::message(format!(
                    "plugin.toml: migration_plan `{raw}` must not contain `.`"
                )));
            }
            Component::ParentDir => {
                return Err(Error::message(format!(
                    "plugin.toml: migration_plan `{raw}` must not contain `..`"
                )));
            }
            _ => {
                return Err(Error::message(format!(
                    "plugin.toml: migration_plan `{raw}` is not a portable relative path"
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::missing_panics_doc)]
mod tests {
    use super::*;

    #[test]
    fn parse_schema_and_data_ops() {
        let file = PluginMigrationPlanFile::parse(
            r#"
[[steps]]
version = 1
introduced_in = "0.1.0"
up = [
  { schema = "CREATE TABLE IF NOT EXISTS notes (id INTEGER PRIMARY KEY, body TEXT NOT NULL)" },
]
down = [
  { schema = "DROP TABLE IF EXISTS notes" },
]
"#,
        )
        .unwrap();
        assert_eq!(file.steps.len(), 1);
        assert!(file.steps[0].up[0].is_schema());
        assert!(file.steps[0].down.is_some());
    }

    #[test]
    fn empty_up_fails() {
        let err = PluginMigrationPlanFile::parse(
            r#"
[[steps]]
version = 1
introduced_in = "0.1.0"
up = []
"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("no up ops"), "{err}");
    }

    #[test]
    fn relative_path_rejects_parent() {
        validate_migration_plan_path("migrations.toml").unwrap();
        validate_migration_plan_path("plans/v1.toml").unwrap();
        assert!(validate_migration_plan_path("../x.toml").is_err());
        assert!(validate_migration_plan_path("/abs.toml").is_err());
        assert!(validate_migration_plan_path("").is_err());
    }
}
