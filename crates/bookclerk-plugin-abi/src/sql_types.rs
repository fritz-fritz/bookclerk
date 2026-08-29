//! Bookclerk SQL v1 types, CAST matrix, and fail-closed expression checking.
//!
//! Schema metadata for plugin-owned bindings is durable in
//! [`SQL_CATALOG_TABLE`]. This module only interprets canonical SQL; adapters
//! persist catalog rows at execute.

#![allow(clippy::missing_docs_in_private_items, clippy::missing_errors_doc)]

use crate::{DbType, DbValue, ExecuteRequest, PluginError, Result, TypedDbStatement};
use std::collections::BTreeMap;

/// Reserved catalog of `(table, column, sql_type)` for isolated bindings.
pub const SQL_CATALOG_TABLE: &str = "bookclerk_sql_catalog";

/// Reserved transactional identity high-water table (Postgres adapter-private).
pub const SQL_IDENTITY_TABLE: &str = "bookclerk_identity";

/// Internal alias used when wrapping `INSERT OR IGNORE … SELECT`.
pub const INSERT_SELECT_WRAP_ALIAS: &str = "_bc_src";

/// Canonical SQL v1 column / expression type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SqlType {
    /// `INTEGER` / `DbValue::Int64`.
    Integer,
    /// `REAL` / `DbValue::Float64`.
    Real,
    /// `TEXT` / `DbValue::Text`.
    Text,
    /// `BLOB` / `DbValue::Bytes`.
    Blob,
    /// `BOOLEAN` / `DbValue::Boolean`.
    Boolean,
    /// Untyped SQL `NULL` (unifies with any type).
    Null,
}

impl SqlType {
    /// Parses a canonical column type ident (already lowercased).
    #[must_use]
    pub fn from_column_ident(ty: &str) -> Option<Self> {
        match ty.to_ascii_lowercase().as_str() {
            "integer" | "bigint" | "int" | "int4" | "int8" | "smallint" => Some(Self::Integer),
            "real" | "float" | "double" => Some(Self::Real),
            "text" | "varchar" => Some(Self::Text),
            "blob" | "bytea" => Some(Self::Blob),
            "boolean" | "bool" => Some(Self::Boolean),
            _ => None,
        }
    }

    /// Catalog / docs spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Integer => "integer",
            Self::Real => "real",
            Self::Text => "text",
            Self::Blob => "blob",
            Self::Boolean => "boolean",
            Self::Null => "null",
        }
    }

    /// True when this type is TEXT.
    #[must_use]
    pub const fn is_text(self) -> bool {
        matches!(self, Self::Text)
    }

    /// True when this type is INTEGER or REAL.
    #[must_use]
    pub const fn is_numeric(self) -> bool {
        matches!(self, Self::Integer | Self::Real)
    }
}

impl From<DbType> for SqlType {
    fn from(ty: DbType) -> Self {
        match ty {
            DbType::Int64 => Self::Integer,
            DbType::Float64 => Self::Real,
            DbType::Text => Self::Text,
            DbType::Bytes => Self::Blob,
            DbType::Bool => Self::Boolean,
            DbType::Unspecified => Self::Null,
        }
    }
}

/// `(table → column → type)` environment used at admission and typed lowering.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SqlTypeEnv {
    tables: BTreeMap<String, BTreeMap<String, SqlType>>,
}

impl SqlTypeEnv {
    /// Empty environment.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// True when no tables are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }

    /// True when `table` is in the environment.
    #[must_use]
    pub fn has_table(&self, table: &str) -> bool {
        self.tables.contains_key(&table.to_ascii_lowercase())
    }

    /// Declared type of `table.column`, if present.
    #[must_use]
    pub fn column_type(&self, table: &str, column: &str) -> Option<SqlType> {
        self.tables
            .get(&table.to_ascii_lowercase())
            .and_then(|cols| cols.get(&column.to_ascii_lowercase()).copied())
    }

    /// True when `column` is TEXT in every table that declares it, and at least one does.
    #[must_use]
    pub fn ident_is_text(&self, column: &str) -> bool {
        let col = column.to_ascii_lowercase();
        let mut saw = false;
        for cols in self.tables.values() {
            if let Some(ty) = cols.get(&col) {
                saw = true;
                if *ty != SqlType::Text {
                    return false;
                }
            }
        }
        saw
    }

    /// Inserts or replaces columns for `table`.
    pub fn insert_table(
        &mut self,
        table: impl Into<String>,
        columns: impl IntoIterator<Item = (String, SqlType)>,
    ) {
        let table = table.into().to_ascii_lowercase();
        let cols = columns
            .into_iter()
            .map(|(n, t)| (n.to_ascii_lowercase(), t))
            .collect();
        self.tables.insert(table, cols);
    }

    /// Drops `table` from the environment.
    pub fn drop_table(&mut self, table: &str) {
        self.tables.remove(&table.to_ascii_lowercase());
    }

    /// Merges `other` (later entries win per column).
    pub fn merge(&mut self, other: &Self) {
        for (table, cols) in &other.tables {
            self.tables
                .entry(table.clone())
                .or_default()
                .extend(cols.clone());
        }
    }

    /// Records one catalog row.
    pub fn insert_column(&mut self, table: &str, column: &str, ty: SqlType) {
        self.tables
            .entry(table.to_ascii_lowercase())
            .or_default()
            .insert(column.to_ascii_lowercase(), ty);
    }

    /// Iterates `(table, column, type)` in sorted order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str, SqlType)> {
        self.tables.iter().flat_map(|(t, cols)| {
            cols.iter()
                .map(move |(c, ty)| (t.as_str(), c.as_str(), *ty))
        })
    }
}

/// Parsed `CREATE TABLE` column list (canonical SQL v1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateTableSchema {
    /// Unquoted folded table name.
    pub table: String,
    /// Column names and types in declaration order.
    pub columns: Vec<(String, SqlType)>,
    /// `INTEGER PRIMARY KEY AUTOINCREMENT` column, if any.
    pub identity_column: Option<String>,
}

