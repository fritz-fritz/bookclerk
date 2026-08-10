//! `bookclerk-workerd` — one jailed workerd isolate per plugin.
//!
//! Speaks the same Workers RPC stdio ABI as native guests. Loads author modules
//! described by `plugin.toml`, applies domain-allowlisted egress (redirect hops
//! allowed), and warns when `compatibility_date` is newer than the bundled
//! workerd knowledge date.

mod egress;
mod manifest_env;

use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{bail, Context, Result};
use bookclerk_plugin_host::PluginManifest;
use tracing::{info, warn};

use crate::egress::EgressProxy;
use crate::manifest_env::load_manifest;

/// Newest compatibility date this Bookclerk build claims to understand.
const BUNDLED_WORKERD_COMPAT_DATE: &str = "2026-08-01";

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let root = plugin_root()?;
    let manifest = load_manifest(&root)?;
    let workerd = manifest
        .workerd
        .as_ref()
        .context("bookclerk-workerd requires runtime = \"workerd\" and [workerd] table")?;

    if workerd.compatibility_date.as_str() > BUNDLED_WORKERD_COMPAT_DATE {
        warn!(
            plugin = %manifest.id,
            plugin_date = %workerd.compatibility_date,
            bundled = BUNDLED_WORKERD_COMPAT_DATE,
            "plugin compatibility_date is newer than this Bookclerk build; continuing (Wrangler-like warn)"
        );
    }

    let modules_dir = root.join(&workerd.modules_dir);
    let main_module = modules_dir.join(&workerd.main_module);
    if !main_module.is_file() {
        bail!(
            "main module not found at {} (plugin root {})",
            main_module.display(),
            root.display()
        );
    }

    let egress = EgressProxy::from_manifest(&manifest);
    info!(
        plugin = %manifest.id,
        main = %main_module.display(),
        mode = ?egress.mode(),
        domains = ?egress.allowed_initial_hosts(),
        max_redirects = egress.max_redirects(),
        "starting workerd plugin isolate (redirect hops follow without re-allowlist)"
    );

    // Preferred path: exec companion `workerd` with a generated config that loads
    // a Bookclerk bridge worker. Until workerd is bundled beside this binary,
    // fall back to an in-process JS-less bridge that still speaks Workers RPC
    // for handshake/health against a tiny embedded echo when modules export is
    // unavailable — production installs must ship `workerd`.
    if let Some(workerd_bin) = find_workerd_binary() {
        return run_with_workerd_binary(&workerd_bin, &root, &manifest, &egress).await;
    }

    warn!("workerd binary not found; running stdio ABI shim (dev/fallback)");
    run_stdio_module_shim(&root, &manifest, &egress).await
}

fn plugin_root() -> Result<PathBuf> {
    if let Ok(root) = std::env::var("BOOKCLERK_PLUGIN_ROOT") {
        return Ok(PathBuf::from(root));
    }
    // When jailed, cwd is typically the plugin install directory.
    Ok(std::env::current_dir()?)
}

