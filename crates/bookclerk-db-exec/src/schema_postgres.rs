//! Adapter execution-edge SQL lowering (host schema packs and binding DDL).
//!
//! `bookclerk-library` owns canonical SQLite-shaped SQL and never lowers it.
//! SeaORM adapters invoke these helpers at execute: the SQLite family applies
//! canonical DDL verbatim; PostgreSQL lowers types/`AUTOINCREMENT` here (not
//! inside [`crate::lower_canonical_sql`], which stays DML/query helpers).
//! There is no hand-authored parallel Postgres schema.

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

/// Mechanical type/identity lowering for one **binding** statement.
///
/// Hosts emit canonical SQLite-shaped `CREATE`/`DROP`. Postgres adapters
/// rewrite `AUTOINCREMENT`/`BLOB`/`INTEGER`/`REAL` here; SQLite/D1 leave the
/// statement unchanged. DML stays for [`crate::lower_canonical_sql`].
#[must_use]
pub fn lower_binding_sql_for_backend(backend: DatabaseBackend, sql: &str) -> Cow<'_, str> {
    if backend == DatabaseBackend::Postgres && bookclerk_plugin_abi::statement_is_ddl(sql) {
        Cow::Owned(crate::lower::rewrite_canonical_ddl_types_for_postgres(sql))
    } else {
        Cow::Borrowed(sql)
    }
}

/// Applies [`lower_binding_sql_for_backend`] to every statement in `req`.
///
/// Identity when no statement changes (SQLite/D1, or Postgres DML-only).
#[must_use]
pub fn lower_binding_ddl_execute_request(
    backend: DatabaseBackend,
    req: &ExecuteRequest,
) -> ExecuteRequest {
    if backend != DatabaseBackend::Postgres {
        return req.clone();
    }
    let mut out = req.clone();
    let mut changed = false;
    for stmt in &mut out.statements {
        let lowered = lower_binding_sql_for_backend(backend, &stmt.sql);
        if lowered.as_ref() != stmt.sql.as_str() {
            stmt.sql = lowered.into_owned();
            changed = true;
        }
    }
    if changed {
        out
    } else {
        req.clone()
    }
}

/// True when `sql` is a host schema version marker (`schema_migrations` or `PRAGMA user_version`).
#[must_use]
pub fn is_host_schema_version_marker(sql: &str) -> bool {
    let t = sql.trim();
    t.starts_with("INSERT INTO schema_migrations") || t.starts_with("PRAGMA user_version =")
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
    let mut stmts: Vec<String> = Vec::new();
    for stmt in split_schema_statements(canonical) {
        let lowered = schema_sql_for_backend(backend, &stmt).into_owned();
        let companions = if backend == DatabaseBackend::Postgres {
            postgres_identity_companions(&lowered)
        } else {
            Vec::new()
        };
        stmts.push(lowered);
        stmts.extend(companions);
    }
    stmts.extend(batch.iter().skip(1).cloned());
    Some(stmts)
}

/// Postgres-only companion DDL that keeps `BIGSERIAL` in sync with explicit ids.
///
/// SQLite `AUTOINCREMENT` records the largest value ever inserted. PostgreSQL
/// serial sequences do not. After a binding/host `CREATE TABLE` that lowered to
/// `BIGSERIAL`, the adapter runs this function + per-table trigger internally
/// (not as extra Cap'n statements). Empty when `sql` is not a CREATE TABLE
/// with an identity column.
#[must_use]
pub fn postgres_identity_companions(sql: &str) -> Vec<String> {
    if !create_table_has_identity(sql) {
        return Vec::new();
    }
    let Some(table) = create_table_ident(sql) else {
        return Vec::new();
    };
    vec![
        BOOKCLERK_SYNC_IDENTITY_FN.to_string(),
        format!("DROP TRIGGER IF EXISTS bookclerk_sync_identity ON {table}"),
        format!(
            "CREATE TRIGGER bookclerk_sync_identity AFTER INSERT ON {table} \
             FOR EACH ROW EXECUTE FUNCTION bookclerk_sync_identity()"
        ),
    ]
}

