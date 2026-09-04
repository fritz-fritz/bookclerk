//! Full preflight of a recovery point before any destructive restore.

use std::collections::HashSet;

use bookclerk_plugin_abi::SQL_CONTRACT_VERSION;

use super::encode::CanonicalObject;
use super::repository::BackupRepository;
use super::schema::admit_canonical_schema;
use super::util::validate_cell;
use super::{
    BackupUnit, CanonicalDatabaseSchema, DatabaseUnitKind, ValidatedBackup, BACKUP_FORMAT_VERSION,
};
use crate::error::{LibraryError, Result};

/// Parses the manifest, rejects unsupported formats, and verifies every
/// referenced object **before** the caller starts a destructive restore.
///
/// # Errors
///
/// Returns when the manifest is malformed, an object is missing/corrupt, schema
/// is not fully admitted, or typed cells violate the contract.
pub fn verify_recovery_point(repo: &BackupRepository, id: &str) -> Result<ValidatedBackup> {
    let manifest = repo.read_manifest(id)?;
    if manifest.format_version != BACKUP_FORMAT_VERSION {
        return Err(LibraryError::Schema(format!(
            "unsupported backup format version {} (this binary supports {BACKUP_FORMAT_VERSION})",
            manifest.format_version
        )));
    }
    if manifest.id.is_empty() || manifest.created_at.is_empty() {
        return Err(LibraryError::Schema(
            "backup manifest is missing id or created_at".into(),
        ));
    }
    if manifest.units.is_empty() {
        return Err(LibraryError::Schema(
            "backup manifest lists no database units".into(),
        ));
    }
    let mut library = None;
    let mut plugin_units = Vec::new();
    let mut plugin_ids = HashSet::new();
    for unit in &manifest.units {
        verify_unit(repo, unit)?;
        match unit.kind {
            DatabaseUnitKind::Library => {
                if library.is_some() {
                    return Err(LibraryError::Schema(
                        "backup lists more than one library unit".into(),
                    ));
                }
                library = Some(unit.clone());
            }
            DatabaseUnitKind::PluginBinding => {
                let plugin_id = unit.plugin_id.as_deref().ok_or_else(|| {
                    LibraryError::Schema("plugin backup unit is missing plugin_id".into())
                })?;
                let binding = unit.binding.as_deref().ok_or_else(|| {
                    LibraryError::Schema("plugin backup unit is missing binding".into())
                })?;
                if plugin_id.is_empty() || binding.is_empty() {
                    return Err(LibraryError::Schema(
                        "plugin backup unit has an empty logical identity".into(),
                    ));
                }
                if !plugin_ids.insert((plugin_id.to_string(), binding.to_string())) {
                    return Err(LibraryError::Schema(format!(
                        "backup lists duplicate plugin identity `{plugin_id}/{binding}`"
                    )));
                }
                plugin_units.push(unit.clone());
            }
        }
    }
    let library = library
        .ok_or_else(|| LibraryError::Schema("backup is missing a library database unit".into()))?;
    Ok(ValidatedBackup {
        manifest,
        library,
        plugin_units,
    })
}

/// Verifies one unit's objects, admitted schema, and typed rows.
///
/// # Errors
///
/// Returns when any object is missing, corrupt, or semantically invalid.
pub fn verify_unit(repo: &BackupRepository, unit: &BackupUnit) -> Result<()> {
    if unit.sql_contract_version == 0 || unit.sql_contract_version > SQL_CONTRACT_VERSION {
        return Err(LibraryError::Schema(format!(
            "backup SQL contract version {} is not supported (this binary supports {SQL_CONTRACT_VERSION})",
            unit.sql_contract_version
        )));
    }
    let schema = load_admitted_schema(repo, unit)?;
    let identity = repo.get_object(&unit.identity_object)?;
    match identity {
        CanonicalObject::Identity { .. } => {}
        other => {
            return Err(LibraryError::Schema(format!(
                "backup identity object is `{other:?}`, not Identity"
            )));
        }
    }
    let mut seen = HashSet::new();
    for table in &unit.tables {
        if !seen.insert(table.name.clone()) {
            return Err(LibraryError::Schema(format!(
                "backup unit lists table `{}` more than once",
                table.name
            )));
        }
        let expected = schema
            .tables
            .iter()
            .find(|t| t.parsed.table == table.name)
            .ok_or_else(|| {
                LibraryError::Schema(format!(
                    "backup has data for `{}` without CREATE TABLE SQL",
                    table.name
                ))
            })?;
        let cols: Vec<String> = expected
            .parsed
            .columns
            .iter()
            .map(|(c, _)| c.clone())
            .collect();
        if table.columns != cols {
            return Err(LibraryError::Schema(format!(
                "backup columns for `{}` do not match admitted schema",
                table.name
            )));
        }
        for digest in &table.chunks {
            let object = repo.get_object(digest)?;
            let CanonicalObject::TableChunk {
                table: chunk_table,
                columns,
                rows,
            } = object
            else {
                return Err(LibraryError::Schema(format!(
                    "backup chunk `{digest}` is not table data"
                )));
            };
            if chunk_table != table.name || columns != table.columns {
                return Err(LibraryError::Schema(format!(
                    "backup chunk `{digest}` does not match table `{}`",
                    table.name
                )));
            }
            for (i, row) in rows.iter().enumerate() {
                if row.len() != columns.len() {
                    return Err(LibraryError::Schema(format!(
                        "backup `{}` row {i} has {} cells; expected {}",
                        table.name,
                        row.len(),
                        columns.len()
                    )));
                }
                for (j, cell) in row.iter().enumerate() {
                    let (col, ty) = &expected.parsed.columns[j];
                    let not_null = expected
                        .parsed
                        .column_not_null
                        .get(j)
                        .copied()
                        .unwrap_or(false);
                    validate_cell(&table.name, col, cell, *ty, not_null)?;
                }
            }
        }
    }
    Ok(())
}

/// Loads and fully admits the schema object for `unit`.
///
/// # Errors
///
/// Returns when the object is missing or not fully admitted Bookclerk SQL.
pub fn load_admitted_schema(
    repo: &BackupRepository,
    unit: &BackupUnit,
) -> Result<CanonicalDatabaseSchema> {
    let object = repo.get_object(&unit.schema_object)?;
    let CanonicalObject::Schema {
        sql_contract_version,
        statements,
    } = object
    else {
        return Err(LibraryError::Schema(
            "backup schema object is not Schema".into(),
        ));
    };
    if sql_contract_version != unit.sql_contract_version {
        return Err(LibraryError::Schema(
            "backup schema object sql_contract_version does not match the unit".into(),
        ));
    }
    let sql = statements.join(";\n");
    let admitted = admit_canonical_schema(sql_contract_version, &sql)?;
    if admitted.schema_sql() != statements {
        // Formatting may differ only if a statement failed to round-trip; fail closed.
        if admitted.tables.len()
            != statements
                .iter()
                .filter(|s| bookclerk_plugin_abi::parse_create_table_schema(s).is_some())
                .count()
            || admitted.indexes.len()
                != statements
                    .iter()
                    .filter(|s| bookclerk_plugin_abi::parse_create_index_sql(s).is_some())
                    .count()
        {
            return Err(LibraryError::Schema(
                "backup schema object is not fully admitted Bookclerk SQL".into(),
            ));
        }
    }
    Ok(admitted)
}
