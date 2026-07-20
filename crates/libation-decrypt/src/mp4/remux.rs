//! Decrypt samples and remux to a DRM-free faststart M4B.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use super::boxutil::FourCC;
use super::parser::parse_mp4;
use super::samples::{filter_samples_by_ms, SampleInfo};
use crate::crypto::{decrypt_aavd_sample_in_place, decrypt_cenc_sample_in_place};
use crate::error::{DecryptError, Result};

/// Optional media-time trim window in milliseconds (absolute, pre-rebase).
#[derive(Debug, Clone, Copy, Default)]
pub struct TrimRange {
    pub start_ms: u64,
    /// Exclusive end; `None` means "through end of media".
    pub end_ms: Option<u64>,
}

/// How sample payloads are decrypted while remuxing.
#[derive(Debug, Clone, Copy)]
pub enum DecryptMode<'a> {
    /// Audible Adrm AES-128-CBC with a constant IV.
    Adrm { key: &'a [u8; 16], iv: &'a [u8; 16] },
    /// Clear copy (already decrypted / plain mp4a).
    None,
    /// Progressive CENC whole-sample AES-CTR with a single constant IV.
    CencConstantIv { key: &'a [u8; 16], iv: &'a [u8; 16] },
    /// Progressive CENC AES-CTR with one IV per sample (full track order).
    /// When trimming, IVs are selected by the same sample indices as the payloads.
    CencSampleIvs {
        key: &'a [u8; 16],
        ivs: &'a [[u8; 16]],
    },
}

#[derive(Debug, Clone)]
pub struct RemuxOptions<'a> {
    pub decrypt: DecryptMode<'a>,
    pub trim: Option<TrimRange>,
    /// Force `ftyp` brands suitable for M4B players.
    pub rewrite_ftyp: bool,
}

/// Inputs for writing a progressive faststart M4B from already-decoded samples.
pub(crate) struct ProgressiveWriteInput<'a> {
    pub moov_bytes: &'a [u8],
    pub moov_file_start: u64,
    pub sample_entry_type_offset: u64,
    pub audio_timescale: u32,
    pub mvhd_timescale: u32,
    pub payloads: Vec<Vec<u8>>,
    pub durations: Vec<u32>,
    pub rewrite_ftyp: bool,
}

/// Decrypt (and optionally trim) a progressive MP4 into a faststart M4B.
pub fn decrypt_and_remux(input: &Path, output: &Path, opts: &RemuxOptions<'_>) -> Result<()> {
    let mp4 = parse_mp4(input)?;
    let timescale = mp4.audio.timescale;

    let (selected, selected_ivs): (Vec<SampleInfo>, Option<Vec<[u8; 16]>>) = match opts.decrypt {
        DecryptMode::CencSampleIvs { ivs, .. } => {
            if ivs.len() != mp4.audio.samples.len() {
                return Err(DecryptError::Mp4(format!(
                    "CENC IV count {} != sample count {}",
                    ivs.len(),
                    mp4.audio.samples.len()
                )));
            }
            let (samples, ivs) = if let Some(trim) = opts.trim {
                filter_samples_and_ivs_by_ms(
                    &mp4.audio.samples,
                    ivs,
                    timescale,
                    trim.start_ms,
                    trim.end_ms,
                )
            } else {
                (mp4.audio.samples.clone(), ivs.to_vec())
            };
            (samples, Some(ivs))
        }
        _ => {
            let samples = if let Some(trim) = opts.trim {
                filter_samples_by_ms(&mp4.audio.samples, timescale, trim.start_ms, trim.end_ms)
            } else {
                mp4.audio.samples.clone()
            };
            (samples, None)
        }
    };

    if selected.is_empty() {
        return Err(DecryptError::Mp4(
            "no samples remain after trim; check brand intro/outro durations".into(),
        ));
    }

    let mut src = File::open(input)?;
    let mut payloads = Vec::with_capacity(selected.len());
    for (i, sample) in selected.iter().enumerate() {
        let mut buf = vec![0u8; sample.size as usize];
        src.seek(SeekFrom::Start(sample.offset))?;
        src.read_exact(&mut buf)?;
        match opts.decrypt {
            DecryptMode::Adrm { key, iv } => decrypt_aavd_sample_in_place(key, iv, &mut buf),
            DecryptMode::CencConstantIv { key, iv } => {
                decrypt_cenc_sample_in_place(key, iv, &mut buf);
            }
            DecryptMode::CencSampleIvs { key, .. } => {
                let iv = selected_ivs
                    .as_ref()
                    .and_then(|ivs| ivs.get(i))
                    .ok_or_else(|| DecryptError::Mp4("missing per-sample IV".into()))?;
                decrypt_cenc_sample_in_place(key, iv, &mut buf);
            }
            DecryptMode::None => {}
        }
        payloads.push(buf);
    }

    let durations: Vec<u32> = selected.iter().map(|s| s.duration).collect();
    write_progressive_m4b(
        output,
        ProgressiveWriteInput {
            moov_bytes: &mp4.moov_bytes,
            moov_file_start: mp4.moov.start,
            sample_entry_type_offset: mp4.audio.sample_entry_type_offset,
            audio_timescale: mp4.audio.timescale,
            mvhd_timescale: mp4.mvhd_timescale,
            payloads,
            durations,
            rewrite_ftyp: opts.rewrite_ftyp,
        },
    )
}

