//! Proof that `bookclerk-jail` on Windows launches the guest inside an
//! AppContainer whose filesystem allowlist actually holds.
//!
//! Uses `bookclerk-ac-probe` (TokenIsAppContainer + path probes) rather than
//! cmd.exe log scraping.

#![cfg(windows)]
#![allow(unsafe_code)] // process_alive uses OpenProcess / GetExitCodeProcess

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

    let host_local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let host_temp = std::env::var("TEMP").unwrap_or_default();
    let host_local_marker = PathBuf::from(&host_local).join("bookclerk-host-local-probe.txt");
    let host_temp_marker = PathBuf::from(&host_temp).join("bookclerk-host-temp-probe.txt");
    let _ = std::fs::write(&host_local_marker, b"host-local");
    let _ = std::fs::write(&host_temp_marker, b"host-temp");
    let host_local_s = host_local_marker.display().to_string();
    let host_temp_s = host_temp_marker.display().to_string();

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
            "--temp-roundtrip",
            "--deny-read",
            &host_local_s,
            "--deny-read",
            &host_temp_s,
        ],
    ));
    let _ = std::fs::remove_file(&host_local_marker);
    let _ = std::fs::remove_file(&host_temp_marker);

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
    assert_eq!(
        report["temp_roundtrip_ok"], true,
        "TEMP create/read/delete must work inside the profile"
    );

    let cwd = report["cwd"].as_str().expect("cwd");
    let local = report["localappdata"].as_str().expect("localappdata");
    let temp = report["temp"].as_str().expect("temp");
    let tmp = report["tmp"].as_str().expect("tmp");
    let cwd_l = cwd.to_ascii_lowercase();
    let local_l = local.to_ascii_lowercase();
    let temp_l = temp.to_ascii_lowercase();
    assert!(
        !cwd_l.contains("\\system32"),
        "cwd must not be System32: {cwd}"
    );
    assert!(
        cwd_l.contains("\\packages\\"),
        "cwd must be under LocalAppData\\Packages: {cwd}"
    );
    assert!(
        local_l.contains("\\packages\\"),
        "LOCALAPPDATA must be under Packages (API may return Packages\\SID or ...\\AC): {local}"
    );
    assert!(
        temp_l.contains("\\temp") || temp_l.ends_with("\\temp"),
        "TEMP must be under the profile folder: {temp}"
    );
    assert_eq!(temp, tmp);
    if !host_temp.is_empty() {
        assert_ne!(
            temp_l,
            host_temp.to_ascii_lowercase(),
            "guest TEMP must not be the host user temp"
        );
    }
    if !host_local.is_empty() {
        assert_ne!(
            local_l,
            host_local.to_ascii_lowercase(),
            "guest LOCALAPPDATA must not be the host user LocalAppData"
        );
    }
    let deny = report["deny_reads"].as_array().expect("deny_reads");
    assert_eq!(
        deny[0]["ok"], false,
        "host LOCALAPPDATA marker must be inaccessible"
    );
    assert_eq!(
        deny[1]["ok"], false,
        "host TEMP marker must be inaccessible"
    );
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

    // Signal files live inside each guest's own allowlisted dir so concurrent
    // launches do not fight over SetNamedSecurityInfo on a shared sync tree.
    let ready_a = dir_a.join("ready");
    let ready_b = dir_b.join("ready");
    let release_a = dir_a.join("release");
    let release_b = dir_b.join("release");

    // Same policy label (the historical Package SID collision), different dirs.
    let label = "media-worker:fixup";
    let spec_a = base_spec(label, vec![file_a.clone()], vec![dir_a.clone()]);
    let spec_b = base_spec(label, vec![file_b.clone()], vec![dir_b.clone()]);

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

    let a_file_a = file_a_s.clone();
    let a_file_b = file_b_s.clone();
    let a_dir_a = dir_a_s.clone();
    let a_dir_b = dir_b_s.clone();
    let a_ready = ready_a_s;
    let a_release = release_a_s;
    let handle_a = thread::spawn(move || {
        barrier_a.wait();
        run_jailed_probe(
            &spec_a,
            &[
                "--read",
                &a_file_a,
                "--read",
                &a_file_b,
                "--write",
                &a_dir_a,
                "--write",
                &a_dir_b,
                "--signal",
                &a_ready,
                "--wait-after",
                &a_release,
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
            ],
        )
    });

    // Both probes finish isolation checks and signal before either is released,
    // so both ACL grant sets are active during the cross-access attempts.
    let deadline = std::time::Instant::now() + Duration::from_secs(90);
    while !(ready_a.exists() && ready_b.exists()) {
        if std::time::Instant::now() >= deadline {
            // Prefer surfacing guest stderr over a bare timeout.
            let _ = std::fs::write(&release_a, b"go");
            let _ = std::fs::write(&release_b, b"go");
            let out_a = handle_a.join().expect("thread a");
            let out_b = handle_b.join().expect("thread b");
            panic!(
                "timed out waiting for both guests to become ready\n--- A ---\nstatus={:?}\nstderr={}\nstdout={}\n--- B ---\nstatus={:?}\nstderr={}\nstdout={}",
                out_a.status.code(),
                String::from_utf8_lossy(&out_a.stderr),
                String::from_utf8_lossy(&out_a.stdout),
                out_b.status.code(),
                String::from_utf8_lossy(&out_b.stderr),
                String::from_utf8_lossy(&out_b.stdout),
            );
        }
        thread::sleep(Duration::from_millis(50));
    }

    // Prove cross-isolation while both grants are live, then let A exit first.
    std::fs::write(&release_a, b"go").expect("release a");
    let out_a = handle_a.join().expect("thread a");
    let report_a = assert_probe_ok(&out_a);
    assert_eq!(report_a["is_app_container"], true);
    assert_eq!(report_a["reads"][0]["ok"], true);
    assert_eq!(report_a["reads"][1]["ok"], false);
    assert_eq!(report_a["writes"][0]["ok"], true);
    assert_eq!(report_a["writes"][1]["ok"], false);

    // A has fully exited (ACEs revoked, profile deleted). B must still work.
    std::fs::write(&release_b, b"go").expect("release b");
    let out_b = handle_b.join().expect("thread b");
    let report_b = assert_probe_ok(&out_b);
    assert_eq!(report_b["is_app_container"], true);
    assert_eq!(report_b["reads"][0]["ok"], true);
    assert_eq!(report_b["reads"][1]["ok"], false);
    assert_eq!(report_b["writes"][0]["ok"], true);
    assert_eq!(report_b["writes"][1]["ok"], false);

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

