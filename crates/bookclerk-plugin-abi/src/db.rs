//! Host-private database connect plumbing (feature `host`).
//!
//! Guests such as `sqlite` / `d1` / `postgres` implement the SeaORM proxy
//! boundary. The host never links SQL engines; it opens the library through
//! `DatabaseContext` + typed adapter sessions after Cap'n Proto spawn.
//!
//! `DbConnectParams` is a host-private serde type for first-party
//! connect-param building — not public JSON RPC. Wire fields use camelCase;
//! its `backend` tag is lowercase (`sqlite`, `d1`, `postgres`). Semantic
//! capability limits live in [`crate::DbCapabilities`]
//! (`crate::db_execute`); bootstrap metadata lives in [`crate::DbBootstrap`].

#[cfg(feature = "host")]
use serde::{Deserialize, Serialize};

/// Host-private tagged connect params for first-party database guests.
///
/// Travels only inside [`crate::DatabaseContext::config`] built by the
/// host ([`database_context_from_params`]); not part of the public plugin
/// author ABI. Discriminant is wire field `backend` with lowercase tags.
/// SQLite guests open `library.db` at [`Self::Sqlite::sqlite_path`] (also
/// injected as `BOOKCLERK_SQLITE_PATH`); D1 / Postgres receive host-injected
/// credentials in the params. Third-party adapters read connection settings
/// from plugin-owned config / secrets bindings instead.
#[cfg(feature = "host")]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "backend", rename_all = "lowercase")]
pub enum DbConnectParams {
    /// Local SQLite file backend (`backend: "sqlite"`).
    #[serde(rename_all = "camelCase")]
    Sqlite {
        /// Scoped writable directory for this plugin
        /// (`…/plugins/<id>/data`, wire `pluginDataDir`).
        plugin_data_dir: String,
        /// Absolute path to the DB file (wire `sqlitePath`). The sqlite jail
        /// grants this file and its journal sidecars at spawn.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sqlite_path: Option<String>,
    },
    /// Cloudflare D1 HTTP API backend (`backend: "d1"`).
    #[serde(rename_all = "camelCase")]
    D1 {
        /// Scoped writable directory for this plugin (wire `pluginDataDir`).
        plugin_data_dir: String,
        /// Cloudflare account id for the D1 API (wire `accountId`).
        account_id: String,
        /// D1 database UUID (wire `databaseId`).
        database_id: String,
        /// API base URL (for example `https://api.cloudflare.com/client/v4`).
        api_base: String,
        /// Bearer / API token the host injects; guests must not read env for this.
        api_token: String,
    },
    /// PostgreSQL connection-string backend (`backend: "postgres"`).
    #[serde(rename_all = "camelCase")]
    Postgres {
        /// Scoped writable directory for this plugin (wire `pluginDataDir`).
        plugin_data_dir: String,
        /// Full Postgres connection URL (host-injected; may contain secrets).
        url: String,
    },
    /// Third-party / conformance database guest (`backend: "guest"`).
    ///
    /// The host does not inject first-party secrets. Connection knobs come from
    /// plugin-owned settings (vat `config` binding) and optional `secrets`
    /// bindings. The guest must return bootstrap `sqlFamily` / `dialect` on
    /// connect so the host can open the SeaORM proxy.
    #[serde(rename_all = "camelCase")]
    Guest {
        /// Scoped writable directory for this plugin (wire `pluginDataDir`).
        plugin_data_dir: String,
    },
}

/// Media type for [`crate::DatabaseContext::config`] connect payloads.
#[cfg(feature = "host")]
pub const DATABASE_CONTEXT_MEDIA_TYPE: &str = "application/vnd.bookclerk.db-connect+json";

/// Schema version for [`crate::DatabaseContext::config`] connect payloads.
#[cfg(feature = "host")]
pub const DATABASE_CONTEXT_SCHEMA_VERSION: u32 = 1;

/// Builds a [`crate::DatabaseContext`] from host-internal connect params.
///
/// # Errors
///
/// Returns when JSON serialization fails.
#[cfg(feature = "host")]
pub fn database_context_from_params(
    params: &DbConnectParams,
) -> crate::Result<crate::DatabaseContext> {
    let payload = serde_json::to_vec(params).map_err(|err| {
        crate::PluginError::internal(format!("database context encode failed: {err}"))
    })?;
    Ok(crate::DatabaseContext {
        json: String::new(),
        config: crate::ExtensibleConfig {
            schema_version: DATABASE_CONTEXT_SCHEMA_VERSION,
            media_type: DATABASE_CONTEXT_MEDIA_TYPE.into(),
            payload,
        },
    })
}

/// Decodes host-internal connect params from a database factory context.
///
/// # Errors
///
/// Returns when the context omits connect params or JSON is invalid.
#[cfg(feature = "host")]
pub fn connect_params_from_context(ctx: &crate::DatabaseContext) -> crate::Result<DbConnectParams> {
    if !ctx.config.payload.is_empty() {
        return serde_json::from_slice(&ctx.config.payload).map_err(|err| {
            crate::PluginError::invalid_params(format!("database context decode failed: {err}"))
        });
    }
    if ctx.json.trim().is_empty() {
        return Err(crate::PluginError::invalid_params(
            "database context is missing connect params",
        ));
    }
    serde_json::from_str(&ctx.json).map_err(|err| {
        crate::PluginError::invalid_params(format!("database context decode failed: {err}"))
    })
}

#[cfg(all(test, feature = "host"))]
#[allow(clippy::missing_panics_doc)]
mod host_tests {
    use super::*;

    #[test]
    fn connect_params_are_tagged_by_backend_with_camel_case_fields() {
        let sqlite = DbConnectParams::Sqlite {
            plugin_data_dir: "/tmp/p".into(),
            sqlite_path: Some("/tmp/library.db".into()),
        };
        let v = serde_json::to_value(&sqlite).unwrap();
        assert_eq!(v["backend"], "sqlite");
        assert!(v.get("pluginDataDir").is_some());
        assert!(v.get("sqlitePath").is_some());
        assert!(v.get("plugin_data_dir").is_none());
        let back: DbConnectParams = serde_json::from_value(v).unwrap();
        assert_eq!(back, sqlite);
    }

    #[test]
    fn connect_params_roundtrip_through_database_context() {
        let params = DbConnectParams::Guest {
            plugin_data_dir: "/tmp/p".into(),
        };
        let ctx = database_context_from_params(&params).unwrap();
        assert_eq!(ctx.config.media_type, DATABASE_CONTEXT_MEDIA_TYPE);
        assert_eq!(ctx.config.schema_version, DATABASE_CONTEXT_SCHEMA_VERSION);
        assert_eq!(connect_params_from_context(&ctx).unwrap(), params);
    }
}
