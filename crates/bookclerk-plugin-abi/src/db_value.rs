//! Universal Cap'n database value domain (`DbValue`).
//!
//! Baseline cells are typed null, bool, int64, finite float64, UTF-8 text, and
//! bytes. SeaORM and JSON bridges live at the adapter edge and are fallible.

use serde::{Deserialize, Serialize};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;

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

/// Converts a legacy JSON bind onto [`DbValue`].
///
/// Accepts JSON null/bool/number/string, `b64:` blobs, and `$sea_null` objects.
/// Arrays, objects (other than typed null), unsigned overflow, and non-finite
/// floats are rejected.
///
/// # Errors
///
/// Returns a static reason when the JSON value is outside the universal domain.
pub fn db_value_from_json(v: &serde_json::Value) -> Result<DbValue, String> {
    if let Some(kind) = crate::sea_null_kind(v) {
        let ty = match kind {
            "Bytes" => DbType::Bytes,
            "BigInt" | "Int" | "TinyInt" | "SmallInt" | "TinyUnsigned" | "SmallUnsigned"
            | "Unsigned" | "BigUnsigned" => DbType::Int64,
            "Bool" => DbType::Bool,
            "Double" | "Float" => DbType::Float64,
            _ => DbType::Text,
        };
        return Ok(DbValue::Null(ty));
    }
    match v {
        serde_json::Value::Null => Ok(DbValue::Null(DbType::Unspecified)),
        serde_json::Value::Bool(b) => Ok(DbValue::Boolean(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                return Ok(DbValue::Int64(i));
            }
            if let Some(u) = n.as_u64() {
                let i = i64::try_from(u)
                    .map_err(|_| format!("unsigned integer {u} overflows int64"))?;
                return Ok(DbValue::Int64(i));
            }
            let f = n
                .as_f64()
                .ok_or_else(|| "number is not a finite float64".to_string())?;
            if !f.is_finite() {
                return Err("float64 value is not finite".into());
            }
            Ok(DbValue::Float64(f))
        }
        serde_json::Value::String(s) => {
            if let Some(rest) = s.strip_prefix("b64:") {
                let bytes = BASE64
                    .decode(rest)
                    .map_err(|err| format!("invalid b64: payload: {err}"))?;
                return Ok(DbValue::Bytes(bytes));
            }
            Ok(DbValue::Text(s.clone()))
        }
        serde_json::Value::Array(_) => Err("arrays are not a baseline DbValue".into()),
        serde_json::Value::Object(_) => Err("objects are not a baseline DbValue".into()),
    }
}

/// Encodes [`DbValue`] as the legacy JSON bind used by in-process executors.
#[must_use]
pub fn db_value_to_json(v: &DbValue) -> serde_json::Value {
    match v {
        DbValue::Null(DbType::Bytes) => crate::sea_null("Bytes"),
        DbValue::Null(DbType::Int64) => crate::sea_null("BigInt"),
        DbValue::Null(DbType::Bool) => crate::sea_null("Bool"),
        DbValue::Null(DbType::Float64) => crate::sea_null("Double"),
        DbValue::Null(DbType::Text | DbType::Unspecified) => serde_json::Value::Null,
        DbValue::Boolean(b) => serde_json::Value::Bool(*b),
        DbValue::Int64(n) => serde_json::json!(*n),
        DbValue::Float64(n) => serde_json::json!(*n),
        DbValue::Text(s) => serde_json::Value::String(s.clone()),
        DbValue::Bytes(b) => serde_json::Value::String(format!("b64:{}", BASE64.encode(b))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn typed_null_bytes_roundtrip() {
        let v = db_value_from_json(&crate::sea_null("Bytes")).unwrap();
        assert_eq!(v, DbValue::Null(DbType::Bytes));
        assert_eq!(crate::sea_null_kind(&db_value_to_json(&v)), Some("Bytes"));
    }

    #[test]
    fn i64_min_max_roundtrip() {
        for n in [i64::MIN, -1, 0, 1, i64::MAX] {
            let v = db_value_from_json(&json!(n)).unwrap();
            assert_eq!(v, DbValue::Int64(n));
            assert_eq!(db_value_to_json(&v), json!(n));
        }
    }

    #[test]
    fn unsigned_overflow_is_rejected() {
        let err = db_value_from_json(&json!(u64::MAX)).unwrap_err();
        assert!(err.contains("overflows"), "{err}");
    }

    #[test]
    fn utf8_and_embedded_zero_bytes() {
        let text = db_value_from_json(&json!("héllo\u{0}world")).unwrap();
        assert_eq!(text, DbValue::Text("héllo\u{0}world".into()));
        let blob = DbValue::Bytes(vec![0, 1, 0, 2]);
        let json = db_value_to_json(&blob);
        let back = db_value_from_json(&json).unwrap();
        assert_eq!(back, blob);
    }

    #[test]
    fn arrays_are_rejected() {
        let err = db_value_from_json(&json!([1, 2])).unwrap_err();
        assert!(err.contains("arrays"), "{err}");
    }

    #[test]
    fn unknown_kind_is_rejected() {
        let err = serde_json::from_str::<DbValue>(r#"{"kind":"xml","value":"<a/>"}"#).unwrap_err();
        assert!(err.to_string().contains("xml") || err.to_string().contains("unknown"));
    }
}
