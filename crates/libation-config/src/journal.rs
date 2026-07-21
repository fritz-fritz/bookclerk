//! OS log facility sink — journald on Linux when the socket is available.

use std::io::{self, Write};

#[cfg(unix)]
use std::os::unix::net::UnixDatagram;

use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

use crate::redact::RedactingVisitor;

#[cfg(unix)]
const JOURNALD_PATH: &str = "/run/systemd/journal/socket";

/// Structured sink that writes redacted events to journald when possible.
///
/// Construction fails (or yields a disabled layer) when the journal socket is
/// absent — callers should fall back to stderr-only logging. Libation does
/// **not** manage log files or rotation; journald/container runtimes own that.
pub struct JournaldLayer {
    #[cfg(unix)]
    socket: UnixDatagram,
    syslog_identifier: String,
}

impl JournaldLayer {
    /// Connect to the journald native socket. Returns `Err` when unavailable.
    pub fn new(syslog_identifier: impl Into<String>) -> io::Result<Self> {
        #[cfg(unix)]
        {
            let socket = UnixDatagram::unbound()?;
            let layer = Self {
                socket,
                syslog_identifier: syslog_identifier.into(),
            };
            // Empty payload probes reachability; journald discards it.
            layer.send_payload(&[])?;
            Ok(layer)
        }
        #[cfg(not(unix))]
        {
            let _ = syslog_identifier;
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "journald is not available on this platform",
            ))
        }
    }

    #[cfg(unix)]
    fn send_payload(&self, payload: &[u8]) -> io::Result<()> {
        self.socket.send_to(payload, JOURNALD_PATH).map(|_| ())
    }

    #[cfg(not(unix))]
    fn send_payload(&self, _payload: &[u8]) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "journald not supported",
        ))
    }
}

impl<S> Layer<S> for JournaldLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let mut visitor = RedactingVisitor::default();
        event.record(&mut visitor);

        let mut buf = Vec::with_capacity(256);
        put_priority(&mut buf, meta.level());
        put_wellformed(&mut buf, "TARGET", meta.target().as_bytes());
        put_length_encoded(&mut buf, "SYSLOG_IDENTIFIER", |b| {
            let _ = write!(b, "{}", self.syslog_identifier);
        });
        if let Some(file) = meta.file() {
            put_wellformed(&mut buf, "CODE_FILE", file.as_bytes());
        }
        if let Some(line) = meta.line() {
            use std::io::Write;
            let _ = writeln!(buf, "CODE_LINE={line}");
        }

        let message = visitor.message.unwrap_or_default();
        put_length_encoded(&mut buf, "MESSAGE", |b| {
            b.extend_from_slice(message.as_bytes());
        });

        for (name, value) in visitor.fields {
            // Prefix user fields to avoid colliding with journald well-known names.
            let field_name = if name.eq_ignore_ascii_case("message") {
                "MESSAGE".to_string()
            } else {
                format!("F_{}", name)
            };
            put_length_encoded(&mut buf, &field_name, |b| {
                b.extend_from_slice(value.as_bytes());
            });
        }

        let _ = self.send_payload(&buf);
    }
}

fn put_priority(buf: &mut Vec<u8>, level: &tracing::Level) {
    let code: u8 = match *level {
        tracing::Level::ERROR => 3,
        tracing::Level::WARN => 4,
        tracing::Level::INFO => 5,
        tracing::Level::DEBUG => 6,
        tracing::Level::TRACE => 7,
    };
    put_wellformed(buf, "PRIORITY", &[code]);
}

fn put_wellformed(buf: &mut Vec<u8>, name: &str, value: &[u8]) {
    buf.extend_from_slice(name.as_bytes());
    buf.push(b'\n');
    buf.extend_from_slice(&(value.len() as u64).to_le_bytes());
    buf.extend_from_slice(value);
    buf.push(b'\n');
}

fn put_length_encoded(buf: &mut Vec<u8>, name: &str, write_value: impl FnOnce(&mut Vec<u8>)) {
    sanitize_name(name, buf);
    buf.push(b'\n');
    buf.extend_from_slice(&[0; 8]);
    let start = buf.len();
    write_value(buf);
    let end = buf.len();
    buf[start - 8..start].copy_from_slice(&((end - start) as u64).to_le_bytes());
    buf.push(b'\n');
}

fn sanitize_name(name: &str, buf: &mut Vec<u8>) {
    buf.extend(
        name.bytes()
            .map(|c| if c == b'.' { b'_' } else { c })
            .skip_while(|&c| c == b'_')
            .filter(|&c| c == b'_' || char::from(c).is_ascii_alphanumeric())
            .map(|c| char::from(c).to_ascii_uppercase() as u8),
    );
}

/// Probe whether journald appears reachable (does not install a subscriber).
#[must_use]
pub fn journald_available() -> bool {
    JournaldLayer::new("libation-probe").is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_uppercases_and_strips() {
        let mut buf = Vec::new();
        sanitize_name("foo.bar-baz", &mut buf);
        assert_eq!(std::str::from_utf8(&buf).unwrap(), "FOO_BARBAZ");
    }
}
