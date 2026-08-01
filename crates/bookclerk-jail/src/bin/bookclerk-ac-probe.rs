//! Windows AppContainer probe used by jail integration tests.
//!
//! Reports TokenIsAppContainer, path read/write, cwd/temp, optional TEMP
//! create/read/delete, listen PoC modes, and child-spawn helpers for Job Object
//! tests. On non-Windows hosts this binary exits with an error.

#![cfg_attr(windows, allow(unsafe_code))]

use std::process::ExitCode;

fn main() -> ExitCode {
    #[cfg(not(windows))]
    {
        eprintln!("bookclerk-ac-probe: Windows only");
        ExitCode::from(2)
    }
    #[cfg(windows)]
    {
        match run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("bookclerk-ac-probe: {err}");
                ExitCode::from(1)
            }
        }
    }
}

#[cfg(windows)]
fn run() -> Result<(), String> {
    use std::env;
    use std::fs;
    use std::io::{self, Write};
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::Duration;

    let mut args = env::args().skip(1);
    let mut wait_before: Option<PathBuf> = None;
    let mut wait_after: Option<PathBuf> = None;
    let mut wait_gone: Option<PathBuf> = None;
    let mut wait_timeout = Duration::from_secs(45);
    let mut reads: Vec<PathBuf> = Vec::new();
    let mut writes: Vec<PathBuf> = Vec::new();
    let mut deny_reads: Vec<PathBuf> = Vec::new();
    let mut signal: Option<PathBuf> = None;
    let mut temp_roundtrip = false;
    let mut listen_bind: Option<String> = None;
    let mut listen_status: Option<PathBuf> = None;
    let mut accept_ms: u64 = 5_000;
    let mut https_get: Option<String> = None;
    let mut spawn_child = false;
    let mut exit_immediately = false;
    let mut hold_ms: Option<u64> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--wait-before" => {
                wait_before = Some(PathBuf::from(
                    args.next().ok_or("--wait-before needs a path")?,
                ));
            }
            "--wait-after" => {
                wait_after = Some(PathBuf::from(
                    args.next().ok_or("--wait-after needs a path")?,
                ));
            }
            "--wait-gone" => {
                wait_gone = Some(PathBuf::from(
                    args.next().ok_or("--wait-gone needs a path")?,
                ));
            }
            "--wait-timeout-ms" => {
                let ms: u64 = args
                    .next()
                    .ok_or("--wait-timeout-ms needs a value")?
                    .parse()
                    .map_err(|err| format!("bad --wait-timeout-ms: {err}"))?;
                wait_timeout = Duration::from_millis(ms);
            }
            "--read" => {
                reads.push(PathBuf::from(args.next().ok_or("--read needs a path")?));
            }
            "--write" => {
                writes.push(PathBuf::from(args.next().ok_or("--write needs a path")?));
            }
            "--deny-read" => {
                deny_reads.push(PathBuf::from(
                    args.next().ok_or("--deny-read needs a path")?,
                ));
            }
            "--signal" => {
                signal = Some(PathBuf::from(args.next().ok_or("--signal needs a path")?));
            }
            "--temp-roundtrip" => {
                temp_roundtrip = true;
            }
            "--listen" => {
                listen_bind = Some(args.next().ok_or("--listen needs bind addr")?);
            }
            "--listen-status" => {
                listen_status = Some(PathBuf::from(
                    args.next().ok_or("--listen-status needs a path")?,
                ));
            }
            "--accept-ms" => {
                accept_ms = args
                    .next()
                    .ok_or("--accept-ms needs a value")?
                    .parse()
                    .map_err(|err| format!("bad --accept-ms: {err}"))?;
            }
            "--https-get" => {
                https_get = Some(args.next().ok_or("--https-get needs URL")?);
            }
            "--spawn-child" => {
                spawn_child = true;
            }
            "--exit-immediately" => {
                exit_immediately = true;
            }
            "--hold-ms" => {
                hold_ms = Some(
                    args.next()
                        .ok_or("--hold-ms needs a value")?
                        .parse()
                        .map_err(|err| format!("bad --hold-ms: {err}"))?,
                );
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    if exit_immediately {
        return Ok(());
    }

    if let Some(path) = &wait_before {
        wait_until(
            || path.exists(),
            wait_timeout,
            &format!("wait-before {}", path.display()),
        )?;
    }

    let is_app_container = token_is_app_container()?;
    let cwd = env::current_dir().map_err(|err| err.to_string())?;
    let localappdata = env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let temp = env::var_os("TEMP").map(PathBuf::from);
    let tmp = env::var_os("TMP").map(PathBuf::from);

    let mut read_results = Vec::new();
    for path in &reads {
        read_results.push(serde_json::json!({
            "path": path.display().to_string(),
            "ok": fs::read(path).is_ok(),
        }));
    }

    let mut write_results = Vec::new();
    for path in &writes {
        let marker = path.join("probe-write.txt");
        let ok = fs::write(&marker, b"probe").is_ok();
        if ok {
            let _ = fs::remove_file(&marker);
        }
        write_results.push(serde_json::json!({
            "path": path.display().to_string(),
            "ok": ok,
        }));
    }

    let mut deny_results = Vec::new();
    for path in &deny_reads {
        deny_results.push(serde_json::json!({
            "path": path.display().to_string(),
            "ok": fs::read(path).is_ok(),
        }));
    }

    let mut temp_ok = None;
    if temp_roundtrip {
        let dir = temp
            .clone()
            .ok_or_else(|| "TEMP unset for --temp-roundtrip".to_string())?;
        fs::create_dir_all(&dir)
            .map_err(|err| format!("TEMP create_dir_all {}: {err}", dir.display()))?;
        let marker = dir.join("bookclerk-temp-roundtrip.txt");
        fs::write(&marker, b"temp-ok")
            .map_err(|err| format!("TEMP write {}: {err}", marker.display()))?;
        let body = fs::read(&marker).map_err(|err| format!("TEMP read: {err}"))?;
        fs::remove_file(&marker).map_err(|err| format!("TEMP delete: {err}"))?;
        temp_ok = Some(body == b"temp-ok");
    }

    let mut child_pid = None;
    if spawn_child {
        let child = Command::new(env::current_exe().map_err(|err| err.to_string())?)
            .arg("--hold-ms")
            .arg("5000")
            .spawn()
            .map_err(|err| format!("spawn child: {err}"))?;
        child_pid = Some(child.id());
        // Intentionally leak the Child handle so the descendant stays alive for
        // Job membership checks by the host test.
        std::mem::forget(child);
    }

    let (listen_prep, listener) = if let Some(addr) = &listen_bind {
        prepare_listen(addr)?
    } else {
        (None, None)
    };

    // Side-channel for hosts that must learn the bound port before guest EOF
    // (stdout may still be mid-proxy). Written immediately after bind.
    if let (Some(path), Some(listen)) = (&listen_status, &listen_prep) {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let body = serde_json::to_vec(listen).map_err(|err| err.to_string())?;
        fs::write(path, body).map_err(|err| format!("listen-status {}: {err}", path.display()))?;
    }

    let https_result = if let Some(url) = &https_get {
        Some(try_https_get(url)?)
    } else {
        None
    };

    let report = serde_json::json!({
        "is_app_container": is_app_container,
        "pid": std::process::id(),
        "cwd": cwd.display().to_string(),
        "localappdata": localappdata.as_ref().map(|p| p.display().to_string()),
        "temp": temp.as_ref().map(|p| p.display().to_string()),
        "tmp": tmp.as_ref().map(|p| p.display().to_string()),
        "reads": read_results,
        "writes": write_results,
        "deny_reads": deny_results,
        "temp_roundtrip_ok": temp_ok,
        "child_pid": child_pid,
        "listen": listen_prep,
        "https_get": https_result,
    });

    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{report}").map_err(|err| err.to_string())?;
    stdout.flush().map_err(|err| err.to_string())?;

    if let Some(path) = &signal {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        fs::write(path, b"ready").map_err(|err| format!("signal {}: {err}", path.display()))?;
    }

    // After publishing the bound address, accept one connection (listen PoC).
    if let Some(listener) = listener {
        let accept = accept_once(listener, Duration::from_millis(accept_ms))?;
        writeln!(stdout, "{accept}").map_err(|err| err.to_string())?;
        stdout.flush().map_err(|err| err.to_string())?;
    }

    if let Some(ms) = hold_ms {
        std::thread::sleep(Duration::from_millis(ms));
    }

    if let Some(path) = &wait_after {
        wait_until(
            || path.exists(),
            wait_timeout,
            &format!("wait-after {}", path.display()),
        )?;
    }

    if let Some(path) = &wait_gone {
        wait_until(
            || !path.exists(),
            wait_timeout,
            &format!("wait-gone {}", path.display()),
        )?;
        if let Some(first) = writes.first() {
            let marker = first.join("probe-after-peer.txt");
            fs::write(&marker, b"still-ok")
                .map_err(|err| format!("post-peer write failed: {err}"))?;
            let _ = fs::remove_file(&marker);
        }
        let follow_up = serde_json::json!({
            "phase": "after-peer-exit",
            "writes_still_ok": true,
        });
        writeln!(stdout, "{follow_up}").map_err(|err| err.to_string())?;
        stdout.flush().map_err(|err| err.to_string())?;
    }

    Ok(())
}

