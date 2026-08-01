//! Proof that `bookclerk-jail` on Windows launches the guest inside an
//! AppContainer whose filesystem allowlist actually holds.
//!
//! Uses `bookclerk-ac-probe` (TokenIsAppContainer + path probes) rather than
//! cmd.exe log scraping.

#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

use bookclerk_sandbox::{Enforcement, NetPolicy, Spec};
use serde_json::Value;

const JAIL: &str = env!("CARGO_BIN_EXE_bookclerk-jail");
const PROBE: &str = env!("CARGO_BIN_EXE_bookclerk-ac-probe");

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

fn base_spec(label: &str, reads: Vec<PathBuf>, writes: Vec<PathBuf>) -> Spec {
    Spec {
        label: label.into(),
        reads,
        writes,
        net: NetPolicy::Deny,
        allow_exec: true,
        system_paths: false,
        enforcement: Enforcement::Required,
        preserve_fds: vec![],
        windows_profile_name: None,
    }
}

fn run_jailed_probe(spec: &Spec, probe_args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(JAIL);
    cmd.arg(PROBE);
    for arg in probe_args {
        cmd.arg(arg);
    }
    cmd.env(
        bookclerk_sandbox::SPEC_ENV,
        serde_json::to_string(spec).expect("encode"),
    )
    .output()
    .expect("run bookclerk-jail + probe")
}

fn first_json_line(stdout: &str) -> Value {
    let line = stdout
        .lines()
        .find(|line| line.trim().starts_with('{'))
        .unwrap_or_else(|| panic!("no JSON line in stdout:\n{stdout}"));
    serde_json::from_str(line).unwrap_or_else(|err| panic!("bad JSON ({err}): {line}"))
}

fn assert_probe_ok(output: &std::process::Output) -> Value {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "jail/probe failed: status={:?}\nstderr={stderr}\nstdout={stdout}",
        output.status.code()
    );
    first_json_line(&stdout)
}

#[test]
fn appcontainer_token_and_path_allowlist_hold() {
    assert_spawn_capable();

    let root = tempfile::tempdir().expect("tempdir");
    let allowed = root.path().join("allowed");
    let forbidden = root.path().join("forbidden");
    std::fs::create_dir_all(&allowed).expect("allowed");
    std::fs::create_dir_all(&forbidden).expect("forbidden");
    let secret = write_secret(&forbidden);
    let readable = allowed.join("readable.txt");
    std::fs::write(&readable, b"hello").expect("readable");

    let spec = base_spec(
        "test:windows-ac-paths",
        vec![readable.clone()],
        vec![allowed.clone()],
    );

    let allowed_s = allowed.display().to_string();
    let readable_s = readable.display().to_string();
    let secret_s = secret.display().to_string();
    let forbidden_s = forbidden.display().to_string();

    let report = assert_probe_ok(&run_jailed_probe(
        &spec,
        &[
            "--read",
            &readable_s,
            "--read",
            &secret_s,
            "--write",
            &allowed_s,
            "--write",
            &forbidden_s,
        ],
    ));

    assert_eq!(report["is_app_container"], true);
    let reads = report["reads"].as_array().expect("reads");
    assert_eq!(reads[0]["ok"], true, "explicit readable file");
    assert_eq!(
        reads[1]["ok"], false,
        "undeclared sibling must not be readable"
    );
    let writes = report["writes"].as_array().expect("writes");
    assert_eq!(writes[0]["ok"], true, "explicit writable dir");
    assert_eq!(
        writes[1]["ok"], false,
        "undeclared sibling must not be writable"
    );

    let cwd = report["cwd"].as_str().expect("cwd");
    let local = report["localappdata"].as_str().expect("localappdata");
    let temp = report["temp"].as_str().expect("temp");
    let tmp = report["tmp"].as_str().expect("tmp");
    assert!(
        !cwd.to_ascii_lowercase().contains("\\system32"),
        "cwd must not be System32: {cwd}"
    );
    assert_eq!(cwd, local, "cwd should be the AppContainer profile folder");
    assert!(
        temp.to_ascii_lowercase().contains("temp"),
        "TEMP should be AppContainer-local: {temp}"
    );
    assert_eq!(temp, tmp);
    if let Ok(host_temp) = std::env::var("TEMP") {
        assert_ne!(
            temp.to_ascii_lowercase(),
            host_temp.to_ascii_lowercase(),
            "guest TEMP must not be the host user temp"
        );
    }
}

