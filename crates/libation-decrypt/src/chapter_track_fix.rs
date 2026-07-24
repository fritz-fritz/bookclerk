//! Post-process QuickTime chapter tracks so AVFoundation (iOS) can read them.
//!
//! `mp4ameta` writes Nero `chpl` and a QuickTime text chapter track, but the
//! text track is missing two things Apple's AVFoundation chapter API requires:
//!
//! 1. an `edts`/`elst` edit list on the chapter `trak`
//! 2. a proper SampleEntry header (`reserved[6]` + `data_reference_index=1`)
//!    before `displayFlags` in the text `stsd` (mp4ameta emits the
//!    `Text::media_chapter` payload directly after `'text'`, so parsers see
//!    `displayFlags` mis-aligned as reserved/data-ref)
//!
//! Without those, iOS players (NovelWave, Apple Books, etc.) report no chapters
//! and often show a single fallback chapter named after the book title. ffmpeg
//! still reads Nero `chpl`, which is why ffprobe looked fine.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::error::{DecryptError, Result};

/// Known-good QuickTime `text` sample entry (59 bytes): SampleEntry header
/// (`reserved[6]` + `data_reference_index=1`) followed by the same
/// `displayFlags=1` + `ftab` payload `mp4ameta::Text::media_chapter` uses.
const AVF_TEXT_SAMPLE_ENTRY: &[u8] = &[
    0x00, 0x00, 0x00, 0x3B, // size = 59
    b't', b'e', b'x', b't', 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // reserved
    0x00, 0x01, // data_reference_index
    // Text::media_chapter payload:
    0x00, 0x00, 0x00, 0x01, // displayFlags
    0x00, 0x00, // justification
    0x00, 0x00, 0x00, 0x00, // bg color
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // default text box
    0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, // style record
    0x00, 0x00, 0x00, 0x00, // fg color
    0x00, 0x00, 0x00, 0x0D, b'f', b't', b'a', b'b', // font table box
    0x00, 0x01, 0x00, 0x01, 0x00,
];

/// Ensure the QuickTime chapter track is readable by AVFoundation.
///
/// Returns `true` when the file was modified.
pub fn ensure_avfoundation_chapter_track(path: &Path) -> Result<bool> {
    let mut file = File::options().read(true).write(true).open(path)?;
    let file_len = file.seek(SeekFrom::End(0))?;
    file.seek(SeekFrom::Start(0))?;

    let mut data = Vec::with_capacity(file_len as usize);
    file.read_to_end(&mut data)?;

    let Some(moov) = find_atom(&data, 0, data.len(), *b"moov") else {
        return Ok(false);
    };
    let moov_start = moov.offset;
    let mut moov_buf = data[moov_start..moov_start + moov.size].to_vec();

    let mut changed = false;
    changed |= patch_chapter_trak_avfoundation(&mut moov_buf)?;

    if !changed {
        return Ok(false);
    }

    // Rewrite moov in place when it sits at EOF (our remux layout); otherwise
    // append a new moov and free the old one so media offsets stay valid.
    let moov_at_eof = moov_start + moov.size == data.len();
    if moov_at_eof {
        file.seek(SeekFrom::Start(moov_start as u64))?;
        file.write_all(&moov_buf)?;
        file.set_len(moov_start as u64 + moov_buf.len() as u64)?;
    } else {
        // Mark old moov as free, append corrected moov.
        if moov.size >= 8 {
            data[moov_start + 4..moov_start + 8].copy_from_slice(b"free");
        }
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&data)?;
        file.write_all(&moov_buf)?;
        file.set_len(data.len() as u64 + moov_buf.len() as u64)?;
    }

    tracing::info!(
        path = %path.display(),
        "patched QuickTime chapter track for AVFoundation (edts/elst + text stsd)"
    );
    Ok(true)
}

#[derive(Debug, Clone, Copy)]
struct AtomSpan {
    offset: usize,
    size: usize,
    header: usize,
}

