//! FLOW-004 capture/ingest (ADR-0135): `capture.start` submits a detached
//! recording session as a fixed-input job; the worker spools captured RGBA8
//! frames to a provisional file outside project state, and `capture.stop`
//! finalizes the spool — hash, probe, asset registration, atomic publication.
//! `capture.status` reports sessions plus typed orphans left by dead workers.
//! `capture.deck_probe` is the vendor-SDK deck boundary: without a linked
//! adapter it is a typed `UNSUPPORTED_FEATURE`, never a silent fallback.
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use kronello_jobs::{JobRecord, JobStore, Submission};
use kronello_media::{EncodeCodec, EncodeFrame, EncodeRequest, MediaRuntime};
use kronello_model::{Asset, AssetId, AssetKind, AssetLocator, DocumentObject, StreamMetadata};
use kronello_time::{FrameRate, Rational};
use serde::{Deserialize, Serialize};

use crate::{ServiceError, open_existing};

/// Capture sources. Selection is explicit: a screen/window/app source names
/// its target; `synthetic` is the deterministic in-repo adapter used by tests;
/// `deck` is the vendor-SDK ingest boundary (ADR-0134 style).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CaptureSource {
    /// Deterministic in-repo source: a paced synthetic frame generator.
    Synthetic,
    /// Full-display capture; `display` is a `CGDirectDisplayID`, absent is main.
    Screen {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display: Option<u32>,
    },
    /// A single window by `CGWindowID`.
    Window { window: u32 },
    /// Every on-screen window owned by the bundle identifier.
    Application { bundle_id: String },
    /// Deck ingest through the vendor SDK adapter (DeckLink / RS-422).
    Deck {
        device: DeckDevice,
        /// Vendor-reported device identity; absent selects the first device.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeckDevice {
    Decklink,
    Rs422,
}

/// The recorded container/codec contract. `pro_res` is the only codec in v1:
/// the provisional spool is lossless RGBA8 and finalization encodes a BT.709
/// ProRes MOV through the bounded `kronello-media` encoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CaptureCodec {
    ProRes,
}

/// Recorded signal color tag. `bt709` is the only supported tag in v1; the
/// ProRes container stream carries the matching `colr` metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CaptureColor {
    Bt709,
}

/// Static output contract fixed at submission. Captured frames are appended in
/// delivery order and container-stamped one tick per frame at `frame_rate` —
/// the record path performs no drops and no re-time correction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaptureFormat {
    pub width: u32,
    pub height: u32,
    pub frame_rate: FrameRate,
    pub codec: CaptureCodec,
    pub color: CaptureColor,
}

/// Fixed worker input serialized inside the job envelope. Every identity field
/// is re-validated against the record before the worker records or registers.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaptureJobInput {
    /// Canonical project path; the worker registers the asset through the
    /// shared store contract like any other edit.
    pub project: PathBuf,
    /// SHA-256 of the serialized document at submit (the record snapshot hash).
    #[schemars(regex(pattern = "^[0-9a-f]{64}$"))]
    pub document_hash: String,
    pub source: CaptureSource,
    pub format: CaptureFormat,
    /// Pre-allocated id of the video `Asset` registered on publication.
    pub capture_asset_id: AssetId,
    /// Publication destination; must equal the job record destination.
    pub destination: PathBuf,
    /// Optional self-finalizing frame bound for deterministic runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_frames: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaptureStartRequest {
    pub project: PathBuf,
    /// Optional fence against changes since the caller inspected the project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<String>,
    pub source: CaptureSource,
    pub format: CaptureFormat,
    /// Caller-pinned asset id; absent mints a fresh one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset: Option<AssetId>,
    /// FLOW-003-style submission key scoped to the project; replay returns the
    /// recorded job without spawning a second recording.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    /// Stop and publish automatically after this many frames.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_frames: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaptureStopRequest {
    pub project: PathBuf,
    /// Capture job id (the session). Stop is a graceful end-of-input signal;
    /// the worker still hashes and registers what was recorded.
    pub job: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaptureStatusRequest {
    pub project: PathBuf,
    /// Restrict the session list to one job id. Orphans are always reported
    /// for the project-wide capture directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<String>,
}

/// One capture session observed by `capture.status`; the job record is the
/// authoritative progress/lifecycle source (ADR-0025).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaptureSessionEntry {
    pub job: JobRecord,
    pub asset: AssetId,
    pub source: CaptureSource,
    /// `true` once `capture.stop` was requested while the worker still runs.
    pub stop_requested: bool,
    /// The live provisional spool path while it exists. It is never a project
    /// asset; only a finalized, hashed publication registers one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provisional: Option<PathBuf>,
    /// Whether the document already contains the registered asset.
    pub asset_registered: bool,
}

