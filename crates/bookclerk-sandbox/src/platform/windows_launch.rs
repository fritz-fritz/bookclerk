//! Bookclerk-owned AppContainer process launch with correct Job Object ordering.
//!
//! rappct 0.13.3 runs `CreateProcessW` then `AssignProcessToJobObject` while the
//! primary thread is already runnable, and does not `TerminateProcess` on later
//! failures. This module owns CreateProcess for production jail launches:
//!
//! 1. Prefer `PROC_THREAD_ATTRIBUTE_JOB_LIST` so assignment happens before any
//!    guest instruction runs (Windows 10+).
//! 2. Fall back to `CREATE_SUSPENDED` → configure Job → `AssignProcessToJobObject`
//!    → `ResumeThread`.
//! 3. On every failure after a successful `CreateProcessW`, terminate the child
//!    and close process, thread, pipe, and Job handles.
//!
//! Profile / capability SID derivation still uses rappct helpers.

#![cfg(windows)]
#![allow(unsafe_code)]

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::{Path, PathBuf};
use std::ptr;
use std::time::Duration;

use rappct::sid::SidAndAttributes;
use rappct::SecurityCapabilities;
use windows::core::{BOOL, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, LocalFree, SetHandleInformation, HANDLE, HANDLE_FLAGS, HANDLE_FLAG_INHERIT,
    HLOCAL, TRUE, WAIT_FAILED, WAIT_TIMEOUT,
};
use windows::Win32::Security::Authorization::ConvertStringSidToSidW;
use windows::Win32::Security::{
    PSID, SECURITY_ATTRIBUTES, SECURITY_CAPABILITIES, SID_AND_ATTRIBUTES,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob, JobObjectCpuRateControlInformation,
    JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
    JOBOBJECT_CPU_RATE_CONTROL_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_CPU_RATE_CONTROL_ENABLE, JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
    JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_JOB_MEMORY,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, ResumeThread, TerminateProcess, UpdateProcThreadAttribute,
    WaitForSingleObject, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
    EXTENDED_STARTUPINFO_PRESENT, INFINITE, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION,
    PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_JOB_LIST,
    PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, STARTF_USESTDHANDLES, STARTUPINFOEXW,
};

use crate::SandboxError;

/// `SE_GROUP_ENABLED` attribute applied to AppContainer capability SIDs.
const SE_GROUP_ENABLED: u32 = 0x0000_0004;

/// Limits applied to the kill-on-close Job Object that owns the guest tree.
#[derive(Debug, Clone, Copy, Default)]
pub struct JobResourceLimits {
    /// Maximum committed memory for the Job Object, when set.
    pub memory_bytes: Option<usize>,
    /// CPU rate cap as a percent of one logical processor, when set.
    pub cpu_rate_percent: Option<u32>,
    /// Maximum active processes in the Job Object, when set.
    pub active_processes: Option<u32>,
}

/// Inputs for an AppContainer CreateProcess launch.
#[derive(Debug)]
pub struct LaunchRequest<'a> {
    /// Guest executable path passed as `lpApplicationName`.
    pub exe: &'a Path,
    /// Full `lpCommandLine` including quoted argv[0].
    pub cmdline: String,
    /// Working directory for CreateProcess.
    pub cwd: PathBuf,
    /// Child environment block entries as key/value pairs.
    pub env: Vec<(OsString, OsString)>,
    /// AppContainer security capabilities. `None` launches without a container
    /// (Windows `Isolation::Off` still uses the jail for handle inheritance).
    pub sec: Option<&'a SecurityCapabilities>,
    /// Resource limits applied to the kill-on-close Job Object.
    pub job: JobResourceLimits,
    /// Extra inheritable handles (already duplicated into this process).
    pub extra_handles: Vec<HANDLE>,
    /// Guest stdin handle; `None` creates a pipe and returns the parent write end.
    pub explicit_stdin: Option<HANDLE>,
    /// Guest stdout handle; `None` creates a pipe and returns the parent read end.
    pub explicit_stdout: Option<HANDLE>,
}

/// Reconstruct a [`HANDLE`] from a [`crate::JailHandoff`] integer.
#[must_use]
pub fn handle_from_u64(value: u64) -> HANDLE {
    HANDLE(value as usize as *mut std::ffi::c_void)
}

