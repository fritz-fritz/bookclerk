//! Lightweight chapter boundary alignment via local waveform analysis.
//!
//! Around each chapter start (except 0), decode only a small window
//! (`±window_ms`) and snap to the nearest speech onset after a short silence.
//! This avoids full-book decode/re-encode while correcting small brand/timing
//! drift between Audible chapter metadata and plain-store audio (Chirp/Libro).

use std::fs::File;
use std::path::Path;

use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::{Decoder, DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

use crate::error::{DecryptError, Result};

/// Tunables for silence / speech-onset chapter snapping.
#[derive(Debug, Clone, Copy)]
pub struct ChapterAlignOptions {
    /// Search window on each side of the expected chapter start.
    pub window_ms: u64,
    /// Minimum contiguous silence before a speech onset counts.
    pub min_silence_ms: u64,
    /// RMS frame size used for energy analysis.
    pub frame_ms: u64,
    /// Absolute RMS floor (s16) treated as silence when the window is quiet.
    pub silence_rms_floor: f32,
    /// Fraction of window median RMS used as the silence threshold.
    pub silence_rms_ratio: f32,
}

impl Default for ChapterAlignOptions {
    fn default() -> Self {
        Self {
            window_ms: 5_000,
            min_silence_ms: 80,
            frame_ms: 20,
            silence_rms_floor: 200.0,
            silence_rms_ratio: 0.35,
        }
    }
}

/// Snap chapter starts to nearby speech onsets found by local waveform analysis.
///
/// Chapters at `0` are left unchanged. Failures to decode a window keep the
/// original timestamp for that chapter. Results stay sorted and monotonic.
///
/// Opens the media once and reuses a seek index across chapter windows so a
/// long book stays cheap (~tens of seconds of decode total, not a full pass).
#[must_use]
pub fn align_chapter_starts(
    path: &Path,
    chapters: &[(String, u64)],
    opts: ChapterAlignOptions,
) -> Vec<(String, u64)> {
    if chapters.len() < 2 || opts.window_ms == 0 {
        return chapters.to_vec();
    }

    let mut reader = match AlignReader::open(path) {
        Ok(reader) => reader,
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                error = %err,
                "chapter align open failed; keeping original chapter starts"
            );
            return chapters.to_vec();
        }
    };

    let mut out = Vec::with_capacity(chapters.len());
    let mut shifted = 0u32;
    for (idx, (title, start_ms)) in chapters.iter().enumerate() {
        if *start_ms == 0 || idx == 0 {
            out.push((title.clone(), *start_ms));
            continue;
        }
        let aligned = match snap_chapter_start(&mut reader, *start_ms, opts) {
            Ok(Some(ms)) => {
                if ms != *start_ms {
                    shifted += 1;
                }
                ms
            }
            Ok(None) => *start_ms,
            Err(err) => {
                tracing::debug!(
                    chapter = %title,
                    start_ms,
                    error = %err,
                    "chapter align window failed; keeping original start"
                );
                *start_ms
            }
        };
        out.push((title.clone(), aligned));
    }

    enforce_monotonic(&mut out);

    if shifted > 0 {
        tracing::info!(
            chapters = chapters.len(),
            shifted,
            window_ms = opts.window_ms,
            path = %path.display(),
            "aligned chapter starts via local waveform analysis"
        );
    } else {
        tracing::debug!(
            chapters = chapters.len(),
            window_ms = opts.window_ms,
            path = %path.display(),
            "chapter align found no start shifts"
        );
    }
    out
}

struct AlignReader {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    sample_rate: u32,
    channels: usize,
}

