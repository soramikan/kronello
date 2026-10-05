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
pub(crate) fn solid_content(
    color: Color,
    extent: DesignExtent,
) -> Result<crate::SceneContent, RenderError> {
    use std::collections::BTreeMap;
    let ids = [1, 2, 3].map(|id| PropertyId::from_uuid(uuid::Uuid::from_u128(id)));
    let definition = Shape {
        id: ContentId::from_uuid(uuid::Uuid::nil()),
        geometry: ShapeGeometry::Rectangle {
            size: ids[0],
            corner_radius: ids[1],
        },
        fill: Some(Fill {
            color: ids[2],
            rule: FillRule::Nonzero,
            gradient: None,
        }),
        stroke: None,
    };
    let values = BTreeMap::from([
        (
            ids[0],
            Value::Vec2([
                FiniteF64::new(extent.width()).expect("extent"),
                FiniteF64::new(extent.height()).expect("extent"),
            ]),
        ),
        (ids[1], Value::Scalar(FiniteF64::new(0.0).expect("zero"))),
        (ids[2], Value::Color(color)),
    ]);
    Ok(crate::SceneContent::Shape {
        resolved: definition.resolve(&values)?,
        definition,
        values,
    })
}
pub fn lower_sequence(project: &Project, id: SequenceId) -> Result<Composition, RenderError> {
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
    if sequence.transitions.iter().any(|t| t.version != 1) {
        return Err(RenderError::UnsupportedFeature("transition version".into()));
    }
    let mut nodes = vec![];
    let mut end = Time::ZERO;
    for track in &sequence.tracks {
        if track.kind == TrackKind::Audio {
            continue;
        }
        let mut clips: Vec<_> = track.clips.iter().collect();
        clips.sort_by_key(|c| c.timeline_range.start());
        for clip in clips {
            if let SourceRef::Asset { asset, .. } = clip.source_ref
                && let Some(DocumentObject::Known(a)) = project
                    .assets
                    .iter()
                    .find(|a| matches!(a, DocumentObject::Known(a) if a.id == asset))
                && a.kind != AssetKind::Video
            {
                return Err(RenderError::UnsupportedFeature(
                    "sequence image source rendering".into(),
                ));
            }
            if let SourceRef::Generator {
                generator, version, ..
            } = &clip.source_ref
                && (generator != SOLID_GENERATOR_ID || *version != GENERATOR_VERSION)
            {
                return Err(RenderError::UnsupportedFeature(
                    "generator id/version".into(),
                ));
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
                name: None,
                enabled: true,
                id: NodeId::from_uuid(clip.id.as_uuid()),
                kind: if let SourceRef::Composition { composition } = clip.source_ref {
                    NodeKind::CompositionInstance(CompositionInstance {
                        id: CompositionInstanceId::from_uuid(clip.id.as_uuid()),
                        definition_ref: composition,
                        input_bindings: Default::default(),
                        local_time_map,
                        seed: 0,
                    })
                } else {
                    NodeKind::Null
                },
                containment_parent: None,
                transform_parent: None,
                child_order: vec![],
                active_range: clip.timeline_range,
                properties: clip.properties.clone(),
                effects: clip.effects.clone(),
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
