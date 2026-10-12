//! One immutable mix per audio identity, with bounded codec block adapters.
use super::*;
use std::collections::BTreeMap;
use std::ops::Range;

struct HashWriter(Sha256);
impl std::io::Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn identity(
    leg: &DeliveryLeg<'_>,
    render_hash: &str,
    samples: &Range<i64>,
    clipping: ClippingPolicy,
) -> Result<[u8; 32], MediaError> {
    // The common validated render hash pins target, plans, authored effects,
    // gain automation, source locks and semantic versions. Preserve clip order
    // and float serialization; codec/container do not change PCM24 mixing.
    let mut writer = HashWriter(Sha256::new());
    serde_json::to_writer(
        &mut writer,
        &(
            render_hash,
            leg.snapshot.schema_version,
            leg.snapshot.audio,
            &leg.snapshot.clips,
            leg.snapshot.audio_layout,
            samples.start,
            samples.end,
            clipping,
        ),
    )?;
    Ok(writer.0.finalize().into())
}

struct BlockAdapter {
    pending: Vec<i32>,
    samples_per_block: usize,
    channels: usize,
}
impl BlockAdapter {
    fn new(frames: usize, channels: usize) -> Result<Self, MediaError> {
        let samples_per_block = frames
            .checked_mul(channels)
            .filter(|_| (1..=65536).contains(&frames) && (1..=8).contains(&channels))
            .ok_or_else(|| MediaError::InvalidInput("audio adapter block budget".into()))?;
        Ok(Self {
            pending: Vec::new(),
            samples_per_block,
            channels,
        })
    }
    fn push(
        &mut self,
        mut samples: &[i32],
        emit: &mut impl FnMut(&[i32]) -> Result<(), MediaError>,
    ) -> Result<(), MediaError> {
        if !samples.len().is_multiple_of(self.channels) {
            return Err(MediaError::InvalidInput(
                "audio adapter channel alignment".into(),
            ));
        }
        while !samples.is_empty() {
            if self.pending.is_empty() && samples.len() >= self.samples_per_block {
                emit(&samples[..self.samples_per_block])?;
                samples = &samples[self.samples_per_block..];
            } else {
                if self.pending.capacity() == 0 {
                    self.pending.reserve_exact(self.samples_per_block);
                }
                let count = (self.samples_per_block - self.pending.len()).min(samples.len());
                self.pending.extend_from_slice(&samples[..count]);
                samples = &samples[count..];
                if self.pending.len() == self.samples_per_block {
                    emit(&self.pending)?;
                    self.pending.clear();
                }
            }
        }
        Ok(())
    }
    fn finish(
        &mut self,
        emit: &mut impl FnMut(&[i32]) -> Result<(), MediaError>,
    ) -> Result<(), MediaError> {
        if !self.pending.is_empty() {
            emit(&self.pending)?;
            self.pending.clear();
        }
        Ok(())
    }
}

