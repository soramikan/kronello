//! AUDIO-004: bounded, immutable, independently addressable sample evaluation.
//! AUDIO-010 adds pitch-preserved (WSOLA) sources and channel-masked mixing.
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use kronello_model::*;
use kronello_time::{Time, TimeMap, TimeRange};

use crate::{
    AudioClip, AudioError, AudioTarget, Bus, ChannelBuffer, ChannelBus, ChannelSourceReader,
    ChannelTrackMeter, DocumentAudioPlan, Gain, MAX_AUDIO_FRAMES, MAX_CHANNELS, channel_meter,
    layout_matrix, sample_range, wsola::Wsola,
};

/// Selected by movie profile 3; legacy movie profiles retain evaluator 1.
pub const AUDIO_EVALUATION_VERSION: u32 = 2;
pub const AUDIO_GENERATOR_SILENCE: &str = "kronello.audio.silence";
pub const AUDIO_GENERATOR_TONE: &str = "kronello.audio.tone440";
const MAX_SAMPLE_OPERATIONS: u64 = 100_000_000;
const MAX_MAP_POINTS: usize = 1024;
/// Abstract per-sample cost of a WSOLA source in the operations budget. It
/// deliberately over-charges the average-case work (the hop search dominates)
/// so a render cannot schedule unbounded correlation work.
const WSOLA_SAMPLE_COST: u64 = 48;

