//! Shared jailed-child spawn for Cap'n Proto `api_version = 3` stdio guests.

#![allow(clippy::missing_docs_in_private_items)]
#![cfg_attr(unix, allow(unsafe_code))] // `Command::pre_exec` + `inherit_fd_at` (dup2).

#[cfg(test)]
use std::collections::BTreeSet;
use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use bookclerk_config::Config;
#[cfg(unix)]
use bookclerk_sandbox::DuplexLink;
#[cfg(any(windows, test))]
use bookclerk_sandbox::GATEWAY_GUEST_RPC_WRITE_ENV;
#[cfg(test)]
use bookclerk_sandbox::GATEWAY_PROXY_ENV;
#[cfg(windows)]
use bookclerk_sandbox::{DuplexHalf, StdioEnds};
#[cfg(windows)]
use bookclerk_sandbox::{JailHandoff, JailHandoffExtra, JAIL_HANDOFF_ENV};
use bookclerk_sandbox::{GATEWAY_GUEST_RPC_ENV, SOCKET_PROXY_ENV, WORKERD_STATE_DIR_ENV};
#[cfg(unix)]
use bookclerk_sandbox::{GATEWAY_RPC_FD, GUEST_PROXY_FD};
use serde_json::Value;
#[cfg(windows)]
use tokio::io::AsyncWriteExt;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};

use crate::consent::{inject_workerd_grant_env, spawn_config_for_grant, spawn_grant, PluginGrant};
use crate::discover::DiscoveredPlugin;
use crate::jail::{GuestJail, Start};
use crate::spawn_plan::{SpawnPlan, WORKERD_BIN_ENV};
use crate::{PluginError, Result};

/// Jailed plugin child with stdio pipes (describe not yet called).
pub(crate) struct SpawnedStdio {
    /// Provenance-qualified PluginKey (canonical text).
    pub id: String,
    /// Manifest display alias (`plugin.toml` `id`).
    pub alias: String,
    /// Child the host speaks Cap'n Proto to (gateway / isolate / direct).
    pub child: Child,
    /// Native sibling when this session is native-behind-workerd.
    pub guest: Option<Child>,
    /// Guest stdin (host writes RPC / capnp) — the gateway / primary child.
    pub stdin: ChildStdin,
    /// Guest stdout (host reads RPC / capnp).
    pub stdout: ChildStdout,
    /// Covering **effective** grant (persisted ∩ overlays).
    pub grant: PluginGrant,
    /// Persisted operator grant before host overlays.
    pub persisted_grant: PluginGrant,
    /// Spawn config JSON or destination context extras.
    pub spawn_config: Value,
    /// Guest HOME / data directory.
    pub data: PathBuf,
    /// Guest TMPDIR / scratch directory.
    pub scratch: PathBuf,
    /// Host-owned gateway session directory (removed when the vat drops).
    pub session_dir: Option<PathBuf>,
    /// Native-behind gateway pid (the Cap'n Proto child).
    pub gateway_pid: Option<u32>,
    /// Native guest pid, or the single child when there is no sibling.
    pub guest_pid: Option<u32>,
    /// AppContainer package SID of the native guest (callback proxy).
    #[cfg(windows)]
    pub package_sid: Option<String>,
    /// Host-owned AppContainer profile for the Cap'n Proto child.
    #[cfg(windows)]
    #[allow(dead_code)]
    pub appcontainer: Option<bookclerk_sandbox::spawn::AppContainerSession>,
    /// Host-owned AppContainer profile for the native sibling.
    #[cfg(windows)]
    #[allow(dead_code)]
    pub guest_appcontainer: Option<bookclerk_sandbox::spawn::AppContainerSession>,
    /// Windows session Job (`KILL_ON_JOB_CLOSE`) covering both jails.
    #[cfg(windows)]
    #[allow(dead_code)]
    pub session_job: Option<bookclerk_sandbox::SessionJob>,
    /// Best-effort spawn continued with no outer Job because that kernel
    /// feature is unsupported. Required isolation fails before this is set.
    #[cfg(windows)]
    pub outer_job_unsupported: bool,
    /// Last lines of guest + gateway stderr, for spawn failures.
    pub stderr_tail: Arc<Mutex<VecDeque<String>>>,
    /// Files dir used to re-read `plugin-grants.json` before returning a session.
    pub files_dir: PathBuf,
}

/// Removes `session_dir` unless [`Self::disarm`] is called after a successful spawn.
struct SessionDirGuard(Option<PathBuf>);

impl SessionDirGuard {
    fn disarm(&mut self) -> Option<PathBuf> {
        self.0.take()
    }
}

