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

/// Host-issued scope for one granted database binding.
///
/// Empty [`Self::deny_all`] denies every table. Catalog identifiers
/// (`encrypted_secrets`, `sqlite_*`, `pg_*`, `information_schema`) are always
/// refused even if listed. [`Self::host_authoritative`] skips this layer so a
/// Cap'n `DatabaseSession` can be the single authorization authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestSqlPolicy {
    tables: std::collections::BTreeSet<String>,
    columns: std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
    functions: std::collections::BTreeSet<String>,
    /// When true, table/column/function checks are deferred to the host session.
    host_authoritative: bool,
}

impl Default for GuestSqlPolicy {
    fn default() -> Self {
        Self::deny_all()
    }
}

impl GuestSqlPolicy {
    /// No tables, columns, or functions.
    #[must_use]
    pub fn deny_all() -> Self {
        Self {
            tables: std::collections::BTreeSet::new(),
            columns: std::collections::BTreeMap::new(),
            functions: std::collections::BTreeSet::new(),
            host_authoritative: false,
        }
    }

    /// Broker passthrough: grammar/size checks still run; table scope is the
    /// host `DatabaseSession`'s responsibility.
    #[must_use]
    pub fn host_authoritative() -> Self {
        Self {
            tables: std::collections::BTreeSet::new(),
            columns: std::collections::BTreeMap::new(),
            functions: std::collections::BTreeSet::new(),
            host_authoritative: true,
        }
    }

    /// Allows `tables` with builtin scalar functions and any column on those tables.
    #[must_use]
    pub fn allow_tables(tables: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        Self {
            tables: tables
                .into_iter()
                .map(|t| normalize_ident(t.as_ref()))
                .filter(|t| !t.is_empty())
                .collect(),
            columns: std::collections::BTreeMap::new(),
            functions: builtin_functions(),
            host_authoritative: false,
        }
    }