/// Like [`filter_samples_by_ms`], but keeps the matching per-sample IVs in lockstep.
fn filter_samples_and_ivs_by_ms(
    samples: &[SampleInfo],
    ivs: &[[u8; 16]],
    timescale: u32,
    start_ms: u64,
    end_ms: Option<u64>,
) -> (Vec<SampleInfo>, Vec<[u8; 16]>) {
    if timescale == 0 {
        return (samples.to_vec(), ivs.to_vec());
    }
    let start_ticks = start_ms.saturating_mul(u64::from(timescale)) / 1000;
    let end_ticks = end_ms.map(|ms| ms.saturating_mul(u64::from(timescale)) / 1000);

    let mut out_samples = Vec::new();
    let mut out_ivs = Vec::new();
    for (sample, iv) in samples.iter().zip(ivs.iter()) {
        let sample_end = sample.start_cts.saturating_add(u64::from(sample.duration));
        if sample_end <= start_ticks {
            continue;
        }
        if let Some(end) = end_ticks {
            if sample.start_cts >= end {
                break;
            }
        }
        let mut adjusted = sample.clone();
        adjusted.start_cts = sample.start_cts.saturating_sub(start_ticks);
        out_samples.push(adjusted);
        out_ivs.push(*iv);
    }
    let mut cts = 0u64;
    for sample in &mut out_samples {
        sample.start_cts = cts;
        cts = cts.saturating_add(u64::from(sample.duration));
    }
    (out_samples, out_ivs)
}