/// A running AppContainer guest with proxied stdio and a kill-on-close Job.
#[derive(Debug)]
pub struct LaunchedGuest {
    /// Process ID of the primary guest process.
    #[allow(dead_code)]
    pub pid: u32,
    /// Parent write end of the guest stdin pipe.
    pub stdin: Option<File>,
    /// Parent read end of the guest stdout pipe.
    pub stdout: Option<File>,
    /// Parent read end of the guest stderr pipe.
    pub stderr: Option<File>,
    /// Primary process handle kept until drop or [`Self::wait`].
    process: HANDLE,
    /// Kill-on-close Job Object owning the guest tree.
    job: HANDLE,
}

// Process/job handles are consumed by Windows integration tests and diagnostics.
#[allow(dead_code)]
impl LaunchedGuest {
    /// Process handle for diagnostics / tests (do not close).
    #[must_use]
    pub fn process_handle(&self) -> HANDLE {
        self.process
    }

    /// Job Object handle for diagnostics / tests (do not close).
    #[must_use]
    pub fn job_handle(&self) -> HANDLE {
        self.job
    }

    /// Whether `process` is a member of this launch's Job Object.
    pub fn contains_process(&self, process: HANDLE) -> Result<bool, SandboxError> {
        let mut inside = BOOL(0);
        unsafe {
            IsProcessInJob(process, Some(self.job), &mut inside)
                .map_err(|err| launch_err("IsProcessInJob", &err.to_string()))?;
        }
        Ok(inside.as_bool())
    }
}

impl LaunchedGuest {
    /// Wait for the primary process to exit. Dropping this value closes the Job
    /// handle (kill-on-close), terminating any remaining descendants.
    pub fn wait(self, timeout: Option<Duration>) -> Result<u32, SandboxError> {
        let ms = timeout
            .map(|d| d.as_millis().min(u128::from(u32::MAX)) as u32)
            .unwrap_or(INFINITE);
        unsafe {
            let r = WaitForSingleObject(self.process, ms);
            if r == WAIT_FAILED {
                return Err(launch_err("WaitForSingleObject", "process wait failed"));
            }
            if r == WAIT_TIMEOUT {
                return Err(launch_err("wait", "timeout waiting for AppContainer guest"));
            }
            let mut code = 0u32;
            GetExitCodeProcess(self.process, &mut code)
                .map_err(|err| launch_err("GetExitCodeProcess", &err.to_string()))?;
            Ok(code)
        }
    }
}

impl Drop for LaunchedGuest {
    fn drop(&mut self) {
        unsafe {
            if !self.process.is_invalid() {
                let _ = CloseHandle(self.process);
                self.process = HANDLE::default();
            }
            // Closing the job with KILL_ON_JOB_CLOSE terminates the tree.
            if !self.job.is_invalid() {
                let _ = CloseHandle(self.job);
                self.job = HANDLE::default();
            }
            // Stdio Files drop normally.
        }
    }
}

/// Launch `request` inside an AppContainer and a kill-on-close Job Object.
pub fn launch_appcontainer_guest(
    request: LaunchRequest<'_>,
) -> Result<LaunchedGuest, SandboxError> {
    unsafe { launch_impl(request) }
}

/// Child of [`spawn_with_handle_list`].
///
/// Drop closes the kill-on-close Job and terminates the process tree.
pub struct HandleListChild {
    /// Running guest. Drop closes the Job and kills the tree.
    inner: LaunchedGuest,
}

impl HandleListChild {
    /// Parent write end when stdin was not an explicit inherited handle.
    pub fn take_stdin(&mut self) -> Option<File> {
        self.inner.stdin.take()
    }

    /// Parent read end when stdout was not an explicit inherited handle.
    pub fn take_stdout(&mut self) -> Option<File> {
        self.inner.stdout.take()
    }

    /// Parent read end of the stderr pipe.
    pub fn take_stderr(&mut self) -> Option<File> {
        self.inner.stderr.take()
    }
}

