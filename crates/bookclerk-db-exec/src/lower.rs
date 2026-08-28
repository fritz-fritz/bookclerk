//! Canonical Bookclerk SQL → engine lowering (provider-SDK / adapter boundary).
//!
//! Host domain plans emit SQLite-shaped canonical SQL (`?`, `INSERT OR IGNORE`,
//! `json_extract`, `json_valid`). Postgres adapters apply this table before
//! execution. Rewrites are SQL-token aware: quoted strings/identifiers, line
//! and block comments, and PostgreSQL dollar quotes are copied verbatim.
//! Do not call this from host domain compilers.

use sea_orm::DatabaseBackend;

/// Lowers canonical Bookclerk **DML/query** SQL for `backend` (identity for SQLite/D1).
///
/// Postgres adapters rewrite helpers (`IFNULL`, `json_extract`, `INSERT OR IGNORE`,
/// 2+-arg `min`/`max`, `json_valid`) and `?` placeholders. Binding and host
/// **DDL** type/identity rewrites (`AUTOINCREMENT`, `BLOB`, `INTEGER`) stay on
/// the adapter execution edge ([`crate::schema_sql_for_backend`],
/// [`crate::lower_binding_ddl_execute_request`]) so this function does not
/// classify statements.
#[must_use]
pub fn lower_canonical_sql(backend: DatabaseBackend, sql: &str) -> String {
    if backend != DatabaseBackend::Postgres {
        return sql.to_string();
    }
    lower_canonical_to_postgres(sql)
}