/// A provisional spool whose owning session ended without publication —
/// worker death, failure, or cancellation. Reported, never silently deleted
/// and never exposed as a project asset.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaptureOrphan {
    pub provisional: PathBuf,
    /// Owning job id when the filename parses as one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<String>,
    /// Job status when the record resolves; `interrupted` marks a dead worker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_status: Option<kronello_jobs::JobStatus>,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaptureStatusResult {
    pub sessions: Vec<CaptureSessionEntry>,
    pub orphans: Vec<CaptureOrphan>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaptureDeckProbeRequest {
    /// Restrict the probe to one vendor adapter family.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<DeckDevice>,
}

/// One deck device reported by a linked vendor adapter build.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeckDeviceEntry {
    pub device: DeckDevice,
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeckProbeResult {
    pub devices: Vec<DeckDeviceEntry>,
}

const CAPTURE_DIR_SUFFIX: &str = "capture";
/// Provisional spools name their owning job so `capture.status` can link an
/// abandoned file to its record (or report it when the record is gone).
const PROVISIONAL_SUFFIX: &str = "provisional";

/// The capture-dedicated directory beside the project file: `<stem>.capture/`.
/// Everything inside is managed scratch/output — never a project asset until
/// a finalized file is hashed, registered and atomically published.
pub(crate) fn capture_dir(project: &Path) -> Result<PathBuf, ServiceError> {
    let stem = project
        .file_stem()
        .ok_or_else(|| ServiceError::invalid("project path has no filename"))?
        .to_string_lossy();
    Ok(project
        .parent()
        .unwrap_or(Path::new("."))
        .join(format!("{stem}.{CAPTURE_DIR_SUFFIX}")))
}

fn provisional_path(capture_dir: &Path, job: &str) -> PathBuf {
    capture_dir.join(format!("{job}.{PROVISIONAL_SUFFIX}"))
}

fn gpu_error(error: kronello_gpu::GpuError) -> ServiceError {
    let code = match &error {
        kronello_gpu::GpuError::UnsupportedFeature(_) => "UNSUPPORTED_FEATURE",
        kronello_gpu::GpuError::AdapterUnavailable(_) => "ADAPTER_UNAVAILABLE",
        kronello_gpu::GpuError::DeviceUnavailable(_) => "DEVICE_UNAVAILABLE",
        kronello_gpu::GpuError::InvalidInput(_) => "INVALID_INPUT",
        kronello_gpu::GpuError::Readback(_) => "READBACK_FAILED",
        kronello_gpu::GpuError::CacheIo(_) => "CACHE_IO",
        kronello_gpu::GpuError::ObservationBusy => "RENDER_BACKEND_BUSY",
    };
    ServiceError::new(code, error.to_string())
}

impl DeckDevice {
    fn adapter(self) -> kronello_framebridge::capture::DeckAdapter {
        match self {
            Self::Decklink => kronello_framebridge::capture::DeckAdapter::Decklink,
            Self::Rs422 => kronello_framebridge::capture::DeckAdapter::Rs422,
        }
    }
}

/// The vendor-SDK availability gate shared by `capture.start` validation and
/// `capture.deck.probe`: typed `UNSUPPORTED_FEATURE` when no adapter is linked.
fn require_deck_adapter(device: Option<DeckDevice>) -> Result<(), ServiceError> {
    match device {
        Some(device) => {
            kronello_framebridge::capture::DeckCapture::open(
                device.adapter(),
                None,
                kronello_framebridge::capture::CaptureConfig {
                    width: 2,
                    height: 2,
                    fps_num: 25,
                    fps_den: 1,
                },
            )
            .map_err(gpu_error)?;
            Ok(())
        }
        None => {
            kronello_framebridge::capture::probe_deck_devices().map_err(gpu_error)?;
            Ok(())
        }
    }
}

pub(crate) fn validate_format(format: &CaptureFormat) -> Result<(), ServiceError> {
    let fps = format.frame_rate.as_rational();
    if format.width == 0
        || format.height == 0
        || format.width & 1 != 0
        || format.height & 1 != 0
        || format.width > 16384
        || format.height > 16384
    {
        return Err(ServiceError::invalid(
            "capture format requires positive even dimensions up to 16384",
        ));
    }
    if fps <= Rational::ZERO {
        return Err(ServiceError::invalid("capture frame_rate must be positive"));
    }
    if fps > Rational::new(240, 1).map_err(|e| ServiceError::invalid(e.to_string()))? {
        return Err(ServiceError::invalid(
            "capture frame_rate must not exceed 240 fps",
        ));
    }
    Ok(())
}

