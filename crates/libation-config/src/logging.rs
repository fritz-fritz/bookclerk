//! Tracing / logging initialization.
//!
//! Sinks:
//! - **stderr** (always): text or JSON, with mandatory secret redaction
//! - **journald** (Linux, when `/run/systemd/journal/socket` is reachable):
//!   structured fields, also redacted — operators use `journalctl`; Libation does
//!   not manage log files or rotation
//! - **diagnostics ring buffer** (always): feeds opt-in crash / error-burst upload

use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use crate::diagnostics::{self, DiagnosticsHandle, DiagnosticsLayer};
use crate::journal::JournaldLayer;
use crate::redact::RedactingWriter;
use crate::settings::DiagnosticsConfig;

/// Output format for the stderr sink.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogFormat {
    /// Human-oriented text (CLI default).
    #[default]
    Text,
    /// Structured JSON (daemon default / log aggregators).
    Json,
}

/// Options for [`init_tracing_with`].
#[derive(Debug, Clone)]
pub struct TracingOptions {
    /// Stderr format (text vs JSON).
    pub format: LogFormat,
    /// Default `EnvFilter` directive when `LIBATION_LOG` / `RUST_LOG` are unset.
    pub default_level: String,
    /// `SYSLOG_IDENTIFIER` for journald (`journalctl -t …`).
    pub syslog_identifier: String,
    /// Opt-in crash / error-burst upload settings.
    pub diagnostics: DiagnosticsConfig,
    /// Package version embedded in diagnostics uploads.
    pub version: String,
    /// Attempt to attach a journald sink (no-op when the socket is missing).
    pub enable_journald: bool,
}

impl Default for TracingOptions {
    fn default() -> Self {
        Self {
            format: LogFormat::Text,
            default_level: String::from("libation=info,warn"),
            syslog_identifier: String::from("libation"),
            diagnostics: DiagnosticsConfig::default(),
            version: String::from(env!("CARGO_PKG_VERSION")),
            enable_journald: true,
        }
    }
}

/// Result of installing the global subscriber.
#[derive(Clone)]
pub struct LoggingHandle {
    /// Diagnostics ring buffer / upload handle.
    pub diagnostics: DiagnosticsHandle,
    /// Whether a journald sink was attached.
    pub journald: bool,
}

/// Install a global tracing subscriber (stderr + optional journald + diagnostics).
///
/// Filter precedence: `LIBATION_LOG` → `RUST_LOG` → `opts.default_level`.
///
/// Secrets are redacted on every sink. See [`crate::redact`].
pub fn init_tracing_with(opts: TracingOptions) -> LoggingHandle {
    let filter = EnvFilter::try_from_env("LIBATION_LOG")
        .or_else(|_| EnvFilter::try_from_default_env())
        .unwrap_or_else(|_| EnvFilter::new(opts.default_level));

    let diag_handle = DiagnosticsHandle::new(opts.diagnostics.clone(), opts.version.clone());
    diagnostics::install_global(diag_handle.clone());
    if opts.diagnostics.share_reports {
        eprintln!(
            "libation: diagnostics.share_reports=true — redacted crash/ERROR reports will POST to {}",
            opts.diagnostics.effective_collector_url()
        );
    }

    let journald = if opts.enable_journald {
        JournaldLayer::new(opts.syslog_identifier.clone()).ok()
    } else {
        None
    };
    let journald_active = journald.is_some();

    let diag_layer = DiagnosticsLayer::new(diag_handle.clone());

    // Build and init. `try_init` ignores a second install (tests / nested tools).
    let result = match (opts.format, journald) {
        (LogFormat::Text, Some(journal)) => {
            let fmt_layer = fmt::layer()
                .with_target(false)
                .with_writer(|| RedactingWriter::new(std::io::stderr()));
            tracing_subscriber::registry()
                .with(filter)
                .with(diag_layer)
                .with(journal)
                .with(fmt_layer)
                .try_init()
        }
        (LogFormat::Text, None) => {
            let fmt_layer = fmt::layer()
                .with_target(false)
                .with_writer(|| RedactingWriter::new(std::io::stderr()));
            tracing_subscriber::registry()
                .with(filter)
                .with(diag_layer)
                .with(fmt_layer)
                .try_init()
        }
        (LogFormat::Json, Some(journal)) => {
            let fmt_layer = fmt::layer()
                .json()
                .with_current_span(true)
                .with_writer(|| RedactingWriter::new(std::io::stderr()));
            tracing_subscriber::registry()
                .with(filter)
                .with(diag_layer)
                .with(journal)
                .with(fmt_layer)
                .try_init()
        }
        (LogFormat::Json, None) => {
            let fmt_layer = fmt::layer()
                .json()
                .with_current_span(true)
                .with_writer(|| RedactingWriter::new(std::io::stderr()));
            tracing_subscriber::registry()
                .with(filter)
                .with(diag_layer)
                .with(fmt_layer)
                .try_init()
        }
    };
    let _ = result;

    LoggingHandle {
        diagnostics: diag_handle,
        journald: journald_active,
    }
}

/// Convenience wrapper matching the historical CLI/daemon call site.
///
/// Uses default diagnostics (upload disabled) and enables journald when available.
pub fn init_tracing(format: LogFormat, default_level: &str) -> LoggingHandle {
    init_tracing_with(TracingOptions {
        format,
        default_level: default_level.to_string(),
        ..TracingOptions::default()
    })
}