/// Shared trigger function: `setval` serial columns to `GREATEST(last, NEW.col)`.
const BOOKCLERK_SYNC_IDENTITY_FN: &str = "\
CREATE OR REPLACE FUNCTION bookclerk_sync_identity() RETURNS trigger \
LANGUAGE plpgsql AS $bookclerk_sync$ \
DECLARE seq text; col name; val bigint; last bigint; \
BEGIN \
  FOR col IN \
    SELECT a.attname FROM pg_attribute a \
    WHERE a.attrelid = TG_RELID AND a.attnum > 0 AND NOT a.attisdropped \
      AND pg_get_serial_sequence(format('%I.%I', TG_TABLE_SCHEMA, TG_TABLE_NAME), a.attname) IS NOT NULL \
  LOOP \
    seq := pg_get_serial_sequence(format('%I.%I', TG_TABLE_SCHEMA, TG_TABLE_NAME), col); \
    EXECUTE format('SELECT ($1).%I', col) INTO val USING NEW; \
    IF val IS NOT NULL THEN \
      EXECUTE format('SELECT last_value FROM %s', seq) INTO last; \
      PERFORM setval(seq::regclass, GREATEST(last, val), true); \
    END IF; \
  END LOOP; \
  RETURN NEW; \
END; \
$bookclerk_sync$";

/// True when `sql` is `CREATE TABLE` with `AUTOINCREMENT` or `BIGSERIAL`.
fn create_table_has_identity(sql: &str) -> bool {
    let t = skip_create_table_head(sql);
    if t.is_none() {
        return false;
    }
    let upper = sql.to_ascii_uppercase();
    upper.contains("AUTOINCREMENT") || upper.contains("BIGSERIAL")
}

/// Accepts a `CREATE [TEMP[ORARY]] TABLE` statement head.
fn skip_create_table_head(sql: &str) -> Option<()> {
    let mut scan = sql.trim_start();
    if !starts_kw(scan, "CREATE") {
        return None;
    }
    scan = skip_kw(scan, "CREATE")?;
    if starts_kw(scan, "TEMP") {
        scan = skip_kw(scan, "TEMP")?;
    } else if starts_kw(scan, "TEMPORARY") {
        scan = skip_kw(scan, "TEMPORARY")?;
    }
    if !starts_kw(scan, "TABLE") {
        return None;
    }
    let _ = scan;
    Some(())
}

/// Unquoted table ident from a `CREATE TABLE` statement.
fn create_table_ident(sql: &str) -> Option<String> {
    let mut s = sql.trim_start();
    s = skip_kw(s, "CREATE")?;
    if starts_kw(s, "TEMP") {
        s = skip_kw(s, "TEMP")?;
    } else if starts_kw(s, "TEMPORARY") {
        s = skip_kw(s, "TEMPORARY")?;
    }
    s = skip_kw(s, "TABLE")?;
    if starts_kw(s, "IF") {
        s = skip_kw(s, "IF")?;
        if starts_kw(s, "NOT") {
            s = skip_kw(s, "NOT")?;
        }
        if starts_kw(s, "EXISTS") {
            s = skip_kw(s, "EXISTS")?;
        }
    }
    read_unquoted_ident(s)
}

/// Case-insensitive keyword at the start of `s`.
fn starts_kw(s: &str, kw: &str) -> bool {
    let s = s.trim_start();
    s.len() >= kw.len()
        && s[..kw.len()].eq_ignore_ascii_case(kw)
        && s.as_bytes()
            .get(kw.len())
            .is_none_or(|b| !b.is_ascii_alphanumeric() && *b != b'_')
}

/// Consumes `kw` from the start of `s`.
fn skip_kw<'a>(s: &'a str, kw: &str) -> Option<&'a str> {
    let s = s.trim_start();
    if !starts_kw(s, kw) {
        return None;
    }
    Some(s[kw.len()..].trim_start())
}

