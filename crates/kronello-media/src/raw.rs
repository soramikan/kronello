//! Camera RAW boundary (ADR-0136): detection-first routing, LibRaw stills,
//! CinemaDNG image sequences, macOS ProRes RAW, and typed vendor rejection.
//!
//! Format ownership is exclusive: detected BRAW/R3D/ProRes RAW files never
//! reach FFmpeg, and detected camera RAW stills never reach the PNG decoder.
//! Unsupported variants fail with `UNSUPPORTED_FEATURE`, never silently.
use crate::{MediaError, content_hash, locate_asset};
use kronello_model::{Asset, AssetKind, ColorSpace, StreamMetadata};
use kronello_render::VideoImage;
use kronello_time::Rational;
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Locked codec id for CinemaDNG frame-sequence video streams (ADR-0136).
pub const CINEMADNG_CODEC: &str = "cinemadng";
/// Locked codec ids for ProRes RAW video streams.
pub const PRORES_RAW_CODECS: &[&str] = &["prores_raw", "prores_raw_hq"];
/// Native pixel-format label locked for LibRaw Bayer outputs.
pub const RAW_PIXEL_FORMAT_BAYER: &str = "bayer16";
pub const RAW_PIXEL_FORMAT_XTRANS: &str = "xtrans16";
pub const RAW_PIXEL_FORMAT_LINEAR: &str = "raw16";
/// Decoded ProRes RAW pixel buffer format (half-float RGBA64).
pub const PRORES_RAW_PIXEL_FORMAT: &str = "rgba64h";
/// Decoded RAW surface budget: 512 MiB of RGB48 output (~89 MP).
const RAW_MAX_DECODED_BYTES: u64 = 512 * 1024 * 1024;
const CINEMADNG_MAX_FRAMES: usize = 1 << 20;
const LIBRAW_CAPS_ZLIB: u32 = 1 << 6;
const LIBRAW_CAPS_JPEG: u32 = 1 << 7;
const LIBRAW_CAPS_DNGSDK: u32 = 1 << 1;

/// TIFF-based camera stills and DNG extensions LibRaw may decode.
const RAW_STILL_EXTENSIONS: &[&str] = &[
    "3fr", "arw", "bay", "bmq", "cap", "cine", "cr2", "cr3", "crw", "cs1", "dc2", "dcr", "dng",
    "drf", "dsc", "erf", "fff", "gpr", "iiq", "k25", "kc2", "kdc", "mdc", "mef", "mfw", "mos",
    "mrw", "nef", "nrw", "orf", "pef", "ptk", "pxn", "qtk", "raf", "raw", "rw2", "rwl", "rwz",
    "sr2", "srf", "srw", "sti", "x3f",
];

/// Exclusive detection of camera media before any generic decoder is allowed
/// to claim the file. Codec strings become `StreamMetadata.codec` locks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraRawDetection {
    /// Camera RAW still frame decodable through the pinned LibRaw pipeline.
    LibRawStill(&'static str),
    /// ProRes RAW inside a QuickTime/ISO-BMFF container (`aprn`/`aprh`).
    ProResRaw(&'static str),
    /// Blackmagic RAW requires the proprietary Blackmagic SDK.
    Braw,
    /// RED R3D requires the proprietary RED SDK.
    R3d,
}
impl CameraRawDetection {
    /// Locked `StreamMetadata.codec` this detection maps to, if decodable.
    pub fn codec(self) -> Option<&'static str> {
        match self {
            Self::LibRawStill(codec) | Self::ProResRaw(codec) => Some(codec),
            Self::Braw | Self::R3d => None,
        }
    }
    /// Typed boundary error used whenever this detection reaches a generic path.
    pub fn unsupported(self) -> MediaError {
        match self {
            Self::LibRawStill(codec) => MediaError::UnsupportedFeature(format!(
                "camera RAW still '{codec}' decodes through the LibRaw image path, not FFmpeg video"
            )),
            Self::ProResRaw(codec) => MediaError::UnsupportedFeature(format!(
                "ProRes RAW '{codec}' requires the macOS VideoToolbox ProRes RAW path"
            )),
            Self::Braw => MediaError::UnsupportedFeature(
                "BRAW requires the proprietary Blackmagic SDK; vendored FFmpeg/LibRaw do not decode it"
                    .into(),
            ),
            Self::R3d => MediaError::UnsupportedFeature(
                "R3D requires the proprietary RED SDK; vendored FFmpeg/LibRaw do not decode it"
                    .into(),
            ),
        }
    }
}

/// Codec ids a `AssetKind::Image` stream may carry through the LibRaw path.
pub fn is_raw_still_codec(codec: &str) -> bool {
    codec == "dng" || RAW_STILL_EXTENSIONS.contains(&codec)
}
/// Codec ids a `AssetKind::Video` stream may carry outside FFmpeg decode.
pub fn is_camera_raw_video_codec(codec: &str) -> bool {
    codec == CINEMADNG_CODEC || PRORES_RAW_CODECS.contains(&codec)
}

fn read_prefix(path: &Path, bytes: &mut [u8]) -> Result<usize, MediaError> {
    let mut file = std::fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(MediaError::InvalidInput(
            "media sniff requires a regular file".into(),
        ));
    }
    Ok(file.read(bytes)?)
}

/// Detection-only container sniff. Returns the exclusive owner classification
/// or `None` for generic media the sniff does not claim. Reads a bounded prefix
/// plus targeted box walks; never decodes pixels.
pub fn sniff_camera_raw(path: &Path) -> Result<Option<CameraRawDetection>, MediaError> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    let mut head = vec![0u8; 8192];
    let len = read_prefix(path, &mut head)?;
    let head = &head[..len];

    // Vendor formats: extension or magic wins immediately; these files must
    // never be attempted by FFmpeg, LibRaw, or the PNG decoder.
    if extension.as_deref() == Some("braw") || head.starts_with(b"PK\x03\x04") && is_braw_zip(head)
    {
        return Ok(Some(CameraRawDetection::Braw));
    }
    if extension.as_deref() == Some("r3d") || head.starts_with(b"RED1") || head.starts_with(b"RED2")
    {
        return Ok(Some(CameraRawDetection::R3d));
    }
    // ISO-BMFF / QuickTime: only the native macOS path may decode ProRes RAW,
    // and CR3 is claimed by LibRaw only after its brand check.
    if (head.len() >= 12 && &head[4..8] == b"ftyp"
        || matches!(extension.as_deref(), Some("mov" | "mp4" | "m4v" | "qt")))
        && let Some(codec) = mov_video_codec(path)?
    {
        return Ok(match &codec {
            b"aprn" => Some(CameraRawDetection::ProResRaw("prores_raw")),
            b"aprh" => Some(CameraRawDetection::ProResRaw("prores_raw_hq")),
            b"crx " => Some(CameraRawDetection::LibRawStill("cr3")),
            _ => None,
        });
    }
    // TIFF family: DNG (DNGVersion tag), CR2 (CR\x02\x00 marker), or a claimed
    // camera-raw extension. LibRaw is the arbiter of decode support.
    let tiff = head.starts_with(b"II*\0") || head.starts_with(b"MM\0*");
    if tiff {
        if head.len() >= 10 && &head[8..10] == b"CR\x02\x00" {
            return Ok(Some(CameraRawDetection::LibRawStill("cr2")));
        }
        if let Some(scan) = scan_tiff(path)?
            && scan.is_dng
        {
            return Ok(Some(CameraRawDetection::LibRawStill("dng")));
        }
        // A camera-raw extension claims the LibRaw path even when the scan is
        // inconclusive; LibRaw's open verdict stays typed and deterministic.
        return Ok(match extension.as_deref() {
            Some(ext) if is_raw_still_codec(ext) => {
                Some(CameraRawDetection::LibRawStill(canonical_raw_codec(ext)))
            }
            _ => None,
        });
    }
    if head.starts_with(b"FUJIFILMCCD-RAW") {
        return Ok(Some(CameraRawDetection::LibRawStill("raf")));
    }
    if head.starts_with(b"FOVb") {
        return Ok(Some(CameraRawDetection::LibRawStill("x3f")));
    }
    if head.starts_with(b"IIII") || head.starts_with(b"MMMM") {
        return Ok(Some(CameraRawDetection::LibRawStill("iiq")));
    }
    if head.len() >= 14 && &head[6..14] == b"HEAPCCDR" {
        return Ok(Some(CameraRawDetection::LibRawStill("crw")));
    }
    if let Some(ext) = extension.as_deref()
        && is_raw_still_codec(ext)
    {
        return Ok(Some(CameraRawDetection::LibRawStill(canonical_raw_codec(
            ext,
        ))));
    }
    Ok(None)
}