impl AlignReader {
    fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        let probed = symphonia::default::get_probe()
            .format(
                &hint,
                mss,
                &FormatOptions {
                    // One index for all chapter windows.
                    prebuild_seek_index: true,
                    ..FormatOptions::default()
                },
                &MetadataOptions::default(),
            )
            .map_err(|err| DecryptError::Native(format!("chapter align probe failed: {err}")))?;
        let format = probed.format;

        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or_else(|| DecryptError::Native("chapter align: no decodable audio track".into()))?
            .clone();
        let track_id = track.id;
        let sample_rate = track
            .codec_params
            .sample_rate
            .ok_or_else(|| DecryptError::Native("chapter align: missing sample rate".into()))?;
        let channels = track
            .codec_params
            .channels
            .map(|c| c.count())
            .unwrap_or(1)
            .max(1);

        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|err| {
                DecryptError::Native(format!("chapter align decoder init failed: {err}"))
            })?;

        Ok(Self {
            format,
            decoder,
            track_id,
            sample_rate,
            channels,
        })
    }

    fn decode_window_energy(
        &mut self,
        start_ms: u64,
        end_ms: u64,
        frame_ms: u64,
    ) -> Result<Vec<(u64, f32)>> {
        if end_ms <= start_ms {
            return Ok(Vec::new());
        }

        let seek_secs = start_ms / 1000;
        let seek_frac = (start_ms % 1000) as f64 / 1000.0;
        self.format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time: Time::new(seek_secs, seek_frac),
                    track_id: Some(self.track_id),
                },
            )
            .map_err(|err| DecryptError::Native(format!("chapter align seek failed: {err}")))?;
        self.decoder.reset();

        let frame_samples =
            ((u64::from(self.sample_rate) * frame_ms.max(1)) / 1000).max(1) as usize;
        let mut mono_frame = Vec::with_capacity(frame_samples);
        let mut energies = Vec::new();
        let mut samples_seen: u64 = 0;
        let window_samples =
            ((end_ms.saturating_sub(start_ms)) * u64::from(self.sample_rate) / 1000).max(1);

        loop {
            if samples_seen >= window_samples {
                break;
            }
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(SymphoniaError::ResetRequired) => {
                    self.decoder.reset();
                    continue;
                }
                Err(SymphoniaError::IoError(err))
                    if err.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break;
                }
                Err(err) => {
                    return Err(DecryptError::Native(format!(
                        "chapter align demux error: {err}"
                    )));
                }
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            let decoded = match self.decoder.decode(&packet) {
                Ok(buf) => buf,
                Err(SymphoniaError::DecodeError(_)) => continue,
                Err(err) => {
                    return Err(DecryptError::Native(format!(
                        "chapter align decode error: {err}"
                    )));
                }
            };
            append_mono_i16(&decoded, self.channels, &mut mono_frame);
            while mono_frame.len() >= frame_samples {
                let frame: Vec<i16> = mono_frame.drain(..frame_samples).collect();
                let rms = rms_i16(&frame);
                let t_ms = start_ms + samples_seen * 1000 / u64::from(self.sample_rate);
                energies.push((t_ms, rms));
                samples_seen = samples_seen.saturating_add(frame_samples as u64);
                if samples_seen >= window_samples {
                    break;
                }
            }
        }

        Ok(energies)
    }
}

fn snap_chapter_start(
    reader: &mut AlignReader,
    expected_ms: u64,
    opts: ChapterAlignOptions,
) -> Result<Option<u64>> {
    let window_start_ms = expected_ms.saturating_sub(opts.window_ms);
    let window_end_ms = expected_ms.saturating_add(opts.window_ms);
    let energies = reader.decode_window_energy(window_start_ms, window_end_ms, opts.frame_ms)?;
    if energies.is_empty() {
        return Ok(None);
    }

    let median = percentile(&energies, 0.50);
    let thr = (median * opts.silence_rms_ratio).max(opts.silence_rms_floor);
    let frame_ms = opts.frame_ms.max(1);
    let min_quiet_frames = opts.min_silence_ms.div_ceil(frame_ms).max(1) as usize;

    let mut best: Option<(u64, u64, f32)> = None; // (abs_delta, onset_ms, rise)
    let mut quiet_run = 0usize;
    for i in 0..energies.len() {
        let (t_ms, rms) = energies[i];
        if rms < thr {
            quiet_run = quiet_run.saturating_add(1);
            continue;
        }
        if quiet_run >= min_quiet_frames {
            let prev = if i > 0 { energies[i - 1].1 } else { 0.0 };
            let rise = (rms - prev).max(0.0);
            let delta = t_ms.abs_diff(expected_ms);
            let cand = (delta, t_ms, rise);
            match best {
                None => best = Some(cand),
                Some(cur) => {
                    // Prefer closeness; break ties toward stronger onsets.
                    if cand.0 < cur.0 || (cand.0 == cur.0 && cand.2 > cur.2) {
                        best = Some(cand);
                    }
                }
            }
        }
        quiet_run = 0;
    }

    Ok(best.map(|(_, onset_ms, _)| onset_ms))
}

fn append_mono_i16(buf: &AudioBufferRef<'_>, channels: usize, dst: &mut Vec<i16>) {
    match buf {
        AudioBufferRef::F32(buf) => {
            let frames = buf.frames();
            let chans = buf.spec().channels.count().min(channels).max(1);
            for i in 0..frames {
                let mut acc = 0.0f32;
                for ch in 0..chans {
                    acc += buf.chan(ch)[i];
                }
                let sample = (acc / chans as f32).clamp(-1.0, 1.0);
                dst.push((sample * f32::from(i16::MAX)) as i16);
            }
        }
        AudioBufferRef::S16(buf) => {
            let frames = buf.frames();
            let chans = buf.spec().channels.count().min(channels).max(1);
            for i in 0..frames {
                let mut acc = 0i32;
                for ch in 0..chans {
                    acc += i32::from(buf.chan(ch)[i]);
                }
                dst.push((acc / chans as i32) as i16);
            }
        }
        other => {
            let frames = other.frames();
            dst.extend(std::iter::repeat_n(0i16, frames));
        }
    }
}