/// Lowers canonical SQLite-shaped SQL onto PostgreSQL.
#[must_use]
pub fn lower_canonical_to_postgres(sql: &str) -> String {
    let sql = insert_or_ignore_postgres(sql);
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
    let mut sql = rewrite_fn_name(sql, "ifnull", "COALESCE");
    sql = rewrite_fn_name(&sql, "json_object", "json_build_object");
    sql = rewrite_variadic_min_max(&sql);
    sql = rewrite_json_valid(&sql);
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

/// Replaces a function ident (`name(`) with `pg_name(` (case-insensitive).
fn rewrite_fn_name(sql: &str, name: &str, pg_name: &str) -> String {
    let mut i = 0;
    let mut out = String::with_capacity(sql.len() + pg_name.len());
    while i < sql.len() {
        if let Some(len) = literal_or_comment_len(&sql[i..]) {
            out.push_str(&sql[i..i + len]);
            i += len;
            continue;
        }
        if ident_call_at(sql, i, name) {
            out.push_str(pg_name);
            i += name.len();
            continue;
        }
        let ch = sql[i..].chars().next().unwrap_or('\0');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Rewrites 2+-arg `min`/`max` (SQLite scalars) to `LEAST`/`GREATEST`.
///
/// One-argument `MIN`/`MAX` stay aggregates. Nested calls are rewritten
/// inside-out.
fn rewrite_variadic_min_max(sql: &str) -> String {
    let sql = rewrite_variadic_fn(sql, "min", "LEAST");
    rewrite_variadic_fn(&sql, "max", "GREATEST")
}

/// Rewrites `name(...)` with two or more arguments to `pg_name(...)`.
fn rewrite_variadic_fn(sql: &str, name: &str, pg_name: &str) -> String {
    let mut i = 0;
    let mut out = String::with_capacity(sql.len());
    while i < sql.len() {
        if let Some(len) = literal_or_comment_len(&sql[i..]) {
            out.push_str(&sql[i..i + len]);
            i += len;
            continue;
        }
        if ident_call_at(sql, i, name) {
            let open = sql[i + name.len()..]
                .char_indices()
                .find(|(_, c)| !c.is_whitespace())
                .map(|(off, _)| i + name.len() + off)
                .unwrap_or(i + name.len());
            if let Some((args, rest)) = split_call_args(&sql[open + 1..]) {
                let rewritten: Vec<String> = args
                    .iter()
                    .map(|a| rewrite_variadic_fn(a, name, pg_name))
                    .collect();
                if rewritten.len() >= 2 {
                    out.push_str(pg_name);
                    out.push('(');
                    out.push_str(&rewritten.join(", "));
                    out.push(')');
                } else {
                    out.push_str(&sql[i..=open]);
                    out.push_str(&rewritten.join(", "));
                    out.push(')');
                }
                i = sql.len() - rest.len();
                continue;
            }
        }
        let ch = sql[i..].chars().next().unwrap_or('\0');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Rewrites `json_valid(expr) = 0/1` and bare `json_valid(expr)` to `IS [NOT] JSON`.
fn rewrite_json_valid(sql: &str) -> String {
    let mut i = 0;
    let mut out = String::with_capacity(sql.len());
    let name = "json_valid";
    while i < sql.len() {
        if let Some(len) = literal_or_comment_len(&sql[i..]) {
            out.push_str(&sql[i..i + len]);
            i += len;
            continue;
        }
        if ident_call_at(sql, i, name) {
            let open = sql[i + name.len()..]
                .char_indices()
                .find(|(_, c)| !c.is_whitespace())
                .map(|(off, _)| i + name.len() + off)
                .unwrap_or(i + name.len());
            if let Some((args, rest)) = split_call_args(&sql[open + 1..]) {
                if args.len() == 1 {
                    let expr = rewrite_json_valid(&args[0]);
                    let rest_trim = rest.trim_start();
                    if let Some(rest2) = rest_trim.strip_prefix('=') {
                        let rest2 = rest2.trim_start();
                        if let Some(rest2) = strip_json_valid_flag(rest2, '0') {
                            out.push_str(&format!("({expr} IS NOT JSON)"));
                            i = sql.len() - rest2.len();
                            continue;
                        }
                        if let Some(rest2) = strip_json_valid_flag(rest2, '1') {
                            out.push_str(&format!("({expr} IS JSON)"));
                            i = sql.len() - rest2.len();
                            continue;
                        }
                    }
                    out.push_str(&format!("(CASE WHEN ({expr}) IS JSON THEN 1 ELSE 0 END)"));
                    i = sql.len() - rest.len();
                    continue;
                }
            }
        }
        let ch = sql[i..].chars().next().unwrap_or('\0');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// True when `sql[i..]` is a call to `name(` (case-insensitive, word-bounded).
fn ident_call_at(sql: &str, i: usize, name: &str) -> bool {
    let rest = &sql[i..];
    if rest.len() < name.len() || !rest[..name.len()].eq_ignore_ascii_case(name) {
        return false;
    }
    if !word_boundary(sql, i.checked_sub(1)) {
        return false;
    }
    rest[name.len()..].chars().find(|c| !c.is_whitespace()) == Some('(')
}

/// Splits the argument list of a call whose `s` starts just after `(`.
///
/// Returns `(args, rest_after_closing_paren)`.
fn split_call_args(s: &str) -> Option<(Vec<String>, &str)> {
    let mut args = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    let mut i = 0;
    while i < s.len() {
        if let Some(len) = literal_or_comment_len(&s[i..]) {
            i += len;
            continue;
        }
        match s[i..].chars().next()? {
            '(' => depth += 1,
            ')' if depth == 0 => {
                let last = s[start..i].trim();
                if !last.is_empty() || !args.is_empty() {
                    args.push(last.to_string());
                }
                return Some((args, &s[i + 1..]));
            }
            ')' => depth -= 1,
            ',' if depth == 0 => {
                args.push(s[start..i].trim().to_string());
                start = i + 1;
            }
            _ => {}
        }
        i += s[i..].chars().next()?.len_utf8();
    }
    None
}

/// Rewrites `json_extract(expr, '$.a.b')` to a guarded `jsonb #>>` extract.
fn rewrite_json_extract(sql: &str) -> String {
    let mut i = 0;
    let mut out = String::with_capacity(sql.len());
    while i < sql.len() {
        if let Some(len) = literal_or_comment_len(&sql[i..]) {
            out.push_str(&sql[i..i + len]);
            i += len;
            continue;
        }
        if ident_call_at(sql, i, "json_extract") {
            let after = &sql[i + "json_extract".len()..];
            let after = after.trim_start();
            let after = after.strip_prefix('(').unwrap_or(after);
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

/// Mechanical SQLite→Postgres type/identity rewrites for one DDL statement.
///
/// Applied at the adapter execution edge ([`crate::schema_sql_for_backend`],
/// [`crate::lower_binding_ddl_execute_request`]), not by
/// [`lower_canonical_sql`]:
///
/// - `INTEGER PRIMARY KEY AUTOINCREMENT` → `BIGSERIAL PRIMARY KEY`
/// - `INTEGER` → `BIGINT` (shared SeaORM entities use `i64` everywhere)
/// - `REAL` → `DOUBLE PRECISION`
/// - `BLOB` → `BYTEA`
///
/// Rewrites are word-boundary and code-span aware; string literals and
/// comments are copied verbatim.
pub(crate) fn rewrite_canonical_ddl_types_for_postgres(sql: &str) -> String {
    let sql = replace_word_in_code(
        sql,
        "INTEGER PRIMARY KEY AUTOINCREMENT",
        "BIGSERIAL PRIMARY KEY",
    );
    let sql = replace_word_in_code(&sql, "INTEGER", "BIGINT");
    let sql = replace_word_in_code(&sql, "REAL", "DOUBLE PRECISION");
    replace_word_in_code(&sql, "BLOB", "BYTEA")
}

/// Lowers one canonical Bookclerk **DDL** statement onto PostgreSQL.
///
/// Type/identity rewrites plus [`lower_canonical_to_postgres`] (placeholders,
/// `INSERT OR IGNORE`). Host schema packs use this through
/// [`crate::schema_sql_for_backend`]; binding DDL uses the type rewrite at
/// the adapter edge and then the DML helper pass.
#[must_use]
pub fn lower_canonical_ddl_to_postgres(sql: &str) -> String {
    lower_canonical_to_postgres(&rewrite_canonical_ddl_types_for_postgres(sql))
}

/// True when `s` starts with JSON-valid flag `flag` (`0` or `1`) at a word boundary.
fn strip_json_valid_flag(s: &str, flag: char) -> Option<&str> {
    let rest = s.strip_prefix(flag)?;
    word_boundary(s, Some(flag.len_utf8())).then_some(rest)
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
    find_in_code_from(sql, needle, false)
}

/// Byte offset of the last `needle` in a code span (literals/comments skipped).
pub(crate) fn find_last_in_code(sql: &str, needle: &str) -> Option<usize> {
    find_in_code_from(sql, needle, true)
}

/// Byte offset of `needle` in a code span; `last` keeps scanning for the final match.
fn find_in_code_from(sql: &str, needle: &str, last: bool) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    let mut i = 0;
    let mut found = None;
    while i < sql.len() {
        if let Some(len) = literal_or_comment_len(&sql[i..]) {
            i += len;
            continue;
        }
        if sql[i..].starts_with(needle) {
            if !last {
                return Some(i);
            }
            found = Some(i);
            i += needle.len();
            continue;
        }
        let ch = sql[i..].chars().next()?;
        i += ch.len_utf8();
    }
    found
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
    fn postgres_rewrites_variadic_min_max_and_json_valid() {
        assert_eq!(
            lower_canonical_to_postgres("SELECT MAX(attempt_count, 1), min(a, b)"),
            "SELECT GREATEST(attempt_count, 1), LEAST(a, b)"
        );
        assert_eq!(
            lower_canonical_to_postgres("SELECT max(x)"),
            "SELECT max(x)"
        );
        let sql = lower_canonical_to_postgres("WHERE json_valid(doc) = 1 AND json_valid(body) = 0");
        assert!(sql.contains("(doc IS JSON)"), "{sql}");
        assert!(sql.contains("(body IS NOT JSON)"), "{sql}");
        assert!(!sql.contains("json_valid"), "{sql}");
        let bare = lower_canonical_to_postgres("SELECT json_valid(payload) FROM t");
        assert!(
            bare.contains("CASE WHEN (payload) IS JSON THEN 1 ELSE 0 END"),
            "{bare}"
        );
    }

    #[test]
    fn postgres_rewrites_ifnull_and_json_object_case_insensitively() {
        assert_eq!(
            lower_canonical_to_postgres("SELECT ifnull(NULL, 5), IFNULL(x, 0)"),
            "SELECT COALESCE(NULL, 5), COALESCE(x, 0)"
        );
        let sql = lower_canonical_to_postgres("SELECT json_object('k', ?), JSON_OBJECT('a', 1)");
        assert!(sql.contains("json_build_object('k', $1)"), "{sql}");
        assert!(sql.contains("json_build_object('a', 1)"), "{sql}");
        assert!(!sql.to_ascii_lowercase().contains("json_object("), "{sql}");
    }

    #[test]
    fn portable_select_lowers_helpers_for_postgres() {
        let sql = lower_canonical_to_postgres(crate::sql_v1::PORTABLE_SELECT);
        assert!(!sql.to_ascii_lowercase().contains("ifnull("), "{sql}");
        assert!(!sql.to_ascii_lowercase().contains("json_object("), "{sql}");
        assert!(sql.contains("COALESCE(NULL, 5)"), "{sql}");
        assert!(sql.contains("LEAST(1, 2)"), "{sql}");
        assert!(sql.contains("GREATEST(3, 1)"), "{sql}");
        assert!(sql.contains("json_build_object"), "{sql}");
        assert!(!sql.contains("json_extract("), "{sql}");
        assert!(!sql.contains("json_valid("), "{sql}");
    }

    #[test]
    fn postgres_dml_lowering_does_not_rewrite_ddl_types() {
        let canonical =
            "CREATE TABLE IF NOT EXISTS t (id INTEGER PRIMARY KEY AUTOINCREMENT, b BLOB)";
        let sql = lower_canonical_sql(DatabaseBackend::Postgres, canonical);
        assert!(
            sql.contains("AUTOINCREMENT") && sql.contains("BLOB"),
            "DDL type/identity lowering is adapter-owned, not generic DML lowering: {sql}"
        );
        let types = rewrite_canonical_ddl_types_for_postgres(canonical);
        assert!(types.contains("BIGSERIAL PRIMARY KEY"), "{types}");
        assert!(types.contains("BYTEA"), "{types}");
        assert!(!types.contains("AUTOINCREMENT"), "{types}");
        assert!(!types.contains("BLOB"), "{types}");
    }

    #[test]
    fn json_valid_flag_does_not_match_multi_digit() {
        let sql = lower_canonical_to_postgres("WHERE json_valid(doc) = 10");
        assert!(
            sql.contains("CASE WHEN (doc) IS JSON THEN 1 ELSE 0 END") && sql.contains("= 10"),
            "{sql}"
        );
        assert!(!sql.contains("doc IS JSON)"), "{sql}");
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
