//! Native-behind-workerd spawn and teardown fail closed.

#[path = "native_gateway/harness.rs"]
mod ng_harness;

use std::ffi::OsString;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use bookclerk_plugin_host::{
    consent_request, reconcile_grants_from_disk, PluginGrantStore, PluginSession, SessionServices,
    HOST_SHARED_ACCOUNT, WORKERD_BIN_ENV,
};
use bookclerk_plugin_sdk::CliInvokeParams;
use ng_harness::{
    kill_pid, linux_fd_count, linux_session_cgroup, open_session, probe, process_alive,
    session_dirs_under, step, wait_for_exit, Install, Listener, ProcessTree, SETTLE_TIMEOUT,
    SPAWN_TIMEOUT,
};

/// Serializes tests in this binary. `missing_workerd_fails_closed` replaces
/// process `BOOKCLERK_WORKERD_BIN`, which every other spawn reads.
async fn workerd_bin_lock() -> tokio::sync::OwnedMutexGuard<()> {
    use std::sync::{Arc, OnceLock};
    static LOCK: OnceLock<Arc<tokio::sync::Mutex<()>>> = OnceLock::new();
    LOCK.get_or_init(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
        .lock_owned()
        .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn guest_that_exits_immediately_fails_closed() {
    let _env = workerd_bin_lock().await;
    let listener = Listener::bind(true).await;
    let install = Install::new(listener.port);
    let extra = [("BOOKCLERK_PROBE_EXIT", OsString::from("1"))];
    let plugin = install.plugin();
    let result = tokio::time::timeout(
        SPAWN_TIMEOUT,
        PluginSession::spawn_with(
            &plugin,
            &install.config,
            serde_json::json!({}),
            HOST_SHARED_ACCOUNT,
            &extra,
            SessionServices::default(),
        ),
    )
    .await
    .unwrap_or_else(|_| {
        ng_harness::fail_deadline(&format!("spawn timed out after {SPAWN_TIMEOUT:?}"))
    });
    let err = match result {
        Ok(_) => panic!("immediate-exit guest must fail closed"),
        Err(err) => err,
    };
    step(&format!("immediate-exit refused: {err}"));
    assert!(
        session_dirs_under(install.files_dir()).is_empty(),
        "session dir leaked after failed spawn"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_workerd_fails_closed() {
    let _env = workerd_bin_lock().await;
    let listener = Listener::bind(true).await;
    let install = Install::new(listener.port);
    let missing = install.files_dir().join("no-such-workerd");
    let previous = std::env::var_os(WORKERD_BIN_ENV);
    std::env::set_var(WORKERD_BIN_ENV, &missing);
    let plugin = install.plugin();
    let result = tokio::time::timeout(
        SPAWN_TIMEOUT,
        PluginSession::spawn_with(
            &plugin,
            &install.config,
            serde_json::json!({}),
            HOST_SHARED_ACCOUNT,
            &[],
            SessionServices::default(),
        ),
    )
    .await;
    match previous {
        Some(value) => std::env::set_var(WORKERD_BIN_ENV, value),
        None => std::env::remove_var(WORKERD_BIN_ENV),
    }
    let err = match result {
        Ok(Ok(_)) => panic!("spawn succeeded without a workerd binary"),
        Ok(Err(err)) => err,
        Err(_) => panic!("spawn timed out after {SPAWN_TIMEOUT:?}"),
    };
    step(&format!("missing workerd refused: {err}"));
    assert!(
        err.to_string().contains("workerd") || err.to_string().contains("front door"),
        "error should name the missing front door: {err}"
    );
    assert!(session_dirs_under(install.files_dir()).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn killing_gateway_exits_the_guest() {
    let _env = workerd_bin_lock().await;
    let listener = Listener::bind(true).await;
    let install = Install::new(listener.port);
    let session = install.spawn().await;
    open_session(&session).await;
    let gateway = session.gateway_pid().expect("gateway");
    let guest = session.guest_pid().expect("guest");
    kill_pid(gateway);
    wait_for_exit(gateway).await;
    wait_for_exit(guest).await;
    drop(session);
    step("guest exited after gateway kill");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn killing_guest_errors_rpc_and_exits_gateway() {
    let _env = workerd_bin_lock().await;
    let listener = Listener::bind(true).await;
    let install = Install::new(listener.port);
    let session = install.spawn().await;
    open_session(&session).await;
    let gateway = session.gateway_pid().expect("gateway");
    let guest = session.guest_pid().expect("guest");
    kill_pid(guest);
    wait_for_exit(guest).await;
    let rpc = tokio::time::timeout(ng_harness::RPC_TIMEOUT, session.describe())
        .await
        .unwrap_or_else(|_| ng_harness::fail_deadline("describe hung after the guest was killed"));
    assert!(rpc.is_err(), "RPC must fail after the guest is killed");
    wait_for_exit(gateway).await;
    drop(session);
    step("gateway exited after guest kill");
}

fn revoke_grant(install: &Install) {
    let plugin = install.plugin();
    let mut grants = PluginGrantStore::load(install.files_dir()).expect("load grants");
    let mut grant = consent_request(&plugin.manifest, plugin.plugin_key());
    grant.extra_processes = Some(1);
    grant.domains.insert("revoked.example".into());
    grants.upsert(grant);
    grants.save(install.files_dir()).expect("save grants");
    reconcile_grants_from_disk(install.files_dir());
}

struct ClearDialDelay;
impl Drop for ClearDialDelay {
    fn drop(&mut self) {
        std::env::remove_var("BOOKCLERK_TEST_PROXY_DIAL_DELAY_MS");
    }
}

/// Echo listener that counts accepts and clean EOFs.
struct EofListener {
    port: u16,
    accepts: Arc<AtomicUsize>,
    eofs: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl EofListener {
    async fn bind() -> Self {
        let listener = tokio::net::TcpListener::bind((ng_harness::LOOPBACK, 0))
            .await
            .expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let accepts = Arc::new(AtomicUsize::new(0));
        let eofs = Arc::new(AtomicUsize::new(0));
        let accepts_task = Arc::clone(&accepts);
        let eofs_task = Arc::clone(&eofs);
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                accepts_task.fetch_add(1, Ordering::SeqCst);
                let eofs_task = Arc::clone(&eofs_task);
                tokio::spawn(async move {
                    let mut buf = [0_u8; 64];
                    loop {
                        match tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await {
                            Ok(0) | Err(_) => {
                                eofs_task.fetch_add(1, Ordering::SeqCst);
                                break;
                            }
                            Ok(n) => {
                                if tokio::io::AsyncWriteExt::write_all(&mut stream, &buf[..n])
                                    .await
                                    .is_err()
                                {
                                    eofs_task.fetch_add(1, Ordering::SeqCst);
                                    break;
                                }
                            }
                        }
                    }
                });
            }
        });
        Self {
            port,
            accepts,
            eofs,
            task,
        }
    }
}

impl Drop for EofListener {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grant_revision_bump_fences_the_live_session() {
    let _env = workerd_bin_lock().await;
    let listener = EofListener::bind().await;
    let install = Install::new(listener.port);
    let session = install.spawn().await;
    open_session(&session).await;
    let gateway = session.gateway_pid().expect("gateway");
    let guest = session.guest_pid().expect("guest");
    let files = install.files_dir().to_path_buf();

    let params = CliInvokeParams {
        command: "probe".into(),
        args: vec![
            bookclerk_plugin_sdk::CliArg {
                name: "op".into(),
                value: "hold".into(),
            },
            bookclerk_plugin_sdk::CliArg {
                name: "host".into(),
                value: ng_harness::LOOPBACK.into(),
            },
            bookclerk_plugin_sdk::CliArg {
                name: "port".into(),
                value: listener.port.to_string(),
            },
            bookclerk_plugin_sdk::CliArg {
                name: "payload".into(),
                value: "hold".into(),
            },
        ],
    };
    let hung = tokio::spawn({
        let session_call = async move { session.cli_invoke(params).await };
        session_call
    });
    let ready = Instant::now() + std::time::Duration::from_secs(15);
    while listener.accepts.load(Ordering::SeqCst) < 1 {
        assert!(Instant::now() < ready, "held TCP stream was not accepted");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    // `session` was moved into the hung task. Revoke fences that RPC directly.
    // The install (and its parent) stay alive for the directory assertion.
    revoke_grant(&install);
    let rpc = tokio::time::timeout(ng_harness::RPC_TIMEOUT, hung)
        .await
        .unwrap_or_else(|_| ng_harness::fail_deadline("held RPC did not return after revoke"))
        .expect("rpc task");
    assert!(
        rpc.is_err(),
        "hung RPC must fail when the grant changes: {rpc:?}"
    );
    let eof_deadline = Instant::now() + ng_harness::SETTLE_TIMEOUT;
    while listener.eofs.load(Ordering::SeqCst) < 1 {
        assert!(
            Instant::now() < eof_deadline,
            "proxied stream stayed open after revoke"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(
        listener.accepts.load(Ordering::SeqCst),
        1,
        "proxy accepted another connection after revoke"
    );
    wait_for_exit(gateway).await;
    wait_for_exit(guest).await;
    assert!(
        session_dirs_under(&files).is_empty(),
        "session dir leaked under {}",
        files.display()
    );
    step("grant revision bump closed the stream and the session");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn revoke_during_dial_does_not_open_the_upstream() {
    let _env = workerd_bin_lock().await;
    let _delay = ClearDialDelay;
    std::env::set_var("BOOKCLERK_TEST_PROXY_DIAL_DELAY_MS", "2000");
    let listener = EofListener::bind().await;
    let install = Install::new(listener.port);
    let session = install.spawn().await;
    open_session(&session).await;
    let gateway = session.gateway_pid().expect("gateway");
    let guest = session.guest_pid().expect("guest");
    let files = install.files_dir().to_path_buf();
    let port = listener.port;
    let hung = tokio::spawn(async move {
        session
            .cli_invoke(CliInvokeParams {
                command: "probe".into(),
                args: vec![
                    bookclerk_plugin_sdk::CliArg {
                        name: "op".into(),
                        value: "connect".into(),
                    },
                    bookclerk_plugin_sdk::CliArg {
                        name: "host".into(),
                        value: ng_harness::LOOPBACK.into(),
                    },
                    bookclerk_plugin_sdk::CliArg {
                        name: "port".into(),
                        value: port.to_string(),
                    },
                    bookclerk_plugin_sdk::CliArg {
                        name: "payload".into(),
                        value: "dial".into(),
                    },
                ],
            })
            .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    revoke_grant(&install);
    let rpc = tokio::time::timeout(ng_harness::RPC_TIMEOUT, hung)
        .await
        .unwrap_or_else(|_| ng_harness::fail_deadline("dial RPC did not return after revoke"))
        .expect("rpc task");
    assert!(rpc.is_err(), "dial RPC must fail when revoked: {rpc:?}");
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(
        listener.accepts.load(Ordering::SeqCst),
        0,
        "dial was committed after the session was revoked"
    );
    wait_for_exit(gateway).await;
    wait_for_exit(guest).await;
    assert!(session_dirs_under(&files).is_empty());
    step("revoke during dial closed the session without an upstream accept");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn revoke_during_initial_describe_fails_the_spawn() {
    let _env = workerd_bin_lock().await;
    let listener = EofListener::bind().await;
    let install = Install::new(listener.port);
    let plugin = install.plugin();
    let config = install.config.clone();
    let files = install.files_dir().to_path_buf();
    let spawned = tokio::spawn(async move {
        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            PluginSession::spawn_with(
                &plugin,
                &config,
                serde_json::json!({}),
                HOST_SHARED_ACCOUNT,
                &[("BOOKCLERK_PROBE_DESCRIBE_DELAY_MS", OsString::from("8000"))],
                SessionServices::default(),
            ),
        )
        .await
    });
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    revoke_grant(&install);
    let result = spawned.await.expect("spawn task");
    let result = result.unwrap_or_else(|_| ng_harness::fail_deadline("startup revoke hung"));
    assert!(
        result.is_err(),
        "spawn must fail when authority changes during describe"
    );
    assert!(
        session_dirs_under(&files).is_empty(),
        "startup failure left a session dir"
    );
    assert_eq!(listener.accepts.load(Ordering::SeqCst), 0);
    step("revoke during describe failed the spawn and removed the session dir");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sequential_and_concurrent_spawn_cycles_do_not_leak() {
    let _env = workerd_bin_lock().await;
    let fds_before = linux_fd_count();
    let listener = Listener::bind(true).await;
    let install = Install::new(listener.port);
    assert!(
        install.files_dir().is_dir(),
        "installation parent missing before churn"
    );
    for i in 0..20 {
        finish_clean_session(&install, &listener, &format!("cycle-{i}")).await;
    }
    tokio::join!(
        finish_clean_session(&install, &listener, "par-0"),
        finish_clean_session(&install, &listener, "par-1"),
        finish_clean_session(&install, &listener, "par-2"),
        finish_clean_session(&install, &listener, "par-3"),
    );
    assert!(
        install.files_dir().is_dir(),
        "installation parent was removed before the session-dir check"
    );
    assert!(
        session_dirs_under(install.files_dir()).is_empty(),
        "churn left session dirs under {}",
        install.files_dir().display()
    );
    if let (Some(before), Some(after)) = (fds_before, linux_fd_count()) {
        assert!(
            after <= before + 8,
            "fd leak: before={before} after={after}"
        );
    }
    step("20 sequential + 4 concurrent cycles left no session dirs or pids");
    drop(install);
}

/// One spawn on a shared installation. The install stays alive so an empty
/// session-dir listing means production cleanup removed the directory.
async fn finish_clean_session(install: &Install, listener: &Listener, label: &str) {
    let session = install.spawn().await;
    open_session(&session).await;
    let outcome = probe(&session, "connect", listener.port, label).await;
    assert_eq!(outcome["ok"], true, "{label}: {outcome}");
    let gateway = session.gateway_pid().expect("gateway");
    let guest = session.guest_pid().expect("guest");
    let tree = ProcessTree::capture();
    let workerd = tree.pinned_workerd(gateway).unwrap_or_else(|| {
        panic!(
            "{label}: pinned workerd missing under gateway {gateway}\n{}",
            tree.describe(gateway)
        )
    });
    let descendant = guest_descendant(&session, guest, label).await;
    assert!(process_alive(workerd), "{label}: pinned workerd {workerd}");
    assert!(
        process_alive(descendant),
        "{label}: guest descendant {descendant}"
    );
    assert_ne!(workerd, gateway, "{label}: workerd pid is the supervisor");
    assert_ne!(
        descendant, guest,
        "{label}: guest descendant pid is the supervisor"
    );
    let cgroup = linux_session_cgroup(guest).or_else(|| linux_session_cgroup(gateway));
    #[cfg(windows)]
    let sid = session.package_sid().map(str::to_string);
    drop(session);
    wait_for_exit(gateway).await;
    wait_for_exit(guest).await;
    wait_for_exit(workerd).await;
    wait_for_exit(descendant).await;
    assert!(
        install.files_dir().is_dir(),
        "{label}: installation parent disappeared"
    );
    let dirs = session_dirs_under(install.files_dir());
    assert!(
        dirs.is_empty(),
        "{label} leaked session dirs under {}: {dirs:?}",
        install.files_dir().display()
    );
    match cgroup {
        Some(dir) => {
            let deadline = Instant::now() + SETTLE_TIMEOUT;
            while dir.exists() && Instant::now() < deadline {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            let procs = std::fs::read_to_string(dir.join("cgroup.procs")).unwrap_or_default();
            assert!(
                !dir.exists(),
                "{label}: cgroup {} remains with members:\n{procs}",
                dir.display()
            );
        }
        None => note_missing_cgroup(),
    }
    #[cfg(windows)]
    if let Some(sid) = sid.as_deref() {
        let plugin = install
            .files_dir()
            .join("plugins")
            .join(ng_harness::PLUGIN_ID);
        for path in [install.files_dir(), plugin.as_path()] {
            let mentioned = bookclerk_sandbox::dacl_mentions_sid(path, sid)
                .unwrap_or_else(|err| panic!("{label}: DACL read {}: {err}", path.display()));
            assert!(
                !mentioned,
                "{label}: package SID {sid} remains on {}",
                path.display()
            );
        }
    }
}

/// Child of the guest supervisor. Unix `exec` replaces the jail, so the probe
/// forks a `pause` sleeper. Windows keeps `bookclerk-jail` and the probe is
/// already that child.
async fn guest_descendant(session: &PluginSession, guest: u32, label: &str) -> u32 {
    let tree = ProcessTree::capture();
    if let Some(pid) = tree.live_descendant(guest) {
        return pid;
    }
    let outcome = probe(session, "descendant", 0, "").await;
    assert_eq!(outcome["ok"], true, "{label}: descendant probe {outcome}");
    let pid = u32::try_from(outcome["pid"].as_u64().expect("descendant pid"))
        .expect("descendant pid fits u32");
    let tree = ProcessTree::capture();
    assert!(
        tree.descendants(guest).contains(&pid),
        "{label}: sleeper {pid} is not under guest {guest}\n{}",
        tree.describe(guest)
    );
    pid
}

fn note_missing_cgroup() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static ONCE: AtomicBool = AtomicBool::new(false);
    if ONCE.swap(true, Ordering::SeqCst) {
        return;
    }
    eprintln!(
        "native_gateway: delegated cgroup unavailable; process-group kill is the fallback \
         and does not cover a descendant that calls setsid"
    );
}

/// Required isolation must fail before either sibling starts when the outer
/// session Job cannot be created or configured.
#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn outer_session_job_failure_leaves_no_session_and_no_proxy() {
    let _env = workerd_bin_lock().await;
    let listener = Listener::bind(false).await;
    let install = Install::new(listener.port);
    struct ClearJobFail;
    impl Drop for ClearJobFail {
        fn drop(&mut self) {
            std::env::remove_var("BOOKCLERK_TEST_FAIL_SESSION_JOB");
        }
    }
    for mode in ["create", "configure"] {
        let _clear = ClearJobFail;
        std::env::set_var("BOOKCLERK_TEST_FAIL_SESSION_JOB", mode);
        let plugin = install.plugin();
        let err = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            PluginSession::spawn_with(
                &plugin,
                &install.config,
                serde_json::json!({}),
                HOST_SHARED_ACCOUNT,
                &[],
                SessionServices::default(),
            ),
        )
        .await
        .unwrap_or_else(|_| panic!("{mode}: spawn hung"))
        .expect_err("required isolation must fail closed");
        std::env::remove_var("BOOKCLERK_TEST_FAIL_SESSION_JOB");
        let text = err.to_string();
        assert!(text.contains("outer session Job"), "{mode}: {text}");
        assert!(text.contains(mode), "{mode}: {text}");
        let dirs = session_dirs_under(install.files_dir());
        assert!(dirs.is_empty(), "{mode} left session dirs {dirs:?}");
        assert_eq!(
            listener.accepts(),
            0,
            "{mode} accepted a proxied connection"
        );
    }
}