/// `capture.start` identity checks shared by the worker entry and
/// `job.resume`: the fixed payload is the whole contract (source, format,
/// pre-allocated asset id, destination) and must equal the recorded
/// submission exactly.
pub(crate) fn validate_capture_job(
    record: &JobRecord,
    input: &CaptureJobInput,
) -> Result<(), ServiceError> {
    if input.document_hash != record.snapshot_hash
        || input.destination != record.destination
        || serde_json::to_value(input)? != record.output_profile
    {
        return Err(ServiceError::new(
            "JOB_INPUT_HASH_MISMATCH",
            "fixed capture input identity differs",
        ));
    }
    validate_format(&input.format)
}

fn validate_request(request: &CaptureStartRequest) -> Result<(), ServiceError> {
    crate::local_locator(&request.project)?;
    validate_format(&request.format)?;
    if request.max_frames == Some(0) {
        return Err(ServiceError::invalid("max_frames must be at least one"));
    }
    match &request.source {
        // The deterministic source is always available: it is how tests
        // exercise the full contract without hardware.
        CaptureSource::Synthetic => Ok(()),
        // OS sources exist only through the macOS ScreenCaptureKit adapter;
        // the platform gate stays typed rather than silently unavailable.
        CaptureSource::Screen { .. }
        | CaptureSource::Window { .. }
        | CaptureSource::Application { .. } => {
            if cfg!(target_os = "macos") {
                Ok(())
            } else {
                Err(ServiceError::new(
                    "UNSUPPORTED_FEATURE",
                    "screen/window/application capture requires the macOS ScreenCaptureKit adapter",
                ))
            }
        }
        // The vendor-SDK adapter boundary (ADR-0134 style): DeckLink/RS-422
        // ingest is legal only when a vendor adapter is actually linked.
        CaptureSource::Deck { device, .. } => require_deck_adapter(Some(*device)),
    }
}

