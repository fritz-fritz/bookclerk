//! Proof that `bookclerk-jail` on Windows launches the guest inside an
//! AppContainer whose filesystem allowlist actually holds.
//!
//! Self-confine helpers cannot prove this (Windows has no post-start jail), so
//! the assertions run in a `cmd.exe` guest started through the launcher.

#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::Command;

use bookclerk_sandbox::{Enforcement, NetPolicy, Spec};

const JAIL: &str = env!("CARGO_BIN_EXE_bookclerk-jail");

fn write_secret(dir: &Path) -> PathBuf {
    let secret = dir.join("secret.txt");
    std::fs::write(&secret, b"top-secret").expect("write secret");
    secret
}

/// Guest that can write under ALLOWED and must fail to read SECRET.
fn guest_batch(path: &Path) {
    // `%1` = allowed dir, `%2` = secret file
    let body = r#"
@echo off
setlocal
set "ALLOWED=%~1"
set "SECRET=%~2"
echo ok> "%ALLOWED%\out.txt"
if errorlevel 1 exit /b 11
type "%SECRET%" >nul 2>&1
if not errorlevel 1 exit /b 22
exit /b 0
"#;
    std::fs::write(path, body).expect("write batch");
}

#[test]
fn appcontainer_guest_cannot_read_outside_allowlist() {
    let root = tempfile::tempdir().expect("tempdir");
    let allowed = root.path().join("allowed");
    let forbidden = root.path().join("forbidden");
    std::fs::create_dir_all(&allowed).expect("allowed");
    std::fs::create_dir_all(&forbidden).expect("forbidden");
    let secret = write_secret(&forbidden);
    let batch = root.path().join("guest.bat");
    guest_batch(&batch);

    let comspec =
        std::env::var_os("ComSpec").unwrap_or_else(|| r"C:\Windows\System32\cmd.exe".into());
    let spec = Spec {
        label: "test:windows-ac".into(),
        reads: vec![batch.clone(), PathBuf::from(r"C:\Windows\System32")],
        writes: vec![allowed.clone()],
        net: NetPolicy::Deny,
        allow_exec: true,
        system_paths: false,
        enforcement: Enforcement::Required,
        preserve_fds: vec![],
    };

    let output = Command::new(JAIL)
        .arg(&comspec)
        .arg("/C")
        .arg(&batch)
        .arg(&allowed)
        .arg(&secret)
        .env(
            bookclerk_sandbox::SPEC_ENV,
            serde_json::to_string(&spec).expect("encode"),
        )
        .output()
        .expect("run bookclerk-jail");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "jail/guest failed: status={:?}\nstderr={stderr}\nstdout={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        allowed.join("out.txt").is_file(),
        "guest should write inside allowlist; stderr={stderr}"
    );
    assert!(
        stderr.contains("AppContainer"),
        "expected AppContainer log line, got: {stderr}"
    );
}

#[test]
fn required_refuses_missing_allowlist_path() {
    let root = tempfile::tempdir().expect("tempdir");
    let missing = root.path().join("nope");
    let comspec =
        std::env::var_os("ComSpec").unwrap_or_else(|| r"C:\Windows\System32\cmd.exe".into());
    let spec = Spec {
        label: "test:windows-missing".into(),
        reads: vec![missing],
        writes: vec![],
        net: NetPolicy::Deny,
        allow_exec: true,
        system_paths: false,
        enforcement: Enforcement::Required,
        preserve_fds: vec![],
    };

    let output = Command::new(JAIL)
        .arg(&comspec)
        .arg("/C")
        .arg("echo hi")
        .env(
            bookclerk_sandbox::SPEC_ENV,
            serde_json::to_string(&spec).expect("encode"),
        )
        .output()
        .expect("run bookclerk-jail");

    assert!(
        !output.status.success(),
        "missing allowlist path must fail closed"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("do not exist"),
        "expected missing-path error, got: {stderr}"
    );
}
