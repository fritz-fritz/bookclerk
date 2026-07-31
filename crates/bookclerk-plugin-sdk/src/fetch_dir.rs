//! Receive the per-fetch work directory the host passes over a side channel.

#![cfg_attr(unix, allow(unsafe_code))]

use std::path::PathBuf;

use crate::error::{Result, SdkError};
use crate::protocol::FetchTitleParams;

/// Descriptor the host leaves open for the fetch-directory side channel.
const PLUGIN_FD_CHANNEL: i32 = 3;

/// Environment variable naming [`PLUGIN_FD_CHANNEL`].
const PLUGIN_FD_CHANNEL_ENV: &str = "BOOKCLERK_PLUGIN_FD_CHANNEL";

/// Resolve the directory a `fetch_title` call should write into.
///
/// Jailed guests receive an open directory descriptor immediately before the RPC
/// arrives. Tests and unconfined development fall back to the `cache_dir` string.
pub fn fetch_work_dir(params: &FetchTitleParams) -> Result<PathBuf> {
    match std::env::var(PLUGIN_FD_CHANNEL_ENV) {
        Ok(raw) => {
            let channel: i32 = raw.parse().map_err(|err| {
                SdkError::message(format!("invalid {PLUGIN_FD_CHANNEL_ENV}={raw:?}: {err}"))
            })?;
            if channel != PLUGIN_FD_CHANNEL {
                return Err(SdkError::message(format!(
                    "unsupported fetch-directory channel fd {channel} (expected {})",
                    PLUGIN_FD_CHANNEL
                )));
            }
            recv_fetch_dir(channel)
        }
        Err(_) => Ok(PathBuf::from(&params.cache_dir)),
    }
}

#[cfg(unix)]
fn recv_fetch_dir(channel: i32) -> Result<PathBuf> {
    use std::os::fd::FromRawFd;

    let received = recv_one_fd(channel).map_err(SdkError::Io)?;
    // SAFETY: the host sent one directory descriptor and we own it now.
    let file = unsafe { std::fs::File::from_raw_fd(received) };
    let fd = file.as_raw_fd();
    fd_proc_path(fd)
}

#[cfg(not(unix))]
fn recv_fetch_dir(_channel: i32) -> Result<PathBuf> {
    Err(SdkError::message(
        "fetch-directory descriptors are not supported on this platform".into(),
    ))
}

#[cfg(unix)]
use std::io;
#[cfg(unix)]
use std::os::fd::AsRawFd;

#[cfg(unix)]
fn recv_one_fd(socket: i32) -> io::Result<i32> {
    let mut byte = [0u8];
    let mut iov = [libc::iovec {
        iov_base: byte.as_mut_ptr().cast(),
        iov_len: 1,
    }];
    let fd_size = std::mem::size_of::<i32>();
    let mut cmsg = vec![0u8; unsafe { libc::CMSG_SPACE(fd_size as u32) as usize }];
    let mut msg = libc::msghdr {
        msg_name: std::ptr::null_mut(),
        msg_namelen: 0,
        msg_iov: iov.as_mut_ptr(),
        msg_iovlen: 1,
        msg_control: cmsg.as_mut_ptr().cast(),
        msg_controllen: cmsg.len() as _,
        msg_flags: 0,
    };

    // SAFETY: msghdr points at stack/local buffers for the duration of the call.
    let received = unsafe { libc::recvmsg(socket, &mut msg, 0) };
    if received < 0 {
        return Err(io::Error::last_os_error());
    }

    // SAFETY: we sized the control buffer with CMSG_SPACE and inspect one header.
    unsafe {
        let hdr = libc::CMSG_FIRSTHDR(&msg);
        if hdr.is_null() {
            return Err(io::Error::other(
                "fetch directory message carried no descriptor",
            ));
        }
        if (*hdr).cmsg_level != libc::SOL_SOCKET || (*hdr).cmsg_type != libc::SCM_RIGHTS {
            return Err(io::Error::other(
                "fetch directory message was not SCM_RIGHTS",
            ));
        }
        let mut fd = 0i32;
        std::ptr::copy_nonoverlapping(
            libc::CMSG_DATA(hdr),
            &mut fd as *mut i32 as *mut u8,
            fd_size,
        );
        if fd < 0 {
            return Err(io::Error::other("fetch directory descriptor was invalid"));
        }
        Ok(fd)
    }
}

#[cfg(unix)]
fn fd_proc_path(fd: i32) -> Result<PathBuf> {
    #[cfg(target_os = "linux")]
    let path = format!("/proc/self/fd/{fd}");
    #[cfg(target_os = "macos")]
    let path = format!("/dev/fd/{fd}");
    #[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
    let path = format!("/dev/fd/{fd}");
    Ok(PathBuf::from(path))
}
