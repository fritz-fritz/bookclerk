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

fn spawn_enforcement_demanded() -> bool {
    std::env::var("BOOKCLERK_SANDBOX_REQUIRE_SPAWN_ENFORCEMENT")
        .is_ok_and(|value| !value.trim().is_empty())
}

fn assert_spawn_capable() {
    let caps = bookclerk_sandbox::capabilities();
    assert!(
        caps.spawn_filesystem || !spawn_enforcement_demanded(),
        "BOOKCLERK_SANDBOX_REQUIRE_SPAWN_ENFORCEMENT is set but spawn_filesystem \
         is false: {} [{}]",
        caps.detail,
        caps.backend
    );
    assert!(
        caps.spawn_filesystem,
        "Windows AppContainer tests require spawn_filesystem; got {} [{}]",
        caps.detail, caps.backend
    );
}

fn write_secret(dir: &Path) -> PathBuf {
    let secret = dir.join("secret.txt");
    std::fs::write(&secret, b"top-secret").expect("write secret");
    secret
}

fn comspec() -> PathBuf {
    PathBuf::from(
        std::env::var_os("ComSpec").unwrap_or_else(|| r"C:\Windows\System32\cmd.exe".into()),
    )
}

/// Guest that can write under ALLOWED and must fail to read SECRET.
fn guest_batch_read_deny(path: &Path) {
    let body = r#"
@echo off
setlocal
set "ALLOWED=%~1"
set "SECRET=%~2"
echo ok> "%ALLOWED%\out.txt"
if errorlevel 1 exit /b 11
type "%SECRET%" >nul 2>&1
if not errorlevel 1 exit /b 22
if defined BOOKCLERK_JAIL_SPEC (
  echo guest can see its own jail spec >&2
  exit /b 33
)
exit /b 0
"#;
    std::fs::write(path, body).expect("write batch");
}

/// Guest that must fail to write outside the allowlist.
fn guest_batch_write_deny(path: &Path) {
    let body = r#"
@echo off
setlocal
set "ALLOWED=%~1"
set "FORBIDDEN=%~2"
echo ok> "%ALLOWED%\out.txt"
if errorlevel 1 exit /b 11
echo leaked> "%FORBIDDEN%\pwned.txt" 2>nul
if exist "%FORBIDDEN%\pwned.txt" exit /b 22
exit /b 0
"#;
    std::fs::write(path, body).expect("write batch");
}

fn run_jailed(spec: &Spec, program: &Path, args: &[&Path]) -> std::process::Output {
    let mut cmd = Command::new(JAIL);
    cmd.arg(program);
    for arg in args {
        cmd.arg(arg);
    }
    cmd.env(
        bookclerk_sandbox::SPEC_ENV,
        serde_json::to_string(spec).expect("encode"),
    )
    .output()
    .expect("run bookclerk-jail")
}

#[test]
fn appcontainer_guest_cannot_read_outside_allowlist() {
    assert_spawn_capable();

    let root = tempfile::tempdir().expect("tempdir");
    let allowed = root.path().join("allowed");
    let forbidden = root.path().join("forbidden");
    std::fs::create_dir_all(&allowed).expect("allowed");
    std::fs::create_dir_all(&forbidden).expect("forbidden");
    let secret = write_secret(&forbidden);
    let batch = root.path().join("guest.bat");
    guest_batch_read_deny(&batch);

    let spec = Spec {
        label: "test:windows-ac-read".into(),
        reads: vec![batch.clone()],
        writes: vec![allowed.clone()],
        net: NetPolicy::Deny,
        allow_exec: true,
        // System32 comes from the platform system set; ACL mutation on that
        // tree is skipped (ACCESS_DENIED), relying on OS ALL APPLICATION PACKAGES.
        system_paths: true,
        enforcement: Enforcement::Required,
        preserve_fds: vec![],
    };

    let output = run_jailed(&spec, &comspec(), &[&batch, &allowed, &secret]);
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
fn appcontainer_guest_cannot_write_outside_allowlist() {
    assert_spawn_capable();

    let root = tempfile::tempdir().expect("tempdir");
    let allowed = root.path().join("allowed");
    let forbidden = root.path().join("forbidden");
    std::fs::create_dir_all(&allowed).expect("allowed");
    std::fs::create_dir_all(&forbidden).expect("forbidden");
    let batch = root.path().join("guest.bat");
    guest_batch_write_deny(&batch);

    let spec = Spec {
        label: "test:windows-ac-write".into(),
        reads: vec![batch.clone()],
        writes: vec![allowed.clone()],
        net: NetPolicy::Deny,
        allow_exec: true,
        system_paths: true,
        enforcement: Enforcement::Required,
        preserve_fds: vec![],
    };

    let output = run_jailed(&spec, &comspec(), &[&batch, &allowed, &forbidden]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "jail/guest failed: status={:?}\nstderr={stderr}\nstdout={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        !forbidden.join("pwned.txt").exists(),
        "guest wrote outside the allowlist"
    );
}

#[test]
fn required_refuses_missing_allowlist_path() {
    assert_spawn_capable();

    let root = tempfile::tempdir().expect("tempdir");
    let missing = root.path().join("nope");
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
        .arg(comspec())
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

#[test]
fn plan_appcontainer_populates_package_sid_on_windows() {
    assert_spawn_capable();
    let plan = bookclerk_sandbox::spawn::plan_appcontainer(
        &bookclerk_sandbox::Policy::new("test:sid").net(NetPolicy::Outbound),
    )
    .expect("plan");
    assert!(
        plan.package_sid.is_some(),
        "Windows plan should ensure a profile SID"
    );
    assert_eq!(plan.capability_names, ["internetClient"]);
    assert!(plan.profile_name.starts_with("bookclerk."));
}

#[test]
fn disabled_enforcement_runs_guest_unconfined() {
    assert_spawn_capable();

    let root = tempfile::tempdir().expect("tempdir");
    let batch = root.path().join("echo.bat");
    std::fs::write(&batch, "@echo off\necho hi\n").expect("batch");
    let spec = Spec {
        label: "test:windows-disabled".into(),
        reads: vec![batch.clone()],
        writes: vec![],
        net: NetPolicy::Deny,
        allow_exec: true,
        system_paths: false,
        enforcement: Enforcement::Disabled,
        preserve_fds: vec![],
    };

    let output = Command::new(JAIL)
        .arg(comspec())
        .arg("/C")
        .arg(&batch)
        .env(
            bookclerk_sandbox::SPEC_ENV,
            serde_json::to_string(&spec).expect("encode"),
        )
        .output()
        .expect("run bookclerk-jail");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "disabled jail should still run guest: {stderr}"
    );
    assert!(
        stderr.contains("disabled") || stderr.contains("unconfined"),
        "expected disabled/unconfined notice, got: {stderr}"
    );
}
