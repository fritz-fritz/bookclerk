//! Host-side grammar and scope checks for guest-authored typed SQL.
//!
//! Overwriting [`crate::DbPlanStatementKind`] is classification, not
//! authorization. Guests may only run canonical DML/SELECT against allowed
//! tables, with bind counts and result-selection fields that match the
//! statement. Host-authored schema batches must not use this path.

#![allow(clippy::missing_docs_in_private_items)]

use crate::{
    DbPlanStatementKind, DbResultSelection, ExecuteRequest, PluginError, Result, TypedDbStatement,
};

/// First-party catalog / engine tables guests must not name.
const DENIED_TABLES: &[&str] = &[
    "encrypted_secrets",
    "sqlite_master",
    "sqlite_temp_master",
    "sqlite_schema",
    "sqlite_temp_schema",
    "sqlite_sequence",
    "information_schema",
];

/// Leading verbs that are never valid guest SQL (DDL, session, admin).
const DENIED_VERBS: &[&str] = &[
    "ALTER",
    "ANALYZE",
    "ATTACH",
    "BEGIN",
    "CALL",
    "CHECKPOINT",
    "CLUSTER",
    "COMMENT",
    "COMMIT",
    "COPY",
    "CREATE",
    "DEALLOCATE",
    "DECLARE",
    "DETACH",
    "DISCARD",
    "DO",
    "DROP",
    "END",
    "EXECUTE",
    "EXPLAIN",
    "GRANT",
    "INSTALL",
    "LISTEN",
    "LOAD",
    "LOCK",
    "NOTIFY",
    "PRAGMA",
    "PREPARE",
    "REINDEX",
    "RELEASE",
    "REVOKE",
    "ROLLBACK",
    "SAVEPOINT",
    "SECURITY",
    "SET",
    "START",
    "TRUNCATE",
    "UNLISTEN",
    "VACUUM",
];

/// Classifies guest SQL the same way the host stamps `DbStatement.kind`.
#[must_use]
pub fn guest_statement_kind(sql: &str) -> DbPlanStatementKind {
    if has_top_level_keyword(sql, "RETURNING") {
        return DbPlanStatementKind::Returning;
    }
    match first_top_level_keyword(sql).as_deref() {
        Some("SELECT" | "WITH" | "VALUES") => DbPlanStatementKind::Select,
        _ => DbPlanStatementKind::Execute,
    }
}

/// Rejects guest SQL that is outside the Bookclerk binding grammar.
///
/// Checks statement verbs, named tables, positional bind counts,
/// `resultSelection` vs classified kind, and `maxRows` consistency.
/// Does **not** stamp the request hash or measure encoded size.
///
/// # Errors
///
/// Returns [`PluginError::invalid_params`] when the request is not allowed.
pub fn validate_guest_execute_request(req: &ExecuteRequest) -> Result<()> {
    if req.statements.is_empty() {
        return Err(PluginError::invalid_params(
            "executeAtomic statements must be non-empty",
        ));
    }
    for (i, stmt) in req.statements.iter().enumerate() {
        validate_guest_statement(i, stmt)?;
    }
    Ok(())
}

fn validate_guest_statement(index: usize, stmt: &TypedDbStatement) -> Result<()> {
    if stmt.sql.trim().is_empty() {
        return Err(PluginError::invalid_params(format!(
            "statement {index} SQL is empty"
        )));
    }
    if has_top_level_semicolon_tail(&stmt.sql) {
        return Err(PluginError::invalid_params(format!(
            "statement {index} must be a single SQL statement"
        )));
    }
    let Some(verb) = first_top_level_keyword(&stmt.sql) else {
        return Err(PluginError::invalid_params(format!(
            "statement {index} is not valid Bookclerk SQL"
        )));
    };
    if DENIED_VERBS.iter().any(|v| verb.eq_ignore_ascii_case(v)) {
        return Err(PluginError::invalid_params(format!(
            "statement {index} uses disallowed SQL verb {verb}"
        )));
    }
    for table in named_tables(&stmt.sql) {
        if table_denied(&table) {
            return Err(PluginError::invalid_params(format!(
                "statement {index} names unauthorized table {table}"
            )));
        }
    }
    let expected = count_placeholders(&stmt.sql);
    if expected != stmt.parameters.len() {
        return Err(PluginError::invalid_params(format!(
            "statement {index} has {} bind(s) but SQL has {expected} placeholder(s)",
            stmt.parameters.len()
        )));
    }
    let kind = guest_statement_kind(&stmt.sql);
    validate_selection(index, kind, stmt.result_selection, stmt.max_rows)?;
    Ok(())
}

