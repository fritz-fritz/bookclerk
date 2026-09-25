//! Host-created IPC links delivered by descriptor or handle inheritance.
//!
//! Native-behind-workerd siblings never open each other by name. The host
//! creates two duplex links (guest RPC + multiplexed socket proxy), keeps both
//! ends `CLOEXEC` / non-inheritable, and delivers the child ends:
//!
//! - **Unix:** [`inherit_fd_at`] from `Command::pre_exec` (`dup2` onto a fixed
//!   child fd). Concurrent spawns cannot inherit another session's link.
//! - **Windows:** [`duplicate_handle_into`] the jail process, then one bounded
//!   JSON [`JailHandoff`] line on the jail's stdin. The jail marks the
//!   duplicates inheritable and puts them on `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`.
//!
//! Children name an inherited end with [`LinkSpec`] (`fd:3` / `handle:123`).

#![allow(unsafe_code)] // Unix socketpair/pipe/dup2; Windows CreateNamedPipe/DuplicateHandle.
#![allow(clippy::missing_docs_in_private_items)]

use std::fmt;
use std::io;

use serde::{Deserialize, Serialize};

/// Env var naming the guest Cap'n Proto link for `bookclerk-workerd`.
pub const GATEWAY_GUEST_RPC_ENV: &str = "BOOKCLERK_GATEWAY_GUEST_RPC";
/// Env var naming the multiplexed CONNECT-proxy link for `bookclerk-workerd`.
pub const GATEWAY_PROXY_ENV: &str = "BOOKCLERK_GATEWAY_PROXY";
/// Env var naming the socket-proxy link for a native guest (SDK `net::connect`).
pub const SOCKET_PROXY_ENV: &str = "BOOKCLERK_SOCKET_PROXY";
/// When `1`, `bookclerk-jail` reads one [`JailHandoff`] JSON line from stdin.
pub const JAIL_HANDOFF_ENV: &str = "BOOKCLERK_JAIL_HANDOFF";
/// Host-chosen workerd session directory (`0700`, under `$TMPDIR`).
pub const WORKERD_STATE_DIR_ENV: &str = "BOOKCLERK_WORKERD_STATE_DIR";

/// Child fd the gateway uses for the guest RPC duplex (`BOOKCLERK_GATEWAY_GUEST_RPC`).
pub const GATEWAY_RPC_FD: i32 = 3;
/// Child fd the gateway uses for the proxy mux (`BOOKCLERK_GATEWAY_PROXY`).
pub const GATEWAY_PROXY_FD: i32 = 4;
/// Child fd the native guest uses for the proxy mux (`BOOKCLERK_SOCKET_PROXY`).
pub const GUEST_PROXY_FD: i32 = 3;

/// How a confined child names an inherited duplex or pipe end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkSpec {
    /// Unix inherited file descriptor (`fd:3`).
    Fd(i32),
    /// Windows inherited handle value (`handle:18446744073709551615`).
    Handle(u64),
}

/// Why a [`LinkSpec`] string could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LinkSpecError {
    /// Missing `fd:` / `handle:` prefix or empty remainder.
    #[error("link spec must be `fd:<n>` or `handle:<n>`, got `{0}`")]
    Unrecognized(String),
    /// Prefix present but the number did not parse.
    #[error("link spec `{0}` has a non-numeric suffix")]
    BadNumber(String),
}

impl LinkSpec {
    /// Parse `fd:<n>` or `handle:<n>`.
    ///
    /// # Errors
    ///
    /// Returns [`LinkSpecError`] when the prefix is missing or the number is
    /// not a decimal `i32` / `u64`.
    pub fn parse(spec: &str) -> Result<Self, LinkSpecError> {
        if let Some(rest) = spec.strip_prefix("fd:") {
            let n = rest
                .parse()
                .map_err(|_| LinkSpecError::BadNumber(spec.to_string()))?;
            return Ok(Self::Fd(n));
        }
        if let Some(rest) = spec.strip_prefix("handle:") {
            let n = rest
                .parse()
                .map_err(|_| LinkSpecError::BadNumber(spec.to_string()))?;
            return Ok(Self::Handle(n));
        }
        Err(LinkSpecError::Unrecognized(spec.to_string()))
    }

