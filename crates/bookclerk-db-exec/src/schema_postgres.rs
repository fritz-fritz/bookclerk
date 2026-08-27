//! Host-schema pack expansion at the adapter execution edge.
//!
//! `bookclerk-library` owns the canonical SQLite-shaped migration plan and
//! never lowers it. Adapters expand the pack here at execution: the SQLite
//! family applies canonical DDL verbatim; PostgreSQL lowers it mechanically
//! via [`crate::lower_canonical_ddl_to_postgres`] (types, `AUTOINCREMENT`,
//! `INSERT OR IGNORE`). There is no hand-authored parallel Postgres schema.

use std::borrow::Cow;

use bookclerk_plugin_abi::ExecuteRequest;
use sea_orm::DatabaseBackend;

/// SQL text for one host-schema statement on the live connection backend.
///
/// SQLite-family backends return the canonical statement unchanged; Postgres
/// applies the mechanical DDL lowering.
#[must_use]
pub fn schema_sql_for_backend(backend: DatabaseBackend, canonical: &str) -> Cow<'_, str> {
    match backend {
        DatabaseBackend::Postgres => Cow::Owned(crate::lower_canonical_ddl_to_postgres(canonical)),
        _ => Cow::Borrowed(canonical),
    }
}

/// True when `sql` is a host schema version marker (`schema_migrations` or `PRAGMA user_version`).
#[must_use]
pub fn is_host_schema_version_marker(sql: &str) -> bool {
    let t = sql.trim();
    t.starts_with("INSERT INTO schema_migrations")
        || t.starts_with("DELETE FROM schema_migrations")
        || t.starts_with("PRAGMA user_version =")
}

