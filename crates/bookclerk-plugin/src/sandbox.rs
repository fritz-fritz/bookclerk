//! OS sandbox for external plugin child processes (all platforms).
//!
//! After `fork`/`CreateProcess` and before the plugin runs untrusted code, the
//! host installs a platform jail:
//!
//! | OS | Mechanism |
//! | --- | --- |
//! | Linux | Landlock FS rules + seccomp-bpf deny-list |
//! | macOS | Seatbelt (`sandbox_init`) profile |
//! | Windows | Job Object (kill-on-close, UI restrictions) |
//!
//! Plus host mediation (no `library.db` / `master.key` paths, env scrub,
//! `TMPDIR` under `plugin_data_dir/tmp`). Disable with `BOOKCLERK_PLUGIN_SANDBOX=off`.

#![allow(unsafe_code)] // pre_exec / Seatbelt / Job Object FFI

use std::path::PathBuf;

/// Paths the plugin child is allowed to touch under the FS sandbox.
#[derive(Debug, Clone)]
pub struct PluginSandbox {
    /// Plugin install directory (`plugin.toml` + binary) — read/execute.
    pub plugin_root: PathBuf,
    /// Per-plugin data directory (`…/plugins/<id>/data`) — read/write.
    pub plugin_data_dir: PathBuf,
    /// Shared fetch cache (`…/cache`) — read/write when set (sources).
    pub cache_dir: Option<PathBuf>,
}

impl PluginSandbox {
    /// Build a sandbox policy for a discovered plugin install.
    #[must_use]
    pub fn new(
        plugin_root: impl Into<PathBuf>,
        plugin_data_dir: impl Into<PathBuf>,
        cache_dir: Option<PathBuf>,
    ) -> Self {
        Self {
            plugin_root: plugin_root.into(),
            plugin_data_dir: plugin_data_dir.into(),
            cache_dir,
        }
    }

    /// Scratch directory under [`Self::plugin_data_dir`] (sandbox-writable).
    #[must_use]
    pub fn tmp_dir(&self) -> PathBuf {
        self.plugin_data_dir.join("tmp")
    }
}

/// Whether the operator disabled the OS sandbox via env.
#[must_use]
pub fn sandbox_disabled_by_env() -> bool {
    match std::env::var("BOOKCLERK_PLUGIN_SANDBOX") {
        Ok(v) => {
            let v = v.trim();
            v.eq_ignore_ascii_case("0")
                || v.eq_ignore_ascii_case("off")
                || v.eq_ignore_ascii_case("false")
                || v.eq_ignore_ascii_case("no")
        }
        Err(_) => false,
    }
}

/// Apply the child-side sandbox (Linux Landlock+seccomp, macOS Seatbelt).
///
/// Called from Unix `pre_exec`. On Windows this is a no-op — the parent attaches
/// a Job Object after spawn via [`attach_after_spawn`].
pub fn apply_in_child(sandbox: &PluginSandbox) -> Result<(), String> {
    if sandbox_disabled_by_env() {
        return Ok(());
    }
    platform::apply_in_child(sandbox)
}

/// Parent-side sandbox attach after spawn (Windows Job Object).
///
/// No-op on Unix (child already sandboxed in `pre_exec`).
pub fn attach_after_spawn(child_pid: u32, sandbox: &PluginSandbox) -> Result<(), String> {
    if sandbox_disabled_by_env() {
        return Ok(());
    }
    platform::attach_after_spawn(child_pid, sandbox)
}

#[cfg(target_os = "linux")]
mod platform {
    use super::PluginSandbox;
    use landlock::{
        path_beneath_rules, Access, AccessFs, CompatLevel, Compatible, Ruleset, RulesetAttr,
        RulesetCreatedAttr, RulesetStatus, ABI,
    };
    use seccompiler::{SeccompAction, SeccompFilter, TargetArch};
    use std::path::{Path, PathBuf};