#[test]
fn os_managed_write_grant_is_rejected_before_acl_mutation() {
    assert_spawn_capable();

    let system_root = std::env::var("SystemRoot").expect("SystemRoot");
    let target = PathBuf::from(&system_root).join("Temp");
    let spec = base_spec("test:windows-os-write", vec![], vec![target]);

    let output = run_jailed_probe(&spec, &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "OS-managed write grant must fail closed"
    );
    assert!(
        stderr.contains("OS-managed") || stderr.contains("refusing ACL write"),
        "expected OS-managed rejection, got: {stderr}"
    );
}

#[test]
fn temporary_aces_are_cleaned_after_exit() {
    assert_spawn_capable();

    let root = tempfile::tempdir().expect("tempdir");
    let allowed = root.path().join("allowed");
    std::fs::create_dir_all(&allowed).expect("allowed");
    let readable = allowed.join("readable.txt");
    std::fs::write(&readable, b"hello").expect("readable");

    // Host creates the profile so we know the Package SID for DACL checks.
    // Jail attaches (no delete); we arm delete after the probe returns.
    let mut session = bookclerk_sandbox::spawn::AppContainerSession::create("test:dacl-cleanup")
        .expect("session");
    let sid = session.package_sid().to_string();
    let profile = session.profile_name().to_string();
    session.disarm_delete();

    let mut spec = base_spec(
        "test:dacl-cleanup",
        vec![readable.clone()],
        vec![allowed.clone()],
    );
    spec.windows_profile_name = Some(profile);

    let readable_s = readable.display().to_string();
    let allowed_s = allowed.display().to_string();
    let _ = assert_probe_ok(&run_jailed_probe(
        &spec,
        &["--read", &readable_s, "--write", &allowed_s],
    ));

    assert!(
        !bookclerk_sandbox::spawn::dacl_mentions_sid(&allowed, &sid).expect("dacl allowed"),
        "Package SID must not remain on allowed dir DACL after revoke"
    );
    assert!(
        !bookclerk_sandbox::spawn::dacl_mentions_sid(&readable, &sid).expect("dacl file"),
        "Package SID must not remain on readable file DACL after revoke"
    );

    session.arm_delete();
    drop(session);
}