/// Parses a canonical `CREATE TABLE` into columns / identity, if the head matches.
#[must_use]
pub fn parse_create_table_schema(sql: &str) -> Option<CreateTableSchema> {
    let mut s = skip_ws(sql);
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
        s = skip_kw(s, "EXISTS")?;
    }
    let (table, rest) = read_ident(s)?;
    let rest = skip_ws(rest);
    if !rest.starts_with('(') {
        return None;
    }
    let inner = balanced_inner(rest)?;
    let mut columns = Vec::new();
    let mut identity_column = None;
    for def in split_top_level_commas(inner) {
        let def = skip_ws(def);
        if starts_kw(def, "PRIMARY")
            || starts_kw(def, "UNIQUE")
            || starts_kw(def, "CHECK")
            || starts_kw(def, "FOREIGN")
            || starts_kw(def, "CONSTRAINT")
        {
            continue;
        }
        let (name, rest) = read_ident(def)?;
        let rest = skip_ws(rest);
        let (ty_ident, rest) = read_ident(rest)?;
        let ty = SqlType::from_column_ident(&ty_ident)?;
        if identity_column.is_none() && column_is_autoincrement(rest) && ty == SqlType::Integer {
            identity_column = Some(name.clone());
        }
        columns.push((name, ty));
    }
    if columns.is_empty() {
        return None;
    }
    Some(CreateTableSchema {
        table,
        columns,
        identity_column,
    })
}

/// Parses `DROP TABLE [IF EXISTS] name`.
#[must_use]
pub fn parse_drop_table_name(sql: &str) -> Option<String> {
    let mut s = skip_ws(sql);
    s = skip_kw(s, "DROP")?;
    s = skip_kw(s, "TABLE")?;
    if starts_kw(s, "IF") {
        s = skip_kw(s, "IF")?;
        s = skip_kw(s, "EXISTS")?;
    }
    let (name, _) = read_ident(s)?;
    Some(name)
}

/// `CREATE TABLE IF NOT EXISTS` for the reserved SQL v1 catalog.
#[must_use]
pub fn sql_catalog_create_table_sql() -> String {
    format!(
        "CREATE TABLE IF NOT EXISTS {SQL_CATALOG_TABLE} (\
         table_name TEXT NOT NULL, column_name TEXT NOT NULL, sql_type TEXT NOT NULL, \
         PRIMARY KEY (table_name, column_name))"
    )
}

