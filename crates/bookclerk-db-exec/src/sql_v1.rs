//! Shared Bookclerk SQL v1 execution vectors (binding DDL + portable helpers).
//!
//! Hosts emit these canonical statements. Adapters lower at execute: SQLite/D1
//! run them verbatim; Postgres applies binding DDL type rewrites and helper
//! lowering. Every name in `GuestSqlPolicy::binding_owned` portable functions
//! (except denied `hex`) appears here.

use bookclerk_plugin_abi::{DbColumn, DbType, DbValue, StatementResult};

/// Binding `CREATE TABLE` covering identity + `BLOB`/`INTEGER`/`REAL`.
pub const BINDING_DDL_AUTOINCREMENT_BLOB: &str = "CREATE TABLE IF NOT EXISTS typed (\
     id INTEGER PRIMARY KEY AUTOINCREMENT, \
     n INTEGER, body TEXT, payload TEXT, blob BLOB, r REAL)";

/// Binding `CREATE TABLE` covering canonical `BOOLEAN` (not INTEGER-as-bool).
pub const BINDING_DDL_BOOLEAN: &str = "CREATE TABLE IF NOT EXISTS flags (\
     id INTEGER PRIMARY KEY AUTOINCREMENT, \
     flag BOOLEAN, \
     missing BOOLEAN)";

/// INSERT [`DbValue::Boolean`] + typed-null into [`BINDING_DDL_BOOLEAN`].
pub const PORTABLE_BOOLEAN_INSERT: &str = "INSERT INTO flags (flag, missing) VALUES (?, ?)";

/// SELECT declared boolean columns from [`BINDING_DDL_BOOLEAN`].
pub const PORTABLE_BOOLEAN_SELECT: &str = "SELECT flag, missing FROM flags";

/// Lowercase / mixed-case DDL admitted by SQL v1 (idents fold) and lowered
/// case-insensitively on Postgres.
pub const BINDING_DDL_LOWERCASE: &str = "create table if not exists typed_lc (\
     id integer primary key autoincrement, \
     blob blob, flag boolean)";

/// Lowercase `INSERT OR IGNORE … RETURNING` against [`BINDING_DDL_LOWERCASE`].
pub const PORTABLE_INSERT_OR_IGNORE_RETURNING_LC: &str =
    "insert or ignore into typed_lc (blob, flag) values (?, ?) returning id";

/// SELECT stored blob + boolean from [`BINDING_DDL_LOWERCASE`].
pub const PORTABLE_LOWERCASE_SELECT: &str = "SELECT blob, flag FROM typed_lc";

/// Seed row for [`PORTABLE_SELECT`]. Bind [`PORTABLE_INSERT_BLOB`] at `?`.
pub const PORTABLE_INSERT: &str =
    "INSERT INTO typed (n, body, payload, blob, r) VALUES (2, 'Ab', '{\"k\":\"v\"}', ?, 1.5)";

/// Blob payload for [`PORTABLE_INSERT`].
pub const PORTABLE_INSERT_BLOB: &[u8] = &[1, 2, 3];

/// Scalar portable helpers (no aggregates).
///
/// Column order matches [`portable_select_expects`]. `json_object` is wrapped
/// in `json_extract` so Postgres `json_build_object` still yields text.
/// Aggregates live in [`PORTABLE_AGGREGATE_SELECT`]: a mixed aggregate +
/// non-aggregate SELECT list is SQLite-shaped, not Bookclerk SQL v1
/// (PostgreSQL requires `GROUP BY`). `round`/`sum`/`avg` are `CAST` to
/// `INTEGER` so cells stay in the universal `DbValue` domain (Postgres
/// `NUMERIC` is not a wire type).
pub const PORTABLE_SELECT: &str = "SELECT \
     abs(-3) AS c0, \
     coalesce(NULL, 4) AS c1, \
     ifnull(NULL, 5) AS c2, \
     length(body) AS c3, \
     lower(body) AS c4, \
     upper(body) AS c5, \
     trim(' x ') AS c6, \
     substr(body, 1, 1) AS c7, \
     CAST(round(1.4) AS INTEGER) AS c8, \
     nullif(n, 9) AS c9, \
     min(1, 2) AS c10, \
     max(3, 1) AS c11, \
     json_extract(payload, '$.k') AS c12, \
     json_extract(json_object('k', 'v'), '$.k') AS c13, \
     json_valid(payload) AS c14, \
     json_valid(body) AS c15, \
     replace(body, 'A', 'Z') AS c16, \
     CAST(n AS INTEGER) AS c17 \
     FROM typed";

