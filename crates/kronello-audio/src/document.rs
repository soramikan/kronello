//! Pure compilation of document audio into ordered, independently scoped placements.
use std::collections::{BTreeMap, BTreeSet};

use kronello_model::*;
use kronello_time::{Time, TimeMap, TimeRange};
use serde::{Deserialize, Serialize};

use crate::{
    AudioClip, AudioError, AudioSources, Bus, ChannelBus, ChannelSourceReader, Gain,
    mix_with_gain_channels, sample_index, sample_range,
};

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum AudioSourceMode {
    Document,
    #[default]
    Explicit,
    Silence,
}
#[derive(Debug, Clone, Copy)]
pub enum AudioTarget {
    Sequence(SequenceId),
    Composition(CompositionId),
}
#[derive(Debug, Clone)]
struct ClipGain {
    property: Property,
    offset: Time,
}
#[derive(Debug, Clone)]
struct Placement {
    clip: AudioClip,
    active: TimeRange,
    gains: Vec<ClipGain>,
}
/// Compilation never decodes assets or consults mutable work. Source origin is
/// kept separate from ancestor clipping so trim cannot re-round sample phase.
#[derive(Debug, Clone, Default)]
pub struct DocumentAudioPlan {
    placements: Vec<Placement>,
    curves: BTreeMap<CurveId, AnimationCurve>,
    advanced: Option<Box<crate::advanced::AdvancedAudioPlan>>,
}
fn invalid(message: impl Into<String>) -> AudioError {
    AudioError::InvalidInput(message.into())
}
fn unsupported(message: impl Into<String>) -> AudioError {
    AudioError::Unsupported(message.into())
}
fn unity_offset(map: &TimeMap) -> Result<Time, AudioError> {
    match map {
        TimeMap::Linear(m) if m.speed() == Time::ONE => Ok(m.offset()),
        _ => Err(unsupported("retimed audio requires AUDIO-004")),
    }
}
fn composition(project: &Project, id: CompositionId) -> Result<&Composition, AudioError> {
    project
        .compositions
        .iter()
        .find_map(|c| match c {
            DocumentObject::Known(c) if c.id == id => Some(c),
            _ => None,
        })
        .ok_or_else(|| unsupported(format!("missing or opaque audio composition {id}")))
}
pub(super) fn has_audio(
    project: &Project,
    id: CompositionId,
    seen: &mut BTreeSet<CompositionId>,
) -> Result<bool, AudioError> {
    if seen.len() >= 64 {
        return Err(invalid("audio recursion budget exceeded"));
    }
    if !seen.insert(id) {
        return Err(invalid("recursive composition audio"));
    }
    let c = composition(project, id)?;
    let mut found = false;
    for n in &c.nodes {
        match &n.kind {
            NodeKind::Media(media) => {
                found |= media_has_audio(project, media.asset, media.stream_index)?
            }
            NodeKind::CompositionInstance(i) => {
                found |= has_audio(project, i.definition_ref, seen)?
            }
            _ => (),
        }
    }
    seen.remove(&id);
    Ok(found)
}
fn shifted(range: TimeRange, offset: Time) -> Result<TimeRange, AudioError> {
    Ok(TimeRange::new(
        range.start().checked_sub(offset)?,
        range.end().checked_sub(offset)?,
    )?)
}
fn validate_asset(project: &Project, id: AssetId, stream: u32) -> Result<(), AudioError> {
    let asset = project
        .assets
        .iter()
        .find_map(|a| match a {
            DocumentObject::Known(a) if a.id == id => Some(a),
            _ => None,
        })
        .ok_or(AudioError::AssetMissing(id))?;
    let _selected = asset
        .streams
        .iter()
        .find(|s| s.index == stream)
        .ok_or_else(|| invalid("audio stream missing"))?;
    if !matches!(asset.kind, AssetKind::Audio | AssetKind::Video) {
        return Err(unsupported("selected Media stream is not audio"));
    }
    Ok(())
}
impl DocumentAudioPlan {
    /// Version 1 retains AUDIO-003; version 2 opts into AUDIO-004 semantics.
    pub fn compile_version(
        project: &Project,
        target: AudioTarget,
        version: u32,
    ) -> Result<Self, AudioError> {
        match version {
            1 => Self::compile(project, target),
            2 if matches!(target, AudioTarget::Composition(_)) => Self::compile(project, target),
            2 => Ok(Self {
                advanced: Some(Box::new(crate::advanced::AdvancedAudioPlan::compile(
                    project, target,
                )?)),
                ..Self::default()
            }),
            _ => Err(unsupported("audio evaluation version")),
        }
    }
    pub fn compile(project: &Project, target: AudioTarget) -> Result<Self, AudioError> {
        Self::compile_impl(project, target, None)
    }
    /// The caller already validated the complete Sequence. Compile one clean
    /// placement without copying unrelated Project resources or revalidating it.
    pub(super) fn compile_isolated_clip(
        project: &Project,
        target: AudioTarget,
        clip: &Clip,
    ) -> Result<Self, AudioError> {
        Self::compile_impl(project, target, Some(clip))
    }
    fn compile_impl(
        project: &Project,
        target: AudioTarget,
        isolated: Option<&Clip>,
    ) -> Result<Self, AudioError> {
        let mut plan = Self::default();
        match target {
            AudioTarget::Composition(id) => {
                let c = composition(project, id)?;
                plan.walk(
                    project,
                    id,
                    InstancePath::root(),
                    Time::ZERO,
                    TimeRange::new(Time::ZERO, c.duration.as_time())?,
                    vec![],
                )?;
            }
            AudioTarget::Sequence(id) => {
                let sequence = project
                    .sequences
                    .iter()
                    .find_map(|s| match s {
                        DocumentObject::Known(s) if s.id == id => Some(s),
                        _ => None,
                    })
                    .ok_or_else(|| unsupported("missing or opaque audio sequence"))?;
                if isolated.is_none() {
                    sequence.validate(project)?;
                }
                for track in &sequence.tracks {
                    if track.muted() {
                        continue;
                    }
                    for clip in &track.clips {
                        if isolated.is_some_and(|selected| selected.id != clip.id) {
                            continue;
                        }
                        let clip = isolated.unwrap_or(clip);
                        // NLE-005: a disabled placement keeps its timeline
                        // occupancy but is inaudible.
                        if !clip.enabled {
                            continue;
                        }
                        let audible = match clip.source_ref {
                            SourceRef::Composition { composition } => {
                                has_audio(project, composition, &mut BTreeSet::new())?
                            }
                            _ => track.kind == TrackKind::Audio,
                        };
                        if !audible {
                            continue;
                        }
                        // Video crossfades blend pixels only; summing both audio
                        // placements at unity would be a silent approximation.
                        if isolated.is_none()
                            && sequence
                                .transitions
                                .iter()
                                .any(|tr| tr.outgoing == clip.id || tr.incoming == clip.id)
                        {
                            return Err(unsupported("audio crossfade requires AUDIO-004"));
                        }
                        if !clip.effects.is_empty() {
                            return Err(unsupported("audio clip effects require AUDIO-004"));
                        }
                        if track.kind == TrackKind::Audio && !clip.properties.is_empty() {
                            return Err(unsupported("audio clip Properties require AUDIO-004"));
                        }
                        if clip.audio_retime != AudioRetimePolicy::Reject {
                            return Err(unsupported("audio_retime requires AUDIO-004 profile"));
                        }
                        let offset = clip
                            .source_in
                            .checked_add(unity_offset(&clip.time_map)?)?
                            .checked_sub(clip.timeline_range.start())?;
                        let gains = clip
                            .volume
                            .as_ref()
                            .map(|p| ClipGain {
                                property: p.as_ref().clone(),
                                offset,
                            })
                            .into_iter()
                            .collect();
                        match clip.source_ref {
                            SourceRef::Asset {
                                asset,
                                stream_index,
                            } => {
                                validate_asset(project, asset, stream_index)?;
                                plan.placements.push(Placement {
                                    clip: AudioClip {
                                        asset,
                                        stream_index,
                                        placement: clip.timeline_range,
                                        source_in: clip.local_time(clip.timeline_range.start())?,
                                        gain: Gain::UNITY,
                                    },
                                    active: clip.timeline_range,
                                    gains,
                                });
                            }
                            SourceRef::Composition { composition } => plan.walk(
                                project,
                                composition,
                                InstancePath::root(),
                                offset,
                                clip.timeline_range,
                                gains,
                            )?,
                            SourceRef::Generator { .. } => {
                                return Err(unsupported("Generator audio requires AUDIO-004"));
                            }
                            SourceRef::Caption { .. } => {
                                // Unreachable: captions only sit on caption
                                // tracks and are already filtered as inaudible.
                                return Err(unsupported(
                                    "caption clips are excluded from audio mixing",
                                ));
                            }
                            SourceRef::Adjustment => {
                                // FX-007: adjustment clips scope a video effect
                                // pass only and never emit audio.
                                return Err(unsupported("adjustment clips produce no audio"));
                            }
                        }
                    }
                }
            }
        }
        if plan.placements.len() > 1024 {
            return Err(invalid("audio placement budget exceeded"));
        }
        for p in &mut plan.placements {
            // Quantize one affine source offset, rather than two independently
            // rounded boundaries. Recompiling after rational trim keeps phase.
            let destination_origin = p.clip.placement.start().checked_sub(p.clip.source_in)?;
            let source_offset = sample_index(destination_origin)?
                .checked_neg()
                .ok_or(AudioError::Overflow)?;
            let source_start = sample_index(p.active.start())?
                .checked_add(source_offset)
                .ok_or(AudioError::Overflow)?;
            if source_start < 0 {
                return Err(AudioError::SourceTooShort(p.clip.asset));
            }
            p.clip.placement = p.active;
            p.clip.source_in = Time::new(source_start, 48_000)?;
            p.clip.validate()?;
            for gain in &p.gains {
                validate_volume(&gain.property).map_err(|e| invalid(e.to_string()))?;
                if let PropertySource::Curve(id) = gain.property.source() {
                    let curve = project
                        .curves
                        .iter()
                        .find_map(|c| match c {
                            DocumentObject::Known(c) if c.id() == *id => Some(c),
                            _ => None,
                        })
                        .ok_or_else(|| invalid("volume curve missing or opaque"))?;
                    if curve.value_type() != ValueType::Scalar || curve.keys().is_empty() {
                        return Err(invalid("volume curve must be a nonempty Scalar curve"));
                    }
                    curve
                        .ensure_supported_version()
                        .map_err(|e| unsupported(e.to_string()))?;
                    plan.curves.entry(*id).or_insert_with(|| curve.clone());
                }
            }
        }
        Ok(plan)
    }
    fn walk(
        &mut self,
        project: &Project,
        id: CompositionId,
        path: InstancePath,
        offset: Time,
        active: TimeRange,
        gains: Vec<ClipGain>,
    ) -> Result<(), AudioError> {
        if path.ids().len() > 64 || self.placements.len() > 1024 {
            return Err(invalid("audio recursion/placement budget exceeded"));
        }
        let c = composition(project, id)?;
        let domain = shifted(TimeRange::new(Time::ZERO, c.duration.as_time())?, offset)?;
        let Some(active) = active.intersection(domain) else {
            return Ok(());
        };
        let mut pending: Vec<_> = c
            .root_nodes
            .iter()
            .rev()
            .map(|n| (*n, active, false))
            .collect();
        let mut visited = BTreeSet::new();
        while let Some((id, parent_active, ancestor_effect)) = pending.pop() {
            if !visited.insert(id) || visited.len() > 100000 {
                return Err(invalid("audio containment cycle/duplicate or node budget"));
            }
            if self.placements.len() >= 1024 {
                return Err(invalid("audio placement budget exceeded"));
            }
            let node = c
                .nodes
                .iter()
                .find(|n| n.id == id)
                .ok_or_else(|| invalid("audio node missing"))?;
            // Disabled containment subtrees leave the evaluated scene (ADR-0061),
            // including their Media audio.
            if !node.enabled {
                continue;
            }
            let node_range = shifted(node.active_range, offset)?;
            let Some(active) = parent_active.intersection(node_range) else {
                continue;
            };
            let effects = ancestor_effect || !node.effects.is_empty();
            for child in node.child_order.iter().rev() {
                pending.push((*child, active, effects));
            }
            match &node.kind {
                NodeKind::Media(media) => {
                    if !media_has_audio(project, media.asset, media.stream_index)? {
                        continue;
                    }
                    if effects {
                        return Err(unsupported("audio node effects require AUDIO-004"));
                    }
                    validate_asset(project, media.asset, media.stream_index)?;
                    let volume = node
                        .properties
                        .iter()
                        .find(|p| p.id() == media.volume)
                        .ok_or_else(|| invalid("Media volume Property missing"))?;
                    validate_volume(volume).map_err(|e| invalid(e.to_string()))?;
                    self.placements.push(Placement {
                        clip: AudioClip {
                            asset: media.asset,
                            stream_index: media.stream_index,
                            placement: node_range,
                            source_in: media
                                .source_in
                                .checked_add(unity_offset(&media.time_map)?)?,
                            gain: Gain::UNITY,
                        },
                        active,
                        gains: gains
                            .iter()
                            .cloned()
                            .chain(std::iter::once(ClipGain {
                                property: volume.clone(),
                                offset,
                            }))
                            .collect(),
                    });
                }
                NodeKind::CompositionInstance(i) => {
                    if !has_audio(project, i.definition_ref, &mut BTreeSet::new())? {
                        continue;
                    }
                    if effects {
                        return Err(unsupported("nested audio effects require AUDIO-004"));
                    }
                    self.walk(
                        project,
                        i.definition_ref,
                        path.child(i.id),
                        offset.checked_add(unity_offset(&i.local_time_map)?)?,
                        active,
                        gains.clone(),
                    )?;
                }
                _ => (),
            }
        }
        Ok(())
    }
    pub fn clips(&self) -> Vec<AudioClip> {
        if let Some(plan) = &self.advanced {
            return plan.clips();
        }
        self.placements.iter().map(|p| p.clip.clone()).collect()
    }
    pub(super) fn audio4_sample_cost(&self) -> Result<u64, AudioError> {
        if self.curves.values().any(|curve| curve.keys().len() > 4096)
            || self
                .curves
                .values()
                .map(|curve| curve.keys().len())
                .sum::<usize>()
                > 65536
        {
            return Err(AudioError::BudgetExceeded("audio Curve keys".into()));
        }
        Ok(self
            .placements
            .iter()
            .map(|p| 1 + p.gains.len() as u64)
            .sum())
    }
    pub(super) fn audio4_curve_keys(&self) -> usize {
        self.curves.values().map(|curve| curve.keys().len()).sum()
    }
    /// Volume Properties and curves are immutable; every sample time is derived
    /// directly from its absolute index. Invocation order is irrelevant.
    pub fn mix(&self, sources: &AudioSources, range: TimeRange) -> Result<Bus, AudioError> {
        self.mix_reader(sources, range)
    }
    /// AUDIO-009: identical mixing plus per-track/master peak-RMS metering.
    /// Per-track levels require the AUDIO-004+ evaluator; the legacy plan
    /// reports only the summed master levels.
    pub fn mix_metered(
        &self,
        sources: &dyn ChannelSourceReader,
        range: TimeRange,
    ) -> Result<(Bus, crate::BusMeters), AudioError> {
        if let Some(plan) = &self.advanced {
            return plan.mix_metered(sources, range);
        }
        let bus = self.mix_reader(sources, range)?;
        let meters = crate::BusMeters {
            tracks: vec![],
            master: crate::stereo_meter(bus.buffer().frames()),
        };
        Ok((bus, meters))
    }
    /// AUDIO-010: identical mixing at an explicit target layout. Sources keep
    /// their decoded channel mask through conversion; LFE is excluded from
    /// fold-down unless the target carries it.
    pub fn mix_channels(
        &self,
        sources: &dyn ChannelSourceReader,
        range: TimeRange,
        target: ChannelMask,
    ) -> Result<ChannelBus, AudioError> {
        Ok(self.mix_channels_metered(sources, range, target)?.0)
    }
    /// Metered variant of [`mix_channels`](Self::mix_channels).
    pub fn mix_channels_metered(
        &self,
        sources: &dyn ChannelSourceReader,
        range: TimeRange,
        target: ChannelMask,
    ) -> Result<(ChannelBus, crate::ChannelBusMeters), AudioError> {
        if let Some(plan) = &self.advanced {
            return plan.mix_channels_metered(sources, range, target);
        }
        let bus = self.mix_channels_impl(sources, range, target)?;
        let meters = crate::ChannelBusMeters {
            tracks: vec![],
            master: crate::channel_meter(target, bus.buffer().samples())?,
        };
        Ok((bus, meters))
    }
    /// Evaluate a bounded Bus without retaining complete source buffers.
    pub fn mix_reader(
        &self,
        sources: &dyn ChannelSourceReader,
        range: TimeRange,
    ) -> Result<Bus, AudioError> {
        if let Some(plan) = &self.advanced {
            return plan.mix(sources, range);
        }
        self.mix_channels_impl(sources, range, ChannelMask::STEREO)?
            .into_stereo_bus()
    }
    fn mix_channels_impl(
        &self,
        sources: &dyn ChannelSourceReader,
        range: TimeRange,
        target: ChannelMask,
    ) -> Result<ChannelBus, AudioError> {
        let active = self
            .placements
            .iter()
            .map(|p| sample_range(p.active))
            .collect::<Result<Vec<_>, _>>()?;
        mix_with_gain_channels(
            &self.clips(),
            sources,
            range,
            &mut |index, sample| {
                if !active[index].contains(&sample) {
                    return Gain::new(0.0);
                }
                let p = &self.placements[index];
                let time = Time::new(sample, 48_000)?;
                let mut gain = 1.0;
                for g in &p.gains {
                    let value = match g.property.source() {
                        PropertySource::Constant(value) => value.clone(),
                        PropertySource::Curve(id) => {
                            let curve = self
                                .curves
                                .get(id)
                                .ok_or_else(|| invalid("volume curve missing or opaque"))?;
                            kronello_animation::sample(curve, time.checked_add(g.offset)?)
                                .map_err(|e| invalid(e.to_string()))?
                        }
                        _ => {
                            return Err(unsupported(
                                "volume expressions require a later audio contract",
                            ));
                        }
                    };
                    gain *= scalar_gain(value)?.linear();
                }
                Gain::new(gain)
            },
            target,
        )
    }
}
pub(super) fn scalar_gain(value: Value) -> Result<Gain, AudioError> {
    let Value::Scalar(value) = value else {
        return Err(invalid("volume must be Scalar Gain"));
    };
    if value.get() < 0.0 || value.get() > f64::from(f32::MAX) {
        return Err(invalid("volume outside Gain range"));
    }
    Gain::new(value.get() as f32)
}

fn media_has_audio(project: &Project, id: AssetId, stream: u32) -> Result<bool, AudioError> {
    let asset = project
        .assets
        .iter()
        .find_map(|a| match a {
            DocumentObject::Known(a) if a.id == id => Some(a),
            _ => None,
        })
        .ok_or(AudioError::AssetMissing(id))?;
    if asset.kind == AssetKind::Image {
        return Ok(false);
    }
    let selected = asset
        .streams
        .iter()
        .find(|s| s.index == stream)
        .ok_or_else(|| invalid("audio stream missing"))?;
    if asset.kind == AssetKind::Video && selected.width.is_some() && selected.height.is_some() {
        return Ok(false);
    }
    if !matches!(asset.kind, AssetKind::Audio | AssetKind::Video) {
        return Err(unsupported("selected Media stream is not audio"));
    }
    Ok(true)
}