impl Drop for SessionDirGuard {
    fn drop(&mut self) {
        if let Some(dir) = self.0.take() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// Spawns the jailed guest with piped stdio. Caller performs Cap'n Proto connect.
///
/// `plan` names the program the jail execs (`bookclerk-workerd` on the product
/// path) and, for a native backend, the sibling the host starts beside it.
///
/// # Errors
///
/// Fails when no covering grant exists, the jail cannot be applied, or the
/// process cannot be started.
pub(crate) async fn spawn_stdio_guest(
    plugin: &DiscoveredPlugin,
    plan: &SpawnPlan,
    config: &Config,
    config_table: Value,
    extra_env: &[(&str, OsString)],
) -> Result<SpawnedStdio> {
    let id = plugin.plugin_key().canonical().to_string();
    let alias = plugin.manifest.id.clone();
    let persisted_grant = spawn_grant(&config.paths().files_dir, plugin)?;
    let grant = effective_spawn_grant(&persisted_grant, plugin, config);
    let spawn_config = spawn_config_for_grant(&grant, config_table);
    let jail = GuestJail::plan(config, plugin, plan)?;
    let mut session_guard = SessionDirGuard(jail.session_dir.clone());
    let stderr_tail = Arc::new(Mutex::new(VecDeque::new()));

    let spawned = if jail.guest_start.is_some() {
        spawn_siblings(
            plugin,
            plan,
            &jail,
            &grant,
            extra_env,
            &id,
            Arc::clone(&stderr_tail),
        )
        .await
    } else {
        spawn_single(
            plugin,
            plan,
            &jail,
            &grant,
            extra_env,
            &id,
            Arc::clone(&stderr_tail),
        )
        .await
    };

    let (child, guest, stdin, stdout, gateway_pid, guest_pid, session_job) = match spawned {
        Ok(parts) => parts,
        Err(err) => {
            return Err(err);
        }
    };
    let session_dir = session_guard.disarm();
    #[cfg(not(windows))]
    let _ = session_job;

    Ok(SpawnedStdio {
        id,
        alias,
        child,
        guest,
        stdin,
        stdout,
        grant,
        persisted_grant,
        spawn_config,
        data: jail.data,
        scratch: jail.scratch,
        session_dir,
        gateway_pid,
        guest_pid,
        #[cfg(windows)]
        package_sid: jail.package_sid,
        #[cfg(windows)]
        appcontainer: jail.appcontainer,
        #[cfg(windows)]
        guest_appcontainer: jail.guest_appcontainer,
        #[cfg(windows)]
        session_job,
        #[cfg(windows)]
        outer_job_unsupported: jail.guest_start.is_some() && session_job.is_none(),
        stderr_tail,
        files_dir: config.paths().files_dir.clone(),
    })
}

/// Single-child spawn (workerd isolate or diagnostic direct native).
#[allow(clippy::too_many_arguments, unused_variables)]
async fn spawn_single(
    plugin: &DiscoveredPlugin,
    plan: &SpawnPlan,
    jail: &GuestJail,
    grant: &PluginGrant,
    extra_env: &[(&str, OsString)],
    id: &str,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
) -> Result<(
    Child,
    Option<Child>,
    ChildStdin,
    ChildStdout,
    Option<u32>,
    Option<u32>,
    Option<WindowsSessionJob>,
)> {
    tracing::debug!(
        plugin = %id,
        program = %plan.launcher.display(),
        runtime = plan.runtime.label(),
        "starting plugin guest"
    );
    let mut cmd = command_for_start(&jail.start, &plan.launcher, &plan.args);
    apply_common_env(&mut cmd, plugin, id);
    if plan.fronted_by_workerd() {
        inject_workerd_grant_env(&mut cmd, grant);
        if let Some(workerd_bin) = &plan.workerd_bin {
            cmd.env(WORKERD_BIN_ENV, workerd_bin);
        }
    }
    apply_temp_and_home(&mut cmd, &jail.scratch, &jail.data);
    for (key, value) in extra_env {
        cmd.env(*key, value);
    }
    apply_spec_env(&mut cmd, &jail.start)?;
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = cmd.spawn()?;
    if let Some(stderr) = child.stderr.take() {
        forward_guest_stderr(id.to_string(), "guest", stderr, Arc::clone(&stderr_tail));
    }
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| PluginError::message("plugin stdin missing"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| PluginError::message("plugin stdout missing"))?;
    let pid = child.id();
    Ok((child, None, stdin, stdout, None, pid, None))
}

/// Host-spawned gateway + native guest joined by inherited duplex links.
#[allow(clippy::too_many_arguments)]
async fn spawn_siblings(
    plugin: &DiscoveredPlugin,
    plan: &SpawnPlan,
    jail: &GuestJail,
    grant: &PluginGrant,
    extra_env: &[(&str, OsString)],
    id: &str,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
) -> Result<(
    Child,
    Option<Child>,
    ChildStdin,
    ChildStdout,
    Option<u32>,
    Option<u32>,
    Option<WindowsSessionJob>,
)> {
    let backend = plan.native_backend.as_ref().ok_or_else(|| {
        PluginError::message("native-behind-workerd spawn is missing the backend path")
    })?;
    let guest_start = jail.guest_start.as_ref().ok_or_else(|| {
        PluginError::message("native-behind-workerd jail plan is missing the guest start")
    })?;
    let session_dir = jail.session_dir.as_ref().ok_or_else(|| {
        PluginError::message("native-behind-workerd jail plan is missing the session directory")
    })?;

    // Outer Job before links, the proxy, or either sibling. A required failure
    // returns with nothing left running; the caller drops profiles and the
    // session directory.
    #[cfg(windows)]
    let session_job = match prepare_windows_session_job(jail, plan.runtime) {
        Ok(job) => job,
        Err(err) => return Err(err),
    };
    #[cfg(not(windows))]
    let session_job: Option<WindowsSessionJob> = None;

    // Unix links are socketpairs. Windows uses two unidirectional pipes per
    // link so a pending read cannot lock a write on the same pipe. Guest RPC
    // ends stay synchronous (Rust std aborts on overlapped stdin). Proxy ends
    // are overlapped on both peers because both wrap them in Tokio.
    #[cfg(unix)]
    let (rpc_gateway, rpc_guest) = DuplexLink::pair()
        .map_err(|err| PluginError::message(format!("could not create guest RPC link: {err}")))?;
    #[cfg(unix)]
    let (proxy_gateway, proxy_guest) = DuplexLink::pair().map_err(|err| {
        PluginError::message(format!("could not create socket-proxy link: {err}"))
    })?;
    #[cfg(windows)]
    let rpc_pipes = StdioEnds::pair()
        .map_err(|err| PluginError::message(format!("could not create guest RPC pipes: {err}")))?;
    #[cfg(windows)]
    let proxy_pipes = StdioEnds::pair_overlapped().map_err(|err| {
        PluginError::message(format!("could not create socket-proxy pipes: {err}"))
    })?;

    tracing::debug!(
        plugin = %id,
        gateway = %plan.launcher.display(),
        guest = %backend.display(),
        session = %session_dir.display(),
        "starting native-behind-workerd siblings"
    );

    let mut gateway_cmd = command_for_start(&jail.start, &plan.launcher, &plan.args);
    apply_common_env(&mut gateway_cmd, plugin, id);
    inject_workerd_grant_env(&mut gateway_cmd, grant);
    if let Some(workerd_bin) = &plan.workerd_bin {
        gateway_cmd.env(WORKERD_BIN_ENV, workerd_bin);
    }
    apply_temp_and_home(&mut gateway_cmd, session_dir, session_dir);
    gateway_cmd.env(WORKERD_STATE_DIR_ENV, session_dir);
    apply_spec_env(&mut gateway_cmd, &jail.start)?;
    gateway_cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut guest_cmd = command_for_start(guest_start, backend, &[]);
    apply_common_env(&mut guest_cmd, plugin, id);
    apply_temp_and_home(&mut guest_cmd, &jail.scratch, &jail.data);
    apply_spec_env(&mut guest_cmd, guest_start)?;
    for (key, value) in extra_env {
        guest_cmd.env(*key, value);
    }
    guest_cmd.stderr(Stdio::piped()).kill_on_drop(true);

    // The unsandboxed host serves the CONNECT mux. A jailed gateway cannot
    // dial the host's loopback on Windows (no machine-wide exemption), and
    // the same host-side check is the policy boundary on every OS.
    #[cfg(unix)]
    serve_host_socket_proxy(proxy_gateway, grant.egress_policy())?;
    #[cfg(windows)]
    serve_host_socket_proxy(
        proxy_pipes.host_stdout,
        proxy_pipes.host_stdin,
        grant.egress_policy(),
    )?;

    #[cfg(unix)]
    {
        inherit_unix_gateway(&mut gateway_cmd, &rpc_gateway);
        inherit_unix_guest(&mut guest_cmd, rpc_guest, &proxy_guest)?;
    }
    #[cfg(windows)]
    {
        guest_cmd.stdin(Stdio::piped()).stdout(Stdio::null());
        gateway_cmd.env(JAIL_HANDOFF_ENV, "1");
        guest_cmd.env(JAIL_HANDOFF_ENV, "1");
    }

    let mut gateway = gateway_cmd.spawn().map_err(|err| {
        PluginError::message(format!("could not start gateway for `{id}`: {err}"))
    })?;
    // Created before the handoff so the jail cannot finish CreateProcess
    // first and miss the event.
    #[cfg(windows)]
    let jail_ready = {
        let pid = gateway.id().ok_or_else(|| {
            PluginError::message(format!("gateway for `{id}` did not report a pid"))
        })?;
        bookclerk_sandbox::JailReady::create(pid).map_err(|err| {
            PluginError::message(format!("could not create the jail-ready event: {err}"))
        })?
    };
    if let Some(stderr) = gateway.stderr.take() {
        forward_guest_stderr(id.to_string(), "gateway", stderr, Arc::clone(&stderr_tail));
    }

    #[cfg(windows)]
    {
        // Longer than the jail's `Local\bookclerk-dacl-tx` wait (120s) so a
        // launch queued behind other grants is not killed while it still holds
        // or waits for that mutex. The e2e spawn deadline is longer than this.
        const READY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);
        // Join the session Job before the handoff. The jail blocks on that
        // line, so its child is created only after the jail is already in the Job.
        if let Some(job) = session_job.as_ref() {
            assign_job(job, &gateway)?;
        }
        if let Err(err) =
            windows_handoff_gateway(&mut gateway, &rpc_pipes.host_stdout, &rpc_pipes.host_stdin)
                .await
        {
            let _ = gateway.kill().await;
            return Err(err);
        }
        let ready_deadline = tokio::time::Instant::now() + READY_TIMEOUT;
        loop {
            match jail_ready.is_signaled() {
                Ok(true) => break,
                Ok(false) => {}
                Err(err) => {
                    let _ = gateway.kill().await;
                    return Err(PluginError::message(format!(
                        "jail-ready wait failed for `{id}`: {err}\n{}",
                        spawn_failure_detail(&mut gateway, None, &stderr_tail)
                    )));
                }
            }
            if gateway
                .try_wait()
                .map_err(|err| PluginError::message(format!("gateway wait: {err}")))?
                .is_some()
            {
                return Err(PluginError::message(format!(
                    "gateway for `{id}` exited before its child started\n{}",
                    spawn_failure_detail(&mut gateway, None, &stderr_tail)
                )));
            }
            if tokio::time::Instant::now() >= ready_deadline {
                let _ = gateway.kill().await;
                return Err(PluginError::message(format!(
                    "gateway for `{id}` did not start its child within {}s\n{}",
                    READY_TIMEOUT.as_secs(),
                    spawn_failure_detail(&mut gateway, None, &stderr_tail)
                )));
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    let guest_spawn = guest_cmd.spawn();
    let mut guest = match guest_spawn {
        Ok(child) => child,
        Err(err) => {
            let _ = gateway.kill().await;
            return Err(PluginError::message(format!(
                "could not start native guest for `{id}`: {err}\n{}",
                spawn_failure_detail(&mut gateway, None, &stderr_tail)
            )));
        }
    };
    if let Some(stderr) = guest.stderr.take() {
        forward_guest_stderr(id.to_string(), "guest", stderr, Arc::clone(&stderr_tail));
    }

    #[cfg(windows)]
    {
        if let Some(job) = session_job.as_ref() {
            if let Err(err) = assign_job(job, &guest) {
                let _ = gateway.kill().await;
                let _ = guest.kill().await;
                return Err(err);
            }
        }
        if let Err(err) = windows_handoff_guest(
            &mut guest,
            &rpc_pipes.guest_stdin,
            &rpc_pipes.guest_stdout,
            &proxy_pipes.guest_stdin,
            &proxy_pipes.guest_stdout,
        )
        .await
        {
            let _ = gateway.kill().await;
            let _ = guest.kill().await;
            return Err(err);
        }
    }
    #[cfg(unix)]
    {
        let _ = (rpc_gateway, proxy_guest);
    }

    let stdin = gateway
        .stdin
        .take()
        .ok_or_else(|| PluginError::message("gateway stdin missing"))?;
    let stdout = gateway
        .stdout
        .take()
        .ok_or_else(|| PluginError::message("gateway stdout missing"))?;
    let gateway_pid = gateway.id();
    let guest_pid = guest.id();
    Ok((
        gateway,
        Some(guest),
        stdin,
        stdout,
        gateway_pid,
        guest_pid,
        session_job,
    ))
}

/// `bookclerk-jail -- program args`, or `program args` when unconfined.
fn command_for_start(start: &Start, program: &std::path::Path, args: &[String]) -> Command {
    match start {
        Start::Confined { launcher, .. } => {
            let mut cmd = Command::new(launcher);
            cmd.arg("--").arg(program).args(args);
            // New process group so shutdown can signal the jail and its
            // grandchildren (pinned `workerd`) together. A lone SIGKILL of the
            // jail skips its `Drop` and leaves `workerd` holding the session dir.
            #[cfg(unix)]
            cmd.process_group(0);
            cmd
        }
        Start::Unconfined { reason } => {
            tracing::warn!(
                %reason,
                program = %program.display(),
                "starting plugin process WITHOUT a jail; it can reach everything this user can"
            );
            let mut cmd = Command::new(program);
            cmd.args(args);
            #[cfg(unix)]
            cmd.process_group(0);
            cmd
        }
    }
}

/// SIGKILL `pid`'s process group. No-op when the group is already gone.
///
/// Spawn uses `process_group(0)`, so `pid` is the group leader.
#[cfg(unix)]
pub(crate) fn kill_process_group(pid: u32) {
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
}

/// Allowlisted host env plus `BOOKCLERK_PLUGIN_*`.
fn apply_common_env(cmd: &mut Command, plugin: &DiscoveredPlugin, id: &str) {
    cmd.current_dir(&plugin.root).env_clear();
    for (key, value) in std::env::vars_os() {
        if crate::rpc::plugin_env_allowed(&key.to_string_lossy()) {
            cmd.env(key, value);
        }
    }
    cmd.env("BOOKCLERK_PLUGIN_ID", id);
    cmd.env("BOOKCLERK_PLUGIN_ROOT", &plugin.root);
    cmd.env("BOOKCLERK_PLUGIN_TOML", plugin.root.join("plugin.toml"));
}

fn apply_temp_and_home(cmd: &mut Command, tmp: &std::path::Path, home: &std::path::Path) {
    for key in ["TMPDIR", "TEMP", "TMP"] {
        cmd.env(key, tmp);
    }
    cmd.env("HOME", home);
}

fn apply_spec_env(cmd: &mut Command, start: &Start) -> Result<()> {
    if let Start::Confined { spec, .. } = start {
        cmd.env(
            bookclerk_sandbox::SPEC_ENV,
            serde_json::to_string(spec.as_ref()).map_err(|err| {
                PluginError::message(format!("could not encode the jail spec: {err}"))
            })?,
        );
    }
    Ok(())
}

#[cfg(unix)]
fn inherit_unix_gateway(cmd: &mut Command, rpc: &DuplexLink) {
    let rpc_fd = rpc.as_raw_fd();
    unsafe {
        cmd.pre_exec(move || {
            bookclerk_sandbox::inherit_fd_at(rpc_fd, GATEWAY_RPC_FD)?;
            Ok(())
        });
    }
    cmd.env(GATEWAY_GUEST_RPC_ENV, format!("fd:{GATEWAY_RPC_FD}"));
}

/// Serve the guest CONNECT mux in this process.
///
/// `link` is the host end of the inherited proxy. The guest holds the other
/// end. Dialing here reaches host loopback; the gateway AppContainer cannot.
#[cfg(unix)]
fn serve_host_socket_proxy(
    link: DuplexLink,
    policy: bookclerk_plugin_manifest::EgressPolicy,
) -> Result<()> {
    use std::os::unix::net::UnixStream;
    let std_stream = UnixStream::from(link.into_owned_fd());
    std_stream
        .set_nonblocking(true)
        .map_err(|err| PluginError::message(format!("host socket proxy nonblocking: {err}")))?;
    let stream = tokio::net::UnixStream::from_std(std_stream)
        .map_err(|err| PluginError::message(format!("host socket proxy wrap: {err}")))?;
    let fence = Arc::new(AtomicBool::new(false));
    bookclerk_workerd::socket_proxy::spawn_link(stream, policy, fence)
        .map_err(|err| PluginError::message(format!("host socket proxy failed to start: {err}")))
}

/// Serve the guest CONNECT mux on two unidirectional overlapped pipes.
#[cfg(windows)]
#[allow(unsafe_code)] // NamedPipeClient::from_raw_handle takes the inherited pipe.
fn serve_host_socket_proxy(
    read: DuplexHalf,
    write: DuplexHalf,
    policy: bookclerk_plugin_manifest::EgressPolicy,
) -> Result<()> {
    use std::os::windows::io::IntoRawHandle;
    let read = unsafe {
        tokio::net::windows::named_pipe::NamedPipeClient::from_raw_handle(
            read.into_owned_handle().into_raw_handle(),
        )
    }
    .map_err(|err| PluginError::message(format!("host proxy read pipe: {err}")))?;
    let write = unsafe {
        tokio::net::windows::named_pipe::NamedPipeClient::from_raw_handle(
            write.into_owned_handle().into_raw_handle(),
        )
    }
    .map_err(|err| PluginError::message(format!("host proxy write pipe: {err}")))?;
    let fence = Arc::new(AtomicBool::new(false));
    bookclerk_workerd::socket_proxy::spawn_halves(read, write, policy, fence)
        .map_err(|err| PluginError::message(format!("host socket proxy failed to start: {err}")))
}

#[cfg(unix)]
fn inherit_unix_guest(cmd: &mut Command, rpc: DuplexLink, proxy: &DuplexLink) -> Result<()> {
    let rpc_in = rpc
        .try_clone()
        .map_err(|err| PluginError::message(format!("clone guest RPC end: {err}")))?;
    cmd.stdin(Stdio::from(rpc_in.into_owned_fd()));
    cmd.stdout(Stdio::from(rpc.into_owned_fd()));
    let proxy_fd = proxy.as_raw_fd();
    unsafe {
        cmd.pre_exec(move || {
            bookclerk_sandbox::inherit_fd_at(proxy_fd, GUEST_PROXY_FD)?;
            Ok(())
        });
    }
    cmd.env(SOCKET_PROXY_ENV, format!("fd:{GUEST_PROXY_FD}"));
    Ok(())
}

#[cfg(windows)]
async fn windows_handoff_gateway(
    child: &mut Child,
    rpc_read: &DuplexHalf,
    rpc_write: &DuplexHalf,
) -> Result<()> {
    let target = process_handle(child)?;
    // Read half is guest → gateway. Write half is gateway → guest.
    // The CONNECT mux stays in the host; this jail only receives guest RPC.
    let rpc_read_h = bookclerk_sandbox::duplicate_handle_into(rpc_read.as_raw_handle(), target)
        .map_err(|err| PluginError::message(format!("DuplicateHandle gateway RPC read: {err}")))?;
    let rpc_write_h = bookclerk_sandbox::duplicate_handle_into(rpc_write.as_raw_handle(), target)
        .map_err(|err| {
        PluginError::message(format!("DuplicateHandle gateway RPC write: {err}"))
    })?;
    let handoff = JailHandoff {
        v: JailHandoff::VERSION,
        stdin: None,
        stdout: None,
        extra: vec![
            JailHandoffExtra {
                env: GATEWAY_GUEST_RPC_ENV.into(),
                handle: rpc_read_h,
            },
            JailHandoffExtra {
                env: GATEWAY_GUEST_RPC_WRITE_ENV.into(),
                handle: rpc_write_h,
            },
        ],
    };
    write_handoff_line(child, &handoff).await
}

#[cfg(windows)]
async fn windows_handoff_guest(
    child: &mut Child,
    rpc_stdin: &DuplexHalf,
    rpc_stdout: &DuplexHalf,
    proxy_read: &DuplexHalf,
    proxy_write: &DuplexHalf,
) -> Result<()> {
    let target = process_handle(child)?;
    // Separate pipes. Duplicating one duplex end for both stdio handles lets a
    // synchronous read lock the write, and the inherit list also rejects a
    // repeated handle value.
    let rpc_in = bookclerk_sandbox::duplicate_handle_into(rpc_stdin.as_raw_handle(), target)
        .map_err(|err| PluginError::message(format!("DuplicateHandle guest RPC stdin: {err}")))?;
    let rpc_out = bookclerk_sandbox::duplicate_handle_into(rpc_stdout.as_raw_handle(), target)
        .map_err(|err| PluginError::message(format!("DuplicateHandle guest RPC stdout: {err}")))?;
    let proxy_read_h = bookclerk_sandbox::duplicate_handle_into(proxy_read.as_raw_handle(), target)
        .map_err(|err| PluginError::message(format!("DuplicateHandle guest proxy read: {err}")))?;
    let proxy_write_h =
        bookclerk_sandbox::duplicate_handle_into(proxy_write.as_raw_handle(), target).map_err(
            |err| PluginError::message(format!("DuplicateHandle guest proxy write: {err}")),
        )?;
    let handoff = JailHandoff {
        v: JailHandoff::VERSION,
        stdin: Some(rpc_in),
        stdout: Some(rpc_out),
        extra: vec![
            JailHandoffExtra {
                env: SOCKET_PROXY_ENV.into(),
                handle: proxy_read_h,
            },
            JailHandoffExtra {
                env: bookclerk_sandbox::SOCKET_PROXY_WRITE_ENV.into(),
                handle: proxy_write_h,
            },
        ],
    };
    write_handoff_line(child, &handoff).await?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.shutdown().await;
    }
    Ok(())
}

#[cfg(windows)]
async fn write_handoff_line(child: &mut Child, handoff: &JailHandoff) -> Result<()> {
    let line = handoff
        .to_line()
        .map_err(|err| PluginError::message(format!("encode jail handoff: {err}")))?;
    let stdin = child
        .stdin
        .as_mut()
        .ok_or_else(|| PluginError::message("jail stdin missing for handoff"))?;
    stdin
        .write_all(line.as_bytes())
        .await
        .map_err(|err| PluginError::message(format!("write jail handoff: {err}")))?;
    stdin
        .write_all(b"\n")
        .await
        .map_err(|err| PluginError::message(format!("write jail handoff newline: {err}")))?;
    stdin
        .flush()
        .await
        .map_err(|err| PluginError::message(format!("flush jail handoff: {err}")))?;
    Ok(())
}

#[cfg(windows)]
fn assign_job(job: &bookclerk_sandbox::SessionJob, child: &Child) -> Result<()> {
    job.assign(process_handle(child)?)
        .map_err(|err| PluginError::message(format!("AssignProcessToJobObject: {err}")))
}

#[cfg(windows)]
fn process_handle(child: &Child) -> Result<std::os::windows::io::RawHandle> {
    child
        .raw_handle()
        .ok_or_else(|| PluginError::message("jail process handle is gone"))
}

#[cfg(windows)]
type WindowsSessionJob = bookclerk_sandbox::SessionJob;
#[cfg(not(windows))]
type WindowsSessionJob = ();

/// Why creating the outer Windows session Job failed.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OuterJobFailure {
    /// The kernel does not implement the Job feature that was requested.
    Unsupported,
    /// Create or configure failed for a reason other than missing support.
    Failed,
}

/// What the host does with an outer-Job outcome.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OuterJobDecision {
    /// Job exists and siblings may be assigned to it.
    Present,
    /// Best-effort / off continued and recorded that there is no outer Job.
    AbsentUnsupported,
    /// Required, or a failure that is not an explicit lack of support.
    FailClosed,
}

/// Required isolation fails closed. Best-effort may continue only when the
/// missing piece is explicitly unsupported.
#[cfg_attr(not(windows), allow(dead_code))]
fn decide_outer_job(
    isolation: bookclerk_config::Isolation,
    failure: Option<OuterJobFailure>,
) -> OuterJobDecision {
    match failure {
        None => OuterJobDecision::Present,
        Some(OuterJobFailure::Unsupported)
            if matches!(
                isolation,
                bookclerk_config::Isolation::BestEffort | bookclerk_config::Isolation::Off
            ) =>
        {
            OuterJobDecision::AbsentUnsupported
        }
        Some(_) => OuterJobDecision::FailClosed,
    }
}

/// Create the outer session Job, or fail before either sibling starts.
#[cfg(windows)]
fn prepare_windows_session_job(
    jail: &GuestJail,
    runtime: crate::GuestRuntimeKind,
) -> Result<Option<bookclerk_sandbox::SessionJob>> {
    let limits = crate::jail::windows_outer_job_limits(jail.session_limits, runtime);
    match bookclerk_sandbox::SessionJob::create(&limits) {
        Ok(job) => Ok(Some(job)),
        Err(err) => {
            let failure = if err.kind() == std::io::ErrorKind::Unsupported {
                OuterJobFailure::Unsupported
            } else {
                OuterJobFailure::Failed
            };
            match decide_outer_job(jail.isolation, Some(failure)) {
                OuterJobDecision::Present => Err(PluginError::message(
                    "outer session Job decision was Present after a create failure",
                )),
                OuterJobDecision::AbsentUnsupported => {
                    tracing::warn!(
                        isolation = jail.isolation.as_str(),
                        error = %err,
                        "outer session Job is unsupported; continuing with no outer job"
                    );
                    Ok(None)
                }
                OuterJobDecision::FailClosed => Err(PluginError::message(format!(
                    "could not create the outer session Job ({err}); no sibling was started"
                ))),
            }
        }
    }
}

/// Keys the host must never place on the native sibling.
#[cfg(test)]
fn guest_env_forbidden_key(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    upper.starts_with("BOOKCLERK_WORKERD_GRANT_")
        || upper.starts_with("BOOKCLERK_JAIL_")
        || upper == GATEWAY_GUEST_RPC_ENV
        || upper == GATEWAY_GUEST_RPC_WRITE_ENV
        || upper == GATEWAY_PROXY_ENV
        || upper == bookclerk_sandbox::GATEWAY_PROXY_WRITE_ENV
        || upper == WORKERD_STATE_DIR_ENV
        || upper == "BOOKCLERK_NATIVE_BACKEND"
        || upper == "BOOKCLERK_NESTED_NATIVE_JAIL"
        || upper == "BOOKCLERK_NESTED_JAIL_ENFORCEMENT"
        || upper == "BOOKCLERK_NESTED_AC_PROFILE"
        || upper == "BOOKCLERK_NESTED_AC_SID"
}

/// Guest environment keys after the host allowlist + curated bootstrap.
///
/// Used by the env-contract unit test. `extra` is applied last (sqlite / S3).
#[cfg(test)]
fn curated_guest_env_keys(extra: &[(&str, OsString)]) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy().into_owned();
        if crate::rpc::plugin_env_allowed(&name) {
            keys.insert(name);
        }
    }
    for key in [
        "BOOKCLERK_PLUGIN_ID",
        "BOOKCLERK_PLUGIN_ROOT",
        "BOOKCLERK_PLUGIN_TOML",
        "HOME",
        "TMPDIR",
        "TEMP",
        "TMP",
        SOCKET_PROXY_ENV,
    ] {
        keys.insert(key.to_string());
    }
    for (key, _) in extra {
        keys.insert((*key).to_string());
    }
    keys.retain(|k| !guest_env_forbidden_key(k));
    keys
}

