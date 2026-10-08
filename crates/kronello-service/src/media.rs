//! MEDIA-001 operations, shared by all transports. Filesystem/native work is
//! performed by media; project persistence remains in the shared store service.
use crate::{AssetAvailability, ProjectInfo, ServiceError, info, open_existing, parse_revision};
use kronello_media::{CollectedProject, MediaRuntime, collect_project, relink_asset};
use kronello_model::{AssetId, DocumentObject};
use kronello_store::{OpenOptions, ProjectStore};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// COLOR-003 `.cube` import request (ADR-0113). The caller-allocated asset id
/// names the record; the content hash is computed from file bytes here and
/// recorded as `content_hash`, never trusted from the request.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LutImportRequest {
    pub project: PathBuf,
    pub base_revision: String,
    pub session_id: uuid::Uuid,
    pub idempotency_key: String,
    /// Local `.cube` locator; stored relative to the project directory when it
    /// lives inside it, otherwise absolute (the relink/search convention).
    pub path: PathBuf,
    /// Id of the `AssetKind::Data` record the import upserts.
    pub asset: AssetId,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RelinkRequest {
    pub project: PathBuf,
    pub base_revision: String,
    pub asset: AssetId,
    pub search_directory: PathBuf,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CollectRequest {
    pub project: PathBuf,
    pub output_directory: PathBuf,
}
impl From<kronello_media::MediaError> for ServiceError {
    fn from(error: kronello_media::MediaError) -> Self {
        Self::new(error.code(), error.to_string())
    }
}
/// FLOW-002 project-wide media browser query (ADR-0129). Reports every asset
/// with the same cheap locator probe `sequence.query` uses — present files are
/// unverified until a hash-checked `resolve_asset` runs — plus the persisted
/// bin list. Ordered by document position, never by wall clock.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaQueryRequest {
    pub project: PathBuf,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaAssetEntry {
    pub asset: AssetId,
    /// The full asset record for known-version objects. Opaque assets carry
    /// only their identity plus an `UNSUPPORTED_SCHEMA_VERSION` status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<kronello_model::Asset>,
    pub availability: AssetAvailability,
    pub size_bytes: Option<u64>,
    pub error: Option<ServiceError>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaQueryResult {
    pub revision: String,
    /// Every project asset, in document order.
    pub assets: Vec<MediaAssetEntry>,
    /// Persisted bins (ADR-0129); membership references `assets` ids.
    pub bins: Vec<kronello_model::Bin>,
}
/// FLOW-002 fixed-snapshot thumbnail (ADR-0129). The decoded source frame is
/// addressed by an exact rational time and scaled with the deterministic
/// integer `scale_rgba8` path — no clock, GPU or random input participates.
/// The pixels are transport data only; caches live in the caller, never in
/// the document.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssetThumbnailRequest {
    pub project: PathBuf,
    pub asset: AssetId,
    /// Source presentation time for video; absent decodes the origin frame.
    /// Ignored for still images.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<kronello_time::Time>,
    /// Longest edge of the output thumbnail in pixels (16..=1024).
    pub max_size: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssetThumbnailResult {
    pub asset: AssetId,
    /// Source frame timestamp actually covered by the decoded video frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pts: Option<kronello_time::Time>,
    pub width: u32,
    pub height: u32,
    /// Straight opaque RGBA8, row-major `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}
pub(crate) fn capabilities() -> Result<crate::MediaCapabilities, ServiceError> {
    Ok(MediaRuntime::load()?.capabilities().clone().into())
}
pub(crate) fn relink(request: RelinkRequest) -> Result<ProjectInfo, ServiceError> {
    let revision = parse_revision(&request.base_revision)?;
    let mut store = open_existing(&request.project)?;
    let mut document = store.snapshot()?.document;
    document
        .ensure_editable()
        .map_err(|e| ServiceError::new("UNSUPPORTED_FEATURE", e.to_string()))?;
    let asset = document
        .assets
        .iter_mut()
        .find_map(|object| match object {
            DocumentObject::Known(a) if a.id == request.asset => Some(a),
            _ => None,
        })
        .ok_or_else(|| ServiceError::new("ASSET_MISSING", "asset ID not present"))?;
    *asset = relink_asset(asset, &request.project, &request.search_directory)?;
    store.import_json(
        revision,
        uuid::Uuid::new_v4(),
        &serde_json::to_string(&document)?,
    )?;
    let result = info(&store)?;
    store.close()?;
    Ok(result)
}
/// `lut.import` is an edit operation: the file is parsed and hash-verified
/// once, then persisted as an external `AssetKind::Data` record through the
/// shared plan/apply path so undo, revision checks and idempotency apply.
/// The document never stores expanded lattice bytes.
pub(crate) fn lut_import(request: LutImportRequest) -> Result<kronello_store::Event, ServiceError> {
    let bytes = std::fs::read(&request.path).map_err(|e| {
        ServiceError::new(
            if e.kind() == std::io::ErrorKind::NotFound {
                "LUT_MISSING"
            } else {
                "IO_ERROR"
            },
            e.to_string(),
        )
    })?;
    let lut = kronello_model::CubeLut::parse(&bytes)
        .map_err(|e| ServiceError::new(e.code(), e.to_string()))?;
    lut.validate_document_size()
        .map_err(|e| ServiceError::new(e.code(), e.to_string()))?;
    let canonical = request
        .path
        .canonicalize()
        .map_err(|e| ServiceError::new("IO_ERROR", e.to_string()))?;
    let hash = kronello_media::content_hash(&canonical)?;
    let base = request
        .project
        .parent()
        .unwrap_or(Path::new("."))
        .canonicalize()
        .map_err(|e| ServiceError::new("IO_ERROR", e.to_string()))?;
    let relative = canonical
        .strip_prefix(&base)
        .ok()
        .map(|p| p.to_string_lossy().into_owned());
    let asset = kronello_model::Asset {
        id: request.asset,
        content_hash: hash,
        kind: kronello_model::AssetKind::Data,
        streams: vec![],
        locator: kronello_model::AssetLocator {
            relative,
            absolute: Some(canonical.to_string_lossy().into_owned()),
        },
    };
    asset
        .validate()
        .map_err(|e| ServiceError::new("INVALID_DOCUMENT", e.to_string()))?;
    let commands = vec![crate::EditCommand::AssetSet { asset }];
    let plan = crate::edit::plan(crate::PlanRequest {
        project: request.project.clone(),
        base_revision: request.base_revision.clone(),
        commands: commands.clone(),
    })?;
    crate::edit::apply(crate::EditApplyRequest {
        project: request.project,
        base_revision: request.base_revision,
        session_id: request.session_id,
        idempotency_key: request.idempotency_key,
        plan_hash: plan.plan_hash,
        commands,
    })
}
pub(crate) fn collect(request: CollectRequest) -> Result<CollectedProject, ServiceError> {
    let store = open_existing(&request.project)?;
    let document = store.snapshot()?.document;
    store.close()?;
    collect_project(
        &document,
        &request.project,
        &request.output_directory,
        |path, document| {
            let mut store = ProjectStore::open(path, OpenOptions::default())?;
            store.import_json(0, uuid::Uuid::new_v4(), &serde_json::to_string(document)?)?;
            store.close()?;
            Ok(())
        },
    )
}
/// FLOW-002: every asset plus bins, with availability probed exactly like
/// `sequence.query` so GUI, CLI and MCP agree on the offline badge.
pub(crate) fn media_query(request: MediaQueryRequest) -> Result<MediaQueryResult, ServiceError> {
    let store = open_existing(&request.project)?;
    let snapshot = store.snapshot()?;
    store.close()?;
    let document = snapshot.document;
    let assets = document
        .assets
        .iter()
        .map(|object| match object {
            DocumentObject::Known(asset) => {
                let (availability, size_bytes, error) =
                    match kronello_media::locate_asset(asset, &request.project) {
                        Ok(located) => (
                            AssetAvailability::PresentUnverified,
                            Some(located.size_bytes),
                            None,
                        ),
                        Err(error) => (
                            if error.code() == "ASSET_MISSING" {
                                AssetAvailability::Missing
                            } else {
                                AssetAvailability::Error
                            },
                            None,
                            Some(ServiceError::new(error.code(), error.to_string())),
                        ),
                    };
                MediaAssetEntry {
                    asset: asset.id,
                    detail: Some(asset.clone()),
                    availability,
                    size_bytes,
                    error,
                }
            }
            DocumentObject::Opaque(object) => MediaAssetEntry {
                asset: AssetId::from_uuid(object.id),
                detail: None,
                availability: AssetAvailability::Error,
                size_bytes: None,
                error: Some(ServiceError::new(
                    "UNSUPPORTED_SCHEMA_VERSION",
                    "opaque asset from a newer document version",
                )),
            },
        })
        .collect();
    Ok(MediaQueryResult {
        revision: snapshot.revision.to_string(),
        assets,
        bins: document.bins,
    })
}
/// Premultiplied linear floats to straight opaque sRGB8 for display.
/// Deterministic: fixed OETF, round-half-up, no clock or device input.
fn linear_premultiplied_to_srgb8(pixels: &[[f32; 4]]) -> Vec<u8> {
    let oetf = |v: f32| {
        let v = v.clamp(0.0, 1.0);
        if v <= 0.003_130_8 {
            12.92 * v
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        }
    };
    let mut rgba = Vec::with_capacity(pixels.len() * 4);
    for &[r, g, b, a] in pixels {
        let alpha = a.clamp(0.0, 1.0);
        let straight = |c: f32| if alpha > 0.0 { c / alpha } else { 0.0 };
        rgba.push((oetf(straight(r)) * 255.0).round() as u8);
        rgba.push((oetf(straight(g)) * 255.0).round() as u8);
        rgba.push((oetf(straight(b)) * 255.0).round() as u8);
        rgba.push((alpha * 255.0).round() as u8);
    }
    rgba
}
/// Aspect-preserving fit inside a `max_size` square; integer math only.
fn fit_extent(width: u32, height: u32, max_size: u32) -> (u32, u32) {
    let (w, h, m) = (u64::from(width), u64::from(height), u64::from(max_size));
    if w >= h {
        (max_size, ((h * m + w / 2) / w).max(1) as u32)
    } else {
        (((w * m + h / 2) / h).max(1) as u32, max_size)
    }
}
/// FLOW-002 fixed-time thumbnail. Only image and video assets have frames;
/// everything else fails closed as UNSUPPORTED_FEATURE.
pub(crate) fn thumbnail(
    request: AssetThumbnailRequest,
) -> Result<AssetThumbnailResult, ServiceError> {
    if !(16..=1024).contains(&request.max_size) {
        return Err(ServiceError::invalid("max_size must be 16..=1024"));
    }
    let store = open_existing(&request.project)?;
    let document = store.snapshot()?.document;
    store.close()?;
    let asset = document
        .assets
        .iter()
        .find_map(|object| match object {
            DocumentObject::Known(asset) if asset.id == request.asset => Some(asset.clone()),
            _ => None,
        })
        .ok_or_else(|| ServiceError::new("ASSET_MISSING", "asset ID not present"))?;
    let (pts, width, height, rgba) = match asset.kind {
        kronello_model::AssetKind::Video => {
            let path = kronello_media::resolve_asset(&asset, &request.project)?;
            let runtime = MediaRuntime::load()?;
            let mut decoder = runtime.open_video(&path)?;
            let frame = decoder.rgba_at(request.time.unwrap_or(kronello_time::Rational::ZERO))?;
            (Some(frame.pts), frame.width, frame.height, frame.rgba)
        }
        kronello_model::AssetKind::Image => {
            let stream = asset
                .streams
                .first()
                .ok_or_else(|| ServiceError::invalid("image asset has no stream"))?
                .index;
            let image = kronello_media::decode_image_asset(
                &asset,
                &request.project,
                stream,
                kronello_model::ColorSpace::LinearRec709,
            )?;
            (
                None,
                image.size[0],
                image.size[1],
                linear_premultiplied_to_srgb8(&image.pixels),
            )
        }
        _ => {
            return Err(ServiceError::new(
                "UNSUPPORTED_FEATURE",
                "thumbnails require a video or image asset",
            ));
        }
    };
    let (out_width, out_height) = fit_extent(width, height, request.max_size);
    Ok(AssetThumbnailResult {
        asset: asset.id,
        pts,
        width: out_width,
        height: out_height,
        rgba: kronello_media::scale_rgba8(&rgba, width, height, out_width, out_height)?,
    })
}
