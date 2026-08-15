//! [`StorageBackend`] adapter over the local filesystem output plugin process.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use async_trait::async_trait;
use base64::Engine;
use bookclerk_config::{normalize_storage_prefix, Config};
use bookclerk_plugin_sdk::v2::{DestinationContext, PRODUCT_API_VERSION};
use bookclerk_storage::{
    ByteRange, ListPage, ObjectInfo, ObjectMeta, ObjectProbe, PutStreamResult, StorageBackend,
    StorageError,
};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde_json::Value;

use super::v1_fail_closed;
use crate::discover::DiscoveredPlugin;
use crate::jail::plugin_data_dir;
use crate::protocol::{
    methods, GetResultDto, LocalCopyParams, LocalGetParams, LocalKeyParams, LocalListParams,
    LocalPutFileParams, LocalPutParams, LocalTouchFileParams, ObjectInfoDto, ObjectMetaDto,
    ObjectProbeDto, OutputLocalContextDto,
};
use crate::rpc::PluginClient;
use crate::rpc_v2::{V2PluginSession, V2Storage};
use crate::Result as PluginResult;

/// Manifest id of the platform local-filesystem destination guest.
const LOCAL_PLUGIN_ID: &str = "local";

/// Absolute local output root: `output.local.root`, or that path joined to `files_dir` when relative.
fn resolved_local_output_root(config: &Config) -> PathBuf {
    let root = &config.output.local.root;
    if root.is_absolute() {
        root.clone()
    } else {
        config.paths().files_dir.join(root)
    }
}

/// External local filesystem destination backed by a discovered output plugin.
#[derive(Clone)]
pub struct ExternalLocalDestination {
    /// RPC client for the jailed local output guest.
    client: Arc<PluginClient>,
    /// Guest `HOME` / data directory under `plugins/local/data`.
    plugin_data_dir: PathBuf,
    /// Absolute directory that destination keys are resolved against.
    root: PathBuf,
    /// Normalized key prefix from `[output.local].prefix`.
    prefix: String,
}

impl ExternalLocalDestination {
    /// Spawn and handshake a local output plugin.
    pub async fn spawn(plugin: &DiscoveredPlugin, config: &Config) -> PluginResult<Self> {
        let table = crate::settings_table(config, plugin);
        let config_json = toml_to_json(&toml::Value::Table(table));
        let client = Arc::new(PluginClient::spawn(plugin, config, config_json).await?);
        let plugin_data_dir = plugin_data_dir(config, &plugin.manifest.id)?;
        let root = resolved_local_output_root(config);
        let prefix = normalize_storage_prefix(config.output.local.prefix.trim());
        Ok(Self {
            client,
            plugin_data_dir,
            root,
            prefix,
        })
    }

    /// Side-channel context (data dir, root, prefix) sent with every local output RPC.
    fn ctx(&self) -> OutputLocalContextDto {
        OutputLocalContextDto {
            plugin_data_dir: self.plugin_data_dir.display().to_string(),
            root: self.root.display().to_string(),
            prefix: self.prefix.clone(),
        }
    }

    /// Maps a plugin RPC error onto [`StorageError`] (`Io` stays `Io`).
    fn map_err(err: crate::PluginError) -> StorageError {
        match err {
            crate::PluginError::Io(io) => StorageError::Io(io),
            other => StorageError::Other(anyhow::anyhow!(other)),
        }
    }

    /// Copies host [`ObjectMeta`] into the guest wire DTO.
    fn meta_to_dto(meta: &ObjectMeta) -> ObjectMetaDto {
        ObjectMetaDto {
            content_type: meta.content_type.clone(),
            content_length: meta.content_length,
            asin: meta.asin.clone(),
            title: meta.title.clone(),
            creation_time: meta.creation_time.clone(),
            last_write_time: meta.last_write_time.clone(),
        }
    }

    /// Copies a guest wire DTO back into host [`ObjectMeta`].
    fn meta_from_dto(dto: ObjectMetaDto) -> ObjectMeta {
        ObjectMeta {
            content_type: dto.content_type,
            content_length: dto.content_length,
            asin: dto.asin,
            title: dto.title,
            creation_time: dto.creation_time,
            last_write_time: dto.last_write_time,
        }
    }

    /// Formats a filesystem timestamp as RFC 3339 UTC for `touchFile`.
    fn rfc3339(time: SystemTime) -> Option<String> {
        Some(DateTime::<Utc>::from(time).to_rfc3339())
    }
}