/// Lines of guest stderr retained for spawn/describe failure messages.
const STDERR_TAIL_LINES: usize = 40;

/// Re-emits each guest stderr line through tracing so `bookclerkd` JSON logs
/// stay structured. ANSI from guest formatters is stripped so JSON
/// does not encode CSI as `\u001b`. Also keeps a short ring for panic text.
fn forward_guest_stderr(
    plugin: String,
    tag: &'static str,
    stderr: ChildStderr,
    tail: Arc<Mutex<VecDeque<String>>>,
) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let line = bookclerk_config::strip_ansi_escapes(&line);
            if line.is_empty() {
                continue;
            }
            let tagged = format!("[{tag}] {line}");
            if let Ok(mut buf) = tail.lock() {
                if buf.len() >= STDERR_TAIL_LINES {
                    buf.pop_front();
                }
                buf.push_back(tagged.clone());
            }
            tracing::info!(plugin = %plugin, sibling = tag, "{line}");
        }
    });
}

/// Guest process status plus captured stderr, for describe/spawn failures.
pub(crate) fn spawn_failure_detail(
    child: &mut Child,
    guest: Option<&mut Child>,
    stderr_tail: &Arc<Mutex<VecDeque<String>>>,
) -> String {
    let mut parts = vec![child_status("gateway", child)];
    if let Some(guest) = guest {
        parts.push(child_status("guest", guest));
    }
    let status = parts.join("; ");
    let stderr = stderr_tail_text(stderr_tail);
    if stderr.is_empty() {
        status
    } else {
        format!("{status}\n--- guest stderr ---\n{stderr}")
    }
}