#[test]
fn forced_job_assign_failure_leaves_no_guest() {
    assert_spawn_capable();
    let root = tempfile::tempdir().expect("tempdir");
    let allowed = root.path().join("allowed");
    std::fs::create_dir_all(&allowed).expect("allowed");
    let spec = base_spec("test:job-assign-fail", vec![], vec![allowed]);

    let mut cmd = Command::new(JAIL);
    cmd.arg(PROBE)
        .arg("--hold-ms")
        .arg("30000")
        .env(
            bookclerk_sandbox::SPEC_ENV,
            serde_json::to_string(&spec).expect("encode"),
        )
        .env("BOOKCLERK_TEST_FAIL_JOB_ASSIGN", "1");
    let output = cmd.output().expect("run jail");
    assert!(
        !output.status.success(),
        "forced Assign failure must fail closed"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("AssignProcessToJobObject")
            || stderr.contains("BOOKCLERK_TEST_FAIL_JOB_ASSIGN")
            || stderr.contains("CreateProcess AppContainer failed"),
        "stderr should mention job assign failure: {stderr}"
    );
}

#[test]
fn jail_exits_promptly_when_guest_exits_with_stdin_held_open() {
    assert_spawn_capable();
    use std::io::Write;
    use std::process::{Command, Stdio};

    let root = tempfile::tempdir().expect("tempdir");
    let allowed = root.path().join("allowed");
    std::fs::create_dir_all(&allowed).expect("allowed");
    let mut session =
        bookclerk_sandbox::spawn::AppContainerSession::create("test:stdin-exit").expect("session");
    let sid = session.package_sid().to_string();
    let profile = session.profile_name().to_string();
    let mut spec = base_spec("test:stdin-exit", vec![], vec![allowed.clone()]);
    spec.windows_profile_name = Some(profile.clone());

    let mut child = Command::new(JAIL)
        .arg(PROBE)
        .arg("--exit-immediately")
        .env(
            bookclerk_sandbox::SPEC_ENV,
            serde_json::to_string(&spec).expect("encode"),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn jail");

    // Keep the write end open (do not drop stdin) — this is the plugin-host lifecycle.
    let mut stdin = child.stdin.take().expect("stdin");
    let _ = writeln!(stdin, "still-open");

    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait().expect("try_wait") {
            Some(status) => break status,
            None => {
                assert!(
                    start.elapsed() < Duration::from_secs(45),
                    "jail must exit promptly after guest exit even with stdin held open"
                );
                thread::sleep(Duration::from_millis(50));
            }
        }
    };
    assert!(status.success(), "jail should succeed: {status:?}");
    // Profile owned by this test session should still exist until we drop it;
    // ACL grants from the jail launch should already be revoked.
    assert!(
        !bookclerk_sandbox::spawn::dacl_mentions_sid(&allowed, &sid).expect("dacl"),
        "ACL grants must be cleaned after jail exit"
    );
    drop(stdin);
    session.arm_delete();
    drop(session);
}

