//! Opt-in diagnostics: recent-log ring buffer, crash + error-burst upload.

use std::collections::VecDeque;
use std::panic::{self, PanicHookInfo};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

use crate::redact::RedactingVisitor;
use crate::settings::DiagnosticsConfig;

/// Snapshot of a single redacted log event kept for crash / error uploads.
#[derive(Debug, Clone, Serialize)]
pub struct BufferedEvent {
    pub ts_unix_ms: u64,
    pub level: String,
    pub target: String,
    pub message: String,
    pub fields: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize)]
struct UploadPayload {
    trigger: String,
    version: String,
    os: String,
    archived_at_unix_ms: u64,
    events: Vec<BufferedEvent>,
}

struct RingState {
    events: VecDeque<BufferedEvent>,
    capacity: usize,
    error_timestamps: VecDeque<Instant>,
    last_upload: Option<Instant>,
}

impl RingState {
    fn new(capacity: usize) -> Self {
        Self {
            events: VecDeque::with_capacity(capacity.min(512)),
            capacity: capacity.max(1),
            error_timestamps: VecDeque::new(),
            last_upload: None,
        }
    }

    fn push(&mut self, event: BufferedEvent) {
        if self.events.len() >= self.capacity {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    fn snapshot(&self) -> Vec<BufferedEvent> {
        self.events.iter().cloned().collect()
    }
}

/// Process-global diagnostics handle installed by [`crate::logging::init_tracing_with`].
#[derive(Clone)]
pub struct DiagnosticsHandle {
    inner: Arc<DiagnosticsInner>,
}

struct DiagnosticsInner {
    config: DiagnosticsConfig,
    version: String,
    ring: Mutex<RingState>,
    upload_in_flight: AtomicBool,
    /// Cooldown between automatic uploads (seconds).
    upload_cooldown_secs: u64,
    /// Monotonic counter of successful/attempted uploads (tests / status).
    uploads_attempted: AtomicU64,
}

impl DiagnosticsHandle {
    pub(crate) fn new(config: DiagnosticsConfig, version: impl Into<String>) -> Self {
        let capacity = config.ring_buffer_capacity.max(1) as usize;
        Self {
            inner: Arc::new(DiagnosticsInner {
                config,
                version: version.into(),
                ring: Mutex::new(RingState::new(capacity)),
                upload_in_flight: AtomicBool::new(false),
                upload_cooldown_secs: 300,
                uploads_attempted: AtomicU64::new(0),
            }),
        }
    }

    /// Whether automatic upload is configured and enabled.
    #[must_use]
    pub fn upload_enabled(&self) -> bool {
        self.inner.config.upload_enabled && !self.inner.config.upload_url.trim().is_empty()
    }

    /// Number of upload attempts since init (successful or failed HTTP tries).
    #[must_use]
    pub fn uploads_attempted(&self) -> u64 {
        self.inner.uploads_attempted.load(Ordering::Relaxed)
    }

    /// Record a redacted event into the ring buffer and maybe trigger an error-burst upload.
    pub fn record_event(&self, event: &Event<'_>) {
        let meta = event.metadata();
        let mut visitor = RedactingVisitor::default();
        event.record(&mut visitor);

        let buffered = BufferedEvent {
            ts_unix_ms: unix_now_ms(),
            level: meta.level().to_string(),
            target: meta.target().to_string(),
            message: visitor.message.unwrap_or_default(),
            fields: visitor.fields,
        };

        let is_error = *meta.level() == Level::ERROR;
        let should_upload_burst = {
            let mut guard = self.inner.ring.lock().unwrap_or_else(|e| e.into_inner());
            guard.push(buffered);
            if is_error && self.inner.config.upload_on_error_burst && self.upload_enabled() {
                note_error_and_check_burst(&mut guard, &self.inner.config)
            } else {
                false
            }
        };

        if should_upload_burst {
            self.spawn_upload("error_burst");
        }
    }

    /// Best-effort upload of the current ring buffer (blocking). Used by panic hook + tests.
    pub fn upload_blocking(&self, trigger: &str) {
        if !self.upload_enabled() {
            return;
        }
        if self
            .inner
            .upload_in_flight
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }

        let events = {
            let guard = self.inner.ring.lock().unwrap_or_else(|e| e.into_inner());
            guard.snapshot()
        };

        let payload = UploadPayload {
            trigger: trigger.to_string(),
            version: self.inner.version.clone(),
            os: std::env::consts::OS.to_string(),
            archived_at_unix_ms: unix_now_ms(),
            events,
        };

        // Defense in depth: re-scrub serialized JSON before send.
        let body = match serde_json::to_string(&payload) {
            Ok(s) => crate::redact::redact_str(&s),
            Err(_) => {
                self.inner.upload_in_flight.store(false, Ordering::SeqCst);
                return;
            }
        };

        self.inner.uploads_attempted.fetch_add(1, Ordering::Relaxed);
        let url = self.inner.config.upload_url.trim().to_string();
        let result = ureq::post(&url)
            .set("Content-Type", "application/json")
            .set(
                "User-Agent",
                &format!("libation-diagnostics/{}", self.inner.version),
            )
            .timeout(Duration::from_secs(10))
            .send_string(&body);

        if let Ok(mut guard) = self.inner.ring.lock() {
            guard.last_upload = Some(Instant::now());
        }
        self.inner.upload_in_flight.store(false, Ordering::SeqCst);

        // Avoid recursive tracing from the upload path when possible; swallow errors.
        let _ = result;
    }

    fn spawn_upload(&self, trigger: &'static str) {
        if !self.upload_enabled() {
            return;
        }
        // Honor cooldown.
        {
            let guard = self.inner.ring.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(last) = guard.last_upload {
                if last.elapsed() < Duration::from_secs(self.inner.upload_cooldown_secs) {
                    return;
                }
            }
        }
        let handle = self.clone();
        let _ = std::thread::Builder::new()
            .name("libation-diag-upload".into())
            .spawn(move || handle.upload_blocking(trigger));
    }
}

fn note_error_and_check_burst(state: &mut RingState, config: &DiagnosticsConfig) -> bool {
    let now = Instant::now();
    let window = Duration::from_secs(config.error_burst_window_secs.max(1));
    state.error_timestamps.push_back(now);
    while state
        .error_timestamps
        .front()
        .is_some_and(|t| now.duration_since(*t) > window)
    {
        state.error_timestamps.pop_front();
    }
    let threshold = config.error_burst_threshold.max(1) as usize;
    if state.error_timestamps.len() < threshold {
        return false;
    }
    if let Some(last) = state.last_upload {
        // Same cooldown as spawn_upload; avoid stampeding.
        if last.elapsed() < Duration::from_secs(300) {
            return false;
        }
    }
    // Reset window so we don't immediately re-trigger.
    state.error_timestamps.clear();
    true
}

fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Tracing layer that feeds the diagnostics ring buffer.
pub struct DiagnosticsLayer {
    handle: DiagnosticsHandle,
}

impl DiagnosticsLayer {
    #[must_use]
    pub fn new(handle: DiagnosticsHandle) -> Self {
        Self { handle }
    }
}

impl<S> Layer<S> for DiagnosticsLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        self.handle.record_event(event);
    }
}