fn rms_i16(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f64 = samples.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
    (sum_sq / samples.len() as f64).sqrt() as f32
}

fn percentile(energies: &[(u64, f32)], q: f32) -> f32 {
    if energies.is_empty() {
        return 0.0;
    }
    let mut vals: Vec<f32> = energies.iter().map(|(_, e)| *e).collect();
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Less));
    let idx = ((vals.len() as f32 - 1.0) * q.clamp(0.0, 1.0)).round() as usize;
    vals[idx.min(vals.len() - 1)]
}

fn enforce_monotonic(chapters: &mut [(String, u64)]) {
    let mut prev = 0u64;
    for (idx, ch) in chapters.iter_mut().enumerate() {
        if idx == 0 {
            prev = ch.1;
            continue;
        }
        if ch.1 <= prev {
            ch.1 = prev.saturating_add(1);
        }
        prev = ch.1;
    }
}

/// Scale rebased chapter starts so they span the probed plain-audio duration.
///
/// When Audible content duration (`runtime - intro - outro`) differs from the
/// plain file, apply a uniform scale. No-ops when either duration is missing or
/// the relative delta is tiny.
#[must_use]
pub fn scale_chapters_to_duration(
    chapters: &[(String, u64)],
    content_duration_ms: Option<u64>,
    plain_duration_ms: Option<u64>,
) -> Vec<(String, u64)> {
    let (Some(content), Some(plain)) = (content_duration_ms, plain_duration_ms) else {
        return chapters.to_vec();
    };
    if content == 0 || plain == 0 || content == plain {
        return chapters.to_vec();
    }
    let delta = content.abs_diff(plain);
    // Ignore sub-250ms measurement noise.
    if delta < 250 {
        return chapters.to_vec();
    }
    let scale = plain as f64 / content as f64;
    // Guard against pathological mismatches (wrong title / bad runtime).
    if !(0.98..=1.02).contains(&scale) {
        tracing::debug!(
            content_duration_ms = content,
            plain_duration_ms = plain,
            scale,
            "skipping chapter duration scale; ratio outside ±2%"
        );
        return chapters.to_vec();
    }
    tracing::info!(
        content_duration_ms = content,
        plain_duration_ms = plain,
        scale,
        "scaling chapter starts to plain-audio duration"
    );
    chapters
        .iter()
        .map(|(title, start)| {
            let scaled = ((*start as f64) * scale).round().max(0.0) as u64;
            let clamped = scaled.min(plain.saturating_sub(1));
            (title.clone(), if *start == 0 { 0 } else { clamped })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package_m4b::package_m4b_from_pcm;

    fn tone_pcm(sample_rate: u32, ms: u64, freq: f32, amp: f32) -> Vec<i16> {
        let frames = (u64::from(sample_rate) * ms / 1000) as usize;
        let mut out = Vec::with_capacity(frames);
        for i in 0..frames {
            let t = i as f32 / sample_rate as f32;
            let s = (2.0 * std::f32::consts::PI * freq * t).sin() * amp;
            out.push((s * f32::from(i16::MAX)) as i16);
        }
        out
    }

    #[test]
    fn scales_when_plain_shorter() {
        // ~1% shorter plain duration (within the ±2% safety guard).
        let chapters = vec![
            ("A".into(), 0u64),
            ("B".into(), 1_000_000),
            ("C".into(), 2_000_000),
        ];
        let out = scale_chapters_to_duration(&chapters, Some(2_000_000), Some(1_980_000));
        assert_eq!(out[0].1, 0);
        assert_eq!(out[1].1, 990_000);
        assert_eq!(out[2].1, 1_980_000 - 1);
    }

    #[test]
    fn snaps_to_speech_onset_in_silence_gap() {
        let sample_rate = 16_000u32;
        // 2s tone | 1s silence | 2s tone. True chapter-2 onset at 3000ms.
        let mut pcm = tone_pcm(sample_rate, 2_000, 440.0, 0.25);
        pcm.extend(std::iter::repeat_n(0i16, sample_rate as usize));
        pcm.extend(tone_pcm(sample_rate, 2_000, 660.0, 0.25));

        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("gap.m4b");
        package_m4b_from_pcm(&pcm, sample_rate, 1, &out, &[]).unwrap();

        // Intentionally off by ~400ms into the silence / early into speech.
        let chapters = vec![("One".into(), 0u64), ("Two".into(), 2_600)];
        let aligned = align_chapter_starts(&out, &chapters, ChapterAlignOptions::default());
        assert_eq!(aligned[0].1, 0);
        // Should land near the 3000ms speech onset (±150ms for AAC framing).
        assert!(
            aligned[1].1.abs_diff(3_000) <= 150,
            "aligned={} expected~3000",
            aligned[1].1
        );
    }
}