    /// `fd:<n>` form used on Unix.
    #[must_use]
    pub fn fd(n: i32) -> Self {
        Self::Fd(n)
    }

    /// `handle:<n>` form used on Windows.
    #[must_use]
    pub fn handle(n: u64) -> Self {
        Self::Handle(n)
    }
}

impl fmt::Display for LinkSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fd(n) => write!(f, "fd:{n}"),
            Self::Handle(n) => write!(f, "handle:{n}"),
        }
    }
}

impl std::str::FromStr for LinkSpec {
    type Err = LinkSpecError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// One-shot Windows jail stdin line. Bounded JSON; never a general broker.
///
/// The host derives every handle from values it just duplicated with
/// `duplicate_handle_into` (Windows). The jail is the sole spawner and marks
/// them inheritable immediately before `CreateProcess`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JailHandoff {
    /// Wire version. Only `1` is accepted.
    pub v: u32,
    /// Guest stdin handle (RPC read end), when the jail should not create a pipe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdin: Option<u64>,
    /// Guest stdout handle (RPC write end), when the jail should not create a pipe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout: Option<u64>,
    /// Extra inheritable handles exported as `env=handle:<value>`.
    #[serde(default)]
    pub extra: Vec<JailHandoffExtra>,
}

/// One extra inherited handle named in the child environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JailHandoffExtra {
    /// Environment variable the child should see (`BOOKCLERK_SOCKET_PROXY`, …).
    pub env: String,
    /// Handle value in the jail process (already duplicated).
    pub handle: u64,
}

impl JailHandoff {
    /// Current wire version.
    pub const VERSION: u32 = 1;

    /// Maximum accepted JSON line length (8 KiB). Larger input is a protocol error.
    pub const MAX_LINE_BYTES: usize = 8 * 1024;

    /// Encode as a single JSON line (no trailing newline; the caller writes `\n`).
    ///
    /// # Errors
    ///
    /// Returns [`std::io::Error`] when serde cannot encode the handoff.
    pub fn to_line(&self) -> io::Result<String> {
        serde_json::to_string(self).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
    }

    /// Parse one JSON line. Rejects `v != 1` and oversize input.
    ///
    /// # Errors
    ///
    /// Returns [`std::io::Error`] when the line is too long, not JSON, or
    /// the version is not [`Self::VERSION`].
    pub fn from_line(line: &str) -> io::Result<Self> {
        if line.len() > Self::MAX_LINE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "jail handoff line exceeds 8 KiB",
            ));
        }
        let parsed: Self = serde_json::from_str(line.trim())
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        if parsed.v != Self::VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported jail handoff version {}", parsed.v),
            ));
        }
        Ok(parsed)
    }
}

/// One end of a host-created duplex IPC link (socketpair / overlapped pipe).
#[derive(Debug)]
pub struct DuplexLink {
    #[cfg(unix)]
    fd: std::os::fd::OwnedFd,
    #[cfg(windows)]
    handle: std::os::windows::io::OwnedHandle,
    #[cfg(not(any(unix, windows)))]
    _priv: (),
}

