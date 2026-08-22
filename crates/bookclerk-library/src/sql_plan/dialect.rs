//! SQLite vs PostgreSQL rendering for host-authored plans.

/// SQL dialect family negotiated with the database guest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlFamily {
    /// SQLite (including Cloudflare D1).
    Sqlite,
    /// PostgreSQL.
    Postgres,
}

impl SqlFamily {
    /// Parses `sqlite` / `postgres` (case-insensitive).
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "sqlite" => Some(Self::Sqlite),
            "postgres" | "postgresql" | "pg" => Some(Self::Postgres),
            _ => None,
        }
    }

    /// SeaORM backend for this family.
    #[must_use]
    pub fn sea_backend(self) -> sea_orm::DatabaseBackend {
        match self {
            Self::Sqlite => sea_orm::DatabaseBackend::Sqlite,
            Self::Postgres => sea_orm::DatabaseBackend::Postgres,
        }
    }

    /// Family for a SeaORM connection backend.
    #[must_use]
    pub fn from_sea(backend: sea_orm::DatabaseBackend) -> Self {
        match backend {
            sea_orm::DatabaseBackend::Postgres => Self::Postgres,
            _ => Self::Sqlite,
        }
    }
}

/// Rewrites SQLite `?` placeholders to Postgres `$1`…`$n`.
#[must_use]
pub fn rewrite_placeholders(family: SqlFamily, sql: &str) -> String {
    if family != SqlFamily::Postgres {
        return sql.to_string();
    }
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

/// Renders `INSERT OR IGNORE` for SQLite or `ON CONFLICT DO NOTHING` for Postgres.
#[must_use]
pub fn insert_or_ignore(family: SqlFamily, sql: &str) -> String {
    if family != SqlFamily::Postgres {
        return sql.to_string();
    }
    if let Some(rest) = sql.trim_start().strip_prefix("INSERT OR IGNORE INTO") {
        return format!("INSERT INTO{rest} ON CONFLICT DO NOTHING");
    }
    sql.to_string()
}

/// Rewrites SQLite `json_object(` to Postgres `json_build_object(`.
#[must_use]
pub fn json_object_fn(family: SqlFamily, sql: &str) -> String {
    if family != SqlFamily::Postgres {
        return sql.to_string();
    }
    sql.replace("json_object(", "json_build_object(")
}

/// Applies dialect rewrites to one statement.
#[must_use]
pub fn render_statement(family: SqlFamily, sql: &str) -> String {
    let sql = insert_or_ignore(family, sql);
    let sql = json_object_fn(family, &sql);
    let sql = sqlite_fns_to_postgres(family, &sql);
    rewrite_placeholders(family, &sql)
}

/// Maps SQLite helpers used in host plans onto PostgreSQL equivalents.
fn sqlite_fns_to_postgres(family: SqlFamily, sql: &str) -> String {
    if family != SqlFamily::Postgres {
        return sql.to_string();
    }
    let mut sql = sql.replace("IFNULL(", "COALESCE(");
    sql = sql.replace("MAX(attempt_count, 1)", "GREATEST(attempt_count, 1)");
    // PG16+ `IS JSON` is the portable equivalent of SQLite `json_valid`.
    // Do not rewrite these to TRUE/FALSE: claim must quarantine poison rows
    // and skip them, not cast invalid text to jsonb (which aborts the batch).
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
    sql = rewrite_julianday_delta(&sql);
    sql
}

/// Rewrites `json_extract(expr, '$.a.b')` to `(expr)::jsonb #>> '{a,b}'`.
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
        // Guard the cast: Postgres OR/AND do not short-circuit, so
        // `json_valid = 0 OR json_extract(...)` must not evaluate `::jsonb`
        // on malformed text.
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
        let sql = render_statement(
            SqlFamily::Postgres,
            "SELECT json_extract(payload, '$.v') FROM t WHERE id = ?",
        );
        assert_eq!(
            sql,
            "SELECT (CASE WHEN (payload) IS JSON THEN ((payload)::jsonb #>> '{v}') END) FROM t WHERE id = $1"
        );
    }

    #[test]
    fn postgres_rewrites_json_valid_to_is_json() {
        let invalid = render_statement(
            SqlFamily::Postgres,
            "WHERE json_valid(payload) = 0 OR json_extract(payload, '$.v') IS NULL",
        );
        assert!(
            invalid.contains("(payload IS NOT JSON)"),
            "malformed check must stay a real predicate:\n{invalid}"
        );
        assert!(
            !invalid.contains("FALSE") && !invalid.contains("json_valid"),
            "{invalid}"
        );
        assert!(
            invalid.contains("IS JSON THEN"),
            "json_extract must not cast invalid text:\n{invalid}"
        );

        let valid = render_statement(
            SqlFamily::Postgres,
            "AND json_valid(payload) = 1 AND json_extract(payload, '$.v') = '1'",
        );
        assert!(
            valid.contains("(payload IS JSON)"),
            "valid check must stay a real predicate:\n{valid}"
        );
        assert!(
            !valid.contains("TRUE") && !valid.contains("json_valid"),
            "{valid}"
        );
    }

    #[test]
    fn sqlite_leaves_question_marks() {
        assert_eq!(
            render_statement(SqlFamily::Sqlite, "a = ? AND b = ?"),
            "a = ? AND b = ?"
        );
    }
}