impl crate::Service<'_> {
    /// `capture.start`: validate, allocate the destination under the managed
    /// `<stem>.capture/` directory and submit the recording as a detached
    /// fixed-input job. The provisional file is written by the worker only.
    pub fn capture_start(&self, request: CaptureStartRequest) -> Result<JobRecord, ServiceError> {
        validate_request(&request)?;
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
        let document_hash = crate::proxy::document_hash(&stored.document)?;
        let store = self.jobs()?;
        // Keyed replay precedes destination/destination-exists checks: the
        // recorded job already owns its output path (FLOW-003 convention).
        let key = request.idempotency_key.clone();
        let payload = serde_json::to_string(&request)?;
        if let Some(key) = &key
            && let Some(record) = store.replay(&stored.document.id.to_string(), key, &payload)?
        {
            return Ok(record);
        }
        let asset_id = request.asset.unwrap_or_default();
        let capture_dir = capture_dir(&project)?;
        let destination = capture_dir.join(format!("{asset_id}.mov"));
        if destination.exists() {
            return Err(ServiceError::new(
                "OUTPUT_EXISTS",
                "capture destination exists",
            ));
        }
        if stored
            .document
            .assets
            .iter()
            .any(|a| matches!(a, DocumentObject::Known(a) if a.id == asset_id))
        {
            return Err(ServiceError::new(
                "CAPTURE_ASSET_CONFLICT",
                "capture asset id already registered",
            ));
        }
        let input = CaptureJobInput {
            project: project.clone(),
            document_hash,
            source: request.source.clone(),
            format: request.format,
            capture_asset_id: asset_id,
            destination: destination.clone(),
            max_frames: request.max_frames,
        };
        let fixed = crate::jobs::FixedInput::capture(input.clone());
        let submission = Submission {
            engine_version: env!("CARGO_PKG_VERSION").into(),
            project_id: stored.document.id.to_string(),
            revision: stored.revision.to_string(),
            snapshot_hash: input.document_hash.clone(),
            output_profile: serde_json::to_value(&input)?,
            destination,
            // Open-ended session: the worker discovers the count at stop time.
            total_frames: 0,
        };
        std::fs::create_dir_all(&capture_dir)?;
        let record = match &key {
            Some(key) => {
                match store.submit_keyed(&serde_json::to_vec(&fixed)?, submission, key, &payload)? {
                    kronello_jobs::KeyedSubmission::Submitted(record) => record,
                    kronello_jobs::KeyedSubmission::Replayed(record) => return Ok(record),
                }
            }
            None => store.submit(&serde_json::to_vec(&fixed)?, submission)?,
        };
        self.spawn_worker(&store, &record)?;
        Ok(record)
    }

    /// `capture.stop`: leave a stop marker in the job directory. The worker
    /// notices between frames and runs normal finalization — hash, register,
    /// publish — so a stopped recording commits as a successful job.
    pub fn capture_stop(&self, request: CaptureStopRequest) -> Result<JobRecord, ServiceError> {
        crate::local_locator(&request.project)?;
        let stored = crate::session::read_snapshot(&request.project)?;
        let store = self.jobs()?;
        let record = store.get(&request.job)?;
        if record.project_id != stored.document.id.to_string()
            || record.output_profile.get("capture_asset_id").is_none()
        {
            return Err(ServiceError::new(
                "CAPTURE_NOT_FOUND",
                "job is not a capture session for this project",
            ));
        }
        Ok(store.request_stop(&request.job)?)
    }

    /// `capture.status`: session list plus typed orphan reconciliation. Reading
    /// the job list performs heartbeat-timeout recovery, so a dead worker's
    /// session surfaces here as `interrupted` alongside its orphan spool.
    pub fn capture_status(
        &self,
        request: CaptureStatusRequest,
    ) -> Result<CaptureStatusResult, ServiceError> {
        crate::local_locator(&request.project)?;
        let stored = crate::session::read_snapshot(&request.project)?;
        let project_id = stored.document.id.to_string();
        let store = self.jobs()?;
        let records = store.list()?;
        let capture_dir = capture_dir(&request.project)?;
        if let Some(job) = &request.job {
            match records.iter().find(|r| r.id == *job) {
                Some(record)
                    if record.project_id == project_id
                        && record.output_profile.get("capture_asset_id").is_some() => {}
                _ => {
                    return Err(ServiceError::new(
                        "CAPTURE_NOT_FOUND",
                        "job is not a capture session for this project",
                    ));
                }
            }
        }
        let mut sessions = Vec::new();
        for record in &records {
            let Some(value) = record.output_profile.get("capture_asset_id") else {
                continue;
            };
            if record.project_id != project_id {
                continue;
            }
            if request.job.as_deref().is_some_and(|job| job != record.id) {
                continue;
            }
            let asset = serde_json::from_value::<AssetId>(value.clone())
                .map_err(|e| ServiceError::new("JOB_STORAGE_ERROR", e.to_string()))?;
            let source = record
                .output_profile
                .get("source")
                .cloned()
                .and_then(|value| serde_json::from_value::<CaptureSource>(value).ok())
                .ok_or_else(|| {
                    ServiceError::new("JOB_STORAGE_ERROR", "capture record lacks a source")
                })?;
            let provisional = provisional_path(&capture_dir, &record.id);
            let asset_registered = stored
                .document
                .assets
                .iter()
                .any(|a| matches!(a, DocumentObject::Known(a) if a.id == asset));
            sessions.push(CaptureSessionEntry {
                stop_requested: store.stop_requested(&record.id)?,
                provisional: provisional.is_file().then_some(provisional),
                asset,
                source,
                asset_registered,
                job: record.clone(),
            });
        }
        let mut orphans = Vec::new();
        if capture_dir.is_dir() {
            for entry in std::fs::read_dir(&capture_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some(PROVISIONAL_SUFFIX)
                    || !path.is_file()
                {
                    continue;
                }
                let job = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| uuid::Uuid::parse_str(s).ok())
                    .map(|id| id.to_string());
                let record = job
                    .as_deref()
                    .and_then(|id| records.iter().find(|r| r.id == id));
                // A provisional whose job is still active is a live spool, not
                // an orphan; every other leftover is reported.
                if record.is_some_and(|r| r.status.active()) {
                    continue;
                }
                orphans.push(CaptureOrphan {
                    bytes: entry.metadata().map(|m| m.len()).unwrap_or(0),
                    provisional: path,
                    job,
                    job_status: record.map(|r| r.status),
                });
            }
        }
        Ok(CaptureStatusResult { sessions, orphans })
    }

    /// `capture.deck.probe`: vendor-SDK device discovery. With no vendor
    /// adapter linked this is the typed `UNSUPPORTED_FEATURE` boundary rather
    /// than an empty or fabricated device list.
    pub fn capture_deck_probe(
        &self,
        request: CaptureDeckProbeRequest,
    ) -> Result<DeckProbeResult, ServiceError> {
        require_deck_adapter(request.device)?;
        let devices = kronello_framebridge::capture::probe_deck_devices()
            .map_err(gpu_error)?
            .into_iter()
            .filter(|device| {
                request
                    .device
                    .is_none_or(|wanted| device.adapter == wanted.adapter())
            })
            .map(|device| DeckDeviceEntry {
                device: match device.adapter {
                    kronello_framebridge::capture::DeckAdapter::Decklink => DeckDevice::Decklink,
                    kronello_framebridge::capture::DeckAdapter::Rs422 => DeckDevice::Rs422,
                },
                id: device.id,
                label: device.label,
            })
            .collect();
        Ok(DeckProbeResult { devices })
    }
}