/// Spawn `exe` with no AppContainer, inheriting only the listed handles.
///
/// Stdio handles and `extra` are placed on `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`.
/// This is the stable replacement for `CommandExt::inherit_handles`, which does
/// not inherit every inheritable handle in the parent. `stdin`, `stdout`, and
/// `extra` are closed in this process after the child inherits them.
///
/// # Errors
///
/// Returns [`SandboxError::Backend`] when CreateProcess fails.
pub fn spawn_with_handle_list(
    exe: &Path,
    cwd: &Path,
    env: Vec<(OsString, OsString)>,
    stdin: Option<OwnedHandle>,
    stdout: Option<OwnedHandle>,
    extra: Vec<OwnedHandle>,
) -> Result<HandleListChild, SandboxError> {
    let explicit_stdin = stdin.map(forget_raw);
    let explicit_stdout = stdout.map(forget_raw);
    let extra_handles = extra.into_iter().map(forget_raw).collect();
    let guest = launch_appcontainer_guest(LaunchRequest {
        exe,
        cmdline: quote_exe(exe),
        cwd: cwd.to_path_buf(),
        env,
        sec: None,
        job: JobResourceLimits {
            memory_bytes: None,
            cpu_rate_percent: None,
            active_processes: None,
        },
        extra_handles,
        explicit_stdin,
        explicit_stdout,
    })?;
    Ok(HandleListChild { inner: guest })
}

/// Drop Rust ownership so CreateProcess can close the raw handle itself.
fn forget_raw(handle: OwnedHandle) -> HANDLE {
    let raw = handle.as_raw_handle();
    std::mem::forget(handle);
    HANDLE(raw)
}

/// Quote `exe` for `lpCommandLine` when the path contains spaces or quotes.
fn quote_exe(exe: &Path) -> String {
    let text = exe.as_os_str().to_string_lossy();
    if text.chars().any(|ch| ch == ' ' || ch == '\t' || ch == '"') {
        format!("\"{text}\"")
    } else {
        text.into_owned()
    }
}