/// Aggregate-only companion to [`PORTABLE_SELECT`].
pub const PORTABLE_AGGREGATE_SELECT: &str = "SELECT \
     count(*) AS a0, \
     CAST(sum(n) AS INTEGER) AS a1, \
     min(n) AS a2, \
     max(n) AS a3, \
     CAST(avg(n) AS INTEGER) AS a4 \
     FROM typed";

/// Expected cells for [`PORTABLE_SELECT`] (adapter-neutral).
#[derive(Clone, Copy, Debug)]
pub enum PortableExpect {
    /// Exact [`DbValue::Int64`].
    Int(i64),
    /// Exact [`DbValue::Text`].
    Text(&'static str),
    /// Exact [`DbValue::Float64`] (tolerance only vs another float).
    Float(f64),
    /// Exact [`DbValue::Boolean`].
    Bool(bool),
    /// Exact [`DbValue::Null`] with [`DbType::Bool`].
    NullBool,
}

/// Expected [`PORTABLE_SELECT`] cells in column order.
#[must_use]
pub fn portable_select_expects() -> &'static [PortableExpect] {
    &[
        PortableExpect::Int(3),
        PortableExpect::Int(4),
        PortableExpect::Int(5),
        PortableExpect::Int(2),
        PortableExpect::Text("ab"),
        PortableExpect::Text("AB"),
        PortableExpect::Text("x"),
        PortableExpect::Text("A"),
        PortableExpect::Int(1),
        PortableExpect::Int(2),
        PortableExpect::Int(1),
        PortableExpect::Int(3),
        PortableExpect::Text("v"),
        PortableExpect::Text("v"),
        PortableExpect::Int(1),
        PortableExpect::Int(0),
        PortableExpect::Text("Zb"),
        PortableExpect::Int(2),
    ]
}

/// Expected [`PORTABLE_BOOLEAN_SELECT`] cells in column order.
#[must_use]
pub fn portable_boolean_expects() -> &'static [PortableExpect] {
    &[PortableExpect::Bool(true), PortableExpect::NullBool]
}

/// Binds for [`PORTABLE_BOOLEAN_INSERT`].
#[must_use]
pub fn portable_boolean_insert_binds() -> Vec<DbValue> {
    vec![DbValue::Boolean(true), DbValue::Null(DbType::Bool)]
}

/// Binds for [`PORTABLE_INSERT_OR_IGNORE_RETURNING_LC`].
#[must_use]
pub fn portable_lowercase_insert_binds() -> Vec<DbValue> {
    vec![
        DbValue::Bytes(PORTABLE_INSERT_BLOB.to_vec()),
        DbValue::Boolean(true),
    ]
}

/// Expected [`PORTABLE_AGGREGATE_SELECT`] cells in column order.
#[must_use]
pub fn portable_aggregate_expects() -> &'static [PortableExpect] {
    &[
        PortableExpect::Int(1),
        PortableExpect::Int(2),
        PortableExpect::Int(2),
        PortableExpect::Int(2),
        PortableExpect::Int(2),
    ]
}

/// True when `got` is the exact [`DbValue`] for `want`.
#[must_use]
pub fn portable_value_matches(got: &DbValue, want: PortableExpect) -> bool {
    match (got, want) {
        (DbValue::Int64(n), PortableExpect::Int(w)) => *n == w,
        (DbValue::Text(s), PortableExpect::Text(w)) => s == w,
        (DbValue::Float64(n), PortableExpect::Float(w)) => (*n - w).abs() < 1e-9,
        (DbValue::Boolean(b), PortableExpect::Bool(w)) => *b == w,
        (DbValue::Null(DbType::Bool), PortableExpect::NullBool) => true,
        _ => false,
    }
}

/// SQLite expressions may omit a declared type; Postgres OIDs map to Int64/Text.
fn portable_column_type_ok(col: Option<&DbColumn>, want: PortableExpect) -> bool {
    let Some(col) = col else {
        return true;
    };
    match want {
        PortableExpect::Int(_) => matches!(col.db_type, DbType::Int64 | DbType::Unspecified),
        PortableExpect::Text(_) => matches!(col.db_type, DbType::Text | DbType::Unspecified),
        PortableExpect::Float(_) => matches!(col.db_type, DbType::Float64 | DbType::Unspecified),
        PortableExpect::Bool(_) | PortableExpect::NullBool => matches!(col.db_type, DbType::Bool),
    }
}