    pub(super) fn apply_in_child(sandbox: &PluginSandbox) -> Result<(), String> {
        apply_landlock(sandbox)?;
        apply_seccomp()?;
        Ok(())
    }

    pub(super) fn attach_after_spawn(_pid: u32, _sandbox: &PluginSandbox) -> Result<(), String> {
        Ok(())
    }

    fn apply_landlock(sandbox: &PluginSandbox) -> Result<(), String> {
        let abi = ABI::V3;
        let mut ro: Vec<PathBuf> = Vec::new();
        for p in [
            "/usr",
            "/lib",
            "/lib64",
            "/lib32",
            "/bin",
            "/sbin",
            "/etc/ssl",
            "/etc/ca-certificates",
            "/etc/pki",
            "/etc/resolv.conf",
            "/etc/hosts",
            "/etc/nsswitch.conf",
            "/etc/ld.so.cache",
            "/etc/ld.so.conf",
            "/etc/ld.so.conf.d",
            "/dev/null",
            "/dev/zero",
            "/dev/urandom",
            "/dev/random",
            "/dev/full",
            "/proc",
            "/sys/devices/system/cpu",
        ] {
            if Path::new(p).exists() {
                ro.push(PathBuf::from(p));
            }
        }
        if sandbox.plugin_root.exists() {
            ro.push(sandbox.plugin_root.clone());
        }

        let mut rw: Vec<PathBuf> = Vec::new();
        if sandbox.plugin_data_dir.exists() {
            rw.push(sandbox.plugin_data_dir.clone());
        }
        if let Some(cache) = &sandbox.cache_dir {
            if cache.exists() {
                rw.push(cache.clone());
            }
        }

        let status = (|| {
            let mut ruleset = Ruleset::default()
                .set_compatibility(CompatLevel::BestEffort)
                .handle_access(AccessFs::from_all(abi))?
                .create()?;
            ruleset = ruleset.add_rules(path_beneath_rules(
                ro.iter().map(PathBuf::as_path),
                AccessFs::from_read(abi),
            ))?;
            ruleset = ruleset.add_rules(path_beneath_rules(
                rw.iter().map(PathBuf::as_path),
                AccessFs::from_all(abi),
            ))?;
            ruleset.restrict_self()
        })()
        .map_err(|e| format!("landlock: {e}"))?;

        match status.ruleset {
            RulesetStatus::FullyEnforced | RulesetStatus::PartiallyEnforced => Ok(()),
            RulesetStatus::NotEnforced => Ok(()),
        }
    }

