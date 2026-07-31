//! Bounded pool of confined media worker processes.
//!
//! Every codec operation used to run on tokio's blocking thread pool, inside
//! whichever process owned the acquire. That process also holds the master data
//! encryption key and an open handle to `library.db`, and the codecs it links —
//! LAME (`mp3lame-sys`) and FDK-AAC (`fdk-aac-sys`) — are C libraries parsing
//! attacker-influenced audio.
//!
//! Jobs now run in short-lived child processes that confine themselves to the
//! paths their job declared before touching any media. That buys three things
//! at once: the codecs cannot reach the key material, a codec crash fails one
//! book instead of the daemon, and concurrency becomes an explicit bound rather
//! than however many blocking threads tokio happened to grow.
//!
//! Workers are per-job rather than long-lived on purpose. Filesystem
//! confinement is irreversible and process-wide, so a reused worker would need
//! a jail wide enough for every job it might later receive. Media operations
//! run for seconds to minutes, which makes the few milliseconds of process
//! spawn irrelevant next to a tight, per-job allowlist.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, OnceLock};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Semaphore;

use crate::error::{MediaError, Result};
use crate::job::{MediaJob, MediaJobOutput, MediaJobReply};

/// Environment variable naming the worker binary explicitly.
pub const WORKER_BIN_ENV: &str = "BOOKCLERK_MEDIA_WORKER";
/// Environment variable the pool sets on each worker to select its failure mode.
pub const WORKER_ENFORCEMENT_ENV: &str = "BOOKCLERK_MEDIA_WORKER_ENFORCEMENT";
/// File name of the worker binary when it sits beside the host executable.
pub const WORKER_BIN_NAME: &str = "bookclerk-media-worker";

/// How strictly a worker must be confined before it will process a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Confinement {
    /// Refuse to run the job unless the worker confined itself. Production
    /// default: a host that cannot sandbox must not silently decode untrusted
    /// media next to the encryption key.
    #[default]
    Required,
    /// Confine where the platform allows and log what did not engage.
    BestEffort,
    /// Run jobs on a blocking thread in this process. Intended for development
    /// and for the crate's own algorithm tests.
    Off,
}

impl Confinement {
    /// Parse the value carried in [`WORKER_ENFORCEMENT_ENV`].
    #[must_use]
    pub fn from_env_value(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "required" => Some(Self::Required),
            "best-effort" | "best_effort" => Some(Self::BestEffort),
            "off" => Some(Self::Off),
            _ => None,
        }
    }

    /// The value to pass through [`WORKER_ENFORCEMENT_ENV`].
    #[must_use]
    pub fn as_env_value(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::BestEffort => "best-effort",
            Self::Off => "off",
        }
    }
}

/// Pool sizing and isolation settings.
#[derive(Debug, Clone, Default)]
pub struct MediaPoolConfig {
    /// Maximum jobs running at once. Zero means "derive from available
    /// parallelism".
    pub workers: usize,
    /// How strictly workers must be confined.
    pub confinement: Confinement,
    /// Explicit worker binary path. When absent the pool looks at
    /// [`WORKER_BIN_ENV`] and then beside the current executable.
    pub worker_bin: Option<PathBuf>,
}

impl From<&bookclerk_config::MediaConfig> for MediaPoolConfig {
    fn from(config: &bookclerk_config::MediaConfig) -> Self {
        Self {
            workers: config.workers,
            confinement: match config.isolation {
                bookclerk_config::MediaIsolation::Required => Confinement::Required,
                bookclerk_config::MediaIsolation::BestEffort => Confinement::BestEffort,
                bookclerk_config::MediaIsolation::Off => Confinement::Off,
            },
            worker_bin: config.worker_bin.clone(),
        }
    }
}

/// Bounded pool of confined media workers.
#[derive(Debug)]
pub struct MediaPool {
    permits: Arc<Semaphore>,
    workers: usize,
    confinement: Confinement,
    worker_bin: Option<PathBuf>,
}