static GLOBAL_DIAGNOSTICS: OnceLock<DiagnosticsHandle> = OnceLock::new();

/// Install (or replace is not allowed — first wins) the process-global diagnostics handle
/// and a panic hook that uploads when configured.
pub fn install_global(handle: DiagnosticsHandle) {
    let _ = GLOBAL_DIAGNOSTICS.set(handle.clone());
    if handle.upload_enabled() && handle.inner.config.upload_on_crash {
        install_panic_hook();
    }
}

/// Test / status accessor for the process-global diagnostics handle.
#[must_use]
#[allow(dead_code)] // Used by binaries / future status endpoints.
pub fn global() -> Option<&'static DiagnosticsHandle> {
    GLOBAL_DIAGNOSTICS.get()
}

fn install_panic_hook() {
    static HOOK_SET: AtomicBool = AtomicBool::new(false);
    if HOOK_SET
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    let prev = panic::take_hook();
    panic::set_hook(Box::new(move |info: &PanicHookInfo<'_>| {
        // Record a synthetic redacted panic line into the buffer, then upload.
        if let Some(handle) = GLOBAL_DIAGNOSTICS.get() {
            let msg = panic_message(info);
            let redacted = crate::redact::redact_str(&msg);
            if let Ok(mut guard) = handle.inner.ring.lock() {
                guard.push(BufferedEvent {
                    ts_unix_ms: unix_now_ms(),
                    level: "ERROR".into(),
                    target: "libation::panic".into(),
                    message: redacted,
                    fields: vec![],
                });
            }
            handle.upload_blocking("crash");
        }
        prev(info);
    }));
}

