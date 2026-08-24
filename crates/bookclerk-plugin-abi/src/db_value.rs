//! Universal Cap'n database value domain (`DbValue`).
//!
//! Baseline cells are typed null, bool, int64, finite float64, UTF-8 text, and
//! bytes. SeaORM and JSON bridges live in `bookclerk-db-exec` at the adapter edge.

use serde::{Deserialize, Serialize};

/// Expected SQL type for a typed null (Cap'n `DbType`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum DbType {
    /// Adapter may infer a type (unspecified null).
    #[default]
    Unspecified,
    /// Boolean null.
    Bool,
    /// Signed 64-bit integer null.
    Int64,
    /// Finite 64-bit float null.
    Float64,
    /// UTF-8 text null.
    Text,
    /// Bytea / blob null.
    Bytes,
}

/// One parameter or cell in the universal database domain.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum DbValue {
    /// Typed SQL null.
    Null(DbType),
    /// Boolean.
    Boolean(bool),
    /// Signed 64-bit integer.
    Int64(i64),
    /// Finite 64-bit float. Non-finite values are rejected at the bridge.
    Float64(f64),
    /// UTF-8 text.
    Text(String),
    /// Opaque bytes (not a `b64:` string).
    Bytes(Vec<u8>),
}

impl DbValue {
    /// Typed null of `ty`.
    #[must_use]
    pub const fn null(ty: DbType) -> Self {
        Self::Null(ty)
    }
}

#[cfg(test)]
#[allow(clippy::missing_panics_doc)]
mod tests {
    use super::*;

    #[test]
    fn unknown_kind_is_rejected() {
        let err = serde_json::from_str::<DbValue>(r#"{"kind":"xml","value":"<a/>"}"#).unwrap_err();
        assert!(err.to_string().contains("xml") || err.to_string().contains("unknown"));
    }

    #[test]
    fn null_default_roundtrip() {
        let v = DbValue::null(DbType::Text);
        assert_eq!(v, DbValue::Null(DbType::Text));
    }
}