#[test]
fn listen_poc_matrix_records_bind_results() {
    assert_spawn_capable();
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::process::Stdio;

    // Phase 0 matrix (real AppContainer spawn):
    // A: internetClient only → Outbound, bind 127.0.0.1:0 + host HTTP
    // B: + internetClientServer → Full, bind 127.0.0.1:0 + host HTTP
    // C: + privateNetworkClientServer → OutboundListen, bind 0.0.0.0:0 + LAN client
    // D: outbound baseline → Outbound HTTPS/TCP:443
    // E: deny / no caps → Deny bind (fail closed expected)
    // F: same as B + CheckNetIsolation LoopbackExempt -is (dev bypass) → expect
    //    host→guest connect to succeed, confirming loopback isolation (not missing
    //    caps) is why A–C fail. Product code never calls CheckNetIsolation.
    //
    // Bind status is published via --listen-status (allowlisted file) so the
    // host can connect while the guest still holds the socket. Do not infer
    // "bind denied" from a missing report — that is a harness/launch failure.
    let scenarios: &[(&str, NetPolicy, &str, bool)] = &[
        ("E-deny", NetPolicy::Deny, "127.0.0.1:0", false),
        (
            "A-outbound-loopback",
            NetPolicy::Outbound,
            "127.0.0.1:0",
            true,
        ),
        ("B-full-loopback", NetPolicy::Full, "127.0.0.1:0", true),
        (
            "C-outbound-listen-lan",
            NetPolicy::OutboundListen,
            "0.0.0.0:0",
            true,
        ),
    ];

    let out_dir = listen_poc_artifact_dir();
    std::fs::create_dir_all(&out_dir).expect("listen-poc artifact dir");
    let mut rows: Vec<Value> = Vec::new();
    let mut missing_reports = Vec::new();

    for (name, net, bind, try_host_http) in scenarios {
        let root = tempfile::tempdir().expect("tempdir");
        let allowed = root.path().join("allowed");
        std::fs::create_dir_all(&allowed).expect("allowed");
        let status_path = allowed.join("listen-status.json");
        let mut spec = base_spec(
            &format!("test:listen-{name}"),
            vec![],
            vec![allowed.clone()],
        );
        spec.net = *net;

        let child = Command::new(JAIL)
            .arg(PROBE)
            .args([
                "--listen",
                bind,
                "--listen-status",
                status_path.to_str().expect("utf-8 status path"),
                "--accept-ms",
                "8000",
                "--hold-ms",
                "0",
            ])
            .env(
                bookclerk_sandbox::SPEC_ENV,
                serde_json::to_string(&spec).expect("encode"),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn listen probe");

        // Prefer the status file (survives stdout buffering / proxy races).
        let listen = poll_listen_status(&status_path, Duration::from_secs(20));
        let bind_ok = listen.as_ref().and_then(|r| r["bind_ok"].as_bool());
        let bound = listen
            .as_ref()
            .and_then(|r| r["bound"].as_str())
            .unwrap_or("")
            .to_string();
        let bind_error = listen
            .as_ref()
            .and_then(|r| r["error"].as_str())
            .unwrap_or("")
            .to_string();

        let mut host_http = "skipped".to_string();
        let mut host_target = String::new();
        if *try_host_http {
            host_http = match bind_ok {
                Some(true) if !bound.is_empty() => {
                    let port = bound.rsplit(':').next().unwrap_or("0");
                    host_target = if bound.starts_with("0.0.0.0:") || bound.starts_with("[::]:") {
                        lan_ipv4()
                            .map(|ip| format!("{ip}:{port}"))
                            .unwrap_or_else(|| format!("127.0.0.1:{port}"))
                    } else {
                        bound.clone()
                    };
                    use std::net::ToSocketAddrs;
                    match host_target
                        .to_socket_addrs()
                        .ok()
                        .and_then(|mut a| a.next())
                    {
                        Some(addr) => {
                            match TcpStream::connect_timeout(&addr, Duration::from_secs(2)) {
                                Ok(mut stream) => {
                                    let _ = stream.write_all(b"GET / HTTP/1.0\r\n\r\n");
                                    let mut resp = [0u8; 64];
                                    let n = stream.read(&mut resp).unwrap_or(0);
                                    format!(
                                        "ok bytes={} body={}",
                                        n,
                                        String::from_utf8_lossy(&resp[..n])
                                    )
                                }
                                Err(err) => format!("connect_failed: {err}"),
                            }
                        }
                        None => format!("resolve_failed: {host_target}"),
                    }
                }
                Some(false) => "bind_failed".into(),
                Some(true) => "bind_ok_missing_bound".into(),
                None => "no_probe_report".into(),
            };
        } else if listen.is_none() {
            host_http = "no_probe_report".into();
        }

        // Wait for natural exit; only kill if the guest wedges past accept.
        let output = wait_or_kill(child, Duration::from_secs(30));
        let stdout_all = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        // Fall back to stdout JSON if the status file was never written.
        let listen = listen.or_else(|| {
            stdout_all
                .lines()
                .find(|l| l.trim().starts_with('{'))
                .and_then(|l| serde_json::from_str::<Value>(l).ok())
                .and_then(|r| r.get("listen").cloned())
        });
        let bind_ok = listen
            .as_ref()
            .and_then(|r| r["bind_ok"].as_bool())
            .or(bind_ok);
        let bound = listen
            .as_ref()
            .and_then(|r| r["bound"].as_str())
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or(bound);
        let bind_error = listen
            .as_ref()
            .and_then(|r| r["error"].as_str())
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or(bind_error);

        if listen.is_none() {
            missing_reports.push((*name).to_string());
        }

        let accept = stdout_all
            .lines()
            .filter(|l| l.trim().starts_with('{'))
            .find_map(|l| {
                let v: Value = serde_json::from_str(l).ok()?;
                if v.get("phase").and_then(|p| p.as_str()) == Some("listen-accept") {
                    Some(v)
                } else {
                    None
                }
            });
        let accepted = accept.as_ref().and_then(|a| a["accepted"].as_bool());
        let accept_error = accept
            .as_ref()
            .and_then(|a| a["accept_error"].as_str())
            .unwrap_or("")
            .to_string();

        let row = serde_json::json!({
            "id": name,
            "net_policy": format!("{net:?}"),
            "bind_addr": bind,
            "jail_exit": output.status.code(),
            "bind_ok": bind_ok,
            "bound": bound,
            "bind_error": bind_error,
            "host_target": host_target,
            "host_http": host_http,
            "accepted": accepted,
            "accept_error": accept_error,
            "probe_stdout": stdout_all,
            "probe_stderr": stderr,
            "report_source": if status_path.exists() { "listen-status" } else { "stdout_or_none" },
        });
        rows.push(row.clone());

        let summary = format!(
            "LISTEN_POC[{name}/{net:?}] bind_ok={bind_ok:?} bound={bound} \
             host_http={} accepted={accepted:?} jail_exit={:?}\n",
            row["host_http"],
            output.status.code()
        );
        eprint!("{summary}");
        append_step_summary(&summary);
    }

    // F: CheckNetIsolation LoopbackExempt -is (dev bypass) while guest listens.
    // Inbound -is must stay running for the duration of the listen window.
    let f_row = run_listen_poc_with_loopback_exempt_is();
    let f_host = f_row["host_http"].as_str().unwrap_or("").to_string();
    let f_accepted = f_row["accepted"].as_bool();
    let f_exempt = f_row["loopback_exempt"].as_str().unwrap_or("").to_string();
    rows.push(f_row);
    let summary = format!(
        "LISTEN_POC[F-loopback-exempt-is] exempt={f_exempt} host_http={f_host} \
         accepted={f_accepted:?}\n"
    );
    eprint!("{summary}");
    append_step_summary(&summary);

    // D: outbound-only baseline (TCP:443) must still work under internetClient.
    let root = tempfile::tempdir().expect("tempdir");
    let allowed = root.path().join("allowed");
    std::fs::create_dir_all(&allowed).expect("allowed");
    let mut spec = base_spec("test:listen-D-outbound", vec![], vec![allowed]);
    spec.net = NetPolicy::Outbound;
    let output = run_jailed_probe(&spec, &["--https-get", "https://example.com"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let https = if output.status.success() {
        let report = first_json_line(&stdout);
        assert!(report["https_get"].is_object(), "https_get report present");
        report["https_get"].clone()
    } else {
        Value::Null
    };
    rows.push(serde_json::json!({
        "id": "D-outbound-https",
        "net_policy": "Outbound",
        "bind_addr": null,
        "jail_exit": output.status.code(),
        "bind_ok": null,
        "bound": null,
        "bind_error": null,
        "host_target": null,
        "host_http": null,
        "accepted": null,
        "accept_error": null,
        "https_get": https,
        "probe_stdout": stdout,
        "probe_stderr": stderr,
    }));
    let summary = format!(
        "LISTEN_POC[D-outbound-https] jail_exit={:?} https_get={https}\n",
        output.status.code()
    );
    eprint!("{summary}");
    append_step_summary(&summary);

    write_listen_poc_artifacts(&out_dir, &rows);

    assert!(
        missing_reports.is_empty(),
        "listen PoC produced no bind report for {missing_reports:?} — \
         that is a harness/launch failure, not evidence that AppContainer \
         denied listen. See listen-poc-results/ artifact."
    );
    assert!(
        output.status.success() && https["ok"].as_bool() == Some(true),
        "outbound baseline D must succeed under internetClient; got exit {:?} https={https}",
        output.status.code()
    );

    // F confirms the A–C diagnosis when the OS allows the exemption tool.
    if f_exempt.starts_with("active:") {
        assert!(
            f_host.starts_with("ok ") && f_accepted == Some(true),
            "with CheckNetIsolation -is active, host→guest must succeed \
             (got host_http={f_host:?} accepted={f_accepted:?}); otherwise \
             A–C failures are not explained by loopback isolation alone"
        );
    } else {
        eprintln!("LISTEN_POC[F] skipped hard assert — CheckNetIsolation unavailable: {f_exempt}");
    }
}

/// Row F: same caps/bind as B, plus a live `CheckNetIsolation LoopbackExempt -is`
/// session for the pre-created profile. Product code must never do this.
fn run_listen_poc_with_loopback_exempt_is() -> Value {
    use std::process::Stdio;

    let root = tempfile::tempdir().expect("tempdir");
    let allowed = root.path().join("allowed");
    std::fs::create_dir_all(&allowed).expect("allowed");
    let status_path = allowed.join("listen-status.json");

    let mut session =
        bookclerk_sandbox::spawn::AppContainerSession::create("test:listen-F-loopback-exempt")
            .expect("create F session");
    let profile = session.profile_name().to_string();
    let package_sid = session.package_sid().to_string();

    let mut exempt = start_loopback_exempt_inbound(&profile);
    // Brief settle so MPSSVC can install the inbound loopback filter.
    thread::sleep(Duration::from_millis(500));

    let mut spec = base_spec(
        "test:listen-F-loopback-exempt",
        vec![],
        vec![allowed.clone()],
    );
    spec.net = NetPolicy::Full;
    spec.windows_profile_name = Some(profile.clone());

    let child = Command::new(JAIL)
        .arg(PROBE)
        .args([
            "--listen",
            "127.0.0.1:0",
            "--listen-status",
            status_path.to_str().expect("utf-8 status path"),
            "--accept-ms",
            "8000",
            "--hold-ms",
            "0",
        ])
        .env(
            bookclerk_sandbox::SPEC_ENV,
            serde_json::to_string(&spec).expect("encode"),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn F listen probe");

    let listen = poll_listen_status(&status_path, Duration::from_secs(20));
    let bind_ok = listen.as_ref().and_then(|r| r["bind_ok"].as_bool());
    let bound = listen
        .as_ref()
        .and_then(|r| r["bound"].as_str())
        .unwrap_or("")
        .to_string();
    let bind_error = listen
        .as_ref()
        .and_then(|r| r["error"].as_str())
        .unwrap_or("")
        .to_string();

    let (host_target, host_http) = match bind_ok {
        Some(true) if !bound.is_empty() => {
            let (target, result) = host_http_get(&bound);
            (target, result)
        }
        Some(false) => (String::new(), "bind_failed".into()),
        Some(true) => (String::new(), "bind_ok_missing_bound".into()),
        None => (String::new(), "no_probe_report".into()),
    };

    let output = wait_or_kill(child, Duration::from_secs(30));
    let stdout_all = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let accept = stdout_all
        .lines()
        .filter(|l| l.trim().starts_with('{'))
        .find_map(|l| {
            let v: Value = serde_json::from_str(l).ok()?;
            if v.get("phase").and_then(|p| p.as_str()) == Some("listen-accept") {
                Some(v)
            } else {
                None
            }
        });
    let accepted = accept.as_ref().and_then(|a| a["accepted"].as_bool());
    let accept_error = accept
        .as_ref()
        .and_then(|a| a["accept_error"].as_str())
        .unwrap_or("")
        .to_string();

    let loopback_exempt = match &mut exempt {
        Ok(child) => {
            // -is must still be alive while we connected; tear down afterward.
            let still = child.try_wait().ok().flatten().is_none();
            let _ = child.kill();
            let _ = child.wait();
            let _ = Command::new("CheckNetIsolation.exe")
                .args(["LoopbackExempt", "-d", "-n", &profile])
                .status();
            if still {
                format!("active:-is:-n={profile}")
            } else {
                format!("exited-early:-is:-n={profile}")
            }
        }
        Err(err) => format!("unavailable: {err}"),
    };

    session.arm_delete();
    drop(session);

    serde_json::json!({
        "id": "F-loopback-exempt-is",
        "net_policy": "Full+LoopbackExempt-is",
        "bind_addr": "127.0.0.1:0",
        "jail_exit": output.status.code(),
        "bind_ok": bind_ok,
        "bound": bound,
        "bind_error": bind_error,
        "host_target": host_target,
        "host_http": host_http,
        "accepted": accepted,
        "accept_error": accept_error,
        "loopback_exempt": loopback_exempt,
        "package_sid": package_sid,
        "profile": profile,
        "probe_stdout": stdout_all,
        "probe_stderr": stderr,
        "report_source": if status_path.exists() { "listen-status" } else { "stdout_or_none" },
    })
}

/// Start `CheckNetIsolation LoopbackExempt -is -n <profile>` and keep it alive.
///
/// Microsoft documents inbound loopback as requiring this process to remain
/// running for the listen window (unlike `-a`, which is a persistent add).
fn start_loopback_exempt_inbound(profile: &str) -> Result<std::process::Child, String> {
    use std::io::Read;
    use std::process::Stdio;
    let mut child = Command::new("CheckNetIsolation.exe")
        .args(["LoopbackExempt", "-is", "-n", profile])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("spawn CheckNetIsolation: {err}"))?;
    thread::sleep(Duration::from_millis(200));
    match child.try_wait() {
        Ok(Some(status)) => {
            let mut stdout = String::new();
            let mut stderr = String::new();
            if let Some(mut out) = child.stdout.take() {
                let _ = out.read_to_string(&mut stdout);
            }
            if let Some(mut err) = child.stderr.take() {
                let _ = err.read_to_string(&mut stderr);
            }
            Err(format!(
                "CheckNetIsolation -is exited immediately ({status}): stdout={stdout:?} stderr={stderr:?}"
            ))
        }
        Ok(None) => Ok(child),
        Err(err) => Err(format!("try_wait CheckNetIsolation: {err}")),
    }
}

fn host_http_get(bound: &str) -> (String, String) {
    use std::io::{Read, Write};
    use std::net::{TcpStream, ToSocketAddrs};

    let port = bound.rsplit(':').next().unwrap_or("0");
    let host_target = if bound.starts_with("0.0.0.0:") || bound.starts_with("[::]:") {
        lan_ipv4()
            .map(|ip| format!("{ip}:{port}"))
            .unwrap_or_else(|| format!("127.0.0.1:{port}"))
    } else {
        bound.to_string()
    };
    let result = match host_target
        .to_socket_addrs()
        .ok()
        .and_then(|mut a| a.next())
    {
        Some(addr) => match TcpStream::connect_timeout(&addr, Duration::from_secs(2)) {
            Ok(mut stream) => {
                let _ = stream.write_all(b"GET / HTTP/1.0\r\n\r\n");
                let mut resp = [0u8; 64];
                let n = stream.read(&mut resp).unwrap_or(0);
                format!(
                    "ok bytes={} body={}",
                    n,
                    String::from_utf8_lossy(&resp[..n])
                )
            }
            Err(err) => format!("connect_failed: {err}"),
        },
        None => format!("resolve_failed: {host_target}"),
    };
    (host_target, result)
}

fn poll_listen_status(path: &Path, timeout: Duration) -> Option<Value> {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if let Ok(body) = std::fs::read_to_string(path) {
            if let Ok(v) = serde_json::from_str::<Value>(body.trim()) {
                if v.get("bind_ok").is_some() {
                    return Some(v);
                }
            }
        }
        thread::sleep(Duration::from_millis(25));
    }
    None
}

fn wait_or_kill(mut child: std::process::Child, timeout: Duration) -> std::process::Output {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child
                    .wait_with_output()
                    .expect("collect listen probe output")
            }
            Ok(None) if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                return child.wait_with_output().expect("collect after kill");
            }
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(_) => {
                let _ = child.kill();
                return child.wait_with_output().expect("collect after wait error");
            }
        }
    }
}

