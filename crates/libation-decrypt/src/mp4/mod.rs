//! Minimal ISO-BMFF helpers for Audible AAXC decrypt / remux.

mod boxutil;
mod parser;
mod remux;
mod samples;

pub use parser::{parse_mp4, track_duration_ms, SampleEntryKind};
pub use remux::{decrypt_and_remux, DecryptMode, RemuxOptions, TrimRange};