/// Write decrypted sample payloads as a progressive faststart M4B.
pub(crate) fn write_progressive_m4b(output: &Path, input: ProgressiveWriteInput<'_>) -> Result<()> {
    if input.payloads.is_empty() {
        return Err(DecryptError::Mp4("no samples to write".into()));
    }
    if input.payloads.len() != input.durations.len() {
        return Err(DecryptError::Mp4(format!(
            "payload/duration count mismatch: {} vs {}",
            input.payloads.len(),
            input.durations.len()
        )));
    }

    let sample_sizes: Vec<u32> = input.payloads.iter().map(|p| p.len() as u32).collect();
    let media_duration: u64 = input.durations.iter().map(|d| u64::from(*d)).sum();

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = File::create(output)?;

    let ftyp_bytes = build_m4b_ftyp();
    let _ = input.rewrite_ftyp;
    out.write_all(&ftyp_bytes)?;

    // --- mdat (64-bit size header so we can fill in later) ---
    let mdat_header_pos = out.stream_position()?;
    out.write_all(&1u32.to_be_bytes())?; // size=1 → 64-bit
    out.write_all(b"mdat")?;
    out.write_all(&0u64.to_be_bytes())?; // placeholder

    let mut chunk_offsets = Vec::with_capacity(input.payloads.len());
    for payload in &input.payloads {
        chunk_offsets.push(out.stream_position()?);
        out.write_all(payload)?;
    }
    let mdat_end = out.stream_position()?;
    let mdat_size = mdat_end - mdat_header_pos;
    out.seek(SeekFrom::Start(mdat_header_pos + 8))?;
    out.write_all(&mdat_size.to_be_bytes())?;
    out.seek(SeekFrom::Start(mdat_end))?;

    let moov = rebuild_moov(
        input.moov_bytes,
        input.moov_file_start,
        input.sample_entry_type_offset,
        input.audio_timescale,
        &sample_sizes,
        &input.durations,
        &chunk_offsets,
        media_duration,
        input.mvhd_timescale,
    )?;
    out.write_all(&moov)?;
    out.sync_all()?;

    relocate_moov_before_mdat(output, ftyp_bytes.len() as u64)?;
    Ok(())
}

fn build_m4b_ftyp() -> Vec<u8> {
    // major=M4B , minor=0, brands: M4B , mp42, isom, iso2
    let brands: &[&[u8; 4]] = &[b"M4B ", b"mp42", b"isom", b"iso2"];
    let size = 8 + 8 + brands.len() * 4;
    let mut buf = Vec::with_capacity(size);
    buf.extend_from_slice(&(size as u32).to_be_bytes());
    buf.extend_from_slice(b"ftyp");
    buf.extend_from_slice(b"M4B ");
    buf.extend_from_slice(&0u32.to_be_bytes());
    for b in brands {
        buf.extend_from_slice(*b);
    }
    buf
}

/// Patch moov sample tables + durations for the remuxed media.
#[allow(clippy::too_many_arguments)]
fn rebuild_moov(
    moov_bytes: &[u8],
    moov_file_start: u64,
    sample_entry_type_offset: u64,
    audio_timescale: u32,
    sample_sizes: &[u32],
    durations: &[u32],
    chunk_offsets: &[u64],
    media_duration: u64,
    mvhd_timescale: u32,
) -> Result<Vec<u8>> {
    if moov_bytes.len() < 8 {
        return Err(DecryptError::Mp4("moov too small".into()));
    }

    let mut body = moov_bytes.to_vec();
    let type_rel = sample_entry_type_offset
        .checked_sub(moov_file_start)
        .ok_or_else(|| DecryptError::Mp4("sample entry offset outside moov".into()))?;
    let type_rel = usize::try_from(type_rel)
        .map_err(|_| DecryptError::Mp4("sample entry offset overflow".into()))?;

    // Clear progressive DRM sample-entry markup before rewriting tables.
    body = clear_progressive_drm_boxes(&body, type_rel)?;

    // Replace stts / stsc / stsz / stco|co64 boxes inside stbl.
    let stts = encode_stts(durations);
    let stsc = encode_stsc_one_per_chunk(sample_sizes.len() as u32);
    let stsz = encode_stsz(sample_sizes);
    let need_co64 = chunk_offsets.iter().any(|&o| o > u64::from(u32::MAX));
    let stco = if need_co64 {
        encode_co64(chunk_offsets)
    } else {
        encode_stco(&chunk_offsets.iter().map(|&o| o as u32).collect::<Vec<_>>())
    };

    body = replace_stbl_child(&body, b"stts", &stts)
        .map_err(|e| DecryptError::Mp4(format!("replace stts: {e}")))?;
    body = replace_stbl_child(&body, b"stsc", &stsc)
        .map_err(|e| DecryptError::Mp4(format!("replace stsc: {e}")))?;
    body = replace_stbl_child(&body, b"stsz", &stsz)
        .map_err(|e| DecryptError::Mp4(format!("replace stsz: {e}")))?;
    body = replace_chunk_offset_box(&body, &stco)
        .map_err(|e| DecryptError::Mp4(format!("replace chunk offsets: {e}")))?;

    // Drop CENC sample-aux boxes if present (IVs are consumed during decrypt).
    body = remove_stbl_children_named(&body, &[b"saiz", b"saio"])?;

    // Update durations in mdhd / tkhd / mvhd.
    let movie_duration = if mvhd_timescale == 0 || audio_timescale == 0 {
        media_duration
    } else {
        media_duration * u64::from(mvhd_timescale) / u64::from(audio_timescale)
    };
    patch_duration_fields(&mut body, media_duration, movie_duration)?;

    // Fix outer moov size.
    let size = body.len() as u32;
    body[0..4].copy_from_slice(&size.to_be_bytes());
    Ok(body)
}

