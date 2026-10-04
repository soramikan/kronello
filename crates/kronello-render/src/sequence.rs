//! Timeline lowering uses authored UUIDs and the existing composition renderer.
use crate::RenderError;
use kronello_model::*;
use kronello_time::{Duration, Time, TimeMap, TimeMapPoint};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RenderTarget {
    Composition { composition: CompositionId },
    Sequence { sequence: SequenceId },
}
impl From<CompositionId> for RenderTarget {
    fn from(composition: CompositionId) -> Self {
        Self::Composition { composition }
    }
}
pub(crate) fn lower_sequence(
    project: &Project,
    id: SequenceId,
) -> Result<Composition, RenderError> {
    if project
        .sequences
        .iter()
        .any(|s| matches!(s, DocumentObject::Opaque(s) if s.id == id.as_uuid()))
    {
        return Err(RenderError::UnsupportedFeature("opaque sequence".into()));
    }
    let sequence = project
        .sequences
        .iter()
        .find_map(|s| match s {
            DocumentObject::Known(s) if s.id == id => Some(s),
            _ => None,
        })
        .ok_or_else(|| RenderError::Sequence(SequenceError::MissingSource(id.to_string())))?;
    sequence.validate(project)?;
    let mut nodes = vec![];
    let mut end = Time::ZERO;
    for track in &sequence.tracks {
        if track.kind == TrackKind::Audio {
            continue;
        }
        for clip in &track.clips {
            let SourceRef::Composition { composition } = clip.source_ref else {
                return Err(RenderError::UnsupportedFeature(
                    "asset/generator sequence video: INTEGRATION-001".into(),
                ));
            };
            if !clip.effects.is_empty() {
                return Err(RenderError::UnsupportedFeature("clip effects".into()));
            }
            let local_time_map = match &clip.time_map {
                TimeMap::Linear(m) => TimeMap::linear(
                    clip.source_in
                        .checked_add(m.offset())?
                        .checked_sub(clip.timeline_range.start().checked_mul(m.speed())?)?,
                    m.speed(),
                )?,
                TimeMap::PiecewiseLinear(m) => TimeMap::piecewise_linear(
                    m.points()
                        .iter()
                        .map(|p| {
                            Ok(TimeMapPoint {
                                parent: p.parent.checked_add(clip.timeline_range.start())?,
                                local: p.local.checked_add(clip.source_in)?,
                            })
                        })
                        .collect::<Result<Vec<_>, kronello_time::TimeError>>()?,
                )?,
                _ => return Err(RenderError::UnsupportedFeature("time map".into())),
            };
            end = end.max(clip.timeline_range.end());
            nodes.push(SceneNode {
                id: NodeId::from_uuid(clip.id.as_uuid()),
                kind: NodeKind::CompositionInstance(CompositionInstance {
                    id: CompositionInstanceId::from_uuid(clip.id.as_uuid()),
                    definition_ref: composition,
                    input_bindings: Default::default(),
                    local_time_map,
                    seed: 0,
                }),
                containment_parent: None,
                transform_parent: None,
                child_order: vec![],
                active_range: clip.timeline_range,
                properties: vec![],
                effects: vec![],
            });
        }
    }
    Ok(Composition {
        id: CompositionId::from_uuid(id.as_uuid()),
        duration: Duration::new(end)?,
        design_extent: sequence.extent,
        edit_rate: sequence.frame_rate,
        root_nodes: nodes.iter().map(|n| n.id).collect(),
        nodes,
        properties: vec![],
    })
}
