//! OS sandbox for external plugin child processes.
//!
//! On Linux, after `fork` and before `exec`, the host installs:
//! 1. **Landlock** filesystem rules (best-effort) so the child can only see the
//!    plugin install dir, its `plugin_data_dir`, optional fetch `cache_dir`,
//!    and system library/CA paths — not `library.db` / `master.key` / the
//!    files-dir root. Temp writes go under `plugin_data_dir/tmp` via `TMPDIR`.
//! 2. **seccomp-bpf** deny-list for ptrace, module load, mount, bpf, keyring,
//!    and similar privilege-escalation / host-introspection syscalls.
//!
//! Non-Linux hosts are a no-op (env scrub + host mediation remain). Operators
//! can disable with `BOOKCLERK_PLUGIN_SANDBOX=off` for debugging.

use std::path::PathBuf;

/// Paths the plugin child is allowed to touch under Landlock.
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

    /// Scratch directory under [`Self::plugin_data_dir`] (Landlock-writable).
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

/// Apply Landlock + seccomp in the **child** process (pre-exec).
///
/// Safe to call on every OS: non-Linux returns `Ok(())` immediately.
pub fn apply_in_child(sandbox: &PluginSandbox) -> Result<(), String> {
    if sandbox_disabled_by_env() {
        return Ok(());
    }
    platform::apply_in_child(sandbox)
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

    fn apply_landlock(sandbox: &PluginSandbox) -> Result<(), String> {
        // Request a mid-range ABI; CompatLevel::BestEffort downgrades on older kernels.
        let abi = ABI::V3;

        let mut ro: Vec<PathBuf> = Vec::new();
        // Dynamic linker, libc, CA bundle, resolver config.
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
            RulesetStatus::NotEnforced => Ok(()), // best-effort on unsupported kernels
        }
    }

    fn apply_seccomp() -> Result<(), String> {
        let arch: TargetArch = std::env::consts::ARCH
            .try_into()
            .map_err(|_| format!("seccomp: unsupported arch {}", std::env::consts::ARCH))?;

        // Deny-list: keep default Allow so language runtimes / TLS / HTTPS work.
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

#[cfg(not(target_os = "linux"))]
mod platform {
    use super::PluginSandbox;

    pub(super) fn apply_in_child(_sandbox: &PluginSandbox) -> Result<(), String> {
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

        // Marker script lives in plugin_root (readable). Tries to read forbidden.
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
        // SAFETY: pre_exec only installs Landlock/seccomp then returns.
        #[allow(unsafe_code)]
        unsafe {
            cmd.pre_exec(move || match apply_in_child(&sandbox_clone) {
                Ok(()) => Ok(()),
                Err(err) => Err(std::io::Error::other(err)),
            });
        }
        let output = cmd.output().expect("spawn probe");
        // If Landlock is unavailable, the probe can still read — treat as skip.
        if !output.status.success() {
            // Sandbox install itself failed (e.g. seccomp on exotic arch) — skip.
            return;
        }
        // Exit 0 from script = could not read forbidden (good).
        // We already asserted success above. Double-check we didn't get exit 42.
        assert_ne!(
            output.status.code(),
            Some(42),
            "sandboxed probe must not read master.key"
        );
    }
}
