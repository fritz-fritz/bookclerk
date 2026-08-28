//! Shared Bookclerk SQL v1 execution vectors (binding DDL + portable helpers).
//!
//! Hosts emit these canonical statements. Adapters lower at execute: SQLite/D1
//! run them verbatim; Postgres applies binding DDL type rewrites and helper
//! lowering. Every name in `GuestSqlPolicy::binding_owned` portable functions
//! (except denied `hex`) appears here.

use bookclerk_plugin_abi::DbValue;

/// Binding `CREATE TABLE` covering identity + `BLOB`/`INTEGER`/`REAL`.
pub const BINDING_DDL_AUTOINCREMENT_BLOB: &str = "CREATE TABLE IF NOT EXISTS typed (\
     id INTEGER PRIMARY KEY AUTOINCREMENT, \
     n INTEGER, body TEXT, payload TEXT, blob BLOB, r REAL)";

/// Seed row for [`PORTABLE_SELECT`]. Bind [`PORTABLE_INSERT_BLOB`] at `?`.
pub const PORTABLE_INSERT: &str =
    "INSERT INTO typed (n, body, payload, blob, r) VALUES (2, 'Ab', '{\"k\":\"v\"}', ?, 1.5)";

/// Blob payload for [`PORTABLE_INSERT`].
pub const PORTABLE_INSERT_BLOB: &[u8] = &[1, 2, 3];

/// One SELECT that exercises every remaining portable helper.
///
/// Column order matches [`portable_select_expects`]. `json_object` is wrapped
/// in `json_extract` so Postgres `json_build_object` still yields text.
pub const PORTABLE_SELECT: &str = "SELECT \
     abs(-3) AS c0, \
     coalesce(NULL, 4) AS c1, \
     ifnull(NULL, 5) AS c2, \
     length(body) AS c3, \
     lower(body) AS c4, \
     upper(body) AS c5, \
     trim(' x ') AS c6, \
     substr(body, 1, 1) AS c7, \
     round(1.4) AS c8, \
     nullif(n, 9) AS c9, \
     min(1, 2) AS c10, \
     max(3, 1) AS c11, \
     json_extract(payload, '$.k') AS c12, \
     json_extract(json_object('k', 'v'), '$.k') AS c13, \
     json_valid(payload) AS c14, \
     json_valid(body) AS c15, \
     replace(body, 'A', 'Z') AS c16, \
     CAST(n AS INTEGER) AS c17, \
     count(*) AS c18, \
     sum(n) AS c19, \
     min(n) AS c20, \
     max(n) AS c21, \
     avg(n) AS c22 \
     FROM typed";

/// Expected cells for [`PORTABLE_SELECT`] (adapter-neutral).
#[derive(Clone, Copy, Debug)]
pub enum PortableExpect {
    /// Integer (or integer-valued float from `round`/`avg`/`json_valid`).
    Int(i64),
    /// UTF-8 text.
    Text(&'static str),
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
        PortableExpect::Int(1),
        PortableExpect::Int(2),
        PortableExpect::Int(2),
        PortableExpect::Int(2),
        PortableExpect::Int(2),
    ]
}

/// True when `got` matches `want` across SQLite / Postgres / D1 encodings.
#[must_use]
pub fn portable_value_matches(got: &DbValue, want: PortableExpect) -> bool {
    match (got, want) {
        (DbValue::Int64(n), PortableExpect::Int(w)) => *n == w,
        (DbValue::Float64(n), PortableExpect::Int(w)) => (*n - w as f64).abs() < 1e-9,
        (DbValue::Text(s), PortableExpect::Text(w)) => s == w,
        (DbValue::Text(s), PortableExpect::Int(w)) => {
            s.parse::<i64>().is_ok_and(|n| n == w)
                || s.parse::<f64>().is_ok_and(|n| (n - w as f64).abs() < 1e-9)
        }
        _ => false,
    }
}

/// Formats a mismatch for adapter execution tests.
#[must_use]
pub fn portable_select_mismatch(values: &[DbValue]) -> Option<String> {
    let expect = portable_select_expects();
    if values.len() != expect.len() {
        return Some(format!(
            "portable SELECT returned {} cells; expected {}",
            values.len(),
            expect.len()
        ));
    }
    for (i, (got, want)) in values.iter().zip(expect.iter()).enumerate() {
        if !portable_value_matches(got, *want) {
            return Some(format!(
                "portable SELECT column {i}: got {got:?}, expected {want:?}"
            ));
        }
    }
    None
}