/// Catalog DML companions for one canonical DDL statement (all backends).
#[must_use]
pub fn catalog_companions(sql: &str) -> Vec<String> {
    if let Some(schema) = parse_create_table_schema(sql) {
        if schema.columns.is_empty() {
            return Vec::new();
        }
        let values = schema
            .columns
            .iter()
            .map(|(col, ty)| {
                format!(
                    "('{}', '{}', '{}')",
                    escape_sql_str(&schema.table),
                    escape_sql_str(col),
                    ty.as_str()
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        return vec![
            sql_catalog_create_table_sql(),
            format!(
            "INSERT INTO {SQL_CATALOG_TABLE} (table_name, column_name, sql_type) \
             VALUES {values} ON CONFLICT (table_name, column_name) DO UPDATE SET sql_type = excluded.sql_type"
            ),
        ];
    }
    if let Some(table) = parse_drop_table_name(sql) {
        return vec![
            sql_catalog_create_table_sql(),
            format!(
                "DELETE FROM {SQL_CATALOG_TABLE} WHERE table_name = '{}'",
                escape_sql_str(&table)
            ),
        ];
    }
    Vec::new()
}

/// Applies one statement's `CREATE`/`DROP TABLE` effects to `env`.
pub fn apply_schema_sql_to_env(env: &mut SqlTypeEnv, sql: &str) {
    if let Some(schema) = parse_create_table_schema(sql) {
        env.insert_table(schema.table, schema.columns);
    } else if let Some(table) = parse_drop_table_name(sql) {
        env.drop_table(&table);
    }
}

/// Reconstructs a type environment from canonical `CREATE TABLE` statements.
#[must_use]
pub fn sql_type_env_from_canonical_ddl(sql: &str) -> SqlTypeEnv {
    let mut env = SqlTypeEnv::new();
    for stmt in sql.split(';') {
        apply_schema_sql_to_env(&mut env, stmt);
    }
    env
}

/// Proven v1 CAST matrix: same type, INTEGER↔REAL, or NULL to any admitted type.
#[must_use]
pub fn cast_is_legal(from: SqlType, to: SqlType) -> bool {
    if from == SqlType::Null || from == to {
        return true;
    }
    matches!(
        (from, to),
        (SqlType::Integer, SqlType::Real) | (SqlType::Real, SqlType::Integer)
    )
}

/// Unifies two types (`NULL` is the identity; INTEGER+REAL → REAL).
///
/// # Errors
///
/// Returns [`PluginError::invalid_params`] when the types cannot unify.
pub fn unify_types(index: usize, a: SqlType, b: SqlType) -> Result<SqlType> {
    match (a, b) {
        (SqlType::Null, t) | (t, SqlType::Null) => Ok(t),
        (x, y) if x == y => Ok(x),
        (SqlType::Integer, SqlType::Real) | (SqlType::Real, SqlType::Integer) => Ok(SqlType::Real),
        _ => Err(PluginError::invalid_params(format!(
            "statement {index} mixes incompatible SQL v1 types {} and {}",
            a.as_str(),
            b.as_str()
        ))),
    }
}

/// Type-checks every statement in `req` against `env` plus in-request `CREATE`.
///
/// `host_authoritative` skips this layer. Unknown column types fail closed.
///
/// # Errors
///
/// Returns [`PluginError::invalid_params`] when an expression is not in the
/// v1 type contract.
pub fn typecheck_execute_request(
    req: &ExecuteRequest,
    env: &SqlTypeEnv,
    host_authoritative: bool,
) -> Result<()> {
    if host_authoritative {
        return Ok(());
    }
    let mut working = env.clone();
    for (index, stmt) in req.statements.iter().enumerate() {
        typecheck_statement(index, stmt, &mut working)?;
    }
    Ok(())
}

/// Applies one statement's schema effects and expression types to `env`.
fn typecheck_statement(index: usize, stmt: &TypedDbStatement, env: &mut SqlTypeEnv) -> Result<()> {
    if let Some(schema) = parse_create_table_schema(&stmt.sql) {
        env.insert_table(schema.table.clone(), schema.columns.iter().cloned());
        return Ok(());
    }
    if let Some(table) = parse_drop_table_name(&stmt.sql) {
        env.drop_table(&table);
        return Ok(());
    }
    let binds: Vec<SqlType> = stmt.parameters.iter().map(sql_type_of_value).collect();
    let mut cx = TypeCx {
        index,
        env,
        binds: &binds,
        bind_i: 0,
        from: Vec::new(),
        ctes: BTreeMap::new(),
    };
    typecheck_sql(stmt.sql.trim(), &mut cx)
}

fn sql_type_of_value(v: &DbValue) -> SqlType {
    match v {
        DbValue::Int64(_) => SqlType::Integer,
        DbValue::Float64(_) => SqlType::Real,
        DbValue::Text(_) => SqlType::Text,
        DbValue::Bytes(_) => SqlType::Blob,
        DbValue::Boolean(_) => SqlType::Boolean,
        DbValue::Null(ty) => SqlType::from(*ty),
    }
}

struct TypeCx<'a> {
    index: usize,
    env: &'a SqlTypeEnv,
    binds: &'a [SqlType],
    bind_i: usize,
    from: Vec<String>,
    ctes: BTreeMap<String, Vec<(String, SqlType)>>,
}

fn parse_optional_with(scan: &mut TScan<'_>, cx: &mut TypeCx<'_>) -> Result<()> {
    if !scan.take_kw("WITH") {
        return Ok(());
    }
    let _ = scan.take_kw("RECURSIVE");
    loop {
        let name = scan
            .read_ident()
            .ok_or_else(|| ty_err(cx.index, "CTE name"))?;
        if scan.take_byte(b'(') {
            let _ = scan.take_balanced_inner();
        }
        if !scan.take_kw("AS") || !scan.take_byte(b'(') {
            return Err(ty_err(cx.index, "CTE AS ("));
        }
        let inner = scan
            .take_balanced_inner()
            .ok_or_else(|| ty_err(cx.index, "CTE body"))?;
        let cols = typecheck_select_list_only(inner, cx)?;
        cx.ctes.insert(name, cols);
        if !scan.take_byte(b',') {
            break;
        }
    }
    Ok(())
}

fn typecheck_sql(sql: &str, cx: &mut TypeCx<'_>) -> Result<()> {
    let mut scan = TScan {
        sql,
        i: 0,
        took_real: false,
    };
    parse_optional_with(&mut scan, cx)?;
    if scan.peek_kw("SELECT") || scan.peek_kw("VALUES") {
        let _ = infer_select(&mut scan, cx)?;
        return Ok(());
    }
    if scan.take_kw("INSERT") {
        if scan.take_kw("OR") {
            let _ = scan.take_kw("IGNORE");
        }
        if !scan.take_kw("INTO") {
            return Err(ty_err(cx.index, "INSERT INTO"));
        }
        let table = scan
            .read_ident()
            .ok_or_else(|| ty_err(cx.index, "INSERT table"))?;
        if !cx.env.is_empty() && !cx.env.has_table(&table) {
            return Err(PluginError::invalid_params(format!(
                "statement {} names unknown table {table}",
                cx.index
            )));
        }
        cx.from = vec![table.clone()];
        let mut col_types = Vec::new();
        if scan.take_byte(b'(') {
            loop {
                let col = scan
                    .read_ident()
                    .ok_or_else(|| ty_err(cx.index, "INSERT column"))?;
                col_types.push(lookup_column(cx, Some(&table), &col)?);
                if !scan.take_byte(b',') {
                    break;
                }
            }
            if !scan.take_byte(b')') {
                return Err(ty_err(cx.index, "INSERT columns"));
            }
        }
        if scan.peek_kw("VALUES") {
            scan.take_kw("VALUES");
            loop {
                if !scan.take_byte(b'(') {
                    return Err(ty_err(cx.index, "VALUES"));
                }
                let mut i = 0usize;
                loop {
                    let ty = infer_expr(&mut scan, cx)?;
                    if let Some(want) = col_types.get(i) {
                        let _ = unify_types(cx.index, ty, *want)?;
                    }
                    i += 1;
                    if !scan.take_byte(b',') {
                        break;
                    }
                }
                if !scan.take_byte(b')') {
                    return Err(ty_err(cx.index, "VALUES close"));
                }
                if !scan.take_byte(b',') {
                    break;
                }
            }
        } else {
            parse_optional_with(&mut scan, cx)?;
            let _ = infer_select(&mut scan, cx)?;
        }
        if scan.take_kw("RETURNING") {
            loop {
                if scan.take_byte(b'*') {
                    break;
                }
                let _ = infer_expr(&mut scan, cx)?;
                let _ = scan.take_kw("AS");
                let _ = scan.read_ident();
                if !scan.take_byte(b',') {
                    break;
                }
            }
        }
        return Ok(());
    }
    if scan.take_kw("UPDATE") {
        let table = scan
            .read_ident()
            .ok_or_else(|| ty_err(cx.index, "UPDATE table"))?;
        if !cx.env.is_empty() && !cx.env.has_table(&table) {
            return Err(PluginError::invalid_params(format!(
                "statement {} names unknown table {table}",
                cx.index
            )));
        }
        cx.from = vec![table];
        if !scan.take_kw("SET") {
            return Err(ty_err(cx.index, "UPDATE SET"));
        }
        loop {
            let _ = scan.read_ident();
            if !scan.take_byte(b'=') {
                return Err(ty_err(cx.index, "UPDATE ="));
            }
            let _ = infer_expr(&mut scan, cx)?;
            if !scan.take_byte(b',') {
                break;
            }
        }
        if scan.take_kw("WHERE") {
            let _ = infer_expr(&mut scan, cx)?;
        }
        if scan.take_kw("RETURNING") {
            loop {
                if scan.take_byte(b'*') {
                    break;
                }
                let _ = infer_expr(&mut scan, cx)?;
                if !scan.take_byte(b',') {
                    break;
                }
            }
        }
        return Ok(());
    }
    if scan.take_kw("DELETE") {
        let _ = scan.take_kw("FROM");
        let table = scan
            .read_ident()
            .ok_or_else(|| ty_err(cx.index, "DELETE table"))?;
        if !cx.env.is_empty() && !cx.env.has_table(&table) {
            return Err(PluginError::invalid_params(format!(
                "statement {} names unknown table {table}",
                cx.index
            )));
        }
        cx.from = vec![table];
        if scan.take_kw("WHERE") {
            let _ = infer_expr(&mut scan, cx)?;
        }
        return Ok(());
    }
    if scan.take_kw("CREATE") || scan.take_kw("DROP") {
        return Ok(());
    }
    Ok(())
}

fn typecheck_select_list_only(sql: &str, cx: &mut TypeCx<'_>) -> Result<Vec<(String, SqlType)>> {
    let mut scan = TScan {
        sql,
        i: 0,
        took_real: false,
    };
    infer_select(&mut scan, cx)
}

fn infer_select(scan: &mut TScan<'_>, cx: &mut TypeCx<'_>) -> Result<Vec<(String, SqlType)>> {
    let mut cols = infer_select_core(scan, cx)?;
    while scan.take_kw("UNION") {
        let _ = scan.take_kw("ALL");
        let other = infer_select_core(scan, cx)?;
        if cols.len() != other.len() {
            return Err(ty_err(cx.index, "UNION column counts differ"));
        }
        for (i, ((n, a), (_, b))) in cols.iter_mut().zip(other.iter()).enumerate() {
            let u = unify_types(cx.index, *a, *b)?;
            *n = format!("c{i}");
            *a = u;
        }
    }
    Ok(cols)
}

fn infer_select_core(scan: &mut TScan<'_>, cx: &mut TypeCx<'_>) -> Result<Vec<(String, SqlType)>> {
    if scan.take_kw("VALUES") {
        let mut cols = Vec::new();
        if !scan.take_byte(b'(') {
            return Err(ty_err(cx.index, "VALUES tuple"));
        }
        let mut i = 0;
        loop {
            let ty = infer_expr(scan, cx)?;
            cols.push((format!("c{i}"), ty));
            i += 1;
            if !scan.take_byte(b',') {
                break;
            }
        }
        if !scan.take_byte(b')') {
            return Err(ty_err(cx.index, "VALUES close"));
        }
        return Ok(cols);
    }
    if !scan.take_kw("SELECT") {
        return Err(ty_err(cx.index, "SELECT"));
    }
    let _ = scan.take_kw("DISTINCT");
    lookahead_from(scan, cx);
    let mut cols = Vec::new();
    loop {
        if scan.take_byte(b'*') {
            cols.extend(star_columns(cx)?);
        } else {
            let ty = infer_expr(scan, cx)?;
            let name = select_item_alias(scan, cols.len());
            cols.push((name, ty));
        }
        if !scan.take_byte(b',') {
            break;
        }
    }
    if scan.take_kw("FROM") {
        cx.from = Vec::new();
        loop {
            take_from_item(scan, cx)?;
            if scan.take_kw("JOIN")
                || scan.take_kw("INNER")
                || scan.take_kw("LEFT")
                || scan.take_kw("CROSS")
            {
                let _ = scan.take_kw("OUTER");
                let _ = scan.take_kw("JOIN");
                take_from_item(scan, cx)?;
                if scan.take_kw("ON") {
                    let _ = infer_expr(scan, cx)?;
                }
                continue;
            }
            break;
        }
    }
    if scan.take_kw("WHERE") {
        let _ = infer_expr(scan, cx)?;
    }
    if scan.take_kw("GROUP") {
        let _ = scan.take_kw("BY");
        loop {
            let _ = infer_expr(scan, cx)?;
            if !scan.take_byte(b',') {
                break;
            }
        }
    }
    if scan.take_kw("HAVING") {
        let _ = infer_expr(scan, cx)?;
    }
    if scan.take_kw("ORDER") {
        let _ = scan.take_kw("BY");
        loop {
            let _ = infer_expr(scan, cx)?;
            let _ = scan.take_kw("ASC");
            let _ = scan.take_kw("DESC");
            if scan.take_kw("NULLS") {
                let _ = scan.take_kw("FIRST");
                let _ = scan.take_kw("LAST");
            }
            if !scan.take_byte(b',') {
                break;
            }
        }
    }
    if scan.take_kw("LIMIT") {
        let _ = infer_expr(scan, cx)?;
    }
    if scan.take_kw("OFFSET") {
        let _ = infer_expr(scan, cx)?;
    }
    Ok(cols)
}

fn select_item_alias(scan: &mut TScan<'_>, i: usize) -> String {
    if scan.take_kw("AS") {
        return scan.read_ident().unwrap_or_else(|| format!("c{i}"));
    }
    if scan.peek_select_clause_kw() {
        return format!("c{i}");
    }
    scan.read_ident().unwrap_or_else(|| format!("c{i}"))
}

fn lookahead_from(scan: &TScan<'_>, cx: &mut TypeCx<'_>) {
    let mut ahead = TScan {
        sql: scan.sql,
        i: scan.i,
        took_real: false,
    };
    let mut depth = 0i32;
    while ahead.i < ahead.sql.len() {
        ahead.skip();
        if ahead.i >= ahead.sql.len() {
            break;
        }
        if ahead.sql.as_bytes()[ahead.i] == b'\'' {
            let _ = ahead.take_string();
            continue;
        }
        if ahead.take_byte(b'(') {
            depth += 1;
            continue;
        }
        if ahead.take_byte(b')') {
            depth -= 1;
            continue;
        }
        if depth == 0 && ahead.take_kw("FROM") {
            let mut tmp = TScan {
                sql: ahead.sql,
                i: ahead.i,
                took_real: false,
            };
            let saved_from = cx.from.clone();
            cx.from.clear();
            loop {
                if tmp.take_byte(b'(') {
                    let _ = tmp.take_balanced_inner();
                    let _ = tmp.take_kw("AS");
                    let alias = tmp.read_ident();
                    if let Some(alias) = alias {
                        cx.from.push(alias);
                    }
                } else if let Some(name) = tmp.read_ident() {
                    cx.from.push(name);
                    if tmp.take_kw("AS") {
                        let _ = tmp.read_ident();
                    }
                } else {
                    break;
                }
                if tmp.take_kw("JOIN")
                    || tmp.take_kw("INNER")
                    || tmp.take_kw("LEFT")
                    || tmp.take_kw("CROSS")
                {
                    let _ = tmp.take_kw("OUTER");
                    let _ = tmp.take_kw("JOIN");
                    continue;
                }
                if tmp.take_byte(b',') {
                    continue;
                }
                break;
            }
            if cx.from.is_empty() {
                cx.from = saved_from;
            }
            return;
        }
        let ch = ahead.sql[ahead.i..].chars().next().unwrap_or('\0');
        ahead.i += ch.len_utf8();
    }
}

fn star_columns(cx: &TypeCx<'_>) -> Result<Vec<(String, SqlType)>> {
    let mut out = Vec::new();
    for table in &cx.from {
        if let Some(cols) = cx.ctes.get(table) {
            out.extend(cols.clone());
            continue;
        }
        if let Some(cols) = cx.env.tables.get(table) {
            out.extend(cols.iter().map(|(n, t)| (n.clone(), *t)));
            continue;
        }
        if cx.env.is_empty() {
            return Ok(Vec::new());
        }
        return Err(PluginError::invalid_params(format!(
            "statement {} names unknown table {table}",
            cx.index
        )));
    }
    Ok(out)
}

fn take_from_item(scan: &mut TScan<'_>, cx: &mut TypeCx<'_>) -> Result<()> {
    if scan.take_byte(b'(') {
        let inner = scan
            .take_balanced_inner()
            .ok_or_else(|| ty_err(cx.index, "FROM subquery"))?;
        let cols = typecheck_select_list_only(inner, cx)?;
        let _ = scan.take_kw("AS");
        let alias = scan
            .read_ident()
            .unwrap_or_else(|| format!("_sub{}", cx.from.len()));
        cx.ctes.insert(alias.clone(), cols);
        cx.from.push(alias);
        return Ok(());
    }
    let name = scan
        .read_ident()
        .ok_or_else(|| ty_err(cx.index, "FROM table"))?;
    cx.from.push(name);
    if scan.take_kw("AS")
        || !(scan.peek_kw("JOIN")
            || scan.peek_kw("INNER")
            || scan.peek_kw("LEFT")
            || scan.peek_kw("CROSS")
            || scan.peek_kw("ON")
            || scan.peek_kw("WHERE")
            || scan.peek_kw("GROUP")
            || scan.peek_kw("HAVING")
            || scan.peek_kw("ORDER")
            || scan.peek_kw("LIMIT")
            || scan.peek_kw("UNION")
            || scan.peek_byte(b','))
    {
        let _ = scan.read_ident();
    }
    Ok(())
}

fn infer_expr(scan: &mut TScan<'_>, cx: &mut TypeCx<'_>) -> Result<SqlType> {
    infer_or(scan, cx)
}

fn infer_or(scan: &mut TScan<'_>, cx: &mut TypeCx<'_>) -> Result<SqlType> {
    let mut t = infer_and(scan, cx)?;
    while scan.take_kw("OR") {
        let r = infer_and(scan, cx)?;
        let _ = unify_types(cx.index, t, r)?;
        t = SqlType::Boolean;
    }
    Ok(t)
}

fn infer_and(scan: &mut TScan<'_>, cx: &mut TypeCx<'_>) -> Result<SqlType> {
    let mut t = infer_not(scan, cx)?;
    while scan.take_kw("AND") {
        let r = infer_not(scan, cx)?;
        let _ = unify_types(cx.index, t, r)?;
        t = SqlType::Boolean;
    }
    Ok(t)
}

fn infer_not(scan: &mut TScan<'_>, cx: &mut TypeCx<'_>) -> Result<SqlType> {
    if scan.take_kw("NOT") {
        let _ = infer_not(scan, cx)?;
        return Ok(SqlType::Boolean);
    }
    infer_cmp(scan, cx)
}

fn infer_cmp(scan: &mut TScan<'_>, cx: &mut TypeCx<'_>) -> Result<SqlType> {
    let left = infer_concat(scan, cx)?;
    if scan.take_kw("IS") {
        let _ = scan.take_kw("NOT");
        if !scan.take_kw("NULL") {
            return Err(ty_err(cx.index, "IS NULL"));
        }
        return Ok(SqlType::Boolean);
    }
    if scan.take_kw("NOT") {
        if scan.take_kw("LIKE") {
            let right = infer_concat(scan, cx)?;
            require_textish(cx.index, left)?;
            require_textish(cx.index, right)?;
            return Ok(SqlType::Boolean);
        }
        if scan.take_kw("IN") {
            infer_in_list(scan, cx, left)?;
            return Ok(SqlType::Boolean);
        }
        if scan.take_kw("BETWEEN") {
            let a = infer_concat(scan, cx)?;
            if !scan.take_kw("AND") {
                return Err(ty_err(cx.index, "BETWEEN AND"));
            }
            let b = infer_concat(scan, cx)?;
            let _ = unify_types(cx.index, left, a)?;
            let _ = unify_types(cx.index, left, b)?;
            return Ok(SqlType::Boolean);
        }
        return Err(ty_err(cx.index, "NOT LIKE/IN/BETWEEN"));
    }
    if scan.take_kw("LIKE") {
        let right = infer_concat(scan, cx)?;
        require_textish(cx.index, left)?;
        require_textish(cx.index, right)?;
        return Ok(SqlType::Boolean);
    }
    if scan.take_kw("IN") {
        infer_in_list(scan, cx, left)?;
        return Ok(SqlType::Boolean);
    }
    if scan.take_kw("BETWEEN") {
        let a = infer_concat(scan, cx)?;
        if !scan.take_kw("AND") {
            return Err(ty_err(cx.index, "BETWEEN AND"));
        }
        let b = infer_concat(scan, cx)?;
        let _ = unify_types(cx.index, left, a)?;
        let _ = unify_types(cx.index, left, b)?;
        return Ok(SqlType::Boolean);
    }
    for op in ["!=", "<>", "<=", ">=", "=", "<", ">"] {
        if scan.take_op(op) {
            let right = infer_concat(scan, cx)?;
            let _ = unify_types(cx.index, left, right)?;
            return Ok(SqlType::Boolean);
        }
    }
    Ok(left)
}

fn infer_in_list(scan: &mut TScan<'_>, cx: &mut TypeCx<'_>, left: SqlType) -> Result<()> {
    if !scan.take_byte(b'(') {
        return Err(ty_err(cx.index, "IN ("));
    }
    if scan.peek_kw("SELECT") || scan.peek_kw("WITH") {
        let inner = scan.take_balanced_inner().unwrap_or("");
        let mut body = TScan {
            sql: inner,
            i: 0,
            took_real: false,
        };
        let cols = infer_select(&mut body, cx)?;
        if let Some((_, ty)) = cols.first() {
            let _ = unify_types(cx.index, left, *ty)?;
        }
        return Ok(());
    }
    loop {
        let t = infer_expr(scan, cx)?;
        let _ = unify_types(cx.index, left, t)?;
        if !scan.take_byte(b',') {
            break;
        }
    }
    if !scan.take_byte(b')') {
        return Err(ty_err(cx.index, "IN )"));
    }
    Ok(())
}

fn infer_concat(scan: &mut TScan<'_>, cx: &mut TypeCx<'_>) -> Result<SqlType> {
    let mut t = infer_add(scan, cx)?;
    while scan.take_op("||") {
        let r = infer_add(scan, cx)?;
        require_textish(cx.index, t)?;
        require_textish(cx.index, r)?;
        t = SqlType::Text;
    }
    Ok(t)
}

fn infer_add(scan: &mut TScan<'_>, cx: &mut TypeCx<'_>) -> Result<SqlType> {
    let mut t = infer_mul(scan, cx)?;
    loop {
        if scan.take_op("+") || scan.take_op("-") {
            let r = infer_mul(scan, cx)?;
            t = unify_numeric(cx.index, t, r)?;
        } else {
            break;
        }
    }
    Ok(t)
}

fn infer_mul(scan: &mut TScan<'_>, cx: &mut TypeCx<'_>) -> Result<SqlType> {
    let mut t = infer_prefix(scan, cx)?;
    loop {
        if scan.take_op("*") || scan.take_op("/") || scan.take_op("%") {
            let r = infer_prefix(scan, cx)?;
            t = unify_numeric(cx.index, t, r)?;
        } else {
            break;
        }
    }
    Ok(t)
}

fn infer_prefix(scan: &mut TScan<'_>, cx: &mut TypeCx<'_>) -> Result<SqlType> {
    if scan.take_op("+") || scan.take_op("-") {
        let t = infer_prefix(scan, cx)?;
        if t != SqlType::Null && !t.is_numeric() {
            return Err(ty_err(cx.index, "unary +/- requires a numeric type"));
        }
        return Ok(t);
    }
    infer_atom(scan, cx)
}

fn infer_atom(scan: &mut TScan<'_>, cx: &mut TypeCx<'_>) -> Result<SqlType> {
    if scan.take_kw("NULL") {
        return Ok(SqlType::Null);
    }
    if scan.take_kw("TRUE") || scan.take_kw("FALSE") {
        return Ok(SqlType::Boolean);
    }
    if scan.take_kw("CAST") {
        if !scan.take_byte(b'(') {
            return Err(ty_err(cx.index, "CAST ("));
        }
        let from = infer_expr(scan, cx)?;
        if !scan.take_kw("AS") {
            return Err(ty_err(cx.index, "CAST AS"));
        }
        let ty_ident = scan
            .read_ident()
            .ok_or_else(|| ty_err(cx.index, "CAST type"))?;
        let to =
            SqlType::from_column_ident(&ty_ident).ok_or_else(|| ty_err(cx.index, "CAST type"))?;
        if !scan.take_byte(b')') {
            return Err(ty_err(cx.index, "CAST )"));
        }
        if !cast_is_legal(from, to) {
            return Err(PluginError::invalid_params(format!(
                "statement {} CAST from {} to {} is not SQL v1",
                cx.index,
                from.as_str(),
                to.as_str()
            )));
        }
        return Ok(to);
    }
    if scan.take_kw("CASE") {
        return infer_case(scan, cx);
    }
    if scan.take_byte(b'?') {
        let ty = cx.binds.get(cx.bind_i).copied().unwrap_or(SqlType::Null);
        cx.bind_i += 1;
        return Ok(ty);
    }
    if scan.take_blob_hex() {
        return Ok(SqlType::Blob);
    }
    if scan.take_string() {
        return Ok(SqlType::Text);
    }
    if scan.take_number() {
        return Ok(if scan.took_real {
            SqlType::Real
        } else {
            SqlType::Integer
        });
    }
    if scan.take_byte(b'(') {
        let inner = scan
            .take_balanced_inner()
            .ok_or_else(|| ty_err(cx.index, "parenthesized expr"))?;
        let mut body = TScan {
            sql: inner,
            i: 0,
            took_real: false,
        };
        if body.peek_kw("SELECT") || body.peek_kw("WITH") || body.peek_kw("VALUES") {
            let cols = infer_select(&mut body, cx)?;
            return Ok(cols.first().map(|(_, t)| *t).unwrap_or(SqlType::Null));
        }
        return infer_expr(&mut body, cx);
    }
    let Some(name) = scan.read_ident() else {
        return Err(ty_err(cx.index, "expression atom"));
    };
    if scan.take_byte(b'(') {
        return infer_call(scan, cx, &name);
    }
    if scan.take_byte(b'.') {
        let col = scan
            .read_ident()
            .ok_or_else(|| ty_err(cx.index, "qualified column"))?;
        return lookup_column(cx, Some(&name), &col);
    }
    lookup_column(cx, None, &name)
}

fn infer_call(scan: &mut TScan<'_>, cx: &mut TypeCx<'_>, name: &str) -> Result<SqlType> {
    let mut args = Vec::new();
    if !scan.peek_byte(b')') {
        if name == "count" && scan.take_byte(b'*') {
            args.push(SqlType::Integer);
        } else {
            loop {
                args.push(infer_expr(scan, cx)?);
                if !scan.take_byte(b',') {
                    break;
                }
            }
        }
    }
    if !scan.take_byte(b')') {
        return Err(ty_err(cx.index, "call )"));
    }
    match name {
        "ifnull" | "coalesce" | "nullif" => {
            let mut t = SqlType::Null;
            for a in args {
                t = unify_types(cx.index, t, a)?;
            }
            Ok(t)
        }
        "min" | "max" if args.len() >= 2 => {
            let mut t = SqlType::Null;
            for a in args {
                t = unify_types(cx.index, t, a)?;
            }
            Ok(t)
        }
        "min" | "max" if args.len() == 1 => Ok(args[0]),
        "sum" => {
            let t = args.first().copied().unwrap_or(SqlType::Null);
            if t != SqlType::Null && t != SqlType::Integer {
                return Err(PluginError::invalid_params(format!(
                    "statement {} sum() requires INTEGER (REAL-sum is not SQL v1)",
                    cx.index
                )));
            }
            Ok(SqlType::Integer)
        }
        "avg" | "round" | "abs" => {
            let t = args.first().copied().unwrap_or(SqlType::Null);
            if t != SqlType::Null && !t.is_numeric() {
                return Err(ty_err(cx.index, "numeric helper requires INTEGER or REAL"));
            }
            if name == "round" || name == "avg" {
                Ok(SqlType::Real)
            } else {
                Ok(if t == SqlType::Null {
                    SqlType::Integer
                } else {
                    t
                })
            }
        }
        "count" => Ok(SqlType::Integer),
        "lower" | "upper" | "trim" | "replace" | "substr" => {
            for a in args.iter().take(1) {
                require_textish(cx.index, *a)?;
            }
            Ok(SqlType::Text)
        }
        "length" => {
            let t = args.first().copied().unwrap_or(SqlType::Null);
            if t != SqlType::Null && t != SqlType::Text && t != SqlType::Blob {
                return Err(ty_err(cx.index, "length() requires TEXT or BLOB"));
            }
            Ok(SqlType::Integer)
        }
        "json_extract" | "json_object" => Ok(SqlType::Text),
        "json_valid" => Ok(SqlType::Integer),
        _ => Ok(SqlType::Null),
    }
}

fn infer_case(scan: &mut TScan<'_>, cx: &mut TypeCx<'_>) -> Result<SqlType> {
    if !scan.peek_kw("WHEN") {
        let _ = infer_expr(scan, cx)?;
    }
    let mut result = SqlType::Null;
    while scan.take_kw("WHEN") {
        let _ = infer_expr(scan, cx)?;
        if !scan.take_kw("THEN") {
            return Err(ty_err(cx.index, "CASE THEN"));
        }
        let t = infer_expr(scan, cx)?;
        result = unify_types(cx.index, result, t)?;
    }
    if scan.take_kw("ELSE") {
        let t = infer_expr(scan, cx)?;
        result = unify_types(cx.index, result, t)?;
    }
    if !scan.take_kw("END") {
        return Err(ty_err(cx.index, "CASE END"));
    }
    Ok(result)
}

fn lookup_column(cx: &TypeCx<'_>, table: Option<&str>, column: &str) -> Result<SqlType> {
    if let Some(table) = table {
        if let Some(cols) = cx.ctes.get(table) {
            if let Some((_, ty)) = cols.iter().find(|(n, _)| n == column) {
                return Ok(*ty);
            }
        }
        if let Some(ty) = cx.env.column_type(table, column) {
            return Ok(ty);
        }
        return unknown_column(cx, Some(table), column);
    }
    for cte in cx.ctes.values() {
        if let Some((_, ty)) = cte.iter().find(|(n, _)| n == column) {
            return Ok(*ty);
        }
    }
    if cx.from.len() == 1 {
        return lookup_column(cx, Some(&cx.from[0]), column);
    }
    let mut found = None;
    for table in &cx.from {
        if let Some(ty) = cx.env.column_type(table, column) {
            if found.is_some() {
                return Err(PluginError::invalid_params(format!(
                    "statement {} unqualified column {column} is ambiguous",
                    cx.index
                )));
            }
            found = Some(ty);
        }
    }
    if let Some(ty) = found {
        return Ok(ty);
    }
    unknown_column(cx, None, column)
}

fn unknown_column(cx: &TypeCx<'_>, table: Option<&str>, column: &str) -> Result<SqlType> {
    if cx.env.is_empty() {
        return Ok(SqlType::Null);
    }
    let msg = match table {
        Some(table) => format!(
            "statement {} names unknown column {table}.{column}",
            cx.index
        ),
        None => format!("statement {} names unknown column {column}", cx.index),
    };
    Err(PluginError::invalid_params(msg))
}

fn require_textish(index: usize, ty: SqlType) -> Result<()> {
    if ty == SqlType::Null || ty == SqlType::Text {
        Ok(())
    } else {
        Err(PluginError::invalid_params(format!(
            "statement {index} TEXT operator applied to {}",
            ty.as_str()
        )))
    }
}

fn unify_numeric(index: usize, a: SqlType, b: SqlType) -> Result<SqlType> {
    let t = unify_types(index, a, b)?;
    if t != SqlType::Null && !t.is_numeric() {
        return Err(PluginError::invalid_params(format!(
            "statement {index} arithmetic requires INTEGER or REAL"
        )));
    }
    Ok(t)
}

fn ty_err(index: usize, msg: &str) -> PluginError {
    PluginError::invalid_params(format!("statement {index} {msg}"))
}

fn column_is_autoincrement(rest: &str) -> bool {
    let u = rest.to_ascii_uppercase();
    u.contains("PRIMARY") && u.contains("KEY") && u.contains("AUTOINCREMENT")
}

fn escape_sql_str(s: &str) -> String {
    s.replace('\'', "''")
}

fn skip_ws(s: &str) -> &str {
    s.trim_start()
}

fn starts_kw(s: &str, kw: &str) -> bool {
    let s = skip_ws(s);
    if s.len() < kw.len() || !s.is_char_boundary(kw.len()) {
        return false;
    }
    s[..kw.len()].eq_ignore_ascii_case(kw)
        && s.as_bytes()
            .get(kw.len())
            .is_none_or(|b| !b.is_ascii_alphanumeric() && *b != b'_')
}

fn skip_kw<'a>(s: &'a str, kw: &str) -> Option<&'a str> {
    let s = skip_ws(s);
    if !starts_kw(s, kw) {
        return None;
    }
    Some(s[kw.len()..].trim_start())
}