#[test]
fn required_refuses_missing_allowlist_path() {
    assert_spawn_capable();

    let root = tempfile::tempdir().expect("tempdir");
    let missing = root.path().join("nope");
    let spec = base_spec("test:windows-missing", vec![missing], vec![]);

    let output = run_jailed_probe(&spec, &[]);
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
fn plan_appcontainer_is_pure_on_windows() {
    assert_spawn_capable();
    let plan = bookclerk_sandbox::spawn::plan_appcontainer(
        &bookclerk_sandbox::Policy::new("test:sid").net(NetPolicy::Outbound),
    );
    assert_eq!(plan.capability_names, ["internetClient"]);
    assert_eq!(plan.label_stem, "test.sid");

    let session =
        bookclerk_sandbox::spawn::AppContainerSession::create("test:sid").expect("create session");
    assert!(!session.package_sid().is_empty());
    assert!(session.profile_name().len() <= 64);
}

#[test]
fn disabled_enforcement_runs_guest_unconfined() {
    assert_spawn_capable();

    let mut spec = base_spec("test:windows-disabled", vec![], vec![]);
    spec.enforcement = Enforcement::Disabled;

    let output = run_jailed_probe(&spec, &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "disabled jail should still run guest: {stderr}"
    );
    assert!(
        stderr.contains("disabled") || stderr.contains("unconfined"),
        "expected disabled/unconfined notice, got: {stderr}"
    );
    let report = first_json_line(&String::from_utf8_lossy(&output.stdout));
    assert_eq!(report["is_app_container"], false);
}

#[test]
fn overlapping_launches_with_same_label_stay_isolated() {
    assert_spawn_capable();

    let root = tempfile::tempdir().expect("tempdir");
    let dir_a = root.path().join("a");
    let dir_b = root.path().join("b");
    std::fs::create_dir_all(&dir_a).expect("a");
    std::fs::create_dir_all(&dir_b).expect("b");
    let file_a = dir_a.join("a.txt");
    let file_b = dir_b.join("b.txt");
    std::fs::write(&file_a, b"secret-a").expect("a.txt");
    std::fs::write(&file_b, b"secret-b").expect("b.txt");

    let sync = root.path().join("sync");
    std::fs::create_dir_all(&sync).expect("sync");
    let ready_a = sync.join("ready-a");
    let ready_b = sync.join("ready-b");
    let release_a = sync.join("release-a");
    let release_b = sync.join("release-b");
    let _ = std::fs::remove_file(&ready_a);
    let _ = std::fs::remove_file(&ready_b);
    let _ = std::fs::remove_file(&release_a);
    let _ = std::fs::remove_file(&release_b);

    // Same policy label (the historical Package SID collision), different dirs.
    // `sync` is a shared harness directory so guests can signal readiness; the
    // isolation assertions below still cover dir_a ↔ dir_b.
    let label = "media-worker:fixup";
    let spec_a = base_spec(
        label,
        vec![file_a.clone()],
        vec![dir_a.clone(), sync.clone()],
    );
    let spec_b = base_spec(
        label,
        vec![file_b.clone()],
        vec![dir_b.clone(), sync.clone()],
    );

    let file_a_s = file_a.display().to_string();
    let file_b_s = file_b.display().to_string();
    let dir_a_s = dir_a.display().to_string();
    let dir_b_s = dir_b.display().to_string();
    let ready_a_s = ready_a.display().to_string();
    let ready_b_s = ready_b.display().to_string();
    let release_a_s = release_a.display().to_string();
    let release_b_s = release_b.display().to_string();

    let barrier = Arc::new(Barrier::new(2));
    let barrier_a = barrier.clone();
    let barrier_b = barrier.clone();

    let handle_a = thread::spawn(move || {
        barrier_a.wait();
        run_jailed_probe(
            &spec_a,
            &[
                "--read",
                &file_a_s,
                "--read",
                &file_b_s,
                "--write",
                &dir_a_s,
                "--write",
                &dir_b_s,
                "--signal",
                &ready_a_s,
                "--wait-after",
                &release_a_s,
            ],
        )
    });
    let handle_b = thread::spawn(move || {
        barrier_b.wait();
        run_jailed_probe(
            &spec_b,
            &[
                "--read",
                &file_b_s,
                "--read",
                &file_a_s,
                "--write",
                &dir_b_s,
                "--write",
                &dir_a_s,
                "--signal",
                &ready_b_s,
                "--wait-after",
                &release_b_s,
                "--wait-gone",
                &ready_a_s,
            ],
        )
    });

    // Both probes finish isolation checks and signal before either is released,
    // so both ACL grant sets are active during the cross-access attempts.
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while !(ready_a.exists() && ready_b.exists()) {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for both guests to become ready"
        );
        thread::sleep(Duration::from_millis(50));
    }

    // Let A exit first; B waits for ready-a to vanish then re-checks its write grant.
    std::fs::write(&release_a, b"go").expect("release a");
    let out_a = handle_a.join().expect("thread a");
    let report_a = assert_probe_ok(&out_a);
    assert_eq!(report_a["is_app_container"], true);
    assert_eq!(report_a["reads"][0]["ok"], true);
    assert_eq!(report_a["reads"][1]["ok"], false);
    assert_eq!(report_a["writes"][0]["ok"], true);
    assert_eq!(report_a["writes"][1]["ok"], false);

    // Remove A's ready marker so B's wait-gone completes, then release B.
    let _ = std::fs::remove_file(&ready_a);
    std::fs::write(&release_b, b"go").expect("release b");
    let out_b = handle_b.join().expect("thread b");
    let stdout_b = String::from_utf8_lossy(&out_b.stdout);
    let report_b = assert_probe_ok(&out_b);
    assert_eq!(report_b["is_app_container"], true);
    assert_eq!(report_b["reads"][0]["ok"], true);
    assert_eq!(report_b["reads"][1]["ok"], false);
    assert_eq!(report_b["writes"][0]["ok"], true);
    assert_eq!(report_b["writes"][1]["ok"], false);
    assert!(
        stdout_b.contains("after-peer-exit"),
        "guest B must keep grants after A exits:\n{stdout_b}"
    );

    // Host can still use the fixture dirs (no sticky broken DACLs).
    std::fs::write(dir_a.join("host.txt"), b"ok").expect("host write a");
    std::fs::write(dir_b.join("host.txt"), b"ok").expect("host write b");
}

#[test]
fn long_label_monikers_do_not_collide() {
    assert_spawn_capable();
    let shared = "z".repeat(80);
    let a = bookclerk_sandbox::spawn::unique_profile_moniker(&format!("{shared}-alpha"));
    let b = bookclerk_sandbox::spawn::unique_profile_moniker(&format!("{shared}-beta"));
    assert_ne!(a, b);
    assert!(a.len() <= 64);
    assert!(b.len() <= 64);

    let sa = bookclerk_sandbox::spawn::AppContainerSession::create(&format!("{shared}-alpha"))
        .expect("session a");
    let sb = bookclerk_sandbox::spawn::AppContainerSession::create(&format!("{shared}-beta"))
        .expect("session b");
    assert_ne!(sa.package_sid(), sb.package_sid());
    assert_ne!(sa.profile_name(), sb.profile_name());
}