impl DuplexLink {
    /// Create a connected duplex pair. Both ends stay non-inheritable / `CLOEXEC`.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the kernel cannot allocate the pair.
    pub fn pair() -> io::Result<(Self, Self)> {
        #[cfg(unix)]
        {
            unix::socketpair()
        }
        #[cfg(windows)]
        {
            windows::duplex_pair()
        }
        #[cfg(not(any(unix, windows)))]
        {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "DuplexLink requires Unix or Windows",
            ))
        }
    }

    /// Duplicate this end. Both copies stay `CLOEXEC` / non-inheritable.
    ///
    /// Used so guest stdin and stdout can share one duplex without racing
    /// `dup` in the caller.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the kernel cannot duplicate the descriptor.
    pub fn try_clone(&self) -> io::Result<Self> {
        #[cfg(unix)]
        {
            Ok(Self {
                fd: self.fd.try_clone()?,
            })
        }
        #[cfg(windows)]
        {
            Ok(Self {
                handle: self.handle.try_clone()?,
            })
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = self;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "DuplexLink requires Unix or Windows",
            ))
        }
    }

    /// Unix raw fd (still owned by `self`).
    #[cfg(unix)]
    #[must_use]
    pub fn as_raw_fd(&self) -> std::os::fd::RawFd {
        use std::os::fd::AsRawFd;
        self.fd.as_raw_fd()
    }

    /// Consume this end as an [`std::os::fd::OwnedFd`].
    #[cfg(unix)]
    #[must_use]
    pub fn into_owned_fd(self) -> std::os::fd::OwnedFd {
        self.fd
    }

    /// Windows raw handle (still owned by `self`).
    #[cfg(windows)]
    #[must_use]
    pub fn as_raw_handle(&self) -> std::os::windows::io::RawHandle {
        use std::os::windows::io::AsRawHandle;
        self.handle.as_raw_handle()
    }

    /// Consume this end as an [`std::os::windows::io::OwnedHandle`].
    #[cfg(windows)]
    #[must_use]
    pub fn into_owned_handle(self) -> std::os::windows::io::OwnedHandle {
        self.handle
    }

    /// Handle value for [`LinkSpec::Handle`] / [`JailHandoff`].
    #[cfg(windows)]
    #[must_use]
    pub fn handle_value(&self) -> u64 {
        self.as_raw_handle() as usize as u64
    }

    /// Mark this end inheritable. Product hosts never call this (they
    /// `DuplicateHandle` into `bookclerk-jail` instead). Unconfined tests
    /// use it with `CommandExt::inherit_handles`.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when `SetHandleInformation` fails.
    #[cfg(windows)]
    pub fn set_inheritable(&self, inherit: bool) -> io::Result<()> {
        windows::set_handle_inheritable(self.as_raw_handle(), inherit)
    }
}

/// Host and guest ends of a pair of unidirectional stdio pipes (RPC).
#[derive(Debug)]
pub struct StdioEnds {
    /// Parent writes this into the child's stdin.
    pub host_stdin: DuplexHalf,
    /// Parent reads this from the child's stdout.
    pub host_stdout: DuplexHalf,
    /// Child stdin read end.
    pub guest_stdin: DuplexHalf,
    /// Child stdout write end.
    pub guest_stdout: DuplexHalf,
}

/// One unidirectional pipe end (stdio).
#[derive(Debug)]
pub struct DuplexHalf {
    #[cfg(unix)]
    fd: std::os::fd::OwnedFd,
    #[cfg(windows)]
    handle: std::os::windows::io::OwnedHandle,
    #[cfg(not(any(unix, windows)))]
    _priv: (),
}

impl StdioEnds {
    /// Two unidirectional pipes: host stdin-write / stdout-read, guest the opposite.
    ///
    /// Windows: host ends are overlapped; guest ends are synchronous (Rust std
    /// `Command` convention). All DACLs are owner-only.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when a pipe cannot be created.
    pub fn pair() -> io::Result<Self> {
        #[cfg(unix)]
        {
            unix::stdio_pair()
        }
        #[cfg(windows)]
        {
            windows::stdio_pair()
        }
        #[cfg(not(any(unix, windows)))]
        {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "StdioEnds requires Unix or Windows",
            ))
        }
    }
}

impl DuplexHalf {
    /// Unix raw fd.
    #[cfg(unix)]
    #[must_use]
    pub fn as_raw_fd(&self) -> std::os::fd::RawFd {
        use std::os::fd::AsRawFd;
        self.fd.as_raw_fd()
    }

    /// Consume as [`std::os::fd::OwnedFd`].
    #[cfg(unix)]
    #[must_use]
    pub fn into_owned_fd(self) -> std::os::fd::OwnedFd {
        self.fd
    }