    /// Restricts `table` to `cols` (must already be an allowed table).
    #[must_use]
    pub fn restrict_columns(
        mut self,
        table: &str,
        cols: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Self {
        self.columns.insert(
            normalize_ident(table),
            cols.into_iter()
                .map(|c| normalize_ident(c.as_ref()))
                .filter(|c| !c.is_empty())
                .collect(),
        );
        self
    }

    /// Authorizes parsed refs against this allowlist.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::invalid_params`] when a table, column, or
    /// function is outside this policy.
    fn authorize(&self, index: usize, refs: &GuestSqlRefs) -> Result<()> {
        if self.host_authoritative {
            return Ok(());
        }
        for table in &refs.tables {
            if is_cte_name(refs, table) {
                continue;
            }
            self.authorize_table(index, table)?;
        }
        for (table, column) in &refs.columns {
            if column == "*" {
                match table {
                    Some(table) => {
                        if !is_cte_name(refs, table) && self.columns.contains_key(table) {
                            return Err(PluginError::invalid_params(format!(
                                "statement {index} SELECT * is not allowed on column-restricted table {table}"
                            )));
                        }
                    }
                    None => {
                        for table in &refs.tables {
                            if self.columns.contains_key(table) {
                                return Err(PluginError::invalid_params(format!(
                                    "statement {index} SELECT * is not allowed on column-restricted table {table}"
                                )));
                            }
                        }
                    }
                }
                continue;
            }
            let table = match table {
                Some(table) if is_cte_name(refs, table) => continue,
                Some(table) => table.clone(),
                None => {
                    if refs.tables.len() == 1 {
                        refs.tables[0].clone()
                    } else if refs.tables.is_empty() && !refs.ctes.is_empty() {
                        continue;
                    } else {
                        return Err(PluginError::invalid_params(format!(
                            "statement {index} unqualified column {column} cannot be resolved"
                        )));
                    }
                }
            };
            self.authorize_table(index, &table)?;
            if let Some(allowed) = self.columns.get(&table) {
                if !allowed.contains(column) {
                    return Err(PluginError::invalid_params(format!(
                        "statement {index} names unauthorized column {table}.{column}"
                    )));
                }
            }
        }
        for func in &refs.functions {
            if !self.functions.contains(func) {
                return Err(PluginError::invalid_params(format!(
                    "statement {index} names unauthorized function {func}"
                )));
            }
        }
        Ok(())
    }

    /// Authorizes one table name.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::invalid_params`] when `table` is catalog-denied
    /// or not in this policy.
    fn authorize_table(&self, index: usize, table: &str) -> Result<()> {
        if table_denied(table) || !self.tables.contains(&normalize_ident(table)) {
            return Err(PluginError::invalid_params(format!(
                "statement {index} names unauthorized table {table}"
            )));
        }
        Ok(())
    }
}

fn builtin_functions() -> std::collections::BTreeSet<String> {
    [
        "abs",
        "avg",
        "cast",
        "coalesce",
        "count",
        "date",
        "datetime",
        "group_concat",
        "hex",
        "ifnull",
        "iif",
        "instr",
        "json_array",
        "json_extract",
        "json_object",
        "json_valid",
        "length",
        "lower",
        "max",
        "min",
        "nullif",
        "quote",
        "replace",
        "round",
        "strftime",
        "substr",
        "sum",
        "time",
        "total",
        "trim",
        "typeof",
        "upper",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

/// Tables, columns, and functions referenced by guest SQL.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GuestSqlRefs {
    /// FROM/JOIN/INTO/UPDATE/TABLE names.
    pub tables: Vec<String>,
    /// `(table, column)` pairs; `table` is `None` when unqualified.
    pub columns: Vec<(Option<String>, String)>,
    /// Function names (`ident(`).
    pub functions: Vec<String>,
    /// CTE names in scope (not physical tables).
    pub ctes: Vec<String>,
}

/// Authorizes parsed table/column/function refs against a host-issued policy.
///
/// # Errors
///
/// Returns [`PluginError::invalid_params`] when a referenced object is outside
/// `policy`.
pub fn authorize_guest_sql_policy(req: &ExecuteRequest, policy: &GuestSqlPolicy) -> Result<()> {
    for (i, stmt) in req.statements.iter().enumerate() {
        policy.authorize(i, &parse_guest_sql_refs(&stmt.sql)?)?;
    }
    Ok(())
}

/// Classifies guest SQL the same way the host stamps `DbStatement.kind`.
///
/// Leading `WITH` / `WITH RECURSIVE` CTE lists are skipped so the **main**
/// statement decides the kind: `WITH … SELECT` is [`DbPlanStatementKind::Select`],
/// `WITH … INSERT/UPDATE/DELETE` is [`DbPlanStatementKind::Execute`], and a
/// top-level `RETURNING` on that main statement is [`DbPlanStatementKind::Returning`].
#[must_use]
pub fn guest_statement_kind(sql: &str) -> DbPlanStatementKind {
    let main = sql_after_leading_ctes(sql);
    if has_top_level_keyword(main, "RETURNING") {
        return DbPlanStatementKind::Returning;
    }
    match first_top_level_keyword(main).as_deref() {
        Some("SELECT" | "VALUES") => DbPlanStatementKind::Select,
        _ => DbPlanStatementKind::Execute,
    }
}

/// Remainder after a leading `WITH [RECURSIVE] cte [, cte …]` list.
///
/// Returns `sql` unchanged when it does not start with `WITH` or the CTE list
/// cannot be parsed (callers then classify from the original text).
fn sql_after_leading_ctes(sql: &str) -> &str {
    let mut i = skip_ws_comments(sql, 0);
    if !keyword_at(sql, i, "WITH") {
        return sql;
    }
    i += 4;
    i = skip_ws_comments(sql, i);
    if keyword_at(sql, i, "RECURSIVE") {
        i += 9;
        i = skip_ws_comments(sql, i);
    }
    loop {
        let Some(next) = skip_ident_or_quoted(sql, i) else {
            return sql;
        };
        i = skip_ws_comments(sql, next);
        if sql.as_bytes().get(i) == Some(&b'(') {
            let Some(next) = skip_balanced_parens(sql, i) else {
                return sql;
            };
            i = skip_ws_comments(sql, next);
        }
        if !keyword_at(sql, i, "AS") {
            return sql;
        }
        i = skip_ws_comments(sql, i + 2);
        if keyword_at(sql, i, "NOT") {
            let after_not = skip_ws_comments(sql, i + 3);
            if keyword_at(sql, after_not, "MATERIALIZED") {
                i = skip_ws_comments(sql, after_not + 12);
            }
        } else if keyword_at(sql, i, "MATERIALIZED") {
            i = skip_ws_comments(sql, i + 12);
        }
        if sql.as_bytes().get(i) != Some(&b'(') {
            return sql;
        }
        let Some(next) = skip_balanced_parens(sql, i) else {
            return sql;
        };
        i = skip_ws_comments(sql, next);
        if sql.as_bytes().get(i) == Some(&b',') {
            i = skip_ws_comments(sql, i + 1);
            continue;
        }
        return &sql[i..];
    }
}

fn skip_ws_comments(sql: &str, mut i: usize) -> usize {
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

fn keyword_at(sql: &str, i: usize, kw: &str) -> bool {
    let bytes = sql.as_bytes();
    let n = kw.len();
    if i.saturating_add(n) > bytes.len() {
        return false;
    }
    if !sql[i..i + n].eq_ignore_ascii_case(kw) {
        return false;
    }
    let before_ok = i == 0 || !ident_cont(bytes[i - 1]);
    let after = bytes.get(i + n).copied().unwrap_or(b' ');
    before_ok && !ident_cont(after)
}

fn skip_ident_or_quoted(sql: &str, i: usize) -> Option<usize> {
    let bytes = sql.as_bytes();
    let c = *bytes.get(i)?;
    if c == b'"' || c == b'`' || c == b'[' {
        let end = if c == b'[' { b']' } else { c };
        let mut j = i + 1;
        while j < bytes.len() {
            if bytes[j] == end {
                if end != b']' && bytes.get(j + 1) == Some(&end) {
                    j += 2;
                    continue;
                }
                return Some(j + 1);
            }
            j += 1;
        }
        return None;
    }
    if ident_start(c) {
        let mut j = i + 1;
        while j < bytes.len() && ident_cont(bytes[j]) {
            j += 1;
        }
        return Some(j);
    }
    None
}

fn skip_balanced_parens(sql: &str, start: usize) -> Option<usize> {
    if sql.as_bytes().get(start) != Some(&b'(') {
        return None;
    }
    let mut depth = 0usize;
    let mut end = None;
    let slice = &sql[start..];
    for_each_unquoted(slice, |text, i| {
        if end.is_some() {
            return 1;
        }
        match text.as_bytes()[i] {
            b'(' => depth += 1,
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    end = Some(start + i + 1);
                }
            }
            _ => {}
        }
        1
    });
    end
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

/// # Errors
///
/// Returns [`PluginError::invalid_params`] when the statement is outside the
/// guest grammar.
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
    for table in named_tables(&stmt.sql)? {
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

/// # Errors
///
/// Returns [`PluginError::invalid_params`] when `resultSelection` or `maxRows`
/// does not match the classified statement.
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
        DbResultSelection::Rows => {}
    }
    Ok(())
}

fn table_denied(name: &str) -> bool {
    name.split('.').any(|part| {
        let lower = normalize_ident(part);
        DENIED_TABLES.iter().any(|t| *t == lower)
            || lower.starts_with("sqlite_")
            || lower.starts_with("pg_")
    })
}

fn normalize_ident(name: &str) -> String {
    name.trim()
        .trim_matches('"')
        .trim_matches('`')
        .trim_matches('[')
        .trim_matches(']')
        .to_ascii_lowercase()
}

/// Parsed table, column, and function names from one guest statement.
///
/// # Errors
///
/// Returns [`PluginError::invalid_params`] when the statement uses a form the
/// authorizer cannot fully resolve (fail closed).
pub fn parse_guest_sql_refs(sql: &str) -> Result<GuestSqlRefs> {
    let mut scan = Scan { sql, i: 0 };
    let mut refs = GuestSqlRefs::default();
    let mut ctes = Vec::new();
    if scan.take_kw("WITH") {
        let recursive = scan.take_kw("RECURSIVE");
        loop {
            let name = scan.read_ident().ok_or_else(|| unresolved("CTE name"))?;
            if scan.take_byte(b'(') {
                scan.take_balanced_inner()
                    .ok_or_else(|| unresolved("CTE column list"))?;
            }
            if !scan.take_kw("AS") {
                return Err(unresolved("CTE AS"));
            }
            let _ = scan.take_kw("NOT");
            let _ = scan.take_kw("MATERIALIZED");
            if !scan.take_byte(b'(') {
                return Err(unresolved("CTE body"));
            }
            let body = scan
                .take_balanced_inner()
                .ok_or_else(|| unresolved("CTE body"))?;
            let mut body_ctes = ctes.clone();
            if recursive {
                body_ctes.push(name.clone());
            }
            parse_statement_into(body, &mut refs, &body_ctes)?;
            ctes.push(name);
            if !scan.take_byte(b',') {
                break;
            }
        }
    }
    parse_statement_into(scan.rest(), &mut refs, &ctes)?;
    refs.ctes = ctes;
    refs.tables.sort();
    refs.tables.dedup();
    refs.ctes.sort();
    refs.ctes.dedup();
    Ok(refs)
}

fn unresolved(what: &str) -> PluginError {
    PluginError::invalid_params(format!(
        "guest SQL is not fully resolvable for authorization ({what})"
    ))
}

/// True when parenthesized SQL starts with a statement verb, not an expression.
fn looks_like_sql_statement(sql: &str) -> bool {
    let mut scan = Scan { sql, i: 0 };
    scan.peek_kw("WITH")
        || scan.peek_kw("SELECT")
        || scan.peek_kw("VALUES")
        || scan.peek_kw("INSERT")
        || scan.peek_kw("REPLACE")
        || scan.peek_kw("UPDATE")
        || scan.peek_kw("DELETE")
}

/// True when `name` is a CTE in `refs` (qualified names match on the last label).
fn is_cte_name(refs: &GuestSqlRefs, name: &str) -> bool {
    let last = name.rsplit('.').next().unwrap_or(name);
    refs.ctes.iter().any(|cte| cte == name || cte == last)
}

#[derive(Clone, Copy)]
struct Scan<'a> {
    /// Remaining SQL being scanned.
    sql: &'a str,
    /// Byte offset into [`Self::sql`].
    i: usize,
}