#[cfg(windows)]
fn try_https_get(url: &str) -> Result<serde_json::Value, String> {
    use std::net::{TcpStream, ToSocketAddrs};
    use std::time::Duration;

    // Minimal TLS-free outbound check: resolve host and open TCP:443.
    // Full HTTPS is unnecessary for the Phase 0 "outbound still works" gate.
    let host = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("");
    if host.is_empty() {
        return Ok(serde_json::json!({ "ok": false, "error": "empty host", "url": url }));
    }
    let addr = format!("{host}:443");
    match TcpStream::connect_timeout(
        &addr
            .to_socket_addrs()
            .map_err(|err| format!("resolve {addr}: {err}"))?
            .next()
            .ok_or_else(|| format!("no addrs for {addr}"))?,
        Duration::from_secs(5),
    ) {
        Ok(_) => Ok(serde_json::json!({ "ok": true, "url": url, "tcp443": true })),
        Err(err) => Ok(serde_json::json!({ "ok": false, "url": url, "error": err.to_string() })),
    }
}

#[cfg(windows)]
fn prepare_listen(
    addr: &str,
) -> Result<(Option<serde_json::Value>, Option<std::net::TcpListener>), String> {
    use std::net::TcpListener;

    match TcpListener::bind(addr) {
        Ok(listener) => {
            let local = listener
                .local_addr()
                .map_err(|err| format!("local_addr: {err}"))?;
            Ok((
                Some(serde_json::json!({
                    "bind_ok": true,
                    "bound": local.to_string(),
                    "addr": addr,
                })),
                Some(listener),
            ))
        }
        Err(err) => Ok((
            Some(serde_json::json!({
                "bind_ok": false,
                "error": err.to_string(),
                "addr": addr,
            })),
            None,
        )),
    }
}