/// Replace `aavd`→`mp4a` or `enca`→`frma` format and strip `sinf`.
fn clear_progressive_drm_boxes(moov: &[u8], type_rel: usize) -> Result<Vec<u8>> {
    let mut body = moov.to_vec();
    if type_rel + 4 > body.len() {
        return Ok(body);
    }
    let entry_type = &body[type_rel..type_rel + 4];
    if entry_type == b"aavd" {
        body[type_rel..type_rel + 4].copy_from_slice(b"mp4a");
        return Ok(body);
    }
    if entry_type != b"enca" {
        return Ok(body);
    }

    // Sample entry box starts 4 bytes before the type field.
    let entry_pos = type_rel.saturating_sub(4);
    if entry_pos + 8 > body.len() {
        return Err(DecryptError::Mp4("enca sample entry truncated".into()));
    }
    let entry_size =
        u32::from_be_bytes(body[entry_pos..entry_pos + 4].try_into().unwrap()) as usize;
    let entry_end = entry_pos + entry_size;
    if entry_end > body.len() || entry_size < 36 {
        return Err(DecryptError::Mp4("invalid enca sample entry".into()));
    }

    let children_start = entry_pos + 36;
    let sinf = find_child_in_buf(&body, children_start, entry_end, b"sinf")?
        .ok_or_else(|| DecryptError::Mp4("enca missing sinf".into()))?;
    let frma = find_child_in_buf(&body, sinf.0 + 8, sinf.1, b"frma")?
        .ok_or_else(|| DecryptError::Mp4("sinf missing frma".into()))?;
    if frma.0 + 12 > frma.1 {
        return Err(DecryptError::Mp4("frma truncated".into()));
    }
    let format = body[frma.0 + 8..frma.0 + 12].to_vec();
    body[type_rel..type_rel + 4].copy_from_slice(&format);
    body = splice_replace(&body, sinf.0, sinf.1, &[])?;
    Ok(body)
}

fn find_child_in_buf(
    buf: &[u8],
    start: usize,
    end: usize,
    fourcc: &[u8; 4],
) -> Result<Option<(usize, usize)>> {
    let mut pos = start;
    while pos + 8 <= end {
        let size = u32::from_be_bytes(buf[pos..pos + 4].try_into().unwrap()) as usize;
        if size < 8 || pos + size > end {
            break;
        }
        let kind = &buf[pos + 4..pos + 8];
        let box_end = pos + size;
        if kind == fourcc {
            return Ok(Some((pos, box_end)));
        }
        pos = box_end;
    }
    Ok(None)
}

fn remove_stbl_children_named(moov: &[u8], names: &[&[u8; 4]]) -> Result<Vec<u8>> {
    let mut body = moov.to_vec();
    loop {
        let (stbl_start, stbl_end) = match find_box_range(&body, b"stbl")? {
            Some(r) => r,
            None => return Ok(body),
        };
        let mut removed = false;
        for name in names {
            if let Some((start, end)) = find_direct_child(&body, stbl_start, stbl_end, name)? {
                body = splice_replace(&body, start, end, &[])?;
                removed = true;
                break;
            }
        }
        if !removed {
            break;
        }
    }
    Ok(body)
}