/// One produced frame: opaque RGBA8, exactly `width * height * 4` bytes.
/// Presentation position is the delivery index; the container stamps one tick
/// per index — capture records what the source delivered, unaltered.
struct CapturedFrame {
    rgba: Vec<u8>,
}

/// What a source produced on this poll. Live capture sources are sessions,
/// not finite streams: recording ends at the stop marker, `max_frames` or
/// cancellation — never at source exhaustion.
enum Produced {
    Frame(CapturedFrame),
    /// Nothing delivered yet; the source is still open. The worker re-checks
    /// the stop marker and calls `next` again — a quiet source never stalls
    /// a stop request.
    Idle,
}

/// The capture-source contract shared by the deterministic synthetic adapter,
/// the ScreenCaptureKit session and the vendor deck boundary.
trait FrameProducer {
    fn next(&mut self) -> Result<Produced, ServiceError>;
}

/// Deterministic in-repo adapter (ADR-0135): emits a fixed synthetic pattern
/// paced at the declared frame rate, so the full record/finalize/hash/register
/// path is exercised without hardware and without wall-clock content.
struct SyntheticSource {
    width: u32,
    height: u32,
    index: u64,
    /// Seconds per frame at the declared rate; pacing only, never content.
    frame_seconds: f64,
    start: Instant,
}

impl SyntheticSource {
    fn new(format: &CaptureFormat) -> Self {
        let rate = format.frame_rate.as_rational();
        Self {
            width: format.width,
            height: format.height,
            index: 0,
            frame_seconds: rate.denominator() as f64 / rate.numerator() as f64,
            start: Instant::now(),
        }
    }
    /// Frame (x, y, index) → RGBA8 is a pure function — identical inputs always
    /// produce identical bytes, which is what makes the pipeline testable.
    fn generate(&self, index: u64) -> Vec<u8> {
        let mut rgba = Vec::with_capacity((self.width * self.height * 4) as usize);
        for y in 0..self.height {
            for x in 0..self.width {
                rgba.extend_from_slice(&[
                    ((x.wrapping_mul(17) + (index as u32).wrapping_mul(7)) & 0xff) as u8,
                    ((y.wrapping_mul(31) + (index as u32).wrapping_mul(3)) & 0xff) as u8,
                    (((x ^ y).wrapping_add(index as u32)) & 0xff) as u8,
                    255,
                ]);
            }
        }
        rgba
    }
}

impl FrameProducer for SyntheticSource {
    fn next(&mut self) -> Result<Produced, ServiceError> {
        let index = self.index;
        self.index = index + 1;
        // Pace delivery like a real-time source: the content is deterministic,
        // the arrival cadence is the declared frame rate.
        let deadline = self.start + Duration::from_secs_f64(self.frame_seconds * index as f64);
        let now = Instant::now();
        if deadline > now {
            std::thread::sleep(deadline - now);
        }
        Ok(Produced::Frame(CapturedFrame {
            rgba: self.generate(index),
        }))
    }
}

struct ScreenCaptureProducer {
    session: kronello_framebridge::capture::ScreenCapture,
}

impl FrameProducer for ScreenCaptureProducer {
    fn next(&mut self) -> Result<Produced, ServiceError> {
        match self
            .session
            .next_rgba(Duration::from_millis(250))
            .map_err(gpu_error)?
        {
            Some(rgba) => Ok(Produced::Frame(CapturedFrame { rgba })),
            // A delivery timeout is not end-of-input.
            None => Ok(Produced::Idle),
        }
    }
}