/// Splits a migration script on `;` and drops empty fragments.
#[must_use]
pub fn split_schema_statements(sql: &str) -> Vec<String> {
    sql.split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Expands `[canonical_ddl, version_marker, …]` at the adapter execution edge.
///
/// Host schema orchestration sends unsplit canonical DDL plus a version marker;
/// adapters lower the canonical pack for the live backend before execution.
#[must_use]
pub fn expand_host_schema_batch(backend: DatabaseBackend, batch: &[String]) -> Option<Vec<String>> {
    if batch.len() < 2 {
        return None;
    }
    let version = batch.last()?;
    if !is_host_schema_version_marker(version) {
        return None;
    }
    let canonical = batch.first()?;
    // Split first, lower per statement: statement-shaped rewrites such as
    // `INSERT OR IGNORE` → `ON CONFLICT DO NOTHING` anchor on the statement
    // head.
    let mut stmts: Vec<String> = split_schema_statements(canonical)
        .iter()
        .map(|stmt| schema_sql_for_backend(backend, stmt).into_owned())
        .collect();
    stmts.extend(batch.iter().skip(1).cloned());
    Some(stmts)
}

/// Expands a typed host schema batch at the adapter execution edge.
#[must_use]
pub fn expand_host_schema_execute_request(
    backend: DatabaseBackend,
    req: &ExecuteRequest,
) -> ExecuteRequest {
    let batch: Vec<String> = req.statements.iter().map(|s| s.sql.clone()).collect();
    let Some(expanded) = expand_host_schema_batch(backend, &batch) else {
        return req.clone();
    };
    // Rebuild even when the statement count is unchanged: per-statement
    // lowering may have rewritten SQL without splitting the pack further.
    if expanded == batch {
        return req.clone();
    }
    let Some(template) = req.statements.first().cloned() else {
        return req.clone();
    };
    let marker_template = req
        .statements
        .last()
        .cloned()
        .unwrap_or_else(|| template.clone());
    let n = expanded.len();
    let statements = expanded
        .into_iter()
        .enumerate()
        .map(|(i, sql)| {
            let mut stmt = if i + 1 == n && is_host_schema_version_marker(&sql) {
                marker_template.clone()
            } else {
                template.clone()
            };
            stmt.sql = sql;
            stmt
        })
        .collect();
    ExecuteRequest {
        statements,
        ..req.clone()
    }
}

/// Collapses statement results for an adapter-expanded host-schema request
/// back to the original wire request shape.
///
/// The canonical pack (statement 0) reports the summed `rowsAffected` of its
/// expanded statements; trailing statements (version marker, …) map
/// one-to-one, so the reply stays positional against the request the host
/// actually sent.
#[must_use]
pub fn collapse_host_schema_results(
    original_len: usize,
    results: Vec<bookclerk_plugin_abi::StatementResult>,
) -> Vec<bookclerk_plugin_abi::StatementResult> {
    if original_len == 0 || results.len() <= original_len {
        return results;
    }
    let tail = original_len - 1;
    let pack_len = results.len() - tail;
    let pack_affected: u64 = results[..pack_len]
        .iter()
        .map(|r| r.rows_affected)
        .fold(0, u64::saturating_add);
    let mut out = Vec::with_capacity(original_len);
    out.push(bookclerk_plugin_abi::StatementResult::from_affected(
        pack_affected,
    ));
    out.extend(results.into_iter().skip(pack_len));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use bookclerk_plugin_abi::{DbPlanStatementKind, DbResultSelection, TypedDbStatement};

    #[test]
    fn collapse_restores_wire_request_shape() {
        use bookclerk_plugin_abi::StatementResult;
        let results = vec![
            StatementResult::from_affected(1),
            StatementResult::from_affected(2),
            StatementResult::from_affected(3),
            StatementResult::from_affected(1),
        ];
        let out = collapse_host_schema_results(2, results);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].rows_affected, 6, "pack sums expanded statements");
        assert_eq!(out[1].rows_affected, 1, "marker maps one-to-one");
        // No-op when the adapter did not expand.
        let same = collapse_host_schema_results(
            2,
            vec![
                StatementResult::from_affected(4),
                StatementResult::from_affected(1),
            ],
        );
        assert_eq!(same.len(), 2);
        assert_eq!(same[0].rows_affected, 4);
    }

    #[test]
    fn expand_host_schema_batch_lowers_canonical_for_postgres() {
        let canonical = "CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY AUTOINCREMENT)";
        let batch = vec![
            canonical.to_string(),
            "INSERT INTO schema_migrations (version) VALUES (1)".to_string(),
        ];
        let expanded =
            expand_host_schema_batch(DatabaseBackend::Postgres, &batch).expect("host schema batch");
        assert!(
            expanded.iter().any(|s| s.contains("BIGSERIAL")),
            "adapter must lower canonical sqlite DDL for postgres: {expanded:?}"
        );
        assert!(
            !expanded[0].contains("AUTOINCREMENT"),
            "host canonical must not be pre-lowered: {}",
            expanded[0]
        );
        assert_eq!(
            expanded.last().map(String::as_str),
            Some("INSERT INTO schema_migrations (version) VALUES (1)")
        );
    }

    #[test]
    fn expand_host_schema_execute_request_preserves_marker_statement() {
        let req = ExecuteRequest {
            operation_id: "host-schema".into(),
            request_hash: String::new(),
            statements: vec![
                TypedDbStatement {
                    sql: "CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY AUTOINCREMENT)"
                        .into(),
                    parameters: vec![],
                    kind: DbPlanStatementKind::Execute,
                    max_rows: 0,
                    result_selection: DbResultSelection::AffectedRows,
                },
                TypedDbStatement {
                    sql: "INSERT INTO schema_migrations (version) VALUES (1)".into(),
                    parameters: vec![],
                    kind: DbPlanStatementKind::Execute,
                    max_rows: 0,
                    result_selection: DbResultSelection::AffectedRows,
                },
            ],
            deadline_unix_ms: 0,
        };
        let expanded = expand_host_schema_execute_request(DatabaseBackend::Postgres, &req);
        assert_eq!(expanded.statements.len(), req.statements.len());
        assert!(
            expanded
                .statements
                .iter()
                .any(|s| s.sql.contains("BIGSERIAL")),
            "typed execute must lower canonical DDL at adapter edge"
        );
        assert!(
            expanded
                .statements
                .iter()
                .all(|s| !s.sql.contains("AUTOINCREMENT")),
            "no sqlite-ism may reach the engine"
        );
        assert_eq!(
            expanded.statements.last().map(|s| s.sql.as_str()),
            Some("INSERT INTO schema_migrations (version) VALUES (1)")
        );
    }
}