fn find_atom(data: &[u8], start: usize, end: usize, fourcc: [u8; 4]) -> Option<AtomSpan> {
    let mut pos = start;
    while pos + 8 <= end {
        let size32 = u32::from_be_bytes(data[pos..pos + 4].try_into().ok()?);
        let typ = <[u8; 4]>::try_from(&data[pos + 4..pos + 8]).ok()?;
        let (size, header) = if size32 == 1 {
            if pos + 16 > end {
                break;
            }
            let size64 = u64::from_be_bytes(data[pos + 8..pos + 16].try_into().ok()?) as usize;
            (size64, 16)
        } else if size32 == 0 {
            (end - pos, 8)
        } else {
            (size32 as usize, 8)
        };
        if size < header || pos + size > end {
            break;
        }
        if typ == fourcc {
            return Some(AtomSpan {
                offset: pos,
                size,
                header,
            });
        }
        pos += size;
    }
    None
}

fn iter_children(data: &[u8], parent: AtomSpan) -> Vec<AtomSpan> {
    let mut out = Vec::new();
    let mut pos = parent.offset + parent.header;
    let end = parent.offset + parent.size;
    while pos + 8 <= end {
        let size32 = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap());
        let (size, header) = if size32 == 1 {
            if pos + 16 > end {
                break;
            }
            let size64 = u64::from_be_bytes(data[pos + 8..pos + 16].try_into().unwrap()) as usize;
            (size64, 16)
        } else if size32 == 0 {
            (end - pos, 8)
        } else {
            (size32 as usize, 8)
        };
        if size < header || pos + size > end {
            break;
        }
        out.push(AtomSpan {
            offset: pos,
            size,
            header,
        });
        pos += size;
    }
    out
}

fn atom_type(data: &[u8], atom: AtomSpan) -> [u8; 4] {
    data[atom.offset + 4..atom.offset + 8]
        .try_into()
        .unwrap_or([0; 4])
}

fn set_atom_size(buf: &mut [u8], atom_offset: usize, new_size: usize) -> Result<()> {
    if new_size > u32::MAX as usize {
        return Err(DecryptError::Native(
            "moov atom too large for 32-bit size field".into(),
        ));
    }
    buf[atom_offset..atom_offset + 4].copy_from_slice(&(new_size as u32).to_be_bytes());
    Ok(())
}

fn patch_chapter_trak_avfoundation(moov: &mut Vec<u8>) -> Result<bool> {
    let moov_span = AtomSpan {
        offset: 0,
        size: moov.len(),
        header: 8,
    };
    let children = iter_children(moov, moov_span);

    let mvhd = children
        .iter()
        .find(|a| atom_type(moov, **a) == *b"mvhd")
        .copied()
        .ok_or_else(|| DecryptError::Native("moov missing mvhd".into()))?;
    let movie_duration = read_mvhd_duration(moov, mvhd)?;

    let traks: Vec<AtomSpan> = children
        .iter()
        .copied()
        .filter(|a| atom_type(moov, *a) == *b"trak")
        .collect();

    let mut chapter_track_id: Option<u32> = None;
    for trak in &traks {
        if let Some(tref) = iter_children(moov, *trak)
            .into_iter()
            .find(|a| atom_type(moov, *a) == *b"tref")
        {
            if let Some(chap) = iter_children(moov, tref)
                .into_iter()
                .find(|a| atom_type(moov, *a) == *b"chap")
            {
                let payload = &moov[chap.offset + chap.header..chap.offset + chap.size];
                if payload.len() >= 4 {
                    chapter_track_id = Some(u32::from_be_bytes(payload[0..4].try_into().unwrap()));
                    break;
                }
            }
        }
    }
    let Some(chapter_id) = chapter_track_id else {
        return Ok(false);
    };

    let Some(chapter_trak) = traks.iter().copied().find(|trak| {
        iter_children(moov, *trak)
            .into_iter()
            .find(|a| atom_type(moov, *a) == *b"tkhd")
            .and_then(|tkhd| read_tkhd_id(moov, tkhd).ok())
            == Some(chapter_id)
    }) else {
        return Ok(false);
    };

    let mut changed = false;
    changed |= ensure_edts(moov, chapter_trak, movie_duration)?;
    // Re-find chapter trak after possible insert (offsets shifted).
    let chapter_trak = find_chapter_trak(moov, chapter_id)?;
    changed |= ensure_text_stsd(moov, chapter_trak)?;
    Ok(changed)
}