/// Creates an AppContainer child with stdio pipes and Job Object membership.
///
/// # Safety
///
/// Caller must ensure `request` paths and Win32 inputs are valid for CreateProcess.
unsafe fn launch_impl(request: LaunchRequest<'_>) -> Result<LaunchedGuest, SandboxError> {
    let owned_caps = match request.sec {
        Some(sec) => Some(OwnedSecurityCapabilities::from_rappct(sec)?),
        None => None,
    };

    let mut sa = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: ptr::null_mut(),
        bInheritHandle: TRUE,
    };

    let (child_stdin, parent_stdin_raw) = match request.explicit_stdin {
        Some(h) => (h, None),
        None => {
            let (read, write) = create_pipe_pair(&mut sa)?;
            (read, Some(write))
        }
    };
    let (parent_stdout_raw, child_stdout) = match request.explicit_stdout {
        Some(h) => (None, h),
        None => {
            let (read, write) = create_pipe_pair(&mut sa)?;
            (Some(read), write)
        }
    };
    let (parent_stderr_raw, child_stderr) = create_pipe_pair(&mut sa)?;

    let close_stdio = || {
        cleanup_handles(&[child_stdin, child_stdout, child_stderr]);
        cleanup_optional(&[parent_stdin_raw, parent_stdout_raw, Some(parent_stderr_raw)]);
    };

    // Parent pipe ends stay non-inheritable. Extra / explicit child ends
    // must be inheritable so HANDLE_LIST can pass them.
    for h in [parent_stdin_raw, parent_stdout_raw, Some(parent_stderr_raw)]
        .into_iter()
        .flatten()
    {
        SetHandleInformation(h, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0)).map_err(|err| {
            close_stdio();
            launch_err("SetHandleInformation", &err.to_string())
        })?;
    }
    for &h in request
        .extra_handles
        .iter()
        .chain(request.explicit_stdin.iter())
        .chain(request.explicit_stdout.iter())
    {
        SetHandleInformation(h, HANDLE_FLAG_INHERIT.0, HANDLE_FLAG_INHERIT).map_err(|err| {
            close_stdio();
            launch_err("SetHandleInformation(inherit extra)", &err.to_string())
        })?;
    }

    let job = match CreateJobObjectW(None, PCWSTR::null()) {
        Ok(h) => h,
        Err(err) => {
            close_stdio();
            return Err(launch_err("CreateJobObjectW", &err.to_string()));
        }
    };

    if let Err(err) = configure_job(job, &request.job) {
        let _ = CloseHandle(job);
        close_stdio();
        return Err(err);
    }

    let mut inherit = vec![child_stdin, child_stdout, child_stderr];
    for &h in &request.extra_handles {
        if !inherit.iter().any(|existing| existing.0 == h.0) {
            inherit.push(h);
        }
    }
    let cleanup_all = || {
        let _ = CloseHandle(job);
        close_stdio();
    };

    // Primary path: CREATE_SUSPENDED → AssignProcessToJobObject → ResumeThread.
    // PROC_THREAD_ATTRIBUTE_JOB_LIST is attempted first when the env opt-in is set;
    // on Windows CI runners CreateProcessW returned ERROR_INVALID_HANDLE (6) with
    // JOB_LIST + AppContainer + HANDLE_LIST, while the suspended path is reliable.
    // Security invariant holds either way: no guest instruction runs before Job assign.
    let force_assign_fail = std::env::var_os("BOOKCLERK_TEST_FAIL_JOB_ASSIGN").is_some();
    let try_job_list =
        !force_assign_fail && std::env::var_os("BOOKCLERK_AC_USE_JOB_LIST").is_some();

    let prepare_attrs = |with_job: bool| -> Result<AttributeList, SandboxError> {
        let count = 1 + u32::from(owned_caps.is_some()) + u32::from(with_job);
        let mut attrs = AttributeList::new(count).inspect_err(|_| cleanup_all())?;
        if let Some(caps) = owned_caps.as_ref() {
            attrs
                .set_security_capabilities(caps)
                .inspect_err(|_| cleanup_all())?;
        }
        attrs
            .set_handle_list(&inherit)
            .inspect_err(|_| cleanup_all())?;
        Ok(attrs)
    };

    let (mut attr, use_job_list) = if try_job_list {
        let mut with_job = prepare_attrs(true)?;
        if with_job.set_job_list(&[job]).is_ok() {
            (with_job, true)
        } else {
            tracing::debug!("PROC_THREAD_ATTRIBUTE_JOB_LIST refused; using CREATE_SUSPENDED");
            drop(with_job);
            (prepare_attrs(false)?, false)
        }
    } else {
        (prepare_attrs(false)?, false)
    };

    let mut si_ex: STARTUPINFOEXW = std::mem::zeroed();
    si_ex.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    si_ex.StartupInfo.dwFlags |= STARTF_USESTDHANDLES;
    si_ex.StartupInfo.hStdInput = child_stdin;
    si_ex.StartupInfo.hStdOutput = child_stdout;
    si_ex.StartupInfo.hStdError = child_stderr;
    si_ex.lpAttributeList = attr.as_mut_ptr();

    let exe_w = wide_os(request.exe.as_os_str());
    let mut cmdline_w = wide_str(&request.cmdline);
    let cwd_w = wide_os(request.cwd.as_os_str());
    let env_block = build_env_block(&request.env);

    let mut flags = EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT;
    if !use_job_list {
        flags |= CREATE_SUSPENDED;
    }

    let mut pi = PROCESS_INFORMATION::default();
    let mut cp = CreateProcessW(
        PCWSTR(exe_w.as_ptr()),
        Some(PWSTR(cmdline_w.as_mut_ptr())),
        None,
        None,
        true,
        flags,
        Some(env_block.as_ptr().cast()),
        PCWSTR(cwd_w.as_ptr()),
        &si_ex.StartupInfo,
        &mut pi,
    );

    // If JOB_LIST CreateProcess failed, rebuild attributes without it and retry
    // suspended — measured ERROR_INVALID_HANDLE on some hosts with JOB_LIST.
    let use_job_list = if cp.is_err() && use_job_list {
        tracing::warn!(
            "CreateProcessW with JOB_LIST failed ({}); retrying CREATE_SUSPENDED",
            std::io::Error::last_os_error()
        );
        drop(attr);
        let mut retry_attrs = match prepare_attrs(false) {
            Ok(a) => a,
            Err(err) => {
                let _ = CloseHandle(job);
                cleanup_handles(&[child_stdin, child_stdout, child_stderr]);
                cleanup_optional(&[parent_stdin_raw, parent_stdout_raw, Some(parent_stderr_raw)]);
                return Err(err);
            }
        };
        si_ex.lpAttributeList = retry_attrs.as_mut_ptr();
        flags = EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT | CREATE_SUSPENDED;
        pi = PROCESS_INFORMATION::default();
        cp = CreateProcessW(
            PCWSTR(exe_w.as_ptr()),
            Some(PWSTR(cmdline_w.as_mut_ptr())),
            None,
            None,
            true,
            flags,
            Some(env_block.as_ptr().cast()),
            PCWSTR(cwd_w.as_ptr()),
            &si_ex.StartupInfo,
            &mut pi,
        );
        attr = retry_attrs;
        false
    } else {
        use_job_list
    };

    // Child pipe / extra ends must not stay open in the parent.
    close_unique_handles(
        [child_stdin, child_stdout, child_stderr]
            .into_iter()
            .chain(request.extra_handles.iter().copied()),
    );
    drop(attr);
    drop(owned_caps);

    if cp.is_err() {
        let _ = CloseHandle(job);
        cleanup_optional(&[parent_stdin_raw, parent_stdout_raw, Some(parent_stderr_raw)]);
        return Err(launch_err(
            "CreateProcessW",
            &format!(
                "AppContainer launch failed: {}",
                std::io::Error::last_os_error()
            ),
        ));
    }

    // From here, every failure must TerminateProcess + close all handles.
    let fail_after_create = |stage: &str, detail: &str| -> SandboxError {
        unsafe {
            let _ = TerminateProcess(pi.hProcess, 1);
            let _ = WaitForSingleObject(pi.hProcess, 5_000);
            let _ = CloseHandle(pi.hThread);
            let _ = CloseHandle(pi.hProcess);
            let _ = CloseHandle(job);
            cleanup_optional(&[parent_stdin_raw, parent_stdout_raw, Some(parent_stderr_raw)]);
        }
        launch_err(stage, detail)
    };

    if force_assign_fail {
        return Err(fail_after_create(
            "AssignProcessToJobObject",
            "BOOKCLERK_TEST_FAIL_JOB_ASSIGN forced failure",
        ));
    }

    if !use_job_list {
        if let Err(err) = AssignProcessToJobObject(job, pi.hProcess) {
            return Err(fail_after_create(
                "AssignProcessToJobObject",
                &err.to_string(),
            ));
        }
        if ResumeThread(pi.hThread) == u32::MAX {
            return Err(fail_after_create(
                "ResumeThread",
                &std::io::Error::last_os_error().to_string(),
            ));
        }
    } else {
        // JOB_LIST path: membership must already be true before any guest
        // instruction runs. Never AssignProcessToJobObject after the fact —
        // that would violate the suspended-until-in-job invariant.
        let mut inside = BOOL(0);
        if IsProcessInJob(pi.hProcess, Some(job), &mut inside).is_err() || !inside.as_bool() {
            return Err(fail_after_create(
                "IsProcessInJob",
                "guest was not in Job Object after PROC_THREAD_ATTRIBUTE_JOB_LIST",
            ));
        }
    }

    let _ = CloseHandle(pi.hThread);

    let stdin = parent_stdin_raw.map(file_from_handle);
    let stdout = parent_stdout_raw.map(file_from_handle);
    let stderr = Some(file_from_handle(parent_stderr_raw));

    Ok(LaunchedGuest {
        pid: pi.dwProcessId,
        stdin,
        stdout,
        stderr,
        process: pi.hProcess,
        job,
    })
}