fn panic_message(info: &PanicHookInfo<'_>) -> String {
    if let Some(s) = info.payload().downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = info.payload().downcast_ref::<String>() {
        s.clone()
    } else {
        "box dyn Any".to_string()
    }
}

#[cfg(test)]
pub fn push_test_event(handle: &DiagnosticsHandle, level: &str, message: &str) {
    use crate::redact::redact_field_value;
    let mut guard = handle.inner.ring.lock().unwrap();
    guard.push(BufferedEvent {
        ts_unix_ms: unix_now_ms(),
        level: level.into(),
        target: "test".into(),
        message: redact_field_value("message", message),
        fields: vec![],
    });
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_caps_capacity() {
        let handle = DiagnosticsHandle::new(
            DiagnosticsConfig {
                ring_buffer_capacity: 3,
                ..DiagnosticsConfig::default()
            },
            "0.0.0-test",
        );
        for i in 0..5 {
            push_test_event(&handle, "INFO", &format!("msg-{i}"));
        }
        let snap = handle.inner.ring.lock().unwrap().snapshot();
        assert_eq!(snap.len(), 3);
        assert_eq!(snap[0].message, "msg-2");
        assert_eq!(snap[2].message, "msg-4");
    }

    #[test]
    fn upload_skipped_when_disabled() {
        let handle = DiagnosticsHandle::new(DiagnosticsConfig::default(), "0.0.0-test");
        push_test_event(&handle, "ERROR", "boom Atna|should-not-upload");
        handle.upload_blocking("test");
        assert_eq!(handle.uploads_attempted(), 0);
    }

    #[test]
    fn upload_posts_redacted_json() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::sync::{Arc, Mutex};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let body = Arc::new(Mutex::new(String::new()));
        let body_thread = Arc::clone(&body);
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut raw = Vec::new();
            let mut buf = [0u8; 1024];
            loop {
                let n = stream.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    break;
                }
                raw.extend_from_slice(&buf[..n]);
                if let Some(header_end) = find_header_end(&raw) {
                    let headers = std::str::from_utf8(&raw[..header_end]).unwrap_or("");
                    let content_len = headers.lines().find_map(|line| {
                        let line = line.trim();
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                    });
                    if let Some(len) = content_len {
                        if raw.len() >= header_end + len {
                            break;
                        }
                    } else if n < buf.len() {
                        break;
                    }
                }
            }
            *body_thread.lock().unwrap() = String::from_utf8_lossy(&raw).into_owned();
            let _ = stream.write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n");
        });

        let handle = DiagnosticsHandle::new(
            DiagnosticsConfig {
                upload_enabled: true,
                upload_url: format!("http://{addr}/diag"),
                ..DiagnosticsConfig::default()
            },
            "0.0.0-test",
        );
        push_test_event(
            &handle,
            "ERROR",
            "refresh failed Atna|leak-me-please and Bearer abc.def.ghi",
        );
        handle.upload_blocking("test");
        server.join().unwrap();

        let captured = body.lock().unwrap().clone();
        assert!(
            captured.contains("POST"),
            "expected HTTP POST, got: {captured}"
        );
        assert!(
            !captured.contains("Atna|leak-me-please"),
            "secret leaked: {captured}"
        );
        assert!(
            !captured.contains("Bearer abc.def.ghi"),
            "secret leaked: {captured}"
        );
        assert!(
            captured.contains("[REDACTED]"),
            "expected redaction marker in body: {captured}"
        );
        assert_eq!(handle.uploads_attempted(), 1);
    }

    fn find_header_end(raw: &[u8]) -> Option<usize> {
        raw.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4)
    }
}