impl<'a> Scan<'a> {
    fn skip(&mut self) {
        self.i = skip_ws_comments(self.sql, self.i);
    }

    fn rest(&self) -> &'a str {
        &self.sql[self.i..]
    }

    fn take_kw(&mut self, kw: &str) -> bool {
        self.skip();
        if keyword_at(self.sql, self.i, kw) {
            self.i += kw.len();
            true
        } else {
            false
        }
    }

    fn peek_kw(&mut self, kw: &str) -> bool {
        self.skip();
        keyword_at(self.sql, self.i, kw)
    }

    fn take_byte(&mut self, b: u8) -> bool {
        self.skip();
        if self.sql.as_bytes().get(self.i) == Some(&b) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn peek_byte(&mut self, b: u8) -> bool {
        self.skip();
        self.sql.as_bytes().get(self.i) == Some(&b)
    }

    fn read_ident(&mut self) -> Option<String> {
        self.skip();
        let start = self.i;
        let end = skip_ident_or_quoted(self.sql, self.i)?;
        self.i = end;
        let ident = normalize_ident(&self.sql[start..end]);
        if ident.is_empty() {
            None
        } else {
            Some(ident)
        }
    }

    fn take_balanced_inner(&mut self) -> Option<&'a str> {
        let start = self.i.saturating_sub(1);
        let end = skip_balanced_parens(self.sql, start)?;
        let inner = &self.sql[start + 1..end - 1];
        self.i = end;
        Some(inner)
    }
}