fn open_source(input: &CaptureJobInput) -> Result<Box<dyn FrameProducer>, ServiceError> {
    match &input.source {
        CaptureSource::Synthetic => Ok(Box::new(SyntheticSource::new(&input.format))),
        CaptureSource::Screen { display } => Ok(Box::new(ScreenCaptureProducer {
            session: kronello_framebridge::capture::ScreenCapture::open(
                kronello_framebridge::capture::CaptureTarget::Screen { display: *display },
                capture_config(&input.format),
            )
            .map_err(gpu_error)?,
        })),
        CaptureSource::Window { window } => Ok(Box::new(ScreenCaptureProducer {
            session: kronello_framebridge::capture::ScreenCapture::open(
                kronello_framebridge::capture::CaptureTarget::Window { window: *window },
                capture_config(&input.format),
            )
            .map_err(gpu_error)?,
        })),
        CaptureSource::Application { bundle_id } => Ok(Box::new(ScreenCaptureProducer {
            session: kronello_framebridge::capture::ScreenCapture::open(
                kronello_framebridge::capture::CaptureTarget::Application {
                    bundle_id: bundle_id.clone(),
                },
                capture_config(&input.format),
            )
            .map_err(gpu_error)?,
        })),
        CaptureSource::Deck { device, device_id } => {
            // `open` is the typed vendor gate. No adapter is linked in this
            // build; a vendor build would extend `DeckCapture` with frame
            // delivery and hand a producer back here.
            let _session = kronello_framebridge::capture::DeckCapture::open(
                device.adapter(),
                device_id.as_deref(),
                capture_config(&input.format),
            )
            .map_err(gpu_error)?;
            Err(ServiceError::new(
                "UNSUPPORTED_FEATURE",
                "deck ingest requires a vendor adapter frame producer",
            ))
        }
    }
}

fn capture_config(format: &CaptureFormat) -> kronello_framebridge::capture::CaptureConfig {
    let rate = format.frame_rate.as_rational();
    kronello_framebridge::capture::CaptureConfig {
        width: format.width,
        height: format.height,
        fps_num: rate.numerator() as u32,
        fps_den: rate.denominator() as u32,
    }
}

/// Worker half of `capture.start` (ADR-0025 detached boundary): record to the
/// provisional spool until `capture.stop`, source exhaustion or `max_frames`,
/// then encode, verify, hash, register and publish exactly once.
pub(crate) fn execute_capture_job(
    store: &JobStore,
    record: &JobRecord,
    input: &CaptureJobInput,
) -> Result<(), ServiceError> {
    if record.engine_version != env!("CARGO_PKG_VERSION") {
        return Err(ServiceError::new(
            "UNSUPPORTED_FEATURE",
            "job engine version differs",
        ));
    }
    validate_capture_job(record, input)?;
    let runtime = MediaRuntime::load()?;
    let capture_dir = input
        .destination
        .parent()
        .ok_or_else(|| ServiceError::invalid("capture destination has no parent"))?;
    std::fs::create_dir_all(capture_dir)?;
    let provisional = provisional_path(capture_dir, &record.id);
    let frame_bytes = input.format.width as usize * input.format.height as usize * 4;
    // A provisional that survives a dead attempt is the session's record: a
    // resumed worker finalizes it instead of re-recording — a live source
    // cannot be rewound. Attempts never share a spool name, so an existing
    // file on a fresh attempt can only come from a previous attempt of this
    // same job.
    let frames = if provisional.exists() {
        resume_spool_frames(&provisional, frame_bytes)?
    } else {
        let mut source = open_source(input)?;
        let mut spool = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&provisional)
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    ServiceError::new("OUTPUT_EXISTS", "capture provisional already exists")
                } else {
                    e.into()
                }
            })?;
        match record_frames(store, record, input, &mut *source, &mut spool) {
            Ok(frames) => frames,
            Err(error) => {
                // The provisional stays as a typed orphan for `capture.status`;
                // only a publication or an explicitly empty spool removes it.
                if error.code == "CAPTURE_EMPTY" {
                    let _ = std::fs::remove_file(&provisional);
                }
                return Err(error);
            }
        }
    };
    finalize_capture(
        store,
        record,
        input,
        &runtime,
        &provisional,
        frames,
        frame_bytes,
    )
}

/// Whole frames recovered from a previous attempt's provisional spool. A torn
/// tail write is truncated to the frame boundary; an empty spool fails the
/// session as `CAPTURE_EMPTY` and is removed.
fn resume_spool_frames(provisional: &Path, frame_bytes: usize) -> Result<u64, ServiceError> {
    let length = std::fs::metadata(provisional)?.len();
    let frames = length / frame_bytes as u64;
    let torn = length % frame_bytes as u64;
    if torn != 0 {
        let file = std::fs::OpenOptions::new().write(true).open(provisional)?;
        file.set_len(length - torn)?;
        file.sync_all()?;
    }
    if frames == 0 {
        let _ = std::fs::remove_file(provisional);
        return Err(ServiceError::new(
            "CAPTURE_EMPTY",
            "capture ended without recording a frame",
        ));
    }
    Ok(frames)
}

