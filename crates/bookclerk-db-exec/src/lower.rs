//! Canonical Bookclerk SQL → engine lowering (provider-SDK / adapter boundary).
//!
//! Host domain plans emit SQLite-shaped canonical SQL (`?`, `INSERT OR IGNORE`,
//! `json_extract`, `json_valid`). Postgres adapters apply this table before
//! execution. Do not call this from host domain compilers.

use sea_orm::DatabaseBackend;

/// Lowers canonical Bookclerk SQL for `backend` (identity for SQLite/D1).
#[must_use]
pub fn lower_canonical_sql(backend: DatabaseBackend, sql: &str) -> String {
    if backend == DatabaseBackend::Postgres {
        lower_canonical_to_postgres(sql)
    } else {
        sql.to_string()
    }
}

/// Lowers canonical SQLite-shaped SQL onto PostgreSQL.
#[must_use]
pub fn lower_canonical_to_postgres(sql: &str) -> String {
    let sql = insert_or_ignore_postgres(sql);
    let sql = sql.replace("json_object(", "json_build_object(");
    let sql = sqlite_fns_to_postgres(&sql);
    rewrite_placeholders_postgres(&sql)
}

/// Rewrites SQLite `?` placeholders to Postgres `$1`…`$n`.
fn rewrite_placeholders_postgres(sql: &str) -> String {
    let mut n = 0u32;
    let mut out = String::with_capacity(sql.len() + 16);
    for ch in sql.chars() {
        if ch == '?' {
            n += 1;
            out.push('$');
            out.push_str(&n.to_string());
        } else {
            out.push(ch);
        }
    }
    out
}

/// Renders `INSERT OR IGNORE` as `ON CONFLICT DO NOTHING` (before `RETURNING`).
fn insert_or_ignore_postgres(sql: &str) -> String {
    let Some(rest) = sql.trim_start().strip_prefix("INSERT OR IGNORE INTO") else {
        return sql.to_string();
    };
    if let Some(idx) = rest.find(" RETURNING ") {
        let (head, returning) = rest.split_at(idx);
        return format!("INSERT INTO{head} ON CONFLICT DO NOTHING{returning}");
    }
    format!("INSERT INTO{rest} ON CONFLICT DO NOTHING")
}

/// Maps SQLite helpers used in host plans onto PostgreSQL equivalents.
fn sqlite_fns_to_postgres(sql: &str) -> String {
    let mut sql = sql.replace("IFNULL(", "COALESCE(");
    sql = sql.replace("MAX(attempt_count, 1)", "GREATEST(attempt_count, 1)");
    sql = sql.replace("json_valid(payload) = 0", "(payload IS NOT JSON)");
    sql = sql.replace("json_valid(payload) = 1", "(payload IS JSON)");
    sql = rewrite_json_extract(&sql);
    sql = sql.replace(
        "json(payload)",
        "(CASE WHEN payload IS JSON THEN payload::jsonb END)",
    );
    sql = sql.replace(
        "json(CASE WHEN password_hash IS NOT NULL AND password_hash != '' THEN 'true' ELSE 'false' END)",
        "(password_hash IS NOT NULL AND password_hash != '')",
    );
    sql = sql.replace(
        "json(CASE WHEN cancel_requested != 0 THEN 'true' ELSE 'false' END)",
        "(cancel_requested != 0)",
    );
    sql = sql.replace(
        "json(CASE WHEN resume_pending != 0 THEN 'true' ELSE 'false' END)",
        "(resume_pending != 0)",
    );
    rewrite_julianday_delta(&sql)
}

/// Rewrites `json_extract(expr, '$.a.b')` to a guarded `jsonb #>>` extract.
fn rewrite_json_extract(sql: &str) -> String {
    let mut rest = sql;
    let mut out = String::with_capacity(sql.len());
    while let Some(idx) = rest.find("json_extract(") {
        out.push_str(&rest[..idx]);
        let after = &rest[idx + "json_extract(".len()..];
        let Some(comma) = find_top_level_comma(after) else {
            out.push_str("json_extract(");
            rest = after;
            continue;
        };
        let expr = after[..comma].trim();
        let after_comma = after[comma + 1..].trim_start();
        let Some(path) = after_comma.strip_prefix("'$.") else {
            out.push_str("json_extract(");
            rest = after;
            continue;
        };
        let Some(endq) = path.find('\'') else {
            out.push_str("json_extract(");
            rest = after;
            continue;
        };
        let json_path = &path[..endq];
        let remainder = path[endq + 1..].trim_start();
        let Some(rest2) = remainder.strip_prefix(')') else {
            out.push_str("json_extract(");
            rest = after;
            continue;
        };
        let pg_path = json_path.replace('.', ",");
        out.push_str(&format!(
            "(CASE WHEN ({expr}) IS JSON THEN (({expr})::jsonb #>> '{{{pg_path}}}') END)"
        ));
        rest = rest2;
    }
    out.push_str(rest);
    out
}

/// Index of the first comma at parenthesis depth 0.
fn find_top_level_comma(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// Rewrites the sqlite `julianday` dispatch-latency expression to `EXTRACT(EPOCH …)`.
fn rewrite_julianday_delta(sql: &str) -> String {
    const NEEDLE: &str = "CAST((julianday(?) - julianday((SELECT created_at FROM domain_events WHERE id = ?))) * 86400000 AS INTEGER)";
    const REPL: &str = "CAST(EXTRACT(EPOCH FROM (?::timestamptz - (SELECT created_at::timestamptz FROM domain_events WHERE id = ?))) * 1000 AS BIGINT)";
    sql.replace(NEEDLE, REPL)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postgres_rewrites_placeholders_and_json_extract() {
        let sql =
            lower_canonical_to_postgres("SELECT json_extract(payload, '$.v') FROM t WHERE id = ?");
        assert_eq!(
            sql,
            "SELECT (CASE WHEN (payload) IS JSON THEN ((payload)::jsonb #>> '{v}') END) FROM t WHERE id = $1"
        );
    }

    #[test]
    fn sqlite_backend_is_identity() {
        assert_eq!(
            lower_canonical_sql(DatabaseBackend::Sqlite, "a = ? AND b = ?"),
            "a = ? AND b = ?"
        );
    }
}