fn encode_stts(durations: &[u32]) -> Vec<u8> {
    // Run-length encode.
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for &d in durations {
        if let Some(last) = runs.last_mut() {
            if last.1 == d {
                last.0 += 1;
                continue;
            }
        }
        runs.push((1, d));
    }
    let mut buf = Vec::new();
    let size = 8 + 4 + 4 + runs.len() * 8;
    buf.extend_from_slice(&(size as u32).to_be_bytes());
    buf.extend_from_slice(b"stts");
    buf.extend_from_slice(&0u32.to_be_bytes()); // version+flags
    buf.extend_from_slice(&(runs.len() as u32).to_be_bytes());
    for (count, delta) in runs {
        buf.extend_from_slice(&count.to_be_bytes());
        buf.extend_from_slice(&delta.to_be_bytes());
    }
    buf
}

fn encode_stsc_one_per_chunk(sample_count: u32) -> Vec<u8> {
    // first_chunk=1, samples_per_chunk=1, desc=1 — one entry covers all chunks.
    let size = 8 + 4 + 4 + 12;
    let mut buf = Vec::with_capacity(size);
    buf.extend_from_slice(&(size as u32).to_be_bytes());
    buf.extend_from_slice(b"stsc");
    buf.extend_from_slice(&0u32.to_be_bytes());
    buf.extend_from_slice(&1u32.to_be_bytes());
    buf.extend_from_slice(&1u32.to_be_bytes()); // first_chunk
    buf.extend_from_slice(&1u32.to_be_bytes()); // samples_per_chunk
    buf.extend_from_slice(&1u32.to_be_bytes()); // sample_description_index
    let _ = sample_count;
    buf
}

fn encode_stsz(sizes: &[u32]) -> Vec<u8> {
    let size = 8 + 4 + 4 + 4 + sizes.len() * 4;
    let mut buf = Vec::with_capacity(size);
    buf.extend_from_slice(&(size as u32).to_be_bytes());
    buf.extend_from_slice(b"stsz");
    buf.extend_from_slice(&0u32.to_be_bytes());
    buf.extend_from_slice(&0u32.to_be_bytes()); // sample_size=0 → table follows
    buf.extend_from_slice(&(sizes.len() as u32).to_be_bytes());
    for s in sizes {
        buf.extend_from_slice(&s.to_be_bytes());
    }
    buf
}

fn encode_stco(offsets: &[u32]) -> Vec<u8> {
    let size = 8 + 4 + 4 + offsets.len() * 4;
    let mut buf = Vec::with_capacity(size);
    buf.extend_from_slice(&(size as u32).to_be_bytes());
    buf.extend_from_slice(b"stco");
    buf.extend_from_slice(&0u32.to_be_bytes());
    buf.extend_from_slice(&(offsets.len() as u32).to_be_bytes());
    for o in offsets {
        buf.extend_from_slice(&o.to_be_bytes());
    }
    buf
}

fn encode_co64(offsets: &[u64]) -> Vec<u8> {
    let size = 8 + 4 + 4 + offsets.len() * 8;
    let mut buf = Vec::with_capacity(size);
    buf.extend_from_slice(&(size as u32).to_be_bytes());
    buf.extend_from_slice(b"co64");
    buf.extend_from_slice(&0u32.to_be_bytes());
    buf.extend_from_slice(&(offsets.len() as u32).to_be_bytes());
    for o in offsets {
        buf.extend_from_slice(&o.to_be_bytes());
    }
    buf
}