fn record_frames(
    store: &JobStore,
    record: &JobRecord,
    input: &CaptureJobInput,
    source: &mut dyn FrameProducer,
    spool: &mut std::fs::File,
) -> Result<u64, ServiceError> {
    let mut frames = 0u64;
    loop {
        // `stop.request` is the graceful end-of-input signal: finish and
        // publish. `cancel` is an abort and aborts mid-spool via checkpoint.
        if store.stop_requested(&record.id)? {
            break;
        }
        if input.max_frames.is_some_and(|max| frames >= max) {
            break;
        }
        match source.next()? {
            Produced::Frame(frame) => {
                spool.write_all(&frame.rgba)?;
                frames += 1;
                if frames.is_multiple_of(16) {
                    spool.sync_data()?;
                }
            }
            // A quiet source still reports progress heartbeats and observes
            // cancel: the checkpoint is the worker's cancel observation point.
            Produced::Idle => {}
        }
        // Bounded SQLite progress/cancel check per poll, matching existing
        // job checkpoint cadence. `sync_data` persists periodically so a dead
        // worker leaves a recoverable spool rather than empty bytes.
        store.checkpoint(&record.id, frames)?;
    }
    spool.sync_all()?;
    if frames == 0 {
        return Err(ServiceError::new(
            "CAPTURE_EMPTY",
            "capture ended without recording a frame",
        ));
    }
    Ok(frames)
}

/// Finalization boundary: re-read the provisional spool, encode the delivered
/// frames through the bounded media path, verify the container, hash the bytes
/// and register the asset — only then atomically publish.
fn finalize_capture(
    store: &JobStore,
    record: &JobRecord,
    input: &CaptureJobInput,
    runtime: &MediaRuntime,
    provisional: &Path,
    frames: u64,
    frame_bytes: usize,
) -> Result<(), ServiceError> {
    let rate = input.format.frame_rate.as_rational();
    // One container tick per delivered frame; the declared interval is the
    // frame duration (e.g. 1001/30000 s for 29.97 fps).
    let time_base = Rational::new(rate.denominator(), rate.numerator())
        .map_err(|e| ServiceError::invalid(e.to_string()))?;
    let staging = store.staging(record)?;
    let stage_path = staging.output();
    let mut spool = std::fs::File::open(provisional)?;
    let mut position = 0u64;
    let count = usize::try_from(frames)
        .map_err(|_| ServiceError::new("RESOURCE_LIMIT", "capture frame count overflow"))?;
    let report = runtime.encode_video_stream(
        &EncodeRequest {
            output: stage_path.clone(),
            codec: EncodeCodec::ProRes,
            width: input.format.width,
            height: input.format.height,
            time_base,
        },
        count,
        &mut |index| {
            spool.seek(SeekFrom::Start(position))?;
            position += frame_bytes as u64;
            let mut rgba = vec![0u8; frame_bytes];
            std::io::Read::read_exact(&mut spool, &mut rgba)?;
            Ok(EncodeFrame {
                pts: Rational::new(
                    (index as i64).checked_mul(rate.denominator()).ok_or(
                        kronello_media::MediaError::InvalidInput("pts overflow".into()),
                    )?,
                    rate.numerator(),
                )
                .map_err(|e| kronello_media::MediaError::InvalidInput(e.to_string()))?,
                rgba,
            })
        },
    )?;
    let probe = runtime
        .probe(&stage_path)
        .map_err(|e| ServiceError::new("OUTPUT_VALIDATION_FAILED", e.to_string()))?;
    let video = probe
        .streams
        .iter()
        .find(|s| s.kind == kronello_media::StreamKind::Video)
        .ok_or_else(|| ServiceError::new("OUTPUT_VALIDATION_FAILED", "no capture video stream"))?;
    if video.width != Some(input.format.width) || video.height != Some(input.format.height) {
        return Err(ServiceError::new(
            "OUTPUT_VALIDATION_FAILED",
            "capture stream dimensions differ",
        ));
    }
    let expected = time_base
        .checked_mul(Rational::from_integer(frames as i64))
        .map_err(|e| ServiceError::invalid(e.to_string()))?;
    if let Some(duration) = video.duration {
        let tolerance = time_base
            .checked_mul(Rational::from_integer(2))
            .map_err(|e| ServiceError::invalid(e.to_string()))?;
        let difference = duration
            .checked_sub(expected)
            .map_err(|e| ServiceError::invalid(e.to_string()))?;
        if difference > tolerance
            || difference
                < tolerance
                    .checked_neg()
                    .map_err(|e| ServiceError::invalid(e.to_string()))?
        {
            return Err(ServiceError::new(
                "OUTPUT_VALIDATION_FAILED",
                format!("capture duration {duration:?} differs from recorded {expected:?}"),
            ));
        }
    }
    let content_hash = kronello_media::content_hash(&stage_path)
        .map_err(|e| ServiceError::new("OUTPUT_VALIDATION_FAILED", e.to_string()))?;
    let report_json = serde_json::json!({
        "capture_asset_id": input.capture_asset_id,
        "source": input.source,
        "width": video.width,
        "height": video.height,
        "frames": frames,
        "duration": video.duration,
        "content_hash": content_hash,
        "encoder": report.encoder,
    });
    let outcome = serde_json::json!({"destination": record.destination, "validated": true, "report": report_json});
    store.prepare_publication(record, &stage_path, outcome.clone())?;
    let asset = capture_asset_object(input, &content_hash, video)?;
    register_capture_asset(input, &asset)?;
    store.publish_attempt_frames(record.id.as_str(), record.attempt, frames, outcome, || {
        kronello_jobs::publish_path(&stage_path, &record.destination)
    })?;
    // Best effort: a stale provisional is reported as an orphan, never reused.
    if let Err(error) = std::fs::remove_file(provisional) {
        eprintln!(
            "capture provisional cleanup failed job={} path={}: {error}",
            record.id,
            provisional.display()
        );
    }
    Ok(())
}