/// Stable codec ids collapsed onto vendor-family names. The input extension
/// is first pinned to a `RAW_STILL_EXTENSIONS` member so the result is always
/// `'static` (the enum stores it as the locked codec id).
fn canonical_raw_codec(ext: &str) -> &'static str {
    let ext = RAW_STILL_EXTENSIONS
        .iter()
        .copied()
        .find(|candidate| *candidate == ext)
        .unwrap_or("raw");
    match ext {
        "3fr" | "fff" => "fff",
        "arw" | "sr2" | "srf" => "arw",
        "nef" | "nrw" => "nef",
        "kdc" | "dcr" | "k25" | "kc2" => "kdc",
        "dng" | "gpr" => "dng",
        "raf" | "rwz" => "raf",
        "rw2" | "rwl" | "raw" => "rw2",
        "orf" => "orf",
        "pef" | "ptk" => "pef",
        "cr2" => "cr2",
        "cr3" => "cr3",
        "crw" => "crw",
        "srw" => "srw",
        "x3f" => "x3f",
        "erf" => "erf",
        "mef" => "mef",
        "mos" => "mos",
        "mrw" => "mrw",
        "iiq" => "iiq",
        other => other,
    }
}

/// BRAW archives are ZIP containers whose entries carry Blackmagic markers.
fn is_braw_zip(head: &[u8]) -> bool {
    head.windows(4)
        .any(|w| w == b"braw" || w == b"BRAW" || w == b"BrRa")
}

// ---------------------------------------------------------------------------
// TIFF/DNG scan: bounded IFD walk for detection and the compression gate.

#[derive(Debug, Clone, Default)]
pub struct TiffScan {
    pub is_dng: bool,
    /// Compression tag of the RAW (CFA or largest) IFD.
    pub compression: Option<u16>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub bits_per_sample: Option<u16>,
    pub photometric: Option<u16>,
}

fn read_exact_at(file: &mut std::fs::File, offset: u64, buf: &mut [u8]) -> Result<(), MediaError> {
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(buf)?;
    Ok(())
}

/// Bounded TIFF/DNG metadata scan used by detection, compression gating and
/// `media.probe` reporting. `None` means the file is not TIFF at all.
pub fn scan_tiff_metadata(path: &Path) -> Result<Option<TiffScan>, MediaError> {
    scan_tiff(path)
}

fn scan_tiff(path: &Path) -> Result<Option<TiffScan>, MediaError> {
    let mut file = std::fs::File::open(path)?;
    let mut header = [0u8; 8];
    file.read_exact(&mut header)?;
    let le = match &header[..4] {
        [b'I', b'I', 42, 0] => true,
        [b'M', b'M', 0, 42] => false,
        _ => return Ok(None),
    };
    let mut scan = TiffScan::default();
    let mut best_area = 0u64;
    let ifd0 = if le {
        u64::from(u32::from_le_bytes(header[4..8].try_into().unwrap()))
    } else {
        u64::from(u32::from_be_bytes(header[4..8].try_into().unwrap()))
    };
    visit_ifd(&mut file, le, ifd0, 0, &mut scan, &mut best_area)?;
    Ok(Some(scan))
}

fn u16v(le: bool, b: &[u8]) -> u16 {
    if le {
        u16::from_le_bytes([b[0], b[1]])
    } else {
        u16::from_be_bytes([b[0], b[1]])
    }
}
fn u32v(le: bool, b: &[u8]) -> u32 {
    if le {
        u32::from_le_bytes([b[0], b[1], b[2], b[3]])
    } else {
        u32::from_be_bytes([b[0], b[1], b[2], b[3]])
    }
}

fn ifd_entry_value(
    file: &mut std::fs::File,
    le: bool,
    entry: &[u8],
    index: usize,
) -> Result<Option<u32>, MediaError> {
    let typ = u16v(le, &entry[2..4]);
    let count = u64::from(u32v(le, &entry[4..8]));
    let typesize = u64::from(match typ {
        1 | 2 | 6 | 7 => 1u32,
        3 | 8 => 2,
        4 | 9 | 11 | 13 => 4,
        5 | 10 | 12 => 8,
        _ => return Ok(None),
    });
    let total = typesize
        .checked_mul(count)
        .ok_or_else(|| MediaError::InvalidInput("TIFF tag size overflow".into()))?;
    if index as u64 >= count || typesize > 4 {
        return Ok(None);
    }
    let at = index as u64 * typesize;
    if total <= 4 {
        // Values pack inline into the 4-byte value/offset field.
        let inline = &entry[8..12];
        let at = usize::try_from(at).unwrap_or(usize::MAX);
        let n = usize::try_from(typesize).unwrap_or(4);
        return Ok(match (inline.get(at..at + n), typesize) {
            (Some(v), 1) => Some(u32::from(v[0])),
            (Some(v), 2) => Some(u32::from(u16v(le, v))),
            (Some(v), _) => Some(u32v(le, v)),
            (None, _) => None,
        });
    }
    if total > 4096 {
        return Ok(None);
    }
    let base = u64::from(u32v(le, &entry[8..12]));
    let absolute = base
        .checked_add(at)
        .ok_or_else(|| MediaError::InvalidInput("TIFF tag offset overflow".into()))?;
    let mut buf = [0u8; 4];
    read_exact_at(file, absolute, &mut buf[..typesize as usize])?;
    Ok(Some(match typesize {
        1 => u32::from(buf[0]),
        2 => u32::from(u16v(le, &buf[..2])),
        _ => u32v(le, &buf[..4]),
    }))
}