#[derive(Debug, Clone)]
enum Source {
    Legacy(DocumentAudioPlan),
    Resampled,
    /// AUDIO-010: deterministic WSOLA over the source's own channel layout.
    PitchPreserved,
    /// FX-008: `kronello.audio.pitch` — the same deterministic WSOLA source
    /// whose emitted windows stride the source by `rate = 2^(semitones/12)`:
    /// pitch shifts by `rate` while duration and cursor position are
    /// preserved (ADR-0137).
    Pitch {
        rate: f64,
    },
    ReversedComposition {
        plan: DocumentAudioPlan,
        duration: Time,
    },
    Generator,
}
/// AUDIO-003 gain Properties remain pointwise; AUDIO-007/008 resolved
/// filters/dynamics carry deterministic state from the placement boundary.
#[derive(Debug, Clone)]
enum EntryEffect {
    Gain(Property),
    Dsp(ResolvedAudioEffect),
}
#[derive(Debug, Clone)]
struct Entry {
    clip: Clip,
    source: Source,
    effects: Vec<EntryEffect>,
    fades: Vec<(Range<i64>, bool)>,
    track: TrackId,
}
impl Entry {
    /// Stateful chains and WSOLA cursors must evaluate from placement start
    /// so any request range reproduces continuous-render samples (ADR-0117).
    fn stateful(&self) -> bool {
        matches!(self.source, Source::PitchPreserved | Source::Pitch { .. })
            || self
                .effects
                .iter()
                .any(|effect| matches!(effect, EntryEffect::Dsp(_)))
    }
    /// Per-sample operation estimate: gain lookup is cheap; DSP stages run
    /// filter/dynamics/delay lines over the bus channels. The reverb's
    /// per-channel comb bank costs more than a single biquad/dynamics pass.
    fn effect_cost(&self) -> u64 {
        self.effects
            .iter()
            .map(|effect| match effect {
                EntryEffect::Gain(_) => 1,
                EntryEffect::Dsp(ResolvedAudioEffect::Reverb { .. }) => 24,
                EntryEffect::Dsp(_) => 8,
            })
            .sum()
    }
}
/// A live processing step inside one mix request. Gains keep evaluated
/// values; DSP stages own the zero-initialized per-call state.
enum ChainStep {
    Gain(Property),
    Dsp(crate::dsp::Processor),
}
impl ChainStep {
    /// DSP processors are instantiated over the bus layout; only the LFE
    /// channel skips compressor/limiter treatment.
    fn new(effect: &EntryEffect, mask: ChannelMask) -> Self {
        match effect {
            EntryEffect::Gain(property) => Self::Gain(property.clone()),
            EntryEffect::Dsp(spec) => Self::Dsp(crate::dsp::processor(spec, mask)),
        }
    }
}
#[derive(Debug, Clone)]
pub(crate) struct AdvancedAudioPlan {
    entries: Vec<Entry>,
    curves: BTreeMap<CurveId, AnimationCurve>,
}
fn invalid(message: &str) -> AudioError {
    AudioError::InvalidInput(message.into())
}
fn unsupported(message: &str) -> AudioError {
    AudioError::Unsupported(message.into())
}
fn budget(message: &str) -> AudioError {
    AudioError::BudgetExceeded(message.into())
}
/// AUDIO-007/008 descriptor registry used to type-check effect parameter
/// Properties against their declared keys and value types.
fn effect_registry() -> &'static SchemaRegistry {
    static REGISTRY: std::sync::LazyLock<SchemaRegistry> = std::sync::LazyLock::new(|| {
        let mut registry = SchemaRegistry::with_builtin();
        for descriptor in kronello_model::effect_descriptors() {
            registry
                .register(descriptor)
                .expect("effect descriptor registration");
        }
        registry
    });
    &REGISTRY
}
/// Property ids referenced by the AUDIO-007/008 constant-parameter effects.
fn audio_parameter_ids(parameters: &EffectParameters) -> Vec<PropertyId> {
    match parameters {
        EffectParameters::AudioEq { bands } => vec![*bands],
        EffectParameters::AudioHpf { cutoff_hz, order }
        | EffectParameters::AudioLpf { cutoff_hz, order } => vec![*cutoff_hz, *order],
        EffectParameters::AudioCompressor {
            threshold_db,
            ratio,
            attack_ms,
            release_ms,
            makeup_db,
        } => vec![*threshold_db, *ratio, *attack_ms, *release_ms, *makeup_db],
        EffectParameters::AudioLimiter {
            ceiling_db,
            release_ms,
        } => vec![*ceiling_db, *release_ms],
        EffectParameters::AudioDelay {
            delay_ms,
            feedback_db,
            wet,
            dry,
        } => vec![*delay_ms, *feedback_db, *wet, *dry],
        EffectParameters::AudioReverb {
            decay_ms,
            damping,
            wet,
            dry,
        } => vec![*decay_ms, *damping, *wet, *dry],
        EffectParameters::AudioPitch { semitones } => vec![*semitones],
        EffectParameters::AudioGate {
            threshold_db,
            attack_ms,
            release_ms,
            hysteresis_db,
        } => vec![*threshold_db, *attack_ms, *release_ms, *hysteresis_db],
        _ => vec![],
    }
}
fn number(t: Time) -> f64 {
    t.numerator() as f64 / t.denominator() as f64
}
fn validate_map(map: &TimeMap) -> Result<(), AudioError> {
    match map {
        TimeMap::Linear(_) => Ok(()),
        TimeMap::PiecewiseLinear(m) if m.points().len() <= MAX_MAP_POINTS => Ok(()),
        TimeMap::PiecewiseLinear(_) => Err(budget("audio map control points")),
        _ => Err(unsupported(
            "audio supports positive linear / piecewise-linear maps only",
        )),
    }
}
/// Floor buckets can start before a fractional authored start. Version 2
/// explicitly extends the first segment for that one partial bucket only.
/// It never substitutes a zero sample or changes negative source positions.
pub(crate) fn local_time(clip: &Clip, sample: i64) -> Result<Time, AudioError> {
    let parent = Time::new(sample, 48_000)?.checked_sub(clip.timeline_range.start())?;
    let mapped = if parent < Time::ZERO {
        match &clip.time_map {
            TimeMap::PiecewiseLinear(m) => {
                let a = m.points()[0];
                let b = m.points()[1];
                if a.parent != Time::ZERO {
                    return Err(unsupported("audio piecewise map must start at parent zero"));
                }
                a.local.checked_add(
                    parent.checked_mul(
                        b.local
                            .checked_sub(a.local)?
                            .checked_div(b.parent.checked_sub(a.parent)?)?,
                    )?,
                )?
            }
            _ => clip.time_map.map(parent)?,
        }
    } else {
        clip.time_map.map(parent)?
    };
    Ok(if clip.reverse_sampling.is_some() {
        clip.source_in.checked_sub(mapped)?
    } else {
        clip.source_in.checked_add(mapped)?
    })
}
impl AdvancedAudioPlan {
    pub(crate) fn compile(project: &Project, target: AudioTarget) -> Result<Self, AudioError> {
        let AudioTarget::Sequence(id) = target else {
            return Err(unsupported("advanced audio target must be a Sequence"));
        };
        let sequence = project
            .sequences
            .iter()
            .find_map(|s| match s {
                DocumentObject::Known(s) if s.id == id => Some(s),
                _ => None,
            })
            .ok_or_else(|| unsupported("missing or opaque audio sequence"))?;
        // Bound the authored work before Sequence's overlap scans and map evaluation.
        if sequence.tracks.len() > 1024
            || sequence.tracks.iter().map(|t| t.clips.len()).sum::<usize>() > 1024
            || sequence.transitions.len() > 1024
        {
            return Err(budget("audio tracks / clips / transitions"));
        }
        for clip in sequence.tracks.iter().flat_map(|t| &t.clips) {
            validate_map(&clip.time_map)?;
            if clip.effects.len() > 16 {
                return Err(budget("audio effect stack"));
            }
        }
        sequence.validate(project)?;
        let mut plan = Self {
            entries: vec![],
            curves: BTreeMap::new(),
        };
        let mut flattened_count = 0;
        let mut legacy_curve_keys = 0;
        for track in &sequence.tracks {
            if track.muted() {
                continue;
            }
            for clip in &track.clips {
                // NLE-005: a disabled placement keeps its timeline occupancy
                // but is inaudible and never enters the mix plan.
                if !clip.enabled {
                    continue;
                }
                let audible = match clip.source_ref {
                    SourceRef::Composition { composition } => {
                        super::document::has_audio(project, composition, &mut BTreeSet::new())?
                    }
                    _ => track.kind == TrackKind::Audio,
                };
                if !audible {
                    continue;
                }
                let mut effects = vec![];
                // FX-008: `kronello.audio.pitch` executes at the source
                // stage — resolved semitones accumulate into the WSOLA rate
                // below instead of entering the DSP chain (ADR-0137).
                let mut pitch_semitones = 0.0_f64;
                let mut pitched = false;
                let mut referenced = BTreeSet::new();
                for effect in &clip.effects {
                    if track.kind != TrackKind::Audio {
                        return Err(unsupported("audio effects require an audio track"));
                    }
                    let definition = effect
                        .definition()
                        .map_err(|_| unsupported("audio effect id / version"))?;
                    match &definition.parameters {
                        EffectParameters::AudioGain { gain } => {
                            let property = clip
                                .properties
                                .iter()
                                .find(|p| p.id() == *gain)
                                .ok_or_else(|| invalid("audio effect gain Property missing"))?;
                            plan.capture_property(project, property)?;
                            referenced.insert(*gain);
                            effects.push(EntryEffect::Gain(property.clone()));
                        }
                        _ => {
                            // AUDIO-007/008: constant-parameter filters and
                            // dynamics, validated through the shared model
                            // resolution so failures are typed parameter errors.
                            definition
                                .validate(&clip.properties, effect_registry())
                                .map_err(|e| match e {
                                    EffectError::InvalidParameter(id) => {
                                        invalid(&format!("audio effect parameter Property {id}"))
                                    }
                                    _ => unsupported("audio effect id / version"),
                                })?;
                            // AUDIO-011 (ADR-0131): third-party plugin code
                            // never runs in the evaluator — only in the
                            // detached plugin worker via audio.plugin_process.
                            if matches!(definition.parameters, EffectParameters::AudioPlugin { .. })
                            {
                                return Err(unsupported(
                                    "kronello.audio.plugin executes only through \
                                     the detached plugin worker (audio.plugin_process)",
                                ));
                            }
                            let mut values = BTreeMap::new();
                            for id in audio_parameter_ids(&definition.parameters) {
                                let property =
                                    clip.properties.iter().find(|p| p.id() == id).ok_or_else(
                                        || invalid("audio effect parameter Property missing"),
                                    )?;
                                let PropertySource::Constant(value) = property.source() else {
                                    return Err(unsupported(
                                        "audio filter/dynamics parameters require Constant sources",
                                    ));
                                };
                                referenced.insert(id);
                                values.insert(id, value.clone());
                            }
                            let spec = definition.resolve_audio(&values).map_err(|e| match e {
                                EffectError::InvalidParameter(id) => {
                                    invalid(&format!("audio effect parameter {id} out of range"))
                                }
                                _ => {
                                    unsupported("audio clips accept kronello.audio.* effects only")
                                }
                            })?;
                            // FX-008: pitch is a source-stage effect; several
                            // pitch entries compose by adding semitones.
                            if let ResolvedAudioEffect::Pitch { semitones } = spec {
                                pitch_semitones += semitones;
                                pitched = true;
                            } else {
                                effects.push(EntryEffect::Dsp(spec));
                            }
                        }
                    }
                }
                // Every authored audio Property must belong to the closed effect contract.
                if track.kind == TrackKind::Audio
                    && clip
                        .properties
                        .iter()
                        .any(|p| !referenced.contains(&p.id()))
                {
                    return Err(unsupported("unused / unsupported audio clip Property"));
                }
                let source = match &clip.source_ref {
                    SourceRef::Generator {
                        generator,
                        version,
                        color,
                    } => {
                        if *version != 1
                            || !matches!(
                                generator.as_str(),
                                AUDIO_GENERATOR_SILENCE | AUDIO_GENERATOR_TONE
                            )
                            || *color != Color::from_srgb8([0; 3], None)
                        {
                            return Err(unsupported("audio Generator id / version / parameters"));
                        }
                        if clip.audio_retime != AudioRetimePolicy::ResampleV1
                            && !matches!(&clip.time_map, TimeMap::Linear(m) if m.speed() == Time::ONE)
                        {
                            return Err(unsupported("Generator retime requires resample_v1"));
                        }
                        Source::Generator
                    }
                    SourceRef::Composition { composition } if clip.reverse_sampling.is_some() => {
                        let duration = project
                            .compositions
                            .iter()
                            .find_map(|c| match c {
                                DocumentObject::Known(c) if c.id == *composition => {
                                    Some(c.duration.as_time())
                                }
                                _ => None,
                            })
                            .ok_or_else(|| unsupported("reverse Composition missing"))?;
                        Source::ReversedComposition {
                            plan: DocumentAudioPlan::compile(
                                project,
                                AudioTarget::Composition(*composition),
                            )?,
                            duration,
                        }
                    }
                    SourceRef::Asset { .. }
                        if clip.audio_retime == AudioRetimePolicy::PitchPreserveV1 =>
                    {
                        Source::PitchPreserved
                    }
                    SourceRef::Asset { .. }
                        if matches!(
                            clip.audio_retime,
                            AudioRetimePolicy::ResampleV1 | AudioRetimePolicy::ReverseResampleV1
                        ) =>
                    {
                        Source::Resampled
                    }
                    _ => {
                        // Legacy recursive placement retains its exact affine sample phase.
                        // Nested/composition retime and node effects remain explicit errors.
                        // GUI-012: pan is applied by this mixer after the Legacy
                        // plan renders (see the pan_gains pass below); stripping
                        // it here keeps the version-1 clip contract untouched.
                        let mut clean = clip.clone();
                        clean.effects.clear();
                        clean.properties.clear();
                        clean.links.clear();
                        clean.pan = None;
                        Source::Legacy(DocumentAudioPlan::compile_isolated_clip(
                            project, target, &clean,
                        )?)
                    }
                };
                // FX-008: a resolved pitch effect replaces the source with
                // the deterministic WSOLA cursor whose emitted windows
                // stride the source by rate 2^(semitones/12), shifting
                // pitch while preserving duration. It requires an Asset
                // source on a forward retime policy — nested compositions,
                // Generators and reverse playback stay typed errors
                // (ADR-0137).
                let source = if pitched {
                    let rate = 2.0_f64.powf(pitch_semitones / 12.0);
                    if !(rate.is_finite() && rate > 0.0) {
                        return Err(invalid("pitch semitone rate out of range"));
                    }
                    let supported = matches!(clip.source_ref, SourceRef::Asset { .. })
                        && matches!(
                            clip.audio_retime,
                            AudioRetimePolicy::Reject
                                | AudioRetimePolicy::ResampleV1
                                | AudioRetimePolicy::PitchPreserveV1
                        )
                        && matches!(
                            source,
                            Source::Resampled | Source::PitchPreserved | Source::Legacy(_)
                        );
                    if !supported {
                        return Err(unsupported(
                            "kronello.audio.pitch requires an Asset source with a \
                             forward retime policy",
                        ));
                    }
                    Source::Pitch { rate }
                } else {
                    source
                };
                if !matches!(source, Source::Legacy(_))
                    && matches!(&clip.time_map, TimeMap::PiecewiseLinear(m) if m.points()[0].parent != Time::ZERO)
                {
                    return Err(unsupported("audio piecewise map must start at parent zero"));
                }
                if let Source::Legacy(legacy) | Source::ReversedComposition { plan: legacy, .. } =
                    &source
                {
                    legacy.audio4_sample_cost()?;
                    legacy_curve_keys += legacy.audio4_curve_keys();
                }
                if !matches!(source, Source::Legacy(_))
                    && let Some(property) = &clip.volume
                {
                    plan.capture_property(project, property)?;
                }
                // GUI-012: pan is a constant scalar — no curve capture, just
                // the shared contract check (applies to Legacy sources too).
                if let Some(pan) = &clip.pan {
                    validate_pan(pan).map_err(|e| invalid(&e.to_string()))?;
                }
                flattened_count += match &source {
                    Source::Legacy(p) | Source::ReversedComposition { plan: p, .. } => {
                        p.clips().len()
                    }
                    _ => 1,
                };
                if flattened_count > 1024 {
                    return Err(budget("flattened audio placements"));
                }
                if legacy_curve_keys
                    + plan
                        .curves
                        .values()
                        .map(|curve| curve.keys().len())
                        .sum::<usize>()
                    > 65536
                {
                    return Err(budget("audio Curve keys across placements"));
                }
                let mut fades = vec![];
                for transition in &sequence.transitions {
                    if transition.outgoing != clip.id && transition.incoming != clip.id {
                        continue;
                    }
                    if transition.version != 1 {
                        return Err(unsupported("audio crossfade version"));
                    }
                    let samples = sample_range(transition.range)?;
                    if samples.is_empty() {
                        return Err(invalid("audio crossfade has no samples"));
                    }
                    samples
                        .end
                        .checked_sub(samples.start)
                        .ok_or(AudioError::Overflow)?;
                    fades.push((samples, transition.incoming == clip.id));
                }
                plan.entries.push(Entry {
                    clip: clip.clone(),
                    source,
                    effects,
                    fades,
                    track: track.id,
                });
            }
        }
        Ok(plan)
    }
    fn capture_property(
        &mut self,
        project: &Project,
        property: &Property,
    ) -> Result<(), AudioError> {
        validate_volume(property)
            .map_err(|_| unsupported("audio effect requires volume Scalar Constant / Curve"))?;
        if let PropertySource::Curve(id) = property.source() {
            if self.curves.contains_key(id) {
                return Ok(());
            }
            let curve = project
                .curves
                .iter()
                .find_map(|c| match c {
                    DocumentObject::Known(c) if c.id() == *id => Some(c),
                    _ => None,
                })
                .ok_or_else(|| invalid("audio curve missing or opaque"))?;
            if curve.value_type() != ValueType::Scalar || curve.keys().is_empty() {
                return Err(invalid("audio curve must be nonempty Scalar"));
            }
            if curve.keys().len() > 4096
                || self.curves.values().map(|c| c.keys().len()).sum::<usize>() + curve.keys().len()
                    > 65536
            {
                return Err(budget("audio Curve keys"));
            }
            curve
                .ensure_supported_version()
                .map_err(|_| unsupported("audio curve interpolation version"))?;
            self.curves.insert(*id, curve.clone());
        }
        Ok(())
    }
    fn gain(&self, property: &Property, time: Time) -> Result<f32, AudioError> {
        let value = match property.source() {
            PropertySource::Constant(v) => v.clone(),
            PropertySource::Curve(id) => kronello_animation::sample(&self.curves[id], time)
                .map_err(|e| invalid(&e.to_string()))?,
            _ => return Err(unsupported("audio Property source")),
        };
        Ok(super::document::scalar_gain(value)?.linear())
    }
    /// Asset decode requests only; Generator entries never invent AssetIds.
    pub(crate) fn clips(&self) -> Vec<AudioClip> {
        self.entries
            .iter()
            .flat_map(|entry| match &entry.source {
                Source::Legacy(plan) | Source::ReversedComposition { plan, .. } => plan.clips(),
                Source::Resampled | Source::PitchPreserved | Source::Pitch { .. } => {
                    let SourceRef::Asset {
                        asset,
                        stream_index,
                    } = entry.clip.source_ref
                    else {
                        unreachable!()
                    };
                    vec![AudioClip {
                        asset,
                        stream_index,
                        placement: entry.clip.timeline_range,
                        source_in: entry.clip.source_in,
                        gain: Gain::UNITY,
                    }]
                }
                Source::Generator => vec![],
            })
            .collect()
    }
    pub(crate) fn mix(
        &self,
        sources: &dyn ChannelSourceReader,
        range: TimeRange,
    ) -> Result<Bus, AudioError> {
        self.mix_channels(sources, range, ChannelMask::STEREO)?
            .into_stereo_bus()
    }
    /// AUDIO-010: identical mixing at any supported bus layout; sources
    /// convert through the explicit ITU matrix (LFE excluded).
    pub(crate) fn mix_channels(
        &self,
        sources: &dyn ChannelSourceReader,
        range: TimeRange,
        target: ChannelMask,
    ) -> Result<ChannelBus, AudioError> {
        Ok(self.mix_channels_impl(sources, range, target, false)?.0)
    }
    /// AUDIO-009: identical mixing path plus per-track and master peak/RMS.
    pub(crate) fn mix_metered(
        &self,
        sources: &dyn ChannelSourceReader,
        range: TimeRange,
    ) -> Result<(Bus, crate::BusMeters), AudioError> {
        let (bus, meters) = self.mix_channels_impl(sources, range, ChannelMask::STEREO, true)?;
        let stereo = |peak: &[f32], rms: &[f32]| -> ([f32; 2], [f32; 2]) {
            ([peak[0], peak[1]], [rms[0], rms[1]])
        };
        let meters = crate::BusMeters {
            tracks: meters
                .tracks
                .iter()
                .map(|track| {
                    let (peak, rms) = stereo(&track.peak, &track.rms);
                    crate::TrackMeter {
                        track: track.track,
                        peak,
                        rms,
                    }
                })
                .collect(),
            master: {
                let (peak, rms) = stereo(&meters.master.peak, &meters.master.rms);
                crate::StereoMeter { peak, rms }
            },
        };
        Ok((bus.into_stereo_bus()?, meters))
    }
    /// AUDIO-010: metered variant at an arbitrary target layout.
    pub(crate) fn mix_channels_metered(
        &self,
        sources: &dyn ChannelSourceReader,
        range: TimeRange,
        target: ChannelMask,
    ) -> Result<(ChannelBus, crate::ChannelBusMeters), AudioError> {
        self.mix_channels_impl(sources, range, target, true)
    }
    fn mix_channels_impl(
        &self,
        sources: &dyn ChannelSourceReader,
        range: TimeRange,
        target: ChannelMask,
        metered: bool,
    ) -> Result<(ChannelBus, crate::ChannelBusMeters), AudioError> {
        let output = sample_range(range)?;
        let length = output
            .end
            .checked_sub(output.start)
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| *n <= MAX_AUDIO_FRAMES)
            .ok_or_else(|| budget("audio Bus frames"))?;
        let channels = target.channels();
        let mut operations = 0_u64;
        for entry in &self.entries {
            let placement = sample_range(entry.clip.timeline_range)?;
            // Stateful chains run from placement start even outside the
            // request; charge the complete evaluated span.
            let span_start = if entry.stateful() {
                placement.start
            } else {
                output.start.max(placement.start)
            };
            let samples = output
                .end
                .min(placement.end)
                .saturating_sub(span_start)
                .max(0) as u64;
            let cost = 1
                + entry.effect_cost()
                + entry.fades.len() as u64
                + match &entry.source {
                    Source::Legacy(p) => p.audio4_sample_cost()?,
                    Source::ReversedComposition { plan, .. } => plan
                        .audio4_sample_cost()?
                        .checked_mul(2)
                        .and_then(|v| v.checked_add(8))
                        .ok_or_else(|| budget("reverse Composition sample cost"))?,
                    Source::PitchPreserved | Source::Pitch { .. } => WSOLA_SAMPLE_COST,
                    _ => 3,
                };
            operations = operations
                .checked_add(
                    samples
                        .checked_mul(cost)
                        .ok_or_else(|| budget("audio sample operations"))?,
                )
                .ok_or_else(|| budget("audio sample operations"))?;
            if matches!(entry.source, Source::Legacy(_)) {
                // The legacy mixer initializes and validates a full-size Bus,
                // even when this placement is disjoint from the request.
                // Charge that work before any output allocation; a stateful
                // chain extends the Bus back to the placement boundary.
                let bus_span = if entry.stateful() {
                    samples
                } else {
                    length as u64
                };
                operations = operations
                    .checked_add(bus_span * 2 * channels as u64)
                    .ok_or_else(|| budget("audio sample operations"))?;
            }
        }
        if operations > MAX_SAMPLE_OPERATIONS {
            return Err(budget("audio sample operations"));
        }
        // Preflight full authored source ranges, including the interpolation tail,
        // even for an empty/disjoint batch or zero gain.
        for entry in &self.entries {
            if matches!(entry.source, Source::Generator) {
                let placement = sample_range(entry.clip.timeline_range)?;
                if !placement.is_empty()
                    && (local_time(&entry.clip, placement.start)? < Time::ZERO
                        || local_time(&entry.clip, placement.end - 1)? < Time::ZERO)
                {
                    return Err(invalid("negative Generator source time"));
                }
            }
            if let Source::ReversedComposition { plan, duration } = &entry.source {
                let placement = sample_range(entry.clip.timeline_range)?;
                if !placement.is_empty() {
                    let mut scratch = [0.0; MAX_CHANNELS];
                    reversed_composition(
                        &entry.clip,
                        plan,
                        *duration,
                        sources,
                        placement.start,
                        target,
                        &mut scratch[..channels],
                    )?;
                    reversed_composition(
                        &entry.clip,
                        plan,
                        *duration,
                        sources,
                        placement.end - 1,
                        target,
                        &mut scratch[..channels],
                    )?;
                }
            }
            if matches!(entry.source, Source::Resampled) {
                let SourceRef::Asset {
                    asset,
                    stream_index,
                } = entry.clip.source_ref
                else {
                    unreachable!()
                };
                let source_channels = sources.layout(asset, stream_index)?.channels();
                sources.frame_count(asset, stream_index)?;
                let placement = sample_range(entry.clip.timeline_range)?;
                if !placement.is_empty() {
                    let mut scratch = [0.0; MAX_CHANNELS];
                    resample(
                        &entry.clip,
                        sources,
                        placement.start,
                        &mut scratch[..source_channels],
                    )?;
                    resample(
                        &entry.clip,
                        sources,
                        placement.end - 1,
                        &mut scratch[..source_channels],
                    )?;
                }
            }
            if matches!(entry.source, Source::PitchPreserved | Source::Pitch { .. }) {
                let SourceRef::Asset {
                    asset,
                    stream_index,
                } = entry.clip.source_ref
                else {
                    unreachable!()
                };
                sources.layout(asset, stream_index)?;
                let source_length = sources.frame_count(asset, stream_index)?;
                let placement = sample_range(entry.clip.timeline_range)?;
                if !placement.is_empty() {
                    // The nominal WSOLA cursor must land inside the source
                    // for the placement endpoints; the correlation window
                    // itself may read zero-filled context at the edges. The
                    // FX-008 pitch rate strides the emitted window but does
                    // not scale the cursor, so the same bound covers
                    // Source::Pitch.
                    for sample in [placement.start, placement.end - 1] {
                        let local = local_time(&entry.clip, sample)?
                            .checked_mul(Time::from_integer(48_000))?;
                        let too_short = local < Time::ZERO
                            || usize::try_from(local.floor())
                                .ok()
                                .is_none_or(|index| index >= source_length);
                        if too_short {
                            return Err(AudioError::SourceTooShort(asset));
                        }
                    }
                }
            }
        }
        let mut frames = vec![0.0_f32; length * channels];
        // Meter accumulation mirrors the output buffer per contributing track.
        let mut track_meters: Vec<(TrackId, Vec<f32>)> = Vec::new();
        for entry in &self.entries {
            let placement = sample_range(entry.clip.timeline_range)?;
            let end = output.end.min(placement.end);
            let start = if entry.stateful() {
                placement.start
            } else {
                output.start.max(placement.start)
            };
            if start >= end {
                continue;
            }
            // A stateful legacy chain evaluates the inner plan from the
            // placement boundary so the DSP sees continuous history. The bus
            // arrives already converted to the target layout.
            let legacy = match &entry.source {
                Source::Legacy(p) => Some(p.mix_channels(
                    sources,
                    TimeRange::new(Time::new(start, 48_000)?, Time::new(end, 48_000)?)?,
                    target,
                )?),
                _ => None,
            };
            let (source_mask, matrix) = match &entry.source {
                Source::Legacy(_) | Source::ReversedComposition { .. } => (target, None),
                Source::Generator => (
                    ChannelMask::MONO,
                    Some(layout_matrix(ChannelMask::MONO, target, false)?),
                ),
                Source::Resampled | Source::PitchPreserved | Source::Pitch { .. } => {
                    let SourceRef::Asset {
                        asset,
                        stream_index,
                    } = entry.clip.source_ref
                    else {
                        unreachable!()
                    };
                    let mask = sources.layout(asset, stream_index)?;
                    (mask, Some(layout_matrix(mask, target, false)?))
                }
            };
            let source_channels = source_mask.channels();
            let track_index = if metered {
                Some(
                    match track_meters.iter().position(|(id, _)| *id == entry.track) {
                        Some(index) => index,
                        None => {
                            track_meters.push((entry.track, vec![0.0; length * channels]));
                            track_meters.len() - 1
                        }
                    },
                )
            } else {
                None
            };
            let mut chain: Vec<ChainStep> = entry
                .effects
                .iter()
                .map(|effect| ChainStep::new(effect, target))
                .collect();
            // AUDIO-010/FX-008: pitch-preserving entries (retime and the
            // source-stage pitch effect) own one deterministic WSOLA cursor
            // spanning the evaluated range from placement start.
            let wsola_rate = match entry.source {
                Source::Pitch { rate } => rate,
                Source::PitchPreserved => 1.0,
                _ => 0.0,
            };
            let mut wsola = if wsola_rate > 0.0 {
                let SourceRef::Asset {
                    asset,
                    stream_index,
                } = entry.clip.source_ref
                else {
                    unreachable!()
                };
                Some(Wsola::new(
                    &entry.clip,
                    asset,
                    stream_index,
                    source_channels,
                    wsola_rate,
                ))
            } else {
                None
            };
            let mut raw = [0.0_f32; MAX_CHANNELS];
            let mut converted = [0.0_f32; MAX_CHANNELS];
            for sample in start..end {
                let time = Time::new(sample, 48_000)?;
                match &entry.source {
                    Source::Legacy(_) => {
                        let index =
                            usize::try_from(sample - start).map_err(|_| AudioError::Overflow)?;
                        let source = legacy
                            .as_ref()
                            .expect("legacy ChannelBus")
                            .buffer()
                            .frame(index)
                            .ok_or(AudioError::Overflow)?;
                        converted[..channels].copy_from_slice(source);
                    }
                    Source::Resampled => {
                        resample(&entry.clip, sources, sample, &mut raw[..source_channels])?;
                        matrix
                            .as_ref()
                            .expect("layout matrix")
                            .apply(&raw[..source_channels], &mut converted[..channels])?;
                    }
                    Source::PitchPreserved | Source::Pitch { .. } => {
                        wsola.as_mut().expect("wsola cursor").frame(
                            &entry.clip,
                            sources,
                            sample,
                            &mut raw[..source_channels],
                        )?;
                        matrix
                            .as_ref()
                            .expect("layout matrix")
                            .apply(&raw[..source_channels], &mut converted[..channels])?;
                    }
                    Source::ReversedComposition { plan, duration } => {
                        reversed_composition(
                            &entry.clip,
                            plan,
                            *duration,
                            sources,
                            sample,
                            target,
                            &mut converted[..channels],
                        )?;
                    }
                    Source::Generator => {
                        let SourceRef::Generator { generator, .. } = &entry.clip.source_ref else {
                            unreachable!()
                        };
                        raw.fill(0.0);
                        if !hold_silent(&entry.clip, sample)? {
                            // NLE-006: a hold segment freezes the source clock;
                            // the mix emits silence instead of repeated frames.
                            let source_time = local_time(&entry.clip, sample)?;
                            if source_time < Time::ZERO {
                                return Err(invalid("negative Generator source time"));
                            }
                            let value = if generator == AUDIO_GENERATOR_SILENCE {
                                0.0
                            } else {
                                let cycles = source_time.checked_mul(Time::from_integer(440))?;
                                let phase =
                                    cycles.checked_sub(Time::from_integer(cycles.floor()))?;
                                (number(phase) * std::f64::consts::TAU).sin() as f32 * 0.25
                            };
                            raw[..source_channels].fill(value);
                        }
                        matrix
                            .as_ref()
                            .expect("layout matrix")
                            .apply(&raw[..source_channels], &mut converted[..channels])?;
                    }
                }
                if !matches!(entry.source, Source::Legacy(_))
                    && let Some(volume) = &entry.clip.volume
                {
                    let gain = self.gain(volume, local_time(&entry.clip, sample)?)?;
                    for value in &mut converted[..channels] {
                        *value *= gain;
                    }
                }
                // Effects run in authored order for every evaluated sample,
                // including warm-up samples before the request boundary, so
                // DSP state matches a continuous render exactly.
                for step in &mut chain {
                    match step {
                        ChainStep::Gain(property) => {
                            let gain = self.gain(property, time)?;
                            for value in &mut converted[..channels] {
                                *value *= gain;
                            }
                        }
                        ChainStep::Dsp(processor) => processor.process(&mut converted[..channels]),
                    }
                }
                if sample < output.start {
                    continue;
                }
                for (bounds, incoming) in &entry.fades {
                    if bounds.contains(&sample) {
                        let progress =
                            (sample - bounds.start) as f64 / (bounds.end - bounds.start) as f64;
                        let gain = if *incoming { progress } else { 1.0 - progress } as f32;
                        for value in &mut converted[..channels] {
                            *value *= gain;
                        }
                    }
                }
                // GUI-012: constant-power balance on the clip's mixed output,
                // after volume, authored effects and fades — every source
                // kind (including Legacy plans) pans uniformly.
                if let Some(pan) = &entry.clip.pan {
                    if channels < 2 {
                        return Err(unsupported("audio pan requires a stereo output bus"));
                    }
                    let (left, right) = pan_gains(pan)?;
                    converted[0] *= left;
                    converted[1] *= right;
                }
                let index =
                    usize::try_from(sample - output.start).map_err(|_| AudioError::Overflow)?;
                for (channel, value) in converted[..channels].iter().enumerate() {
                    if !value.is_finite() {
                        return Err(AudioError::Overflow);
                    }
                    let sum = &mut frames[index * channels + channel];
                    *sum += value;
                    if !sum.is_finite() {
                        return Err(AudioError::Overflow);
                    }
                    if let Some(track) = track_index {
                        track_meters[track].1[index * channels + channel] += value;
                    }
                }
            }
        }
        let bus = ChannelBus {
            start_sample: output.start,
            buffer: ChannelBuffer::new(target, frames)?,
        };
        let meters = crate::ChannelBusMeters {
            tracks: track_meters
                .iter()
                .map(|(track, buffer)| {
                    let meter = channel_meter(target, buffer)?;
                    Ok(ChannelTrackMeter {
                        track: *track,
                        peak: meter.peak,
                        rms: meter.rms,
                    })
                })
                .collect::<Result<Vec<_>, AudioError>>()?,
            master: channel_meter(target, bus.buffer().samples())?,
        };
        Ok((bus, meters))
    }
}
/// GUI-012: constant-power stereo balance for `kronello.audio.pan` in
/// [-1, 1]; center is -3 dB per channel, hard pan silences the other side.
/// `validate_pan` (model + compile) already confined the source to a
/// Constant scalar, so anything else is a contract violation.
fn pan_gains(property: &Property) -> Result<(f32, f32), AudioError> {
    let PropertySource::Constant(Value::Scalar(value)) = property.source() else {
        return Err(unsupported("audio pan requires a Constant scalar"));
    };
    let pan = value.get().clamp(-1.0, 1.0) as f32;
    let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
    Ok((angle.cos(), angle.sin()))
}
/// NLE-006: a piecewise hold segment has zero source-time advance; resampling
/// emits silence there rather than reading the pinned source frame as audio.
/// Negative parents follow the same first-segment extension `local_time`
/// uses, so a hold leading the placement is silent too.
fn hold_silent(clip: &Clip, sample: i64) -> Result<bool, AudioError> {
    let TimeMap::PiecewiseLinear(map) = &clip.time_map else {
        return Ok(false);
    };
    let parent = Time::new(sample, 48_000)?.checked_sub(clip.timeline_range.start())?;
    Ok(map.is_hold(parent))
}

