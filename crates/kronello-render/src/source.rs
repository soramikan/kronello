//! GUI-011 source-monitor preview lowering (ADR-0128). A `RenderTarget::Source`
//! renders one document source — a bin asset stream or the resolved angle of a
//! multicam group — through the ordinary fixed-snapshot scene path by lowering
//! the source to a synthetic single-node `Composition`. Monitor time is the
//! clip-local source domain: media PTS for asset/multicam sources (multicam
//! time maps through the active angle's `sync_offset`), composition-local for
//! composition sources. The synthetic root is rebuilt from the frozen project
//! on every evaluation, exactly like a lowered sequence root.
use kronello_model::*;
use kronello_time::{Duration, FrameRate, Rational, Time, TimeMap, TimeRange};
use serde::{Deserialize, Serialize};

use crate::RenderError;

/// Fixed synthetic identity for the lowered source root. It is addressed
/// through `RenderSnapshot.source`, never through `project.compositions`, so
/// authored content cannot reference it.
pub(crate) const SOURCE_ROOT_UUID: uuid::Uuid =
    uuid::Uuid::from_u128(0x6b72_6f6e_736f_7572_6365_0000_0000_0001_u128);
pub(crate) const SOURCE_ROOT_ID: CompositionId = CompositionId::from_uuid(SOURCE_ROOT_UUID);
const SOURCE_NODE_UUID: uuid::Uuid =
    uuid::Uuid::from_u128(0x6b72_6f6e_736f_7572_6365_0000_0000_0002_u128);
const SOURCE_VOLUME_PROPERTY_UUID: uuid::Uuid =
    uuid::Uuid::from_u128(0x6b72_6f6e_736f_7572_6365_0000_0000_0003_u128);
/// Sources with no locked duration preview across one day of source time.
const UNBOUNDED_SOURCE_DURATION: i64 = 86_400;