#[cfg(windows)]
fn accept_once(
    listener: std::net::TcpListener,
    timeout: std::time::Duration,
) -> Result<serde_json::Value, String> {
    use std::io::{Read, Write};
    use std::time::Duration;

    listener
        .set_nonblocking(true)
        .map_err(|err| format!("set_nonblocking: {err}"))?;
    let start = std::time::Instant::now();
    let mut accepted = false;
    let mut accept_error = None;
    while start.elapsed() < timeout {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let mut buf = [0u8; 256];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
                accepted = true;
                break;
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(err) => {
                accept_error = Some(err.to_string());
                break;
            }
        }
    }
    Ok(serde_json::json!({
        "phase": "listen-accept",
        "accepted": accepted,
        "accept_error": accept_error,
    }))
}

#[cfg(windows)]
fn wait_until(
    mut pred: impl FnMut() -> bool,
    timeout: std::time::Duration,
    label: &str,
) -> Result<(), String> {
    let start = std::time::Instant::now();
    while !pred() {
        if start.elapsed() > timeout {
            return Err(format!("timed out: {label}"));
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Ok(())
}

#[cfg(windows)]
fn token_is_app_container() -> Result<bool, String> {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{GetTokenInformation, TokenIsAppContainer, TOKEN_QUERY};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)
            .map_err(|err| format!("OpenProcessToken failed: {err}"))?;
        let mut is_ac: u32 = 0;
        let mut returned = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenIsAppContainer,
            Some((&mut is_ac as *mut u32).cast()),
            std::mem::size_of::<u32>() as u32,
            &mut returned,
        );
        let _ = CloseHandle(token);
        ok.map_err(|err| format!("GetTokenInformation(TokenIsAppContainer) failed: {err}"))?;
        Ok(is_ac != 0)
    }
}