/// # Errors
///
/// Returns [`PluginError::invalid_params`] when the statement cannot be fully
/// resolved for authorization.
fn parse_statement_into(sql: &str, refs: &mut GuestSqlRefs, ctes: &[String]) -> Result<()> {
    let mut scan = Scan { sql, i: 0 };
    if scan.take_kw("INSERT") || scan.take_kw("REPLACE") {
        let _ = scan.take_kw("OR");
        let _ = scan.take_kw("IGNORE");
        let _ = scan.take_kw("REPLACE");
        if !scan.take_kw("INTO") {
            return Err(unresolved("INSERT INTO"));
        }
        collect_physical_table(&mut scan, refs, ctes)?;
        if scan.take_byte(b'(') {
            collect_ident_list(&mut scan, refs)?;
            if !scan.take_byte(b')') {
                return Err(unresolved("INSERT column list"));
            }
        }
        collect_expr_tail(&mut scan, refs, ctes)?;
        return Ok(());
    }
    if scan.take_kw("UPDATE") {
        collect_physical_table(&mut scan, refs, ctes)?;
        collect_expr_tail(&mut scan, refs, ctes)?;
        return Ok(());
    }
    if scan.take_kw("DELETE") {
        if !scan.take_kw("FROM") {
            return Err(unresolved("DELETE FROM"));
        }
        collect_physical_table(&mut scan, refs, ctes)?;
        collect_expr_tail(&mut scan, refs, ctes)?;
        return Ok(());
    }
    if scan.take_kw("SELECT") || scan.take_kw("VALUES") {
        collect_expr_tail(&mut scan, refs, ctes)?;
        return Ok(());
    }
    if scan.take_kw("WITH") {
        return parse_guest_sql_refs(sql).map(|nested| {
            refs.tables.extend(nested.tables);
            refs.columns.extend(nested.columns);
            refs.functions.extend(nested.functions);
            refs.ctes.extend(nested.ctes);
        });
    }
    Err(unresolved("statement verb"))
}

/// # Errors
///
/// Returns [`PluginError::invalid_params`] when the table name cannot be read.
fn collect_physical_table(
    scan: &mut Scan<'_>,
    refs: &mut GuestSqlRefs,
    ctes: &[String],
) -> Result<String> {
    if scan.peek_byte(b'(') {
        return Err(unresolved("derived table"));
    }
    let mut name = scan.read_ident().ok_or_else(|| unresolved("table name"))?;
    if scan.take_byte(b'.') {
        let second = scan
            .read_ident()
            .ok_or_else(|| unresolved("qualified table"))?;
        name = format!("{name}.{second}");
    }
    let last = name.rsplit('.').next().unwrap_or(name.as_str());
    if !ctes.iter().any(|c| c == last || c == &name) {
        refs.tables.push(name.clone());
    }
    Ok(name)
}

fn take_alias(scan: &mut Scan<'_>) -> Option<String> {
    if scan.take_kw("AS") {
        return scan.read_ident();
    }
    if scan.peek_kw("WHERE")
        || scan.peek_kw("SET")
        || scan.peek_kw("JOIN")
        || scan.peek_kw("LEFT")
        || scan.peek_kw("RIGHT")
        || scan.peek_kw("INNER")
        || scan.peek_kw("FULL")
        || scan.peek_kw("CROSS")
        || scan.peek_kw("OUTER")
        || scan.peek_kw("ON")
        || scan.peek_kw("USING")
        || scan.peek_kw("GROUP")
        || scan.peek_kw("HAVING")
        || scan.peek_kw("ORDER")
        || scan.peek_kw("LIMIT")
        || scan.peek_kw("OFFSET")
        || scan.peek_kw("RETURNING")
        || scan.peek_kw("UNION")
        || scan.peek_kw("EXCEPT")
        || scan.peek_kw("INTERSECT")
        || scan.peek_kw("FROM")
        || scan.peek_kw("INTO")
        || scan.peek_kw("VALUES")
        || scan.peek_kw("SELECT")
        || scan.peek_kw("AND")
        || scan.peek_kw("OR")
        || scan.peek_byte(b',')
        || scan.peek_byte(b')')
        || scan.peek_byte(b';')
    {
        return None;
    }
    scan.read_ident()
}

/// # Errors
///
/// Returns [`PluginError::invalid_params`] when a column identifier is missing.
fn collect_ident_list(scan: &mut Scan<'_>, refs: &mut GuestSqlRefs) -> Result<()> {
    loop {
        if scan.peek_byte(b')') {
            return Ok(());
        }
        let ident = scan.read_ident().ok_or_else(|| unresolved("column list"))?;
        refs.columns.push((None, ident));
        if !scan.take_byte(b',') {
            return Ok(());
        }
    }
}