/// The registered `Asset` mirrors the published file: the container probe
/// supplies stream metadata, the hash covers exactly the published bytes.
fn capture_asset_object(
    input: &CaptureJobInput,
    content_hash: &str,
    video: &kronello_media::MediaStream,
) -> Result<Asset, ServiceError> {
    let base = input.project.parent().unwrap_or(Path::new("."));
    let relative = input
        .destination
        .strip_prefix(base)
        .ok()
        .map(|p| p.to_string_lossy().into_owned());
    Ok(Asset {
        id: input.capture_asset_id,
        content_hash: content_hash.into(),
        kind: AssetKind::Video,
        streams: vec![StreamMetadata {
            index: video.index,
            codec: video.codec.clone(),
            time_base: video.time_base,
            duration: video.duration,
            start_time: video.start,
            width: video.width,
            height: video.height,
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

/// Register the finished capture through the store's normal import path,
/// retried across concurrent edits like proxy/scene registration.
fn register_capture_asset(input: &CaptureJobInput, asset: &Asset) -> Result<(), ServiceError> {
    let mut delay_ms = 0u64;
    for attempt in 0..64u32 {
        match register_capture_asset_once(input, asset) {
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
        "capture asset registration exhausted retries",
    ))
}

fn register_capture_asset_once(input: &CaptureJobInput, asset: &Asset) -> Result<(), ServiceError> {
    let mut store = open_existing(&input.project)?;
    let result = (|| {
        let snapshot = store.snapshot()?;
        let mut document = snapshot.document;
        document
            .ensure_editable()
            .map_err(|e| ServiceError::new("UNSUPPORTED_FEATURE", e.to_string()))?;
        if let Some(existing) = crate::proxy::known_asset(&document, asset.id) {
            // Registration retries are idempotent only for the identical
            // asset; a different object under the same id is a conflict.
            if existing == asset {
                return Ok(());
            }
            return Err(ServiceError::new(
                "CAPTURE_ASSET_CONFLICT",
                "asset id already registered with different content",
            ));
        }
        document.assets.push(DocumentObject::Known(asset.clone()));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_source_is_deterministic_and_opaque() {
        let format = CaptureFormat {
            width: 16,
            height: 8,
            frame_rate: FrameRate::new(24, 1).unwrap(),
            codec: CaptureCodec::ProRes,
            color: CaptureColor::Bt709,
        };
        let mut a = SyntheticSource::new(&format);
        let mut b = SyntheticSource::new(&format);
        let Produced::Frame(first_a) = a.next().unwrap() else {
            panic!("synthetic frame")
        };
        let Produced::Frame(first_b) = b.next().unwrap() else {
            panic!("synthetic frame")
        };
        assert_eq!(first_a.rgba, first_b.rgba, "frame 0 must be deterministic");
        assert_eq!(first_a.rgba.len(), 16 * 8 * 4);
        assert!(first_a.rgba.chunks_exact(4).all(|p| p[3] == 255));
        let Produced::Frame(second) = a.next().unwrap() else {
            panic!("synthetic frame")
        };
        assert_ne!(first_a.rgba, second.rgba, "frames differ by index");
    }
}