fn find_chapter_trak(moov: &[u8], chapter_id: u32) -> Result<AtomSpan> {
    let moov_span = AtomSpan {
        offset: 0,
        size: moov.len(),
        header: 8,
    };
    for trak in iter_children(moov, moov_span)
        .into_iter()
        .filter(|a| atom_type(moov, *a) == *b"trak")
    {
        if let Some(tkhd) = iter_children(moov, trak)
            .into_iter()
            .find(|a| atom_type(moov, *a) == *b"tkhd")
        {
            if read_tkhd_id(moov, tkhd)? == chapter_id {
                return Ok(trak);
            }
        }
    }
    Err(DecryptError::Native(
        "chapter trak disappeared during AVFoundation patch".into(),
    ))
}

fn read_mvhd_duration(data: &[u8], mvhd: AtomSpan) -> Result<u32> {
    let p = mvhd.offset + mvhd.header;
    let version = data[p];
    let dur = if version == 0 {
        u32::from_be_bytes(data[p + 16..p + 20].try_into().unwrap())
    } else {
        let d = u64::from_be_bytes(data[p + 24..p + 32].try_into().unwrap());
        u32::try_from(d).unwrap_or(u32::MAX)
    };
    Ok(dur)
}

fn read_tkhd_id(data: &[u8], tkhd: AtomSpan) -> Result<u32> {
    let p = tkhd.offset + tkhd.header;
    let version = data[p];
    let id = if version == 0 {
        u32::from_be_bytes(data[p + 12..p + 16].try_into().unwrap())
    } else {
        u32::from_be_bytes(data[p + 20..p + 24].try_into().unwrap())
    };
    Ok(id)
}

fn ensure_edts(moov: &mut Vec<u8>, trak: AtomSpan, movie_duration: u32) -> Result<bool> {
    let kids = iter_children(moov, trak);
    if kids.iter().any(|a| atom_type(moov, *a) == *b"edts") {
        return Ok(false);
    }
    let Some(tkhd) = kids.iter().find(|a| atom_type(moov, **a) == *b"tkhd") else {
        return Err(DecryptError::Native("chapter trak missing tkhd".into()));
    };
    let insert_at = tkhd.offset + tkhd.size;
    let edts = build_edts(movie_duration);
    moov.splice(insert_at..insert_at, edts.iter().copied());
    // Update trak + moov sizes.
    set_atom_size(moov, trak.offset, trak.size + edts.len())?;
    let moov_len = moov.len();
    set_atom_size(moov, 0, moov_len)?;
    Ok(true)
}

fn build_edts(movie_duration: u32) -> Vec<u8> {
    // elst: header(8) + ver/flags(4) + count(4) + entry(12) = 28
    let mut elst = Vec::with_capacity(28);
    elst.extend_from_slice(&28u32.to_be_bytes());
    elst.extend_from_slice(b"elst");
    elst.extend_from_slice(&0u32.to_be_bytes()); // version + flags
    elst.extend_from_slice(&1u32.to_be_bytes()); // entry count
    elst.extend_from_slice(&movie_duration.to_be_bytes());
    elst.extend_from_slice(&0u32.to_be_bytes()); // media time
    elst.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // media rate 1.0
    let mut edts = Vec::with_capacity(8 + elst.len());
    edts.extend_from_slice(&((8 + elst.len()) as u32).to_be_bytes());
    edts.extend_from_slice(b"edts");
    edts.extend_from_slice(&elst);
    edts
}