/// # Errors
///
/// Returns [`PluginError::invalid_params`] when an expression or table ref
/// cannot be fully resolved.
fn collect_expr_tail(scan: &mut Scan<'_>, refs: &mut GuestSqlRefs, ctes: &[String]) -> Result<()> {
    let mut aliases: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    let start = *scan;
    collect_from_aliases(scan, refs, ctes, &mut aliases)?;
    *scan = start;
    collect_expr_atoms_with_aliases(scan, refs, ctes, &aliases)
}

/// Walks FROM/JOIN sources first so SELECT-list aliases resolve.
///
/// # Errors
///
/// Returns [`PluginError::invalid_params`] when a table ref cannot be resolved.
fn collect_from_aliases(
    scan: &mut Scan<'_>,
    refs: &mut GuestSqlRefs,
    ctes: &[String],
    aliases: &mut std::collections::BTreeMap<String, String>,
) -> Result<()> {
    while !scan_at_end(scan) {
        if scan.take_kw("FROM") {
            parse_table_refs(scan, refs, ctes, aliases)?;
            continue;
        }
        if take_join_kw(scan) {
            parse_one_table_ref(scan, refs, ctes, aliases)?;
            continue;
        }
        skip_one_expr_atom(scan, refs, ctes)?;
    }
    Ok(())
}

/// Second pass: column/function refs after FROM aliases are known.
///
/// # Errors
///
/// Returns [`PluginError::invalid_params`] when a column, function, or table
/// ref cannot be fully resolved.
fn collect_expr_atoms_with_aliases(
    scan: &mut Scan<'_>,
    refs: &mut GuestSqlRefs,
    ctes: &[String],
    aliases: &std::collections::BTreeMap<String, String>,
) -> Result<()> {
    while !scan_at_end(scan) {
        if scan.take_kw("FROM") {
            skip_table_refs(scan)?;
            continue;
        }
        if take_join_kw(scan) {
            skip_one_table_ref(scan)?;
            if scan.take_kw("ON") {
                continue;
            }
            if scan.take_kw("USING") {
                if !scan.take_byte(b'(') {
                    return Err(unresolved("USING"));
                }
                collect_ident_list(scan, refs)?;
                if !scan.take_byte(b')') {
                    return Err(unresolved("USING"));
                }
            }
            continue;
        }
        collect_one_expr_atom(scan, refs, ctes, aliases)?;
        if scan.take_kw("AS") {
            let _ = scan.read_ident();
        }
    }
    Ok(())
}

fn take_join_kw(scan: &mut Scan<'_>) -> bool {
    scan.take_kw("JOIN")
        || (scan.take_kw("INNER") && scan.take_kw("JOIN"))
        || (scan.take_kw("CROSS") && scan.take_kw("JOIN"))
        || join_with_outer(scan)
}

/// Skips a comma-separated FROM list without collecting identifiers.
///
/// # Errors
///
/// Returns [`PluginError::invalid_params`] when a table ref is malformed.
fn skip_table_refs(scan: &mut Scan<'_>) -> Result<()> {
    skip_one_table_ref(scan)?;
    while scan.take_byte(b',') {
        skip_one_table_ref(scan)?;
    }
    Ok(())
}

/// Skips one FROM/JOIN table ref (name, alias, or parenthesized subquery).
///
/// # Errors
///
/// Returns [`PluginError::invalid_params`] when the table ref is malformed or
/// a table-valued function.
fn skip_one_table_ref(scan: &mut Scan<'_>) -> Result<()> {
    if scan.take_byte(b'(') {
        scan.take_balanced_inner()
            .ok_or_else(|| unresolved("subquery"))?;
        let _ = take_alias(scan);
        return Ok(());
    }
    let _ = scan.read_ident().ok_or_else(|| unresolved("table name"))?;
    if scan.take_byte(b'.') {
        let _ = scan
            .read_ident()
            .ok_or_else(|| unresolved("qualified table"))?;
    }
    if scan.peek_byte(b'(') {
        return Err(unresolved("table-valued function"));
    }
    let _ = take_alias(scan);
    Ok(())
}

/// Skips one expression atom during the FROM-alias collection pass.
///
/// # Errors
///
/// Returns [`PluginError::invalid_params`] when a nested subquery cannot be
/// resolved.
fn skip_one_expr_atom(scan: &mut Scan<'_>, refs: &mut GuestSqlRefs, ctes: &[String]) -> Result<()> {
    scan.skip();
    if scan.i >= scan.sql.len() {
        return Ok(());
    }
    let c = scan.sql.as_bytes()[scan.i];
    if c == b'\'' {
        skip_quoted_string(scan, b'\'');
        return Ok(());
    }
    if c == b'(' {
        scan.i += 1;
        let inner = scan
            .take_balanced_inner()
            .ok_or_else(|| unresolved("subquery"))?;
        if looks_like_sql_statement(inner) {
            parse_statement_into(inner, refs, ctes)?;
        }
        return Ok(());
    }
    if scan.read_ident().is_some() {
        if scan.take_byte(b'(') {
            let _ = scan.take_balanced_inner();
            return Ok(());
        }
        if scan.take_byte(b'.') {
            if scan.take_byte(b'*') {
                return Ok(());
            }
            let _ = scan.read_ident();
            if scan.take_byte(b'(') {
                let _ = scan.take_balanced_inner();
            }
        }
        return Ok(());
    }
    scan.i += 1;
    Ok(())
}

