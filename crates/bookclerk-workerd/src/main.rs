//! `bookclerk-workerd` — one jailed workerd isolate per plugin.
//!
//! Speaks the same Workers RPC stdio ABI as native guests. Loads author modules
//! via a pinned Cloudflare `workerd` binary, applies domain-allowlisted egress
//! (redirect hops allowed), and warns when `compatibility_date` is newer than
//! the bundled knowledge date.

mod manifest_env;

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use bookclerk_plugin_abi::{methods, PluginError, PluginErrorCode, RpcRequest, RpcResponse};
use bookclerk_plugin_manifest::PluginManifest;
use bookclerk_workerd::config;
use bookclerk_workerd::egress::EgressProxy;
use bookclerk_workerd::ensure::ensure_workerd;
use bookclerk_workerd::pin::{binary_name, BUNDLED_WORKERD_COMPAT_DATE, WORKERD_RELEASE_TAG};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener as TokioTcpListener;
use tokio::process::Child;
use tracing::{info, warn};

use crate::manifest_env::load_manifest;

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
    let workerd_meta = manifest
        .workerd
        .as_ref()
        .context("bookclerk-workerd requires runtime = \"workerd\" and [workerd] table")?;

    if workerd_meta.compatibility_date.as_str() > BUNDLED_WORKERD_COMPAT_DATE {
        warn!(
            plugin = %manifest.id,
            plugin_date = %workerd_meta.compatibility_date,
            bundled = BUNDLED_WORKERD_COMPAT_DATE,
            "plugin compatibility_date is newer than this Bookclerk build; continuing (Wrangler-like warn)"
        );
    }

    let modules_dir = root.join(&workerd_meta.modules_dir);
    let main_module = modules_dir.join(&workerd_meta.main_module);
    if !main_module.is_file() {
        bail!(
            "main module not found at {} (plugin root {})",
            main_module.display(),
            root.display()
        );
    }

    let workerd_bin = resolve_workerd_binary()?;
    let egress = EgressProxy::from_manifest(&manifest);
    info!(
        plugin = %manifest.id,
        main = %main_module.display(),
        workerd = %workerd_bin.display(),
        pin = WORKERD_RELEASE_TAG,
        mode = ?egress.mode(),
        domains = ?egress.allowed_initial_hosts(),
        "starting workerd plugin isolate"
    );

    run_isolate(&workerd_bin, &root, &manifest, &egress).await
}

fn plugin_root() -> Result<PathBuf> {
    if let Ok(root) = std::env::var("BOOKCLERK_PLUGIN_ROOT") {
        return Ok(PathBuf::from(root));
    }
    Ok(std::env::current_dir()?)
}

fn resolve_workerd_binary() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("BOOKCLERK_WORKERD_BIN") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Ok(path);
        }
        bail!(
            "BOOKCLERK_WORKERD_BIN={} is not a file; run `cargo ensure-workerd` (or build-app/dev)",
            path.display()
        );
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(binary_name());
            if candidate.is_file() {
                return Ok(candidate);
            }
            match ensure_workerd(dir) {
                Ok(path) => return Ok(path),
                Err(err) => {
                    warn!(
                        error = %err,
                        "ensure_workerd beside launcher failed; workerd binary required"
                    );
                }
            }
        }
    }
    bail!(
        "workerd binary not found (pin {WORKERD_RELEASE_TAG}). \
         Run `cargo ensure-workerd` or `cargo build-app --platform` / `cargo dev` first."
    )
}

async fn run_isolate(
    workerd_bin: &Path,
    root: &Path,
    manifest: &PluginManifest,
    egress: &EgressProxy,
) -> Result<()> {
    let port = free_loopback_port()?;
    let notify_port = free_loopback_port()?;
    let notify_addr = format!("127.0.0.1:{notify_port}");
    let notify_events: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(Vec::new()));
    let notify_task = spawn_notify_server(notify_port, Arc::clone(&notify_events));

    let generated = config::materialize(root, manifest, egress, port, Some(notify_addr.as_str()))?;
    let base = format!("http://{}", generated.listen_addr);

    let mut child = tokio::process::Command::new(workerd_bin)
        .arg("serve")
        .arg(&generated.config_path)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("BOOKCLERK_PLUGIN_ROOT", root)
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("spawn {}", workerd_bin.display()))?;

    forward_child_logs(&mut child);

    wait_for_health(&base)
        .await
        .context("workerd bridge /health did not become ready")?;

    let result = mediate_stdio(&base).await;
    let _ = child.kill().await;
    let _ = child.wait().await;
    notify_task.abort();
    let buffered = notify_events.lock().map(|g| g.len()).unwrap_or(0);
    if buffered > 0 {
        info!(
            plugin = %manifest.id,
            events = buffered,
            "HOST.notify reverse-channel events buffered this session"
        );
    }
    result
}

fn free_loopback_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").context("bind ephemeral loopback")?;
    Ok(listener.local_addr()?.port())
}

