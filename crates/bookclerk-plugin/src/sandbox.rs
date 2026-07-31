//! OS sandbox for external plugin child processes (fail-closed).
//!
//! After fork/`CreateProcess` and before the plugin runs untrusted code, the
//! host installs a platform jail. If the OS cannot enforce the jail, spawn
//! **fails** unless `BOOKCLERK_PLUGIN_SANDBOX=off`.
//!
//! | OS | Mechanism |
//! | --- | --- |
//! | Linux | Landlock FS rules + seccomp-bpf deny-list |
//! | macOS | Seatbelt (`sandbox_init`) profile |
//! | Windows | AppContainer at process creation (+ Job Object kill-on-close) |
//!
//! Host mediation still applies: no `library.db` / `master.key` paths, env scrub,
//! `TMPDIR` under `plugin_data_dir/tmp`, per-plugin cache only.

#![allow(unsafe_code)] // pre_exec / Seatbelt / AppContainer FFI

use std::path::PathBuf;

/// Paths the plugin child is allowed to touch under the FS sandbox.
#[derive(Debug, Clone)]
pub struct PluginSandbox {
    /// Plugin id (AppContainer profile name / logging).
    pub plugin_id: String,
    /// Plugin install directory (`plugin.toml` + binary) — read/execute.
    pub plugin_root: PathBuf,
    /// Per-plugin data directory (`…/plugins/<id>/data`) — read/write.
    pub plugin_data_dir: PathBuf,
    /// Per-plugin fetch cache (`…/cache/plugins/<id>`) — read/write when set.
    pub cache_dir: Option<PathBuf>,
}

