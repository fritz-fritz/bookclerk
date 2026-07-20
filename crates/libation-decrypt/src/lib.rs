//! Decrypt pipeline: native Rust Adrm + DASH/CENC remux (ffmpeg optional fallback/mp3/fixup).

mod brand;
mod crypto;
mod error;
mod metadata;
mod mp4;
mod native;

pub use brand::{
    brand_durations_from_chapter_info, brand_trim_range, rebase_chapters_after_brand_trim,
    BrandDurations,
};
pub use error::{DecryptError, Result};
pub use metadata::{fixup_audiobook, FixupRequest};
pub use mp4::{decrypt_dash_cenc, track_duration_ms, TrimRange};
pub use native::{decrypt_adrm_native, decrypt_cenc_native};

use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::process::Command;

/// Input for an Adrm aaxc decrypt job.
#[derive(Debug, Clone)]
pub struct DecryptRequest {
    /// Encrypted AAX/AAXC file.
    pub input: PathBuf,
    /// Destination m4b/m4a path.
    pub output: PathBuf,
    /// Adrm aaxc content key (hex) — preferred modern path.
    pub audible_key: Option<String>,
    /// Adrm aaxc IV (hex).
    pub audible_iv: Option<String>,
    /// Legacy AAX activation bytes (unsupported by the native key/iv path).
    pub activation_bytes: Option<String>,
    /// Optional brand / chapter trim window (milliseconds).
    pub trim: Option<TrimRange>,
    /// Deprecated: ignored. Native decrypt no longer shells out to aaxclean-cli.
    pub aaxclean_bin: Option<PathBuf>,
}

/// Input for a Widevine CENC decrypt job.
#[derive(Debug, Clone)]
pub struct CencDecryptRequest {
    pub input: PathBuf,
    pub output: PathBuf,
    /// 32-hex key id.
    pub kid: String,
    /// 32-hex content key.
    pub key: String,
    pub trim: Option<TrimRange>,
    /// Deprecated: ignored.
    pub aaxclean_bin: Option<PathBuf>,
    pub ffmpeg_bin: Option<PathBuf>,
}

/// Outcome of a successful decrypt / encode.
#[derive(Debug, Clone)]
pub struct DecryptOutcome {
    pub output: PathBuf,
}

fn resolve_ffmpeg_bin(explicit: Option<&Path>) -> PathBuf {
    if let Some(path) = explicit {
        return path.to_path_buf();
    }
    if let Ok(path) = std::env::var("LIBATION_FFMPEG") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    PathBuf::from("ffmpeg")
}

/// Decrypt Adrm aaxc using the native Rust remuxer (no external binary).
pub async fn decrypt_with_aaxclean(req: DecryptRequest) -> Result<DecryptOutcome> {
    // Kept name for call-site compatibility; implementation is native.
    decrypt_adrm(req).await
}

/// Decrypt Adrm aaxc natively. Falls back to ffmpeg `-audible_key`/`-audible_iv`
/// when the native remuxer cannot handle the file.
pub async fn decrypt_adrm(req: DecryptRequest) -> Result<DecryptOutcome> {
    if !req.input.exists() {
        return Err(DecryptError::InputMissing(req.input));
    }
    let (Some(key), Some(iv)) = (&req.audible_key, &req.audible_iv) else {
        if req.activation_bytes.is_some() {
            return Err(DecryptError::UnsupportedActivationBytes);
        }
        return Err(DecryptError::MissingCredentials);
    };
    if let Some(parent) = req.output.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let input = req.input.clone();
    let output = req.output.clone();
    let key = key.clone();
    let iv = iv.clone();
    let trim = req.trim;
    let native =
        tokio::task::spawn_blocking(move || decrypt_adrm_native(&input, &output, &key, &iv, trim))
            .await
            .map_err(|err| DecryptError::Native(format!("decrypt task join error: {err}")))?;

    match native {
        Ok(outcome) => Ok(outcome),
        Err(err) => {
            tracing::warn!(
                error = %err,
                "native Adrm decrypt failed; trying ffmpeg audible_key/iv"
            );
            decrypt_adrm_ffmpeg(&req).await
        }
    }
}