fn visit_ifd(
    file: &mut std::fs::File,
    le: bool,
    offset: u64,
    depth: u32,
    scan: &mut TiffScan,
    best_area: &mut u64,
) -> Result<u64, MediaError> {
    if offset == 0 || depth > 8 {
        return Ok(0);
    }
    let mut count_buf = [0u8; 2];
    if read_exact_at(file, offset, &mut count_buf).is_err() {
        return Ok(0);
    }
    let count = u16v(le, &count_buf);
    if count > 1024 {
        return Ok(0);
    }
    let table_len = u64::from(count)
        .checked_mul(12)
        .and_then(|v| v.checked_add(2))
        .ok_or_else(|| MediaError::InvalidInput("TIFF IFD size overflow".into()))?;
    let mut table = vec![0u8; table_len as usize];
    if read_exact_at(file, offset, &mut table).is_err() {
        return Ok(0);
    }
    let next_at = offset + table_len;
    let mut next_buf = [0u8; 4];
    if read_exact_at(file, next_at, &mut next_buf).is_err() {
        return Ok(0);
    }
    let mut photo = None;
    let mut width = None;
    let mut height = None;
    let mut compression = None;
    let mut bps = None;
    let mut subfile_type = None;
    for i in 0..count as usize {
        let entry = &table[2 + i * 12..14 + i * 12];
        let tag = u16v(le, &entry[0..2]);
        let mut value = |index| ifd_entry_value(file, le, entry, index);
        match tag {
            254 => subfile_type = value(0)?,
            256 => width = value(0)?,
            257 => height = value(0)?,
            258 => bps = value(0)?.map(|v| v as u16),
            259 => compression = value(0)?.map(|v| v as u16),
            262 => photo = value(0)?.map(|v| v as u16),
            // DNGVersion is BYTE[4] packed inline; a nonzero major marks DNG.
            50706 if entry[8] != 0 => scan.is_dng = true,
            _ => {}
        }
    }
    let is_cfa = photo == Some(32803);
    let area = u64::from(width.unwrap_or(0)) * u64::from(height.unwrap_or(0));
    // RAW IFD selection: the CFA IFD wins; otherwise the largest subfile-0 IFD.
    if area > 0 && (is_cfa || (subfile_type == Some(0) && area > *best_area)) {
        *best_area = area.max(*best_area);
        scan.width = width;
        scan.height = height;
        scan.compression = compression;
        scan.bits_per_sample = bps;
        scan.photometric = photo;
    }
    // Recurse into SubIFDs (tag 330) which commonly carry the RAW data.
    for i in 0..count as usize {
        let entry = &table[2 + i * 12..14 + i * 12];
        if u16v(le, &entry[0..2]) == 330 {
            let n = u32v(le, &entry[4..8]).min(16);
            for j in 0..n {
                if let Some(sub) = ifd_entry_value(file, le, entry, j as usize)? {
                    visit_ifd(file, le, u64::from(sub), depth + 1, scan, best_area)?;
                }
            }
        }
    }
    visit_ifd(
        file,
        le,
        u64::from(u32v(le, &next_buf)),
        depth + 1,
        scan,
        best_area,
    )
}