/// Applies memory, CPU rate, and process limits plus kill-on-close to `job`.
///
/// # Errors
///
/// Returns [`SandboxError::Backend`] when `SetInformationJobObject` fails.
fn configure_job(job: HANDLE, limits: &JobResourceLimits) -> Result<(), SandboxError> {
    unsafe {
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if let Some(bytes) = limits.memory_bytes {
            // Job-wide commit charge (main + children), aligned with Linux cgroup memory.max.
            info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_JOB_MEMORY;
            info.JobMemoryLimit = bytes;
        }
        if let Some(n) = limits.active_processes {
            info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
            info.BasicLimitInformation.ActiveProcessLimit = n;
        }
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
        .map_err(|err| launch_err("SetInformationJobObject(ext)", &err.to_string()))?;

        if let Some(percent) = limits.cpu_rate_percent {
            let mut cpu = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION {
                ControlFlags: JOB_OBJECT_CPU_RATE_CONTROL_ENABLE
                    | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
                ..Default::default()
            };
            // Job CpuRate is % of *all* processors; Spec percent is one-core.
            let logical_cpus = std::thread::available_parallelism()
                .map(|n| n.get() as u32)
                .unwrap_or(1);
            cpu.Anonymous.CpuRate = crate::windows_job_cpu_rate(percent, logical_cpus);
            SetInformationJobObject(
                job,
                JobObjectCpuRateControlInformation,
                &cpu as *const _ as *const _,
                size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32,
            )
            .map_err(|err| launch_err("SetInformationJobObject(cpu)", &err.to_string()))?;
        }
    }
    Ok(())
}