fn validate_selection(
    index: usize,
    kind: DbPlanStatementKind,
    selection: DbResultSelection,
    max_rows: u32,
) -> Result<()> {
    match selection {
        DbResultSelection::Discard => {}
        DbResultSelection::AffectedRows => {
            if !matches!(
                kind,
                DbPlanStatementKind::Execute | DbPlanStatementKind::Returning
            ) {
                return Err(PluginError::invalid_params(format!(
                    "statement {index} resultSelection affectedRows requires DML"
                )));
            }
            if max_rows != 0 {
                return Err(PluginError::invalid_params(format!(
                    "statement {index} affectedRows cannot set maxRows"
                )));
            }
        }
        DbResultSelection::Rows | DbResultSelection::Cursor => {
            if matches!(kind, DbPlanStatementKind::Execute) {
                return Err(PluginError::invalid_params(format!(
                    "statement {index} resultSelection {:?} requires a row-producing statement",
                    selection
                )));
            }
        }
    }
    Ok(())
}

fn table_denied(name: &str) -> bool {
    name.split('.').any(|part| {
        let lower = part
            .trim()
            .trim_matches('"')
            .trim_matches('`')
            .trim_matches('[')
            .trim_matches(']')
            .to_ascii_lowercase();
        DENIED_TABLES.iter().any(|t| *t == lower)
            || lower.starts_with("sqlite_")
            || lower.starts_with("pg_")
    })
}

/// Positional `?` count (or unique `$n` when the SQL has no `?`).
fn count_placeholders(sql: &str) -> usize {
    let mut n_q = 0usize;
    let mut dollars = Vec::new();
    for_each_unquoted(sql, |slice, i| {
        let c = slice.as_bytes()[i];
        if c == b'?' {
            if slice.as_bytes().get(i + 1) == Some(&b'?') {
                return 2;
            }
            n_q += 1;
            let mut j = i + 1;
            while slice.as_bytes().get(j).is_some_and(u8::is_ascii_digit) {
                j += 1;
            }
            return j - i;
        }
        if c == b'$' {
            let mut j = i + 1;
            while slice.as_bytes().get(j).is_some_and(u8::is_ascii_digit) {
                j += 1;
            }
            if j > i + 1 {
                if let Ok(idx) = slice[i + 1..j].parse::<u32>() {
                    if !dollars.contains(&idx) {
                        dollars.push(idx);
                    }
                }
                return j - i;
            }
        }
        1
    });
    if n_q > 0 {
        n_q
    } else {
        dollars.len()
    }
}

