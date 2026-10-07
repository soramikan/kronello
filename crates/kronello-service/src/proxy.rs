//! Preview proxy workflow (ADR-0119): `proxy.generate` fixed-input jobs,
//! `proxy.status` link inspection and `proxy.clear` link removal. Generated
//! proxies are ordinary video assets; the link registers provenance only.
use std::path::{Path, PathBuf};

use kronello_jobs::{JobRecord, JobStore, Submission};
use kronello_media::{MediaRuntime, ProxyEncodeResult};
use kronello_model::{
    Asset, AssetId, AssetKind, AssetLocator, DocumentObject, FiniteF64, ProxyLink, StreamMetadata,
    proxy_dimensions,
};
use kronello_time::Rational;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{ProjectInfo, ServiceError, info, open_existing};

/// Fixed worker input for `proxy.generate`. Serialized inside the job
/// envelope; the worker re-validates every identity field against the record.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProxyJobInput {
    /// Canonical project path; the worker registers the link through the
    /// shared store contract like any other edit.
    pub project: PathBuf,
    /// SHA-256 of the serialized document at submit (the record snapshot hash).
    #[schemars(regex(pattern = "^[0-9a-f]{64}$"))]
    pub document_hash: String,
    /// Locked original asset object, content-hash verified on decode.
    pub asset: Asset,
    pub stream_index: u32,
    pub scale: FiniteF64,
    /// Pre-allocated id of the proxy `Asset` the worker registers.
    pub proxy_asset_id: AssetId,
    pub proxy_width: u32,
    pub proxy_height: u32,
    /// Publication destination; must equal the job record destination.
    pub destination: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProxyGenerateRequest {
    pub project: PathBuf,
    /// Optional fence against changes since the caller inspected the project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<String>,
    /// One fixed-input job per authored video asset.
    pub assets: Vec<AssetId>,
    /// Linear scale for proxy dimensions; defaults to 0.5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    /// Source stream per asset; defaults to the first locked video stream.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_index: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProxyStatusRequest {
    pub project: PathBuf,
    /// Restrict the report to links for this original asset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset: Option<AssetId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProxyState {
    /// Link metadata is consistent and the proxy file resolves.
    Ready,
    /// Link identity no longer matches the document (hash, streams, dims).
    Stale,
    /// Link is consistent but the proxy file cannot be located.
    Missing,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProxyStatusEntry {
    pub link: ProxyLink,
    pub state: ProxyState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProxyStatusResult {
    pub proxies: Vec<ProxyStatusEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProxyClearRequest {
    pub project: PathBuf,
    /// Decimal revision, independent of JSON number precision.
    pub base_revision: String,
    /// Either end of the link (original or proxy asset id).
    pub asset: AssetId,
}

fn document_hash(document: &kronello_model::Project) -> Result<String, ServiceError> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&serde_json::to_value(document)?)?)
    ))
}

/// Adjacent managed folder `<stem>.proxies/` beside the project file; a fresh
/// UUID filename keeps every publication name collision-free.
/// Shared per-request context for submitting proxy generation jobs.
struct ProxyJobContext<'a> {
    store: &'a JobStore,
    project: &'a Path,
    document: &'a kronello_model::Project,
    revision: u64,
    document_hash: &'a str,
}

fn proxy_destination(project: &Path, proxy_asset_id: AssetId) -> Result<PathBuf, ServiceError> {
    let stem = project
        .file_stem()
        .ok_or_else(|| ServiceError::invalid("project path has no filename"))?
        .to_string_lossy();
    let directory = project
        .parent()
        .unwrap_or(Path::new("."))
        .join(format!("{stem}.proxies"));
    Ok(directory.join(format!("{proxy_asset_id}.mov")))
}

