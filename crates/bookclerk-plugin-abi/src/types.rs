//! Shared ABI DTOs (camelCase on the wire).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// `handshake` params.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HandshakeParams {
    /// API version.
    pub api_version: u32,
    /// Config.
    #[serde(default)]
    pub config: Value,
}

/// `handshake` result.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HandshakeResult {
    /// API version.
    pub api_version: u32,
    /// Identifier.
    pub id: String,
    /// Kind.
    pub kind: String,
    /// Display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Capabilities.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Portal auth mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub portal_auth_mode: Option<String>,
    /// Password env var.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_env_var: Option<String>,
    /// Aliases.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// Sort key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_key: Option<u32>,
    /// Brand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brand: Option<BrandDto>,
    /// Config options.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub config_options: Vec<ConfigOptionDto>,
    /// CLI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli: Option<CliSchema>,
}

/// Portal brand crossing the RPC boundary (owned strings).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BrandDto {
    /// Identifier.
    pub id: String,
    /// Name.
    pub name: String,
    /// Bg.
    pub bg: String,
    /// Fg.
    pub fg: String,
    /// Accent.
    pub accent: String,
    /// Icon URL.
    pub icon_url: String,
}

/// Config option discovery for sources.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConfigOptionDto {
    /// Key.
    pub key: String,
    /// Label.
    pub label: String,
    /// Values.
    pub values: Vec<ConfigOptionValueDto>,
}

/// One selectable value under a [`ConfigOptionDto`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConfigOptionValueDto {
    /// Identifier.
    pub id: String,
    /// Label.
    pub label: String,
}

/// Declared CLI surface.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CliSchema {
    /// Commands.
    #[serde(default)]
    pub commands: Vec<CliCommandSpec>,
}

/// One plugin CLI command.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CliCommandSpec {
    /// Name.
    pub name: String,
    /// About.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub about: Option<String>,
    /// Args.
    #[serde(default)]
    pub args: Vec<CliArgSpec>,
}

/// CLI argument kind.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum CliArgKind {
    /// String variant.
    #[default]
    String,
    /// Bool variant.
    Bool,
    /// Int variant.
    Int,
    /// Path variant.
    Path,
}

/// One CLI argument.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CliArgSpec {
    /// Name.
    pub name: String,
    /// Long.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long: Option<String>,
    /// Short.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short: Option<char>,
    /// Kind.
    #[serde(default)]
    pub kind: CliArgKind,
    /// Required.
    #[serde(default)]
    pub required: bool,
    /// Default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// About.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub about: Option<String>,
    /// Positional.
    #[serde(default)]
    pub positional: bool,
}

/// `cliInvoke` params.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CliInvokeParams {
    /// Command.
    pub command: String,
    /// Args.
    #[serde(default)]
    pub args: Map<String, Value>,
}

/// `cliInvoke` result.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CliInvokeResult {
    /// Exit code.
    #[serde(default)]
    pub exit_code: i32,
    /// Stdout.
    #[serde(default)]
    pub stdout: String,
    /// Stderr.
    #[serde(default)]
    pub stderr: String,
    /// JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json: Option<Value>,
}

/// `health` result (branded trait / schema — optional identity fields).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HealthResult {
    /// Ok.
    #[serde(default)]
    pub ok: bool,
    /// Identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Detail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Serializable health payload used by host adapters (required `id` / `enabled`).
///
/// Prefer [`HealthResult`] for new branded guests; this shape remains for
/// existing host deserialization and first-party guests.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HealthDto {
    /// Identifier.
    pub id: String,
    /// Enabled.
    pub enabled: bool,
    /// Ok.
    pub ok: bool,
    /// Detail.
    #[serde(default)]
    pub detail: Option<String>,
}

/// `diagnose` result.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnoseResult {
    /// Lines.
    #[serde(default)]
    pub lines: Vec<String>,
}

/// Stdio Workers RPC request frame (no `jsonrpc` field).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    /// Identifier.
    pub id: Value,
    /// Method.
    pub method: String,
    /// Params.
    #[serde(default)]
    pub params: Option<Value>,
}

/// Stdio Workers RPC response frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    /// Identifier.
    pub id: Value,
    /// Result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<crate::PluginError>,
}
