//! Portable BookclerkSQL text domain and statement-boundary helpers.
//!
//! BookclerkSQL `TEXT` is Unicode scalar values representable on the UTF-8
//! wire, excluding U+0000. UTF-8 is the encoding, not a smaller repertoire.
//! `BYTES` / `BLOB` may contain zero bytes.

use sha2::{Digest, Sha256};

use crate::{PluginError, Result};

/// True when `text` contains U+0000.
#[must_use]
pub fn text_contains_nul(text: &str) -> bool {
    text.as_bytes().contains(&0)
}

/// Rejects BookclerkSQL `TEXT` that contains U+0000.
///
/// # Errors
///
/// Returns [`PluginError::invalid_params`] when `text` contains NUL.
pub fn require_portable_text(text: &str) -> Result<()> {
    if text_contains_nul(text) {
        Err(PluginError::invalid_params(
            "BookclerkSQL TEXT cannot contain U+0000 (use BYTES/BLOB for binary)",
        ))
    } else {
        Ok(())
    }
}

/// Rejects U+0000 in every BookclerkSQL `TEXT` bind.
///
/// # Errors
///
/// Returns [`PluginError::invalid_params`] when a [`crate::DbValue::Text`] bind
/// contains NUL.
pub fn require_portable_text_binds(parameters: &[crate::DbValue]) -> Result<()> {
    for (i, param) in parameters.iter().enumerate() {
        if let crate::DbValue::Text(text) = param {
            require_portable_text(text)
                .map_err(|err| PluginError::invalid_params(format!("parameter {i}: {err}")))?;
        }
    }
    Ok(())
}

/// UTF-8 byte length of a sqlite-family GLOB pattern after Bookclerk LIKE→GLOB
/// metacharacter escaping (`[` `*` `?` become three bytes).
///
/// D1's physical GLOB/LIKE pattern cap is 50 bytes. Adapters that perform this
/// expansion must advertise [`super::DbCapabilities::max_pattern_bytes`] small
/// enough that the expanded pattern still fits.
#[must_use]
pub fn glob_expanded_like_pattern_bytes(like_pattern: &str) -> usize {
    let mut n = 0usize;
    for c in like_pattern.chars() {
        n = n.saturating_add(match c {
            '[' | '*' | '?' => 3,
            _ => c.len_utf8(),
        });
    }
    n
}

/// Conservative portable LIKE pattern cap for D1 while LIKE lowers to GLOB.
///
/// Cloudflare D1 allows 50 bytes in a LIKE/GLOB pattern. Worst-case expansion
/// of `[` / `*` / `?` is 3×, so `floor(50/3) = 16`.
pub const D1_PORTABLE_LIKE_PATTERN_BYTES: u32 = 16;

/// Physical D1 LIKE/GLOB pattern cap in bytes.
pub const D1_PHYSICAL_LIKE_GLOB_PATTERN_BYTES: u32 = 50;

/// Length-prefixed SHA-256 of an ordered canonical statement list.
///
/// Statement boundaries are part of the digest: joining with `;` then hashing
/// would collide with literals such as `DEFAULT 'a;b'`.
#[must_use]
pub fn canonical_statements_checksum(statements: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"bookclerk-sql-stmts-v1\0");
    for sql in statements {
        let len = u64::try_from(sql.len()).unwrap_or(u64::MAX);
        hasher.update(len.to_le_bytes());
        hasher.update(sql.as_bytes());
    }
    hex::encode(hasher.finalize())
}

/// Splits a BookclerkSQL pack into statements using the SQL-v1 lexer.
///
/// Semicolons inside string literals, quoted identifiers, line comments, and
/// block comments do not end a statement. Empty fragments are dropped.
///
/// # Errors
///
/// Returns [`PluginError::invalid_params`] when quotes or comments are
/// unterminated, or when a fragment is not a single SQL-v1 statement.
pub fn sql_v1_pack_statements(sql: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < sql.len() {
        i = skip_trivia(sql, i);
        if i >= sql.len() {
            break;
        }
        let start = i;
        let (end, saw_semi) = statement_span(sql, start)?;
        let raw = sql.get(start..end).unwrap_or("");
        let stmt = raw.trim();
        if !stmt.is_empty() {
            require_portable_text(stmt)?;
            out.push(stmt.to_string());
        }
        i = end;
        if saw_semi {
            i += 1;
        }
    }
    Ok(out)
}