impl crate::Service<'_> {
    /// `proxy.generate`: submit one fixed-input transcode job per asset.
    /// Nothing about the document changes at submit; the worker registers the
    /// `ProxyLink` and proxy `Asset` after a validated publication staging.
    pub fn generate_proxies(
        &self,
        request: ProxyGenerateRequest,
    ) -> Result<crate::JobListResult, ServiceError> {
        crate::local_locator(&request.project)?;
        if request.assets.is_empty() || request.assets.len() > 256 {
            return Err(ServiceError::invalid(
                "proxy.generate requires 1..=256 assets",
            ));
        }
        let scale = request.scale.unwrap_or(0.5);
        if !scale.is_finite() || scale <= 0.0 || scale > 1.0 {
            return Err(ServiceError::invalid("proxy scale must be in (0, 1]"));
        }
        let project = request.project.canonicalize().map_err(|e| {
            ServiceError::new(
                if e.kind() == std::io::ErrorKind::NotFound {
                    "PROJECT_NOT_FOUND"
                } else {
                    "IO_ERROR"
                },
                e.to_string(),
            )
        })?;
        let stored = crate::session::read_snapshot(&project)?;
        crate::jobs::check_expected_revision(
            request.expected_revision.as_deref(),
            stored.revision,
        )?;
        stored
            .document
            .ensure_editable()
            .map_err(|e| ServiceError::new("UNSUPPORTED_FEATURE", e.to_string()))?;
        let document_hash = document_hash(&stored.document)?;
        let store = self.jobs()?;
        let mut jobs = Vec::with_capacity(request.assets.len());
        let ctx = ProxyJobContext {
            store: &store,
            project: &project,
            document: &stored.document,
            revision: stored.revision,
            document_hash: &document_hash,
        };
        for asset_id in &request.assets {
            jobs.push(self.submit_proxy_job(&ctx, *asset_id, request.stream_index, scale)?);
        }
        Ok(crate::JobListResult { jobs })
    }

    fn submit_proxy_job(
        &self,
        ctx: &ProxyJobContext<'_>,
        asset_id: AssetId,
        stream_index: Option<u32>,
        scale: f64,
    ) -> Result<JobRecord, ServiceError> {
        let ProxyJobContext {
            store,
            project,
            document,
            revision,
            document_hash,
        } = *ctx;
        let asset = document
            .assets
            .iter()
            .find_map(|a| match a {
                DocumentObject::Known(a) if a.id == asset_id => Some(a),
                _ => None,
            })
            .ok_or_else(|| ServiceError::new("ASSET_MISSING", asset_id.to_string()))?;
        if asset.kind != AssetKind::Video {
            return Err(ServiceError::new(
                "INVALID_MEDIA_INPUT",
                "proxy generation requires a video asset",
            ));
        }
        let stream_index = match stream_index {
            Some(index) => index,
            None => asset
                .streams
                .iter()
                .find(|s| s.width.is_some() && s.height.is_some())
                .map(|s| s.index)
                .ok_or_else(|| {
                    ServiceError::new("INVALID_MEDIA_INPUT", "asset has no video stream")
                })?,
        };
        let stream = asset
            .streams
            .iter()
            .find(|s| s.index == stream_index)
            .ok_or_else(|| {
                ServiceError::new("INVALID_MEDIA_INPUT", "proxy stream index missing")
            })?;
        let (Some(width), Some(height)) = (stream.width, stream.height) else {
            return Err(ServiceError::new(
                "INVALID_MEDIA_INPUT",
                "proxy stream lacks locked dimensions",
            ));
        };
        let (proxy_width, proxy_height) = proxy_dimensions(width, height, scale)
            .ok_or_else(|| ServiceError::invalid("proxy dimensions out of range"))?;
        let proxy_asset_id = AssetId::new();
        let destination = proxy_destination(project, proxy_asset_id)?;
        if destination.exists() {
            return Err(ServiceError::new(
                "OUTPUT_EXISTS",
                "proxy destination exists",
            ));
        }
        let input = ProxyJobInput {
            project: project.to_path_buf(),
            document_hash: document_hash.into(),
            asset: asset.clone(),
            stream_index,
            scale: FiniteF64::new(scale).map_err(|e| ServiceError::invalid(e.to_string()))?,
            proxy_asset_id,
            proxy_width,
            proxy_height,
            destination: destination.clone(),
        };
        let total_frames = stream
            .duration
            .and_then(|d| d.checked_div(stream.time_base).ok())
            .map(|ticks| ticks.floor().max(1) as u64)
            .unwrap_or(1);
        let fixed = crate::jobs::FixedInput::proxy(input.clone());
        let submission = Submission {
            engine_version: env!("CARGO_PKG_VERSION").into(),
            project_id: document.id.to_string(),
            revision: revision.to_string(),
            snapshot_hash: document_hash.into(),
            output_profile: serde_json::to_value(&input)?,
            destination,
            total_frames,
        };
        let record = store.submit(&serde_json::to_vec(&fixed)?, submission)?;
        let executable = match &self.worker_executable {
            Some(path) => path.clone(),
            None => std::env::current_exe()?,
        };
        if let Err(error) = store.spawn_attempt(&record.id, record.attempt, &executable) {
            store.finish_error_attempt(&record.id, record.attempt, &error)?;
            return Err(error.into());
        }
        Ok(record)
    }

    /// `proxy.status`: metadata-level link report; proxy files are located but
    /// never hashed (decode verifies content at use).
    pub fn proxy_status(
        &self,
        request: ProxyStatusRequest,
    ) -> Result<ProxyStatusResult, ServiceError> {
        crate::local_locator(&request.project)?;
        let stored = crate::session::read_snapshot(&request.project)?;
        let mut proxies = Vec::new();
        for link in &stored.document.proxies {
            if request
                .asset
                .is_some_and(|a| a != link.original && a != link.proxy)
            {
                continue;
            }
            let (state, detail) = match stored.document.proxy_link_state(link) {
                Err(error) => (ProxyState::Stale, Some(error.to_string())),
                Ok(()) => {
                    let proxy = stored.document.assets.iter().find_map(|a| match a {
                        DocumentObject::Known(a) if a.id == link.proxy => Some(a),
                        _ => None,
                    });
                    match proxy {
                        Some(asset) => {
                            match kronello_media::locate_asset(asset, &request.project) {
                                Ok(_) => (ProxyState::Ready, None),
                                Err(error) => (ProxyState::Missing, Some(error.to_string())),
                            }
                        }
                        None => (ProxyState::Stale, Some("proxy asset object missing".into())),
                    }
                }
            };
            proxies.push(ProxyStatusEntry {
                link: link.clone(),
                state,
                detail,
            });
        }
        Ok(ProxyStatusResult { proxies })
    }

    /// `proxy.clear`: remove the link; remove the managed proxy asset object
    /// when no authored structure still references it. The file on disk is
    /// never deleted by this command.
    pub fn proxy_clear(&self, request: ProxyClearRequest) -> Result<ProjectInfo, ServiceError> {
        crate::local_locator(&request.project)?;
        let revision = crate::parse_revision(&request.base_revision)?;
        let mut store = open_existing(&request.project)?;
        let result = (|| {
            let mut document = store.snapshot()?.document;
            document
                .ensure_editable()
                .map_err(|e| ServiceError::new("UNSUPPORTED_FEATURE", e.to_string()))?;
            let Some(position) = document
                .proxies
                .iter()
                .position(|l| l.original == request.asset || l.proxy == request.asset)
            else {
                return Err(ServiceError::new(
                    "PROXY_NOT_FOUND",
                    "no proxy link for asset",
                ));
            };
            let link = document.proxies.remove(position);
            if !document.asset_in_use(link.proxy) {
                document
                    .assets
                    .retain(|a| !matches!(a, DocumentObject::Known(a) if a.id == link.proxy));
            }
            store.import_json(
                revision,
                uuid::Uuid::new_v4(),
                &serde_json::to_string(&document)?,
            )?;
            info(&store)
        })();
        let result = result?;
        store.close()?;
        Ok(result)
    }
}

