//! Authoritative Bookclerk plugin ABI (`api_version = 1`).
//!
//! The JSON Schema in `schema/abi.json` is the canonical contract. Types here
//! are the Rust projection used by host and guest SDKs. Field names serialize
//! as camelCase to match Workers RPC / TypeScript.

#![allow(missing_docs)]

pub mod error;
pub mod events;
pub mod methods;
pub mod types;

pub use error::{PluginError, PluginErrorCode, Result};
pub use events::{HostToPluginEvent, PluginToHostEvent};
pub use methods::METHOD_NAMES;
pub use types::*;

/// Wire API version for all guests.
pub const API_VERSION: u32 = 1;

/// Embedded schema bytes (CI / docs tooling can compare generators against this).
pub const ABI_SCHEMA_JSON: &str = include_str!("../schema/abi.json");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_parses_as_json() {
        let v: serde_json::Value =
            serde_json::from_str(ABI_SCHEMA_JSON).expect("abi.json must be valid JSON");
        assert_eq!(v["title"], "BookclerkPluginAbi");
    }

    #[test]
    fn handshake_roundtrip_camel_case() {
        let hs = HandshakeResult {
            api_version: API_VERSION,
            id: "echo".into(),
            kind: "integration".into(),
            capabilities: vec!["health".into()],
            ..HandshakeResult::default()
        };
        let v = serde_json::to_value(&hs).unwrap();
        assert!(v.get("apiVersion").is_some());
        assert!(v.get("api_version").is_none());
        let back: HandshakeResult = serde_json::from_value(v).unwrap();
        assert_eq!(back.id, "echo");
    }
}