/// Find a direct child of the first `stbl` in `moov` and replace it.
fn replace_stbl_child(moov: &[u8], fourcc: &[u8; 4], replacement: &[u8]) -> Result<Vec<u8>> {
    let (stbl_start, stbl_end) = find_box_range(moov, b"stbl")?
        .ok_or_else(|| DecryptError::Mp4("moov missing stbl".into()))?;
    let child = find_direct_child(moov, stbl_start, stbl_end, fourcc)?
        .ok_or_else(|| DecryptError::Mp4(format!("stbl missing {}", FourCC(*fourcc))))?;
    splice_replace(moov, child.0, child.1, replacement)
}

fn replace_chunk_offset_box(moov: &[u8], replacement: &[u8]) -> Result<Vec<u8>> {
    let (stbl_start, stbl_end) = find_box_range(moov, b"stbl")?
        .ok_or_else(|| DecryptError::Mp4("moov missing stbl".into()))?;
    if let Some(child) = find_direct_child(moov, stbl_start, stbl_end, b"stco")? {
        return splice_replace(moov, child.0, child.1, replacement);
    }
    if let Some(child) = find_direct_child(moov, stbl_start, stbl_end, b"co64")? {
        return splice_replace(moov, child.0, child.1, replacement);
    }
    Err(DecryptError::Mp4("stbl missing stco/co64".into()))
}

pub(super) fn splice_replace(
    buf: &[u8],
    start: usize,
    end: usize,
    replacement: &[u8],
) -> Result<Vec<u8>> {
    let old_len = end - start;
    let new_len = replacement.len();
    let delta = new_len as i64 - old_len as i64;
    let ancestors = if delta == 0 {
        Vec::new()
    } else {
        ancestor_size_offsets(buf, start)?
    };
    let mut out = Vec::with_capacity(buf.len() - old_len + new_len);
    out.extend_from_slice(&buf[..start]);
    out.extend_from_slice(replacement);
    out.extend_from_slice(&buf[end..]);
    for (offset, old_size) in ancestors {
        let new_size = (old_size as i64 + delta) as u32;
        if offset + 4 <= out.len() {
            out[offset..offset + 4].copy_from_slice(&new_size.to_be_bytes());
        }
    }
    Ok(out)
}

/// Size-field offsets for every box that strictly contains `at`.
fn ancestor_size_offsets(buf: &[u8], at: usize) -> Result<Vec<(usize, usize)>> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    let mut end = buf.len();
    loop {
        let mut found = None;
        let mut child_pos = pos;
        while child_pos + 8 <= end {
            let size =
                u32::from_be_bytes(buf[child_pos..child_pos + 4].try_into().unwrap()) as usize;
            if size < 8 || child_pos + size > end {
                break;
            }
            let kind = &buf[child_pos + 4..child_pos + 8];
            let box_end = child_pos + size;
            if at > child_pos && at < box_end {
                out.push((child_pos, size));
                let content = match kind {
                    b"meta" => child_pos + 12,
                    // stsd FullBox + entry_count
                    b"stsd" => child_pos + 16,
                    // AudioSampleEntry fixed header after size+type
                    b"enca" | b"mp4a" | b"aavd" => child_pos + 36,
                    _ => child_pos + 8,
                };
                found = Some((content, box_end, kind.to_vec()));
                break;
            }
            child_pos = box_end;
        }
        let Some((content, box_end, kind)) = found else {
            break;
        };
        if !matches!(
            kind.as_slice(),
            b"moov"
                | b"trak"
                | b"mdia"
                | b"minf"
                | b"stbl"
                | b"udta"
                | b"meta"
                | b"stsd"
                | b"enca"
                | b"mp4a"
                | b"aavd"
                | b"sinf"
                | b"schi"
        ) {
            break;
        }
        pos = content;
        end = box_end;
    }
    Ok(out)
}

