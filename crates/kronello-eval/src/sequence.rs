//! Stateless resolution of timeline placements into independent scenes.
use crate::{DependencyGraph, EvaluatedScene, EvaluationError, EvaluationSnapshot};
use kronello_model::{ClipId, Sequence, SequenceError, SourceRef, TrackId, TrackKind};
use kronello_time::Time;

#[derive(Debug, Clone, PartialEq)]
pub struct EvaluatedClip {
    pub track: TrackId,
    pub clip: ClipId,
    pub local_time: Time,
    pub scene: EvaluatedScene,
}
#[derive(Debug, thiserror::Error)]
pub enum SequenceEvaluationError {
    #[error(transparent)]
    Sequence(#[from] SequenceError),
    #[error(transparent)]
    Evaluation(#[from] EvaluationError),
}
/// Returns active video placements in bottom-to-top authored track order.
/// Clip identity scopes each scene; its nested instance paths remain unchanged.
pub fn evaluate_sequence(
    snapshot: &EvaluationSnapshot<'_>,
    sequence: &Sequence,
    time: Time,
) -> Result<Vec<EvaluatedClip>, SequenceEvaluationError> {
    let mut result = vec![];
    for track in &sequence.tracks {
        if track.kind == TrackKind::Audio {
            continue;
        }
        // Disabled clips still occupy their range on the track but contribute
        // nothing to evaluation, so they cannot shadow an enabled clip or
        // trip the single-active-clip rule.
        let active: Vec<_> = track
            .clips
            .iter()
            .filter(|c| c.enabled && c.timeline_range.contains(time))
            .collect();
        if active.len() > 1 {
            return Err(SequenceError::Overlap(track.id).into());
        }
        for clip in active {
            let SourceRef::Composition { composition } = clip.source_ref else {
                return Err(
                    SequenceError::Unsupported("non-composition video evaluation".into()).into(),
                );
            };
            let local_time = clip.local_time(time)?;
            let graph = DependencyGraph::compile(
                EvaluationSnapshot {
                    expressions: snapshot.expressions,
                    compositions: snapshot.compositions,
                    curves: snapshot.curves,
                    registry: snapshot.registry,
                    reference_bindings: snapshot.reference_bindings,
                    dependencies: snapshot.dependencies,
                    working_space: sequence.working_space,
                },
                composition,
            )?;
            result.push(EvaluatedClip {
                track: track.id,
                clip: clip.id,
                local_time,
                scene: graph.evaluate_scene(local_time)?,
            });
        }
    }
    Ok(result)
}
