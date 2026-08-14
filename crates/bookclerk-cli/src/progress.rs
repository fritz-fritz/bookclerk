//! Terminal progress helpers (LibationCli-style bars when stderr is a TTY).

use std::io::{self, IsTerminal, Write};
use std::time::Instant;

/// True when interactive progress output is appropriate (stderr TTY, not redirected).
#[must_use]
pub fn is_interactive() -> bool {
    io::stderr().is_terminal()
}

/// Simple batch progress: `[####------] 2/5 title`.
pub struct BatchProgress {
    /// Holds the `total` value (`usize`) for this type.
    total: usize,
    /// Holds the `current` value (`usize`) for this type.
    current: usize,
    /// Holds the `label` value (`String`) for this type.
    label: String,
    /// Holds the `started` value (`Instant`) for this type.
    started: Instant,
    /// Holds the `enabled` value (`bool`) for this type.
    enabled: bool,
}

impl BatchProgress {
    #[must_use]
    /// Constructs a new value for the enclosing type.
    pub fn new(total: usize, label: impl Into<String>) -> Self {
        Self {
            total: total.max(1),
            current: 0,
            label: label.into(),
            started: Instant::now(),
            enabled: is_interactive(),
        }
    }

    /// Internal `set` helper used by this module.
    pub fn set(&mut self, current: usize, detail: &str) {
        if !self.enabled {
            return;
        }
        self.current = current;
        let width = 20usize;
        let frac = (current as f64 / self.total as f64).clamp(0.0, 1.0);
        let filled = (frac * width as f64).round() as usize;
        let bar: String = (0..width)
            .map(|i| if i < filled { '#' } else { '-' })
            .collect();
        let eta = if current > 0 && current < self.total {
            let elapsed = self.started.elapsed().as_secs_f64();
            let per = elapsed / current as f64;
            let remain = per * (self.total - current) as f64;
            format!(" {:.1} min remaining", remain / 60.0)
        } else {
            String::new()
        };
        eprint!(
            "\r[{bar}] {current}/{total} {label}: {detail}{eta}   ",
            total = self.total,
            label = self.label,
            detail = detail,
            eta = eta,
        );
        let _ = io::stderr().flush();
    }

    /// Internal `finish` helper used by this module.
    pub fn finish(&mut self) {
        if self.enabled {
            eprintln!();
        }
    }
}