/// Next unquoted ident at the start of `s`.
fn read_unquoted_ident(s: &str) -> Option<String> {
    let s = s.trim_start();
    let mut chars = s.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic() && first != '_' {
        return None;
    }
    let mut n = first.len_utf8();
    for c in chars {
        if c.is_ascii_alphanumeric() || c == '_' {
            n += c.len_utf8();
        } else {
            break;
        }
    }
    Some(s[..n].to_string())
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
            expanded
                .iter()
                .any(|s| s.contains("bookclerk_sync_identity")),
            "identity CREATE TABLE must install serial-sync companions: {expanded:?}"
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
        assert!(
            expanded.statements.len() > req.statements.len(),
            "identity CREATE TABLE must expand with serial-sync companions"
        );
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
                .any(|s| s.sql.contains("bookclerk_sync_identity")),
            "typed execute must attach identity companions"
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

    #[test]
    fn postgres_adapter_lowers_binding_ddl_types() {
        let canonical =
            "CREATE TABLE IF NOT EXISTS t (id INTEGER PRIMARY KEY AUTOINCREMENT, b BLOB, n INTEGER, r REAL)";
        let req = ExecuteRequest {
            operation_id: "binding-ddl".into(),
            request_hash: String::new(),
            statements: vec![
                TypedDbStatement {
                    sql: canonical.into(),
                    parameters: vec![],
                    kind: DbPlanStatementKind::Execute,
                    max_rows: 0,
                    result_selection: DbResultSelection::AffectedRows,
                },
                TypedDbStatement {
                    sql: "INSERT INTO t (n) VALUES (?)".into(),
                    parameters: vec![],
                    kind: DbPlanStatementKind::Execute,
                    max_rows: 0,
                    result_selection: DbResultSelection::AffectedRows,
                },
            ],
            deadline_unix_ms: 0,
        };
        let sqlite = lower_binding_ddl_execute_request(DatabaseBackend::Sqlite, &req);
        assert_eq!(sqlite.statements[0].sql, canonical);
        let pg = lower_binding_ddl_execute_request(DatabaseBackend::Postgres, &req);
        assert!(
            pg.statements[0].sql.contains("BIGSERIAL PRIMARY KEY"),
            "{}",
            pg.statements[0].sql
        );
        assert!(
            pg.statements[0].sql.contains("BYTEA"),
            "{}",
            pg.statements[0].sql
        );
        assert!(
            pg.statements[0].sql.contains("BIGINT"),
            "{}",
            pg.statements[0].sql
        );
        assert!(
            pg.statements[0].sql.contains("DOUBLE PRECISION"),
            "{}",
            pg.statements[0].sql
        );
        assert!(
            !pg.statements[0].sql.contains("AUTOINCREMENT"),
            "{}",
            pg.statements[0].sql
        );
        assert!(
            !pg.statements[0].sql.contains("BLOB"),
            "{}",
            pg.statements[0].sql
        );
        assert_eq!(
            pg.statements[1].sql, req.statements[1].sql,
            "DML must not take the DDL type pass"
        );
    }

    #[test]
    fn postgres_adapter_lowers_lowercase_binding_ddl_and_keeps_boolean() {
        let canonical = "create table if not exists t (\
             id integer primary key autoincrement, b blob, flag boolean)";
        let req = ExecuteRequest {
            operation_id: "binding-ddl-lc".into(),
            request_hash: String::new(),
            statements: vec![TypedDbStatement {
                sql: canonical.into(),
                parameters: vec![],
                kind: DbPlanStatementKind::Execute,
                max_rows: 0,
                result_selection: DbResultSelection::AffectedRows,
            }],
            deadline_unix_ms: 0,
        };
        let pg = lower_binding_ddl_execute_request(DatabaseBackend::Postgres, &req);
        assert!(
            pg.statements[0].sql.contains("BIGSERIAL PRIMARY KEY"),
            "{}",
            pg.statements[0].sql
        );
        assert!(
            pg.statements[0].sql.contains("BYTEA"),
            "{}",
            pg.statements[0].sql
        );
        assert!(
            pg.statements[0].sql.contains("boolean"),
            "{}",
            pg.statements[0].sql
        );
        assert!(
            !pg.statements[0].sql.contains("autoincrement"),
            "{}",
            pg.statements[0].sql
        );
        assert!(
            !pg.statements[0].sql.contains("blob"),
            "{}",
            pg.statements[0].sql
        );
    }

    #[test]
    fn postgres_identity_companions_follow_autoincrement_create() {
        let sql =
            "CREATE TABLE IF NOT EXISTS ident (id INTEGER PRIMARY KEY AUTOINCREMENT, n INTEGER)";
        let companions = postgres_identity_companions(sql);
        assert_eq!(companions.len(), 3, "{companions:?}");
        assert!(
            companions[0].contains("CREATE OR REPLACE FUNCTION bookclerk_sync_identity"),
            "{}",
            companions[0]
        );
        assert!(
            companions[1].contains("DROP TRIGGER IF EXISTS bookclerk_sync_identity ON ident"),
            "{}",
            companions[1]
        );
        assert!(
            companions[2].contains("EXECUTE FUNCTION bookclerk_sync_identity()"),
            "{}",
            companions[2]
        );
        assert!(postgres_identity_companions("CREATE TABLE t (n INTEGER)").is_empty());
    }
}