pub(super) fn find_box_range(buf: &[u8], fourcc: &[u8; 4]) -> Result<Option<(usize, usize)>> {
    let mut stack = vec![(0usize, buf.len())];
    while let Some((start, end)) = stack.pop() {
        let mut pos = start;
        while pos + 8 <= end {
            let size = u32::from_be_bytes(buf[pos..pos + 4].try_into().unwrap()) as usize;
            if size < 8 || pos + size > end {
                break;
            }
            let kind = &buf[pos + 4..pos + 8];
            let content_start = pos + 8;
            let box_end = pos + size;
            if kind == fourcc {
                return Ok(Some((pos, box_end)));
            }
            if matches!(
                kind,
                b"moov" | b"trak" | b"mdia" | b"minf" | b"stbl" | b"udta"
            ) {
                stack.push((content_start, box_end));
            } else if kind == b"meta" {
                stack.push((content_start + 4, box_end));
            }
            pos = box_end;
        }
    }
    Ok(None)
}

pub(super) fn find_direct_child(
    buf: &[u8],
    parent_start: usize,
    parent_end: usize,
    fourcc: &[u8; 4],
) -> Result<Option<(usize, usize)>> {
    let kind = &buf[parent_start + 4..parent_start + 8];
    let mut pos = parent_start + 8;
    if kind == b"meta" {
        pos += 4;
    }
    while pos + 8 <= parent_end {
        let size = u32::from_be_bytes(buf[pos..pos + 4].try_into().unwrap()) as usize;
        if size < 8 || pos + size > parent_end {
            break;
        }
        let child_kind = &buf[pos + 4..pos + 8];
        let end = pos + size;
        if child_kind == fourcc {
            return Ok(Some((pos, end)));
        }
        pos = end;
    }
    Ok(None)
}

fn patch_duration_fields(moov: &mut [u8], media_duration: u64, movie_duration: u64) -> Result<()> {
    // Patch first mvhd, first tkhd (audio), first mdhd durations.
    patch_named_duration(moov, b"mvhd", movie_duration)?;
    patch_named_duration(moov, b"tkhd", movie_duration)?;
    patch_named_duration(moov, b"mdhd", media_duration)?;
    Ok(())
}

fn patch_named_duration(moov: &mut [u8], fourcc: &[u8; 4], duration: u64) -> Result<()> {
    let Some((start, end)) = find_box_range(moov, fourcc)? else {
        return Ok(());
    };
    if start + 12 > end {
        return Ok(());
    }
    let version = moov[start + 8];
    match (fourcc, version) {
        (b"mvhd", 1) => {
            // version(1)+flags(3)+ctime(8)+mtime(8)+timescale(4)+duration(8)
            let off = start + 8 + 4 + 8 + 8 + 4;
            if off + 8 <= end {
                moov[off..off + 8].copy_from_slice(&duration.to_be_bytes());
            }
        }
        (b"mvhd", _) => {
            let off = start + 8 + 4 + 4 + 4 + 4;
            if off + 4 <= end {
                moov[off..off + 4].copy_from_slice(&(duration as u32).to_be_bytes());
            }
        }
        (b"tkhd", 1) => {
            // ver+flags + ctime8 + mtime8 + track_id4 + reserved4 + duration8
            let off = start + 8 + 4 + 8 + 8 + 4 + 4;
            if off + 8 <= end {
                moov[off..off + 8].copy_from_slice(&duration.to_be_bytes());
            }
        }
        (b"tkhd", _) => {
            let off = start + 8 + 4 + 4 + 4 + 4 + 4;
            if off + 4 <= end {
                moov[off..off + 4].copy_from_slice(&(duration as u32).to_be_bytes());
            }
        }
        (b"mdhd", 1) => {
            let off = start + 8 + 4 + 8 + 8 + 4;
            if off + 8 <= end {
                moov[off..off + 8].copy_from_slice(&duration.to_be_bytes());
            }
        }
        (b"mdhd", _) => {
            let off = start + 8 + 4 + 4 + 4 + 4;
            if off + 4 <= end {
                moov[off..off + 4].copy_from_slice(&(duration as u32).to_be_bytes());
            }
        }
        _ => {}
    }
    Ok(())
}