/// Directory for the listen PoC artifact (CI uploads this).
///
/// Override with `BOOKCLERK_LISTEN_POC_DIR`; default `listen-poc-results/` in CWD.
fn listen_poc_artifact_dir() -> PathBuf {
    std::env::var_os("BOOKCLERK_LISTEN_POC_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("listen-poc-results"))
}

fn write_listen_poc_artifacts(out_dir: &Path, rows: &[Value]) {
    use std::io::Write;

    let json_path = out_dir.join("listen-poc.json");
    let md_path = out_dir.join("listen-poc.md");
    let payload = serde_json::json!({
        "title": "AppContainer Phase 0 listen matrix",
        "generated_at_unix": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        "rows": rows,
    });
    std::fs::write(
        &json_path,
        serde_json::to_string_pretty(&payload).expect("encode listen-poc.json"),
    )
    .expect("write listen-poc.json");

    let mut md = String::new();
    md.push_str("# AppContainer Phase 0 listen matrix\n\n");
    md.push_str("| ID | NetPolicy | Bind | bind_ok | bound | host_http | accepted | jail_exit |\n");
    md.push_str("| --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for row in rows {
        let id = row["id"].as_str().unwrap_or("?");
        let net = row["net_policy"].as_str().unwrap_or("?");
        let bind = row["bind_addr"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "—".into());
        let bind_ok = match &row["bind_ok"] {
            Value::Bool(b) => b.to_string(),
            Value::Null if id.starts_with('D') => row["https_get"]["ok"]
                .as_bool()
                .map(|b| format!("tcp443={b}"))
                .unwrap_or_else(|| "n/a".into()),
            Value::Null => "no_report".into(),
            _ => "n/a".into(),
        };
        let bound = row["bound"].as_str().unwrap_or("—");
        let host_http = row["host_http"]
            .as_str()
            .map(|s| s.replace('|', "\\|"))
            .unwrap_or_else(|| {
                if id.starts_with('D') {
                    row["https_get"]
                        .as_object()
                        .map(|o| format!("{o:?}"))
                        .unwrap_or_else(|| "—".into())
                } else {
                    "—".into()
                }
            });
        let accepted = match &row["accepted"] {
            Value::Bool(b) => b.to_string(),
            _ => "—".into(),
        };
        let exit = row["jail_exit"]
            .as_i64()
            .map(|n| n.to_string())
            .unwrap_or_else(|| "?".into());
        md.push_str(&format!(
            "| {id} | {net} | {bind} | {bind_ok} | {bound} | {host_http} | {accepted} | {exit} |\n"
        ));
    }
    md.push_str("\nRaw JSON: `listen-poc.json`\n");
    std::fs::write(&md_path, md).expect("write listen-poc.md");

    // Also mirror into the Job Summary when present.
    if let Ok(summary_path) = std::env::var("GITHUB_STEP_SUMMARY") {
        if let Ok(body) = std::fs::read_to_string(&md_path) {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(summary_path)
                .expect("open GITHUB_STEP_SUMMARY");
            let _ = writeln!(file, "\n{body}");
        }
    }

    eprintln!(
        "LISTEN_POC wrote artifacts: {} and {}",
        json_path.display(),
        md_path.display()
    );
}

fn append_step_summary(text: &str) {
    let Ok(path) = std::env::var("GITHUB_STEP_SUMMARY") else {
        return;
    };
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "```\n{text}```");
    }
}