/// Gate DNG compression against the linked LibRaw's advertised capabilities.
/// Under the vendored build (zlib/JPEG disabled) every compressed DNG fails
/// here with UNSUPPORTED_FEATURE before LibRaw is asked to decode it.
fn gate_dng_compression(path: &Path) -> Result<(), MediaError> {
    let Some(scan) = scan_tiff(path)? else {
        return Ok(());
    };
    if !scan.is_dng {
        return Ok(());
    }
    let caps = libraw_capabilities();
    match scan.compression.unwrap_or(1) {
        1 => Ok(()),
        7 | 34892 if caps & LIBRAW_CAPS_JPEG != 0 => Ok(()),
        8 if caps & LIBRAW_CAPS_ZLIB != 0 => Ok(()),
        52546 | 52547 if caps & LIBRAW_CAPS_DNGSDK != 0 => Ok(()),
        7 | 34892 => Err(MediaError::UnsupportedFeature(
            "compressed DNG (JPEG) requires LibRaw JPEG support; the vendored build disables it"
                .into(),
        )),
        8 => Err(MediaError::UnsupportedFeature(
            "compressed DNG (deflate) requires LibRaw zlib support; the vendored build disables it"
                .into(),
        )),
        52546 | 52547 => Err(MediaError::UnsupportedFeature(
            "compressed DNG (JPEG XL) requires the Adobe DNG SDK; unsupported".into(),
        )),
        other => Err(MediaError::UnsupportedFeature(format!(
            "DNG compression {other} unsupported"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Minimal QuickTime/ISO-BMFF walk used for ProRes RAW detection and metadata.

#[derive(Debug, Clone, Default)]
pub struct MovVideoInfo {
    pub codec: [u8; 4],
    pub width: u32,
    pub height: u32,
    pub timescale: u32,
    pub duration: u64,
    pub frame_count: u64,
    pub primaries: Option<u16>,
    pub transfer: Option<u16>,
    pub matrix: Option<u16>,
    pub full_range: Option<bool>,
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes(b.try_into().unwrap_or([0; 4]))
}
fn be64(b: &[u8]) -> u64 {
    u64::from_be_bytes(b.try_into().unwrap_or([0; 8]))
}
fn be16(b: &[u8]) -> u16 {
    u16::from_be_bytes(b.try_into().unwrap_or([0; 2]))
}

/// Box visitor: `(file, fourcc, payload_offset, payload_len)`.
type BoxVisitor<'a> =
    dyn FnMut(&mut std::fs::File, &[u8; 4], u64, u64) -> Result<(), MediaError> + 'a;

/// Walk boxes in `range` invoking `f(type, payload_offset, payload_len)`.
fn walk_boxes(
    file: &mut std::fs::File,
    start: u64,
    end: u64,
    f: &mut BoxVisitor<'_>,
) -> Result<(), MediaError> {
    let mut at = start;
    while at + 8 <= end {
        let mut head = [0u8; 16];
        read_exact_at(file, at, &mut head)?;
        let mut size = u64::from(be32(&head[0..4]));
        let four = [head[4], head[5], head[6], head[7]];
        let mut payload = at + 8;
        if size == 1 {
            size = be64(&head[8..16]);
            payload = at + 16;
        } else if size == 0 {
            size = end - at;
        }
        if size < 8 || payload + (size - (payload - at)) > end + 8 {
            break;
        }
        let payload_len = size.saturating_sub(payload - at);
        f(file, &four, payload, payload_len)?;
        at = match at.checked_add(size) {
            Some(next) if next > at => next,
            _ => break,
        };
    }
    Ok(())
}

/// Extract the first video track's sample description codec (`stsd` entry).
/// Also captures dimensions, `mdhd` timing, `stts` frame count and `colr`.
fn mov_video_info(path: &Path) -> Result<Option<MovVideoInfo>, MediaError> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    if len < 16 {
        return Ok(None);
    }
    let mut head = [0u8; 16];
    read_exact_at(&mut file, 0, &mut head)?;
    let is_bmff = &head[4..8] == b"ftyp" || {
        // A bare `moov`/`mdat`/`wide` first box still identifies QuickTime.
        matches!(&head[4..8], b"moov" | b"mdat" | b"wide" | b"free")
    };
    if !is_bmff {
        return Ok(None);
    }
    let mut info = MovVideoInfo::default();
    let mut found = false;
    let file_len = len;
    walk_boxes(&mut file, 0, file_len, &mut |file, four, payload, plen| {
        if four != b"moov" {
            return Ok(());
        }
        walk_boxes(
            file,
            payload,
            payload + plen,
            &mut |file, four, payload, plen| {
                if four != b"trak" {
                    return Ok(());
                }
                let mut video = false;
                let mut local = MovVideoInfo::default();
                walk_boxes(
                    file,
                    payload,
                    payload + plen,
                    &mut |file, four, payload, plen| {
                        if four != b"mdia" {
                            return Ok(());
                        }
                        walk_boxes(
                            file,
                            payload,
                            payload + plen,
                            &mut |file, four, payload, plen| {
                                match four {
                                    b"mdhd" if plen >= 24 => {
                                        let mut buf = [0u8; 32];
                                        read_exact_at(
                                            file,
                                            payload,
                                            &mut buf[..plen.min(32) as usize],
                                        )?;
                                        if buf[0] == 1 && plen >= 32 {
                                            local.timescale = be32(&buf[20..24]);
                                            local.duration = be64(&buf[24..32]);
                                        } else {
                                            local.timescale = be32(&buf[12..16]);
                                            local.duration = u64::from(be32(&buf[16..20]));
                                        }
                                        Ok(())
                                    }
                                    b"hdlr" if plen >= 12 => {
                                        let mut buf = [0u8; 12];
                                        read_exact_at(file, payload, &mut buf)?;
                                        video = &buf[8..12] == b"vide";
                                        Ok(())
                                    }
                                    b"minf" => {
                                        walk_boxes(
                                            file,
                                            payload,
                                            payload + plen,
                                            &mut |file, four, payload, plen| {
                                                if four != b"stbl" {
                                                    return Ok(());
                                                }
                                                walk_boxes(
                                                    file,
                                                    payload,
                                                    payload + plen,
                                                    &mut |file, four, payload, plen| {
                                                        match four {
                                                            b"stsd" if plen >= 8 => {
                                                                let mut buf = [0u8; 8];
                                                                read_exact_at(
                                                                    file, payload, &mut buf,
                                                                )?;
                                                                let entries = be32(&buf[4..8]);
                                                                // stsd: version+flags(4), entry_count(4),
                                                                // then entries — each ≥86 bytes for video.
                                                                if entries == 0 || plen < 8 + 86 {
                                                                    return Ok(());
                                                                }
                                                                let entry = payload + 8;
                                                                let mut ehead = [0u8; 86];
                                                                read_exact_at(
                                                                    file, entry, &mut ehead,
                                                                )?;
                                                                local.codec = [
                                                                    ehead[4], ehead[5], ehead[6],
                                                                    ehead[7],
                                                                ];
                                                                // VisualSampleEntry: width/height at +32/+34.
                                                                local.width =
                                                                    u32::from(be16(&ehead[32..34]));
                                                                local.height =
                                                                    u32::from(be16(&ehead[34..36]));
                                                                // Sample-entry sub-atoms start at +86 for video entries.
                                                                let sub_start = entry + 86;
                                                                let entry_size =
                                                                    u64::from(be32(&ehead[0..4]));
                                                                let sub_end = entry
                                                                    .saturating_add(entry_size)
                                                                    .min(
                                                                        payload
                                                                            .saturating_add(plen),
                                                                    );
                                                                if entry_size >= 8
                                                                    && sub_end > sub_start
                                                                {
                                                                    let _ = walk_boxes(file, sub_start, sub_end, &mut |file, four, payload, plen| {
                                                    if four == b"colr" && plen >= 11 {
                                                        let mut buf = [0u8; 24];
                                                        read_exact_at(file, payload, &mut buf[..plen.min(24) as usize])?;
                                                        if &buf[0..4] == b"nclx" || &buf[0..4] == b"nclc" {
                                                            local.primaries = Some(be16(&buf[4..6]));
                                                            local.transfer = Some(be16(&buf[6..8]));
                                                            local.matrix = Some(be16(&buf[8..10]));
                                                            if &buf[0..4] == b"nclx" && plen >= 11 {
                                                                local.full_range = Some(buf[10] & 0x80 != 0);
                                                            }
                                                        }
                                                    }
                                                    Ok(())
                                                });
                                                                }
                                                            }
                                                            b"stts" if plen >= 8 => {
                                                                let mut buf = [0u8; 8];
                                                                read_exact_at(
                                                                    file, payload, &mut buf,
                                                                )?;
                                                                let count =
                                                                    be32(&buf[4..8]).min(4096);
                                                                for i in 0..count {
                                                                    let mut e = [0u8; 8];
                                                                    read_exact_at(
                                                                        file,
                                                                        payload
                                                                            + 8
                                                                            + u64::from(i) * 8,
                                                                        &mut e,
                                                                    )?;
                                                                    local.frame_count +=
                                                                        u64::from(be32(&e[0..4]));
                                                                }
                                                            }
                                                            _ => {}
                                                        }
                                                        Ok(())
                                                    },
                                                )
                                            },
                                        )
                                    }
                                    _ => Ok(()),
                                }
                            },
                        )
                    },
                )?;
                if video && !found && local.codec != [0; 4] {
                    info = local;
                    found = true;
                }
                Ok(())
            },
        )
    })?;
    Ok(found.then_some(info))
}

fn mov_video_codec(path: &Path) -> Result<Option<[u8; 4]>, MediaError> {
    Ok(mov_video_info(path)?.map(|i| i.codec))
}

/// Full ProRes RAW metadata probe for `media.probe`. Returns `None` when the
/// first video track is not `aprn`/`aprh`.
pub fn probe_prores_raw_container(path: &Path) -> Result<Option<MovVideoInfo>, MediaError> {
    Ok(mov_video_info(path)?.filter(|i| i.codec == *b"aprn" || i.codec == *b"aprh"))
}

fn colr_primaries(code: Option<u16>) -> Option<String> {
    match code {
        Some(1) => Some("bt709".into()),
        Some(9) | Some(10) | Some(11) => Some("bt2020".into()),
        Some(_) => Some("unknown".into()),
        None => None,
    }
}

/// Metadata probe for a detected camera RAW file; no FFmpeg demuxer is
/// involved. The returned stream fields lock the decode contract: RAW stills
/// report one `Other` stream (they register as `AssetKind::Image`), ProRes RAW
/// reports a `Video` stream with the `rgba64h` contract tags.
pub fn probe_camera_raw(
    path: &Path,
    detection: CameraRawDetection,
) -> Result<crate::MediaProbe, MediaError> {
    use crate::{MediaProbe, MediaStream, StreamKind};
    let stream = |codec: &str, kind| -> Result<MediaStream, MediaError> {
        Ok(MediaStream {
            index: 0,
            kind,
            codec: codec.to_string(),
            time_base: Rational::new(1, 1)?,
            start: Some(Rational::ZERO),
            duration: None,
            sample_rate: None,
            channels: None,
            channel_mask: None,
            width: None,
            height: None,
            pixel_format: None,
            color_primaries: None,
            color_transfer: None,
            color_matrix: None,
            color_range: None,
        })
    };
    let probe = |streams| MediaProbe {
        streams,
        duration: None,
        render_snapshot_hash: String::new(),
        export_snapshot_hash: String::new(),
    };
    match detection {
        CameraRawDetection::Braw | CameraRawDetection::R3d => Err(detection.unsupported()),
        CameraRawDetection::LibRawStill(codec) => {
            let mut stream = stream(codec, StreamKind::Other)?;
            match probe_raw_still_inner(path) {
                Ok(info) => {
                    stream.width = Some(info.width);
                    stream.height = Some(info.height);
                    stream.pixel_format = Some(info.pixel_format.into());
                }
                // LibRaw absent or declined: TIFF-only metadata still reports
                // what the scan knows; decode itself will gate by capability.
                Err(_) => {
                    if let Some(scan) = scan_tiff(path)? {
                        stream.width = scan.width;
                        stream.height = scan.height;
                        stream.pixel_format = Some(
                            if scan.photometric == Some(32803) {
                                RAW_PIXEL_FORMAT_BAYER
                            } else {
                                RAW_PIXEL_FORMAT_LINEAR
                            }
                            .into(),
                        );
                    }
                }
            }
            // Locked contract of the pinned LibRaw pipeline output.
            stream.color_primaries = Some("bt709".into());
            stream.color_transfer = Some("linear".into());
            stream.color_matrix = Some("gbr".into());
            stream.color_range = Some("pc".into());
            Ok(probe(vec![stream]))
        }
        CameraRawDetection::ProResRaw(codec) => {
            let info = mov_video_info(path)?.ok_or_else(|| {
                MediaError::InvalidInput("ProRes RAW container lacks a video track".into())
            })?;
            let mut stream = stream(codec, StreamKind::Video)?;
            stream.width = (info.width > 0).then_some(info.width);
            stream.height = (info.height > 0).then_some(info.height);
            if info.timescale > 0 && info.duration > 0 {
                stream.time_base = Rational::new(1, i64::from(info.timescale))?;
                stream.duration = Some(Rational::new(
                    i64::try_from(info.duration)
                        .map_err(|_| MediaError::InvalidInput("duration overflow".into()))?,
                    i64::from(info.timescale),
                )?);
            }
            stream.pixel_format = Some(PRORES_RAW_PIXEL_FORMAT.into());
            stream.color_primaries = colr_primaries(info.primaries);
            stream.color_transfer = Some("linear".into());
            stream.color_matrix = Some("gbr".into());
            stream.color_range = Some("pc".into());
            Ok(probe(vec![stream]))
        }
    }
}

/// Registration-time view of a CinemaDNG frame sequence.
#[derive(Debug, Clone)]
pub struct CinemaDngProbe {
    pub manifest: CinemaDngManifest,
    pub width: u32,
    pub height: u32,
    /// `member_count * frame_duration`; locked stream duration.
    pub duration: Rational,
    /// Ordered member content hash; becomes `Asset::content_hash`.
    pub content_hash: String,
}

/// Probe a CinemaDNG frame sequence at an explicit frame interval (the files
/// carry no fps). `None` when `first` is not a numbered `.dng` frame.
/// The returned duration and content hash register a `AssetKind::Video` asset
/// with codec `cinemadng`; dimensions come from the TIFF scan of frame one.
pub fn probe_cinemadng_sequence(
    first: &Path,
    frame_duration: Rational,
) -> Result<Option<CinemaDngProbe>, MediaError> {
    let Some(manifest) = cinemadng_members(first)? else {
        return Ok(None);
    };
    if frame_duration <= Rational::ZERO {
        return Err(MediaError::InvalidInput(
            "CinemaDNG frame interval must be positive".into(),
        ));
    }
    let (width, height) = scan_tiff(&manifest.members[0])?
        .and_then(|s| s.width.zip(s.height))
        .ok_or_else(|| MediaError::InvalidInput("CinemaDNG frame lacks TIFF dimensions".into()))?;
    let duration =
        Rational::from_integer(manifest.members.len() as i64).checked_mul(frame_duration)?;
    let content_hash = cinemadng_content_hash(&manifest.members)?;
    Ok(Some(CinemaDngProbe {
        manifest,
        width,
        height,
        duration,
        content_hash,
    }))
}

// ---------------------------------------------------------------------------
// LibRaw decode wrapper.

/// Whether this build links LibRaw (vendored or system probe at build time).
pub fn libraw_available() -> bool {
    cfg!(kronello_libraw)
}
/// LibRaw capability bitmask (`LIBRAW_CAPS_*`); 0 when LibRaw is absent.
pub fn libraw_capabilities() -> u32 {
    #[cfg(kronello_libraw)]
    {
        crate::rawffi::capabilities()
    }
    #[cfg(not(kronello_libraw))]
    {
        0
    }
}
/// LibRaw version string for acceptance/probe reporting.
pub fn libraw_version() -> Option<String> {
    #[cfg(kronello_libraw)]
    {
        Some(crate::rawffi::version())
    }
    #[cfg(not(kronello_libraw))]
    {
        None
    }
}

/// Metadata mirrored out of LibRaw `open_file` (no pixels unpacked).
#[derive(Debug, Clone)]
pub struct RawStillInfo {
    pub width: u32,
    pub height: u32,
    pub pixel_format: &'static str,
    pub make: String,
    pub model: String,
    pub dng_version: u32,
    pub flip: i32,
}

fn raw_pixel_format(filters: u32, is_foveon: bool) -> &'static str {
    if filters == 9 {
        RAW_PIXEL_FORMAT_XTRANS
    } else if filters != 0 && !is_foveon {
        RAW_PIXEL_FORMAT_BAYER
    } else {
        RAW_PIXEL_FORMAT_LINEAR
    }
}

/// Copy a NUL-terminated fixed field without unsafe: bytes up to the first
/// NUL are lossily decoded; the field is always in-bounds of the struct.
#[cfg(kronello_libraw)]
fn cstr(buf: &[std::ffi::c_char]) -> String {
    let bytes: Vec<u8> = buf
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| *c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// LibRaw error codes are part of the contract: unsupported formats map to
/// UNSUPPORTED_FEATURE, everything else to DECODE_ERROR with the native text.
#[cfg(kronello_libraw)]
fn libraw_error(code: i32, stage: &str) -> MediaError {
    const FILE_UNSUPPORTED: i32 = -2;
    const TOO_BIG: i32 = -9;
    match code {
        FILE_UNSUPPORTED => MediaError::UnsupportedFeature(format!(
            "{stage}: unsupported or non-RAW camera file (LibRaw {code})"
        )),
        TOO_BIG => MediaError::UnsupportedFeature(format!(
            "{stage}: file exceeds LibRaw size limits ({code})"
        )),
        _ => MediaError::Decode(format!(
            "{stage}: LibRaw {code} {}",
            crate::rawffi::strerror(code)
        )),
    }
}

/// Metadata-only probe for registration: cheap `open_file` without unpacking.
pub fn probe_raw_still(path: &Path) -> Result<RawStillInfo, MediaError> {
    probe_raw_still_inner(path)
}

#[cfg(kronello_libraw)]
fn probe_raw_still_inner(path: &Path) -> Result<RawStillInfo, MediaError> {
    let (session, code) = crate::rawffi::RawSession::open(path).map_err(MediaError::Decode)?;
    if !session.opened() {
        return Err(libraw_error(code, "LibRaw open"));
    }
    let info = session
        .info()
        .map_err(|c| libraw_error(c, "LibRaw metadata"))?;
    if info.iwidth == 0 || info.iheight == 0 {
        return Err(MediaError::UnsupportedFeature(
            "camera RAW reports empty decoded dimensions".into(),
        ));
    }
    Ok(RawStillInfo {
        width: info.iwidth,
        height: info.iheight,
        pixel_format: raw_pixel_format(info.filters, info.is_foveon != 0),
        make: cstr(&info.make),
        model: cstr(&info.model),
        dng_version: info.dng_version,
        flip: info.flip,
    })
}

#[cfg(not(kronello_libraw))]
fn probe_raw_still_inner(_path: &Path) -> Result<RawStillInfo, MediaError> {
    Err(MediaError::UnsupportedFeature(
        "LibRaw unavailable in this build; camera RAW decode is disabled".into(),
    ))
}

#[cfg(kronello_libraw)]
fn decode_raw_frame(path: &Path) -> Result<(RawStillInfo, u32, u32, Vec<u8>), MediaError> {
    let (session, code) = crate::rawffi::RawSession::open(path).map_err(MediaError::Decode)?;
    if !session.opened() {
        return Err(libraw_error(code, "LibRaw open"));
    }
    let info = session
        .info()
        .map_err(|c| libraw_error(c, "LibRaw metadata"))?;
    if info.iwidth == 0 || info.iheight == 0 {
        return Err(MediaError::UnsupportedFeature(
            "camera RAW reports empty decoded dimensions".into(),
        ));
    }
    let budget = u64::from(info.iwidth)
        .checked_mul(u64::from(info.iheight))
        .and_then(|p| p.checked_mul(6))
        .ok_or_else(|| MediaError::InvalidInput("RAW surface size overflow".into()))?;
    if budget > RAW_MAX_DECODED_BYTES {
        return Err(MediaError::UnsupportedFeature(
            "RAW decoded surface budget exceeded".into(),
        ));
    }
    let image = session
        .process()
        .map_err(|c| libraw_error(c, "LibRaw decode"))?;
    if image.bits != 16 || image.colors != 3 {
        return Err(MediaError::UnsupportedFeature(format!(
            "pinned LibRaw pipeline produced {}-bit {}-channel output",
            image.bits, image.colors
        )));
    }
    if u64::from(image.width) * u64::from(image.height) * 6 != image.data_size {
        return Err(MediaError::Decode(
            "LibRaw RGB48 output size disagrees with dimensions".into(),
        ));
    }
    let mut pixels = vec![0u8; image.data_size as usize];
    session
        .copy_image(&mut pixels)
        .map_err(|c| libraw_error(c, "LibRaw image copy"))?;
    Ok((
        RawStillInfo {
            width: image.width,
            height: image.height,
            pixel_format: raw_pixel_format(info.filters, info.is_foveon != 0),
            make: cstr(&info.make),
            model: cstr(&info.model),
            dng_version: info.dng_version,
            flip: info.flip,
        },
        image.width,
        image.height,
        pixels,
    ))
}

/// sRGB/Rec.709 primaries -> Rec.2020, exact matrix shared with the PNG path.
fn rec709_to_rec2020(rgb: [f64; 3]) -> [f64; 3] {
    [
        0.6274039 * rgb[0] + 0.3292830 * rgb[1] + 0.0433131 * rgb[2],
        0.0690973 * rgb[0] + 0.9195404 * rgb[1] + 0.0113623 * rgb[2],
        0.0163914 * rgb[0] + 0.0880133 * rgb[1] + 0.8955953 * rgb[2],
    ]
}
fn rec2020_to_rec709(rgb: [f64; 3]) -> [f64; 3] {
    [
        1.6604910 * rgb[0] - 0.5876411 * rgb[1] - 0.0728499 * rgb[2],
        -0.1245505 * rgb[0] + 1.1328999 * rgb[1] - 0.0083494 * rgb[2],
        -0.0181508 * rgb[0] - 0.1005788 * rgb[1] + 1.1187297 * rgb[2],
    ]
}

/// Convert pinned LibRaw RGB48 output into premultiplied linear working space.
/// The LibRaw pipeline outputs linear-light sRGB-primaries samples (ADR-0136).
fn rgb48_to_working(rgb48: &[u8], working: ColorSpace) -> Result<Vec<[f32; 4]>, MediaError> {
    Ok(rgb48
        .chunks_exact(6)
        .map(|p| {
            let mut rgb =
                [0, 2, 4].map(|i| f64::from(u16::from_le_bytes([p[i], p[i + 1]])) / 65535.0);
            if working == ColorSpace::LinearRec2020 {
                rgb = rec709_to_rec2020(rgb);
            }
            [rgb[0] as f32, rgb[1] as f32, rgb[2] as f32, 1.0]
        })
        .collect())
}

/// Shared color-tag gate for the RAW contract: decoded output is always linear
/// transfer, sRGB/Rec.709 primaries, GBRA order, full range.
fn locked_raw_color_tags(stream: &StreamMetadata) -> Result<(), MediaError> {
    for (tag, allowed) in [
        (
            stream.color_primaries.as_deref(),
            &["bt709", "unknown", "unspecified"][..],
        ),
        (
            stream.color_transfer.as_deref(),
            &["linear", "unknown", "unspecified"][..],
        ),
        (
            stream.color_matrix.as_deref(),
            &["gbr", "unknown", "unspecified"][..],
        ),
        (
            stream.color_range.as_deref(),
            &["pc", "unknown", "unspecified"][..],
        ),
    ] {
        if tag.is_some_and(|tag| !allowed.contains(&tag)) {
            return Err(MediaError::UnsupportedFeature(
                "RAW locked color tags differ from the scene-referred contract".into(),
            ));
        }
    }
    Ok(())
}

/// Decode a `AssetKind::Image` camera RAW still via the pinned LibRaw pipeline.
/// Mirrors the PNG contract: hash-verified asset, codec/pixel/dimension locks,
/// deterministic scene-referred conversion into the linear working space.
pub fn decode_raw_image_asset(
    asset: &Asset,
    project_path: &Path,
    stream_index: u32,
    working: ColorSpace,
) -> Result<VideoImage, MediaError> {
    if asset.kind != AssetKind::Image || working == ColorSpace::Srgb {
        return Err(MediaError::UnsupportedFeature(
            "image asset/linear working space required".into(),
        ));
    }
    let stream = asset
        .streams
        .iter()
        .find(|s| s.index == stream_index)
        .ok_or_else(|| MediaError::InvalidInput("image stream missing".into()))?;
    if !is_raw_still_codec(&stream.codec) {
        return Err(MediaError::UnsupportedFeature(format!(
            "camera RAW codec {}",
            stream.codec
        )));
    }
    locked_raw_color_tags(stream)?;
    let path = crate::resolve_asset(asset, project_path)?;
    match sniff_camera_raw(&path)? {
        Some(CameraRawDetection::LibRawStill(codec)) if codec == stream.codec => {}
        Some(other) => {
            return Err(MediaError::InvalidInput(format!(
                "RAW content differs from locked codec {} ({other:?})",
                stream.codec
            )));
        }
        None => {
            return Err(MediaError::InvalidInput(
                "content is not a camera RAW still for the locked codec".into(),
            ));
        }
    }
    if stream.codec == "dng" {
        gate_dng_compression(&path)?;
    }
    decode_raw_file(&path, stream, working)
}

#[cfg(kronello_libraw)]
fn decode_raw_file(
    path: &Path,
    stream: &StreamMetadata,
    working: ColorSpace,
) -> Result<VideoImage, MediaError> {
    let (info, width, height, rgb48) = decode_raw_frame(path)?;
    if stream.width != Some(width) || stream.height != Some(height) {
        return Err(MediaError::InvalidInput(
            "RAW dimensions differ from locked metadata".into(),
        ));
    }
    if stream.pixel_format.as_deref() != Some(info.pixel_format) {
        return Err(MediaError::InvalidInput(format!(
            "RAW native format {} differs from locked metadata {:?}",
            info.pixel_format, stream.pixel_format
        )));
    }
    Ok(VideoImage {
        size: [width, height],
        pixels: rgb48_to_working(&rgb48, working)?,
    })
}

#[cfg(not(kronello_libraw))]
fn decode_raw_file(
    _path: &Path,
    _stream: &StreamMetadata,
    _working: ColorSpace,
) -> Result<VideoImage, MediaError> {
    Err(MediaError::UnsupportedFeature(
        "LibRaw unavailable in this build; camera RAW decode is disabled".into(),
    ))
}

// ---------------------------------------------------------------------------
// CinemaDNG frame sequences (ADR-0136): a `AssetKind::Video` whose locator is
// the first frame and whose content hash is the ordered member manifest.

/// Sorted, verified member list of one CinemaDNG sequence.
#[derive(Debug, Clone)]
pub struct CinemaDngManifest {
    pub members: Vec<PathBuf>,
}

/// `<stem><digits>.dng` naming: the trailing decimal run defines the pattern.
fn cinemadng_pattern(first: &Path) -> Option<(String, usize)> {
    let ext = first
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())?;
    if ext != "dng" {
        return None;
    }
    let stem = first.file_stem()?.to_str()?;
    let digits = stem
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .count();
    if digits == 0 {
        return None;
    }
    Some((stem[..stem.len() - digits].to_string(), digits))
}

/// Enumerate `<stem><same-width digits>.dng` siblings of the first frame,
/// sorted by numeric value. Deterministic; never follows symlinks/dirs.
pub fn cinemadng_members(first: &Path) -> Result<Option<CinemaDngManifest>, MediaError> {
    let Some((prefix, digits)) = cinemadng_pattern(first) else {
        return Ok(None);
    };
    let canonical_first = first.canonicalize()?;
    let mut members = Vec::new();
    let mut entries = std::fs::read_dir(first.parent().unwrap_or(Path::new(".")))?
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let kind = entry.file_type()?;
        if !kind.is_file() {
            continue;
        }
        let path = entry.path();
        let Some(candidate) = cinemadng_pattern(&path) else {
            continue;
        };
        if candidate.0 != prefix || candidate.1 != digits {
            continue;
        }
        members.push(path.canonicalize()?);
    }
    members.sort_by(|a, b| {
        let num = |p: &Path| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s[s.len() - digits..].parse::<u64>().ok())
                .unwrap_or(u64::MAX)
        };
        num(a).cmp(&num(b)).then_with(|| a.cmp(b))
    });
    if members.len() > CINEMADNG_MAX_FRAMES {
        return Err(MediaError::UnsupportedFeature(
            "CinemaDNG sequence exceeds frame budget".into(),
        ));
    }
    if members.is_empty() || !members.contains(&canonical_first) {
        return Err(MediaError::AssetMissing(
            "CinemaDNG first frame not among sequence members".into(),
        ));
    }
    Ok(Some(CinemaDngManifest { members }))
}