async fn decrypt_adrm_ffmpeg(req: &DecryptRequest) -> Result<DecryptOutcome> {
    let (Some(key), Some(iv)) = (&req.audible_key, &req.audible_iv) else {
        return Err(DecryptError::MissingCredentials);
    };
    let ffmpeg = resolve_ffmpeg_bin(None);
    if !tool_available(&ffmpeg).await {
        return Err(DecryptError::FfmpegNotFound(ffmpeg));
    }

    // ffmpeg cannot apply brand trim during decrypt; callers should trim via
    // native path. Here we decrypt the full file.
    tracing::info!(
        input = %req.input.display(),
        output = %req.output.display(),
        bin = %ffmpeg.display(),
        "running ffmpeg Adrm decrypt"
    );
    let output = Command::new(&ffmpeg)
        .args([
            "-y",
            "-nostdin",
            "-loglevel",
            "error",
            "-audible_key",
            key,
            "-audible_iv",
            iv,
            "-i",
            &req.input.display().to_string(),
            "-c",
            "copy",
            "-map_metadata",
            "0",
            "-movflags",
            "+faststart",
            &req.output.display().to_string(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(DecryptError::Io)?;

    if !output.status.success() {
        let _ = tokio::fs::remove_file(&req.output).await;
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(DecryptError::FfmpegFailed {
            status: output.status.code(),
            stderr: stderr.trim().to_string(),
        });
    }
    if !req.output.exists() {
        return Err(DecryptError::OutputMissing(req.output.clone()));
    }
    Ok(DecryptOutcome {
        output: req.output.clone(),
    })
}

/// Decrypt Widevine CENC. Tries native DASH/progressive path, then ffmpeg.
pub async fn decrypt_cenc(req: CencDecryptRequest) -> Result<DecryptOutcome> {
    if !req.input.exists() {
        return Err(DecryptError::InputMissing(req.input.clone()));
    }
    if let Some(parent) = req.output.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let input = req.input.clone();
    let output = req.output.clone();
    let kid = req.kid.clone();
    let key = req.key.clone();
    let trim = req.trim;
    let native =
        tokio::task::spawn_blocking(move || decrypt_cenc_native(&input, &output, &kid, &key, trim))
            .await
            .map_err(|err| DecryptError::Native(format!("decrypt task join error: {err}")))?;

    if let Ok(outcome) = native {
        return Ok(outcome);
    }
    if let Err(ref err) = native {
        tracing::debug!(error = %err, "native CENC unavailable; using ffmpeg");
    }

    let ffmpeg = resolve_ffmpeg_bin(req.ffmpeg_bin.as_deref());
    if !tool_available(&ffmpeg).await {
        return Err(DecryptError::FfmpegNotFound(ffmpeg));
    }

    tracing::info!(
        input = %req.input.display(),
        output = %req.output.display(),
        bin = %ffmpeg.display(),
        "running ffmpeg CENC decrypt"
    );
    let output = Command::new(&ffmpeg)
        .args([
            "-y",
            "-nostdin",
            "-loglevel",
            "error",
            "-decryption_key",
            &req.key,
            "-i",
            &req.input.display().to_string(),
            "-c",
            "copy",
            "-map_metadata",
            "0",
            "-movflags",
            "+faststart",
            &req.output.display().to_string(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(DecryptError::Io)?;

    if !output.status.success() {
        let _ = tokio::fs::remove_file(&req.output).await;
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(DecryptError::FfmpegFailed {
            status: output.status.code(),
            stderr: stderr.trim().to_string(),
        });
    }
    if !req.output.exists() {
        return Err(DecryptError::OutputMissing(req.output.clone()));
    }
    Ok(DecryptOutcome { output: req.output })
}

/// Re-encode audio to MP3 via ffmpeg (classic Libation `DecryptToLossy`).
pub async fn encode_to_mp3(
    input: &Path,
    output: &Path,
    ffmpeg_bin: Option<&Path>,
    lame: &libation_config::LameConfig,
    max_sample_rate: Option<u32>,
) -> Result<DecryptOutcome> {
    if !input.exists() {
        return Err(DecryptError::InputMissing(input.to_path_buf()));
    }
    if let Some(parent) = output.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let ffmpeg = resolve_ffmpeg_bin(ffmpeg_bin);
    if !tool_available(&ffmpeg).await {
        return Err(DecryptError::FfmpegNotFound(ffmpeg));
    }

    tracing::info!(
        input = %input.display(),
        output = %output.display(),
        bin = %ffmpeg.display(),
        "running ffmpeg mp3 encode"
    );
    let mut args = vec![
        "-y".to_string(),
        "-nostdin".to_string(),
        "-loglevel".to_string(),
        "error".to_string(),
        "-i".to_string(),
        input.display().to_string(),
        "-codec:a".to_string(),
        "libmp3lame".to_string(),
    ];
    if lame.constant_bitrate || lame.target.eq_ignore_ascii_case("bitrate") {
        args.push("-b:a".into());
        args.push(format!("{}k", lame.bitrate_kbps));
    } else {
        args.push("-qscale:a".into());
        args.push(lame.vbr_quality.to_string());
    }
    if lame.downsample_mono || lame.mode.eq_ignore_ascii_case("mono") {
        args.push("-ac".into());
        args.push("1".into());
    }
    if let Some(max_hz) = max_sample_rate {
        args.push("-ar".into());
        args.push(max_hz.to_string());
    }
    args.push("-map_metadata".into());
    args.push("0".into());
    args.push("-id3v2_version".into());
    args.push("3".into());
    args.push(output.display().to_string());

    let output_status = Command::new(&ffmpeg)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(DecryptError::Io)?;

    if !output_status.status.success() {
        let _ = tokio::fs::remove_file(output).await;
        let stderr = String::from_utf8_lossy(&output_status.stderr);
        return Err(DecryptError::FfmpegFailed {
            status: output_status.status.code(),
            stderr: stderr.trim().to_string(),
        });
    }
    if !output.exists() {
        return Err(DecryptError::OutputMissing(output.to_path_buf()));
    }
    Ok(DecryptOutcome {
        output: output.to_path_buf(),
    })
}

/// Locate an external tool without executing it.
fn find_tool(bin: &Path) -> Option<PathBuf> {
    if bin.as_os_str().is_empty() {
        return None;
    }
    if bin.is_absolute() || bin.components().count() > 1 {
        return bin.is_file().then(|| bin.to_path_buf());
    }
    let path_os = std::env::var_os("PATH")?;
    find_tool_in_dirs(bin, std::env::split_paths(&path_os))
}

fn find_tool_in_dirs<I>(bin: &Path, dirs: I) -> Option<PathBuf>
where
    I: IntoIterator<Item = PathBuf>,
{
    let name = bin.as_os_str();
    for dir in dirs {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        if let Some(found) = find_with_pathext(&dir, name) {
            return Some(found);
        }
    }
    None
}

#[cfg(windows)]
fn find_with_pathext(dir: &Path, name: &std::ffi::OsStr) -> Option<PathBuf> {
    if Path::new(name).extension().is_some() {
        return None;
    }
    let pathext = std::env::var_os("PATHEXT")?;
    for ext in pathext.to_string_lossy().split(';') {
        let ext = ext.trim();
        if ext.is_empty() {
            continue;
        }
        let mut file_name = name.to_os_string();
        file_name.push(ext);
        let candidate = dir.join(file_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

async fn tool_available(bin: &Path) -> bool {
    find_tool(bin).is_some()
}

/// Always true — Adrm decrypt is native and does not need aaxclean-cli.
pub async fn aaxclean_available(_bin: Option<&Path>) -> bool {
    true
}

/// True when `ffmpeg` appears to be available.
pub async fn ffmpeg_available(bin: Option<&Path>) -> bool {
    tool_available(&resolve_ffmpeg_bin(bin)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{decrypt_aavd_sample_in_place, parse_aes128_hex};
    use cbc::cipher::{block_padding::NoPadding, BlockModeEncrypt, KeyIvInit};
    use std::io::Write;

    type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;

    #[tokio::test]
    async fn activation_bytes_alone_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("in.aax");
        std::fs::write(&input, b"not-a-real-aax").unwrap();
        let req = DecryptRequest {
            input,
            output: dir.path().join("out.m4b"),
            audible_key: None,
            audible_iv: None,
            activation_bytes: Some("deadbeef".into()),
            trim: None,
            aaxclean_bin: None,
        };
        let err = decrypt_adrm(req).await.unwrap_err();
        assert!(matches!(err, DecryptError::UnsupportedActivationBytes));
    }

    #[tokio::test]
    async fn tool_available_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let tool = dir.path().join("stub-ffmpeg");
        std::fs::write(&tool, b"stub").unwrap();
        assert!(tool_available(&tool).await);
    }

    /// Build a minimal progressive MP4 with one encrypted AAC-like sample and remux it.
    #[test]
    fn native_adrm_roundtrip_synthetic() {
        let key = parse_aes128_hex("00112233445566778899aabbccddeeff").unwrap();
        let iv = parse_aes128_hex("ffeeddccbbaa99887766554433221100").unwrap();

        let mut plain = vec![0u8; 32];
        for (i, b) in plain.iter_mut().enumerate() {
            *b = (i * 7) as u8;
        }
        let mut enc = plain.clone();
        {
            let len = enc.len();
            Aes128CbcEnc::new(&key.into(), &iv.into())
                .encrypt_padded::<NoPadding>(&mut enc, len)
                .unwrap();
        }
        assert_ne!(enc, plain);

        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("book.aaxc");
        let output = dir.path().join("book.m4b");
        write_synthetic_aavd_mp4(&input, &enc).unwrap();

        decrypt_adrm_native(
            &input,
            &output,
            "00112233445566778899aabbccddeeff",
            "ffeeddccbbaa99887766554433221100",
            None,
        )
        .unwrap();

        assert!(output.exists());
        let out_mp4 = crate::mp4::parse_mp4(&output).unwrap();
        assert_eq!(out_mp4.audio.samples.len(), 1);
        let sample = &out_mp4.audio.samples[0];
        let mut got = vec![0u8; sample.size as usize];
        let mut f = std::fs::File::open(&output).unwrap();
        use std::io::{Read, Seek, SeekFrom};
        f.seek(SeekFrom::Start(sample.offset)).unwrap();
        f.read_exact(&mut got).unwrap();
        assert_eq!(got, plain);

        // Trim that keeps the only sample should still succeed.
        decrypt_adrm_native(
            &input,
            &output,
            "00112233445566778899aabbccddeeff",
            "ffeeddccbbaa99887766554433221100",
            Some(TrimRange {
                start_ms: 0,
                end_ms: Some(10_000),
            }),
        )
        .unwrap();
    }

    fn write_synthetic_aavd_mp4(path: &Path, encrypted_sample: &[u8]) -> std::io::Result<()> {
        // Layout: ftyp + moov(stbl pointing at mdat sample) + mdat
        // We write mdat first after a placeholder moov size, then patch offsets —
        // simpler: write ftyp, mdat, then moov with absolute offsets known up front.

        let mut ftyp = Vec::new();
        let brands = [b"aaxc", b"aax ", b"isom"];
        let ftyp_size = 8 + 8 + brands.len() * 4;
        ftyp.extend_from_slice(&(ftyp_size as u32).to_be_bytes());
        ftyp.extend_from_slice(b"ftyp");
        ftyp.extend_from_slice(b"aaxc");
        ftyp.extend_from_slice(&0u32.to_be_bytes());
        for b in brands {
            ftyp.extend_from_slice(b);
        }

        // mdat with one sample
        let mut mdat = Vec::new();
        let mdat_size = 8 + encrypted_sample.len();
        mdat.extend_from_slice(&(mdat_size as u32).to_be_bytes());
        mdat.extend_from_slice(b"mdat");
        mdat.extend_from_slice(encrypted_sample);

        let mdat_file_offset = ftyp.len();
        let sample_offset = (mdat_file_offset + 8) as u32;

        // Build a minimal moov with one audio track / one sample.
        let stsd = {
            // sample entry aavd (minimal AudioSampleEntry)
            let mut e = Vec::new();
            e.extend_from_slice(&0u32.to_be_bytes()); // size placeholder
            e.extend_from_slice(b"aavd");
            e.extend_from_slice(&[0u8; 6]); // reserved
            e.extend_from_slice(&1u16.to_be_bytes()); // data_reference_index
            e.extend_from_slice(&[0u8; 8]); // reserved
            e.extend_from_slice(&2u16.to_be_bytes()); // channelcount
            e.extend_from_slice(&16u16.to_be_bytes()); // samplesize
            e.extend_from_slice(&0u16.to_be_bytes()); // pre_defined
            e.extend_from_slice(&0u16.to_be_bytes()); // reserved
            e.extend_from_slice(&(44100u32 << 16).to_be_bytes()); // samplerate
            let esize = e.len() as u32;
            e[0..4].copy_from_slice(&esize.to_be_bytes());

            let mut b = Vec::new();
            let size = 8 + 4 + 4 + e.len();
            b.extend_from_slice(&(size as u32).to_be_bytes());
            b.extend_from_slice(b"stsd");
            b.extend_from_slice(&0u32.to_be_bytes());
            b.extend_from_slice(&1u32.to_be_bytes());
            b.extend_from_slice(&e);
            b
        };
        let stts = {
            let mut b = Vec::new();
            let size = 8 + 4 + 4 + 8;
            b.extend_from_slice(&(size as u32).to_be_bytes());
            b.extend_from_slice(b"stts");
            b.extend_from_slice(&0u32.to_be_bytes());
            b.extend_from_slice(&1u32.to_be_bytes());
            b.extend_from_slice(&1u32.to_be_bytes());
            b.extend_from_slice(&1024u32.to_be_bytes());
            b
        };
        let stsc = {
            let mut b = Vec::new();
            let size = 8 + 4 + 4 + 12;
            b.extend_from_slice(&(size as u32).to_be_bytes());
            b.extend_from_slice(b"stsc");
            b.extend_from_slice(&0u32.to_be_bytes());
            b.extend_from_slice(&1u32.to_be_bytes());
            b.extend_from_slice(&1u32.to_be_bytes());
            b.extend_from_slice(&1u32.to_be_bytes());
            b.extend_from_slice(&1u32.to_be_bytes());
            b
        };
        let stsz = {
            let mut b = Vec::new();
            let size = 8 + 4 + 4 + 4 + 4;
            b.extend_from_slice(&(size as u32).to_be_bytes());
            b.extend_from_slice(b"stsz");
            b.extend_from_slice(&0u32.to_be_bytes());
            b.extend_from_slice(&0u32.to_be_bytes());
            b.extend_from_slice(&1u32.to_be_bytes());
            b.extend_from_slice(&(encrypted_sample.len() as u32).to_be_bytes());
            b
        };
        let stco = {
            let mut b = Vec::new();
            let size = 8 + 4 + 4 + 4;
            b.extend_from_slice(&(size as u32).to_be_bytes());
            b.extend_from_slice(b"stco");
            b.extend_from_slice(&0u32.to_be_bytes());
            b.extend_from_slice(&1u32.to_be_bytes());
            b.extend_from_slice(&sample_offset.to_be_bytes());
            b
        };

        let stbl = wrap_box(b"stbl", &[stsd, stts, stsc, stsz, stco].concat());
        let smhd = {
            let mut b = Vec::new();
            let size = 16;
            b.extend_from_slice(&(size as u32).to_be_bytes());
            b.extend_from_slice(b"smhd");
            b.extend_from_slice(&0u32.to_be_bytes());
            b.extend_from_slice(&0u32.to_be_bytes());
            b
        };
        let dinf = {
            let mut dref_entry = Vec::new();
            dref_entry.extend_from_slice(&12u32.to_be_bytes());
            dref_entry.extend_from_slice(b"url ");
            dref_entry.extend_from_slice(&1u32.to_be_bytes()); // self-contained flag
            let mut dref = Vec::new();
            let dref_size = 8 + 4 + 4 + dref_entry.len();
            dref.extend_from_slice(&(dref_size as u32).to_be_bytes());
            dref.extend_from_slice(b"dref");
            dref.extend_from_slice(&0u32.to_be_bytes());
            dref.extend_from_slice(&1u32.to_be_bytes());
            dref.extend_from_slice(&dref_entry);
            wrap_box(b"dinf", &dref)
        };
        let minf = wrap_box(b"minf", &[smhd, dinf, stbl].concat());
        let hdlr = {
            let mut b = Vec::new();
            let name = b"SoundHandler\0";
            let size = 8 + 4 + 4 + 4 + 12 + name.len();
            b.extend_from_slice(&(size as u32).to_be_bytes());
            b.extend_from_slice(b"hdlr");
            b.extend_from_slice(&0u32.to_be_bytes());
            b.extend_from_slice(&0u32.to_be_bytes());
            b.extend_from_slice(b"soun");
            b.extend_from_slice(&[0u8; 12]);
            b.extend_from_slice(name);
            b
        };
        let mdhd = {
            let mut b = Vec::new();
            let size = 8 + 4 + 4 + 4 + 4 + 4 + 2 + 2;
            b.extend_from_slice(&(size as u32).to_be_bytes());
            b.extend_from_slice(b"mdhd");
            b.extend_from_slice(&0u32.to_be_bytes());
            b.extend_from_slice(&0u32.to_be_bytes());
            b.extend_from_slice(&0u32.to_be_bytes());
            b.extend_from_slice(&44100u32.to_be_bytes());
            b.extend_from_slice(&1024u32.to_be_bytes());
            b.extend_from_slice(&0x55c4u16.to_be_bytes()); // lang
            b.extend_from_slice(&0u16.to_be_bytes());
            b
        };
        let mdia = wrap_box(b"mdia", &[mdhd, hdlr, minf].concat());
        let tkhd = {
            let mut b = Vec::new();
            // version0 tkhd: 84 bytes content typically; keep a compact valid-ish header
            let size = 92;
            b.extend_from_slice(&(size as u32).to_be_bytes());
            b.extend_from_slice(b"tkhd");
            b.extend_from_slice(&0x000003u32.to_be_bytes()); // flags=track enabled+in movie+preview
            b.extend_from_slice(&0u32.to_be_bytes()); // ctime
            b.extend_from_slice(&0u32.to_be_bytes()); // mtime
            b.extend_from_slice(&1u32.to_be_bytes()); // track id
            b.extend_from_slice(&0u32.to_be_bytes()); // reserved
            b.extend_from_slice(&1024u32.to_be_bytes()); // duration
            b.extend_from_slice(&[0u8; 8]); // reserved
            b.extend_from_slice(&0u16.to_be_bytes()); // layer
            b.extend_from_slice(&0u16.to_be_bytes()); // alternate
            b.extend_from_slice(&0x0100u16.to_be_bytes()); // volume
            b.extend_from_slice(&0u16.to_be_bytes());
            // matrix 36 bytes identity
            let matrix = [0x00010000u32, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000];
            for v in matrix {
                b.extend_from_slice(&v.to_be_bytes());
            }
            b.extend_from_slice(&0u32.to_be_bytes()); // width
            b.extend_from_slice(&0u32.to_be_bytes()); // height
            assert_eq!(b.len(), size);
            b
        };
        let trak = wrap_box(b"trak", &[tkhd, mdia].concat());
        let mvhd = {
            let mut b = Vec::new();
            let size = 108;
            b.extend_from_slice(&(size as u32).to_be_bytes());
            b.extend_from_slice(b"mvhd");
            b.extend_from_slice(&0u32.to_be_bytes());
            b.extend_from_slice(&0u32.to_be_bytes());
            b.extend_from_slice(&0u32.to_be_bytes());
            b.extend_from_slice(&44100u32.to_be_bytes());
            b.extend_from_slice(&1024u32.to_be_bytes());
            b.extend_from_slice(&0x00010000u32.to_be_bytes()); // rate
            b.extend_from_slice(&0x0100u16.to_be_bytes()); // volume
            b.extend_from_slice(&0u16.to_be_bytes());
            b.extend_from_slice(&[0u8; 8]); // reserved
            let matrix = [0x00010000u32, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000];
            for v in matrix {
                b.extend_from_slice(&v.to_be_bytes());
            }
            b.extend_from_slice(&[0u8; 24]); // pre_defined
            b.extend_from_slice(&2u32.to_be_bytes()); // next_track_ID
            assert_eq!(b.len(), size);
            b
        };
        let moov = wrap_box(b"moov", &[mvhd, trak].concat());

        let mut file = std::fs::File::create(path)?;
        file.write_all(&ftyp)?;
        file.write_all(&mdat)?;
        file.write_all(&moov)?;
        Ok(())
    }

    fn wrap_box(kind: &[u8; 4], content: &[u8]) -> Vec<u8> {
        let size = 8 + content.len();
        let mut b = Vec::with_capacity(size);
        b.extend_from_slice(&(size as u32).to_be_bytes());
        b.extend_from_slice(kind);
        b.extend_from_slice(content);
        b
    }

    #[test]
    fn decrypt_helper_matches_fixture_crypto() {
        let key = [1u8; 16];
        let iv = [2u8; 16];
        let mut data = vec![9u8; 16];
        let original = data.clone();
        {
            let len = data.len();
            Aes128CbcEnc::new(&key.into(), &iv.into())
                .encrypt_padded::<NoPadding>(&mut data, len)
                .unwrap();
        }
        decrypt_aavd_sample_in_place(&key, &iv, &mut data);
        assert_eq!(data, original);
    }
}
