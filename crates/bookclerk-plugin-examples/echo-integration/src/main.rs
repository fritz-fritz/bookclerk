//! Minimal external integration plugin for Bookclerk.
//!
//! Speaks the Workers RPC ABI on stdio via [`BookclerkPlugin`] /
//! [`serve_native`]. Install `plugin.toml` + this binary under
//! `$BOOKCLERK_FILES_DIR/plugins/echo/` and enable in config after consent:
//!
//! ```bash
//! bookclerk plugins approve echo --yes
//! bookclerk plugins enable echo
//! ```
//!
//! Declares a sample CLI command:
//!
//! ```bash
//! bookclerk plugins echo ping --message hi
//! ```

use async_trait::async_trait;
use bookclerk_plugin_abi::{
    CliArgKind, CliArgSpec, CliCommandSpec, CliInvokeParams, CliInvokeResult, CliSchema,
    DiagnoseResult, HandshakeParams, HandshakeResult, HealthResult, HostToPluginEvent, PluginError,
    API_VERSION,
};
use bookclerk_plugin_sdk::{serve_native, BookclerkPlugin};

fn cli_schema() -> CliSchema {
    CliSchema {
        commands: vec![CliCommandSpec {
            name: "ping".into(),
            about: Some("Probe echo plugin".into()),
            args: vec![CliArgSpec {
                name: "message".into(),
                long: Some("message".into()),
                short: Some('m'),
                kind: CliArgKind::String,
                required: false,
                default: Some("hi".into()),
                about: Some("Message to echo".into()),
                positional: false,
            }],
        }],
    }
}

struct EchoPlugin;

#[async_trait]
impl BookclerkPlugin for EchoPlugin {
    async fn handshake(&self, _params: HandshakeParams) -> Result<HandshakeResult, PluginError> {
        Ok(HandshakeResult {
            api_version: API_VERSION,
            id: "echo".into(),
            kind: "integration".into(),
            display_name: Some("Echo Integration".into()),
            capabilities: vec![
                "health".into(),
                "diagnose".into(),
                "onEvent".into(),
                "start".into(),
                "cli".into(),
            ],
            cli: Some(cli_schema()),
            ..HandshakeResult::default()
        })
    }

    async fn health(&self) -> Result<HealthResult, PluginError> {
        Ok(HealthResult {
            ok: true,
            id: Some("echo".into()),
            enabled: Some(true),
            detail: Some("echo plugin ready".into()),
        })
    }

    async fn diagnose(&self) -> Result<DiagnoseResult, PluginError> {
        Ok(DiagnoseResult {
            lines: vec!["echo plugin diagnose: ok".into()],
        })
    }

    async fn on_event(&self, event: HostToPluginEvent) -> Result<(), PluginError> {
        eprintln!("echo-integration event: {event:?}");
        Ok(())
    }

    async fn cli_describe(&self) -> Result<CliSchema, PluginError> {
        Ok(cli_schema())
    }

    async fn cli_invoke(&self, params: CliInvokeParams) -> Result<CliInvokeResult, PluginError> {
        if params.command != "ping" {
            return Ok(CliInvokeResult {
                exit_code: 2,
                stdout: String::new(),
                stderr: format!("unknown command {}", params.command),
                json: None,
            });
        }
        let message = params
            .args
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("hi");
        Ok(CliInvokeResult {
            exit_code: 0,
            stdout: format!("pong: {message}\n"),
            stderr: String::new(),
            json: None,
        })
    }

    async fn call_raw(
        &self,
        method: &str,
        _params: serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        if method == "start" {
            return Ok(serde_json::json!({ "ok": true }));
        }
        Err(PluginError::unsupported(format!(
            "method `{method}` not implemented"
        )))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    serve_native(EchoPlugin).await?;
    Ok(())
}