#[async_trait]
impl StorageBackend for ExternalLocalDestination {
    fn name(&self) -> &'static str {
        "local"
    }

    fn clone_box(&self) -> Box<dyn StorageBackend> {
        Box::new(self.clone())
    }

    async fn put(&self, key: &str, data: Bytes, meta: ObjectMeta) -> bookclerk_storage::Result<()> {
        v1_fail_closed::reject_oversize_scalar(data.len() as u64, "put")?;
        let params = LocalPutParams {
            ctx: self.ctx(),
            key: key.to_string(),
            data_base64: base64::engine::general_purpose::STANDARD.encode(data),
            meta: Self::meta_to_dto(&meta),
        };
        self.client
            .call::<Value>(
                methods::PUT,
                serde_json::to_value(params).map_err(map_json_err)?,
            )
            .await
            .map_err(Self::map_err)?;
        Ok(())
    }

    async fn put_file(
        &self,
        key: &str,
        path: &Path,
        meta: ObjectMeta,
    ) -> bookclerk_storage::Result<()> {
        let params = LocalPutFileParams {
            ctx: self.ctx(),
            key: key.to_string(),
            meta: Self::meta_to_dto(&meta),
            local_path: if self.client.has_side_channel() {
                None
            } else {
                Some(path.display().to_string())
            },
        };
        let params_json = serde_json::to_value(params).map_err(map_json_err)?;
        if self.client.has_side_channel() || self.client.has_acl_grants() {
            self.client
                .call_raw_with_upload_file(methods::PUT_FILE, params_json, path)
                .await
                .map_err(Self::map_err)?;
        } else {
            self.client
                .call_raw(methods::PUT_FILE, params_json)
                .await
                .map_err(Self::map_err)?;
        }
        Ok(())
    }

    async fn get(&self, key: &str) -> bookclerk_storage::Result<Bytes> {
        let probe = self.probe(key).await?;
        v1_fail_closed::reject_oversize_probe(&probe, "get")?;
        let params = LocalGetParams {
            ctx: self.ctx(),
            key: key.to_string(),
        };
        let result: GetResultDto = self
            .client
            .call(
                methods::GET,
                serde_json::to_value(params).map_err(map_json_err)?,
            )
            .await
            .map_err(Self::map_err)?;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(result.data_base64)
            .map(Bytes::from)
            .map_err(|err| StorageError::InvalidKey(format!("invalid get base64: {err}")))?;
        v1_fail_closed::reject_oversize_scalar(decoded.len() as u64, "get")?;
        Ok(decoded)
    }

    async fn exists(&self, key: &str) -> bookclerk_storage::Result<bool> {
        let params = LocalKeyParams {
            ctx: self.ctx(),
            key: key.to_string(),
        };
        let value: Value = self
            .client
            .call(
                methods::EXISTS,
                serde_json::to_value(params).map_err(map_json_err)?,
            )
            .await
            .map_err(Self::map_err)?;
        parse_exists_response(&value)
    }

    async fn list(&self, prefix: &str) -> bookclerk_storage::Result<Vec<ObjectInfo>> {
        let params = LocalListParams {
            ctx: self.ctx(),
            prefix: prefix.to_string(),
        };
        let items: Vec<ObjectInfoDto> = self
            .client
            .call(
                methods::LIST,
                serde_json::to_value(params).map_err(map_json_err)?,
            )
            .await
            .map_err(Self::map_err)?;
        Ok(items
            .into_iter()
            .map(|item| ObjectInfo {
                key: item.key,
                size: item.size,
            })
            .collect())
    }

    async fn probe(&self, key: &str) -> bookclerk_storage::Result<ObjectProbe> {
        let params = LocalKeyParams {
            ctx: self.ctx(),
            key: key.to_string(),
        };
        let dto: ObjectProbeDto = self
            .client
            .call(
                methods::PROBE,
                serde_json::to_value(params).map_err(map_json_err)?,
            )
            .await
            .map_err(Self::map_err)?;
        Ok(ObjectProbe {
            key: dto.key,
            size: dto.size,
            content_type: dto.content_type,
            meta: Self::meta_from_dto(dto.meta),
        })
    }

    async fn copy(&self, from: &str, to: &str) -> bookclerk_storage::Result<()> {
        let params = LocalCopyParams {
            ctx: self.ctx(),
            from: from.to_string(),
            to: to.to_string(),
        };
        self.client
            .call::<Value>(
                methods::COPY,
                serde_json::to_value(params).map_err(map_json_err)?,
            )
            .await
            .map_err(Self::map_err)?;
        Ok(())
    }

    async fn delete(&self, key: &str) -> bookclerk_storage::Result<()> {
        let params = LocalKeyParams {
            ctx: self.ctx(),
            key: key.to_string(),
        };
        self.client
            .call::<Value>(
                methods::DELETE,
                serde_json::to_value(params).map_err(map_json_err)?,
            )
            .await
            .map_err(Self::map_err)?;
        Ok(())
    }

    async fn touch_file(
        &self,
        key: &str,
        created: Option<SystemTime>,
        modified: Option<SystemTime>,
    ) -> bookclerk_storage::Result<()> {
        let params = LocalTouchFileParams {
            ctx: self.ctx(),
            key: key.to_string(),
            created: created.and_then(Self::rfc3339),
            modified: modified.and_then(Self::rfc3339),
        };
        self.client
            .call::<Value>(
                methods::TOUCH_FILE,
                serde_json::to_value(params).map_err(map_json_err)?,
            )
            .await
            .map_err(Self::map_err)?;
        Ok(())
    }

    async fn list_page(
        &self,
        prefix: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> bookclerk_storage::Result<ListPage> {
        let all = self.list(prefix).await?;
        Ok(v1_fail_closed::paginate_objects(all, cursor, limit))
    }

    async fn get_stream(
        &self,
        key: &str,
        range: Option<ByteRange>,
    ) -> bookclerk_storage::Result<(
        ObjectProbe,
        std::pin::Pin<Box<dyn tokio::io::AsyncRead + Send>>,
    )> {
        v1_fail_closed::reject_range(range)?;
        let probe = self.probe(key).await?;
        v1_fail_closed::reject_oversize_probe(&probe, "get_stream")?;
        let data = self.get(key).await?;
        Ok((probe, v1_fail_closed::cursor_stream(data)))
    }

    async fn put_stream(
        &self,
        key: &str,
        body: std::pin::Pin<Box<dyn tokio::io::AsyncRead + Send>>,
        meta: ObjectMeta,
    ) -> bookclerk_storage::Result<PutStreamResult> {
        let data = v1_fail_closed::read_capped_stream(body).await?;
        let n = data.len() as u64;
        self.put(key, data, meta).await?;
        Ok(v1_fail_closed::put_result(n))
    }

    fn supports_server_copy(&self) -> bool {
        true
    }
}

