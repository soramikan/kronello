//! AUDIO-004: bounded, immutable, independently addressable sample evaluation.
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use kronello_model::*;
use kronello_time::{Time, TimeMap, TimeRange};

use crate::{
    AudioBuffer, AudioClip, AudioError, AudioSources, AudioTarget, Bus, DocumentAudioPlan, Gain,
    MAX_AUDIO_FRAMES, sample_range,
};

/// Selected by movie profile 3; legacy movie profiles retain evaluator 1.
pub const AUDIO_EVALUATION_VERSION: u32 = 2;
pub const AUDIO_GENERATOR_SILENCE: &str = "kronello.audio.silence";
pub const AUDIO_GENERATOR_TONE: &str = "kronello.audio.tone440";
const MAX_SAMPLE_OPERATIONS: u64 = 100_000_000;
const MAX_MAP_POINTS: usize = 1024;

#[derive(Debug, Clone)]
enum Source {
    Legacy(DocumentAudioPlan),
    Resampled,
    Generator,
}
#[derive(Debug, Clone)]
struct Entry {
    clip: Clip,
    source: Source,
    effects: Vec<Property>,
    fades: Vec<(Range<i64>, bool)>,
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
fn local_time(clip: &Clip, sample: i64) -> Result<Time, AudioError> {
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
    Ok(clip.source_in.checked_add(mapped)?)
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
            for clip in &track.clips {
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
                for effect in &clip.effects {
                    if track.kind != TrackKind::Audio {
                        return Err(unsupported("audio effects require an audio track"));
                    }
                    let definition = effect
                        .definition()
                        .map_err(|_| unsupported("audio effect id / version"))?;
                    let EffectParameters::AudioGain { gain } = definition.parameters else {
                        return Err(unsupported("audio supports kronello.audio.gain v1 only"));
                    };
                    let property = clip
                        .properties
                        .iter()
                        .find(|p| p.id() == gain)
                        .ok_or_else(|| invalid("audio effect gain Property missing"))?;
                    plan.capture_property(project, property)?;
                    effects.push(property.clone());
                }
                // Every authored audio Property must belong to the closed effect contract.
                if track.kind == TrackKind::Audio
                    && clip
                        .properties
                        .iter()
                        .any(|p| !effects.iter().any(|e| e.id() == p.id()))
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
                    SourceRef::Asset { .. }
                        if clip.audio_retime == AudioRetimePolicy::ResampleV1 =>
                    {
                        Source::Resampled
                    }
                    _ => {
                        // Legacy recursive placement retains its exact affine sample phase.
                        // Nested/composition retime and node effects remain explicit errors.
                        let mut clean = clip.clone();
                        clean.effects.clear();
                        clean.properties.clear();
                        clean.links.clear();
                        Source::Legacy(DocumentAudioPlan::compile_isolated_clip(
                            project, target, &clean,
                        )?)
                    }
                };
                if !matches!(source, Source::Legacy(_))
                    && matches!(&clip.time_map, TimeMap::PiecewiseLinear(m) if m.points()[0].parent != Time::ZERO)
                {
                    return Err(unsupported("audio piecewise map must start at parent zero"));
                }
                if let Source::Legacy(legacy) = &source {
                    legacy.audio4_sample_cost()?;
                    legacy_curve_keys += legacy.audio4_curve_keys();
                }
                if !matches!(source, Source::Legacy(_))
                    && let Some(property) = &clip.volume
                {
                    plan.capture_property(project, property)?;
                }
                flattened_count += match &source {
                    Source::Legacy(p) => p.clips().len(),
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
                Source::Legacy(plan) => plan.clips(),
                Source::Resampled => {
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
    pub(crate) fn mix(&self, sources: &AudioSources, range: TimeRange) -> Result<Bus, AudioError> {
        let output = sample_range(range)?;
        let length = output
            .end
            .checked_sub(output.start)
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| *n <= MAX_AUDIO_FRAMES)
            .ok_or_else(|| budget("audio Bus frames"))?;
        let mut operations = 0_u64;
        for entry in &self.entries {
            let placement = sample_range(entry.clip.timeline_range)?;
            let samples = output
                .end
                .min(placement.end)
                .saturating_sub(output.start.max(placement.start))
                .max(0) as u64;
            let cost = 1
                + entry.effects.len() as u64
                + entry.fades.len() as u64
                + match &entry.source {
                    Source::Legacy(p) => p.audio4_sample_cost()?,
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
                // Charge that work before any output allocation.
                operations = operations
                    .checked_add((length as u64) * 2)
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
            if matches!(entry.source, Source::Resampled) {
                let SourceRef::Asset {
                    asset,
                    stream_index,
                } = entry.clip.source_ref
                else {
                    unreachable!()
                };
                if !sources.contains_key(&(asset, stream_index)) {
                    return Err(AudioError::AssetMissing(asset));
                }
                let placement = sample_range(entry.clip.timeline_range)?;
                if !placement.is_empty() {
                    resample(&entry.clip, sources, placement.start)?;
                    resample(&entry.clip, sources, placement.end - 1)?;
                }
            }
        }
        let mut frames = vec![[0.0; 2]; length];
        for entry in &self.entries {
            let legacy = match &entry.source {
                Source::Legacy(p) => Some(p.mix(sources, range)?),
                _ => None,
            };
            let placement = sample_range(entry.clip.timeline_range)?;
            for sample in output.start.max(placement.start)..output.end.min(placement.end) {
                let index =
                    usize::try_from(sample - output.start).map_err(|_| AudioError::Overflow)?;
                let time = Time::new(sample, 48_000)?;
                let mut frame = match &entry.source {
                    Source::Legacy(_) => {
                        legacy.as_ref().expect("legacy Bus").buffer().frames()[index]
                    }
                    Source::Resampled => resample(&entry.clip, sources, sample)?,
                    Source::Generator => {
                        let SourceRef::Generator { generator, .. } = &entry.clip.source_ref else {
                            unreachable!()
                        };
                        let source_time = local_time(&entry.clip, sample)?;
                        if source_time < Time::ZERO {
                            return Err(invalid("negative Generator source time"));
                        }
                        let value = if generator == AUDIO_GENERATOR_SILENCE {
                            0.0
                        } else {
                            let cycles = source_time.checked_mul(Time::from_integer(440))?;
                            let phase = cycles.checked_sub(Time::from_integer(cycles.floor()))?;
                            (number(phase) * std::f64::consts::TAU).sin() as f32 * 0.25
                        };
                        [value; 2]
                    }
                };
                if !matches!(entry.source, Source::Legacy(_))
                    && let Some(volume) = &entry.clip.volume
                {
                    let gain = self.gain(volume, local_time(&entry.clip, sample)?)?;
                    frame = frame.map(|v| v * gain);
                }
                // Ordered pointwise operations have no history or mutable state.
                for effect in &entry.effects {
                    let gain = self.gain(effect, time)?;
                    frame = frame.map(|v| v * gain);
                }
                for (bounds, incoming) in &entry.fades {
                    if bounds.contains(&sample) {
                        let progress =
                            (sample - bounds.start) as f64 / (bounds.end - bounds.start) as f64;
                        let gain = if *incoming { progress } else { 1.0 - progress } as f32;
                        frame = frame.map(|v| v * gain);
                    }
                }
                for channel in 0..2 {
                    if !frame[channel].is_finite() {
                        return Err(AudioError::Overflow);
                    }
                    frames[index][channel] += frame[channel];
                    if !frames[index][channel].is_finite() {
                        return Err(AudioError::Overflow);
                    }
                }
            }
        }
        Ok(Bus {
            start_sample: output.start,
            buffer: AudioBuffer::new(frames)?,
        })
    }
}
fn resample(clip: &Clip, sources: &AudioSources, sample: i64) -> Result<[f32; 2], AudioError> {
    let SourceRef::Asset {
        asset,
        stream_index,
    } = clip.source_ref
    else {
        unreachable!()
    };
    let source = sources
        .get(&(asset, stream_index))
        .ok_or(AudioError::AssetMissing(asset))?;
    let position = local_time(clip, sample)?.checked_mul(Time::from_integer(48_000))?;
    let floor = position.floor();
    let index = usize::try_from(floor).map_err(|_| AudioError::SourceTooShort(asset))?;
    let a = *source
        .frames()
        .get(index)
        .ok_or(AudioError::SourceTooShort(asset))?;
    let fraction = position.checked_sub(Time::from_integer(floor))?;
    if fraction == Time::ZERO {
        return Ok(a);
    }
    let b = *source
        .frames()
        .get(index.checked_add(1).ok_or(AudioError::Overflow)?)
        .ok_or(AudioError::SourceTooShort(asset))?;
    let fraction = number(fraction);
    let result = std::array::from_fn(|channel| {
        (f64::from(a[channel]) * (1.0 - fraction) + f64::from(b[channel]) * fraction) as f32
    });
    if result.iter().any(|v| !v.is_finite()) {
        return Err(AudioError::Overflow);
    }
    Ok(result)
}
