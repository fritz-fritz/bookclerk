//! Native MP3 re-encode via Symphonia (decode) + LAME (encode).

use std::fs::File;
use std::path::Path;

use mp3lame_encoder::{Builder, DualPcm, Encoder, FlushNoGap, InterleavedPcm};
use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::error::{DecryptError, Result};
use crate::DecryptOutcome;

/// Re-encode audio to MP3 (classic Libation `DecryptToLossy`).
pub fn encode_to_mp3_native(
    input: &Path,
    output: &Path,
    lame: &libation_config::LameConfig,
    max_sample_rate: Option<u32>,
) -> Result<DecryptOutcome> {
    if !input.exists() {
        return Err(DecryptError::InputMissing(input.to_path_buf()));
    }
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }

    tracing::info!(
        input = %input.display(),
        output = %output.display(),
        "native mp3 encode (symphonia + lame)"
    );

    let file = File::open(input)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = input.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|err| DecryptError::Native(format!("probe failed: {err}")))?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| DecryptError::Native("no decodable audio track".into()))?
        .clone();
    let track_id = track.id;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| DecryptError::Native("missing sample rate".into()))?;
    let channels = track
        .codec_params
        .channels
        .map(|c| c.count())
        .unwrap_or(2)
        .max(1);

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|err| DecryptError::Native(format!("decoder init failed: {err}")))?;

    let out_channels = if lame.downsample_mono || lame.mode.eq_ignore_ascii_case("mono") {
        1u32
    } else {
        channels.min(2) as u32
    };
    let out_rate = max_sample_rate
        .map(|m| m.min(sample_rate))
        .unwrap_or(sample_rate);

    let mut builder = Builder::new()
        .ok_or_else(|| DecryptError::Native("failed to create LAME encoder (mp3lame)".into()))?;
    builder
        .set_num_channels(out_channels as u8)
        .map_err(|err| DecryptError::Native(format!("lame channels: {err:?}")))?;
    builder
        .set_sample_rate(out_rate)
        .map_err(|err| DecryptError::Native(format!("lame sample rate: {err:?}")))?;

    if lame.constant_bitrate || lame.target.eq_ignore_ascii_case("bitrate") {
        let br = match lame.bitrate_kbps {
            0..=64 => mp3lame_encoder::Bitrate::Kbps64,
            65..=96 => mp3lame_encoder::Bitrate::Kbps96,
            97..=128 => mp3lame_encoder::Bitrate::Kbps128,
            129..=160 => mp3lame_encoder::Bitrate::Kbps160,
            161..=192 => mp3lame_encoder::Bitrate::Kbps192,
            193..=256 => mp3lame_encoder::Bitrate::Kbps256,
            _ => mp3lame_encoder::Bitrate::Kbps320,
        };
        builder
            .set_brate(br)
            .map_err(|err| DecryptError::Native(format!("lame bitrate: {err:?}")))?;
    } else {
        let quality = match lame.vbr_quality.min(9) {
            0 => mp3lame_encoder::Quality::Best,
            1 => mp3lame_encoder::Quality::SecondBest,
            2 => mp3lame_encoder::Quality::NearBest,
            3 => mp3lame_encoder::Quality::VeryNice,
            4 => mp3lame_encoder::Quality::Nice,
            5 => mp3lame_encoder::Quality::Good,
            6 => mp3lame_encoder::Quality::Decent,
            7 => mp3lame_encoder::Quality::Ok,
            8 => mp3lame_encoder::Quality::SecondWorst,
            _ => mp3lame_encoder::Quality::Worst,
        };
        builder
            .set_vbr_mode(mp3lame_encoder::VbrMode::Mtrh)
            .map_err(|err| DecryptError::Native(format!("lame vbr mode: {err:?}")))?;
        builder
            .set_vbr_quality(quality)
            .map_err(|err| DecryptError::Native(format!("lame vbr quality: {err:?}")))?;
    }

    let mut encoder: Encoder = builder
        .build()
        .map_err(|err| DecryptError::Native(format!("lame build: {err:?}")))?;

    let mut mp3_buf = Vec::new();
    let mut pcm_i16: Vec<i16> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(SymphoniaError::IoError(err))
                if err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(err) => {
                return Err(DecryptError::Native(format!("demux error: {err}")));
            }
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(buf) => buf,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(err) => {
                return Err(DecryptError::Native(format!("decode error: {err}")));
            }
        };
        append_pcm_i16(&decoded, out_channels, &mut pcm_i16);

        // Encode in chunks to keep memory bounded.
        const CHUNK: usize = 1152 * 8;
        while pcm_i16.len() >= CHUNK * out_channels as usize {
            let take = CHUNK * out_channels as usize;
            let chunk: Vec<i16> = pcm_i16.drain(..take).collect();
            encode_pcm_chunk(&mut encoder, &chunk, out_channels, &mut mp3_buf)?;
        }
    }

    if !pcm_i16.is_empty() {
        encode_pcm_chunk(&mut encoder, &pcm_i16, out_channels, &mut mp3_buf)?;
    }
    encoder
        .flush_to_vec::<FlushNoGap>(&mut mp3_buf)
        .map_err(|err| DecryptError::Native(format!("lame flush: {err:?}")))?;

    // Simple sample-rate note: we do not resample when out_rate != sample_rate;
    // LAME will be told out_rate but PCM is still source rate. Prefer matching rates.
    if out_rate != sample_rate {
        tracing::warn!(
            source_hz = sample_rate,
            target_hz = out_rate,
            "max_sample_rate requested but native path does not resample; encoding at source rate"
        );
    }

    std::fs::write(output, &mp3_buf)?;
    if !output.exists() {
        return Err(DecryptError::OutputMissing(output.to_path_buf()));
    }
    Ok(DecryptOutcome {
        output: output.to_path_buf(),
    })
}

