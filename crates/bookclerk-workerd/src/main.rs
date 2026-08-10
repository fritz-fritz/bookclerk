//! `bookclerk-workerd` — one jailed workerd isolate per plugin.
//!
//! Speaks the same Workers RPC stdio ABI as native guests. Loads author modules
//! via a pinned Cloudflare `workerd` binary, applies domain-allowlisted egress
//! (redirect hops allowed), and warns when `compatibility_date` is newer than
//! the bundled knowledge date.

mod manifest_env;

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use bookclerk_plugin_abi::{methods, PluginError, PluginErrorCode, RpcRequest, RpcResponse};
use bookclerk_plugin_manifest::PluginManifest;
use bookclerk_workerd::config::{self, ListenSpec};
use bookclerk_workerd::egress::EgressProxy;
use bookclerk_workerd::ensure::ensure_workerd;
use bookclerk_workerd::pin::{binary_name, BUNDLED_WORKERD_COMPAT_DATE, WORKERD_RELEASE_TAG};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
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
    let state_dir = config::workerd_state_dir(root)?;
    let notify_events: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(Vec::new()));

    #[cfg(unix)]
    let (listen, notify_addr, notify_task) = {
        let rpc_sock = state_dir.join("rpc.sock");
        let notify_sock = state_dir.join("notify.sock");
        let _ = std::fs::remove_file(&rpc_sock);
        let _ = std::fs::remove_file(&notify_sock);
        let notify_addr = format!("unix:{}", notify_sock.display());
        let notify_task = spawn_notify_unix(notify_sock, Arc::clone(&notify_events));
        (
            ListenSpec::Unix(rpc_sock),
            Some(notify_addr),
            Some(notify_task),
        )
    };

    #[cfg(not(unix))]
    let (listen, notify_addr, notify_task) = {
        let port = free_loopback_port()?;
        let notify_listener = std::net::TcpListener::bind("127.0.0.1:0")
            .context("bind HOST.notify ephemeral loopback")?;
        let notify_port = notify_listener.local_addr()?.port();
        let notify_addr = format!("127.0.0.1:{notify_port}");
        let notify_task = spawn_notify_tcp(notify_listener, Arc::clone(&notify_events));
        (
            ListenSpec::TcpLoopback(port),
            Some(notify_addr),
            Some(notify_task),
        )
    };

    let generated = config::materialize(root, manifest, egress, listen, notify_addr.as_deref())?;

    let mut child = tokio::process::Command::new(workerd_bin)
        .arg("serve")
        .arg(&generated.config_path)
        // Config embeds use paths relative to the config file (state dir) and
        // absolute paths into the plugin root for author modules.
        .current_dir(&generated.state_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("BOOKCLERK_PLUGIN_ROOT", root)
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("spawn {}", workerd_bin.display()))?;

    forward_child_logs(&mut child);

    wait_for_bridge(&generated.listen)
        .await
        .context("workerd bridge /health did not become ready")?;

    let result = mediate_stdio(&generated.listen).await;
    let _ = child.kill().await;
    let _ = child.wait().await;
    if let Some(task) = notify_task {
        task.abort();
    }
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

#[cfg(not(unix))]
fn free_loopback_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").context("bind ephemeral loopback")?;
    Ok(listener.local_addr()?.port())
}