/// Order-sensitive manifest hash over member byte digests; survives renames
/// (content-only) and pins membership exactly.
pub fn cinemadng_content_hash(members: &[PathBuf]) -> Result<String, MediaError> {
    let mut digest = Sha256::new();
    digest.update(b"kronello-cinemadng-v1\0");
    digest.update((members.len() as u64).to_le_bytes());
    for member in members {
        let bytes = std::fs::metadata(member)?.len();
        digest.update(bytes.to_le_bytes());
        digest.update(content_hash(member)?);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn frame_index(
    time: Rational,
    start: Rational,
    frame: Rational,
    count: u64,
    reverse: bool,
) -> Result<u64, MediaError> {
    let rel = time.checked_sub(start)?;
    if rel < Rational::ZERO {
        return Err(MediaError::FrameNotFound(format!("{time:?}")));
    }
    let q = rel.checked_div(frame)?;
    let mut index = q.floor();
    if reverse && q.denominator() == 1 {
        // Reverse intervals are (pts, end]: an exact boundary selects the
        // preceding frame.
        index = index.saturating_sub(1);
    }
    if index < 0 || index >= count as i64 {
        return Err(MediaError::FrameNotFound(format!("{time:?}")));
    }
    Ok(index as u64)
}

/// Decode the CinemaDNG frame whose presentation interval covers `time`.
/// Membership and content are pinned by the manifest hash on every access.
#[allow(clippy::too_many_arguments)]
pub fn decode_cinemadng_image(
    asset: &Asset,
    project_path: &Path,
    stream_index: u32,
    time: Rational,
    working: ColorSpace,
    reverse_sampling: bool,
    interpolation: Option<kronello_time::FrameInterpolation>,
) -> Result<VideoImage, MediaError> {
    if interpolation.is_some() {
        return Err(MediaError::UnsupportedFeature(
            "optical-flow interpolation requires decoded video frames".into(),
        ));
    }
    if asset.kind != AssetKind::Video || working == ColorSpace::Srgb {
        return Err(MediaError::UnsupportedFeature(
            "CinemaDNG sequence asset/linear working space required".into(),
        ));
    }
    let locked = asset
        .streams
        .iter()
        .find(|s| s.index == stream_index)
        .ok_or_else(|| MediaError::InvalidInput("CinemaDNG stream lock missing".into()))?;
    if locked.codec != CINEMADNG_CODEC {
        return Err(MediaError::UnsupportedFeature(format!(
            "camera RAW video codec {}",
            locked.codec
        )));
    }
    locked_raw_color_tags(locked)?;
    let located = locate_asset(asset, project_path)?;
    let Some(manifest) = cinemadng_members(&located.path)? else {
        return Err(MediaError::InvalidInput(
            "CinemaDNG locator is not a numbered <stem><digits>.dng frame".into(),
        ));
    };
    if cinemadng_content_hash(&manifest.members)? != asset.content_hash {
        return Err(MediaError::AssetHashMismatch(format!(
            "CinemaDNG manifest for {}",
            located.path.display()
        )));
    }
    let count = manifest.members.len() as u64;
    let frame = locked.time_base;
    if frame <= Rational::ZERO {
        return Err(MediaError::InvalidInput(
            "CinemaDNG stream requires a positive frame interval time_base".into(),
        ));
    }
    let expected_duration = Rational::from_integer(count as i64)
        .checked_mul(frame)
        .map_err(MediaError::from)?;
    if locked.duration != Some(expected_duration) {
        return Err(MediaError::InvalidInput(
            "CinemaDNG frame count differs from locked duration".into(),
        ));
    }
    let start = locked.start_time.unwrap_or(Rational::ZERO);
    let index = frame_index(time, start, frame, count, reverse_sampling)?;
    let member = &manifest.members[index as usize];
    match sniff_camera_raw(member)? {
        Some(CameraRawDetection::LibRawStill("dng")) => {}
        _ => {
            return Err(MediaError::InvalidInput(
                "CinemaDNG member is not a DNG frame".into(),
            ));
        }
    }
    gate_dng_compression(member)?;
    decode_raw_file(member, locked, working)
}

/// Shared codec dispatch for camera RAW video assets. Returns `Err` when the
/// locked codec is not a camera RAW video codec.
#[allow(clippy::too_many_arguments)]
pub fn decode_video_dispatch(
    asset: &Asset,
    project_path: &Path,
    stream_index: u32,
    time: Rational,
    working: ColorSpace,
    reverse_sampling: bool,
    interpolation: Option<kronello_time::FrameInterpolation>,
) -> Result<VideoImage, MediaError> {
    match asset
        .streams
        .iter()
        .find(|s| s.index == stream_index)
        .map(|s| s.codec.as_str())
    {
        Some(CINEMADNG_CODEC) => decode_cinemadng_image(
            asset,
            project_path,
            stream_index,
            time,
            working,
            reverse_sampling,
            interpolation,
        ),
        Some(codec) if PRORES_RAW_CODECS.contains(&codec) => decode_prores_raw_image(
            asset,
            project_path,
            stream_index,
            time,
            working,
            reverse_sampling,
            interpolation,
        ),
        _ => Err(MediaError::InvalidInput(
            "not a camera RAW video codec".into(),
        )),
    }
}

// ---------------------------------------------------------------------------
// ProRes RAW: macOS-only VideoToolbox/AVFoundation admission (ADR-0136/0081).

/// Hardware capability query: true only when VideoToolbox reports ProRes RAW
/// or ProRes RAW HQ hardware decode support. Always false off macOS.
pub fn prores_raw_hardware_supported() -> bool {
    #[cfg(target_os = "macos")]
    {
        kronello_framebridge::resident::prores_raw_hardware_supported()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Decode one ProRes RAW frame at `time` into premultiplied linear working
/// space through AVFoundation demux + VideoToolbox hardware decode.
#[allow(clippy::too_many_arguments)]
pub fn decode_prores_raw_image(
    asset: &Asset,
    project_path: &Path,
    stream_index: u32,
    time: Rational,
    working: ColorSpace,
    reverse_sampling: bool,
    interpolation: Option<kronello_time::FrameInterpolation>,
) -> Result<VideoImage, MediaError> {
    if interpolation.is_some() {
        return Err(MediaError::UnsupportedFeature(
            "optical-flow interpolation is unsupported for ProRes RAW".into(),
        ));
    }
    if asset.kind != AssetKind::Video || working == ColorSpace::Srgb {
        return Err(MediaError::UnsupportedFeature(
            "ProRes RAW video asset/linear working space required".into(),
        ));
    }
    let locked = asset
        .streams
        .iter()
        .find(|s| s.index == stream_index)
        .ok_or_else(|| MediaError::InvalidInput("ProRes RAW stream lock missing".into()))?;
    if !PRORES_RAW_CODECS.contains(&locked.codec.as_str()) {
        return Err(MediaError::UnsupportedFeature(format!(
            "camera RAW video codec {}",
            locked.codec
        )));
    }
    let path = crate::resolve_asset(asset, project_path)?;
    match sniff_camera_raw(&path)? {
        Some(CameraRawDetection::ProResRaw(codec)) if codec == locked.codec => {}
        Some(other) => {
            return Err(MediaError::InvalidInput(format!(
                "content differs from locked codec {} ({other:?})",
                locked.codec
            )));
        }
        None => {
            return Err(MediaError::InvalidInput(
                "content is not a ProRes RAW QuickTime stream".into(),
            ));
        }
    }
    prores_raw_decode(&path, locked, stream_index, time, working, reverse_sampling)
}

#[cfg(target_os = "macos")]
fn prores_raw_decode(
    path: &Path,
    locked: &StreamMetadata,
    stream_index: u32,
    time: Rational,
    working: ColorSpace,
    reverse_sampling: bool,
) -> Result<VideoImage, MediaError> {
    if !prores_raw_hardware_supported() {
        return Err(MediaError::UnsupportedFeature(
            "ProRes RAW requires VideoToolbox hardware decode on macOS".into(),
        ));
    }
    if reverse_sampling {
        return Err(MediaError::UnsupportedFeature(
            "ProRes RAW reverse sampling is not implemented".into(),
        ));
    }
    if locked.time_base <= Rational::ZERO {
        return Err(MediaError::InvalidInput(
            "ProRes RAW stream requires a positive time_base".into(),
        ));
    }
    let start = locked.start_time.unwrap_or(Rational::ZERO);
    let Some(duration) = locked.duration else {
        return Err(MediaError::InvalidInput(
            "ProRes RAW stream requires a locked duration".into(),
        ));
    };
    let end = start.checked_add(duration)?;
    let frame = kronello_framebridge::resident::decode_file_prores_raw(
        path,
        stream_index,
        (time.numerator(), time.denominator()),
        [
            (start.numerator(), start.denominator()),
            (end.numerator(), end.denominator()),
        ],
    )
    .map_err(|e| MediaError::UnsupportedFeature(format!("ProRes RAW decode: {e}")))?;
    let size = frame.size();
    if locked.width != Some(size[0]) || locked.height != Some(size[1]) {
        return Err(MediaError::InvalidInput(
            "ProRes RAW dimensions differ from locked metadata".into(),
        ));
    }
    if locked.pixel_format.as_deref() != Some(PRORES_RAW_PIXEL_FORMAT) {
        return Err(MediaError::InvalidInput(
            "ProRes RAW locked pixel format must be rgba64h".into(),
        ));
    }
    let primaries = frame
        .color_primaries()
        .unwrap_or_else(|| locked.color_primaries.clone().unwrap_or_default());
    // Linear scene-referred output; untagged buffers follow the locked contract.
    let pixels = frame
        .linear_pixels()
        .map_err(|e| MediaError::Decode(format!("ProRes RAW pixel read: {e}")))?;
    let to_2020 = match primaries.as_str() {
        "bt2020" | "" | "unknown" | "unspecified" => false,
        "bt709" => true,
        other => {
            return Err(MediaError::UnsupportedFeature(format!(
                "ProRes RAW primaries {other}"
            )));
        }
    };
    let pixels = pixels
        .chunks_exact(4)
        .map(|p| {
            let mut rgb = [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])];
            if to_2020 && working == ColorSpace::LinearRec2020 {
                rgb = rec709_to_rec2020(rgb);
            } else if !to_2020 && working == ColorSpace::LinearRec709 {
                rgb = rec2020_to_rec709(rgb);
            }
            let alpha = f64::from(p[3]).clamp(0.0, 1.0);
            [
                (rgb[0] * alpha) as f32,
                (rgb[1] * alpha) as f32,
                (rgb[2] * alpha) as f32,
                alpha as f32,
            ]
        })
        .collect();
    Ok(VideoImage {
        size: [size[0], size[1]],
        pixels,
    })
}

#[cfg(not(target_os = "macos"))]
fn prores_raw_decode(
    _path: &Path,
    _locked: &StreamMetadata,
    _stream_index: u32,
    _time: Rational,
    _working: ColorSpace,
    _reverse_sampling: bool,
) -> Result<VideoImage, MediaError> {
    Err(MediaError::UnsupportedFeature(
        "ProRes RAW requires macOS VideoToolbox/AVFoundation".into(),
    ))
}
