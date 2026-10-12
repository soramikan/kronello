//! Explicit experimental delivery paths. Never enabled by public export defaults.
use std::path::PathBuf;
use std::time::Instant;

use kronello_model::ChannelMask;
use kronello_time::{FrameRate, Rational};
use serde::{Deserialize, Serialize};

use crate::{EncodeCodec, MediaError, MediaProbe, MediaRuntime, MovieProfile, ffi};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PacketMuxMode {
    Intermediate,
    Direct,
}
#[derive(Debug, Clone)]
pub struct PacketMuxProbeRequest {
    pub output: PathBuf,
    pub pixels: [u32; 2],
    pub frame_rate: FrameRate,
    pub frames: u64,
    pub render_snapshot_hash: String,
    pub export_snapshot_hash: String,
}
#[derive(Debug, Serialize)]
pub struct PacketMuxProbeReport {
    pub mode: PacketMuxMode,
    pub total_us: u64,
    pub audio_preparation_us: u64,
    pub video_and_mux_us: u64,
    pub validation_us: u64,
    pub intermediate_video_bytes: u64,
    pub audio_stage_bytes: u64,
    pub final_bytes: u64,
    pub rgba_buffer_bytes: usize,
    pub probe: MediaProbe,
}

/// Closed SDR ProRes/stereo PCM24 experiment with zero-origin exact frame spans.
/// The caller fills one reused RGBA8 frame, allowing real rendering or a fixed
/// encoder fixture; setup is identical for both paths. Chapters are not yet
/// accepted by this prototype and must be added before wider adoption.
pub type ProbeFrameProducer<'a> = dyn FnMut(u64, &mut [u8]) -> Result<(), MediaError> + 'a;
pub fn packet_mux_probe(
    runtime: &MediaRuntime,
    request: &PacketMuxProbeRequest,
    mode: PacketMuxMode,
    frame: &mut ProbeFrameProducer<'_>,
    checkpoint: &mut dyn FnMut(u64) -> Result<(), MediaError>,
) -> Result<PacketMuxProbeReport, MediaError> {
    if request
        .pixels
        .iter()
        .any(|n| *n == 0 || !n.is_multiple_of(2))
        || request.pixels[0] > 3840
        || request.pixels[1] > 2160
        || u64::from(request.pixels[0]) * u64::from(request.pixels[1]) > 3840 * 2160
        || !(1..=60_000).contains(&request.frames)
        || [&request.render_snapshot_hash, &request.export_snapshot_hash]
            .iter()
            .any(|hash| {
                hash.len() != 64
                    || !hash
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
    {
        return Err(MediaError::InvalidInput(
            "packet mux prototype limits/identity".into(),
        ));
    }
    if request.output.exists() {
        return Err(MediaError::InvalidInput("prototype output exists".into()));
    }
    let time_base = request.frame_rate.frame_to_time(1)?;
    let duration = request.frame_rate.frame_to_time(request.frames as i64)?;
    let samples = duration.checked_mul(Rational::from_integer(48_000))?;
    if samples.denominator() != 1 || samples.numerator() <= 0 || samples.numerator() > 600 * 48_000
    {
        return Err(MediaError::InvalidInput(
            "prototype requires integral audio frames within ten minutes".into(),
        ));
    }
    let total = Instant::now();
    let staging = tempfile::Builder::new()
        .prefix(".packet-mux-probe-")
        .tempdir_in(
            request
                .output
                .parent()
                .ok_or_else(|| MediaError::InvalidInput("output parent".into()))?,
        )?;
    let audio = staging.path().join("audio.mov");
    let phase = Instant::now();
    let mut encoder = ffi::NativeAudioEncoder::open(
        &runtime.native,
        &audio,
        ffi::AudioEncoderKind::Pcm24,
        ChannelMask::STEREO,
    )?;
    if encoder.block == 0 || encoder.block > 65536 {
        return Err(MediaError::InvalidInput(
            "prototype audio block budget".into(),
        ));
    }
    let block = vec![0; encoder.block * 2];
    let mut remaining = samples.numerator() as usize;
    while remaining > 0 {
        checkpoint(0)?;
        let count = remaining.min(encoder.block);
        encoder.frame(&block[..count * 2])?;
        remaining -= count;
    }
    encoder.finish()?;
    drop(encoder);
    let audio_preparation_us = phase.elapsed().as_micros() as u64;
    let audio_stage_bytes = std::fs::metadata(&audio)?.len();
    let final_path = staging.path().join("final.mov");
    let video = match mode {
        PacketMuxMode::Direct => final_path.clone(),
        PacketMuxMode::Intermediate => staging.path().join("video.mov"),
    };
    let phase = Instant::now();
    let codec = runtime.capabilities().select_encoder(EncodeCodec::ProRes)?;
    let mut encoder = ffi::NativeEncoder::open_color(
        &runtime.native,
        &video,
        codec,
        request.pixels[0] as i32,
        request.pixels[1] as i32,
        time_base,
        None,
    )?;
    if matches!(mode, PacketMuxMode::Direct) {
        encoder.direct_pcm_mux(
            &audio,
            &request.render_snapshot_hash,
            &request.export_snapshot_hash,
        )?;
    }
    let mut buffer = vec![0; request.pixels[0] as usize * request.pixels[1] as usize * 4];
    for index in 0..request.frames {
        checkpoint(index)?;
        frame(index, &mut buffer)?;
        if buffer.chunks_exact(4).any(|pixel| pixel[3] != 255) {
            return Err(MediaError::InvalidInput(
                "prototype input must be opaque transfer-encoded BT.709 RGBA8".into(),
            ));
        }
        encoder.frame(&buffer, index as i64, 1)?;
    }
    encoder.finish()?;
    drop(encoder);
    let intermediate_video_bytes = if matches!(mode, PacketMuxMode::Intermediate) {
        let bytes = std::fs::metadata(&video)?.len();
        runtime.mux_av(
            &video,
            &audio,
            &final_path,
            &request.render_snapshot_hash,
            &request.export_snapshot_hash,
        )?;
        bytes
    } else {
        0
    };
    let video_and_mux_us = phase.elapsed().as_micros() as u64;
    checkpoint(request.frames)?;
    let phase = Instant::now();
    let probe = runtime.probe(&final_path)?;
    probe.verify_movie_layout(MovieProfile::ProResPcm24, ChannelMask::STEREO)?;
    if probe
        .streams
        .iter()
        .any(|stream| stream.duration != Some(duration))
    {
        return Err(MediaError::Encode(
            "prototype stream duration differs".into(),
        ));
    }
    if probe.render_snapshot_hash != request.render_snapshot_hash
        || probe.export_snapshot_hash != request.export_snapshot_hash
    {
        return Err(MediaError::Encode(
            "prototype snapshot metadata differs".into(),
        ));
    }
    let final_bytes = std::fs::metadata(&final_path)?.len();
    let validation_us = phase.elapsed().as_micros() as u64;
    kronello_no_clobber(&final_path, &request.output)?;
    Ok(PacketMuxProbeReport {
        mode,
        total_us: total.elapsed().as_micros() as u64,
        audio_preparation_us,
        video_and_mux_us,
        validation_us,
        intermediate_video_bytes,
        audio_stage_bytes,
        final_bytes,
        rgba_buffer_bytes: buffer.len(),
        probe,
    })
}
fn kronello_no_clobber(
    source: &std::path::Path,
    target: &std::path::Path,
) -> Result<(), MediaError> {
    // Publish from the same volume without replacing a competing destination.
    std::fs::File::open(source)?.sync_all()?;
    std::fs::hard_link(source, target)?;
    // The staging directory owns the source link. Its Drop can reclaim it
    // after publication without turning an already-published file into error.
    Ok(())
}