/// Walks one statement starting at `start` (trivia already skipped).
///
/// # Errors
///
/// Returns [`PluginError::invalid_params`] when a string or quoted identifier
/// is unterminated.
fn statement_span(sql: &str, start: usize) -> Result<(usize, bool)> {
    let bytes = sql.as_bytes();
    let mut i = start;
    let mut depth = 0usize;
    while i < bytes.len() {
        i = skip_trivia(sql, i);
        if i >= bytes.len() {
            break;
        }
        let c = bytes[i];
        match c {
            b'\'' => {
                i = skip_sql_string(sql, i)?;
            }
            b'"' | b'`' => {
                i = skip_quoted(sql, i, c)?;
            }
            b'[' => {
                i = skip_quoted(sql, i, b']')?;
            }
            b'(' => {
                depth = depth.saturating_add(1);
                i += 1;
            }
            b')' => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            b';' if depth == 0 => return Ok((i, true)),
            _ => {
                i += 1;
            }
        }
    }
    Ok((sql.len(), false))
}

/// Skips whitespace, `--` line comments, and `/* */` block comments.
fn skip_trivia(sql: &str, mut i: usize) -> usize {
    let bytes = sql.as_bytes();
    loop {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if bytes.get(i) == Some(&b'-') && bytes.get(i + 1) == Some(&b'-') {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes.get(i) == Some(&b'/') && bytes.get(i + 1) == Some(&b'*') {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = i.saturating_add(2).min(bytes.len());
            continue;
        }
        break;
    }
    i
}

/// Skips a `'…'` SQL string, honoring doubled quotes.
///
/// # Errors
///
/// Returns [`PluginError::invalid_params`] when `start` is not `'`, or the
/// string is unterminated.
fn skip_sql_string(sql: &str, start: usize) -> Result<usize> {
    let bytes = sql.as_bytes();
    if bytes.get(start) != Some(&b'\'') {
        return Err(PluginError::invalid_params("expected SQL string"));
    }
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == b'\'' {
            if bytes.get(i + 1) == Some(&b'\'') {
                i += 2;
                continue;
            }
            return Ok(i + 1);
        }
        i += 1;
    }
    Err(PluginError::invalid_params(
        "unterminated SQL string in canonical pack",
    ))
}

/// Skips a quoted identifier delimited by `end` (doubled for `"` / `` ` ``).
///
/// # Errors
///
/// Returns [`PluginError::invalid_params`] when the quoted identifier is
/// unterminated.
fn skip_quoted(sql: &str, start: usize, end: u8) -> Result<usize> {
    let bytes = sql.as_bytes();
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == end {
            if end != b']' && bytes.get(i + 1) == Some(&end) {
                i += 2;
                continue;
            }
            return Ok(i + 1);
        }
        i += 1;
    }
    Err(PluginError::invalid_params(
        "unterminated quoted identifier in canonical pack",
    ))
}

/// Proven LIKE pattern sources in one canonical statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LikePatternSrc {
    /// `'literal'` (possibly concatenated with `||`).
    Literal(String),
    /// Positional `?` bind (0-based bind index in this statement).
    Bind(usize),
    /// Pattern expression whose length cannot be proven.
    Unproven,
    /// SQL `NULL` (no pattern bytes).
    Null,
}

/// Collects LIKE / NOT LIKE pattern sources with proven lengths when possible.
#[must_use]
pub fn like_pattern_sources(sql: &str) -> Vec<LikePatternSrc> {
    let mut out = Vec::new();
    let mut i = 0usize;
    let mut bind_i = 0usize;
    let bytes = sql.as_bytes();
    while i < bytes.len() {
        i = skip_trivia(sql, i);
        if i >= bytes.len() {
            break;
        }
        if bytes[i] == b'\'' {
            if let Ok(end) = skip_sql_string(sql, i) {
                i = end;
                continue;
            }
            break;
        }
        if bytes[i] == b'?' {
            bind_i += 1;
            i += 1;
            continue;
        }
        if keyword_at(sql, i, "LIKE") {
            let after = skip_trivia(sql, i + 4);
            out.push(parse_like_pattern(sql, after, bind_i).0);
            i = after;
            continue;
        }
        i += 1;
    }
    out
}