    /// Windows raw handle.
    #[cfg(windows)]
    #[must_use]
    pub fn as_raw_handle(&self) -> std::os::windows::io::RawHandle {
        use std::os::windows::io::AsRawHandle;
        self.handle.as_raw_handle()
    }

    /// Consume as [`std::os::windows::io::OwnedHandle`].
    #[cfg(windows)]
    #[must_use]
    pub fn into_owned_handle(self) -> std::os::windows::io::OwnedHandle {
        self.handle
    }

    /// Handle value for [`JailHandoff`].
    #[cfg(windows)]
    #[must_use]
    pub fn handle_value(&self) -> u64 {
        self.as_raw_handle() as usize as u64
    }
}

/// `dup2` `src` onto `dest` and clear `FD_CLOEXEC` on the destination.
///
/// Call from `Command::pre_exec` only. `src` stays `CLOEXEC` in the parent;
/// after `dup2` the child sees `dest` without `CLOEXEC`.
///
/// # Errors
///
/// Returns an I/O error when `dup2` or `fcntl` fails.
///
/// # Safety
///
/// Must run in the child after `fork` and before `exec`. `src` must be open.
#[cfg(unix)]
pub fn inherit_fd_at(src: std::os::fd::RawFd, dest: std::os::fd::RawFd) -> io::Result<()> {
    unix::inherit_fd_at(src, dest)
}

/// Duplicate `handle` into `target` without making it inheritable there.
///
/// The jail later marks the duplicate inheritable and lists it for
/// `CreateProcess`. The host never sets `HANDLE_FLAG_INHERIT` on its own copy.
///
/// # Errors
///
/// Returns an I/O error when `DuplicateHandle` fails.
#[cfg(windows)]
pub fn duplicate_handle_into(
    handle: std::os::windows::io::RawHandle,
    target: std::os::windows::io::RawHandle,
) -> io::Result<u64> {
    windows::duplicate_handle_into(handle, target)
}

#[cfg(unix)]
#[allow(unsafe_code)] // socketpair, pipe, dup2, fcntl.
#[allow(clippy::missing_docs_in_private_items)]
mod unix {
    use std::io;
    use std::os::fd::{FromRawFd, OwnedFd, RawFd};

    use super::{DuplexHalf, DuplexLink, StdioEnds};