impl PluginSandbox {
    /// Build a sandbox policy for a discovered plugin install.
    #[must_use]
    pub fn new(
        plugin_id: impl Into<String>,
        plugin_root: impl Into<PathBuf>,
        plugin_data_dir: impl Into<PathBuf>,
        cache_dir: Option<PathBuf>,
    ) -> Self {
        Self {
            plugin_id: plugin_id.into(),
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

/// Whether the operator disabled the OS sandbox via env (explicit opt-out).
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
/// Fail-closed: returns `Err` when the jail cannot be enforced.
/// On Windows the jail is applied at `CreateProcess` time instead.
#[cfg_attr(windows, allow(dead_code))]
pub fn apply_in_child(sandbox: &PluginSandbox) -> Result<(), String> {
    if sandbox_disabled_by_env() {
        return Ok(());
    }
    platform::apply_in_child(sandbox)
}

/// Parent-side sandbox attach after spawn.
///
/// - Unix: no-op (child already sandboxed in `pre_exec`).
/// - Windows: assigns the child to a Job Object after AppContainer creation.
pub fn attach_after_spawn(child_pid: u32, sandbox: &PluginSandbox) -> Result<(), String> {
    if sandbox_disabled_by_env() {
        return Ok(());
    }
    platform::attach_after_spawn(child_pid, sandbox)
}

/// Ensure sandbox writable roots exist before spawn.
pub fn prepare_dirs(sandbox: &PluginSandbox) -> std::io::Result<()> {
    std::fs::create_dir_all(&sandbox.plugin_data_dir)?;
    std::fs::create_dir_all(sandbox.tmp_dir())?;
    if let Some(cache) = &sandbox.cache_dir {
        std::fs::create_dir_all(cache)?;
    }
    Ok(())
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
        // Intentionally omit broad `/proc` — same-uid plugins must not read
        // `/proc/<daemon>/environ` / fds. Allow only `/proc/self` (+ cpu info).
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
            "/proc/self",
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

        // HardRequirement: fail closed when Landlock cannot enforce.
        let status = (|| {
            let mut ruleset = Ruleset::default()
                .set_compatibility(CompatLevel::HardRequirement)
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
        .map_err(|e| {
            format!(
                "landlock failed (fail-closed; set BOOKCLERK_PLUGIN_SANDBOX=off to disable): {e}"
            )
        })?;

        match status.ruleset {
            RulesetStatus::FullyEnforced | RulesetStatus::PartiallyEnforced => Ok(()),
            RulesetStatus::NotEnforced => {
                Err("landlock NotEnforced (kernel/container lacks Landlock); \
                 refusing to spawn plugin without FS jail \
                 (BOOKCLERK_PLUGIN_SANDBOX=off to override)"
                    .into())
            }
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
        seccompiler::apply_filter(&prog)
            .map_err(|e| format!("seccomp apply failed (fail-closed): {e}"))?;
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
        let c_profile =
            CString::new(profile).map_err(|_| "seatbelt profile contains NUL".to_string())?;
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
            return Err(format!(
                "seatbelt failed (fail-closed; BOOKCLERK_PLUGIN_SANDBOX=off to override): {msg}"
            ));
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
            "/bin".to_string(),
            "/sbin".to_string(),
            "/dev".to_string(),
            "/private/etc".to_string(),
            "/etc".to_string(),
            "/private/var/db/mds".to_string(),
            "/opt/homebrew".to_string(),
            "/usr/local".to_string(),
        ];
        allows_ro.push(sandbox.plugin_root.display().to_string());

        let mut allows_rw = vec![sandbox.plugin_data_dir.display().to_string()];
        if let Some(cache) = &sandbox.cache_dir {
            allows_rw.push(cache.display().to_string());
        }

        let mut out = String::from("(version 1)\n(deny default)\n");
        out.push_str("(allow process-exec)\n");
        out.push_str("(allow process-fork)\n");
        out.push_str("(allow signal (target self))\n");
        out.push_str("(allow sysctl-read)\n");
        out.push_str("(allow mach-lookup)\n");
        out.push_str("(allow system-socket)\n");
        out.push_str("(allow network-outbound)\n");
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
        path.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace(['\n', '\r', '(', ')'], "")
    }
}

#[cfg(windows)]
mod platform {
    //! Windows: AppContainer at CreateProcess + Job Object (fail-closed).

    use super::PluginSandbox;
    use std::ffi::OsStr;
    use std::mem::zeroed;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{FromRawHandle, OwnedHandle, RawHandle};
    use std::path::Path;
    use std::ptr;
    use windows_sys::Win32::Foundation::{
        CloseHandle, LocalFree, SetHandleInformation, ERROR_ALREADY_EXISTS, FALSE, HANDLE,
        HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, TRUE,
    };
    use windows_sys::Win32::Security::Authorization::{
        GetNamedSecurityInfoW, SetEntriesInAclW, SetNamedSecurityInfoW, EXPLICIT_ACCESS_W,
        GRANT_ACCESS, SE_FILE_OBJECT, TRUSTEE_IS_SID, TRUSTEE_IS_USER,
    };
    use windows_sys::Win32::Security::Isolation::{
        CreateAppContainerProfile, DeriveAppContainerSidFromAppContainerName,
    };
    use windows_sys::Win32::Security::{
        ACL, CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION, OBJECT_INHERIT_ACE, PSID,
        SECURITY_ATTRIBUTES, SECURITY_CAPABILITIES,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_TRAVERSE,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Pipes::CreatePipe;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, DeleteProcThreadAttributeList, InitializeProcThreadAttributeList,
        TerminateProcess, UpdateProcThreadAttribute, CREATE_UNICODE_ENVIRONMENT,
        EXTENDED_STARTUPINFO_PRESENT, PROCESS_INFORMATION,
        PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, STARTF_USESTDHANDLES, STARTUPINFOEXW,
    };

    /// Win32 `PROCESS_ASSIGN_PROCESS_TO_JOB_OBJECT` (not yet in windows-sys 0.61).
    const PROCESS_ASSIGN_PROCESS_TO_JOB_OBJECT: u32 = 0x0005;

    #[allow(dead_code)] // child-side hook unused; AppContainer is parent-side.
    pub(super) fn apply_in_child(_sandbox: &PluginSandbox) -> Result<(), String> {
        Ok(())
    }

    #[allow(dead_code)] // Job lifetime owned by [`AppContainerChild`].
    pub(super) fn attach_after_spawn(_pid: u32, _sandbox: &PluginSandbox) -> Result<(), String> {
        Ok(())
    }

    /// Spawn `command` inside an AppContainer with pipes for stdin/stdout.
    pub fn spawn_appcontainer(
        sandbox: &PluginSandbox,
        command: &Path,
        args: &[String],
        cwd: &Path,
        env_pairs: &[(String, String)],
    ) -> Result<AppContainerChild, String> {
        prepare_profile_and_acls(sandbox)?;

        let name = appcontainer_name(&sandbox.plugin_id);
        let name_w = wide(&name);
        let mut sid: PSID = ptr::null_mut();
        let hr = unsafe { DeriveAppContainerSidFromAppContainerName(name_w.as_ptr(), &mut sid) };
        if hr != 0 || sid.is_null() {
            return Err(format!(
                "DeriveAppContainerSidFromAppContainerName failed: hr={hr:#x}"
            ));
        }

        let result = unsafe { spawn_with_sid(sandbox, command, args, cwd, env_pairs, sid) };
        unsafe {
            LocalFree(sid.cast());
        }
        result
    }

    unsafe fn spawn_with_sid(
        sandbox: &PluginSandbox,
        command: &Path,
        args: &[String],
        cwd: &Path,
        env_pairs: &[(String, String)],
        sid: PSID,
    ) -> Result<AppContainerChild, String> {
        let mut sa: SECURITY_ATTRIBUTES = zeroed();
        sa.nLength = std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32;
        sa.bInheritHandle = TRUE;

        let mut stdin_r: HANDLE = ptr::null_mut();
        let mut stdin_w: HANDLE = ptr::null_mut();
        let mut stdout_r: HANDLE = ptr::null_mut();
        let mut stdout_w: HANDLE = ptr::null_mut();
        if CreatePipe(&mut stdin_r, &mut stdin_w, &sa, 0) == 0
            || CreatePipe(&mut stdout_r, &mut stdout_w, &sa, 0) == 0
        {
            return Err(format!(
                "CreatePipe failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        // Parent keeps stdin_w / stdout_r — clear inherit on those.
        SetHandleInformation(stdin_w, HANDLE_FLAG_INHERIT, 0);
        SetHandleInformation(stdout_r, HANDLE_FLAG_INHERIT, 0);

        let mut caps: SECURITY_CAPABILITIES = zeroed();
        caps.AppContainerSid = sid;

        let mut attr_size: usize = 0;
        InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut attr_size);
        let mut attr_buf = vec![0u8; attr_size];
        let attr_list = attr_buf.as_mut_ptr().cast();
        if InitializeProcThreadAttributeList(attr_list, 1, 0, &mut attr_size) == 0 {
            let err = std::io::Error::last_os_error();
            CloseHandle(stdin_r);
            CloseHandle(stdin_w);
            CloseHandle(stdout_r);
            CloseHandle(stdout_w);
            return Err(format!("InitializeProcThreadAttributeList failed: {err}"));
        }

        let ok = UpdateProcThreadAttribute(
            attr_list,
            0,
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
            (&raw const caps).cast(),
            std::mem::size_of::<SECURITY_CAPABILITIES>(),
            ptr::null_mut(),
            ptr::null(),
        );
        if ok == 0 {
            let err = std::io::Error::last_os_error();
            DeleteProcThreadAttributeList(attr_list);
            CloseHandle(stdin_r);
            CloseHandle(stdin_w);
            CloseHandle(stdout_r);
            CloseHandle(stdout_w);
            return Err(format!(
                "UpdateProcThreadAttribute(SECURITY_CAPABILITIES) failed: {err}"
            ));
        }

        let mut siex: STARTUPINFOEXW = zeroed();
        siex.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        siex.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        siex.StartupInfo.hStdInput = stdin_r;
        siex.StartupInfo.hStdOutput = stdout_w;
        siex.StartupInfo.hStdError = stdout_w;
        siex.lpAttributeList = attr_list;

        let mut cmdline = wide(&build_command_line(command, args));
        let cwd_w = wide_path(cwd);
        let env_block = build_env_block(env_pairs);
        let mut pi: PROCESS_INFORMATION = zeroed();

        let created = CreateProcessW(
            ptr::null(),
            cmdline.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            TRUE,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
            env_block.as_ptr().cast(),
            cwd_w.as_ptr(),
            (&raw const siex.StartupInfo).cast(),
            &mut pi,
        );

        DeleteProcThreadAttributeList(attr_list);
        CloseHandle(stdin_r);
        CloseHandle(stdout_w);

        if created == 0 {
            let err = std::io::Error::last_os_error();
            CloseHandle(stdin_w);
            CloseHandle(stdout_r);
            return Err(format!(
                "CreateProcessW(AppContainer) failed for plugin `{}`: {err} \
                 (fail-closed; BOOKCLERK_PLUGIN_SANDBOX=off to override)",
                sandbox.plugin_id
            ));
        }

        CloseHandle(pi.hThread);
        let pid = pi.dwProcessId;
        let job = match create_kill_on_close_job(pid) {
            Ok(job) => job,
            Err(err) => {
                TerminateProcess(pi.hProcess, 1);
                CloseHandle(pi.hProcess);
                CloseHandle(stdin_w);
                CloseHandle(stdout_r);
                return Err(err);
            }
        };

        let stdin = std::fs::File::from_raw_handle(stdin_w as RawHandle);
        let stdout = std::fs::File::from_raw_handle(stdout_r as RawHandle);
        let process = OwnedHandle::from_raw_handle(pi.hProcess as RawHandle);

        Ok(AppContainerChild {
            pid,
            process,
            job,
            stdin: Some(stdin),
            stdout: Some(stdout),
        })
    }

    fn prepare_profile_and_acls(sandbox: &PluginSandbox) -> Result<(), String> {
        let sid = ensure_appcontainer_profile(&sandbox.plugin_id)?;
        let grant = grant_path_access(sandbox, sid);
        unsafe {
            LocalFree(sid.cast());
        }
        grant
    }

    fn ensure_appcontainer_profile(plugin_id: &str) -> Result<PSID, String> {
        let name = appcontainer_name(plugin_id);
        let name_w = wide(&name);
        let display = wide(&format!("Bookclerk plugin {plugin_id}"));
        let desc = wide("Bookclerk external plugin AppContainer");

        unsafe {
            let mut sid: PSID = ptr::null_mut();
            let hr = CreateAppContainerProfile(
                name_w.as_ptr(),
                display.as_ptr(),
                desc.as_ptr(),
                ptr::null(),
                0,
                &mut sid,
            );
            if hr == 0 || hr == hresult_from_win32(ERROR_ALREADY_EXISTS) {
                if !sid.is_null() {
                    return Ok(sid);
                }
                let mut derived: PSID = ptr::null_mut();
                let hr2 = DeriveAppContainerSidFromAppContainerName(name_w.as_ptr(), &mut derived);
                if hr2 == 0 && !derived.is_null() {
                    return Ok(derived);
                }
                return Err(format!(
                    "DeriveAppContainerSidFromAppContainerName failed: hr={hr2:#x}"
                ));
            }
            Err(format!(
                "CreateAppContainerProfile failed: hr={hr:#x} \
                 (fail-closed; BOOKCLERK_PLUGIN_SANDBOX=off to override)"
            ))
        }
    }

    fn grant_path_access(sandbox: &PluginSandbox, sid: PSID) -> Result<(), String> {
        // Install dir: read + traverse only.
        grant_ace(&sandbox.plugin_root, sid, false)?;
        grant_ace(&sandbox.plugin_data_dir, sid, true)?;
        if let Some(c) = &sandbox.cache_dir {
            grant_ace(c, sid, true)?;
        }
        Ok(())
    }

    fn grant_ace(path: &Path, sid: PSID, writable: bool) -> Result<(), String> {
        if !path.exists() {
            return Ok(());
        }
        let mut path_w = wide_path(path);
        let access = if writable {
            FILE_GENERIC_READ | FILE_GENERIC_WRITE | FILE_TRAVERSE
        } else {
            FILE_GENERIC_READ | FILE_TRAVERSE
        };

        unsafe {
            let mut ea: EXPLICIT_ACCESS_W = zeroed();
            ea.grfAccessPermissions = access;
            ea.grfAccessMode = GRANT_ACCESS;
            ea.grfInheritance = OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE;
            ea.Trustee.TrusteeForm = TRUSTEE_IS_SID;
            ea.Trustee.TrusteeType = TRUSTEE_IS_USER;
            ea.Trustee.ptstrName = sid.cast();

            let mut old_sd: windows_sys::Win32::Security::PSECURITY_DESCRIPTOR = ptr::null_mut();
            let mut old_dacl: *mut ACL = ptr::null_mut();
            let g = GetNamedSecurityInfoW(
                path_w.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut old_dacl,
                ptr::null_mut(),
                &mut old_sd,
            );
            if g != 0 {
                return Err(format!(
                    "GetNamedSecurityInfoW({}) failed: {g}",
                    path.display()
                ));
            }

            let mut new_dacl: *mut ACL = ptr::null_mut();
            let s = SetEntriesInAclW(1, &ea, old_dacl, &mut new_dacl);
            if s != 0 {
                LocalFree(old_sd);
                return Err(format!("SetEntriesInAclW failed: {s}"));
            }
            let set = SetNamedSecurityInfoW(
                path_w.as_mut_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                new_dacl,
                ptr::null_mut(),
            );
            LocalFree(new_dacl.cast());
            LocalFree(old_sd);
            if set != 0 {
                return Err(format!(
                    "SetNamedSecurityInfoW({}) failed: {set}",
                    path.display()
                ));
            }
        }
        Ok(())
    }

    /// Create a kill-on-close Job Object and assign `pid`. Caller owns the handle
    /// (dropping it terminates the plugin tree).
    fn create_kill_on_close_job(pid: u32) -> Result<OwnedHandle, String> {
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
        };

        unsafe {
            let job = CreateJobObjectW(ptr::null(), ptr::null());
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
            let process = OpenProcess(access, FALSE, pid);
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

            Ok(OwnedHandle::from_raw_handle(job as RawHandle))
        }
    }

    fn appcontainer_name(plugin_id: &str) -> String {
        let safe: String = plugin_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' {
                    c
                } else {
                    '.'
                }
            })
            .collect();
        format!("Bookclerk.Plugin.{safe}")
    }

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    fn wide_path(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    const fn hresult_from_win32(x: u32) -> i32 {
        if x as i32 <= 0 {
            x as i32
        } else {
            ((x & 0x0000_FFFF) | 0x8007_0000) as i32
        }
    }

    fn build_command_line(command: &Path, args: &[String]) -> String {
        let mut parts = Vec::with_capacity(1 + args.len());
        parts.push(quote_win_arg(&command.display().to_string()));
        for a in args {
            parts.push(quote_win_arg(a));
        }
        parts.join(" ")
    }

    fn quote_win_arg(arg: &str) -> String {
        if arg.is_empty() {
            return "\"\"".into();
        }
        let needs = arg.chars().any(|c| c == ' ' || c == '\t' || c == '"');
        if !needs {
            return arg.to_string();
        }
        let mut out = String::from("\"");
        let mut backslashes = 0u32;
        for c in arg.chars() {
            match c {
                '\\' => backslashes += 1,
                '"' => {
                    out.push_str(&"\\".repeat((backslashes * 2 + 1) as usize));
                    out.push('"');
                    backslashes = 0;
                }
                _ => {
                    if backslashes > 0 {
                        out.push_str(&"\\".repeat(backslashes as usize));
                        backslashes = 0;
                    }
                    out.push(c);
                }
            }
        }
        if backslashes > 0 {
            out.push_str(&"\\".repeat((backslashes * 2) as usize));
        }
        out.push('"');
        out
    }

    fn build_env_block(pairs: &[(String, String)]) -> Vec<u16> {
        let mut block: Vec<u16> = Vec::new();
        for (k, v) in pairs {
            for c in format!("{k}={v}").encode_utf16() {
                block.push(c);
            }
            block.push(0);
        }
        block.push(0);
        block
    }

    /// Child process created inside an AppContainer.
    ///
    /// Holds the kill-on-close Job Object; dropping this child closes the job and
    /// terminates the plugin process tree.
    pub struct AppContainerChild {
        #[allow(dead_code)] // retained for diagnostics / future wait APIs
        pub pid: u32,
        pub process: OwnedHandle,
        /// Kill-on-close job; closed on drop (field unread by design).
        #[allow(dead_code)]
        pub job: OwnedHandle,
        pub stdin: Option<std::fs::File>,
        pub stdout: Option<std::fs::File>,
    }

    impl AppContainerChild {
        pub fn start_kill(&self) {
            unsafe {
                TerminateProcess(self.process.as_raw_handle() as HANDLE, 1);
            }
        }

        pub fn take_stdio(&mut self) -> Option<(std::fs::File, std::fs::File)> {
            let stdin = self.stdin.take()?;
            let stdout = self.stdout.take()?;
            Some((stdin, stdout))
        }
    }

    use std::os::windows::io::AsRawHandle;
}

#[cfg(windows)]
pub use platform::{spawn_appcontainer as windows_spawn_appcontainer, AppContainerChild};

/// Fallback platform (unsupported) — fail closed.
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod platform {
    use super::PluginSandbox;

    pub(super) fn apply_in_child(_sandbox: &PluginSandbox) -> Result<(), String> {
        Err(
            "plugin OS sandbox is not supported on this platform (fail-closed); \
             set BOOKCLERK_PLUGIN_SANDBOX=off to override"
                .into(),
        )
    }

    pub(super) fn attach_after_spawn(_pid: u32, _sandbox: &PluginSandbox) -> Result<(), String> {
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

        let sandbox = PluginSandbox::new("probe", &plugin_root, &data, None);
        let mut cmd = Command::new("/bin/sh");
        cmd.arg(&script);
        let sandbox_clone = sandbox.clone();
        unsafe {
            cmd.pre_exec(move || match apply_in_child(&sandbox_clone) {
                Ok(()) => Ok(()),
                Err(err) => Err(std::io::Error::other(err)),
            });
        }
        let output = match cmd.output() {
            Ok(o) => o,
            Err(err) => panic!("spawn probe: {err}"),
        };

        // Exit 42 means the probe read master.key — jail failed. Always fail.
        assert_ne!(
            output.status.code(),
            Some(42),
            "sandboxed probe must never exit 42 (read master.key); jail failed open"
        );

        if output.status.success() {
            // Jail enforced: cat denied → exit 0.
            assert_eq!(output.status.code(), Some(0));
        }
        // Else: pre_exec failed (Landlock/seccomp unavailable) — fail-closed is OK;
        // the child must not have run unconfined (would be exit 42 above).
    }

    #[test]
    fn landlock_unconfined_read_would_be_exit_42() {
        // Control: without sandbox, the probe can read the file → exit 42.
        let tmp = tempfile::tempdir().unwrap();
        let plugin_root = tmp.path().join("plugin");
        let forbidden_dir = tmp.path().join("host-secrets");
        let forbidden = forbidden_dir.join("master.key");
        std::fs::create_dir_all(&plugin_root).unwrap();
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
        let output = Command::new("/bin/sh").arg(&script).output().unwrap();
        assert_eq!(output.status.code(), Some(42));
    }
}