/// Owned `PROC_THREAD_ATTRIBUTE_LIST` buffer for `STARTUPINFOEXW`.
struct AttributeList {
    /// Backing storage for the attribute list.
    _buf: Vec<u8>,
    /// Pointer into `_buf` passed to Win32 APIs.
    ptr: LPPROC_THREAD_ATTRIBUTE_LIST,
}

impl AttributeList {
    /// Allocates an attribute list with `count` entries.
    ///
    /// # Errors
    ///
    /// Returns [`SandboxError::Backend`] when `InitializeProcThreadAttributeList` fails.
    fn new(count: u32) -> Result<Self, SandboxError> {
        let mut bytes = 0usize;
        unsafe {
            let _ = InitializeProcThreadAttributeList(None, count, Some(0), &mut bytes);
        }
        let mut buf = vec![0u8; bytes];
        let ptr = LPPROC_THREAD_ATTRIBUTE_LIST(buf.as_mut_ptr().cast());
        unsafe {
            InitializeProcThreadAttributeList(Some(ptr), count, Some(0), &mut bytes)
                .map_err(|err| launch_err("InitializeProcThreadAttributeList", &err.to_string()))?;
        }
        Ok(Self { _buf: buf, ptr })
    }

    /// Returns the list pointer for `STARTUPINFOEXW.lpAttributeList`.
    fn as_mut_ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.ptr
    }

    /// Sets `PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES` on the list.
    ///
    /// # Errors
    ///
    /// Returns [`SandboxError::Backend`] when `UpdateProcThreadAttribute` fails.
    fn set_security_capabilities(
        &mut self,
        caps: &OwnedSecurityCapabilities,
    ) -> Result<(), SandboxError> {
        unsafe {
            UpdateProcThreadAttribute(
                self.ptr,
                0,
                PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
                Some(caps.as_ptr().cast()),
                size_of::<SECURITY_CAPABILITIES>(),
                None,
                None,
            )
            .map_err(|err| launch_err("UpdateProcThreadAttribute(security)", &err.to_string()))
        }
    }

    /// Sets `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` for stdio inheritance.
    ///
    /// # Errors
    ///
    /// Returns [`SandboxError::Backend`] when `UpdateProcThreadAttribute` fails.
    fn set_handle_list(&mut self, handles: &[HANDLE]) -> Result<(), SandboxError> {
        unsafe {
            UpdateProcThreadAttribute(
                self.ptr,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                Some(handles.as_ptr().cast()),
                std::mem::size_of_val(handles),
                None,
                None,
            )
            .map_err(|err| launch_err("UpdateProcThreadAttribute(handles)", &err.to_string()))
        }
    }

    /// Sets `PROC_THREAD_ATTRIBUTE_JOB_LIST` for pre-create Job assignment.
    ///
    /// # Errors
    ///
    /// Returns [`SandboxError::Backend`] when `UpdateProcThreadAttribute` fails.
    fn set_job_list(&mut self, jobs: &[HANDLE]) -> Result<(), SandboxError> {
        unsafe {
            UpdateProcThreadAttribute(
                self.ptr,
                0,
                PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                Some(jobs.as_ptr().cast()),
                std::mem::size_of_val(jobs),
                None,
                None,
            )
            .map_err(|err| launch_err("UpdateProcThreadAttribute(job_list)", &err.to_string()))
        }
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        unsafe {
            DeleteProcThreadAttributeList(self.ptr);
        }
    }
}