fn ensure_text_stsd(moov: &mut Vec<u8>, trak: AtomSpan) -> Result<bool> {
    let Some(mdia) = iter_children(moov, trak)
        .into_iter()
        .find(|a| atom_type(moov, *a) == *b"mdia")
    else {
        return Ok(false);
    };
    let Some(minf) = iter_children(moov, mdia)
        .into_iter()
        .find(|a| atom_type(moov, *a) == *b"minf")
    else {
        return Ok(false);
    };
    let Some(stbl) = iter_children(moov, minf)
        .into_iter()
        .find(|a| atom_type(moov, *a) == *b"stbl")
    else {
        return Ok(false);
    };
    let Some(stsd) = iter_children(moov, stbl)
        .into_iter()
        .find(|a| atom_type(moov, *a) == *b"stsd")
    else {
        return Ok(false);
    };

    // stsd layout: size(4) type(4) ver/flags(4) entry_count(4) then entries
    let entries_at = stsd.offset + stsd.header + 8;
    if entries_at + 8 > stsd.offset + stsd.size {
        return Ok(false);
    }
    let entry_size =
        u32::from_be_bytes(moov[entries_at..entries_at + 4].try_into().unwrap()) as usize;
    let entry_type = &moov[entries_at + 4..entries_at + 8];
    if entry_type != b"text" {
        return Ok(false);
    }
    if entry_size < 16 || entries_at + entry_size > stsd.offset + stsd.size {
        return Ok(false);
    }

    // Already AVFoundation-shaped?
    let reserved_ok = moov[entries_at + 8..entries_at + 14] == [0; 6];
    let data_ref = u16::from_be_bytes(moov[entries_at + 14..entries_at + 16].try_into().unwrap());
    let display_flags = if entry_size >= 20 {
        u32::from_be_bytes(moov[entries_at + 16..entries_at + 20].try_into().unwrap())
    } else {
        0
    };
    if reserved_ok
        && data_ref == 1
        && display_flags == 1
        && entry_size == AVF_TEXT_SAMPLE_ENTRY.len()
    {
        return Ok(false);
    }

    let old_entry_size = entry_size;
    let new_entry = AVF_TEXT_SAMPLE_ENTRY;
    let delta = new_entry.len() as isize - old_entry_size as isize;
    moov.splice(
        entries_at..entries_at + old_entry_size,
        new_entry.iter().copied(),
    );

    // Update ancestor sizes along the chain.
    let delta_usize = delta.unsigned_abs();
    if delta >= 0 {
        bump_size(moov, stsd.offset, delta_usize)?;
        // Re-find ancestors by walking from moov (offsets after splice shifted
        // only at/after entries_at; ancestors start before entries_at so their
        // headers are stable).
        bump_size(moov, stbl.offset, delta_usize)?;
        bump_size(moov, minf.offset, delta_usize)?;
        bump_size(moov, mdia.offset, delta_usize)?;
        bump_size(moov, trak.offset, delta_usize)?;
        let moov_len = moov.len();
        set_atom_size(moov, 0, moov_len)?;
    } else {
        // Shrink — same bump with saturating sub via set.
        set_atom_size(moov, stsd.offset, stsd.size - delta_usize)?;
        set_atom_size(moov, stbl.offset, stbl.size - delta_usize)?;
        set_atom_size(moov, minf.offset, minf.size - delta_usize)?;
        set_atom_size(moov, mdia.offset, mdia.size - delta_usize)?;
        set_atom_size(moov, trak.offset, trak.size - delta_usize)?;
        let moov_len = moov.len();
        set_atom_size(moov, 0, moov_len)?;
    }
    Ok(true)
}

