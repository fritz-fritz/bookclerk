//! Shared jailed-child spawn for Cap'n Proto `api_version = 3` stdio guests.

#![allow(clippy::missing_docs_in_private_items)]

use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use bookclerk_config::Config;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};

use crate::consent::{inject_workerd_grant_env, spawn_config_for_grant, spawn_grant, PluginGrant};
use crate::discover::DiscoveredPlugin;
use crate::jail::{GuestJail, Start};
use crate::spawn_plan::{
    SpawnPlan, NATIVE_BACKEND_ENV, NESTED_JAIL_BIN_ENV, NESTED_JAIL_ENFORCEMENT_ENV,
    NESTED_NATIVE_JAIL_ENV, WORKERD_BIN_ENV,
};
#[cfg(windows)]
use crate::spawn_plan::{NESTED_AC_PROFILE_ENV, NESTED_AC_SID_ENV};
use crate::{PluginError, Result};

/// Literal jail launcher basename (`bookclerk-jail[.exe]`).
#[cfg(windows)]
const JAIL_BIN_LITERAL: &str = "bookclerk-jail.exe";
/// Literal jail launcher basename (`bookclerk-jail[.exe]`).
#[cfg(not(windows))]
const JAIL_BIN_LITERAL: &str = "bookclerk-jail";

/// Literal workerd front-door basename (`bookclerk-workerd[.exe]`).
#[cfg(windows)]
const WORKERD_LAUNCHER_LITERAL: &str = "bookclerk-workerd.exe";
/// Literal workerd front-door basename (`bookclerk-workerd[.exe]`).
#[cfg(not(windows))]
const WORKERD_LAUNCHER_LITERAL: &str = "bookclerk-workerd";

/// Prepends `dir` to an existing PATH value (or starts a new PATH).
fn prepend_path_dir_to(dir: &Path, existing: Option<&OsString>) -> OsString {
    let dir = dir.to_string_lossy();
    match existing {
        Some(existing) => {
            #[cfg(windows)]
            let sep = ';';
            #[cfg(not(windows))]
            let sep = ':';
            format!("{dir}{sep}{}", existing.to_string_lossy()).into()
        }
        None => dir.into_owned().into(),
    }
}

/// Prepends `dir` to the process `PATH`.
fn prepend_path_dir(dir: &Path) -> OsString {
    prepend_path_dir_to(dir, std::env::var_os("PATH").as_ref())
}

/// Maps sandbox path validation failures into plugin spawn errors.
fn spawn_path_err(err: bookclerk_sandbox::SpawnPathError) -> PluginError {
    PluginError::message(format!("invalid spawn path: {err}"))
}

/// Requires `path`'s basename equals `expected`, returning its parent directory.
fn require_named_helper(path: &Path, expected: &str) -> Result<PathBuf> {
    let validated = bookclerk_sandbox::require_spawn_executable(path).map_err(spawn_path_err)?;
    let base = validated
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if base != expected {
        return Err(PluginError::message(format!(
            "expected helper named {expected}, got {base}"
        )));
    }
    validated.parent().map(|p| p.to_path_buf()).ok_or_else(|| {
        PluginError::message(format!(
            "helper {} has no parent directory",
            validated.display()
        ))
    })
}

/// Jailed plugin child with stdio pipes (describe not yet called).
pub(crate) struct SpawnedStdio {
    /// Provenance-qualified PluginKey (canonical text).
    pub id: String,
    /// Manifest display alias (`plugin.toml` `id`).
    pub alias: String,
    /// Child process; killed on drop of the session that owns it.
    pub child: Child,
    /// Guest stdin (host writes RPC / capnp).
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
    /// AppContainer package SID.
    #[cfg(windows)]
    pub package_sid: Option<String>,
    /// Host-owned AppContainer profile. Held so Drop does not tear down the
    /// jail while the vat thread still owns this guest.
    #[cfg(windows)]
    #[allow(dead_code)]
    pub appcontainer: Option<bookclerk_sandbox::spawn::AppContainerSession>,
    /// Nested Deny AppContainer for the native backend. Held for the same
    /// lifetime as [`Self::appcontainer`].
    #[cfg(windows)]
    #[allow(dead_code)]
    pub nested_appcontainer: Option<bookclerk_sandbox::spawn::AppContainerSession>,
    /// Last lines of guest stderr (workerd + native child), for spawn failures.
    pub stderr_tail: Arc<Mutex<VecDeque<String>>>,
    /// Files dir used to re-read `plugin-grants.json` before returning a session.
    pub files_dir: PathBuf,
}

