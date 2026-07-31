//! Portable, unprivileged process confinement.
//!
//! Bookclerk confines code by what it may touch, not by which uid it runs as.
//! Every backend here works without root, without setuid helpers, and without
//! user namespaces or bind mounts:
//!
//! - **Linux** — Landlock filesystem allowlist plus a seccomp-bpf deny list.
//! - **macOS** — Seatbelt (`sandbox_init`) with a deny-default SBPL profile.
//! - **Windows** — AppContainer applied at `CreateProcess`; see
//!   [`spawn`](crate::spawn). A process cannot confine *itself* on Windows, so
//!   [`Policy::confine_current_process`] reports the filesystem layer as
//!   [`LayerStatus::Unsupported`] there.
//!
//! Callers pick the failure mode with [`Enforcement`]. `Required` turns a
//! backend that cannot enforce into an error, which is what production paths
//! use so a missing jail can never pass silently.

use std::fmt;
use std::path::{Path, PathBuf};

mod platform;

pub use platform::BACKEND;

/// What to do when a confinement layer cannot be enforced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Enforcement {
    /// Fail the call when any requested layer does not engage.
    #[default]
    Required,
    /// Apply what the host supports and report the rest as unsupported.
    BestEffort,
    /// Apply nothing. Used only by the documented opt-out.
    Disabled,
}

/// Network reachability granted to the confined process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NetPolicy {
    /// No IP sockets at all. Media workers only touch local files.
    #[default]
    Deny,
    /// Outbound connections allowed, inbound listeners refused. Storefront
    /// plugins fetch over HTTPS but never need to listen.
    Outbound,
    /// Unrestricted. The daemon binds its own control-plane listener.
    Full,
}

/// A confinement request: an allowlist of paths plus a few coarse switches.
///
/// Paths that do not exist when the policy is applied are skipped, because a
/// Landlock rule on a missing path is an error rather than a wider grant.
#[derive(Debug, Clone)]
pub struct Policy {
    label: String,
    reads: Vec<PathBuf>,
    writes: Vec<PathBuf>,
    net: NetPolicy,
    allow_exec: bool,
    system_paths: bool,
    enforcement: Enforcement,
}