/// Parses the LIKE pattern starting at `start`.
fn parse_like_pattern(sql: &str, start: usize, binds_before: usize) -> (LikePatternSrc, usize) {
    let mut i = skip_trivia(sql, start);
    let bytes = sql.as_bytes();
    let mut literal = String::new();
    let mut saw_literal = false;
    let mut bind = None;
    loop {
        i = skip_trivia(sql, i);
        if i >= bytes.len() {
            break;
        }
        if bytes[i] == b'\'' {
            let Ok(end) = skip_sql_string(sql, i) else {
                return (LikePatternSrc::Unproven, i);
            };
            let inner = unquote_sql_string(&sql[i..end]);
            literal.push_str(&inner);
            saw_literal = true;
            i = skip_trivia(sql, end);
            if sql.get(i..).is_some_and(|s| s.starts_with("||")) {
                i += 2;
                continue;
            }
            break;
        }
        if bytes[i] == b'?' {
            bind = Some(binds_before);
            i += 1;
            break;
        }
        if keyword_at(sql, i, "NULL") {
            if saw_literal || bind.is_some() {
                return (LikePatternSrc::Unproven, i);
            }
            return (LikePatternSrc::Null, i + 4);
        }
        return (LikePatternSrc::Unproven, i);
    }
    if let Some(b) = bind {
        if saw_literal {
            return (LikePatternSrc::Unproven, i);
        }
        return (LikePatternSrc::Bind(b), i);
    }
    if saw_literal {
        (LikePatternSrc::Literal(literal), i)
    } else {
        (LikePatternSrc::Unproven, i)
    }
}

/// Unquotes a `'…'` SQL string (doubled quotes become one quote).
fn unquote_sql_string(quoted: &str) -> String {
    let bytes = quoted.as_bytes();
    if bytes.first() != Some(&b'\'') || bytes.last() != Some(&b'\'') || bytes.len() < 2 {
        return String::new();
    }
    let inner = &quoted[1..quoted.len() - 1];
    inner.replace("''", "'")
}

/// True when `sql[i..]` is keyword `kw` with identifier boundaries.
fn keyword_at(sql: &str, i: usize, kw: &str) -> bool {
    if !sql.is_char_boundary(i) {
        return false;
    }
    let n = kw.len();
    let Some(end) = i.checked_add(n) else {
        return false;
    };
    if end > sql.len() || !sql.is_char_boundary(end) {
        return false;
    }
    if !sql[i..end].eq_ignore_ascii_case(kw) {
        return false;
    }
    let bytes = sql.as_bytes();
    let before_ok = i == 0 || !is_ident_cont(bytes[i - 1]);
    let after = bytes.get(end).copied().unwrap_or(b' ');
    before_ok && !is_ident_cont(after)
}

/// True when `c` may continue an unquoted identifier.
const fn is_ident_cont(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// One helper call found in canonical SQL (code spans only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlFnCall {
    /// Folded helper name (`json_object`, `replace`, …).
    pub name: String,
    /// Top-level argument count (`json_object('k', 1)` is 2).
    pub arg_count: usize,
}

/// Helpers adapters may nest so each physical call stays inside
/// [`crate::DbCapabilities::max_function_args`].
#[must_use]
pub fn sql_v1_helper_is_chunkable(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "json_object" | "min" | "max" | "coalesce"
    )
}