/// Spawns the jailed guest with piped stdio. Caller performs Cap'n Proto connect.
///
/// `plan` names the program the jail execs (`bookclerk-workerd` on the product
/// path) and, for a native backend, the executable the launcher must front.
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
    extra_env: &[(&str, std::ffi::OsString)],
) -> Result<SpawnedStdio> {
    let id = plugin.plugin_key().canonical().to_string();
    let alias = plugin.manifest.id.clone();
    let persisted_grant = spawn_grant(&config.paths().files_dir, plugin)?;
    let grant = effective_spawn_grant(&persisted_grant, plugin, config);
    let spawn_config = spawn_config_for_grant(&grant, config_table);
    let jail = GuestJail::plan(config, plugin, plan)?;
    #[cfg(windows)]
    let mut nested_appcontainer = None;

    // When confined, PATH must resolve literal `bookclerk-jail` / `bookclerk-workerd`
    // beside the validated helpers — re-applied after `Command::env_clear` below.
    let mut confined_helper_path: Option<OsString> = None;
    let mut cmd = match &jail.start {
        Start::Confined { launcher, .. } => {
            tracing::debug!(
                plugin = %id,
                launcher = %launcher.display(),
                program = %plan.launcher.display(),
                runtime = plan.runtime.label(),
                "starting plugin guest under a jail"
            );
            let jail_dir = require_named_helper(launcher, JAIL_BIN_LITERAL)?;
            let guest_dir = require_named_helper(&plan.launcher, WORKERD_LAUNCHER_LITERAL)?;
            // Product workerd plans never carry argv; refuse so we never pass
            // tainted `.args` into the literal jail/workerd command line.
            if !plan.args.is_empty() {
                return Err(PluginError::message(format!(
                    "confined spawn of `{alias}` refuses non-empty argv behind bookclerk-workerd"
                )));
            }
            // Jail dir first, then workerd front-door dir, then process PATH.
            let path = prepend_path_dir_to(&guest_dir, Some(&prepend_path_dir(&jail_dir)));
            confined_helper_path = Some(path);
            // Literal jail + literal front-door only (no tainted argv).
            // codeql[rust/command-line-injection]
            let mut cmd = Command::new(JAIL_BIN_LITERAL);
            // codeql[rust/command-line-injection]
            cmd.arg("--").arg(WORKERD_LAUNCHER_LITERAL);
            cmd
        }
        Start::Unconfined { reason } => {
            tracing::warn!(
                plugin = %id,
                %reason,
                runtime = plan.runtime.label(),
                "starting plugin guest WITHOUT a jail; it can reach everything \
                 this user can"
            );
            let guest = bookclerk_sandbox::require_spawn_executable(&plan.launcher)
                .map_err(spawn_path_err)?;
            let base = guest
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if base == WORKERD_LAUNCHER_LITERAL {
                if !plan.args.is_empty() {
                    return Err(PluginError::message(format!(
                        "unconfined workerd spawn of `{alias}` refuses non-empty argv"
                    )));
                }
                let guest_dir = guest.parent().ok_or_else(|| {
                    PluginError::message(format!(
                        "front-door launcher {} has no parent directory",
                        guest.display()
                    ))
                })?;
                confined_helper_path = Some(prepend_path_dir(guest_dir));
                // Literal front-door only (no tainted argv).
                // codeql[rust/command-line-injection]
                Command::new(WORKERD_LAUNCHER_LITERAL)
            } else {
                // Diagnostic direct-native transport (tests): same Command shape as
                // main so existing alerts are not reintroduced as PR-new findings.
                let mut cmd = Command::new(&plan.launcher);
                cmd.args(&plan.args);
                cmd
            }
        }
    };

    cmd.current_dir(&plugin.root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env_clear();
    for (key, value) in std::env::vars_os() {
        if crate::rpc::plugin_env_allowed(&key.to_string_lossy()) {
            cmd.env(key, value);
        }
    }
    if let Some(path) = confined_helper_path {
        cmd.env("PATH", path);
    }
    cmd.env("BOOKCLERK_PLUGIN_ID", &id);
    // `bookclerk-workerd` reads the manifest from here (falling back to cwd).
    cmd.env("BOOKCLERK_PLUGIN_ROOT", &plugin.root);
    cmd.env("BOOKCLERK_PLUGIN_TOML", plugin.root.join("plugin.toml"));
    if plan.fronted_by_workerd() {
        inject_workerd_grant_env(&mut cmd, &grant);
        if let Some(backend) = &plan.native_backend {
            cmd.env(NATIVE_BACKEND_ENV, backend);
            // Nested Deny is independent of the outer launcher jail. Isolation::Off
            // still fronts native guests with workerd; ambient AF_INET stays denied
            // when bookclerk-jail is beside the launcher.
            if let Some(jail_bin) = plan.nested_jail_helper() {
                cmd.env(NESTED_JAIL_BIN_ENV, jail_bin);
            }
            #[cfg(not(windows))]
            cmd.env(NESTED_NATIVE_JAIL_ENV, "1");
            #[cfg(windows)]
            {
                let label = format!("native-behind-workerd:{alias}");
                match bookclerk_sandbox::spawn::AppContainerSession::create(&label) {
                    Ok(session) => {
                        cmd.env(NESTED_NATIVE_JAIL_ENV, "1");
                        cmd.env(NESTED_AC_PROFILE_ENV, session.profile_name());
                        cmd.env(NESTED_AC_SID_ENV, session.package_sid());
                        nested_appcontainer = Some(session);
                    }
                    Err(err) => {
                        let required = matches!(
                            &jail.start,
                            Start::Confined { spec, .. }
                                if spec.enforcement == bookclerk_sandbox::Enforcement::Required
                        );
                        if required {
                            return Err(PluginError::message(format!(
                                "could not pre-create nested AppContainer for `{alias}`: {err}"
                            )));
                        }
                        tracing::warn!(
                            error = %err,
                            "could not pre-create nested AppContainer; native guest will not \
                             get nested Deny (OAuth and SOCKET_PROXY need the Package SID)"
                        );
                    }
                }
            }
            if let Start::Confined { spec, .. } = &jail.start {
                if spec.enforcement == bookclerk_sandbox::Enforcement::Required {
                    cmd.env(NESTED_JAIL_ENFORCEMENT_ENV, "required");
                }
            }
        }
        // The launcher resolves `workerd` beside itself unless told otherwise;
        // the jail already grants whichever the plan resolved.
        if let Some(workerd_bin) = &plan.workerd_bin {
            cmd.env(WORKERD_BIN_ENV, workerd_bin);
        }
    }
    for key in ["TMPDIR", "TEMP", "TMP"] {
        cmd.env(key, &jail.scratch);
    }
    cmd.env("HOME", &jail.data);
    for (key, value) in extra_env {
        cmd.env(key, value);
    }
    if let Start::Confined { spec, .. } = &jail.start {
        cmd.env(
            bookclerk_sandbox::SPEC_ENV,
            serde_json::to_string(spec.as_ref()).map_err(|err| {
                PluginError::message(format!("could not encode the jail spec: {err}"))
            })?,
        );
    }

    let mut child = cmd.spawn()?;

    let stderr_tail = Arc::new(Mutex::new(VecDeque::new()));
    if let Some(stderr) = child.stderr.take() {
        forward_guest_stderr(id.clone(), stderr, Arc::clone(&stderr_tail));
    }
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| PluginError::message("plugin stdin missing"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| PluginError::message("plugin stdout missing"))?;

    Ok(SpawnedStdio {
        id,
        alias,
        child,
        stdin,
        stdout,
        grant,
        persisted_grant,
        spawn_config,
        data: jail.data,
        scratch: jail.scratch,
        #[cfg(windows)]
        package_sid: nested_appcontainer
            .as_ref()
            .map(|s| s.package_sid().to_string())
            .or(jail.package_sid),
        #[cfg(windows)]
        appcontainer: jail.appcontainer,
        #[cfg(windows)]
        nested_appcontainer,
        stderr_tail,
        files_dir: config.paths().files_dir.clone(),
    })
}

/// Lines of guest stderr retained for spawn/describe failure messages.
const STDERR_TAIL_LINES: usize = 40;

/// Re-emits each guest stderr line through tracing so `bookclerkd` JSON logs
/// stay structured (jail summaries used to land as raw `eprintln!` on the
/// inherited daemon stderr). ANSI from guest formatters is stripped so JSON
/// does not encode CSI as `\u001b`. Also keeps a short ring for panic text:
/// tests often have no tracing subscriber, so workerd/guest logs were invisible.
fn forward_guest_stderr(plugin: String, stderr: ChildStderr, tail: Arc<Mutex<VecDeque<String>>>) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let line = bookclerk_config::strip_ansi_escapes(&line);
            if line.is_empty() {
                continue;
            }
            if let Ok(mut buf) = tail.lock() {
                if buf.len() >= STDERR_TAIL_LINES {
                    buf.pop_front();
                }
                buf.push_back(line.to_string());
            }
            tracing::info!(plugin = %plugin, "{line}");
        }
    });
}

/// Guest process status plus captured stderr, for describe/spawn failures.
pub(crate) fn spawn_failure_detail(
    child: &mut Child,
    stderr_tail: &Arc<Mutex<VecDeque<String>>>,
) -> String {
    let status = match child.try_wait() {
        Ok(Some(st)) => format!("guest exited: {st}"),
        Ok(None) => "guest still running".into(),
        Err(e) => format!("guest wait error: {e}"),
    };
    let stderr = stderr_tail_text(stderr_tail);
    if stderr.is_empty() {
        status
    } else {
        format!("{status}\n--- guest stderr ---\n{stderr}")
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