/// Keeps AppContainer SID allocations alive for `SECURITY_CAPABILITIES`.
struct OwnedSecurityCapabilities {
    /// Package SID converted from rappct.
    _app_sid: LocalSid,
    /// Capability SIDs converted from rappct.
    _cap_sids: Vec<LocalSid>,
    /// `SID_AND_ATTRIBUTES` slice referenced by `sc`.
    _caps: Box<[SID_AND_ATTRIBUTES]>,
    /// Win32 structure passed to CreateProcess extended attributes.
    sc: SECURITY_CAPABILITIES,
}

impl OwnedSecurityCapabilities {
    /// Builds owned Win32 security capabilities from rappct inputs.
    ///
    /// # Errors
    ///
    /// Returns [`SandboxError::Backend`] when SID SDDL conversion fails.
    fn from_rappct(sec: &SecurityCapabilities) -> Result<Self, SandboxError> {
        let app = LocalSid::from_sddl(sec.package.as_string())?;
        let mut cap_sids = Vec::with_capacity(sec.caps.len());
        for SidAndAttributes { sid_sddl, .. } in &sec.caps {
            cap_sids.push(LocalSid::from_sddl(sid_sddl)?);
        }
        let caps_vec: Vec<SID_AND_ATTRIBUTES> = cap_sids
            .iter()
            .map(|sid| SID_AND_ATTRIBUTES {
                Sid: sid.as_psid(),
                Attributes: SE_GROUP_ENABLED,
            })
            .collect();
        let caps = caps_vec.into_boxed_slice();
        let sc = SECURITY_CAPABILITIES {
            AppContainerSid: app.as_psid(),
            Capabilities: caps.as_ptr() as *mut _,
            CapabilityCount: caps.len() as u32,
            Reserved: 0,
        };
        Ok(Self {
            _app_sid: app,
            _cap_sids: cap_sids,
            _caps: caps,
            sc,
        })
    }

    /// Returns a pointer to the embedded `SECURITY_CAPABILITIES`.
    fn as_ptr(&self) -> *const SECURITY_CAPABILITIES {
        &self.sc
    }
}

/// Owned PSID allocated with `ConvertStringSidToSidW`.
struct LocalSid {
    /// Win32 SID pointer freed on drop.
    psid: PSID,
}

impl LocalSid {
    /// Parses an SDDL string into an owned SID.
    ///
    /// # Errors
    ///
    /// Returns [`SandboxError::Backend`] when Win32 rejects the SDDL.
    fn from_sddl(sddl: &str) -> Result<Self, SandboxError> {
        let wide = wide_str(sddl);
        let mut psid = PSID(ptr::null_mut());
        unsafe {
            ConvertStringSidToSidW(PCWSTR(wide.as_ptr()), &mut psid)
                .map_err(|err| launch_err("ConvertStringSidToSidW", &err.to_string()))?;
        }
        Ok(Self { psid })
    }

    /// Returns the raw PSID for Win32 trustee and capability structs.
    fn as_psid(&self) -> PSID {
        self.psid
    }
}

impl Drop for LocalSid {
    fn drop(&mut self) {
        if !self.psid.0.is_null() {
            unsafe {
                let _ = LocalFree(Some(HLOCAL(self.psid.0)));
            }
            self.psid = PSID(ptr::null_mut());
        }
    }
}

/// Returns `(read, write)` ends from `CreatePipe`.
fn create_pipe_pair(sa: &mut SECURITY_ATTRIBUTES) -> Result<(HANDLE, HANDLE), SandboxError> {
    let mut read = HANDLE::default();
    let mut write = HANDLE::default();
    unsafe {
        CreatePipe(&mut read, &mut write, Some(sa), 0)
            .map_err(|err| launch_err("CreatePipe", &err.to_string()))?;
    }
    Ok((read, write))
}

/// Wraps a Win32 handle in a `File` that closes the handle on drop.
fn file_from_handle(handle: HANDLE) -> File {
    unsafe { File::from_raw_handle(handle.0 as RawHandle) }
}

/// Closes every non-invalid handle in `handles`, ignoring errors.
fn cleanup_handles(handles: &[HANDLE]) {
    for &h in handles {
        if !h.is_invalid() && h != HANDLE::default() {
            unsafe {
                let _ = CloseHandle(h);
            }
        }
    }
}