/// Collects helper calls in code spans (strings/comments are skipped).
#[must_use]
pub fn sql_v1_function_calls(sql: &str) -> Vec<SqlFnCall> {
    let mut out = Vec::new();
    let mut i = 0usize;
    let bytes = sql.as_bytes();
    while i < bytes.len() {
        i = skip_trivia(sql, i);
        if i >= bytes.len() {
            break;
        }
        match bytes[i] {
            b'\'' => {
                i = skip_sql_string(sql, i).unwrap_or(i + 1);
                continue;
            }
            b'"' | b'`' => {
                i = skip_quoted(sql, i, bytes[i]).unwrap_or(i + 1);
                continue;
            }
            b'[' => {
                i = skip_quoted(sql, i, b']').unwrap_or(i + 1);
                continue;
            }
            c if is_ident_start(c) => {
                let start = i;
                i += 1;
                while i < bytes.len() && is_ident_cont(bytes[i]) {
                    i += 1;
                }
                let name = sql[start..i].to_ascii_lowercase();
                let j = skip_trivia(sql, i);
                if bytes.get(j) == Some(&b'(') && !matches!(name.as_str(), "cast" | "case") {
                    if let Some((n, end)) = count_call_args(sql, j) {
                        out.push(SqlFnCall { name, arg_count: n });
                        i = end;
                        continue;
                    }
                }
            }
            _ => i += 1,
        }
    }
    out
}

/// True when `c` may start an unquoted identifier.
fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

/// Counts top-level arguments of a call whose `(` is at `open`.
fn count_call_args(sql: &str, open: usize) -> Option<(usize, usize)> {
    let bytes = sql.as_bytes();
    if bytes.get(open) != Some(&b'(') {
        return None;
    }
    let mut i = open + 1;
    let mut depth = 0usize;
    let mut n = 0usize;
    let mut saw_atom = false;
    while i < bytes.len() {
        i = skip_trivia(sql, i);
        if i >= bytes.len() {
            return None;
        }
        match bytes[i] {
            b'\'' => {
                i = skip_sql_string(sql, i).ok()?;
                saw_atom = true;
            }
            b'"' | b'`' => {
                i = skip_quoted(sql, i, bytes[i]).ok()?;
                saw_atom = true;
            }
            b'[' => {
                i = skip_quoted(sql, i, b']').ok()?;
                saw_atom = true;
            }
            b'(' => {
                depth = depth.saturating_add(1);
                saw_atom = true;
                i += 1;
            }
            b')' if depth == 0 => {
                if saw_atom {
                    n = n.saturating_add(1);
                }
                return Some((n, i + 1));
            }
            b')' => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            b',' if depth == 0 => {
                n = n.saturating_add(1);
                saw_atom = false;
                i += 1;
            }
            _ => {
                saw_atom = true;
                i += 1;
            }
        }
    }
    None
}

/// Enforces [`crate::DbCapabilities::max_function_args`] on non-chunkable helpers.
///
/// `json_object` / `min` / `max` / `coalesce` are omitted: adapters nest them
/// so each physical call stays inside the advertised cap.
///
/// # Errors
///
/// Returns [`PluginError::invalid_params`] when a non-chunkable call exceeds
/// the cap or the cap is unspecified.
pub fn require_function_args_within(sql: &str, max_function_args: u32) -> Result<()> {
    if max_function_args == 0 {
        return Err(PluginError::invalid_params(
            "database guest maxFunctionArgs is unspecified",
        ));
    }
    let cap = usize::try_from(max_function_args).unwrap_or(usize::MAX);
    for call in sql_v1_function_calls(sql) {
        if sql_v1_helper_is_chunkable(&call.name) {
            continue;
        }
        if call.arg_count > cap {
            return Err(PluginError::invalid_params(format!(
                "function {} has {} arguments; guest maxFunctionArgs is {max_function_args}",
                call.name, call.arg_count
            )));
        }
    }
    Ok(())
}

