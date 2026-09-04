//! Domain JSON projection for typed [`DbValue`] cells.

use bookclerk_plugin_abi::{DbType, DbValue};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;

/// Converts JSON onto [`DbValue`] for leftover adapter-edge helpers.
///
/// # Errors
///
/// Returns a static reason when the JSON value is outside the universal domain.
pub fn db_value_from_json(v: &serde_json::Value) -> Result<DbValue, String> {
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

/// Encodes [`DbValue`] as domain JSON (typed nulls become JSON null).
#[must_use]
pub fn db_value_to_json(v: &DbValue) -> serde_json::Value {
    match v {
        DbValue::Null(_) => serde_json::Value::Null,
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
    fn null_and_bytes_roundtrip_without_sea_null() {
        assert_eq!(
            db_value_to_json(&DbValue::Null(DbType::Bytes)),
            serde_json::Value::Null
        );
        let bytes = db_value_from_json(&json!("b64:AA==")).unwrap();
        assert_eq!(bytes, DbValue::Bytes(vec![0]));
    }

    #[test]
    fn arrays_are_rejected() {
        let err = db_value_from_json(&json!([1, 2])).unwrap_err();
        assert!(err.contains("arrays"), "{err}");
    }
}