fn child_status(tag: &str, child: &mut Child) -> String {
    match child.try_wait() {
        Ok(Some(st)) => format!("{tag} exited: {st}"),
        Ok(None) => format!("{tag} still running"),
        Err(e) => format!("{tag} wait error: {e}"),
    }
}

/// Attaches [`spawn_failure_detail`] without losing the ABI error class.
pub(crate) fn with_spawn_detail(err: PluginError, extra: String) -> PluginError {
    match err {
        PluginError::Unavailable(message) => {
            PluginError::unavailable(format!("{message}; {extra}"))
        }
        PluginError::Message(message) => PluginError::message(format!("{message}; {extra}")),
        PluginError::Abi { code, message } => {
            PluginError::from_abi(Some(&code), format!("{message}; {extra}"))
        }
        other => PluginError::message(format!("{other}; {extra}")),
    }
}

/// Applies host-implied network overlays to a persisted spawn grant.
pub(crate) fn effective_spawn_grant(
    persisted: &PluginGrant,
    plugin: &DiscoveredPlugin,
    config: &Config,
) -> PluginGrant {
    let mut grant = persisted.clone();
    crate::consent::overlay_host_implied_network(
        &mut grant,
        plugin,
        config,
        &overlay_discovered_plugins(config, plugin),
    );
    grant
}