fn named_tables(sql: &str) -> Vec<String> {
    let mut tables = Vec::new();
    let bytes = sql.as_bytes();
    let mut i = 0;
    let mut in_s = false;
    let mut in_d = false;
    let mut in_line = false;
    let mut in_block = false;
    while i < bytes.len() {
        let c = bytes[i];
        if in_line {
            if c == b'\n' {
                in_line = false;
            }
            i += 1;
            continue;
        }
        if in_block {
            if c == b'*' && bytes.get(i + 1) == Some(&b'/') {
                in_block = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if in_s {
            if c == b'\'' {
                if bytes.get(i + 1) == Some(&b'\'') {
                    i += 2;
                    continue;
                }
                in_s = false;
            }
            i += 1;
            continue;
        }
        if in_d {
            if c == b'"' {
                if bytes.get(i + 1) == Some(&b'"') {
                    i += 2;
                    continue;
                }
                in_d = false;
            }
            i += 1;
            continue;
        }
        if c == b'-' && bytes.get(i + 1) == Some(&b'-') {
            in_line = true;
            i += 2;
            continue;
        }
        if c == b'/' && bytes.get(i + 1) == Some(&b'*') {
            in_block = true;
            i += 2;
            continue;
        }
        if c == b'\'' {
            in_s = true;
            i += 1;
            continue;
        }
        if c == b'"' {
            in_d = true;
            i += 1;
            continue;
        }
        if ident_start(c) {
            let start = i;
            i += 1;
            while i < bytes.len() && ident_cont(bytes[i]) {
                i += 1;
            }
            let word = sql[start..i].to_ascii_uppercase();
            if matches!(word.as_str(), "FROM" | "JOIN" | "INTO" | "UPDATE" | "TABLE") {
                skip_ws_and_comments(bytes, &mut i);
                if let Some(name) = read_table_name(sql, &mut i) {
                    tables.push(name);
                }
            }
            continue;
        }
        i += 1;
    }
    tables
}

fn skip_ws_and_comments(bytes: &[u8], i: &mut usize) {
    loop {
        while *i < bytes.len() && bytes[*i].is_ascii_whitespace() {
            *i += 1;
        }
        if bytes.get(*i) == Some(&b'-') && bytes.get(*i + 1) == Some(&b'-') {
            *i += 2;
            while *i < bytes.len() && bytes[*i] != b'\n' {
                *i += 1;
            }
            continue;
        }
        if bytes.get(*i) == Some(&b'/') && bytes.get(*i + 1) == Some(&b'*') {
            *i += 2;
            while *i + 1 < bytes.len() && !(bytes[*i] == b'*' && bytes[*i + 1] == b'/') {
                *i += 1;
            }
            *i = (*i + 2).min(bytes.len());
            continue;
        }
        break;
    }
}

fn read_table_name(sql: &str, i: &mut usize) -> Option<String> {
    let bytes = sql.as_bytes();
    skip_ws_and_comments(bytes, i);
    if *i >= bytes.len() {
        return None;
    }
    if bytes[*i] == b'(' {
        return None;
    }
    let start = *i;
    if bytes[*i] == b'"' || bytes[*i] == b'`' {
        let q = bytes[*i];
        *i += 1;
        while *i < bytes.len() && bytes[*i] != q {
            *i += 1;
        }
        if *i < bytes.len() {
            *i += 1;
        }
    } else if ident_start(bytes[*i]) {
        *i += 1;
        while *i < bytes.len() && ident_cont(bytes[*i]) {
            *i += 1;
        }
    } else {
        return None;
    }
    skip_ws_and_comments(bytes, i);
    if bytes.get(*i) == Some(&b'.') {
        *i += 1;
        skip_ws_and_comments(bytes, i);
        if *i < bytes.len() && (ident_start(bytes[*i]) || bytes[*i] == b'"' || bytes[*i] == b'`') {
            if bytes[*i] == b'"' || bytes[*i] == b'`' {
                let q = bytes[*i];
                *i += 1;
                while *i < bytes.len() && bytes[*i] != q {
                    *i += 1;
                }
                if *i < bytes.len() {
                    *i += 1;
                }
            } else {
                *i += 1;
                while *i < bytes.len() && ident_cont(bytes[*i]) {
                    *i += 1;
                }
            }
        }
    }
    let raw = sql[start..*i].trim();
    if raw.is_empty() {
        None
    } else {
        Some(raw.to_string())
    }
}

fn ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

fn ident_cont(c: u8) -> bool {
    ident_start(c) || c.is_ascii_digit()
}

fn first_top_level_keyword(sql: &str) -> Option<String> {
    let mut first = None;
    for_each_top_level_keyword(sql, |_, kw| {
        if first.is_none() {
            first = Some(kw.to_ascii_uppercase());
        }
    });
    first
}

fn has_top_level_keyword(sql: &str, keyword: &str) -> bool {
    let want = keyword.to_ascii_uppercase();
    let mut found = false;
    for_each_top_level_keyword(sql, |_, kw| {
        if kw.eq_ignore_ascii_case(&want) {
            found = true;
        }
    });
    found
}

fn has_top_level_semicolon_tail(sql: &str) -> bool {
    let mut depth = 0usize;
    let mut in_s = false;
    let mut in_d = false;
    let bytes = sql.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if in_s {
            if c == b'\'' {
                if bytes.get(i + 1) == Some(&b'\'') {
                    i += 2;
                    continue;
                }
                in_s = false;
            }
            i += 1;
            continue;
        }
        if in_d {
            if c == b'"' {
                if bytes.get(i + 1) == Some(&b'"') {
                    i += 2;
                    continue;
                }
                in_d = false;
            }
            i += 1;
            continue;
        }
        match c {
            b'\'' => in_s = true,
            b'"' => in_d = true,
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b';' if depth == 0 => {
                let rest = sql[i + 1..].trim();
                return !rest.is_empty();
            }
            _ => {}
        }
        i += 1;
    }
    false
}

fn for_each_top_level_keyword(sql: &str, mut on_keyword: impl FnMut(usize, &str)) {
    let bytes = sql.as_bytes();
    let mut depth = 0usize;
    for_each_unquoted(sql, |slice, i| {
        let c = slice.as_bytes()[i];
        if c == b'(' {
            depth += 1;
            return 1;
        }
        if c == b')' {
            depth = depth.saturating_sub(1);
            return 1;
        }
        if depth == 0 && ident_start(c) {
            let mut j = i + 1;
            while j < slice.len() && ident_cont(bytes[j]) {
                j += 1;
            }
            on_keyword(i, &slice[i..j]);
            return j - i;
        }
        1
    });
}

/// Walks `sql`, skipping quoted strings and comments. `step` is given the
/// current index in the original string and returns how many bytes to consume
/// (at least 1). Parentheses are visible so placeholders inside subqueries
/// are counted.
fn for_each_unquoted(sql: &str, mut step: impl FnMut(&str, usize) -> usize) {
    let bytes = sql.as_bytes();
    let mut i = 0;
    let mut in_s = false;
    let mut in_d = false;
    let mut in_line = false;
    let mut in_block = false;
    while i < bytes.len() {
        let c = bytes[i];
        if in_line {
            if c == b'\n' {
                in_line = false;
            }
            i += 1;
            continue;
        }
        if in_block {
            if c == b'*' && bytes.get(i + 1) == Some(&b'/') {
                in_block = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if in_s {
            if c == b'\'' {
                if bytes.get(i + 1) == Some(&b'\'') {
                    i += 2;
                    continue;
                }
                in_s = false;
            }
            i += 1;
            continue;
        }
        if in_d {
            if c == b'"' {
                if bytes.get(i + 1) == Some(&b'"') {
                    i += 2;
                    continue;
                }
                in_d = false;
            }
            i += 1;
            continue;
        }
        if c == b'-' && bytes.get(i + 1) == Some(&b'-') {
            in_line = true;
            i += 2;
            continue;
        }
        if c == b'/' && bytes.get(i + 1) == Some(&b'*') {
            in_block = true;
            i += 2;
            continue;
        }
        if c == b'\'' {
            in_s = true;
            i += 1;
            continue;
        }
        if c == b'"' {
            in_d = true;
            i += 1;
            continue;
        }
        let n = step(sql, i).max(1);
        i += n;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DbValue, ExecuteRequest};

    fn req(
        sql: &str,
        params: Vec<DbValue>,
        selection: DbResultSelection,
        max_rows: u32,
    ) -> ExecuteRequest {
        ExecuteRequest {
            operation_id: "op".into(),
            request_hash: String::new(),
            statements: vec![TypedDbStatement {
                sql: sql.into(),
                parameters: params,
                kind: DbPlanStatementKind::Query,
                max_rows,
                result_selection: selection,
            }],
            outcome_index: 0,
            payload_index: 0,
            has_payload_index: false,
            prior_receipt_index: 0,
            has_prior_receipt_index: false,
            receipt_select_index: 0,
            has_receipt_select_index: false,
            deadline_unix_ms: 0,
        }
    }

    #[test]
    fn allows_select_and_insert() {
        validate_guest_execute_request(&req(
            "SELECT id FROM books WHERE id = ?",
            vec![DbValue::Int64(1)],
            DbResultSelection::Rows,
            1,
        ))
        .unwrap();
        validate_guest_execute_request(&req(
            "INSERT INTO books (id) VALUES (?)",
            vec![DbValue::Int64(1)],
            DbResultSelection::AffectedRows,
            0,
        ))
        .unwrap();
    }

    #[test]
    fn rejects_ddl_and_pragma() {
        let err = validate_guest_execute_request(&req(
            "CREATE TABLE t (id INTEGER)",
            vec![],
            DbResultSelection::AffectedRows,
            0,
        ))
        .unwrap_err();
        assert!(err.to_string().contains("disallowed"), "{err}");
        let err = validate_guest_execute_request(&req(
            "PRAGMA user_version",
            vec![],
            DbResultSelection::Rows,
            0,
        ))
        .unwrap_err();
        assert!(err.to_string().contains("disallowed"), "{err}");
    }

    #[test]
    fn rejects_encrypted_secrets_and_sqlite_catalog() {
        let err = validate_guest_execute_request(&req(
            "SELECT * FROM encrypted_secrets",
            vec![],
            DbResultSelection::Rows,
            0,
        ))
        .unwrap_err();
        assert!(err.to_string().contains("unauthorized"), "{err}");
        let err = validate_guest_execute_request(&req(
            "SELECT * FROM sqlite_master",
            vec![],
            DbResultSelection::Rows,
            0,
        ))
        .unwrap_err();
        assert!(err.to_string().contains("unauthorized"), "{err}");
        let err = validate_guest_execute_request(&req(
            r#"SELECT * FROM "pg_catalog"."pg_class""#,
            vec![],
            DbResultSelection::Rows,
            0,
        ))
        .unwrap_err();
        assert!(err.to_string().contains("unauthorized"), "{err}");
    }

    #[test]
    fn rejects_bind_count_mismatch() {
        let err = validate_guest_execute_request(&req(
            "SELECT ? FROM books WHERE id = ?",
            vec![DbValue::Int64(1)],
            DbResultSelection::Rows,
            1,
        ))
        .unwrap_err();
        assert!(err.to_string().contains("placeholder"), "{err}");
    }

    #[test]
    fn rejects_rows_on_plain_dml() {
        let err = validate_guest_execute_request(&req(
            "INSERT INTO books (id) VALUES (?)",
            vec![DbValue::Int64(1)],
            DbResultSelection::Rows,
            1,
        ))
        .unwrap_err();
        assert!(err.to_string().contains("row-producing"), "{err}");
    }

    #[test]
    fn ignores_placeholders_inside_strings() {
        validate_guest_execute_request(&req(
            "SELECT '?' FROM books WHERE title = ?",
            vec![DbValue::Text("x".into())],
            DbResultSelection::Rows,
            1,
        ))
        .unwrap();
    }

    #[test]
    fn allows_replace_upsert() {
        validate_guest_execute_request(&req(
            "REPLACE INTO books (id) VALUES (?)",
            vec![DbValue::Int64(1)],
            DbResultSelection::AffectedRows,
            0,
        ))
        .unwrap();
    }

    #[test]
    fn rejects_information_schema_and_nested_catalog() {
        let err = validate_guest_execute_request(&req(
            "SELECT * FROM information_schema.tables",
            vec![],
            DbResultSelection::Rows,
            0,
        ))
        .unwrap_err();
        assert!(err.to_string().contains("unauthorized"), "{err}");
        let err = validate_guest_execute_request(&req(
            "SELECT * FROM (SELECT * FROM encrypted_secrets)",
            vec![],
            DbResultSelection::Rows,
            0,
        ))
        .unwrap_err();
        assert!(err.to_string().contains("unauthorized"), "{err}");
    }

    #[test]
    fn rejects_second_statement_and_affected_rows_max_rows() {
        let err = validate_guest_execute_request(&req(
            "SELECT 1; DROP TABLE books",
            vec![],
            DbResultSelection::Rows,
            1,
        ))
        .unwrap_err();
        assert!(err.to_string().contains("single SQL"), "{err}");
        let err = validate_guest_execute_request(&req(
            "INSERT INTO books (id) VALUES (?)",
            vec![DbValue::Int64(1)],
            DbResultSelection::AffectedRows,
            4,
        ))
        .unwrap_err();
        assert!(
            err.to_string().contains("affectedRows cannot set maxRows"),
            "{err}"
        );
    }
}