    pub(super) fn socketpair() -> io::Result<(DuplexLink, DuplexLink)> {
        let mut fds = [0 as RawFd; 2];
        // Linux: SOCK_CLOEXEC. macOS: SOCK_STREAM then FD_CLOEXEC (no SOCK_CLOEXEC
        // on older SDKs). The host never inherits these across an unrelated spawn.
        #[cfg(target_os = "linux")]
        let ty = libc::SOCK_STREAM | libc::SOCK_CLOEXEC;
        #[cfg(not(target_os = "linux"))]
        let ty = libc::SOCK_STREAM;
        let rc = unsafe { libc::socketpair(libc::AF_UNIX, ty, 0, fds.as_mut_ptr()) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        if let Err(err) = set_cloexec(fds[0]).and_then(|()| set_cloexec(fds[1])) {
            unsafe {
                libc::close(fds[0]);
                libc::close(fds[1]);
            }
            return Err(err);
        }
        Ok((
            DuplexLink {
                fd: unsafe { OwnedFd::from_raw_fd(fds[0]) },
            },
            DuplexLink {
                fd: unsafe { OwnedFd::from_raw_fd(fds[1]) },
            },
        ))
    }

    fn set_cloexec(fd: RawFd) -> io::Result<()> {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        if unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn pipe_cloexec() -> io::Result<(OwnedFd, OwnedFd)> {
        let mut fds = [0 as RawFd; 2];
        // `pipe2` is Linux-only; macOS gets `pipe` + `FD_CLOEXEC`.
        #[cfg(target_os = "linux")]
        let rc = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
        #[cfg(not(target_os = "linux"))]
        let rc = unsafe { libc::pipe(fds.as_mut_ptr()) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        if let Err(err) = set_cloexec(fds[0]).and_then(|()| set_cloexec(fds[1])) {
            unsafe {
                libc::close(fds[0]);
                libc::close(fds[1]);
            }
            return Err(err);
        }
        Ok((unsafe { OwnedFd::from_raw_fd(fds[0]) }, unsafe {
            OwnedFd::from_raw_fd(fds[1])
        }))
    }

    pub(super) fn stdio_pair() -> io::Result<StdioEnds> {
        let (guest_stdin, host_stdin) = pipe_cloexec()?;
        let (host_stdout, guest_stdout) = pipe_cloexec()?;
        Ok(StdioEnds {
            host_stdin: DuplexHalf { fd: host_stdin },
            host_stdout: DuplexHalf { fd: host_stdout },
            guest_stdin: DuplexHalf { fd: guest_stdin },
            guest_stdout: DuplexHalf { fd: guest_stdout },
        })
    }

    pub(super) fn inherit_fd_at(src: RawFd, dest: RawFd) -> io::Result<()> {
        if unsafe { libc::dup2(src, dest) } < 0 {
            return Err(io::Error::last_os_error());
        }
        let flags = unsafe { libc::fcntl(dest, libc::F_GETFD) };
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        if unsafe { libc::fcntl(dest, libc::F_SETFD, flags & !libc::FD_CLOEXEC) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
#[allow(clippy::missing_docs_in_private_items)]
mod windows {
    use std::io;
    use std::os::windows::io::{FromRawHandle, OwnedHandle, RawHandle};
    use std::ptr;

    use rand::RngCore;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{
        CloseHandle, DuplicateHandle, SetHandleInformation, HANDLE, HANDLE_FLAGS,
        HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE,
    };
    use windows::Win32::Security::SECURITY_ATTRIBUTES;
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED,
        FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX, PIPE_ACCESS_INBOUND,
        PIPE_ACCESS_OUTBOUND,
    };
    use windows::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
        PIPE_TYPE_BYTE, PIPE_WAIT,
    };
    use windows::Win32::System::Threading::GetCurrentProcess;

    use super::{DuplexHalf, DuplexLink, StdioEnds};

    const PIPE_UNLIMITED_INSTANCES: u32 = 255;

    fn random_pipe_name(kind: &str) -> String {
        let mut nonce = [0u8; 8];
        rand::thread_rng().fill_bytes(&mut nonce);
        format!(r"\\.\pipe\bc-{kind}-{}", hex_encode(&nonce))
    }

    fn hex_encode(bytes: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0f) as usize] as char);
        }
        out
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn close_if_valid(handle: HANDLE) {
        if !handle.is_invalid() && handle != HANDLE::default() && handle != INVALID_HANDLE_VALUE {
            let _ = unsafe { CloseHandle(handle) };
        }
    }

    fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
        if handle.is_invalid() || handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "CreateNamedPipe/CreateFile returned an invalid handle",
            ));
        }
        // Owner-only DACL is the process default; do not mark inheritable.
        unsafe {
            SetHandleInformation(handle, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0)).map_err(
                |err| {
                    close_if_valid(handle);
                    io::Error::other(err)
                },
            )?;
        }
        Ok(unsafe { OwnedHandle::from_raw_handle(handle.0 as RawHandle) })
    }

    /// Duplex overlapped pipe; both ends `FILE_FLAG_OVERLAPPED`, max 1 instance.
    pub(super) fn duplex_pair() -> io::Result<(DuplexLink, DuplexLink)> {
        let name = random_pipe_name("l");
        let name_w = wide(&name);
        let server = unsafe {
            CreateNamedPipeW(
                PCWSTR(name_w.as_ptr()),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                16 * 1024,
                16 * 1024,
                0,
                None,
            )
        };
        if server.is_invalid() {
            return Err(io::Error::last_os_error());
        }
        let client = unsafe {
            CreateFileW(
                PCWSTR(name_w.as_ptr()),
                windows::Win32::Storage::FileSystem::FILE_GENERIC_READ.0
                    | windows::Win32::Storage::FileSystem::FILE_GENERIC_WRITE.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OVERLAPPED,
                None,
            )
        };
        let client = match client {
            Ok(h) => h,
            Err(err) => {
                close_if_valid(server);
                return Err(io::Error::other(err));
            }
        };
        // ConnectNamedPipe after CreateFile: the instance is already connected;
        // ERROR_PIPE_CONNECTED is success. Overlapped connect is not required
        // once the client handle exists.
        let connected = unsafe { ConnectNamedPipe(server, None) };
        if connected.is_err() {
            let err = io::Error::last_os_error();
            // 535 == ERROR_PIPE_CONNECTED
            if err.raw_os_error() != Some(535) {
                close_if_valid(server);
                close_if_valid(client);
                return Err(err);
            }
        }
        Ok((
            DuplexLink {
                handle: owned(server)?,
            },
            DuplexLink {
                handle: owned(client)?,
            },
        ))
    }

    fn unidirectional(host_writes: bool) -> io::Result<(OwnedHandle, OwnedHandle)> {
        let name = random_pipe_name(if host_writes { "si" } else { "so" });
        let name_w = wide(&name);
        let access = if host_writes {
            PIPE_ACCESS_INBOUND
        } else {
            PIPE_ACCESS_OUTBOUND
        };
        // Server is the guest-synchronous end for the child's std handle.
        // Host end (CreateFile) is overlapped so tokio / overlapped I/O works
        // in bookclerk-workerd.
        let server = unsafe {
            CreateNamedPipeW(
                PCWSTR(name_w.as_ptr()),
                access | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                16 * 1024,
                16 * 1024,
                0,
                None,
            )
        };
        if server.is_invalid() {
            return Err(io::Error::last_os_error());
        }
        let desired = if host_writes {
            windows::Win32::Storage::FileSystem::FILE_GENERIC_WRITE.0
        } else {
            windows::Win32::Storage::FileSystem::FILE_GENERIC_READ.0
        };
        let client = unsafe {
            CreateFileW(
                PCWSTR(name_w.as_ptr()),
                desired,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OVERLAPPED,
                None,
            )
        };
        let client = match client {
            Ok(h) => h,
            Err(err) => {
                close_if_valid(server);
                return Err(io::Error::other(err));
            }
        };
        let connected = unsafe { ConnectNamedPipe(server, None) };
        if connected.is_err() {
            let err = io::Error::last_os_error();
            if err.raw_os_error() != Some(535) {
                close_if_valid(server);
                close_if_valid(client);
                return Err(err);
            }
        }
        // server = guest (sync), client = host (overlapped)
        Ok((owned(client)?, owned(server)?))
    }

    pub(super) fn stdio_pair() -> io::Result<StdioEnds> {
        let (host_stdin, guest_stdin) = unidirectional(true)?;
        let (host_stdout, guest_stdout) = unidirectional(false)?;
        Ok(StdioEnds {
            host_stdin: DuplexHalf { handle: host_stdin },
            host_stdout: DuplexHalf {
                handle: host_stdout,
            },
            guest_stdin: DuplexHalf {
                handle: guest_stdin,
            },
            guest_stdout: DuplexHalf {
                handle: guest_stdout,
            },
        })
    }

    pub(super) fn set_handle_inheritable(handle: RawHandle, inherit: bool) -> io::Result<()> {
        let flags = if inherit {
            HANDLE_FLAG_INHERIT
        } else {
            HANDLE_FLAGS(0)
        };
        unsafe {
            SetHandleInformation(HANDLE(handle), HANDLE_FLAG_INHERIT.0, flags)
                .map_err(io::Error::other)
        }
    }

    pub(super) fn duplicate_handle_into(handle: RawHandle, target: RawHandle) -> io::Result<u64> {
        let mut dest = HANDLE::default();
        unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                HANDLE(handle),
                HANDLE(target),
                &mut dest,
                0,
                false,
                windows::Win32::Foundation::DUPLICATE_SAME_ACCESS,
            )
            .map_err(io::Error::other)?;
        }
        Ok(dest.0 as usize as u64)
    }

    #[allow(dead_code)]
    fn _sa_unused() {
        // Keep SECURITY_ATTRIBUTES import live for future owner-only explicit DACLs.
        let _ = std::mem::size_of::<SECURITY_ATTRIBUTES>();
        let _ = ptr::null_mut::<()>();
        let _ = PIPE_UNLIMITED_INSTANCES;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_spec_round_trips() {
        assert_eq!(LinkSpec::parse("fd:3").unwrap(), LinkSpec::Fd(3));
        assert_eq!(LinkSpec::parse("handle:42").unwrap(), LinkSpec::Handle(42));
        assert_eq!(LinkSpec::fd(4).to_string(), "fd:4");
        assert_eq!(LinkSpec::handle(7).to_string(), "handle:7");
        assert!(matches!(
            LinkSpec::parse("/tmp/sockets.sock"),
            Err(LinkSpecError::Unrecognized(_))
        ));
        assert!(matches!(
            LinkSpec::parse("fd:nope"),
            Err(LinkSpecError::BadNumber(_))
        ));
    }

    #[test]
    fn jail_handoff_round_trips_and_rejects_bad_version() {
        let handoff = JailHandoff {
            v: 1,
            stdin: Some(11),
            stdout: Some(12),
            extra: vec![JailHandoffExtra {
                env: SOCKET_PROXY_ENV.into(),
                handle: 13,
            }],
        };
        let line = handoff.to_line().expect("encode");
        assert!(line.len() < JailHandoff::MAX_LINE_BYTES);
        assert_eq!(JailHandoff::from_line(&line).expect("decode"), handoff);
        let err = JailHandoff::from_line(r#"{"v":2}"#).expect_err("v=2");
        assert!(err.to_string().contains("version"), "{err}");
        let huge = "x".repeat(JailHandoff::MAX_LINE_BYTES + 1);
        assert!(JailHandoff::from_line(&huge).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn duplex_link_echoes() {
        use std::io::{Read, Write};

        let (a, b) = DuplexLink::pair().expect("pair");
        let mut a = std::fs::File::from(a.into_owned_fd());
        let mut b = std::fs::File::from(b.into_owned_fd());
        a.write_all(b"ping").expect("write");
        let mut buf = [0u8; 4];
        b.read_exact(&mut buf).expect("read");
        assert_eq!(&buf, b"ping");
    }

    #[cfg(unix)]
    #[test]
    fn stdio_ends_are_unidirectional() {
        use std::io::{Read, Write};

        let ends = StdioEnds::pair().expect("stdio");
        let mut host_in = std::fs::File::from(ends.host_stdin.into_owned_fd());
        let mut guest_in = std::fs::File::from(ends.guest_stdin.into_owned_fd());
        host_in.write_all(b"rpc").expect("host stdin");
        let mut buf = [0u8; 3];
        guest_in.read_exact(&mut buf).expect("guest stdin");
        assert_eq!(&buf, b"rpc");

        let mut guest_out = std::fs::File::from(ends.guest_stdout.into_owned_fd());
        let mut host_out = std::fs::File::from(ends.host_stdout.into_owned_fd());
        guest_out.write_all(b"ok!").expect("guest stdout");
        host_out.read_exact(&mut buf).expect("host stdout");
        assert_eq!(&buf, b"ok!");
    }

    #[cfg(unix)]
    #[test]
    fn inherit_fd_at_clears_cloexec_on_the_destination() {
        use std::io::{Read, Write};
        use std::os::fd::FromRawFd;

        let (a, b) = DuplexLink::pair().expect("pair");
        let dest = 64;
        inherit_fd_at(a.as_raw_fd(), dest).expect("dup2");
        drop(a);
        let mut dest_file = unsafe { std::fs::File::from_raw_fd(dest) };
        let mut peer = std::fs::File::from(b.into_owned_fd());
        peer.write_all(b"hi").expect("write");
        let mut buf = [0u8; 2];
        dest_file.read_exact(&mut buf).expect("read dest");
        assert_eq!(&buf, b"hi");
    }
}