pub(super) struct Config<'a> {
    pub runtime: &'a MediaRuntime,
    pub legs: &'a [DeliveryLeg<'a>],
    pub render_hash: &'a str,
    pub samples: Range<i64>,
    pub stage: &'a Path,
    pub clipping: ClippingPolicy,
    pub diagnostics: crate::ExportDiagnosticsCollector,
}
pub(super) struct ResultData {
    pub reports: Vec<Option<AudioEncodeReport>>,
    pub files: Vec<Option<LegStage>>,
}
pub(super) fn run(
    config: Config<'_>,
    sources: &dyn kronello_audio::ChannelSourceReader,
    checkpoint: &mut dyn FnMut(u64) -> Result<(), MediaError>,
) -> Result<ResultData, MediaError> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut identities = BTreeMap::new();
    for (index, leg) in config.legs.iter().enumerate() {
        if leg.profile.audio_kind().is_none() {
            continue;
        }
        let key = identity(leg, config.render_hash, &config.samples, config.clipping)?;
        let group = *identities.entry(key).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[group].push(index);
    }
    let mut result = ResultData {
        reports: (0..config.legs.len()).map(|_| None).collect(),
        files: (0..config.legs.len()).map(|_| None).collect(),
    };
    for indexes in groups {
        checkpoint(0)?;
        config.diagnostics.count("audio_mix_groups", 1);
        let leg = &config.legs[indexes[0]];
        let layout = leg.snapshot.audio_layout;
        let prepare = config.diagnostics.time("audio_plan_encoder_open");
        let plan = if leg.snapshot.audio == AudioSourceMode::Document {
            config.diagnostics.count("audio_plan_compiles", 1);
            Some(document_plan(
                &leg.snapshot.render,
                leg.snapshot.schema_version,
            )?)
        } else {
            None
        };
        let mut session = plan.as_ref().map(|plan| plan.playback_session());
        let mut encoders = Vec::new();
        for index in indexes {
            let leg = &config.legs[index];
            let kind = leg.profile.audio_kind().expect("audio group member");
            let file = if leg.profile.shape() == DeliveryShape::AudioOnly {
                LegStage::Publish(stage_file(&leg.output)?)
            } else {
                LegStage::Intermediate(
                    config
                        .stage
                        .join(format!("audio_{index}.{}", kind.intermediate_suffix())),
                )
            };
            let encoder = crate::ffi::NativeAudioEncoder::open(
                &config.runtime.native,
                file.path(),
                kind,
                layout,
            )?;
            let adapter = BlockAdapter::new(encoder.block, layout.channels())?;
            encoders.push((index, kind, file, encoder, adapter));
        }
        // Never enlarge a codec's former mix request: DSP operation budgets
        // and preroll remain valid even for a codec with short native blocks.
        let quantum = encoders
            .iter()
            .map(|(_, _, _, encoder, _)| encoder.block)
            .min()
            .unwrap()
            .min(4096);
        drop(prepare);
        let mut start = config.samples.start;
        let mut clipped_samples = 0_usize;
        while start < config.samples.end {
            checkpoint(0)?;
            let end = config.samples.end.min(
                start
                    .checked_add(quantum as i64)
                    .ok_or_else(|| MediaError::InvalidInput("audio batch overflow".into()))?,
            );
            let range = TimeRange::new(
                SAMPLE_RATE.sample_to_time(start)?,
                SAMPLE_RATE.sample_to_time(end)?,
            )?;
            let mix = config.diagnostics.time("audio_mix");
            let bus = match (&plan, session.as_mut()) {
                (Some(plan), Some(session)) => {
                    plan.mix_channels_streaming(sources, range, layout, session)?
                }
                (Some(plan), None) => plan.mix_channels(sources, range, layout)?,
                (None, _) => {
                    kronello_audio::mix_channels(&leg.snapshot.clips, sources, range, layout)?
                }
            };
            drop(mix);
            let quantize = config.diagnostics.time("audio_quantize");
            let quantized = bus.quantize_pcm24(config.clipping)?;
            config
                .diagnostics
                .peak_payload("audio_quantized_block", quantized.samples.len() as u64 * 4);
            drop(quantize);
            config.diagnostics.count("audio_mix_blocks", 1);
            config
                .diagnostics
                .count("audio_mixed_frames", (end - start) as u64);
            clipped_samples = clipped_samples
                .checked_add(quantized.clipped_samples)
                .ok_or_else(|| MediaError::InvalidInput("clipping count overflow".into()))?;
            for (_, _, _, encoder, adapter) in &mut encoders {
                let _encode = config.diagnostics.time("audio_encode");
                adapter.push(&quantized.samples, &mut |samples| encoder.frame(samples))?;
            }
            config.diagnostics.peak_payload(
                "audio_codec_adapters",
                encoders
                    .iter()
                    .map(|(_, _, _, _, adapter)| adapter.pending.capacity() as u64 * 4)
                    .sum(),
            );
            start = end;
        }
        for (index, kind, file, mut encoder, mut adapter) in encoders {
            let _finish = config.diagnostics.time("audio_flush");
            adapter.finish(&mut |samples| encoder.frame(samples))?;
            encoder.finish()?;
            drop(encoder);
            result.reports[index] = Some(AudioEncodeReport {
                codec: kind.codec_name().into(),
                sample_rate: 48000,
                channels: layout.channels() as u32,
                frames: usize::try_from(config.samples.end - config.samples.start)
                    .map_err(|_| MediaError::InvalidInput("audio count overflow".into()))?,
                clipped_samples,
            });
            result.files[index] = Some(file);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kronello_time::Time;

    #[test]
    fn adapters_preserve_samples_full_blocks_and_single_final_partial() {
        let samples: Vec<_> = (0..10301 * 2).map(|i| (i * 257) ^ 0x12345678).collect();
        for block in [960, 1024, 1152, 4096, 4608] {
            for quantum in [17, 960, 1024, 4096, 4608] {
                let mut adapter = BlockAdapter::new(block, 2).unwrap();
                let mut emitted = Vec::new();
                let mut emit = |samples: &[i32]| {
                    emitted.push(samples.to_vec());
                    Ok(())
                };
                for chunk in samples.chunks(quantum * 2) {
                    adapter.push(chunk, &mut emit).unwrap();
                }
                adapter.finish(&mut emit).unwrap();
                adapter.finish(&mut emit).unwrap();
                assert_eq!(emitted.concat(), samples);
                assert!(
                    emitted[..emitted.len() - 1]
                        .iter()
                        .all(|block_samples| block_samples.len() == block * 2)
                );
                assert_eq!(emitted.last().unwrap().len(), (10301 % block) * 2);
                assert!(adapter.pending.capacity() <= block * 2);
            }
        }
        let mut adapter = BlockAdapter::new(4096, 2).unwrap();
        adapter.push(&samples[..8192], &mut |_| Ok(())).unwrap();
        assert_eq!(adapter.pending.capacity(), 0);
    }

    #[test]
    fn adapters_reject_invalid_layout_budget_and_stop_on_encoder_error() {
        for (frames, channels) in [(0, 2), (65537, 2), (1024, 0), (1024, 9)] {
            assert!(BlockAdapter::new(frames, channels).is_err());
        }
        let mut adapter = BlockAdapter::new(1024, 2).unwrap();
        assert!(
            adapter
                .push(&[1], &mut |_| panic!("unaligned input must not encode"))
                .is_err()
        );
        let mut calls = 0;
        let error = adapter
            .push(&vec![0; 4096], &mut |_| {
                calls += 1;
                Err(MediaError::Encode("injected encoder failure".into()))
            })
            .unwrap_err();
        assert_eq!(calls, 1);
        assert_eq!(error.code(), "ENCODE_ERROR");
    }

    #[test]
    fn identity_pins_audio_semantics_and_excludes_delivery_settings() {
        let project: kronello_model::Project =
            serde_json::from_str(include_str!("../../../examples/perf-codec.project.json"))
                .unwrap();
        let DocumentObject::Known(sequence) = &project.sequences[0] else {
            panic!("known sequence")
        };
        let render = RenderSnapshot::for_target(
            &project,
            kronello_render::RenderTarget::Sequence {
                sequence: sequence.id,
            },
            0,
            kronello_render::RenderProfile::default(),
        )
        .unwrap();
        let snapshot =
            AvExportSnapshot::with_audio_profile(&render, AudioSourceMode::Silence, vec![], 3)
                .unwrap();
        let mut other = snapshot.clone();
        fn leg(snapshot: &AvExportSnapshot) -> DeliveryLeg<'_> {
            DeliveryLeg {
                snapshot,
                output: PathBuf::from("unused.mov"),
                profile: MovieProfile::ProResPcm24,
                background: [0.0; 3],
                chapters_policy: ChapterPolicy::Omit,
            }
        }
        let key = identity(
            &leg(&snapshot),
            "render-lock",
            &(100..10301),
            ClippingPolicy::Reject,
        )
        .unwrap();
        let mut other_leg = leg(&other);
        other_leg.profile = MovieProfile::FlacV1;
        other_leg.background = [0.5; 3];
        other_leg.chapters_policy = ChapterPolicy::Transfer;
        assert_eq!(
            identity(
                &other_leg,
                "render-lock",
                &(100..10301),
                ClippingPolicy::Reject
            )
            .unwrap(),
            key
        );
        assert_ne!(
            identity(
                &leg(&snapshot),
                "different-source-lock-plan",
                &(100..10301),
                ClippingPolicy::Reject
            )
            .unwrap(),
            key
        );
        assert_ne!(
            identity(
                &leg(&snapshot),
                "render-lock",
                &(101..10301),
                ClippingPolicy::Reject
            )
            .unwrap(),
            key
        );
        assert_ne!(
            identity(
                &leg(&snapshot),
                "render-lock",
                &(100..10301),
                ClippingPolicy::Saturate
            )
            .unwrap(),
            key
        );
        for change in 0..3 {
            other = snapshot.clone();
            match change {
                0 => other.schema_version = 2,
                1 => other.audio = AudioSourceMode::Document,
                _ => other.audio_layout = kronello_model::ChannelMask::MONO,
            }
            assert_ne!(
                identity(
                    &leg(&other),
                    "render-lock",
                    &(100..10301),
                    ClippingPolicy::Reject
                )
                .unwrap(),
                key
            );
        }
        other = snapshot.clone();
        other.audio = AudioSourceMode::Explicit;
        other.clips = vec![AudioClip {
            asset: kronello_model::AssetId::new(),
            stream_index: 0,
            placement: TimeRange::new(Time::ZERO, Time::new(1, 1).unwrap()).unwrap(),
            source_in: Time::ZERO,
            gain: kronello_audio::Gain::new(1.0).unwrap(),
        }];
        let original = identity(
            &leg(&other),
            "render-lock",
            &(100..10301),
            ClippingPolicy::Reject,
        )
        .unwrap();
        other.clips[0].gain = kronello_audio::Gain::new(0.5).unwrap();
        assert_ne!(
            identity(
                &leg(&other),
                "render-lock",
                &(100..10301),
                ClippingPolicy::Reject
            )
            .unwrap(),
            original
        );
    }
}