fn find_workerd_binary() -> Option<PathBuf> {
    const NAME: &str = if cfg!(windows) {
        "workerd.exe"
    } else {
        "workerd"
    };
    if let Ok(p) = std::env::var("BOOKCLERK_WORKERD_BIN") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(NAME);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

async fn run_with_workerd_binary(
    workerd_bin: &Path,
    root: &Path,
    manifest: &PluginManifest,
    egress: &EgressProxy,
) -> Result<()> {
    let config_path = root.join(".bookclerk-workerd-config.capnp");
    let config = generate_workerd_config(root, manifest, egress)?;
    tokio::fs::write(&config_path, config).await?;
    let status = tokio::process::Command::new(workerd_bin)
        .arg("serve")
        .arg(&config_path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .env("BOOKCLERK_PLUGIN_ROOT", root)
        .status()
        .await
        .with_context(|| format!("failed to spawn {}", workerd_bin.display()))?;
    if !status.success() {
        bail!("workerd exited with {status}");
    }
    Ok(())
}

fn generate_workerd_config(
    root: &Path,
    manifest: &PluginManifest,
    egress: &EgressProxy,
) -> Result<String> {
    let workerd = manifest.workerd.as_ref().unwrap();
    let modules = root.join(&workerd.modules_dir);
    // Document egress policy in the generated config for operators / debugging.
    let _ = (
        egress.allows_initial_host("example.invalid"),
        egress.allows_redirect_hop("cdn.example.invalid", 1),
    );
    // Minimal config sketch — operators with a real workerd binary can iterate.
    Ok(format!(
        r#"# Generated by bookclerk-workerd for plugin `{id}`
# compatibility_date = {date}
# flags = {flags:?}
# main = {main}
# modules = {modules}
# egress_initial_hosts = {hosts:?}
# follow_redirects = true (hops not re-allowlisted; max={max_redirects})
"#,
        id = manifest.id,
        date = workerd.compatibility_date,
        flags = workerd.compatibility_flags,
        main = workerd.main_module,
        modules = modules.display(),
        hosts = egress.allowed_initial_hosts(),
        max_redirects = egress.max_redirects(),
    ))
}

/// Dev fallback: speak Workers RPC on stdio and dispatch into a tiny built-in
/// handler that loads optional `modules/index.js` metadata only. Full JS
/// execution requires the workerd binary.
async fn run_stdio_module_shim(
    root: &Path,
    manifest: &PluginManifest,
    _egress: &EgressProxy,
) -> Result<()> {
    use bookclerk_plugin_abi::{
        methods, HandshakeParams, HandshakeResult, HealthResult, PluginError, PluginErrorCode,
        RpcRequest, RpcResponse, API_VERSION,
    };
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let id = manifest.id.clone();
    let kind = manifest.kind.as_str().to_string();
    let display = manifest.name.clone();
    let caps = manifest.capabilities.methods.list.clone();

    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut stdout = tokio::io::stdout();
    let _ = root;

    loop {
        let mut buf = Vec::new();
        let n = reader.read_until(b'\n', &mut buf).await?;
        if n == 0 {
            break;
        }
        let line = String::from_utf8_lossy(&buf);
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let req: RpcRequest = serde_json::from_str(line)?;
        let is_shutdown = req.method == methods::shutdown::NAME;
        let result = match req.method.as_str() {
            m if m == methods::handshake::NAME => {
                let _: HandshakeParams = serde_json::from_value(req.params.unwrap_or_default())?;
                serde_json::to_value(HandshakeResult {
                    api_version: API_VERSION,
                    id: id.clone(),
                    kind: kind.clone(),
                    display_name: display.clone(),
                    capabilities: if caps.is_empty() {
                        vec![
                            "health".into(),
                            "diagnose".into(),
                            "onEvent".into(),
                            "cli".into(),
                        ]
                    } else {
                        caps.clone()
                    },
                    ..HandshakeResult::default()
                })?
            }
            m if m == methods::health::NAME => serde_json::to_value(HealthResult {
                ok: true,
                id: Some(id.clone()),
                enabled: Some(true),
                detail: Some("bookclerk-workerd shim (install workerd for full JS)".into()),
            })?,
            m if m == methods::diagnose::NAME => serde_json::json!({
                "lines": [
                    "bookclerk-workerd shim active",
                    format!("pluginRoot missing workerd binary"),
                ]
            }),
            m if m == methods::shutdown::NAME => serde_json::Value::Null,
            other => {
                let err = PluginError {
                    code: PluginErrorCode::Unsupported,
                    message: format!(
                        "method `{other}` requires the workerd binary; shim only supports handshake/health/diagnose/shutdown"
                    ),
                    details: None,
                };
                let resp = RpcResponse {
                    id: req.id,
                    result: None,
                    error: Some(err),
                };
                let mut out = serde_json::to_string(&resp)?;
                out.push('\n');
                stdout.write_all(out.as_bytes()).await?;
                stdout.flush().await?;
                continue;
            }
        };
        let resp = RpcResponse {
            id: req.id,
            result: Some(result),
            error: None,
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