/// Formats a mismatch for adapter execution tests.
#[must_use]
pub fn portable_select_mismatch(stmt: &StatementResult) -> Option<String> {
    portable_statement_mismatch(stmt, portable_select_expects(), "portable SELECT")
}

/// Formats a mismatch for [`PORTABLE_BOOLEAN_SELECT`].
#[must_use]
pub fn portable_boolean_mismatch(stmt: &StatementResult) -> Option<String> {
    portable_statement_mismatch(stmt, portable_boolean_expects(), "portable BOOLEAN SELECT")
}

/// Formats a mismatch for [`PORTABLE_AGGREGATE_SELECT`].
#[must_use]
pub fn portable_aggregate_mismatch(stmt: &StatementResult) -> Option<String> {
    portable_statement_mismatch(
        stmt,
        portable_aggregate_expects(),
        "portable aggregate SELECT",
    )
}

/// Compares `stmt` to `expect` and describes the first mismatch.
fn portable_statement_mismatch(
    stmt: &StatementResult,
    expect: &[PortableExpect],
    label: &str,
) -> Option<String> {
    let Some(row) = stmt.rows.first() else {
        return Some(format!("{label} returned no rows"));
    };
    portable_row_mismatch(&stmt.columns, &row.values, expect, label)
}

/// Compares `values` to `expect` and describes the first mismatch.
fn portable_row_mismatch(
    columns: &[DbColumn],
    values: &[DbValue],
    expect: &[PortableExpect],
    label: &str,
) -> Option<String> {
    if values.len() != expect.len() {
        return Some(format!(
            "{label} returned {} cells; expected {}",
            values.len(),
            expect.len()
        ));
    }
    for (i, (got, want)) in values.iter().zip(expect.iter()).enumerate() {
        if !portable_value_matches(got, *want) {
            return Some(format!(
                "{label} column {i}: got {got:?}, expected {want:?}"
            ));
        }
        if !portable_column_type_ok(columns.get(i), *want) {
            return Some(format!(
                "{label} column {i} db_type {:?}, expected matching DbType (Bool required for boolean cells; Int64/Text/Float64 or Unspecified otherwise) for {want:?}",
                columns[i].db_type
            ));
        }
    }
    None
}

/// Mixed DDL whose companion INSERT stores the receipt-gate text as a value.
pub const MIXED_GATE_LITERAL_DDL: &str =
    "CREATE TABLE IF NOT EXISTS gated_notes (id INTEGER PRIMARY KEY, body TEXT)";

/// INSERT that embeds [`crate::GUEST_RECEIPT_WRITE_GATE`] in a comment and a string.
#[must_use]
pub fn mixed_gate_literal_insert() -> String {
    let gate = crate::GUEST_RECEIPT_WRITE_GATE;
    format!("INSERT INTO gated_notes (id, body)\n-- {gate}\nVALUES (1, '{gate}')")
}

/// SELECT the stored gate literal from [`mixed_gate_literal_insert`].
pub const MIXED_GATE_LITERAL_SELECT: &str = "SELECT body FROM gated_notes WHERE id = 1";

/// COUNT helper for mixed-gate replay assertions.
pub const MIXED_GATE_LITERAL_COUNT: &str = "SELECT count(*) FROM gated_notes";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_int_does_not_accept_float_or_text() {
        assert!(portable_value_matches(
            &DbValue::Int64(2),
            PortableExpect::Int(2)
        ));
        assert!(!portable_value_matches(
            &DbValue::Float64(2.0),
            PortableExpect::Int(2)
        ));
        assert!(!portable_value_matches(
            &DbValue::Text("2".into()),
            PortableExpect::Int(2)
        ));
        assert!(portable_value_matches(
            &DbValue::Text("ab".into()),
            PortableExpect::Text("ab")
        ));
        assert!(portable_value_matches(
            &DbValue::Boolean(true),
            PortableExpect::Bool(true)
        ));
        assert!(!portable_value_matches(
            &DbValue::Int64(1),
            PortableExpect::Bool(true)
        ));
        assert!(portable_value_matches(
            &DbValue::Null(DbType::Bool),
            PortableExpect::NullBool
        ));
        assert!(!portable_value_matches(
            &DbValue::Null(DbType::Int64),
            PortableExpect::NullBool
        ));
    }
}