impl MediaPool {
    /// Build a pool from `config`, resolving the worker binary once.
    #[must_use]
    pub fn new(config: MediaPoolConfig) -> Self {
        let workers = if config.workers == 0 {
            default_worker_count()
        } else {
            config.workers
        };
        let worker_bin = match config.confinement {
            Confinement::Off => None,
            _ => config.worker_bin.or_else(discover_worker_bin),
        };

        if config.confinement != Confinement::Off && worker_bin.is_none() {
            tracing::warn!(
                bin = WORKER_BIN_NAME,
                env = WORKER_BIN_ENV,
                "media worker binary not found; codec work will run in-process \
                 without confinement"
            );
        }

        Self {
            permits: Arc::new(Semaphore::new(workers)),
            workers,
            confinement: config.confinement,
            worker_bin,
        }
    }

    /// A pool that runs every job on a blocking thread in this process.
    #[must_use]
    pub fn in_process() -> Self {
        Self::new(MediaPoolConfig {
            confinement: Confinement::Off,
            ..MediaPoolConfig::default()
        })
    }

    /// Maximum jobs this pool will run at once.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.workers
    }

    /// Whether jobs run in a separate, confined process.
    #[must_use]
    pub fn is_isolated(&self) -> bool {
        self.worker_bin.is_some()
    }

    /// Resolved worker binary, when one was found.
    #[must_use]
    pub fn worker_bin(&self) -> Option<&PathBuf> {
        self.worker_bin.as_ref()
    }

    /// One-line summary for startup logs and `bookclerk doctor`.
    #[must_use]
    pub fn summary(&self) -> String {
        match &self.worker_bin {
            Some(bin) => format!(
                "media pool: {} workers, confinement={}, worker={}",
                self.workers,
                self.confinement.as_env_value(),
                bin.display()
            ),
            None => format!(
                "media pool: {} workers, in-process (no confinement)",
                self.workers
            ),
        }
    }

    /// Run `job`, waiting for a free slot first.
    ///
    /// # Errors
    ///
    /// Returns [`MediaError::Worker`] when the worker cannot be started, exits
    /// without a reply, or returns something unparseable, and propagates the
    /// job's own error otherwise.
    pub async fn run(&self, job: MediaJob) -> Result<MediaJobOutput> {
        let label = job.label();
        job.prepare_output_dirs()?;

        let _permit = self
            .permits
            .acquire()
            .await
            .map_err(|err| MediaError::Worker {
                job: label,
                detail: format!("pool closed: {err}"),
            })?;

        match self.worker_bin.clone() {
            Some(bin) => self.run_in_worker(&bin, job).await,
            None => run_in_process(job).await,
        }
    }

    async fn run_in_worker(&self, bin: &PathBuf, job: MediaJob) -> Result<MediaJobOutput> {
        let label = job.label();
        let request = serde_json::to_vec(&job).map_err(|err| MediaError::Worker {
            job: label,
            detail: format!("could not serialize job: {err}"),
        })?;

        let mut child = tokio::process::Command::new(bin)
            .env(WORKER_ENFORCEMENT_ENV, self.confinement.as_env_value())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|err| MediaError::Worker {
                job: label,
                detail: format!("could not spawn {}: {err}", bin.display()),
            })?;

        // Close stdin after the single request so the worker sees EOF and does
        // not wait for a second job.
        {
            let mut stdin = child.stdin.take().ok_or_else(|| MediaError::Worker {
                job: label,
                detail: "worker stdin unavailable".to_string(),
            })?;
            stdin
                .write_all(&request)
                .await
                .map_err(|err| MediaError::Worker {
                    job: label,
                    detail: format!("could not send job: {err}"),
                })?;
            stdin.shutdown().await.ok();
        }

        let mut stdout = child.stdout.take().ok_or_else(|| MediaError::Worker {
            job: label,
            detail: "worker stdout unavailable".to_string(),
        })?;
        let mut reply = Vec::new();
        stdout
            .read_to_end(&mut reply)
            .await
            .map_err(|err| MediaError::Worker {
                job: label,
                detail: format!("could not read reply: {err}"),
            })?;

        let status = child.wait().await.map_err(|err| MediaError::Worker {
            job: label,
            detail: format!("could not await worker: {err}"),
        })?;

        if reply.is_empty() {
            return Err(MediaError::Worker {
                job: label,
                detail: format!("worker exited with {status} before replying"),
            });
        }

        match serde_json::from_slice::<MediaJobReply>(&reply) {
            Ok(MediaJobReply::Ok(output)) => Ok(output),
            Ok(MediaJobReply::Err { message }) => Err(MediaError::Native(message)),
            Err(err) => Err(MediaError::Worker {
                job: label,
                detail: format!(
                    "unparseable reply ({err}); worker exited with {status}: {}",
                    String::from_utf8_lossy(&reply).trim()
                ),
            }),
        }
    }
}