/// Enforces [`crate::DbCapabilities::max_pattern_bytes`] on LIKE patterns.
///
/// # Errors
///
/// Returns [`PluginError::invalid_params`] when a proven pattern exceeds the
/// cap or a LIKE pattern length cannot be proven.
pub fn require_like_patterns_within(
    sql: &str,
    parameters: &[crate::DbValue],
    max_pattern_bytes: u32,
) -> Result<()> {
    if max_pattern_bytes == 0 {
        return Err(PluginError::invalid_params(
            "database guest maxPatternBytes is unspecified",
        ));
    }
    let cap = usize::try_from(max_pattern_bytes).unwrap_or(usize::MAX);
    for src in like_pattern_sources(sql) {
        match src {
            LikePatternSrc::Literal(pat) => {
                require_portable_text(&pat)?;
                if pat.len() > cap {
                    return Err(PluginError::invalid_params(format!(
                        "LIKE pattern is {} bytes; guest maxPatternBytes is {max_pattern_bytes}",
                        pat.len()
                    )));
                }
            }
            LikePatternSrc::Bind(idx) => match parameters.get(idx) {
                Some(crate::DbValue::Text(pat)) => {
                    require_portable_text(pat)?;
                    if pat.len() > cap {
                        return Err(PluginError::invalid_params(format!(
                            "LIKE bind {idx} is {} bytes; guest maxPatternBytes is {max_pattern_bytes}",
                            pat.len()
                        )));
                    }
                }
                Some(crate::DbValue::Null(_)) => {}
                Some(_) => {
                    return Err(PluginError::invalid_params(format!(
                        "LIKE bind {idx} must be TEXT"
                    )));
                }
                None => {
                    return Err(PluginError::invalid_params(format!(
                        "LIKE bind {idx} is missing"
                    )));
                }
            },
            LikePatternSrc::Null => {}
            LikePatternSrc::Unproven => {
                return Err(PluginError::invalid_params(
                    "LIKE pattern length is not proven to fit maxPatternBytes",
                ));
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
    fn pack_statements_preserve_semicolons_in_literals_and_comments() {
        let sql = r#"
            CREATE TABLE t (
                x TEXT NOT NULL DEFAULT 'a;b',
                y TEXT CHECK (x <> ';')
            );
            -- trailing; comment
            /* block ; comment */
            CREATE INDEX idx ON t (x);
        "#;
        let stmts = sql_v1_pack_statements(sql).expect("pack");
        assert_eq!(stmts.len(), 2, "{stmts:?}");
        assert!(stmts[0].contains("DEFAULT 'a;b'"), "{}", stmts[0]);
        assert!(stmts[0].contains("CHECK (x <> ';')"), "{}", stmts[0]);
        assert!(stmts[1].to_ascii_lowercase().contains("create index"));
        let again = sql_v1_pack_statements(&stmts.join(";\n")).expect("repack");
        assert_eq!(again, stmts);
        assert_eq!(
            canonical_statements_checksum(&stmts.iter().map(String::as_str).collect::<Vec<_>>()),
            canonical_statements_checksum(&again.iter().map(String::as_str).collect::<Vec<_>>())
        );
    }

    #[test]
    fn escaped_quotes_around_semicolons() {
        let sql = "CREATE TABLE t (x TEXT DEFAULT 'a'';b')";
        let stmts = sql_v1_pack_statements(sql).expect("pack");
        assert_eq!(stmts.len(), 1);
        assert!(stmts[0].contains("''"));
    }

    #[test]
    fn nul_is_rejected_in_text() {
        assert!(require_portable_text("ok").is_ok());
        assert!(require_portable_text("a\0b").is_err());
        assert!(!text_contains_nul("🎵"));
    }

    #[test]
    fn glob_expansion_is_three_for_metacharacters() {
        assert_eq!(glob_expanded_like_pattern_bytes("abc"), 3);
        assert_eq!(glob_expanded_like_pattern_bytes("["), 3);
        assert_eq!(
            glob_expanded_like_pattern_bytes(&"[".repeat(D1_PORTABLE_LIKE_PATTERN_BYTES as usize)),
            (D1_PORTABLE_LIKE_PATTERN_BYTES as usize) * 3
        );
        assert!(
            glob_expanded_like_pattern_bytes(&"[".repeat(D1_PORTABLE_LIKE_PATTERN_BYTES as usize))
                <= D1_PHYSICAL_LIKE_GLOB_PATTERN_BYTES as usize
        );
    }

    #[test]
    fn like_literal_and_bind_sources() {
        let srcs = like_pattern_sources("SELECT * FROM t WHERE a LIKE 'ab%' AND b LIKE ?");
        assert_eq!(
            srcs,
            vec![
                LikePatternSrc::Literal("ab%".into()),
                LikePatternSrc::Bind(0)
            ]
        );
    }

    #[test]
    fn like_null_is_proven_and_fits_any_pattern_cap() {
        let srcs = like_pattern_sources(
            "SELECT CASE WHEN 'A' LIKE NULL THEN 1 ELSE 0 END AS c7, \
             CASE WHEN body LIKE ? THEN 1 ELSE 0 END AS c8 FROM liked",
        );
        assert_eq!(
            srcs,
            vec![LikePatternSrc::Null, LikePatternSrc::Bind(0)],
            "{srcs:?}"
        );
        require_like_patterns_within(
            "SELECT CASE WHEN 'A' LIKE NULL THEN 1 ELSE 0 END AS c0 FROM t",
            &[],
            D1_PORTABLE_LIKE_PATTERN_BYTES,
        )
        .expect("LIKE NULL has no pattern bytes");
        require_like_patterns_within(
            "SELECT CASE WHEN 'A' LIKE 'a' THEN 1 ELSE 0 END AS c0, \
             CASE WHEN 'A' LIKE NULL THEN 1 ELSE 0 END AS c7, \
             CASE WHEN body LIKE ? THEN 1 ELSE 0 END AS c8 FROM liked",
            &[crate::DbValue::Text("A".into())],
            D1_PORTABLE_LIKE_PATTERN_BYTES,
        )
        .expect("portable LIKE vector patterns");
    }

    #[test]
    fn portable_text_accepts_unicode_and_rejects_nul() {
        for sample in ["", "ascii", "café", "a\u{0301}", "🎵", "\u{1F980}", "שלום"] {
            require_portable_text(sample).expect(sample);
        }
        assert!(require_portable_text("a\0b").is_err());
        require_portable_text_binds(&[crate::DbValue::Bytes(vec![0])]).expect("BLOB NUL");
        assert!(require_portable_text_binds(&[crate::DbValue::Text("a\0".into())]).is_err());
    }

    #[test]
    fn checksum_distinguishes_semicolon_in_literal() {
        let a = canonical_statements_checksum(&["CREATE TABLE t (x TEXT DEFAULT 'a;b')"]);
        let b = canonical_statements_checksum(&["CREATE TABLE t (x TEXT DEFAULT 'a", "b')"]);
        assert_ne!(a, b);
    }

    #[test]
    fn function_call_arity_and_chunkable_helpers() {
        let calls = sql_v1_function_calls(
            "SELECT replace(a, 'x', 'y'), json_object('k', 1, 'm', 2), min(1, 2, 3) FROM t",
        );
        assert!(
            calls
                .iter()
                .any(|c| c.name == "replace" && c.arg_count == 3),
            "{calls:?}"
        );
        assert!(
            calls
                .iter()
                .any(|c| c.name == "json_object" && c.arg_count == 4),
            "{calls:?}"
        );
        require_function_args_within("SELECT replace(a, 'x', 'y')", 3).expect("N");
        let err = require_function_args_within("SELECT replace(a, 'x', 'y')", 2).unwrap_err();
        assert!(err.to_string().contains("maxFunctionArgs"), "{err}");
        require_function_args_within("SELECT json_object('a', 1, 'b', 2, 'c', 3, 'd', 4)", 2)
            .expect("json_object is adapter-chunked");
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 64,
            ..ProptestConfig::default()
        })]

        #[test]
        fn pack_never_panics_on_arbitrary_text(s in r"[\x01-\x7f]{0,200}") {
            let _ = sql_v1_pack_statements(&s);
        }

        #[test]
        fn semicolon_inside_string_stays_one_statement(inner in r"[^\x00']{0,40}") {
            let sql = format!("SELECT '{inner};still'");
            let stmts = sql_v1_pack_statements(&sql).expect("quoted semicolon");
            prop_assert_eq!(stmts.len(), 1);
        }
    }
}