fn read_ident(s: &str) -> Option<(String, &str)> {
    let s = skip_ws(s);
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
    Some((s[..n].to_ascii_lowercase(), &s[n..]))
}

fn balanced_inner(s: &str) -> Option<&str> {
    if !s.starts_with('(') {
        return None;
    }
    let mut depth = 0i32;
    let mut i = 0;
    while i < s.len() {
        let b = s.as_bytes()[i];
        if b == b'\'' {
            i += 1;
            while i < s.len() {
                if s.as_bytes()[i] == b'\'' {
                    if s.as_bytes().get(i + 1) == Some(&b'\'') {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if b == b'(' {
            depth += 1;
        } else if b == b')' {
            depth -= 1;
            if depth == 0 {
                return Some(&s[1..i]);
            }
        }
        i += 1;
    }
    None
}

fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    let mut i = 0;
    while i < s.len() {
        let b = s.as_bytes()[i];
        if b == b'\'' {
            i += 1;
            while i < s.len() {
                if s.as_bytes()[i] == b'\'' {
                    if s.as_bytes().get(i + 1) == Some(&b'\'') {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if b == b'(' {
            depth += 1;
        } else if b == b')' {
            depth -= 1;
        } else if b == b',' && depth == 0 {
            out.push(&s[start..i]);
            start = i + 1;
        }
        i += 1;
    }
    out.push(&s[start..]);
    out
}

struct TScan<'a> {
    sql: &'a str,
    i: usize,
    took_real: bool,
}

impl<'a> TScan<'a> {
    fn skip(&mut self) {
        while self.i < self.sql.len() {
            let rest = &self.sql[self.i..];
            if rest.starts_with("--") {
                self.i += rest.find('\n').unwrap_or(rest.len());
                continue;
            }
            if rest.starts_with("/*") {
                self.i += rest.find("*/").map(|p| p + 2).unwrap_or(rest.len());
                continue;
            }
            let Some(ch) = rest.chars().next() else {
                break;
            };
            if ch.is_whitespace() {
                self.i += ch.len_utf8();
                continue;
            }
            break;
        }
    }

    fn take_kw(&mut self, kw: &str) -> bool {
        self.skip();
        if starts_kw(&self.sql[self.i..], kw) {
            self.i += kw.len();
            true
        } else {
            false
        }
    }

    fn peek_kw(&mut self, kw: &str) -> bool {
        self.skip();
        starts_kw(&self.sql[self.i..], kw)
    }

    fn peek_select_clause_kw(&mut self) -> bool {
        [
            "FROM",
            "WHERE",
            "GROUP",
            "HAVING",
            "ORDER",
            "LIMIT",
            "OFFSET",
            "UNION",
            "RETURNING",
        ]
        .iter()
        .any(|kw| self.peek_kw(kw))
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

    fn take_op(&mut self, op: &str) -> bool {
        self.skip();
        if self.sql[self.i..].starts_with(op) {
            let after = self.i + op.len();
            if op.chars().all(|c| c.is_ascii_alphabetic()) {
                return false;
            }
            self.i = after;
            true
        } else {
            false
        }
    }

    fn read_ident(&mut self) -> Option<String> {
        self.skip();
        let (name, rest) = read_ident(&self.sql[self.i..])?;
        self.i = self.sql.len() - rest.len();
        Some(name)
    }

    fn take_balanced_inner(&mut self) -> Option<&'a str> {
        self.skip();
        let start = self.i.saturating_sub(1);
        if start >= self.sql.len() || self.sql.as_bytes().get(start) != Some(&b'(') {
            let s = skip_ws(&self.sql[self.i.saturating_sub(1).min(self.sql.len())..]);
            let _ = s;
        }
        let from = if self.i > 0 && self.sql.as_bytes()[self.i - 1] == b'(' {
            self.i - 1
        } else {
            return None;
        };
        let inner = balanced_inner(&self.sql[from..])?;
        self.i = from + 1 + inner.len() + 1;
        Some(inner)
    }

    fn take_string(&mut self) -> bool {
        self.skip();
        if self.sql.as_bytes().get(self.i) != Some(&b'\'') {
            return false;
        }
        self.i += 1;
        while self.i < self.sql.len() {
            if self.sql.as_bytes()[self.i] == b'\'' {
                if self.sql.as_bytes().get(self.i + 1) == Some(&b'\'') {
                    self.i += 2;
                    continue;
                }
                self.i += 1;
                return true;
            }
            self.i += 1;
        }
        true
    }

    fn take_blob_hex(&mut self) -> bool {
        self.skip();
        let rest = &self.sql[self.i..];
        if rest.len() < 3 {
            return false;
        }
        if !rest.as_bytes()[0].eq_ignore_ascii_case(&b'x') || rest.as_bytes()[1] != b'\'' {
            return false;
        }
        self.i += 2;
        while self.i < self.sql.len() {
            let b = self.sql.as_bytes()[self.i];
            if b == b'\'' {
                self.i += 1;
                return true;
            }
            if !b.is_ascii_hexdigit() {
                return false;
            }
            self.i += 1;
        }
        false
    }

    fn take_number(&mut self) -> bool {
        self.skip();
        self.took_real = false;
        let bytes = self.sql.as_bytes();
        let mut i = self.i;
        if i >= bytes.len() {
            return false;
        }
        if bytes[i] == b'+' || bytes[i] == b'-' {
            i += 1;
        }
        let start = i;
        let mut saw = false;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            saw = true;
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'.' {
            self.took_real = true;
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                saw = true;
                i += 1;
            }
        }
        if !saw || i == start {
            return false;
        }
        self.i = i;
        true
    }
}

#[cfg(test)]
#[allow(clippy::missing_panics_doc)]
mod tests {
    use super::*;
    use crate::{DbPlanStatementKind, DbResultSelection};

    fn stmt(sql: &str) -> TypedDbStatement {
        TypedDbStatement {
            sql: sql.into(),
            parameters: Vec::new(),
            kind: DbPlanStatementKind::Select,
            max_rows: 0,
            result_selection: DbResultSelection::Rows,
        }
    }

    fn req(sql: &str) -> ExecuteRequest {
        ExecuteRequest {
            operation_id: "t".into(),
            request_hash: String::new(),
            statements: vec![stmt(sql)],
            deadline_unix_ms: 0,
        }
    }

    #[test]
    fn parse_identity_column_is_not_always_id() {
        let schema = parse_create_table_schema(
            "CREATE TABLE IF NOT EXISTS t (pk INTEGER PRIMARY KEY AUTOINCREMENT, n INTEGER)",
        )
        .expect("schema");
        assert_eq!(schema.identity_column.as_deref(), Some("pk"));
        let lowered = parse_create_table_schema(
            "CREATE TABLE IF NOT EXISTS t (id BIGINT PRIMARY KEY, body TEXT, blob BYTEA, r DOUBLE PRECISION)",
        )
        .expect("postgres-lowered schema");
        assert_eq!(
            lowered.columns,
            vec![
                ("id".into(), SqlType::Integer),
                ("body".into(), SqlType::Text),
                ("blob".into(), SqlType::Blob),
                ("r".into(), SqlType::Real),
            ]
        );
    }

    #[test]
    fn cast_matrix_rejects_text_to_integer() {
        assert!(!cast_is_legal(SqlType::Text, SqlType::Integer));
        assert!(cast_is_legal(SqlType::Integer, SqlType::Real));
        assert!(cast_is_legal(SqlType::Null, SqlType::Boolean));
    }

    #[test]
    fn ifnull_mixed_literals_fail_closed() {
        let err =
            typecheck_execute_request(&req("SELECT IFNULL('x', 0)"), &SqlTypeEnv::new(), false)
                .unwrap_err();
        assert!(err.to_string().contains("incompatible"), "{err}");
    }

    #[test]
    fn union_mixed_types_fail_closed() {
        let err = typecheck_execute_request(
            &req("SELECT 1 UNION ALL SELECT 'x'"),
            &SqlTypeEnv::new(),
            false,
        )
        .unwrap_err();
        assert!(err.to_string().contains("incompatible"), "{err}");
    }

    #[test]
    fn catalog_ddl_round_trip_and_unknown_column() {
        let mut env = SqlTypeEnv::new();
        apply_schema_sql_to_env(
            &mut env,
            "CREATE TABLE IF NOT EXISTS typed (n INTEGER, body TEXT)",
        );
        assert_eq!(env.column_type("typed", "body"), Some(SqlType::Text));
        let err =
            typecheck_execute_request(&req("SELECT missing FROM typed"), &env, false).unwrap_err();
        assert!(err.to_string().contains("unknown column"), "{err}");
        let from_ddl = sql_type_env_from_canonical_ddl(
            "CREATE TABLE a (id INTEGER PRIMARY KEY); CREATE TABLE b (body TEXT);",
        );
        assert_eq!(from_ddl.column_type("a", "id"), Some(SqlType::Integer));
        assert_eq!(from_ddl.column_type("b", "body"), Some(SqlType::Text));
    }

    #[test]
    fn insert_with_source_typechecks() {
        typecheck_execute_request(
            &req("INSERT OR IGNORE INTO ign_sel (id) WITH s AS (SELECT 1) SELECT * FROM s"),
            &SqlTypeEnv::new(),
            false,
        )
        .expect("INSERT … WITH");
    }

    #[test]
    fn sum_real_without_cast_fails_closed() {
        let mut env = SqlTypeEnv::new();
        apply_schema_sql_to_env(&mut env, "CREATE TABLE IF NOT EXISTS typed (r REAL)");
        let err =
            typecheck_execute_request(&req("SELECT sum(r) FROM typed"), &env, false).unwrap_err();
        assert!(err.to_string().contains("sum()"), "{err}");
    }
}
