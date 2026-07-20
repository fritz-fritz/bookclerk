//! Native Rust Adrm / CENC decrypt entry points (no aaxclean-cli).

use std::path::Path;

use crate::crypto::parse_aes128_hex;
use crate::error::{DecryptError, Result};
use crate::mp4::{
    decrypt_and_remux, decrypt_dash_cenc, looks_like_dash, parse_mp4, DecryptMode, RemuxOptions,
    SampleEntryKind, TrimRange,
};
use crate::DecryptOutcome;

/// Native Adrm aaxc decrypt (+ optional brand trim) to a DRM-free M4B.
pub fn decrypt_adrm_native(
    input: &Path,
    output: &Path,
    audible_key_hex: &str,
    audible_iv_hex: &str,
    trim: Option<TrimRange>,
) -> Result<DecryptOutcome> {
    if !input.exists() {
        return Err(DecryptError::InputMissing(input.to_path_buf()));
    }
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let key = parse_aes128_hex(audible_key_hex)?;
    let iv = parse_aes128_hex(audible_iv_hex)?;

    // Validate the input looks like an Adrm / aaxc file when possible.
    match parse_mp4(input) {
        Ok(mp4) => match mp4.audio.sample_entry_kind {
            SampleEntryKind::Aavd | SampleEntryKind::Mp4a => {}
            SampleEntryKind::Enca => {
                return Err(DecryptError::Native(
                    "file looks like CENC (enca); use decrypt_cenc_native / decrypt_cenc".into(),
                ));
            }
            SampleEntryKind::Other(kind) => {
                tracing::warn!(
                    sample_entry = %kind,
                    "unexpected audio sample entry; attempting Adrm decrypt anyway"
                );
            }
        },
        Err(err) => {
            tracing::warn!(error = %err, "MP4 pre-parse failed; attempting Adrm decrypt anyway");
        }
    }

    tracing::info!(
        input = %input.display(),
        output = %output.display(),
        trim = trim.is_some(),
        "native Adrm aaxc decrypt"
    );

    decrypt_and_remux(
        input,
        output,
        &RemuxOptions {
            decrypt: DecryptMode::Adrm { key: &key, iv: &iv },
            trim,
            rewrite_ftyp: true,
        },
    )?;

    if !output.exists() {
        return Err(DecryptError::OutputMissing(output.to_path_buf()));
    }
    Ok(DecryptOutcome {
        output: output.to_path_buf(),
    })
}

/// Native CENC decrypt: fragmented DASH (per-sample IVs) or progressive remux.
pub fn decrypt_cenc_native(
    input: &Path,
    output: &Path,
    kid_hex: &str,
    key_hex: &str,
    trim: Option<TrimRange>,
) -> Result<DecryptOutcome> {
    if !input.exists() {
        return Err(DecryptError::InputMissing(input.to_path_buf()));
    }

    if looks_like_dash(input).unwrap_or(false) {
        return decrypt_dash_cenc(input, output, kid_hex, key_hex, trim);
    }

    let mp4 = parse_mp4(input).map_err(|err| {
        DecryptError::Native(format!(
            "native CENC requires a progressive MP4 or DASH fragment ({err})"
        ))
    })?;

    if !matches!(
        mp4.audio.sample_entry_kind,
        SampleEntryKind::Enca | SampleEntryKind::Mp4a
    ) {
        return Err(DecryptError::Native(format!(
            "unsupported CENC sample entry {:?}",
            mp4.audio.sample_entry_kind
        )));
    }

    if matches!(mp4.audio.sample_entry_kind, SampleEntryKind::Mp4a) {
        let _key = parse_aes128_hex(key_hex)?;
        decrypt_and_remux(
            input,
            output,
            &RemuxOptions {
                decrypt: DecryptMode::None,
                trim,
                rewrite_ftyp: true,
            },
        )?;
        return Ok(DecryptOutcome {
            output: output.to_path_buf(),
        });
    }

    Err(DecryptError::Native(
        "progressive enca CENC without fragment IVs is not handled natively yet; use ffmpeg".into(),
    ))
}