fn known_asset(document: &kronello_model::Project, id: AssetId) -> Option<&Asset> {
    document.assets.iter().find_map(|a| match a {
        DocumentObject::Known(a) if a.id == id => Some(a),
        _ => None,
    })
}

/// Register the proxy asset and link through the store's normal import path.
/// Retried across concurrent edits while the original asset stays unchanged.
fn register_proxy_link(
    input: &ProxyJobInput,
    record: &JobRecord,
    proxy_asset: &Asset,
) -> Result<(), ServiceError> {
    let link = ProxyLink {
        original: input.asset.id,
        proxy: input.proxy_asset_id,
        original_stream_index: input.stream_index,
        proxy_stream_index: 0,
        scale: input.scale,
        width: input.proxy_width,
        height: input.proxy_height,
        source_content_hash: input.asset.content_hash.clone(),
        source_duration: input
            .asset
            .streams
            .iter()
            .find(|s| s.index == input.stream_index)
            .and_then(|s| s.duration),
        job: Some(record.id.clone()),
    };
    // Bounded against concurrent editors: revision conflicts retry immediately,
    // an exclusive safe-mode lock waits briefly between attempts.
    let mut delay_ms = 0u64;
    for attempt in 0..64u32 {
        match register_proxy_link_once(input, &link, proxy_asset) {
            Ok(()) => return Ok(()),
            Err(error)
                if (error.code == "REVISION_CONFLICT" || error.code == "PROJECT_LOCKED")
                    && attempt < 63 =>
            {
                if error.code == "PROJECT_LOCKED" {
                    delay_ms = (delay_ms + 25).min(250);
                    std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                }
            }
            Err(error) => return Err(error),
        }
    }
    Err(ServiceError::new(
        "PROJECT_LOCKED",
        "proxy link registration exhausted retries",
    ))
}