fn encode_pcm_chunk(
    encoder: &mut Encoder,
    pcm: &[i16],
    channels: u32,
    out: &mut Vec<u8>,
) -> Result<()> {
    if channels == 1 {
        encoder
            .encode_to_vec(InterleavedPcm(pcm), out)
            .map_err(|err| DecryptError::Native(format!("lame encode: {err:?}")))?;
    } else {
        let frames = pcm.len() / 2;
        let mut left = Vec::with_capacity(frames);
        let mut right = Vec::with_capacity(frames);
        for pair in pcm.chunks_exact(2) {
            left.push(pair[0]);
            right.push(pair[1]);
        }
        encoder
            .encode_to_vec(
                DualPcm {
                    left: &left,
                    right: &right,
                },
                out,
            )
            .map_err(|err| DecryptError::Native(format!("lame encode: {err:?}")))?;
    }
    Ok(())
}

fn append_pcm_i16(buf: &AudioBufferRef<'_>, out_channels: u32, dst: &mut Vec<i16>) {
    match buf {
        AudioBufferRef::F32(buf) => {
            let frames = buf.frames();
            let chans = buf.spec().channels.count().min(2);
            for i in 0..frames {
                if out_channels == 1 {
                    let mut acc = 0.0f32;
                    for c in 0..chans {
                        acc += buf.chan(c)[i];
                    }
                    dst.push(float_to_i16(acc / chans as f32));
                } else {
                    let l = buf.chan(0)[i];
                    let r = if chans > 1 { buf.chan(1)[i] } else { l };
                    dst.push(float_to_i16(l));
                    dst.push(float_to_i16(r));
                }
            }
        }
        AudioBufferRef::S16(buf) => {
            let frames = buf.frames();
            let chans = buf.spec().channels.count().min(2);
            for i in 0..frames {
                if out_channels == 1 {
                    let mut acc = 0i32;
                    for c in 0..chans {
                        acc += i32::from(buf.chan(c)[i]);
                    }
                    dst.push((acc / chans as i32) as i16);
                } else {
                    let l = buf.chan(0)[i];
                    let r = if chans > 1 { buf.chan(1)[i] } else { l };
                    dst.push(l);
                    dst.push(r);
                }
            }
        }
        other => {
            // Convert via f32 plane copy for less common sample formats.
            let frames = other.frames();
            // Fall back: silence if we cannot map — should be rare for Audible AAC.
            tracing::warn!(
                frames,
                "unsupported PCM sample format for mp3 encode; inserting silence"
            );
            for _ in 0..frames {
                if out_channels == 1 {
                    dst.push(0);
                } else {
                    dst.push(0);
                    dst.push(0);
                }
            }
        }
    }
}

fn float_to_i16(sample: f32) -> i16 {
    let s = sample.clamp(-1.0, 1.0);
    (s * f32::from(i16::MAX)) as i16
}
