//! Canonical Bookclerk SQL → engine lowering (provider-SDK / adapter boundary).
//!
//! Host domain plans emit SQLite-shaped canonical SQL (`?`, `INSERT OR IGNORE`,
//! `json_extract`, `json_valid`). Postgres adapters apply this table before
//! execution. Rewrites are SQL-token aware: quoted strings/identifiers, line
//! and block comments, and PostgreSQL dollar quotes are copied verbatim.
//! Do not call this from host domain compilers.

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
    let sql = replace_in_code(&sql, "json_object(", "json_build_object(");
    let sql = sqlite_fns_to_postgres(&sql);
    rewrite_placeholders_postgres(&sql)
}

/// Rewrites SQLite `?` placeholders to Postgres `$1`…`$n` (code spans only).
fn rewrite_placeholders_postgres(sql: &str) -> String {
    let mut n = 0u32;
    let mut out = String::with_capacity(sql.len() + 16);
    let mut i = 0;
    while i < sql.len() {
        if let Some(len) = literal_or_comment_len(&sql[i..]) {
            out.push_str(&sql[i..i + len]);
            i += len;
            continue;
        }
        let ch = sql[i..].chars().next().unwrap_or('\0');
        if ch == '?' {
            n += 1;
            out.push('$');
            out.push_str(&n.to_string());
            i += 1;
        } else {
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// Renders `INSERT OR IGNORE` as `ON CONFLICT DO NOTHING` (before `RETURNING`).
fn insert_or_ignore_postgres(sql: &str) -> String {
    let trimmed = skip_trivia(sql);
    let Some(rest) = strip_prefix_ci(trimmed, "INSERT OR IGNORE INTO") else {
        return sql.to_string();
    };
    let prefix_len = sql.len() - trimmed.len();
    let rebuilt = if let Some(idx) = find_in_code(rest, " RETURNING ") {
        let (head, returning) = rest.split_at(idx);
        format!("INSERT INTO{head} ON CONFLICT DO NOTHING{returning}")
    } else {
        format!("INSERT INTO{rest} ON CONFLICT DO NOTHING")
    };
    let mut out = String::with_capacity(prefix_len + rebuilt.len());
    out.push_str(&sql[..prefix_len]);
    out.push_str(&rebuilt);
    out
}

/// Maps SQLite helpers used in host plans onto PostgreSQL equivalents.
fn sqlite_fns_to_postgres(sql: &str) -> String {
    let mut sql = replace_in_code(sql, "IFNULL(", "COALESCE(");
    sql = replace_in_code(&sql, "MAX(attempt_count, 1)", "GREATEST(attempt_count, 1)");
    sql = replace_in_code(&sql, "json_valid(payload) = 0", "(payload IS NOT JSON)");
    sql = replace_in_code(&sql, "json_valid(payload) = 1", "(payload IS JSON)");
    sql = rewrite_json_extract(&sql);
    sql = replace_in_code(
        &sql,
        "json(payload)",
        "(CASE WHEN payload IS JSON THEN payload::jsonb END)",
    );
    sql = replace_in_code(
        &sql,
        "json(CASE WHEN password_hash IS NOT NULL AND password_hash != '' THEN 'true' ELSE 'false' END)",
        "(password_hash IS NOT NULL AND password_hash != '')",
    );
    sql = replace_in_code(
        &sql,
        "json(CASE WHEN cancel_requested != 0 THEN 'true' ELSE 'false' END)",
        "(cancel_requested != 0)",
    );
    sql = replace_in_code(
        &sql,
        "json(CASE WHEN resume_pending != 0 THEN 'true' ELSE 'false' END)",
        "(resume_pending != 0)",
    );
    rewrite_julianday_delta(&sql)
}

/// Rewrites `json_extract(expr, '$.a.b')` to a guarded `jsonb #>>` extract.
fn rewrite_json_extract(sql: &str) -> String {
    let mut i = 0;
    let mut out = String::with_capacity(sql.len());
    let needle = "json_extract(";
    while i < sql.len() {
        if let Some(len) = literal_or_comment_len(&sql[i..]) {
            out.push_str(&sql[i..i + len]);
            i += len;
            continue;
        }
        if sql[i..].starts_with(needle) {
            let after = &sql[i + needle.len()..];
            if let Some((expr, json_path, rest2)) = parse_json_extract_args(after) {
                let expr = rewrite_json_extract(expr);
                let pg_path = json_path.replace('.', ",");
                out.push_str(&format!(
                    "(CASE WHEN ({expr}) IS JSON THEN (({expr})::jsonb #>> '{{{pg_path}}}') END)"
                ));
                i = sql.len() - rest2.len();
                continue;
            }
        }
        let ch = sql[i..].chars().next().unwrap_or('\0');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Parses `expr, '$.path')` from `json_extract(` arguments (string-aware).
fn parse_json_extract_args(after: &str) -> Option<(&str, &str, &str)> {
    let comma = find_top_level_comma(after)?;
    let expr = after[..comma].trim();
    let after_comma = after[comma + 1..].trim_start();
    let path = after_comma.strip_prefix("'$.")?;
    let endq = path.find('\'')?;
    let json_path = &path[..endq];
    let remainder = path[endq + 1..].trim_start();
    let rest2 = remainder.strip_prefix(')')?;
    Some((expr, json_path, rest2))
}

/// Index of the first comma at parenthesis depth 0, skipping literals.
fn find_top_level_comma(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut i = 0;
    while i < s.len() {
        if let Some(len) = literal_or_comment_len(&s[i..]) {
            i += len;
            continue;
        }
        match s[i..].chars().next()? {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => return Some(i),
            ch => {
                i += ch.len_utf8();
                continue;
            }
        }
        i += 1;
    }
    None
}

/// Rewrites the sqlite `julianday` dispatch-latency expression to `EXTRACT(EPOCH …)`.
fn rewrite_julianday_delta(sql: &str) -> String {
    const NEEDLE: &str = "CAST((julianday(?) - julianday((SELECT created_at FROM domain_events WHERE id = ?))) * 86400000 AS INTEGER)";
    const REPL: &str = "CAST(EXTRACT(EPOCH FROM (?::timestamptz - (SELECT created_at::timestamptz FROM domain_events WHERE id = ?))) * 1000 AS BIGINT)";
    replace_in_code(sql, NEEDLE, REPL)
}

/// Lowers one canonical Bookclerk **DDL** statement onto PostgreSQL.
///
/// Mechanical type/identity rewrites for the host schema pack, applied at
/// the adapter execution edge before [`lower_canonical_to_postgres`]:
///
/// - `INTEGER PRIMARY KEY AUTOINCREMENT` → `BIGSERIAL PRIMARY KEY`
/// - `INTEGER` → `BIGINT` (shared SeaORM entities use `i64` everywhere)
/// - `REAL` → `DOUBLE PRECISION`
/// - `BLOB` → `BYTEA`
///
/// Rewrites are word-boundary and code-span aware; string literals and
/// comments are copied verbatim.
#[must_use]
pub fn lower_canonical_ddl_to_postgres(sql: &str) -> String {
    let sql = replace_word_in_code(
        sql,
        "INTEGER PRIMARY KEY AUTOINCREMENT",
        "BIGSERIAL PRIMARY KEY",
    );
    let sql = replace_word_in_code(&sql, "INTEGER", "BIGINT");
    let sql = replace_word_in_code(&sql, "REAL", "DOUBLE PRECISION");
    let sql = replace_word_in_code(&sql, "BLOB", "BYTEA");
    lower_canonical_to_postgres(&sql)
}

/// True when the byte at `idx` (or the string edge) is not an identifier char.
fn word_boundary(sql: &str, idx: Option<usize>) -> bool {
    match idx.and_then(|i| sql.as_bytes().get(i)) {
        Some(b) => !(b.is_ascii_alphanumeric() || *b == b'_'),
        None => true,
    }
}

/// Replaces whole-word `from` with `to` in SQL code spans only.
fn replace_word_in_code(sql: &str, from: &str, to: &str) -> String {
    if from.is_empty() {
        return sql.to_string();
    }
    let mut i = 0;
    let mut out = String::with_capacity(sql.len());
    while i < sql.len() {
        if let Some(len) = literal_or_comment_len(&sql[i..]) {
            out.push_str(&sql[i..i + len]);
            i += len;
            continue;
        }
        if sql[i..].starts_with(from)
            && word_boundary(sql, i.checked_sub(1))
            && word_boundary(sql, Some(i + from.len()))
        {
            out.push_str(to);
            i += from.len();
            continue;
        }
        let ch = sql[i..].chars().next().unwrap_or('\0');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Replaces `from` with `to` only in SQL code spans (not strings/comments).
fn replace_in_code(sql: &str, from: &str, to: &str) -> String {
    if from.is_empty() {
        return sql.to_string();
    }
    let mut i = 0;
    let mut out = String::with_capacity(sql.len());
    while i < sql.len() {
        if let Some(len) = literal_or_comment_len(&sql[i..]) {
            out.push_str(&sql[i..i + len]);
            i += len;
            continue;
        }
        if sql[i..].starts_with(from) {
            out.push_str(to);
            i += from.len();
            continue;
        }
        let ch = sql[i..].chars().next().unwrap_or('\0');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Byte offset of `needle` in a code span, if present.
fn find_in_code(sql: &str, needle: &str) -> Option<usize> {
    let mut i = 0;
    while i < sql.len() {
        if let Some(len) = literal_or_comment_len(&sql[i..]) {
            i += len;
            continue;
        }
        if sql[i..].starts_with(needle) {
            return Some(i);
        }
        let ch = sql[i..].chars().next()?;
        i += ch.len_utf8();
    }
    None
}

/// Leading comments and whitespace.
fn skip_trivia(sql: &str) -> &str {
    let mut i = 0;
    while i < sql.len() {
        let rest = &sql[i..];
        let Some(ch) = rest.chars().next() else {
            break;
        };
        if ch.is_whitespace() {
            i += ch.len_utf8();
            continue;
        }
        if let Some(len) = comment_len(rest) {
            i += len;
            continue;
        }
        break;
    }
    &sql[i..]
}

/// ASCII-case-insensitive prefix strip.
fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() < prefix.len() {
        return None;
    }
    if s.get(..prefix.len())?.eq_ignore_ascii_case(prefix) {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

/// Length of a string, identifier, dollar-quote, or comment at `s`, if any.
fn literal_or_comment_len(s: &str) -> Option<usize> {
    comment_len(s).or_else(|| quoted_len(s))
}

/// Length of a `--` line comment or `/* */` block at `s`.
fn comment_len(s: &str) -> Option<usize> {
    if s.starts_with("--") {
        let nl = s.find('\n').unwrap_or(s.len());
        return Some(nl);
    }
    if s.starts_with("/*") {
        return s.find("*/").map(|i| i + 2).or(Some(s.len()));
    }
    None
}

/// Length of a quoted identifier/string or dollar-quote at `s`.
fn quoted_len(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    match bytes[0] {
        b'\'' => scan_quote(s, '\''),
        b'"' => scan_quote(s, '"'),
        b'$' => dollar_quote_len(s),
        _ => None,
    }
}

/// Length of a `'…'` or `"…"` literal, honoring doubled escapes.
fn scan_quote(s: &str, q: char) -> Option<usize> {
    let mut chars = s.char_indices();
    chars.next()?;
    while let Some((i, ch)) = chars.next() {
        if ch == q {
            if s[i + q.len_utf8()..].starts_with(q) {
                chars.next();
                continue;
            }
            return Some(i + q.len_utf8());
        }
    }
    Some(s.len())
}

/// Length of a PostgreSQL `$tag$…$tag$` dollar quote at `s`.
fn dollar_quote_len(s: &str) -> Option<usize> {
    let rest = s.get(1..)?;
    let tag_end = rest.find('$').filter(|i| {
        rest[..*i]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
    })?;
    let tag = &s[..=tag_end + 1];
    let after = s.get(tag.len()..)?;
    let close = after.find(tag)?;
    Some(tag.len() + close + tag.len())
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

    #[test]
    fn placeholders_inside_strings_and_comments_are_preserved() {
        let sql = lower_canonical_to_postgres(
            "SELECT '?' AS literal, /* ? */ ? FROM t WHERE x = '?' -- ?\nAND y = ?",
        );
        assert!(sql.contains("'?'"), "{sql}");
        assert!(sql.contains("/* ? */"), "{sql}");
        assert!(sql.contains("-- ?"), "{sql}");
        assert!(sql.contains("$1"), "{sql}");
        assert!(sql.contains("$2"), "{sql}");
        assert!(!sql.contains("$3"), "{sql}");
    }

    #[test]
    fn dollar_quotes_are_preserved() {
        let sql = lower_canonical_to_postgres("SELECT $tag$?$tag$, ?");
        assert!(sql.contains("$tag$?$tag$"), "{sql}");
        assert!(sql.contains("$1"), "{sql}");
        assert!(!sql.contains("$2"), "{sql}");
    }

    #[test]
    fn json_extract_inside_a_string_is_not_rewritten() {
        let sql = lower_canonical_to_postgres("SELECT 'json_extract(payload, ''$.v'')', ?");
        assert!(sql.contains("json_extract(payload"), "{sql}");
        assert!(sql.contains("$1"), "{sql}");
    }

    #[test]
    fn nested_json_extract_lowers() {
        let sql = lower_canonical_to_postgres(
            "SELECT json_extract(json_extract(payload, '$.a'), '$.b') FROM t",
        );
        assert!(!sql.contains("json_extract("), "{sql}");
        assert!(sql.contains("#>> '{a}'"), "{sql}");
        assert!(sql.contains("#>> '{b}'"), "{sql}");
    }

    #[test]
    fn sql_v1_corpus_drives_lowering() {
        let raw = include_str!("../testdata/sql_v1/corpus.json");
        let corpus: serde_json::Value = serde_json::from_str(raw).expect("sql_v1 corpus JSON");
        assert_eq!(corpus["contractVersion"], 1);
        for entry in corpus["lowering"].as_array().expect("lowering") {
            let id = entry["id"].as_str().unwrap_or("?");
            let canonical = entry["canonical"].as_str().expect("canonical");
            let got = lower_canonical_to_postgres(canonical);
            if let Some(exact) = entry["postgres"].as_str() {
                assert_eq!(got, exact, "corpus {id}");
            }
            if let Some(contains) = entry["postgresContains"].as_array() {
                for needle in contains {
                    let n = needle.as_str().unwrap();
                    assert!(got.contains(n), "corpus {id}: expected `{n}` in:\n{got}");
                }
            }
            if let Some(forbidden) = entry["postgresNotContains"].as_array() {
                for needle in forbidden {
                    let n = needle.as_str().unwrap();
                    assert!(!got.contains(n), "corpus {id}: unexpected `{n}` in:\n{got}");
                }
            }
        }
        for entry in corpus["sqliteIdentity"].as_array().expect("sqliteIdentity") {
            let id = entry["id"].as_str().unwrap_or("?");
            let canonical = entry["canonical"].as_str().expect("canonical");
            assert_eq!(
                lower_canonical_sql(DatabaseBackend::Sqlite, canonical),
                canonical,
                "corpus {id}"
            );
        }
    }
}
