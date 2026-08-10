//! Branded `BookclerkPlugin` guest trait + native Workers RPC server.

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use bookclerk_plugin_abi::{
    methods, CliInvokeParams, CliInvokeResult, CliSchema, DiagnoseResult, HandshakeParams,
    HandshakeResult, HealthResult, HostToPluginEvent, PluginError, PluginErrorCode, RpcRequest,
    RpcResponse, API_VERSION,
};

use crate::error::{Result, SdkError};
use crate::protocol::MAX_RPC_LINE_BYTES;

/// Branded guest contract — identical method surface for native and workerd SDKs.
#[async_trait]
pub trait BookclerkPlugin: Send + Sync + 'static {
    async fn handshake(
        &self,
        params: HandshakeParams,
    ) -> std::result::Result<HandshakeResult, PluginError>;

    async fn shutdown(&self) -> std::result::Result<(), PluginError> {
        Ok(())
    }

    async fn health(&self) -> std::result::Result<HealthResult, PluginError> {
        Ok(HealthResult {
            ok: true,
            ..HealthResult::default()
        })
    }

    async fn diagnose(&self) -> std::result::Result<DiagnoseResult, PluginError> {
        Ok(DiagnoseResult { lines: vec![] })
    }

    async fn on_event(&self, _event: HostToPluginEvent) -> std::result::Result<(), PluginError> {
        Err(PluginError::unsupported("onEvent not implemented"))
    }

    async fn cli_describe(&self) -> std::result::Result<CliSchema, PluginError> {
        Ok(CliSchema::default())
    }

    async fn cli_invoke(
        &self,
        _params: CliInvokeParams,
    ) -> std::result::Result<CliInvokeResult, PluginError> {
        Err(PluginError::unsupported("cliInvoke not implemented"))
    }

    /// Escape hatch for kind-specific methods not yet on the trait.
    async fn call_raw(
        &self,
        method: &str,
        _params: Value,
    ) -> std::result::Result<Value, PluginError> {
        Err(PluginError::unsupported(format!(
            "method `{method}` not implemented"
        )))
    }
}

/// Serve a [`BookclerkPlugin`] on stdin/stdout using Workers RPC framing.
pub async fn serve_native<P: BookclerkPlugin>(plugin: P) -> Result<()> {
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut stdout = tokio::io::stdout();
    loop {
        let mut buf = Vec::new();
        let n = reader.read_until(b'\n', &mut buf).await?;
        if n == 0 {
            break;
        }
        if buf.len() > MAX_RPC_LINE_BYTES {
            return Err(SdkError::message("RPC line exceeds max size"));
        }
        let line = String::from_utf8_lossy(&buf);
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let req: RpcRequest = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!(%err, "invalid Workers RPC request");
                continue;
            }
        };
        let is_shutdown = req.method == methods::shutdown::NAME;
        let outcome = dispatch(&plugin, &req.method, req.params.unwrap_or(Value::Null)).await;
        let resp = match outcome {
            Ok(result) => RpcResponse {
                id: req.id.clone(),
                result: Some(result),
                error: None,
            },
            Err(_err) if is_shutdown => RpcResponse {
                id: req.id.clone(),
                result: Some(Value::Null),
                error: None,
            },
            Err(err) => RpcResponse {
                id: req.id.clone(),
                result: None,
                error: Some(err),
            },
        };
        let mut out = serde_json::to_string(&resp)?;
        out.push('\n');
        stdout.write_all(out.as_bytes()).await?;
        stdout.flush().await?;
        if is_shutdown {
            break;
        }
    }
    Ok(())
}

async fn dispatch<P: BookclerkPlugin>(
    plugin: &P,
    method: &str,
    params: Value,
) -> std::result::Result<Value, PluginError> {
    match method {
        m if m == methods::handshake::NAME => {
            let p: HandshakeParams = serde_json::from_value(params)
                .map_err(|e| PluginError::invalid_params(format!("handshake params: {e}")))?;
            if p.api_version != API_VERSION {
                return Err(PluginError::invalid_params(format!(
                    "unsupported apiVersion {}",
                    p.api_version
                )));
            }
            let result = plugin.handshake(p).await?;
            serde_json::to_value(result).map_err(|e| PluginError::internal(e.to_string()))
        }
        m if m == methods::shutdown::NAME => {
            plugin.shutdown().await?;
            Ok(Value::Null)
        }
        m if m == methods::health::NAME => {
            let result = plugin.health().await?;
            serde_json::to_value(result).map_err(|e| PluginError::internal(e.to_string()))
        }
        m if m == methods::diagnose::NAME => {
            let result = plugin.diagnose().await?;
            // Host historically accepted a bare string array; prefer object with lines.
            serde_json::to_value(result).map_err(|e| PluginError::internal(e.to_string()))
        }
        m if m == methods::on_event::NAME => {
            let event: HostToPluginEvent = serde_json::from_value(params)
                .map_err(|e| PluginError::invalid_params(format!("onEvent params: {e}")))?;
            plugin.on_event(event).await?;
            Ok(json!({ "ok": true }))
        }
        m if m == methods::cli_describe::NAME => {
            let result = plugin.cli_describe().await?;
            serde_json::to_value(result).map_err(|e| PluginError::internal(e.to_string()))
        }
        m if m == methods::cli_invoke::NAME => {
            let p: CliInvokeParams = serde_json::from_value(params)
                .map_err(|e| PluginError::invalid_params(format!("cliInvoke params: {e}")))?;
            let result = plugin.cli_invoke(p).await?;
            serde_json::to_value(result).map_err(|e| PluginError::internal(e.to_string()))
        }
        other => plugin.call_raw(other, params).await,
    }
}

/// Map a string error into [`PluginError`] for legacy handlers.
#[must_use]
pub fn plugin_error_from_message(message: String) -> PluginError {
    PluginError {
        code: PluginErrorCode::Internal,
        message,
        details: None,
    }
}