/// Interpolated source frame in the *source's* layout; `out` has the source
/// channel count. Hold segments emit silence; reverse retime uses the exact
/// reverse neighbor pair.
fn resample(
    clip: &Clip,
    sources: &dyn ChannelSourceReader,
    sample: i64,
    out: &mut [f32],
) -> Result<(), AudioError> {
    let SourceRef::Asset {
        asset,
        stream_index,
    } = clip.source_ref
    else {
        unreachable!()
    };
    if hold_silent(clip, sample)? {
        out.fill(0.0);
        return Ok(());
    }
    let mut position = local_time(clip, sample)?.checked_mul(Time::from_integer(48_000))?;
    if clip.audio_retime == AudioRetimePolicy::ReverseResampleV1 {
        // Exact reverse neighbors: ceil(q)-1 and ceil(q)-2 with reverse fraction
        // ceil(q)-q. This is algebraically the ordinary interpolation at q-1.
        position = position.checked_sub(Time::ONE)?;
    }
    let floor = position.floor();
    let index = usize::try_from(floor).map_err(|_| AudioError::SourceTooShort(asset))?;
    let mut a = [0.0_f32; MAX_CHANNELS];
    sources.read_frame(asset, stream_index, index, &mut a[..out.len()])?;
    let fraction = position.checked_sub(Time::from_integer(floor))?;
    if fraction == Time::ZERO {
        out.copy_from_slice(&a[..out.len()]);
        return Ok(());
    }
    let mut b = [0.0_f32; MAX_CHANNELS];
    sources.read_frame(
        asset,
        stream_index,
        index.checked_add(1).ok_or(AudioError::Overflow)?,
        &mut b[..out.len()],
    )?;
    let fraction = number(fraction);
    for (channel, value) in out.iter_mut().enumerate() {
        *value =
            (f64::from(a[channel]) * (1.0 - fraction) + f64::from(b[channel]) * fraction) as f32;
    }
    if out.iter().any(|v| !v.is_finite()) {
        return Err(AudioError::Overflow);
    }
    Ok(())
}