fn join_with_outer(scan: &mut Scan<'_>) -> bool {
    if scan.take_kw("LEFT") || scan.take_kw("RIGHT") || scan.take_kw("FULL") {
        let _ = scan.take_kw("OUTER");
        return scan.take_kw("JOIN");
    }
    false
}

fn scan_at_end(scan: &mut Scan<'_>) -> bool {
    scan.skip();
    scan.i >= scan.sql.len() || scan.peek_byte(b';')
}

/// # Errors
///
/// Returns [`PluginError::invalid_params`] when a FROM/JOIN source cannot be
/// resolved.
fn parse_table_refs(
    scan: &mut Scan<'_>,
    refs: &mut GuestSqlRefs,
    ctes: &[String],
    aliases: &mut std::collections::BTreeMap<String, String>,
) -> Result<()> {
    parse_one_table_ref(scan, refs, ctes, aliases)?;
    while scan.take_byte(b',') {
        parse_one_table_ref(scan, refs, ctes, aliases)?;
    }
    Ok(())
}

/// # Errors
///
/// Returns [`PluginError::invalid_params`] when the table, subquery, or
/// table-valued function cannot be resolved.
fn parse_one_table_ref(
    scan: &mut Scan<'_>,
    refs: &mut GuestSqlRefs,
    ctes: &[String],
    aliases: &mut std::collections::BTreeMap<String, String>,
) -> Result<()> {
    if scan.take_byte(b'(') {
        let inner = scan
            .take_balanced_inner()
            .ok_or_else(|| unresolved("subquery"))?;
        parse_statement_into(inner, refs, ctes)?;
        let alias = take_alias(scan);
        let _ = alias;
        return Ok(());
    }
    let table = collect_physical_table(scan, refs, ctes)?;
    if scan.peek_byte(b'(') {
        return Err(unresolved("table-valued function"));
    }
    if let Some(alias) = take_alias(scan) {
        aliases.insert(alias, table);
    }
    Ok(())
}

/// # Errors
///
/// Returns [`PluginError::invalid_params`] when a qualified column cannot be
/// read.
fn collect_one_expr_atom(
    scan: &mut Scan<'_>,
    refs: &mut GuestSqlRefs,
    ctes: &[String],
    aliases: &std::collections::BTreeMap<String, String>,
) -> Result<()> {
    scan.skip();
    if scan.i >= scan.sql.len() {
        return Ok(());
    }
    let c = scan.sql.as_bytes()[scan.i];
    if c == b'\'' {
        skip_quoted_string(scan, b'\'');
        return Ok(());
    }
    if c == b'(' {
        scan.i += 1;
        let inner = scan
            .take_balanced_inner()
            .ok_or_else(|| unresolved("subquery"))?;
        if looks_like_sql_statement(inner) {
            parse_statement_into(inner, refs, ctes)?;
        } else {
            let mut inner_scan = Scan { sql: inner, i: 0 };
            collect_expr_atoms_with_aliases(&mut inner_scan, refs, ctes, aliases)?;
        }
        return Ok(());
    }
    if scan.peek_byte(b'*') {
        scan.i += 1;
        refs.columns.push((None, "*".into()));
        return Ok(());
    }
    if let Some(ident) = scan.read_ident() {
        if scan.take_byte(b'(') {
            if !sql_keyword(&ident) {
                refs.functions.push(ident);
            }
            return Ok(());
        }
        if scan.take_byte(b'.') {
            if scan.take_byte(b'*') {
                let table = aliases.get(&ident).cloned().unwrap_or(ident);
                refs.columns.push((Some(table), "*".into()));
                return Ok(());
            }
            let col = scan
                .read_ident()
                .ok_or_else(|| unresolved("qualified column"))?;
            if scan.take_byte(b'(') {
                if !sql_keyword(&col) {
                    refs.functions.push(col);
                }
                return Ok(());
            }
            let table = aliases.get(&ident).cloned().unwrap_or(ident);
            refs.columns.push((Some(table), col));
            return Ok(());
        }
        if ident == "*" {
            refs.columns.push((None, "*".into()));
            return Ok(());
        }
        if !sql_keyword(&ident) {
            refs.columns.push((None, ident));
        }
        return Ok(());
    }
    scan.i += 1;
    Ok(())
}

fn skip_quoted_string(scan: &mut Scan<'_>, q: u8) {
    scan.i += 1;
    let bytes = scan.sql.as_bytes();
    while scan.i < bytes.len() {
        if bytes[scan.i] == q {
            if bytes.get(scan.i + 1) == Some(&q) {
                scan.i += 2;
                continue;
            }
            scan.i += 1;
            return;
        }
        scan.i += 1;
    }
}

/// # Errors
///
/// Returns [`PluginError::invalid_params`] when table refs cannot be fully
/// resolved.
fn named_tables(sql: &str) -> Result<Vec<String>> {
    Ok(parse_guest_sql_refs(sql)?.tables)
}