impl Default for MediaPool {
    fn default() -> Self {
        Self::new(MediaPoolConfig::default())
    }
}

async fn run_in_process(job: MediaJob) -> Result<MediaJobOutput> {
    let label = job.label();
    tokio::task::spawn_blocking(move || job.run())
        .await
        .map_err(|err| MediaError::Worker {
            job: label,
            detail: format!("in-process job panicked: {err}"),
        })?
}

/// Default concurrency: one job per core, capped so a large machine does not
/// spawn dozens of memory-hungry encoders at once.
fn default_worker_count() -> usize {
    const MAX_DEFAULT_WORKERS: usize = 8;
    std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1)
        .clamp(1, MAX_DEFAULT_WORKERS)
}

/// Find the worker binary: explicit env override first, then beside the current
/// executable, which covers both an installed layout and `target/debug`.
fn discover_worker_bin() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(WORKER_BIN_ENV) {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    let exe = std::env::current_exe().ok()?;
    let candidate = exe
        .parent()?
        .join(format!("{WORKER_BIN_NAME}{}", std::env::consts::EXE_SUFFIX));
    candidate.is_file().then_some(candidate)
}

static POOL: OnceLock<MediaPool> = OnceLock::new();

/// Install the process-wide pool. Call once during startup, before any acquire.
///
/// # Errors
///
/// Returns the rejected pool when one was already installed.
pub fn init_pool(pool: MediaPool) -> std::result::Result<(), MediaPool> {
    POOL.set(pool)
}

/// Build the pool from `[media]` config, install it, and log what it will do.
///
/// Safe to call more than once; a second call logs and leaves the first pool in
/// place, since replacing it mid-run would let jobs escape their intended
/// isolation.
pub fn init_pool_from_config(config: &bookclerk_config::MediaConfig) {
    let pool = MediaPool::new(MediaPoolConfig::from(config));
    let summary = pool.summary();
    if init_pool(pool).is_err() {
        tracing::debug!("media pool already installed; keeping the existing one");
        return;
    }
    tracing::info!("{summary}");
}

/// The process-wide pool, creating a default one if startup never installed it.
pub fn pool() -> &'static MediaPool {
    POOL.get_or_init(MediaPool::default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confinement_env_values_round_trip() {
        for mode in [
            Confinement::Required,
            Confinement::BestEffort,
            Confinement::Off,
        ] {
            assert_eq!(
                Confinement::from_env_value(mode.as_env_value()),
                Some(mode),
                "round trip failed for {mode:?}"
            );
        }
        assert_eq!(Confinement::from_env_value("nonsense"), None);
    }

    #[test]
    fn confinement_parsing_is_forgiving_about_case_and_separators() {
        assert_eq!(
            Confinement::from_env_value("  BEST_EFFORT "),
            Some(Confinement::BestEffort)
        );
    }

    #[test]
    fn default_worker_count_is_bounded() {
        let count = default_worker_count();
        assert!((1..=8).contains(&count), "unexpected worker count {count}");
    }

    #[test]
    fn in_process_pool_reports_no_isolation() {
        let pool = MediaPool::in_process();
        assert!(!pool.is_isolated());
        assert!(pool.worker_bin().is_none());
        assert!(pool.summary().contains("in-process"));
    }

    #[test]
    fn explicit_worker_count_is_respected() {
        let pool = MediaPool::new(MediaPoolConfig {
            workers: 3,
            confinement: Confinement::Off,
            worker_bin: None,
        });
        assert_eq!(pool.capacity(), 3);
    }
}