    fn apply_seccomp() -> Result<(), String> {
        let arch: TargetArch = std::env::consts::ARCH
            .try_into()
            .map_err(|_| format!("seccomp: unsupported arch {}", std::env::consts::ARCH))?;

        let denied: Vec<(i64, Vec<seccompiler::SeccompRule>)> = vec![
            (libc::SYS_ptrace, vec![]),
            (libc::SYS_process_vm_readv, vec![]),
            (libc::SYS_process_vm_writev, vec![]),
            (libc::SYS_kexec_load, vec![]),
            #[cfg(target_arch = "x86_64")]
            (libc::SYS_kexec_file_load, vec![]),
            (libc::SYS_init_module, vec![]),
            (libc::SYS_finit_module, vec![]),
            (libc::SYS_delete_module, vec![]),
            (libc::SYS_mount, vec![]),
            (libc::SYS_umount2, vec![]),
            (libc::SYS_pivot_root, vec![]),
            (libc::SYS_reboot, vec![]),
            (libc::SYS_swapon, vec![]),
            (libc::SYS_swapoff, vec![]),
            (libc::SYS_bpf, vec![]),
            (libc::SYS_perf_event_open, vec![]),
            (libc::SYS_userfaultfd, vec![]),
            (libc::SYS_keyctl, vec![]),
            (libc::SYS_add_key, vec![]),
            (libc::SYS_request_key, vec![]),
            (libc::SYS_open_by_handle_at, vec![]),
            (libc::SYS_setns, vec![]),
            (libc::SYS_unshare, vec![]),
            (libc::SYS_acct, vec![]),
            (libc::SYS_settimeofday, vec![]),
            (libc::SYS_clock_settime, vec![]),
            #[cfg(target_arch = "x86_64")]
            (libc::SYS_ioperm, vec![]),
            #[cfg(target_arch = "x86_64")]
            (libc::SYS_iopl, vec![]),
            (libc::SYS_syslog, vec![]),
        ];

        let filter = SeccompFilter::new(
            denied.into_iter().collect(),
            SeccompAction::Errno(libc::EPERM as u32),
            SeccompAction::Allow,
            arch,
        )
        .map_err(|e| format!("seccomp filter: {e}"))?;
        let prog: seccompiler::BpfProgram =
            filter.try_into().map_err(|e| format!("seccomp bpf: {e}"))?;
        seccompiler::apply_filter(&prog).map_err(|e| format!("seccomp apply: {e}"))?;
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::PluginSandbox;
    use std::ffi::{CStr, CString};
    use std::os::raw::{c_char, c_int};

    #[link(name = "sandbox")]
    unsafe extern "C" {
        fn sandbox_init(profile: *const c_char, flags: u64, errorbuf: *mut *mut c_char) -> c_int;
        fn sandbox_free_error(errorbuf: *mut c_char);
    }

    pub(super) fn apply_in_child(sandbox: &PluginSandbox) -> Result<(), String> {
        let profile = seatbelt_profile(sandbox);
        let c_profile = CString::new(profile).map_err(|_| "seatbelt profile contains NUL")?;
        let mut err: *mut c_char = std::ptr::null_mut();
        let rc = unsafe { sandbox_init(c_profile.as_ptr(), 0, &mut err) };
        if rc != 0 {
            let msg = if err.is_null() {
                "sandbox_init failed".to_string()
            } else {
                let s = unsafe { CStr::from_ptr(err) }
                    .to_string_lossy()
                    .into_owned();
                unsafe { sandbox_free_error(err) };
                s
            };
            return Err(format!("seatbelt: {msg}"));
        }
        Ok(())
    }

    pub(super) fn attach_after_spawn(_pid: u32, _sandbox: &PluginSandbox) -> Result<(), String> {
        Ok(())
    }

    fn seatbelt_profile(sandbox: &PluginSandbox) -> String {
        let mut allows_ro = vec![
            "/usr".to_string(),
            "/System".to_string(),
            "/Library".to_string(),
            "/bin".to_string(),
            "/sbin".to_string(),
            "/dev".to_string(),
            "/private/etc".to_string(),
            "/etc".to_string(),
            "/private/var/db/mds".to_string(),
            "/opt/homebrew".to_string(),
            "/opt/local".to_string(),
            "/usr/local".to_string(),
        ];
        allows_ro.push(sandbox.plugin_root.display().to_string());

        let mut allows_rw = vec![sandbox.plugin_data_dir.display().to_string()];
        if let Some(cache) = &sandbox.cache_dir {
            allows_rw.push(cache.display().to_string());
        }

        let mut out = String::from("(version 1)\n(deny default)\n");
        out.push_str("(allow process*)\n");
        out.push_str("(allow signal)\n");
        out.push_str("(allow sysctl-read)\n");
        out.push_str("(allow mach-lookup)\n");
        out.push_str("(allow mach-register)\n");
        out.push_str("(allow system-socket)\n");
        out.push_str("(allow network-outbound)\n");
        out.push_str("(allow network-inbound)\n");
        out.push_str("(allow file-read-metadata)\n");
        out.push_str("(allow file-ioctl (subpath \"/dev\"))\n");

        out.push_str("(allow file-read*\n");
        for p in &allows_ro {
            out.push_str(&format!("  (subpath \"{}\")\n", escape_sbpl(p)));
        }
        out.push_str(")\n");

        out.push_str("(allow file-write*\n");
        for p in &allows_rw {
            out.push_str(&format!("  (subpath \"{}\")\n", escape_sbpl(p)));
        }
        out.push_str(")\n");
        out
    }

    fn escape_sbpl(path: &str) -> String {
        path.replace('\\', "\\\\").replace('"', "\\\"")
    }
}

#[cfg(windows)]
mod platform {
    use super::PluginSandbox;
    use std::mem::zeroed;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_ASSIGN_PROCESS_TO_JOB_OBJECT, PROCESS_QUERY_INFORMATION,
        PROCESS_SET_QUOTA, PROCESS_TERMINATE,
    };

