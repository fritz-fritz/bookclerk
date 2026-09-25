//! Native-behind-workerd spawn and teardown fail closed.

#[path = "native_gateway/harness.rs"]
mod ng_harness;

use std::ffi::OsString;
use std::time::Instant;

use bookclerk_plugin_host::{
    consent_request, PluginGrantStore, PluginSession, SessionServices, HOST_SHARED_ACCOUNT,
    WORKERD_BIN_ENV,
};
use ng_harness::{
    kill_pid, linux_fd_count, open_session, probe, process_alive, session_dirs_under, step,
    wait_for_exit, Install, Listener, SPAWN_TIMEOUT,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn guest_that_exits_immediately_fails_closed() {
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
    .expect("spawn timed out");
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
        .expect("describe hung after the guest was killed");
    assert!(rpc.is_err(), "RPC must fail after the guest is killed");
    wait_for_exit(gateway).await;
    drop(session);
    step("gateway exited after guest kill");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grant_revision_bump_fences_the_live_session() {
    let listener = Listener::bind(true).await;
    let install = Install::new(listener.port);
    let session = install.spawn().await;
    open_session(&session).await;
    let gateway = session.gateway_pid().expect("gateway");
    let guest = session.guest_pid().expect("guest");

    let plugin = install.plugin();
    let mut grants = PluginGrantStore::load(install.files_dir()).expect("load grants");
    let mut grant = consent_request(&plugin.manifest, plugin.plugin_key());
    grant.extra_processes = Some(1);
    grant.domains.insert("revoked.example".into());
    grants.upsert(grant);
    grants.save(install.files_dir()).expect("save grants");

    let deadline = Instant::now() + SPAWN_TIMEOUT;
    let mut fenced = false;
    while Instant::now() < deadline {
        match tokio::time::timeout(ng_harness::RPC_TIMEOUT, session.describe())
            .await
            .expect("describe hung while waiting for the grant fence")
        {
            Ok(_) => tokio::time::sleep(std::time::Duration::from_millis(50)).await,
            Err(_) => {
                fenced = true;
                break;
            }
        }
    }
    assert!(fenced, "session was not fenced after grant revision bump");
    wait_for_exit(gateway).await;
    wait_for_exit(guest).await;
    drop(session);
    step("grant revision bump shut the session down");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sequential_and_concurrent_spawn_cycles_do_not_leak() {
    let fds_before = linux_fd_count();
    for i in 0..20 {
        let listener = Listener::bind(true).await;
        let install = Install::new(listener.port);
        let session = install.spawn().await;
        open_session(&session).await;
        let payload = format!("cycle-{i}");
        let outcome = probe(&session, "connect", listener.port, &payload).await;
        assert_eq!(outcome["ok"], true, "cycle {i}: {outcome}");
        let gateway = session.gateway_pid().expect("gateway");
        let guest = session.guest_pid().expect("guest");
        let files = install.files_dir().to_path_buf();
        drop(session);
        wait_for_exit(gateway).await;
        wait_for_exit(guest).await;
        drop(install);
        assert!(
            session_dirs_under(&files).is_empty(),
            "cycle {i} leaked session dirs"
        );
        assert!(!process_alive(gateway) && !process_alive(guest));
    }

    let mut joins = Vec::new();
    for i in 0..4 {
        joins.push(tokio::spawn(async move {
            let listener = Listener::bind(true).await;
            let install = Install::new(listener.port);
            let session = install.spawn().await;
            open_session(&session).await;
            let outcome = probe(&session, "connect", listener.port, &format!("par-{i}")).await;
            assert_eq!(outcome["ok"], true, "parallel {i}: {outcome}");
            let gateway = session.gateway_pid().expect("gateway");
            let guest = session.guest_pid().expect("guest");
            let files = install.files_dir().to_path_buf();
            drop(session);
            wait_for_exit(gateway).await;
            wait_for_exit(guest).await;
            drop(install);
            assert!(session_dirs_under(&files).is_empty());
        }));
    }
    for join in joins {
        join.await.expect("parallel cycle");
    }
    if let (Some(before), Some(after)) = (fds_before, linux_fd_count()) {
        assert!(
            after <= before + 8,
            "fd leak: before={before} after={after}"
        );
    }
    step("20 sequential + 4 concurrent cycles left no session dirs or pids");
}