/// Move `moov` in front of `mdat` and shift chunk offsets accordingly.
fn relocate_moov_before_mdat(path: &Path, ftyp_len: u64) -> Result<()> {
    let data = std::fs::read(path)?;
    // Locate mdat and moov at top level.
    let mut pos = 0usize;
    let mut mdat = None;
    let mut moov = None;
    while pos + 8 <= data.len() {
        let mut size = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
        let kind = &data[pos + 4..pos + 8];
        let mut header = 8usize;
        if size == 1 {
            if pos + 16 > data.len() {
                break;
            }
            size = u64::from_be_bytes(data[pos + 8..pos + 16].try_into().unwrap()) as usize;
            header = 16;
        } else if size == 0 {
            size = data.len() - pos;
        }
        if kind == b"mdat" {
            mdat = Some((pos, size, header));
        } else if kind == b"moov" {
            moov = Some((pos, size));
        }
        if size < header {
            break;
        }
        pos += size;
    }
    let (mdat_pos, mdat_size, _mdat_hdr) =
        mdat.ok_or_else(|| DecryptError::Mp4("relocate: missing mdat".into()))?;
    let (moov_pos, moov_size) =
        moov.ok_or_else(|| DecryptError::Mp4("relocate: missing moov".into()))?;

    if moov_pos < mdat_pos {
        // Already faststart.
        return Ok(());
    }

    let moov_bytes = data[moov_pos..moov_pos + moov_size].to_vec();
    let mdat_bytes = data[mdat_pos..mdat_pos + mdat_size].to_vec();
    let shift = moov_size as u64;

    // Patch chunk offsets inside moov: they currently point into the file where mdat
    // started at `mdat_pos`. After move, mdat starts at `ftyp_len + moov_size`.
    let old_mdat_data_bias = mdat_pos as u64;
    let new_mdat_data_bias = ftyp_len + shift;
    let offset_delta = new_mdat_data_bias as i64 - old_mdat_data_bias as i64;
    let mut patched_moov = moov_bytes;
    shift_chunk_offsets(&mut patched_moov, offset_delta)?;

    let mut out = Vec::with_capacity(data.len());
    out.extend_from_slice(&data[..ftyp_len as usize]);
    out.extend_from_slice(&patched_moov);
    out.extend_from_slice(&mdat_bytes);
    // Preserve any trailing boxes after moov (rare).
    let after = moov_pos + moov_size;
    if after < data.len() {
        out.extend_from_slice(&data[after..]);
    }
    std::fs::write(path, out)?;
    Ok(())
}

fn shift_chunk_offsets(moov: &mut [u8], delta: i64) -> Result<()> {
    if let Some((start, end)) = find_box_range(moov, b"stco")? {
        let mut pos = start + 12; // hdr + ver/flags
        if pos + 4 > end {
            return Ok(());
        }
        let count = u32::from_be_bytes(moov[pos..pos + 4].try_into().unwrap()) as usize;
        pos += 4;
        for _ in 0..count {
            if pos + 4 > end {
                break;
            }
            let old = u32::from_be_bytes(moov[pos..pos + 4].try_into().unwrap());
            let new = (i64::from(old) + delta) as u32;
            moov[pos..pos + 4].copy_from_slice(&new.to_be_bytes());
            pos += 4;
        }
    }
    if let Some((start, end)) = find_box_range(moov, b"co64")? {
        let mut pos = start + 12;
        if pos + 4 > end {
            return Ok(());
        }
        let count = u32::from_be_bytes(moov[pos..pos + 4].try_into().unwrap()) as usize;
        pos += 4;
        for _ in 0..count {
            if pos + 8 > end {
                break;
            }
            let old = u64::from_be_bytes(moov[pos..pos + 8].try_into().unwrap());
            let new = (old as i64 + delta) as u64;
            moov[pos..pos + 8].copy_from_slice(&new.to_be_bytes());
            pos += 8;
        }
    }
    Ok(())
}