#[cfg(unix)]
fn spawn_notify_unix(
    path: PathBuf,
    events: Arc<Mutex<Vec<serde_json::Value>>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let listener = match tokio::net::UnixListener::bind(&path) {
            Ok(l) => l,
            Err(err) => {
                warn!(
                    error = %err,
                    path = %path.display(),
                    "HOST.notify unix listener failed to bind"
                );
                return;
            }
        };
        info!(path = %path.display(), "HOST.notify reverse channel listening");
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

#[cfg(not(unix))]
fn spawn_notify_tcp(
    std_listener: std::net::TcpListener,
    events: Arc<Mutex<Vec<serde_json::Value>>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // Keep the already-bound port-0 socket (do not rebind a concrete port).
        if let Err(err) = std_listener.set_nonblocking(true) {
            warn!(error = %err, "HOST.notify set_nonblocking failed");
            return;
        }
        let listener = match tokio::net::TcpListener::from_std(std_listener) {
            Ok(l) => l,
            Err(err) => {
                warn!(error = %err, "HOST.notify from_std failed");
                return;
            }
        };
        info!("HOST.notify reverse channel listening");
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

async fn handle_notify_connection<S>(
    stream: &mut S,
    events: &Mutex<Vec<serde_json::Value>>,
) -> Result<()>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin,
{
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

async fn wait_for_bridge(listen: &ListenSpec) -> Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        match bridge_get(listen, "/health").await {
            Ok(_) => return Ok(()),
            Err(_) => {
                if tokio::time::Instant::now() > deadline {
                    bail!("timeout waiting for bridge /health on {:?}", listen);
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }
}

async fn mediate_stdio(listen: &ListenSpec) -> Result<()> {
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut stdout = tokio::io::stdout();

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
        let resp = forward_rpc(listen, &req)
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

async fn forward_rpc(listen: &ListenSpec, req: &RpcRequest) -> Result<RpcResponse> {
    let body = serde_json::to_vec(req)?;
    let text = bridge_post(listen, "/rpc", &body).await?;
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
}

async fn bridge_get(listen: &ListenSpec, path: &str) -> Result<String> {
    match listen {
        ListenSpec::TcpLoopback(port) => {
            let url = format!("http://127.0.0.1:{port}{path}");
            let url_owned = url.clone();
            tokio::task::spawn_blocking(move || {
                let mut response = ureq::get(&url_owned).call().with_context(|| url_owned)?;
                if !response.status().is_success() {
                    bail!("HTTP {}", response.status());
                }
                response
                    .body_mut()
                    .read_to_string()
                    .context("read health body")
            })
            .await?
        }
        #[cfg(unix)]
        ListenSpec::Unix(sock) => http_unix(sock, "GET", path, None).await,
        #[cfg(not(unix))]
        ListenSpec::Unix(_) => bail!("unix listen is not supported on this platform"),
    }
}

async fn bridge_post(listen: &ListenSpec, path: &str, body: &[u8]) -> Result<String> {
    match listen {
        ListenSpec::TcpLoopback(port) => {
            let url = format!("http://127.0.0.1:{port}{path}");
            let url_owned = url.clone();
            let body = body.to_vec();
            tokio::task::spawn_blocking(move || {
                let mut response = ureq::post(&url_owned)
                    .header("content-type", "application/json")
                    .send(body)
                    .with_context(|| format!("POST {url_owned}"))?;
                let status = response.status();
                let text = response
                    .body_mut()
                    .read_to_string()
                    .context("read bridge body")?;
                if !status.is_success() && status.as_u16() != 400 {
                    bail!("bridge HTTP {status}: {text}");
                }
                Ok(text)
            })
            .await?
        }
        #[cfg(unix)]
        ListenSpec::Unix(sock) => http_unix(sock, "POST", path, Some(body)).await,
        #[cfg(not(unix))]
        ListenSpec::Unix(_) => bail!("unix listen is not supported on this platform"),
    }
}

#[cfg(unix)]
async fn http_unix(sock: &Path, method: &str, path: &str, body: Option<&[u8]>) -> Result<String> {
    use tokio::net::UnixStream;

    let mut stream = UnixStream::connect(sock)
        .await
        .with_context(|| format!("connect {}", sock.display()))?;
    let body = body.unwrap_or(b"");
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    stream.write_all(req.as_bytes()).await?;
    if !body.is_empty() {
        stream.write_all(body).await?;
    }
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await?;
    let raw = String::from_utf8_lossy(&buf);
    let (_headers, resp_body) = raw
        .split_once("\r\n\r\n")
        .or_else(|| raw.split_once("\n\n"))
        .context("incomplete HTTP response from workerd")?;
    // Status line check
    let status_line = raw.lines().next().unwrap_or("");
    if !(status_line.contains(" 200 ") || status_line.contains(" 400 ")) {
        // Allow empty-body 200 from /health as well as bridge RPC.
        if !status_line.contains(" 200") {
            bail!("bridge unix HTTP: {status_line}");
        }
    }
    Ok(resp_body.to_string())
}

fn plugin_error_code(code: &str) -> PluginErrorCode {
    match code {
        "unsupported" => PluginErrorCode::Unsupported,
        "invalid_params" | "invalidParams" => PluginErrorCode::InvalidParams,
        "unauthorized" => PluginErrorCode::Unauthorized,
        "not_found" | "notFound" => PluginErrorCode::NotFound,
        "forbidden" => PluginErrorCode::Forbidden,
        "unavailable" => PluginErrorCode::Unavailable,
        _ => PluginErrorCode::Internal,
    }
}