/// One frame of a reverse-played nested composition, rendered at the bus
/// layout so >2 channel content survives composition nesting (ADR-0124).
fn reversed_composition(
    clip: &Clip,
    plan: &DocumentAudioPlan,
    duration: Time,
    sources: &dyn ChannelSourceReader,
    sample: i64,
    target: ChannelMask,
    out: &mut [f32],
) -> Result<(), AudioError> {
    let position = local_time(clip, sample)?
        .checked_mul(Time::from_integer(48_000))?
        .checked_sub(Time::ONE)?;
    let floor = position.floor();
    let fraction = position.checked_sub(Time::from_integer(floor))?;
    let count = if fraction == Time::ZERO { 1 } else { 2 };
    let start = Time::new(floor, 48_000)?;
    let end = Time::new(
        floor.checked_add(count).ok_or(AudioError::Overflow)?,
        48_000,
    )?;
    if start < Time::ZERO || end > duration {
        return Err(invalid(
            "reverse Composition audio source neighbor outside bounds",
        ));
    }
    let bus = plan.mix_channels(sources, TimeRange::new(start, end)?, target)?;
    let a = bus.buffer().frame(0).ok_or(AudioError::Overflow)?;
    if count == 1 {
        out.copy_from_slice(a);
        return Ok(());
    }
    let b = bus.buffer().frame(1).ok_or(AudioError::Overflow)?;
    let fraction = number(fraction);
    for (channel, value) in out.iter_mut().enumerate() {
        *value =
            (f64::from(a[channel]) * (1.0 - fraction) + f64::from(b[channel]) * fraction) as f32;
    }
    Ok(())
}