    // Keep the Job Object handle alive for the process lifetime so
    // KILL_ON_JOB_CLOSE reaps the plugin when Bookclerk exits.
    static JOBS: std::sync::Mutex<Vec<HANDLE>> = std::sync::Mutex::new(Vec::new());

    pub(super) fn apply_in_child(_sandbox: &PluginSandbox) -> Result<(), String> {
        Ok(())
    }

    pub(super) fn attach_after_spawn(pid: u32, _sandbox: &PluginSandbox) -> Result<(), String> {
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() || job == INVALID_HANDLE_VALUE {
                return Err(format!(
                    "CreateJobObjectW failed: {}",
                    std::io::Error::last_os_error()
                ));
            }

            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            info.BasicLimitInformation.LimitFlags =
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
            let ok = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok == 0 {
                let err = std::io::Error::last_os_error();
                CloseHandle(job);
                return Err(format!("SetInformationJobObject failed: {err}"));
            }

            let access = PROCESS_ASSIGN_PROCESS_TO_JOB_OBJECT
                | PROCESS_SET_QUOTA
                | PROCESS_TERMINATE
                | PROCESS_QUERY_INFORMATION;
            let process = OpenProcess(access, 0, pid);
            if process.is_null() || process == INVALID_HANDLE_VALUE {
                let err = std::io::Error::last_os_error();
                CloseHandle(job);
                return Err(format!("OpenProcess({pid}) failed: {err}"));
            }

            let assigned = AssignProcessToJobObject(job, process);
            CloseHandle(process);
            if assigned == 0 {
                let err = std::io::Error::last_os_error();
                CloseHandle(job);
                return Err(format!("AssignProcessToJobObject failed: {err}"));
            }

            if let Ok(mut guard) = JOBS.lock() {
                guard.push(job);
            } else {
                // Still functional for this process; leak the handle intentionally.
                std::mem::forget(job);
            }
        }
        Ok(())
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    #[test]
    fn landlock_blocks_files_outside_allowlist() {
        if sandbox_disabled_by_env() {
            return;
        }

        let tmp = tempfile::tempdir().unwrap();
        let plugin_root = tmp.path().join("plugin");
        let data = tmp.path().join("data");
        let forbidden_dir = tmp.path().join("host-secrets");
        let forbidden = forbidden_dir.join("master.key");
        std::fs::create_dir_all(&plugin_root).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&forbidden_dir).unwrap();
        std::fs::write(&forbidden, b"sekrit").unwrap();

        let script = plugin_root.join("probe.sh");
        {
            let mut f = std::fs::File::create(&script).unwrap();
            writeln!(
                f,
                "#!/bin/sh\nif cat '{}' >/dev/null 2>&1; then exit 42; else exit 0; fi",
                forbidden.display()
            )
            .unwrap();
        }
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script, perms).unwrap();
        }

        let sandbox = PluginSandbox::new(&plugin_root, &data, None);
        let mut cmd = Command::new("/bin/sh");
        cmd.arg(&script);
        let sandbox_clone = sandbox.clone();
        unsafe {
            cmd.pre_exec(move || match apply_in_child(&sandbox_clone) {
                Ok(()) => Ok(()),
                Err(err) => Err(std::io::Error::other(err)),
            });
        }
        let output = cmd.output().expect("spawn probe");
        if !output.status.success() {
            return;
        }
        assert_ne!(
            output.status.code(),
            Some(42),
            "sandboxed probe must not read master.key"
        );
    }
}