impl Policy {
    /// Start a deny-default policy. `label` appears in diagnostics only.
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            reads: Vec::new(),
            writes: Vec::new(),
            net: NetPolicy::Deny,
            allow_exec: false,
            system_paths: true,
            enforcement: Enforcement::Required,
        }
    }

    /// Grant read access to `path` and everything beneath it.
    #[must_use]
    pub fn read(mut self, path: impl Into<PathBuf>) -> Self {
        self.reads.push(path.into());
        self
    }

    /// Grant read and write access to `path` and everything beneath it.
    #[must_use]
    pub fn write(mut self, path: impl Into<PathBuf>) -> Self {
        self.writes.push(path.into());
        self
    }

    /// Grant read access to each of `paths`.
    #[must_use]
    pub fn reads<I, P>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        self.reads.extend(paths.into_iter().map(Into::into));
        self
    }

    /// Grant read and write access to each of `paths`.
    #[must_use]
    pub fn writes<I, P>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        self.writes.extend(paths.into_iter().map(Into::into));
        self
    }

    /// Set network reachability. Defaults to [`NetPolicy::Deny`].
    #[must_use]
    pub fn net(mut self, net: NetPolicy) -> Self {
        self.net = net;
        self
    }

    /// Allow `execve` of binaries inside the read allowlist. Defaults to false.
    #[must_use]
    pub fn allow_exec(mut self, allow: bool) -> Self {
        self.allow_exec = allow;
        self
    }

    /// Include the platform's read-only system set (shared libraries, the CA
    /// bundle, resolver configuration). Defaults to true; a worker that needs
    /// nothing but its own scratch directory can turn it off.
    #[must_use]
    pub fn system_paths(mut self, include: bool) -> Self {
        self.system_paths = include;
        self
    }

    /// Set the failure mode. Defaults to [`Enforcement::Required`].
    #[must_use]
    pub fn enforcement(mut self, enforcement: Enforcement) -> Self {
        self.enforcement = enforcement;
        self
    }

    /// Diagnostics label.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Read allowlist, including the platform system set when enabled.
    ///
    /// Missing paths are filtered out.
    #[must_use]
    pub fn resolved_reads(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = Vec::new();
        if self.system_paths {
            out.extend(
                platform::system_read_paths()
                    .iter()
                    .map(PathBuf::from)
                    .filter(|p| p.exists()),
            );
        }
        out.extend(self.reads.iter().filter(|p| p.exists()).cloned());
        dedupe(out)
    }

    /// Write allowlist. Missing paths are filtered out.
    #[must_use]
    pub fn resolved_writes(&self) -> Vec<PathBuf> {
        dedupe(self.writes.iter().filter(|p| p.exists()).cloned().collect())
    }

    /// Network policy.
    #[must_use]
    pub fn net_policy(&self) -> NetPolicy {
        self.net
    }

    /// Whether `execve` is permitted.
    #[must_use]
    pub fn exec_allowed(&self) -> bool {
        self.allow_exec
    }

    /// Failure mode.
    #[must_use]
    pub fn enforcement_mode(&self) -> Enforcement {
        self.enforcement
    }

    /// Confine the calling process. Restrictions are irreversible and are
    /// inherited by children, which can only narrow them further.
    ///
    /// Call this at the top of `main`, before spawning threads or a runtime.
    /// It allocates, so it is *not* async-signal-safe and must not be used from
    /// `Command::pre_exec` in a threaded parent. Child processes we control
    /// confine themselves at startup instead, which closes the window between
    /// `exec` and the jail taking effect.
    ///
    /// # Errors
    ///
    /// Returns [`SandboxError::NotEnforced`] when [`Enforcement::Required`] was
    /// requested and a layer did not engage, and [`SandboxError::Backend`] when
    /// a backend call fails outright.
    pub fn confine_current_process(&self) -> Result<Report, SandboxError> {
        if self.enforcement == Enforcement::Disabled {
            return Ok(Report::disabled(&self.label));
        }
        let report = platform::confine_current_process(self)?;
        if self.enforcement == Enforcement::Required {
            report.require_enforced()?;
        }
        Ok(report)
    }
}

fn dedupe(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::with_capacity(paths.len());
    for path in paths {
        if !out.contains(&path) {
            out.push(path);
        }
    }
    out
}

/// Outcome for one confinement layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayerStatus {
    /// The layer engaged fully.
    Enforced,
    /// The layer engaged, but the host does not support every requested
    /// restriction. Carries what was lost.
    Partial(String),
    /// The host has no mechanism for this layer.
    Unsupported(String),
    /// The policy did not ask for this layer.
    NotRequested,
}

impl LayerStatus {
    /// Whether this layer is providing protection.
    #[must_use]
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Enforced | Self::Partial(_))
    }
}

impl fmt::Display for LayerStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Enforced => write!(f, "enforced"),
            Self::Partial(detail) => write!(f, "partial ({detail})"),
            Self::Unsupported(detail) => write!(f, "unsupported ({detail})"),
            Self::NotRequested => write!(f, "not requested"),
        }
    }
}

/// What actually engaged when a [`Policy`] was applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The policy's diagnostics label.
    pub label: String,
    /// Backend name, e.g. `landlock+seccomp`.
    pub backend: &'static str,
    /// Filesystem allowlist status.
    pub filesystem: LayerStatus,
    /// Syscall restriction status.
    pub syscall: LayerStatus,
    /// Network restriction status.
    pub network: LayerStatus,
}

impl Report {
    fn disabled(label: &str) -> Self {
        Self {
            label: label.to_string(),
            backend: "disabled",
            filesystem: LayerStatus::NotRequested,
            syscall: LayerStatus::NotRequested,
            network: LayerStatus::NotRequested,
        }
    }

    /// Whether the filesystem allowlist — the layer that protects `master.key`
    /// and `library.db` — is active.
    #[must_use]
    pub fn is_confined(&self) -> bool {
        self.filesystem.is_active()
    }