/// Spawns the local output guest when enabled; logs and falls back in-process on failure.
pub(crate) async fn try_load_local(
    plugin: &DiscoveredPlugin,
    config: &Config,
    registry: &mut super::destination::DestinationRegistry,
) {
    if plugin.manifest.id != LOCAL_PLUGIN_ID {
        return;
    }
    if !config.output.local.enabled {
        tracing::debug!(id = %plugin.manifest.id, "local output disabled in config; skipping external plugin");
        return;
    }
    if plugin.manifest.api_version == PRODUCT_API_VERSION {
        match spawn_v2_local(plugin, config).await {
            Ok((storage, session)) => {
                tracing::info!(
                    id = %plugin.manifest.id,
                    path = %plugin.command.display(),
                    "loaded external local output plugin (api_version 2)"
                );
                registry.set_local(Arc::new(storage));
                registry.set_v2_session(session);
            }
            Err(err) => {
                tracing::warn!(
                    id = %plugin.manifest.id,
                    %err,
                    "failed to start v2 local output plugin; falling back to in-process backend"
                );
            }
        }
        return;
    }
    match ExternalLocalDestination::spawn(plugin, config).await {
        Ok(dest) => {
            tracing::info!(
                id = %plugin.manifest.id,
                path = %plugin.command.display(),
                "loaded external local output plugin"
            );
            registry.set_local(Arc::new(dest));
        }
        Err(err) => {
            tracing::warn!(
                id = %plugin.manifest.id,
                %err,
                "failed to start external local output plugin; falling back to in-process backend"
            );
        }
    }
}

/// Spawns the local destination as an ABI v2 Cap'n Proto guest.
async fn spawn_v2_local(
    plugin: &DiscoveredPlugin,
    config: &Config,
) -> PluginResult<(V2Storage, Arc<V2PluginSession>)> {
    let table = crate::settings_table(config, plugin);
    let config_json = toml_to_json(&toml::Value::Table(table));
    let root = resolved_local_output_root(config);
    let prefix = normalize_storage_prefix(config.output.local.prefix.trim());
    let extra_env = [(
        "BOOKCLERK_OUTPUT_LOCAL_ROOT",
        std::ffi::OsString::from(root.as_os_str()),
    )];
    let session = Arc::new(
        V2PluginSession::spawn_for_account_with_env(
            plugin,
            config,
            config_json,
            crate::OPERATOR_ACCOUNT,
            &extra_env,
        )
        .await?,
    );
    let ctx = OutputLocalContextDto {
        plugin_data_dir: String::new(),
        root: String::new(),
        prefix,
    };
    session
        .ensure_destination(DestinationContext {
            json: serde_json::to_string(&ctx)?,
        })
        .await?;
    Ok((V2Storage::new(Arc::clone(&session)), session))
}

/// Converts a plugin settings TOML table to JSON for guest handshake; `Null` on failure.
fn toml_to_json(value: &toml::Value) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// Wraps a serde JSON error as [`StorageError::Other`] when encoding RPC params.
fn map_json_err(err: serde_json::Error) -> StorageError {
    StorageError::Other(anyhow::anyhow!("serialize output RPC params: {err}"))
}

/// Reads the boolean `exists` field from a guest `exists` RPC result.
fn parse_exists_response(value: &Value) -> bookclerk_storage::Result<bool> {
    value
        .get("exists")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| {
            StorageError::Other(anyhow::anyhow!(
                "plugin exists response missing boolean \"exists\" field: {value}"
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_local_destination_json_omits_filesystem_paths() {
        let ctx = OutputLocalContextDto {
            plugin_data_dir: String::new(),
            root: String::new(),
            prefix: "audiobooks".into(),
        };
        let json = serde_json::to_string(&ctx).unwrap();
        assert!(
            !json.contains("pluginDataDir") && !json.contains("plugin_data_dir"),
            "{json}"
        );
        assert!(!json.contains("root"), "{json}");
        assert!(!json.contains('/'), "{json}");
        assert!(!json.contains('\\'), "{json}");
    }
}