/// Occupancy list used to uniquify host overlays, always including `plugin`.
///
/// Must **not** call [`crate::discover_plugins`]: that re-hashes every staged
/// payload (debug guest binaries are hundreds of MiB) on each spawn.
pub(crate) fn overlay_discovered_plugins(
    config: &Config,
    plugin: &DiscoveredPlugin,
) -> Vec<DiscoveredPlugin> {
    match crate::discover::discover_occupancy_plugins(config) {
        Ok(mut list) => {
            list.retain(|found| found.root != plugin.root);
            list.push(plugin.clone());
            list
        }
        Err(err) => {
            tracing::warn!(
                error = %err,
                "overlay occupancy scan failed; skipping host-implied network overlay"
            );
            Vec::new()
        }
    }
}

fn stderr_tail_text(tail: &Arc<Mutex<VecDeque<String>>>) -> String {
    tail.lock()
        .map(|buf| buf.iter().cloned().collect::<Vec<_>>().join("\n"))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guest_env_contract_excludes_gateway_secrets() {
        let extra = [("BOOKCLERK_SQLITE_PATH", OsString::from("/tmp/library.db"))];
        let keys = curated_guest_env_keys(&extra);
        assert!(keys.contains("BOOKCLERK_PLUGIN_ID"));
        assert!(keys.contains("BOOKCLERK_PLUGIN_ROOT"));
        assert!(keys.contains("BOOKCLERK_SOCKET_PROXY"));
        assert!(keys.contains("BOOKCLERK_SQLITE_PATH"));
        assert!(keys.contains("HOME"));
        assert!(keys.contains("TMPDIR"));
        for key in &keys {
            assert!(
                !guest_env_forbidden_key(key),
                "guest env must not contain {key}"
            );
        }
        assert!(!keys.contains(GATEWAY_GUEST_RPC_ENV));
        assert!(!keys.contains(GATEWAY_GUEST_RPC_WRITE_ENV));
        assert!(!keys.contains(GATEWAY_PROXY_ENV));
        assert!(!keys.contains(WORKERD_STATE_DIR_ENV));
        assert!(!keys.contains(bookclerk_sandbox::SPEC_ENV));
        assert!(!keys
            .iter()
            .any(|k| k.starts_with("BOOKCLERK_WORKERD_GRANT_")));
        assert!(!keys.iter().any(|k| k.starts_with("BOOKCLERK_JAIL_")));
    }

    #[test]
    fn outer_job_fails_closed_unless_the_gap_is_explicitly_unsupported() {
        use bookclerk_config::Isolation;
        assert_eq!(
            decide_outer_job(Isolation::Required, None),
            OuterJobDecision::Present
        );
        assert_eq!(
            decide_outer_job(Isolation::Required, Some(OuterJobFailure::Unsupported)),
            OuterJobDecision::FailClosed
        );
        assert_eq!(
            decide_outer_job(Isolation::Required, Some(OuterJobFailure::Failed)),
            OuterJobDecision::FailClosed
        );
        assert_eq!(
            decide_outer_job(Isolation::BestEffort, Some(OuterJobFailure::Failed)),
            OuterJobDecision::FailClosed
        );
        assert_eq!(
            decide_outer_job(Isolation::BestEffort, Some(OuterJobFailure::Unsupported)),
            OuterJobDecision::AbsentUnsupported
        );
        assert_eq!(
            decide_outer_job(Isolation::Off, Some(OuterJobFailure::Unsupported)),
            OuterJobDecision::AbsentUnsupported
        );
    }
}