fn sql_keyword(ident: &str) -> bool {
    matches!(
        ident,
        "and"
            | "as"
            | "asc"
            | "between"
            | "by"
            | "case"
            | "collate"
            | "cross"
            | "desc"
            | "distinct"
            | "else"
            | "end"
            | "exists"
            | "from"
            | "full"
            | "group"
            | "having"
            | "in"
            | "inner"
            | "insert"
            | "into"
            | "is"
            | "join"
            | "left"
            | "like"
            | "limit"
            | "not"
            | "null"
            | "offset"
            | "on"
            | "or"
            | "order"
            | "outer"
            | "over"
            | "recursive"
            | "replace"
            | "returning"
            | "right"
            | "select"
            | "set"
            | "table"
            | "then"
            | "union"
            | "update"
            | "using"
            | "values"
            | "when"
            | "where"
            | "with"
    )
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
#[allow(clippy::missing_panics_doc)]
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
                kind: guest_statement_kind(sql),
                max_rows,
                result_selection: selection,
            }],
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
    fn allows_rows_on_plain_dml_for_d1_run_parity() {
        validate_guest_execute_request(&req(
            "INSERT INTO books (id) VALUES (?)",
            vec![DbValue::Int64(1)],
            DbResultSelection::Rows,
            1,
        ))
        .unwrap();
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

    #[test]
    fn cte_kind_follows_the_main_statement() {
        assert_eq!(
            guest_statement_kind("WITH seed AS (SELECT 1) SELECT * FROM seed"),
            DbPlanStatementKind::Select
        );
        assert_eq!(
            guest_statement_kind("WITH seed AS (SELECT 1) INSERT INTO t SELECT * FROM seed"),
            DbPlanStatementKind::Execute
        );
        assert_eq!(
            guest_statement_kind("WITH seed AS (SELECT 1) UPDATE t SET x = 1"),
            DbPlanStatementKind::Execute
        );
        assert_eq!(
            guest_statement_kind("WITH seed AS (SELECT 1) DELETE FROM t"),
            DbPlanStatementKind::Execute
        );
        assert_eq!(
            guest_statement_kind(
                "WITH seed AS (SELECT 1) INSERT INTO t SELECT * FROM seed RETURNING id"
            ),
            DbPlanStatementKind::Returning
        );
        assert_eq!(
            guest_statement_kind("WITH seed AS (SELECT 1) UPDATE t SET x = 1 RETURNING x"),
            DbPlanStatementKind::Returning
        );
        assert_eq!(
            guest_statement_kind("WITH seed AS (SELECT 1) DELETE FROM t RETURNING id"),
            DbPlanStatementKind::Returning
        );
        assert_eq!(
            guest_statement_kind(
                "WITH x AS (INSERT INTO t VALUES (1) RETURNING id) SELECT * FROM x"
            ),
            DbPlanStatementKind::Select
        );
        assert_eq!(
            guest_statement_kind(
                "WITH RECURSIVE t(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM t WHERE x < 3) INSERT INTO u SELECT x FROM t"
            ),
            DbPlanStatementKind::Execute
        );
    }

    #[test]
    fn cte_dml_selection_matches_main_statement() {
        validate_guest_execute_request(&req(
            "WITH seed AS (SELECT 1) INSERT INTO books (id) SELECT * FROM seed",
            vec![],
            DbResultSelection::AffectedRows,
            0,
        ))
        .unwrap();
        validate_guest_execute_request(&req(
            "WITH seed AS (SELECT 1) INSERT INTO books (id) SELECT * FROM seed",
            vec![],
            DbResultSelection::Rows,
            1,
        ))
        .unwrap();
        validate_guest_execute_request(&req(
            "WITH seed AS (SELECT 1) INSERT INTO books (id) SELECT * FROM seed RETURNING id",
            vec![],
            DbResultSelection::Rows,
            1,
        ))
        .unwrap();
        let err = validate_guest_execute_request(&req(
            "WITH seed AS (SELECT 1) SELECT * FROM seed",
            vec![],
            DbResultSelection::AffectedRows,
            0,
        ))
        .unwrap_err();
        assert!(err.to_string().contains("affectedRows"), "{err}");
    }

    #[test]
    fn policy_allowlist_rejects_unrelated_tables_and_functions() {
        let books = GuestSqlPolicy::allow_tables(["books"]);
        authorize_guest_sql_policy(
            &req("SELECT id FROM books", vec![], DbResultSelection::Rows, 1),
            &books,
        )
        .unwrap();
        let err = authorize_guest_sql_policy(
            &req("SELECT id FROM jobs", vec![], DbResultSelection::Rows, 1),
            &books,
        )
        .unwrap_err();
        assert!(err.to_string().contains("unauthorized table"), "{err}");
        let err = authorize_guest_sql_policy(
            &req(
                "SELECT load_extension('x') FROM books",
                vec![],
                DbResultSelection::Rows,
                1,
            ),
            &books,
        )
        .unwrap_err();
        assert!(err.to_string().contains("unauthorized function"), "{err}");
        let restricted = GuestSqlPolicy::allow_tables(["books"]).restrict_columns("books", ["id"]);
        let err = authorize_guest_sql_policy(
            &req(
                "SELECT books.token FROM books",
                vec![],
                DbResultSelection::Rows,
                1,
            ),
            &restricted,
        )
        .unwrap_err();
        assert!(err.to_string().contains("unauthorized column"), "{err}");
        let err = authorize_guest_sql_policy(
            &req("SELECT id FROM books", vec![], DbResultSelection::Rows, 1),
            &GuestSqlPolicy::deny_all(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("unauthorized table"), "{err}");
    }

    #[test]
    fn policy_fail_closed_on_joins_quotes_and_unqualified_columns() {
        let books = GuestSqlPolicy::allow_tables(["books"]);
        let err = authorize_guest_sql_policy(
            &req("SELECT * FROM [jobs]", vec![], DbResultSelection::Rows, 1),
            &books,
        )
        .unwrap_err();
        assert!(err.to_string().contains("unauthorized table"), "{err}");
        let err = authorize_guest_sql_policy(
            &req(
                "SELECT secret FROM books, jobs",
                vec![],
                DbResultSelection::Rows,
                1,
            ),
            &books,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("unauthorized table")
                || err.to_string().contains("cannot be resolved"),
            "{err}"
        );
        let err = authorize_guest_sql_policy(
            &req(
                "SELECT secret FROM books JOIN jobs USING (id)",
                vec![],
                DbResultSelection::Rows,
                1,
            ),
            &books,
        )
        .unwrap_err();
        assert!(err.to_string().contains("unauthorized table"), "{err}");
        authorize_guest_sql_policy(
            &req(
                "WITH recent AS (SELECT id FROM books) SELECT id FROM recent",
                vec![],
                DbResultSelection::Rows,
                1,
            ),
            &books,
        )
        .unwrap();
        let restricted = GuestSqlPolicy::allow_tables(["books"]).restrict_columns("books", ["id"]);
        let err = authorize_guest_sql_policy(
            &req(
                "SELECT token FROM books",
                vec![],
                DbResultSelection::Rows,
                1,
            ),
            &restricted,
        )
        .unwrap_err();
        assert!(err.to_string().contains("unauthorized column"), "{err}");
        authorize_guest_sql_policy(
            &req(
                "SELECT \"hex\"(id) FROM books",
                vec![],
                DbResultSelection::Rows,
                1,
            ),
            &books,
        )
        .unwrap();
        let restricted = GuestSqlPolicy::allow_tables(["books"]).restrict_columns("books", ["id"]);
        let err = authorize_guest_sql_policy(
            &req("SELECT * FROM books", vec![], DbResultSelection::Rows, 1),
            &restricted,
        )
        .unwrap_err();
        assert!(err.to_string().contains("SELECT * is not allowed"), "{err}");
        let err = authorize_guest_sql_policy(
            &req(
                "SELECT books.* FROM books",
                vec![],
                DbResultSelection::Rows,
                1,
            ),
            &restricted,
        )
        .unwrap_err();
        assert!(err.to_string().contains("SELECT * is not allowed"), "{err}");
        authorize_guest_sql_policy(
            &req(
                "SELECT id AS label FROM books",
                vec![],
                DbResultSelection::Rows,
                1,
            ),
            &restricted,
        )
        .unwrap();
        let err = parse_guest_sql_refs("SELECT id FROM generate_series(1, 2)").unwrap_err();
        assert!(err.to_string().contains("not fully resolvable"), "{err}");
        authorize_guest_sql_policy(
            &req("SELECT id FROM jobs", vec![], DbResultSelection::Rows, 1),
            &GuestSqlPolicy::host_authoritative(),
        )
        .unwrap();
    }

    #[test]
    fn policy_allows_from_aliases_and_as_aliases() {
        let books = GuestSqlPolicy::allow_tables(["books"]);
        authorize_guest_sql_policy(
            &req(
                "SELECT b.id FROM books b",
                vec![],
                DbResultSelection::Rows,
                1,
            ),
            &books,
        )
        .unwrap();
        authorize_guest_sql_policy(
            &req(
                "SELECT b.id FROM books AS b",
                vec![],
                DbResultSelection::Rows,
                1,
            ),
            &books,
        )
        .unwrap();
    }

    #[test]
    fn policy_allows_cte_qualified_columns() {
        let books = GuestSqlPolicy::allow_tables(["books"]);
        authorize_guest_sql_policy(
            &req(
                "WITH recent AS (SELECT id FROM books) SELECT recent.id FROM recent",
                vec![],
                DbResultSelection::Rows,
                1,
            ),
            &books,
        )
        .unwrap();
    }

    #[test]
    fn policy_allows_recursive_cte_self_scope() {
        let books = GuestSqlPolicy::allow_tables(["books"]);
        authorize_guest_sql_policy(
            &req(
                "WITH RECURSIVE t(x) AS (\
                     SELECT id FROM books \
                     UNION ALL \
                     SELECT t.x FROM t WHERE t.x < 3\
                 ) SELECT t.x FROM t",
                vec![],
                DbResultSelection::Rows,
                1,
            ),
            &books,
        )
        .unwrap();
    }

    #[test]
    fn policy_allows_parenthesized_column_expressions() {
        let books = GuestSqlPolicy::allow_tables(["books"]);
        authorize_guest_sql_policy(
            &req(
                "SELECT (id + 1) FROM books",
                vec![],
                DbResultSelection::Rows,
                1,
            ),
            &books,
        )
        .unwrap();
    }
}