    fn require_enforced(&self) -> Result<(), SandboxError> {
        for (layer, status) in [
            ("filesystem", &self.filesystem),
            ("syscall", &self.syscall),
            ("network", &self.network),
        ] {
            if let LayerStatus::Unsupported(detail) = status {
                return Err(SandboxError::NotEnforced {
                    label: self.label.clone(),
                    layer,
                    detail: detail.clone(),
                });
            }
        }
        Ok(())
    }

    /// One-line summary for startup logs and `bookclerk doctor`.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} [{}]: filesystem={}, syscall={}, network={}",
            self.label, self.backend, self.filesystem, self.syscall, self.network
        )
    }
}

/// What this host can enforce, without applying anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    /// Backend name for this platform.
    pub backend: &'static str,
    /// Whether a filesystem allowlist is available.
    pub filesystem: bool,
    /// Whether syscall filtering is available.
    pub syscall: bool,
    /// Whether network restriction is available.
    pub network: bool,
    /// Human-readable detail, e.g. the Landlock ABI level found.
    pub detail: String,
}

/// Probe what this host supports. Applies nothing; safe to call at any time.
#[must_use]
pub fn capabilities() -> Capabilities {
    platform::capabilities()
}

/// Confinement failure.
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    /// A required layer did not engage.
    #[error("{label}: {layer} confinement not enforced ({detail})")]
    NotEnforced {
        /// Policy label.
        label: String,
        /// Which layer failed.
        layer: &'static str,
        /// Why it failed.
        detail: String,
    },
    /// A backend call failed.
    #[error("{label}: {backend} failed: {detail}")]
    Backend {
        /// Policy label.
        label: String,
        /// Backend name.
        backend: &'static str,
        /// Underlying error.
        detail: String,
    },
}

impl SandboxError {
    pub(crate) fn backend(label: &str, detail: impl fmt::Display) -> Self {
        Self::Backend {
            label: label.to_string(),
            backend: BACKEND,
            detail: detail.to_string(),
        }
    }
}

/// Resolve a path for use in an allowlist, creating it if absent.
///
/// Landlock and Seatbelt both need the path to exist when the rule is added, so
/// callers that will write into a scratch directory should create it first.
///
/// # Errors
///
/// Propagates directory-creation failures.
pub fn ensure_dir(path: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(path)?;
    // Canonicalize so a symlinked allowlist entry matches the resolved path the
    // kernel sees; a rule on the link alone would not cover the target.
    std::fs::canonicalize(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_enforcement_short_circuits() {
        let report = Policy::new("test")
            .enforcement(Enforcement::Disabled)
            .confine_current_process()
            .expect("disabled policy always succeeds");
        assert_eq!(report.backend, "disabled");
        assert!(!report.is_confined());
    }

    #[test]
    fn missing_paths_are_filtered_out() {
        let policy = Policy::new("test")
            .system_paths(false)
            .read("/definitely/not/a/real/path/for/bookclerk")
            .write("/also/not/real");
        assert!(policy.resolved_reads().is_empty());
        assert!(policy.resolved_writes().is_empty());
    }

    #[test]
    fn existing_paths_survive_resolution() {
        let dir = tempfile::tempdir().expect("tempdir");
        let policy = Policy::new("test").system_paths(false).write(dir.path());
        assert_eq!(policy.resolved_writes(), vec![dir.path().to_path_buf()]);
    }

    #[test]
    fn duplicate_paths_collapse() {
        let dir = tempfile::tempdir().expect("tempdir");
        let policy = Policy::new("test")
            .system_paths(false)
            .write(dir.path())
            .write(dir.path());
        assert_eq!(policy.resolved_writes().len(), 1);
    }

    #[test]
    fn required_enforcement_rejects_unsupported_layer() {
        let report = Report {
            label: "test".into(),
            backend: "none",
            filesystem: LayerStatus::Unsupported("no backend".into()),
            syscall: LayerStatus::NotRequested,
            network: LayerStatus::NotRequested,
        };
        let err = report.require_enforced().expect_err("should reject");
        assert!(matches!(
            err,
            SandboxError::NotEnforced {
                layer: "filesystem",
                ..
            }
        ));
    }

    #[test]
    fn capabilities_probe_does_not_panic() {
        let caps = capabilities();
        assert!(!caps.backend.is_empty());
    }
}