/// Closes every present handle in `handles`.
fn cleanup_optional(handles: &[Option<HANDLE>]) {
    for handle in handles.iter().flatten() {
        cleanup_handles(&[*handle]);
    }
}

/// Closes each distinct non-invalid handle once.
fn close_unique_handles(handles: impl IntoIterator<Item = HANDLE>) {
    let mut seen = std::collections::HashSet::new();
    for h in handles {
        let key = h.0 as usize;
        if !h.is_invalid() && h != HANDLE::default() && seen.insert(key) {
            unsafe {
                let _ = CloseHandle(h);
            }
        }
    }
}

/// Encodes a Rust str as a NUL-terminated UTF-16 vector for Win32 APIs.
fn wide_str(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Encodes an `OsStr` as a NUL-terminated UTF-16 vector for Win32 APIs.
fn wide_os(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// Builds a double-NUL-terminated environment block for `CreateProcessW`.
fn build_env_block(env: &[(OsString, OsString)]) -> Vec<u16> {
    let mut block = Vec::new();
    for (k, v) in env {
        let mut kv = OsString::from(k);
        kv.push("=");
        kv.push(v);
        block.extend(kv.as_os_str().encode_wide());
        block.push(0);
    }
    block.push(0);
    block
}

/// Formats an AppContainer launch failure as [`SandboxError::Backend`].
fn launch_err(stage: &str, detail: &str) -> SandboxError {
    SandboxError::Backend {
        label: "appcontainer".into(),
        backend: "appcontainer",
        detail: format!("{stage}: {detail}"),
    }
}

/// Host-owned Job that holds both sibling `bookclerk-jail` processes.
///
/// Nested per-guest Jobs created by the jail still work; this Job is the
/// session-level `KILL_ON_JOB_CLOSE` cap (aggregate memory / PIDs / CPU).
pub struct SessionJob {
    /// Owned job-object handle as an integer so `SessionJob` is `Send`.
    ///
    /// The kernel object can be used from the vat thread. Drop closes it and
    /// kills the tree (`KILL_ON_JOB_CLOSE`). `0` means already closed.
    handle: isize,
}

impl SessionJob {
    /// Create a kill-on-close Job with `limits` (best-effort when a field is unset).
    ///
    /// # Errors
    ///
    /// Returns an I/O error when `CreateJobObjectW` or Job configuration fails.
    pub fn create(limits: &crate::ResourceLimits) -> std::io::Result<Self> {
        let job =
            unsafe { CreateJobObjectW(None, PCWSTR::null()) }.map_err(std::io::Error::other)?;
        let job_limits = JobResourceLimits {
            memory_bytes: limits.memory_bytes.and_then(|b| usize::try_from(b).ok()),
            cpu_rate_percent: limits.cpu_rate_percent,
            active_processes: limits.active_processes,
        };
        if let Err(err) = configure_job(job, &job_limits) {
            let _ = unsafe { CloseHandle(job) };
            return Err(std::io::Error::other(err.to_string()));
        }
        Ok(Self {
            handle: job.0 as isize,
        })
    }

    /// Assign an already-started process (typically `bookclerk-jail.exe`).
    ///
    /// # Errors
    ///
    /// Returns an I/O error when `AssignProcessToJobObject` fails.
    pub fn assign(&self, process: RawHandle) -> std::io::Result<()> {
        unsafe {
            AssignProcessToJobObject(self.as_handle(), HANDLE(process))
                .map_err(std::io::Error::other)
        }
    }

    /// Reconstruct the Win32 job handle from the stored integer.
    fn as_handle(&self) -> HANDLE {
        HANDLE(self.handle as *mut std::ffi::c_void)
    }
}

impl Drop for SessionJob {
    fn drop(&mut self) {
        if self.handle != 0 {
            // Closing the Job kills the tree (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`).
            let _ = unsafe { CloseHandle(self.as_handle()) };
            self.handle = 0;
        }
    }
}

/// Terminate every process in `job` (best-effort) then close it.
#[allow(dead_code)]
pub fn terminate_job_tree(job: HANDLE) {
    unsafe {
        if !job.is_invalid() {
            let _ = TerminateJobObject(job, 1);
            let _ = CloseHandle(job);
        }
    }
}