fn spawn_notify_server(
    port: u16,
    events: Arc<Mutex<Vec<serde_json::Value>>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let listener = match TokioTcpListener::bind(("127.0.0.1", port)).await {
            Ok(l) => l,
            Err(err) => {
                warn!(error = %err, port, "HOST.notify listener failed to bind");
                return;
            }
        };
        info!(port, "HOST.notify reverse channel listening");
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                continue;
            };
            let events = Arc::clone(&events);
            tokio::spawn(async move {
                if let Err(err) = handle_notify_connection(&mut stream, &events).await {
                    warn!(error = %err, "HOST.notify request failed");
                }
            });
        }
    })
}

async fn handle_notify_connection(
    stream: &mut tokio::net::TcpStream,
    events: &Mutex<Vec<serde_json::Value>>,
) -> Result<()> {
    let mut buf = vec![0u8; 65536];
    let n = stream.read(&mut buf).await?;
    if n == 0 {
        return Ok(());
    }
    let raw = String::from_utf8_lossy(&buf[..n]);
    let (status, body) = match parse_notify_http(&raw) {
        Ok(event) => {
            info!(event = %event, "HOST.notify");
            if let Ok(mut guard) = events.lock() {
                guard.push(event);
            }
            (200u16, "ok")
        }
        Err(err) => {
            warn!(error = %err, "HOST.notify bad request");
            (400, "bad request")
        }
    };
    let resp = format!(
        "HTTP/1.1 {status} {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        if status == 200 { "OK" } else { "Bad Request" },
        body.len()
    );
    stream.write_all(resp.as_bytes()).await?;
    Ok(())
}

fn parse_notify_http(raw: &str) -> Result<serde_json::Value> {
    let (headers, body) = raw
        .split_once("\r\n\r\n")
        .or_else(|| raw.split_once("\n\n"))
        .context("incomplete HTTP request")?;
    let request_line = headers.lines().next().unwrap_or("");
    if !request_line.starts_with("POST ") {
        bail!("expected POST");
    }
    if !request_line.contains("/notify") {
        bail!("expected /notify path");
    }
    let body = body.trim_start_matches('\u{feff}').trim();
    if body.is_empty() {
        return Ok(serde_json::Value::Null);
    }
    serde_json::from_str(body).context("notify JSON body")
}

fn forward_child_logs(child: &mut Child) {
    if let Some(stdout) = child.stdout.take() {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                eprintln!("workerd: {line}");
            }
        });
    }
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                eprintln!("workerd: {line}");
            }
        });
    }
}

async fn wait_for_health(base: &str) -> Result<()> {
    let url = format!("{base}/health");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        match ureq::get(&url).call() {
            Ok(resp) if resp.status().is_success() => return Ok(()),
            _ => {
                if tokio::time::Instant::now() > deadline {
                    bail!("timeout waiting for {url}");
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }
}

async fn mediate_stdio(base: &str) -> Result<()> {
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut stdout = tokio::io::stdout();
    let rpc_url = format!("{base}/rpc");

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
        let resp = forward_rpc(&rpc_url, &req)
            .await
            .unwrap_or_else(|err| RpcResponse {
                id: req.id.clone(),
                result: None,
                error: Some(PluginError {
                    code: PluginErrorCode::Internal,
                    message: err.to_string(),
                    details: None,
                }),
            });
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

async fn forward_rpc(url: &str, req: &RpcRequest) -> Result<RpcResponse> {
    let url = url.to_string();
    let body = serde_json::to_value(req)?;
    tokio::task::spawn_blocking(move || {
        let mut response = ureq::post(&url)
            .header("content-type", "application/json")
            .send_json(&body)
            .with_context(|| format!("POST {url}"))?;
        let status = response.status();
        let text = response
            .body_mut()
            .read_to_string()
            .context("read bridge body")?;
        if !status.is_success() && status.as_u16() != 400 {
            bail!("bridge HTTP {status}: {text}");
        }
        let value: serde_json::Value = serde_json::from_str(&text).context("parse bridge JSON")?;
        if let Some(err) = value.get("error") {
            let code = err
                .get("code")
                .and_then(|c| c.as_str())
                .unwrap_or("internal");
            let message = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("bridge error")
                .to_string();
            return Ok(RpcResponse {
                id: value.get("id").cloned().unwrap_or(serde_json::Value::Null),
                result: None,
                error: Some(PluginError {
                    code: plugin_error_code(code),
                    message,
                    details: None,
                }),
            });
        }
        Ok(RpcResponse {
            id: value.get("id").cloned().unwrap_or(serde_json::Value::Null),
            result: value.get("result").cloned(),
            error: None,
        })
    })
    .await?
}

fn plugin_error_code(code: &str) -> PluginErrorCode {
    match code {
        "unsupported" => PluginErrorCode::Unsupported,
        "invalid_params" => PluginErrorCode::InvalidParams,
        "unauthorized" => PluginErrorCode::Unauthorized,
        "not_found" => PluginErrorCode::NotFound,
        "forbidden" => PluginErrorCode::Forbidden,
        _ => PluginErrorCode::Internal,
    }
}
