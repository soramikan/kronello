//! Two reusable frame bundles; native handles never cross threads.
use super::*;
use std::sync::mpsc::{Receiver, Sender, SyncSender, channel, sync_channel};

#[derive(Clone)]
pub(super) struct VideoLeg {
    pub profile: MovieProfile,
    pub background: [f32; 3],
    pub output: PathBuf,
    pub audio_layout: kronello_model::ChannelMask,
    pub color: color::OutputColor,
}
pub(super) fn conversion_owners(legs: &[VideoLeg]) -> Vec<usize> {
    (0..legs.len())
        .map(|index| {
            (0..index)
                .find(|&other| {
                    legs[index].stride() != 0
                        && legs[other].stride() != 0
                        && legs[index].color == legs[other].color
                        && legs[index].background.map(f32::to_bits)
                            == legs[other].background.map(f32::to_bits)
                })
                .unwrap_or(index)
        })
        .collect()
}
impl VideoLeg {
    pub fn stride(&self) -> usize {
        if !self.profile.has_video() {
            0
        } else if self.profile.hdr_transfer().is_some() {
            8
        } else {
            4
        }
    }
}
fn encoding_owners(legs: &[VideoLeg], conversions: &[usize]) -> Vec<usize> {
    // Raster, timestamps, fixed native bitrate/thread policy and selected
    // runtime are common to this pipeline. Compare every per-leg video input
    // to the native constructor; final container/audio/metadata remain separate.
    (0..legs.len())
        .map(|index| {
            (0..index)
                .find(|&other| {
                    legs[index].profile.shape() == DeliveryShape::Movie
                        && legs[other].profile.shape() == DeliveryShape::Movie
                        && conversions[index] == conversions[other]
                        && legs[index].profile.video_codec() == legs[other].profile.video_codec()
                        && legs[index].profile.hdr_transfer() == legs[other].profile.hdr_transfer()
                        && legs[index]
                            .profile
                            .dnx_profile()
                            .map(|(profile, _)| profile.kind())
                            == legs[other]
                                .profile
                                .dnx_profile()
                                .map(|(profile, _)| profile.kind())
                })
                .unwrap_or(index)
        })
        .collect()
}
pub(super) struct Config {
    pub capabilities: crate::MediaCapabilities,
    pub stage: PathBuf,
    pub pixels: [u32; 2],
    pub time_base: Rational,
    pub frames: usize,
    pub bundles: usize,
    pub duration: Rational,
    pub legs: Vec<VideoLeg>,
    pub diagnostics: crate::ExportDiagnosticsCollector,
}
pub(super) type Bundle = Vec<Vec<u8>>;
pub(super) struct ResultData {
    pub files: Vec<Option<PathBuf>>,
    pub reports: Vec<Option<MediaPathReport>>,
    pub staged: Vec<Option<tempfile::NamedTempFile>>,
    pub probes: Vec<Option<MediaProbe>>,
}
enum Command {
    Frame {
        bundle: Bundle,
        tick: i64,
        span: i64,
    },
    Finish(Bundle),
    Continue,
}
enum Event {
    Ready,
    Recycle(Bundle),
    Checkpoint,
    Finished(Result<ResultData, MediaError>),
}
pub(super) struct Producer {
    sender: SyncSender<Command>,
    events: Receiver<Event>,
    free: Vec<Bundle>,
    diagnostics: crate::ExportDiagnosticsCollector,
}
impl Producer {
    pub fn acquire(&mut self) -> Result<Bundle, MediaError> {
        let _wait = self.diagnostics.time("pipeline_buffer_acquire");
        if let Some(bundle) = self.free.pop() {
            return Ok(bundle);
        }
        match self.events.recv() {
            Ok(Event::Recycle(bundle)) => Ok(bundle),
            Ok(Event::Finished(Err(error))) => Err(error),
            _ => Err(MediaError::Encode(
                "video pipeline stopped before recycling a frame".into(),
            )),
        }
    }
    fn send(&mut self, command: Command, reason: &str) -> Result<(), MediaError> {
        let _wait = self.diagnostics.time("pipeline_submit_wait");
        if self.sender.send(command).is_ok() {
            return Ok(());
        }
        // Preserve the consumer's typed error even if it closed its command
        // receiver between the last recycle and this submission.
        loop {
            match self.events.recv() {
                Ok(Event::Recycle(bundle)) => self.free.push(bundle),
                Ok(Event::Finished(Err(error))) => return Err(error),
                _ => return Err(MediaError::Encode(reason.into())),
            }
        }
    }
    pub fn frame(&mut self, bundle: Bundle, tick: i64, span: i64) -> Result<(), MediaError> {
        self.send(
            Command::Frame { bundle, tick, span },
            "video pipeline stopped before frame submission",
        )
    }
    fn finish(
        &mut self,
        checkpoint: &mut dyn FnMut(u64) -> Result<(), MediaError>,
    ) -> Result<ResultData, MediaError> {
        let bundle = self.acquire()?;
        self.send(
            Command::Finish(bundle),
            "video pipeline stopped before flush",
        )?;
        loop {
            match self.events.recv() {
                Ok(Event::Recycle(bundle)) => self.free.push(bundle),
                Ok(Event::Checkpoint) => {
                    checkpoint(0)?;
                    self.send(Command::Continue, "video pipeline stopped during GIF pass")?;
                }
                Ok(Event::Finished(result)) => return result,
                _ => {
                    return Err(MediaError::Encode(
                        "video pipeline stopped during flush".into(),
                    ));
                }
            }
        }
    }
}
pub(super) fn run<T>(
    config: Config,
    checkpoint: &mut dyn FnMut(u64) -> Result<(), MediaError>,
    produce: impl FnOnce(
        &mut Producer,
        &mut dyn FnMut(u64) -> Result<(), MediaError>,
    ) -> Result<T, MediaError>,
) -> Result<(T, ResultData), MediaError> {
    if !(1..=2).contains(&config.bundles) {
        return Err(MediaError::InvalidInput(
            "video pipeline bundle budget".into(),
        ));
    }
    let diagnostics = config.diagnostics.clone();
    let owners = conversion_owners(&config.legs);
    let mut free = Vec::with_capacity(config.bundles);
    for _ in 0..config.bundles {
        let mut bundle = Vec::with_capacity(config.legs.len());
        for (index, leg) in config.legs.iter().enumerate() {
            let bytes = (config.pixels[0] as usize)
                .checked_mul(config.pixels[1] as usize)
                .and_then(|n| {
                    n.checked_mul(if owners[index] == index {
                        leg.stride()
                    } else {
                        0
                    })
                })
                .ok_or_else(|| MediaError::InvalidInput("video bundle byte overflow".into()))?;
            bundle.push(vec![0; bytes]);
        }
        free.push(bundle);
    }
    diagnostics.peak_payload(
        "video_bundles",
        free.iter().flatten().map(|b| b.len() as u64).sum(),
    );
    let (sender, commands) = sync_channel(1);
    let (events_sender, events) = channel();
    std::thread::scope(|scope| {
        let worker = scope.spawn(move || {
            let result = consume(&config, &commands, &events_sender);
            // A cancelled producer may already have dropped its receiver.
            let _ = events_sender.send(Event::Finished(result));
        });
        let mut producer = Producer {
            sender,
            events,
            free,
            diagnostics: diagnostics.clone(),
        };
        let result = (|| {
            let ready = diagnostics.time("pipeline_encoder_initialization_wait");
            match producer.events.recv() {
                Ok(Event::Ready) => {}
                Ok(Event::Finished(Err(error))) => return Err(error),
                _ => {
                    return Err(MediaError::Encode(
                        "video pipeline failed to initialize".into(),
                    ));
                }
            }
            drop(ready);
            let output = produce(&mut producer, checkpoint)?;
            let result = producer.finish(checkpoint)?;
            Ok((output, result))
        })();
        // Disconnect on every render/checkpoint/consumer failure, then join
        // before the caller drops the staging directory or publishes outputs.
        drop(producer);
        let joined = worker.join();
        match (result, joined) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(_)) => Err(MediaError::Encode("video pipeline worker panicked".into())),
            (Ok(result), Ok(())) => Ok(result),
        }
    })
}
enum Sink<'a> {
    Color {
        encoder: crate::ffi::NativeEncoder<'a>,
        name: String,
        hardware: bool,
        stride: usize,
    },
    Gif {
        encoder: crate::ffi::NativeGifEncoder<'a>,
        spool: std::io::BufWriter<std::fs::File>,
        path: PathBuf,
    },
}
fn stopped() -> MediaError {
    MediaError::Encode("video pipeline producer disconnected".into())
}
fn consume(
    config: &Config,
    commands: &Receiver<Command>,
    events: &Sender<Event>,
) -> Result<ResultData, MediaError> {
    let _diagnostic_scope = config.diagnostics.activate();
    // Open a separate runtime from the exact selected directory. Neither its
    // Rc nor the native shim's mutable error buffer is shared with rendering.
    let runtime = MediaRuntime::load_directory(
        config.capabilities.library_directory.clone(),
        config.capabilities.substituted,
    )?;
    if runtime.capabilities() != &config.capabilities {
        return Err(MediaError::Encode(
            "video pipeline runtime capabilities changed".into(),
        ));
    }
    let mut sinks = Vec::with_capacity(config.legs.len());
    let mut files: Vec<Option<PathBuf>> = Vec::with_capacity(config.legs.len());
    let conversions = conversion_owners(&config.legs);
    let encodings = encoding_owners(&config.legs, &conversions);
    for (index, leg) in config.legs.iter().enumerate() {
        if encodings[index] != index {
            sinks.push(None);
            files.push(files[encodings[index]].clone());
            continue;
        }
        match leg.profile.shape() {
            DeliveryShape::AudioOnly => {
                sinks.push(None);
                files.push(None);
            }
            DeliveryShape::VideoOnly => {
                let path = config.stage.join(format!("gif_{index}.rgba"));
                let spool = std::io::BufWriter::new(std::fs::File::create(&path)?);
                let encoder = crate::ffi::NativeGifEncoder::open(
                    &runtime.native,
                    config.pixels[0],
                    config.pixels[1],
                    config.time_base,
                )?;
                sinks.push(Some(Sink::Gif {
                    encoder,
                    spool,
                    path,
                }));
                files.push(None);
            }
            DeliveryShape::Movie => {
                let path = config.stage.join(format!("video_{index}.mov"));
                let (encoder, name, hardware) = if let Some((dnx, _)) = leg.profile.dnx_profile() {
                    (
                        crate::ffi::NativeEncoder::open_dnx(
                            &runtime.native,
                            &path,
                            dnx.kind(),
                            config.pixels[0] as i32,
                            config.pixels[1] as i32,
                            config.time_base,
                        )?,
                        "dnxhd".into(),
                        false,
                    )
                } else {
                    let codec = runtime
                        .capabilities
                        .select_encoder(leg.profile.video_codec().unwrap_or(EncodeCodec::ProRes))?;
                    let name = codec.name.clone();
                    let hardware = codec.hardware;
                    (
                        crate::ffi::NativeEncoder::open_color(
                            &runtime.native,
                            &path,
                            codec,
                            config.pixels[0] as i32,
                            config.pixels[1] as i32,
                            config.time_base,
                            leg.profile.hdr_transfer(),
                        )?,
                        name,
                        hardware,
                    )
                };
                sinks.push(Some(Sink::Color {
                    encoder,
                    name,
                    hardware,
                    stride: leg.stride(),
                }));
                files.push(Some(path));
            }
        }
    }
    events.send(Event::Ready).map_err(|_| stopped())?;
    let mut delivered = 0_usize;
    let mut final_bundle = loop {
        let wait = config.diagnostics.time("pipeline_consumer_input_wait");
        let command = commands.recv().map_err(|_| stopped())?;
        drop(wait);
        match command {
            Command::Frame { bundle, tick, span } => {
                if span <= 0 || delivered >= config.frames {
                    return Err(MediaError::Encode(
                        "invalid pipeline frame span/count".into(),
                    ));
                }
                use std::io::Write;
                if bundle.len() != config.legs.len() {
                    return Err(MediaError::InvalidInput(
                        "video frame leg count mismatch".into(),
                    ));
                }
                for (index, (bytes, leg)) in bundle.iter().zip(&config.legs).enumerate() {
                    let expected = config.pixels[0] as usize
                        * config.pixels[1] as usize
                        * if conversions[index] == index {
                            leg.stride()
                        } else {
                            0
                        };
                    if bytes.len() != expected {
                        return Err(MediaError::InvalidInput("video frame size mismatch".into()));
                    }
                }
                for (index, sink) in sinks.iter_mut().enumerate() {
                    let bytes = &bundle[conversions[index]];
                    match sink {
                        Some(Sink::Color { encoder, .. }) => {
                            let _encode = config.diagnostics.time("native_video_conversion_encode");
                            encoder.frame(bytes, tick, span)?;
                            config.diagnostics.count("video_encoder_frames", 1);
                        }
                        Some(Sink::Gif { encoder, spool, .. }) => {
                            let _gif = config.diagnostics.time("gif_palette_spool");
                            encoder.frame(bytes)?;
                            spool.write_all(bytes)?;
                        }
                        None => {}
                    }
                }
                delivered += 1;
                events.send(Event::Recycle(bundle)).map_err(|_| stopped())?;
            }
            Command::Finish(bundle) => break bundle,
            Command::Continue => {
                return Err(MediaError::Encode(
                    "unexpected pipeline checkpoint acknowledgment".into(),
                ));
            }
        }
    };
    if delivered != config.frames {
        return Err(MediaError::Encode(
            "video pipeline frame count differs".into(),
        ));
    }
    let mut result = ResultData {
        files,
        reports: vec![],
        staged: vec![],
        probes: vec![],
    };
    for (index, (sink, leg)) in sinks.into_iter().zip(&config.legs).enumerate() {
        match sink {
            Some(Sink::Color {
                mut encoder,
                name,
                hardware,
                stride,
            }) => {
                let flush = config.diagnostics.time("video_encoder_flush");
                encoder.finish()?;
                drop(flush);
                let pixel_format = encoder.pixel_format.clone();
                let converted = encoder
                    .frame_size
                    .checked_mul(config.frames as u64)
                    .ok_or_else(|| MediaError::InvalidInput("transfer counter overflow".into()))?;
                let input_bytes = (final_bundle[conversions[index]].len() as u64)
                    .checked_mul(config.frames as u64)
                    .ok_or_else(|| MediaError::InvalidInput("transfer counter overflow".into()))?;
                drop(encoder);
                result.reports.push(Some(MediaPathReport {
                    decoder: None,
                    encoder: Some(name),
                    execution: if hardware {
                        ExecutionKind::Hardware
                    } else {
                        ExecutionKind::Software
                    },
                    input_pixel_format: if stride == 8 { "rgba64le" } else { "rgba" }.into(),
                    output_pixel_format: pixel_format,
                    transfer_path: if stride == 8 {
                        "cpu_rec2100_rgba64_to_prores_10bit"
                    } else if hardware {
                        "cpu_rgba_to_hardware_encoder"
                    } else {
                        "cpu_rgba_to_software_encoder"
                    }
                    .into(),
                    transfers: MediaTransferStats {
                        cpu_copy_bytes: 0,
                        cpu_conversion_input_bytes: input_bytes,
                        cpu_conversion_output_bytes: converted,
                        cpu_upload_bytes: if hardware { converted } else { 0 },
                        ..Default::default()
                    },
                }));
                result.staged.push(None);
                result.probes.push(None);
            }
            Some(Sink::Gif {
                mut encoder,
                mut spool,
                path,
            }) => {
                use std::io::{Read, Write};
                encoder.palette()?;
                spool.flush()?;
                let mut spool = std::fs::File::open(path)?;
                let temp = stage_file(&leg.output)?;
                encoder.encode_start(temp.path())?;
                // Reuse one of the two bundles for pass 2, with no third frame.
                let rgba = &mut final_bundle[conversions[index]];
                for _ in 0..config.frames {
                    events.send(Event::Checkpoint).map_err(|_| stopped())?;
                    if !matches!(commands.recv().map_err(|_| stopped())?, Command::Continue) {
                        return Err(MediaError::Encode(
                            "invalid GIF checkpoint acknowledgment".into(),
                        ));
                    }
                    spool.read_exact(rgba)?;
                    encoder.encode_frame(rgba)?;
                }
                encoder.flush()?;
                drop(encoder);
                let probe = runtime.probe(temp.path())?;
                probe.verify_delivery(leg.profile, leg.audio_layout)?;
                let tolerance = Rational::new(config.frames as i64 + 1, 100)?;
                let duration = probe
                    .duration
                    .or_else(|| probe.streams[0].duration)
                    .ok_or_else(|| MediaError::Encode("missing GIF duration".into()))?;
                let drift = duration.checked_sub(config.duration)?;
                if drift >= tolerance || drift <= tolerance.checked_neg()? {
                    return Err(MediaError::Encode(format!(
                        "GIF duration differs from export range: {duration:?} vs {:?}",
                        config.duration
                    )));
                }
                result.reports.push(Some(MediaPathReport {
                    decoder: None,
                    encoder: Some("gif".into()),
                    execution: ExecutionKind::Software,
                    input_pixel_format: "rgba".into(),
                    output_pixel_format: "pal8".into(),
                    transfer_path: "cpu_rgba_to_indexed_gif".into(),
                    transfers: MediaTransferStats {
                        cpu_conversion_input_bytes: (rgba.len() as u64)
                            .checked_mul(config.frames as u64)
                            .ok_or_else(|| {
                                MediaError::InvalidInput("transfer counter overflow".into())
                            })?,
                        ..Default::default()
                    },
                }));
                result.staged.push(Some(temp));
                result.probes.push(Some(probe));
            }
            None => {
                let report = if encodings[index] != index {
                    let mut report = result.reports[encodings[index]]
                        .clone()
                        .ok_or_else(|| MediaError::Encode("missing shared video report".into()))?;
                    report.transfer_path.push_str("_shared_video_stream");
                    report.transfers = MediaTransferStats::default();
                    Some(report)
                } else {
                    None
                };
                result.reports.push(report);
                result.staged.push(None);
                result.probes.push(None);
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config(stage: &Path, output: &Path, frames: usize, profiles: &[MovieProfile]) -> Config {
        Config {
            diagnostics: crate::ExportDiagnosticsCollector::default(),
            capabilities: MediaRuntime::load().unwrap().capabilities().clone(),
            stage: stage.into(),
            pixels: [64, 64],
            time_base: Rational::new(1, 24).unwrap(),
            frames,
            bundles: 2,
            duration: Rational::new(frames as i64, 24).unwrap(),
            legs: profiles
                .iter()
                .enumerate()
                .map(|(i, profile)| VideoLeg {
                    profile: *profile,
                    background: [0.0; 3],
                    output: output.join(format!("{i}.{}", profile.container())),
                    audio_layout: kronello_model::ChannelMask::STEREO,
                    color: profile
                        .hdr_transfer()
                        .map(color::OutputColor::Hdr)
                        .unwrap_or(color::OutputColor::Sdr { tone_map: false }),
                })
                .collect(),
        }
    }
    #[test]
    fn sharing_keys_cover_color_and_native_encoder_inputs() {
        let stage = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let mut config = config(
            stage.path(),
            output.path(),
            1,
            &[
                MovieProfile::H264AlacV1,
                MovieProfile::H264AacV1,
                MovieProfile::HevcAacV1,
                MovieProfile::DnxhrHqxMovPcm24V1,
                MovieProfile::DnxhrHqxMxfPcm24V1,
                MovieProfile::DnxhrHqMovPcm24V1,
                MovieProfile::ProResPqPcm24V1,
                MovieProfile::ProResHlgPcm24V1,
                MovieProfile::H264AacV1,
                MovieProfile::H264AacV1,
                MovieProfile::GifV1,
                MovieProfile::FlacV1,
            ],
        );
        config.legs[8].background = [0.125; 3];
        config.legs[9].color = color::OutputColor::Sdr { tone_map: true };
        let conversions = conversion_owners(&config.legs);
        assert_eq!(conversions, [0, 0, 0, 0, 0, 0, 6, 7, 8, 9, 0, 11]);
        assert_eq!(
            encoding_owners(&config.legs, &conversions),
            [0, 0, 2, 3, 3, 5, 6, 7, 8, 9, 10, 11]
        );
    }

    #[test]
    fn duplicate_movie_uses_one_native_stage_and_counts_transfers_once() {
        let stage = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let config = config(
            stage.path(),
            output.path(),
            4,
            &[MovieProfile::ProResPcm24; 2],
        );
        let (_, result) = run(config, &mut |_| Ok(()), |producer, _| {
            for tick in 0..4 {
                let mut bundle = producer.acquire()?;
                assert!(bundle[1].is_empty());
                for pixel in bundle[0].chunks_exact_mut(4) {
                    pixel.copy_from_slice(&[37, 89, 123, 255]);
                }
                producer.frame(bundle, tick, 1)?;
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(result.files[0], result.files[1]);
        assert_eq!(std::fs::read_dir(stage.path()).unwrap().count(), 1);
        assert_eq!(
            result.reports[0]
                .as_ref()
                .unwrap()
                .transfers
                .cpu_conversion_input_bytes,
            4 * 64 * 64 * 4
        );
        assert_eq!(
            result.reports[1].as_ref().unwrap().transfers,
            MediaTransferStats::default()
        );
        assert!(
            result.reports[1]
                .as_ref()
                .unwrap()
                .transfer_path
                .ends_with("shared_video_stream")
        );
    }

    #[test]
    fn two_shared_allocations_survive_backpressure_and_gif_second_pass() {
        let stage = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let config = config(
            stage.path(),
            output.path(),
            32,
            &[MovieProfile::ProResPcm24, MovieProfile::GifV1],
        );
        let caller = std::thread::current().id();
        let mut pointers = [
            std::collections::BTreeSet::new(),
            std::collections::BTreeSet::new(),
        ];
        let mut gates = 0;
        let (_, result) = run(
            config,
            &mut |_| {
                assert_eq!(std::thread::current().id(), caller);
                gates += 1;
                Ok(())
            },
            |producer, _| {
                for tick in 0..32 {
                    assert_eq!(std::thread::current().id(), caller);
                    let mut bundle = producer.acquire()?;
                    for (index, buffer) in bundle.iter_mut().enumerate() {
                        pointers[index].insert(buffer.as_ptr() as usize);
                        for pixel in buffer.chunks_exact_mut(4) {
                            pixel.copy_from_slice(&[tick as u8, 37, 89, 255]);
                        }
                    }
                    producer.frame(bundle, tick, 1)?;
                }
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(pointers[0].len(), 2);
        assert_eq!(pointers[1].len(), 1); // Empty alias, no second pixel allocation.
        assert_eq!(gates, 32);
        assert!(result.files[0].as_ref().unwrap().is_file());
        assert!(result.staged[1].is_some());
        assert!(result.probes[1].is_some());
        assert_eq!(
            result.reports[0]
                .as_ref()
                .unwrap()
                .transfers
                .cpu_conversion_input_bytes,
            32 * 64 * 64 * 4
        );
        drop(result);
        assert_eq!(std::fs::read_dir(output.path()).unwrap().count(), 0);
    }
    #[test]
    fn gif_cancel_gate_disconnects_consumer_and_removes_publish_stage() {
        let stage = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let config = config(stage.path(), output.path(), 4, &[MovieProfile::GifV1]);
        let error = run(
            config,
            &mut |_| {
                Err(MediaError::UnsupportedFeature(
                    "gif pass cancellation".into(),
                ))
            },
            |producer, _| {
                for tick in 0..4 {
                    let bundle = producer.acquire()?;
                    producer.frame(bundle, tick, 1)?;
                }
                Ok(())
            },
        )
        .err()
        .unwrap();
        assert_eq!(error.code(), "UNSUPPORTED_FEATURE");
        assert_eq!(std::fs::read_dir(output.path()).unwrap().count(), 0);
    }
    #[test]
    fn consumer_error_survives_failed_submission_and_worker_shutdown() {
        let stage = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let config = config(stage.path(), output.path(), 4, &[MovieProfile::ProResPcm24]);
        let error = run(config, &mut |_| Ok(()), |producer, _| {
            for tick in 0..4 {
                let mut bundle = producer.acquire()?;
                bundle[0].truncate(1);
                producer.frame(bundle, tick, 1)?;
            }
            Ok(())
        })
        .err()
        .unwrap();
        assert!(error.to_string().contains("frame size"), "{error}");
        assert_eq!(error.code(), "INVALID_MEDIA_INPUT");
        assert_eq!(std::fs::read_dir(output.path()).unwrap().count(), 0);
    }
}