/// Previewable document source for `RenderTarget::Source` (GUI-011,
/// ADR-0128). Unlike `SourceRef`, only kinds with a fixed media or
/// composition identity are representable — bin assets, multicam angles, and
/// compositions — so the value stays `Copy + Eq` like the rest of
/// `RenderTarget`. Generator, caption, and adjustment clips have no monitor
/// preview and are rejected at the conversion boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourcePreviewRef {
    Composition {
        composition: CompositionId,
    },
    Asset {
        asset: AssetId,
        stream_index: u32,
    },
    Multicam {
        multicam: MulticamId,
        angle: AngleId,
    },
}
impl SourcePreviewRef {
    /// Narrow an arbitrary clip/source reference to its previewable form.
    /// Returns `None` for generator, caption, and adjustment sources, which
    /// have no monitor preview.
    pub fn from_source_ref(source: &SourceRef) -> Option<Self> {
        match *source {
            SourceRef::Composition { composition } => Some(Self::Composition { composition }),
            SourceRef::Asset {
                asset,
                stream_index,
            } => Some(Self::Asset {
                asset,
                stream_index,
            }),
            SourceRef::Multicam { multicam, angle } => Some(Self::Multicam { multicam, angle }),
            _ => None,
        }
    }
    /// The equivalent `SourceRef` for resolution against the document.
    pub fn source_ref(self) -> SourceRef {
        match self {
            Self::Composition { composition } => SourceRef::Composition { composition },
            Self::Asset {
                asset,
                stream_index,
            } => SourceRef::Asset {
                asset,
                stream_index,
            },
            Self::Multicam { multicam, angle } => SourceRef::Multicam { multicam, angle },
        }
    }
}
/// Resolved picture/audio decode identity for one source. `stream_index` is
/// the picture stream the video path samples at `time + offset`;
/// `audio_stream` is the stream monitor audio decodes, shifted by the same
/// absolute offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedSource {
    pub asset: AssetId,
    pub stream_index: u32,
    pub audio_stream: Option<u32>,
    /// Absolute media-time shift: `media_time = source_time + offset`.
    pub offset: Time,
}
fn asset_stream(asset: &Asset, index: u32) -> Result<&StreamMetadata, RenderError> {
    asset
        .streams
        .iter()
        .find(|s| s.index == index)
        .ok_or_else(|| RenderError::Backend {
            code: "SOURCE_MISSING",
            message: format!("asset stream {index}"),
        })
}
fn known_asset(project: &Project, id: AssetId) -> Result<&Asset, RenderError> {
    for asset in &project.assets {
        match asset {
            DocumentObject::Known(a) if a.id == id => return Ok(a),
            DocumentObject::Opaque(a) if a.id == id.as_uuid() => {
                return Err(RenderError::UnsupportedFeature("opaque asset".into()));
            }
            _ => (),
        }
    }
    Err(RenderError::Backend {
        code: "ASSET_MISSING",
        message: id.to_string(),
    })
}
/// Resolve an asset or multicam source reference to its decode stream and
/// absolute offset. `audio_stream` selects the stream's own index when it
/// carries audio, otherwise the asset's first audio stream.
pub fn resolve_source(
    project: &Project,
    source: &SourceRef,
) -> Result<ResolvedSource, RenderError> {
    let (asset_id, stream_index, offset) = match source {
        SourceRef::Asset {
            asset,
            stream_index,
        } => (*asset, *stream_index, Time::ZERO),
        SourceRef::Multicam { multicam, angle } => {
            let group = project
                .multicams
                .iter()
                .find(|group| group.id == *multicam)
                .ok_or_else(|| RenderError::Backend {
                    code: "SOURCE_MISSING",
                    message: format!("multicam {multicam}"),
                })?;
            let angle = group.angle(*angle).ok_or_else(|| RenderError::Backend {
                code: "SOURCE_MISSING",
                message: format!("multicam angle {angle}"),
            })?;
            (angle.asset, angle.stream_index, angle.sync_offset)
        }
        _ => {
            return Err(RenderError::UnsupportedFeature(
                "source preview requires an asset or multicam source".into(),
            ));
        }
    };
    let asset = known_asset(project, asset_id)?;
    let stream = asset_stream(asset, stream_index)?;
    // The selected stream itself may carry audio (audio-only asset preview);
    // otherwise the monitor plays the asset's first audio stream.
    let audio_stream = if stream.width.is_none() && stream.height.is_none() {
        Some(stream_index)
    } else {
        asset
            .streams
            .iter()
            .find(|s| s.width.is_none() && s.height.is_none())
            .map(|s| s.index)
    };
    Ok(ResolvedSource {
        asset: asset_id,
        stream_index,
        audio_stream,
        offset,
    })
}
fn source_volume() -> Result<Property, RenderError> {
    let registry = SchemaRegistry::with_builtin();
    let key = SchemaKey::new("kronello.audio.volume")
        .map_err(|e| RenderError::InvalidInput(e.to_string()))?;
    let descriptor = registry
        .lookup(&key)
        .map_err(|e| RenderError::InvalidInput(e.to_string()))?;
    Property::new(
        PropertyId::from_uuid(SOURCE_VOLUME_PROPERTY_UUID),
        DescriptorRef::new(descriptor),
        PropertySource::Constant(Value::Scalar(FiniteF64::new(1.0).expect("unity"))),
        vec![],
        &registry,
    )
    .map_err(|e| RenderError::InvalidInput(e.to_string()))
}
/// Lower one previewable source into a synthetic composition containing a
/// single Media node. Monitor time zero maps to `offset` in the stream's
/// absolute PTS domain; the node stays inactive before the stream start so
/// out-of-range scrubs produce an empty frame rather than a decode error.
pub fn lower_source(
    project: &Project,
    source: &SourcePreviewRef,
) -> Result<Composition, RenderError> {
    let resolved = resolve_source(project, &source.source_ref())?;
    let asset = known_asset(project, resolved.asset)?;
    let stream = asset_stream(asset, resolved.stream_index)?;
    if asset.kind == AssetKind::Data {
        return Err(RenderError::UnsupportedFeature(
            "data assets have no source preview".into(),
        ));
    }
    let start = stream.start_time.unwrap_or(Time::ZERO);
    let duration = stream
        .duration
        .unwrap_or_else(|| Rational::new(UNBOUNDED_SOURCE_DURATION, 1).expect("bounded fallback"));
    let active_start = (start.checked_sub(resolved.offset)?).max(Time::ZERO);
    let active_end = start
        .checked_add(duration)?
        .checked_sub(resolved.offset)?
        .max(active_start);
    let extent = match (stream.width, stream.height) {
        (Some(w), Some(h)) if w > 0 && h > 0 => DesignExtent::new(f64::from(w), f64::from(h))
            .map_err(|e| RenderError::InvalidInput(e.to_string()))?,
        _ => DesignExtent::new(1920.0, 1080.0)
            .map_err(|e| RenderError::InvalidInput(e.to_string()))?,
    };
    let node_id = NodeId::from_uuid(SOURCE_NODE_UUID);
    let node = SceneNode {
        tags: Default::default(),
        name: Some("Source".into()),
        enabled: true,
        effects: vec![],
        id: node_id,
        kind: NodeKind::Media(MediaNode {
            asset: resolved.asset,
            stream_index: resolved.stream_index,
            source_in: resolved.offset.checked_add(active_start)?,
            time_map: TimeMap::linear(Time::ZERO, Rational::ONE)?,
            volume: PropertyId::from_uuid(SOURCE_VOLUME_PROPERTY_UUID),
        }),
        containment_parent: None,
        transform_parent: None,
        child_order: vec![],
        active_range: TimeRange::new(active_start, active_end)?,
        properties: vec![source_volume()?],
    };
    Ok(Composition {
        id: SOURCE_ROOT_ID,
        duration: Duration::new(active_end)?,
        design_extent: extent,
        edit_rate: FrameRate::new(24, 1)?,
        root_nodes: vec![node_id],
        nodes: vec![node],
        properties: vec![],
    })
}