fn register_proxy_link_once(
    input: &ProxyJobInput,
    link: &ProxyLink,
    proxy_asset: &Asset,
) -> Result<(), ServiceError> {
    let mut store = open_existing(&input.project)?;
    let result = (|| {
        let snapshot = store.snapshot()?;
        let mut document = snapshot.document;
        document
            .ensure_editable()
            .map_err(|e| ServiceError::new("UNSUPPORTED_FEATURE", e.to_string()))?;
        let original = known_asset(&document, link.original).ok_or_else(|| {
            ServiceError::new("PROXY_SOURCE_CHANGED", "original asset was removed")
        })?;
        if original.content_hash != link.source_content_hash {
            return Err(ServiceError::new(
                "PROXY_SOURCE_CHANGED",
                "original asset content changed during generation",
            ));
        }
        if let Some(existing) = document.proxy_link(link.original) {
            if existing == link {
                // Worker resume: the earlier attempt already committed.
                return Ok(());
            }
            return Err(ServiceError::new(
                "PROXY_LINK_CONFLICT",
                "a different proxy link already exists for the original",
            ));
        }
        if known_asset(&document, link.proxy).is_some() {
            return Err(ServiceError::new(
                "PROXY_LINK_CONFLICT",
                "proxy asset id already present without a link",
            ));
        }
        document
            .assets
            .push(DocumentObject::Known(proxy_asset.clone()));
        document.proxies.push(link.clone());
        store.import_json(
            snapshot.revision,
            uuid::Uuid::new_v4(),
            &serde_json::to_string(&document)?,
        )?;
        Ok(())
    })();
    let close = store.close();
    result.and(close.map_err(Into::into))
}