fn bump_size(buf: &mut [u8], atom_offset: usize, delta: usize) -> Result<()> {
    let old = u32::from_be_bytes(buf[atom_offset..atom_offset + 4].try_into().unwrap()) as usize;
    set_atom_size(buf, atom_offset, old + delta)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::{fixup_audiobook, FixupRequest};
    use crate::package_m4b::package_m4b_from_pcm;

    fn has_edts(path: &Path) -> bool {
        let data = std::fs::read(path).unwrap();
        let moov = find_atom(&data, 0, data.len(), *b"moov").unwrap();
        let moov_buf = &data[moov.offset..moov.offset + moov.size];
        let moov_span = AtomSpan {
            offset: 0,
            size: moov_buf.len(),
            header: 8,
        };
        for trak in iter_children(moov_buf, moov_span)
            .into_iter()
            .filter(|a| atom_type(moov_buf, *a) == *b"trak")
        {
            let kids = iter_children(moov_buf, trak);
            let is_text = kids.iter().any(|a| {
                if atom_type(moov_buf, *a) != *b"mdia" {
                    return false;
                }
                iter_children(moov_buf, *a).into_iter().any(|h| {
                    atom_type(moov_buf, h) == *b"hdlr"
                        && moov_buf[h.offset + h.header + 8..h.offset + h.header + 12] == *b"text"
                })
            });
            if is_text {
                return kids.iter().any(|a| atom_type(moov_buf, *a) == *b"edts");
            }
        }
        false
    }

    fn text_stsd_display_flags(path: &Path) -> Option<(bool, u16, u32, usize)> {
        let data = std::fs::read(path).unwrap();
        let moov = find_atom(&data, 0, data.len(), *b"moov")?;
        let moov_buf = &data[moov.offset..moov.offset + moov.size];
        let moov_span = AtomSpan {
            offset: 0,
            size: moov_buf.len(),
            header: 8,
        };
        for trak in iter_children(moov_buf, moov_span)
            .into_iter()
            .filter(|a| atom_type(moov_buf, *a) == *b"trak")
        {
            let Some(mdia) = iter_children(moov_buf, trak)
                .into_iter()
                .find(|a| atom_type(moov_buf, *a) == *b"mdia")
            else {
                continue;
            };
            let is_text = iter_children(moov_buf, mdia).into_iter().any(|h| {
                atom_type(moov_buf, h) == *b"hdlr"
                    && moov_buf[h.offset + h.header + 8..h.offset + h.header + 12] == *b"text"
            });
            if !is_text {
                continue;
            }
            let minf = iter_children(moov_buf, mdia)
                .into_iter()
                .find(|a| atom_type(moov_buf, *a) == *b"minf")?;
            let stbl = iter_children(moov_buf, minf)
                .into_iter()
                .find(|a| atom_type(moov_buf, *a) == *b"stbl")?;
            let stsd = iter_children(moov_buf, stbl)
                .into_iter()
                .find(|a| atom_type(moov_buf, *a) == *b"stsd")?;
            let entries_at = stsd.offset + stsd.header + 8;
            let entry_size =
                u32::from_be_bytes(moov_buf[entries_at..entries_at + 4].try_into().unwrap())
                    as usize;
            let reserved_ok = moov_buf[entries_at + 8..entries_at + 14] == [0; 6];
            let data_ref = u16::from_be_bytes(
                moov_buf[entries_at + 14..entries_at + 16]
                    .try_into()
                    .unwrap(),
            );
            let flags = u32::from_be_bytes(
                moov_buf[entries_at + 16..entries_at + 20]
                    .try_into()
                    .unwrap(),
            );
            return Some((reserved_ok, data_ref, flags, entry_size));
        }
        None
    }

    #[tokio::test]
    async fn patches_mp4ameta_chapter_track_for_avfoundation() {
        let sample_rate = 16_000u32;
        let pcm = vec![0i16; sample_rate as usize]; // 1s silence
        let dir = tempfile::tempdir().unwrap();
        let packaged = dir.path().join("raw.m4b");
        package_m4b_from_pcm(&pcm, sample_rate, 1, &packaged, &[]).unwrap();

        let fixed = dir.path().join("fixed.m4b");
        fixup_audiobook(FixupRequest {
            input: packaged,
            output: fixed.clone(),
            title: "Test Book".into(),
            author: Some("Author".into()),
            narrator: None,
            cover: None,
            chapters: vec![("Opening".into(), 0), ("Chapter 1".into(), 500)],
            replace_chapters: true,
            subtitle: None,
            publisher: None,
            year: None,
            genre: None,
            series: None,
            series_index: None,
            asin: None,
            isbn: None,
            description: None,
            language: None,
            tool: None,
        })
        .await
        .unwrap();

        assert!(has_edts(&fixed), "chapter trak should have edts/elst");
        let (reserved_ok, data_ref, flags, entry_size) =
            text_stsd_display_flags(&fixed).expect("text stsd");
        assert!(reserved_ok, "SampleEntry reserved must be zero");
        assert_eq!(data_ref, 1);
        assert_eq!(flags, 1, "displayFlags must be 1 for AVFoundation");
        assert_eq!(entry_size, AVF_TEXT_SAMPLE_ENTRY.len());

        // Idempotent.
        assert!(!ensure_avfoundation_chapter_track(&fixed).unwrap());
    }

    #[test]
    fn build_edts_has_expected_layout() {
        let edts = build_edts(12_345);
        assert_eq!(&edts[4..8], b"edts");
        assert_eq!(&edts[12..16], b"elst");
        let dur = u32::from_be_bytes(edts[24..28].try_into().unwrap());
        assert_eq!(dur, 12_345);
    }
}