fn lan_ipv4() -> Option<std::net::Ipv4Addr> {
    use std::net::{SocketAddr, UdpSocket};
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    match socket.local_addr().ok()? {
        SocketAddr::V4(v4) => Some(*v4.ip()),
        SocketAddr::V6(_) => None,
    }
}

#[test]
fn job_kill_on_close_terminates_spawned_descendant() {
    assert_spawn_capable();
    // Guest spawns a long-lived child then exits. Jail drop of the Job
    // (KILL_ON_JOB_CLOSE) must terminate the descendant before ACL/profile cleanup.
    let root = tempfile::tempdir().expect("tempdir");
    let allowed = root.path().join("allowed");
    std::fs::create_dir_all(&allowed).expect("allowed");
    let spec = base_spec("test:job-kill-tree", vec![], vec![allowed]);
    let output = run_jailed_probe(&spec, &["--spawn-child"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "jail/probe failed: status={:?}\nstderr={stderr}\nstdout={stdout}",
        output.status.code()
    );
    let report = first_json_line(&stdout);
    let child_pid = report["child_pid"].as_u64().expect("child_pid") as u32;
    // After jail exit the Job is closed; grandchild must not remain running.
    let start = std::time::Instant::now();
    while process_alive(child_pid) {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "descendant pid {child_pid} still alive after Job close"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

fn process_alive(pid: u32) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let mut code = 0u32;
        let ok = GetExitCodeProcess(handle, &mut code).is_ok();
        let _ = CloseHandle(handle);
        // STILL_ACTIVE == 259
        ok && code == 259
    }
}

#[test]
fn named_acl_mutex_serializes_cross_process_grant_revoke() {
    assert_spawn_capable();
    use std::process::Stdio;

    let root = tempfile::tempdir().expect("tempdir");
    let dir = root.path().join("shared");
    std::fs::create_dir_all(&dir).expect("dir");

    // Capture the original DACL mention state for two fresh SIDs (should be absent).
    let sa = bookclerk_sandbox::spawn::AppContainerSession::create("test:acl-race-a").expect("a");
    let sb = bookclerk_sandbox::spawn::AppContainerSession::create("test:acl-race-b").expect("b");
    let sid_a = sa.package_sid().to_string();
    let sid_b = sb.package_sid().to_string();
    assert!(!bookclerk_sandbox::spawn::dacl_mentions_sid(&dir, &sid_a).unwrap());
    assert!(!bookclerk_sandbox::spawn::dacl_mentions_sid(&dir, &sid_b).unwrap());

    let helper = env!("CARGO_BIN_EXE_bookclerk-acl-race");
    let dir_s = dir.display().to_string();
    let mut children = Vec::new();
    for sid in [&sid_a, &sid_b] {
        let child = Command::new(helper)
            .args(["--dir", &dir_s, "--sid", sid, "--rounds", "8"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn acl-race helper");
        children.push(child);
    }
    for mut child in children {
        let status = child.wait().expect("wait helper");
        assert!(status.success(), "acl-race helper failed: {status:?}");
    }

    // Both SIDs must be absent after helpers finish (each revokes its own grant).
    assert!(
        !bookclerk_sandbox::spawn::dacl_mentions_sid(&dir, &sid_a).unwrap(),
        "sid A must not remain"
    );
    assert!(
        !bookclerk_sandbox::spawn::dacl_mentions_sid(&dir, &sid_b).unwrap(),
        "sid B must not remain"
    );
    drop(sa);
    drop(sb);
}