/// Worker half of `proxy.generate`: fixed-input validation, transcode,
/// document registration, atomic publication. Called for every attempt,
/// including resumed jobs; every step is idempotent or content-verified.
pub(crate) fn execute_proxy_job(
    store: &JobStore,
    record: &JobRecord,
    input: &ProxyJobInput,
) -> Result<(), ServiceError> {
    if record.engine_version != env!("CARGO_PKG_VERSION") {
        return Err(ServiceError::new(
            "UNSUPPORTED_FEATURE",
            "job engine version differs",
        ));
    }
    if input.document_hash != record.snapshot_hash
        || input.destination != record.destination
        || serde_json::to_value(input)? != record.output_profile
    {
        return Err(ServiceError::new(
            "JOB_INPUT_HASH_MISMATCH",
            "fixed proxy input identity differs",
        ));
    }
    input
        .asset
        .validate()
        .map_err(|e| ServiceError::invalid(e.to_string()))?;
    if kronello_model::proxy_dimensions(
        input
            .asset
            .streams
            .iter()
            .find(|s| s.index == input.stream_index)
            .and_then(|s| s.width)
            .unwrap_or(0),
        input
            .asset
            .streams
            .iter()
            .find(|s| s.index == input.stream_index)
            .and_then(|s| s.height)
            .unwrap_or(0),
        input.scale.get(),
    ) != Some((input.proxy_width, input.proxy_height))
    {
        return Err(ServiceError::new(
            "JOB_INPUT_HASH_MISMATCH",
            "proxy dimensions do not match the locked stream",
        ));
    }
    let runtime = MediaRuntime::load()?;
    let source = kronello_media::resolve_asset(&input.asset, &input.project)?;
    // The managed `<stem>.proxies/` directory may not exist yet; job staging
    // and atomic publication both require it on the destination volume.
    if let Some(parent) = record.destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let staging = store.staging(record)?;
    let stage_path = staging.output();
    let mut poll = 0u64;
    let result = runtime.encode_proxy(
        &source,
        input.stream_index,
        input.proxy_width,
        input.proxy_height,
        &stage_path,
        &mut |frames| {
            store
                .checkpoint(&record.id, frames)
                .map_err(|e| kronello_media::MediaError::InvalidInput(e.to_string()))?;
            poll += 1;
            if poll.is_multiple_of(32) {
                let current = store
                    .get(&record.id)
                    .map_err(|e| kronello_media::MediaError::InvalidInput(e.to_string()))?;
                if current.cancel_requested {
                    return Err(kronello_media::MediaError::InvalidInput(
                        "JOB_CANCELED: cancel requested".into(),
                    ));
                }
                if current.attempt != record.attempt {
                    return Err(kronello_media::MediaError::InvalidInput(
                        "JOB_INTERRUPTED: attempt superseded".into(),
                    ));
                }
            }
            Ok(())
        },
    )?;
    // Duration agreement between the locked source stream and the written
    // proxy is bounded by one source-stream tick (container rounding).
    let source_duration = input
        .asset
        .streams
        .iter()
        .find(|s| s.index == input.stream_index)
        .and_then(|s| s.duration);
    if let (Some(source), Some(proxied)) = (source_duration, result.duration) {
        let difference = source
            .checked_sub(proxied)
            .map_err(|e| ServiceError::invalid(e.to_string()))?;
        let tolerance = result
            .time_base
            .checked_mul(Rational::from_integer(2))
            .map_err(|e| ServiceError::invalid(e.to_string()))?;
        if difference > tolerance
            || difference
                < tolerance
                    .checked_neg()
                    .map_err(|e| ServiceError::invalid(e.to_string()))?
        {
            return Err(ServiceError::new(
                "OUTPUT_VALIDATION_FAILED",
                format!("proxy duration {proxied:?} differs from source {source:?}"),
            ));
        }
    }
    let report = serde_json::json!({
        "proxy_asset_id": input.proxy_asset_id,
        "original": input.asset.id,
        "stream_index": input.stream_index,
        "scale": input.scale,
        "width": result.width,
        "height": result.height,
        "frames": result.frames,
        "duration": result.duration,
        "content_hash": result.content_hash,
    });
    let outcome =
        serde_json::json!({"destination": record.destination, "validated": true, "report": report});
    store.prepare_publication(record, &stage_path, outcome.clone())?;
    let proxy_asset = proxy_asset_object(input, &result)?;
    register_proxy_link(input, record, &proxy_asset)?;
    store.publish_attempt(&record.id, record.attempt, outcome, || {
        kronello_jobs::publish_path(&stage_path, &record.destination)
    })?;
    Ok(())
}

/// Proxy `Asset` object derived from the verified container probe; the locator
/// prefers a project-relative path so collect/relink traversal stays portable.
fn proxy_asset_object(
    input: &ProxyJobInput,
    result: &ProxyEncodeResult,
) -> Result<Asset, ServiceError> {
    let video = result
        .probe
        .streams
        .iter()
        .find(|s| s.kind == kronello_media::StreamKind::Video)
        .ok_or_else(|| ServiceError::new("OUTPUT_VALIDATION_FAILED", "no proxy video stream"))?;
    let base = input.project.parent().unwrap_or(Path::new("."));
    let relative = input
        .destination
        .strip_prefix(base)
        .ok()
        .map(|p| p.to_string_lossy().into_owned());
    Ok(Asset {
        id: input.proxy_asset_id,
        content_hash: result.content_hash.clone(),
        kind: AssetKind::Video,
        streams: vec![StreamMetadata {
            index: video.index,
            codec: video.codec.clone(),
            time_base: video.time_base,
            duration: result.duration,
            start_time: video.start,
            width: Some(result.width),
            height: Some(result.height),
            pixel_format: video.pixel_format.clone(),
            color_primaries: video.color_primaries.clone(),
            color_transfer: video.color_transfer.clone(),
            color_matrix: video.color_matrix.clone(),
            color_range: video.color_range.clone(),
        }],
        locator: AssetLocator {
            relative,
            absolute: Some(input.destination.to_string_lossy().into_owned()),
        },
    })
}
