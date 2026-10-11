//! Exact compressed-order cursor with bounded retained native outputs.
use super::{HardwareFrame, ResidentFormat, error, exact_time_add};
use crate::videotoolbox::{DecodeMode, NativeError, NativeStage, SampleToken, StreamingDecoder};
use objc2_core_foundation::CFRetained;
use objc2_core_media::{CMSampleBuffer, CMTime};
use objc2_core_video::CVPixelBufferGetDataSize;
use std::{
    ffi::{CString, c_char, c_void},
    path::{Path, PathBuf},
    ptr::NonNull,
};

unsafe extern "C" {
    fn kronello_fb_cursor_open(
        path: *const c_char,
        stream: u32,
        cursor: *mut *mut c_void,
        error: *mut c_char,
        error_size: usize,
    ) -> i32;
    fn kronello_fb_cursor_next(
        cursor: *mut c_void,
        sample: *mut *mut c_void,
        sample_bytes: *mut usize,
        error: *mut c_char,
        error_size: usize,
    ) -> i32;
    fn kronello_fb_cursor_close(cursor: *mut c_void);
}
struct CompressedCursor {
    native: NonNull<c_void>,
    _owner: std::marker::PhantomData<std::rc::Rc<()>>,
}
impl Drop for CompressedCursor {
    fn drop(&mut self) {
        // SAFETY: This thread owns the +1 native cursor; close cancels reading and
        // releases ARC ownership once. Returned samples have separate ownership.
        unsafe {
            kronello_fb_cursor_close(self.native.as_ptr());
        }
    }
}
impl CompressedCursor {
    fn open(path: &Path, stream: u32) -> Result<Self, NativeError> {
        use std::os::unix::ffi::OsStrExt;
        let path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| error(NativeStage::FormatDescription, "path NUL"))?;
        let mut pointer = std::ptr::null_mut();
        let mut diagnostic = [0u8; 512];
        // SAFETY: NUL path, writable out-pointer and diagnostic; successful open
        // transfers one native cursor to this owning-thread guard.
        let code = unsafe {
            kronello_fb_cursor_open(
                path.as_ptr(),
                stream,
                &mut pointer,
                diagnostic.as_mut_ptr().cast(),
                diagnostic.len(),
            )
        };
        native_result(code, &diagnostic)?;
        Ok(Self {
            native: NonNull::new(pointer)
                .ok_or_else(|| error(NativeStage::FormatDescription, "null compressed cursor"))?,
            _owner: std::marker::PhantomData,
        })
    }
    fn next(&mut self) -> Result<Option<(SampleToken, usize)>, NativeError> {
        let mut sample = std::ptr::null_mut();
        let mut bytes = 0;
        let mut diagnostic = [0u8; 512];
        // SAFETY: Live private reader; successful next transfers +1 sample and
        // no compressed pointers remain borrowed after the call.
        let code = unsafe {
            kronello_fb_cursor_next(
                self.native.as_ptr(),
                &mut sample,
                &mut bytes,
                diagnostic.as_mut_ptr().cast(),
                diagnostic.len(),
            )
        };
        if code < 0 {
            native_result(code, &diagnostic)?;
        }
        if code == 0 {
            return Ok(None);
        }
        let sample = NonNull::new(sample.cast::<CMSampleBuffer>())
            .ok_or_else(|| error(NativeStage::FormatDescription, "null compressed sample"))?;
        // SAFETY: Native copyNextSampleBuffer returned +1 ownership.
        Ok(Some((
            SampleToken(unsafe { CFRetained::from_raw(sample) }),
            bytes,
        )))
    }
}
fn native_result(code: i32, diagnostic: &[u8]) -> Result<(), NativeError> {
    if code == 0 {
        return Ok(());
    }
    Err(error(
        NativeStage::FormatDescription,
        String::from_utf8_lossy(diagnostic)
            .trim_end_matches('\0')
            .to_string(),
    ))
}
fn compare(a: CMTime, b: CMTime) -> std::cmp::Ordering {
    (i128::from(a.value) * i128::from(b.timescale))
        .cmp(&(i128::from(b.value) * i128::from(a.timescale)))
}
fn sample_pts(sample: &SampleToken) -> Result<CMTime, NativeError> {
    // SAFETY: Retained compressed sample; no pending native decode borrows it.
    let pts = unsafe { sample.0.output_presentation_time_stamp() };
    if pts.timescale <= 0 || pts.epoch != 0 {
        return Err(error(
            NativeStage::FrameSelection,
            "invalid compressed output PTS",
        ));
    }
    Ok(pts)
}
fn time(value: (i64, i64)) -> Result<CMTime, NativeError> {
    let scale = i32::try_from(value.1).map_err(|_| {
        error(
            NativeStage::FrameSelection,
            "native time denominator overflow",
        )
    })?;
    if scale <= 0 {
        return Err(error(
            NativeStage::FrameSelection,
            "native time denominator must be positive",
        ));
    }
    // SAFETY: Exact finite integer ratio with positive native timescale.
    Ok(unsafe { CMTime::new(value.0, scale) })
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fingerprint {
    device: u64,
    inode: u64,
    length: u64,
    mtime: (i64, i64),
    ctime: (i64, i64),
}
fn fingerprint(path: &Path) -> Result<Fingerprint, NativeError> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(path)
        .map_err(|e| error(NativeStage::FormatDescription, e.to_string()))?;
    if !m.is_file() {
        return Err(error(
            NativeStage::FormatDescription,
            "local regular file required",
        ));
    }
    Ok(Fingerprint {
        device: m.dev(),
        inode: m.ino(),
        length: m.len(),
        mtime: (m.mtime(), m.mtime_nsec()),
        ctime: (m.ctime(), m.ctime_nsec()),
    })
}
/// Payload counters exclude AVFoundation/VT internal buffering, allocator,
/// returned frames, GPU import ownership and native codec reference surfaces.
#[derive(Debug, Default, Clone, Copy)]
pub struct ResidentCursorStats {
    pub metadata_samples: u64,
    pub compressed_samples: u64,
    pub compressed_sample_peak_bytes: usize,
    pub sessions_created: u64,
    pub origin_restarts: u64,
    pub retained_outputs: usize,
    pub retained_output_bytes: u64,
    pub peak_output_bytes: u64,
    pub peak_outputs: usize,
}
/// Owning-thread SDR hardware cursor. Metadata is scanned once with one retained
/// sample; no complete clip array, compressed spool, GOP/FPS inference or pixel
/// digest is retained. Backward requests restart exactly at the canonical origin.
pub struct ResidentFileDecoder {
    path: PathBuf,
    stream: u32,
    format: ResidentFormat,
    interval: [CMTime; 2],
    source: Fingerprint,
    first: CMTime,
    reorder_gap: CMTime,
    reader: Option<CompressedCursor>,
    decoder: Option<StreamingDecoder>,
    maximum_submitted: Option<CMTime>,
    watermark: Option<CMTime>,
    finished: bool,
    frames: Vec<HardwareFrame>,
    last_selected: Option<CMTime>,
    stats: ResidentCursorStats,
}
impl ResidentFileDecoder {
    pub fn open(
        path: &Path,
        stream: u32,
        format: ResidentFormat,
        interval: [(i64, i64); 2],
    ) -> Result<Self, NativeError> {
        if format == ResidentFormat::ProResRawRgbah {
            return Err(error(
                NativeStage::FormatDescription,
                "persistent cursor requires explicit SDR H264/HEVC; ProRes RAW uses its separate validated path",
            ));
        }
        let path = path
            .canonicalize()
            .map_err(|e| error(NativeStage::FormatDescription, e.to_string()))?;
        let source = fingerprint(&path)?;
        let interval = [time(interval[0])?, time(interval[1])?];
        if compare(interval[0], interval[1]) != std::cmp::Ordering::Less {
            return Err(error(
                NativeStage::FrameSelection,
                "invalid locked video interval",
            ));
        }
        let mut reader = CompressedCursor::open(&path, stream)?;
        let mut first = None;
        let mut maximum = None;
        let mut reorder_gap = time((0, 1))?;
        let mut stats = ResidentCursorStats::default();
        while let Some((sample, bytes)) = reader.next()? {
            stats.metadata_samples = stats.metadata_samples.saturating_add(1);
            stats.compressed_sample_peak_bytes = stats.compressed_sample_peak_bytes.max(bytes);
            let pts = sample_pts(&sample)?;
            if first.is_none_or(|previous| compare(pts, previous).is_lt()) {
                first = Some(pts);
            }
            if let Some(previous) = maximum {
                let gap = exact_time_add(previous, pts, -1)?;
                if compare(gap, reorder_gap).is_gt() {
                    reorder_gap = gap;
                }
            }
            if maximum.is_none_or(|previous| compare(pts, previous).is_gt()) {
                maximum = Some(pts);
            }
        }
        drop(reader);
        if fingerprint(&path)? != source {
            return Err(error(
                NativeStage::FormatDescription,
                "source changed during compressed metadata validation",
            ));
        }
        Ok(Self {
            path,
            stream,
            format,
            interval,
            source,
            first: first
                .ok_or_else(|| error(NativeStage::FormatDescription, "no compressed samples"))?,
            reorder_gap,
            reader: None,
            decoder: None,
            maximum_submitted: None,
            watermark: None,
            finished: false,
            frames: Vec::new(),
            last_selected: None,
            stats,
        })
    }
    /// Caller admission may retire an obsolete cursor after independently
    /// verifying the current asset; decode_at repeats the fence during work.
    pub fn source_fingerprint_matches(&self) -> Result<bool, NativeError> {
        Ok(fingerprint(&self.path)? == self.source)
    }
    pub fn stats(&self) -> ResidentCursorStats {
        self.stats
    }
    fn clear(&mut self) {
        self.decoder = None;
        self.reader = None;
        self.frames.clear();
        self.maximum_submitted = None;
        self.watermark = None;
        self.last_selected = None;
        self.finished = false;
        self.stats.retained_outputs = 0;
        self.stats.retained_output_bytes = 0;
    }
    pub fn decode_at(&mut self, request: (i64, i64)) -> Result<HardwareFrame, NativeError> {
        let result = self.decode_inner(request);
        if result.is_err() {
            self.clear();
        }
        result
    }
    fn decode_inner(&mut self, request: (i64, i64)) -> Result<HardwareFrame, NativeError> {
        let requested = time(request)?;
        if compare(requested, self.interval[0]).is_lt()
            || !compare(requested, self.interval[1]).is_lt()
        {
            return Err(error(
                NativeStage::FrameSelection,
                "time outside canonical locked half-open stream interval",
            ));
        }
        if fingerprint(&self.path)? != self.source {
            return Err(error(
                NativeStage::FormatDescription,
                "resident source fingerprint changed; reopen and revalidate",
            ));
        }
        if self
            .last_selected
            .is_some_and(|last| compare(requested, last).is_lt())
        {
            self.clear();
            self.stats.origin_restarts = self.stats.origin_restarts.saturating_add(1);
        }
        if self.reader.is_none() {
            self.reader = Some(CompressedCursor::open(&self.path, self.stream)?);
        }
        // The preflight proves that every later compressed PTS is at least
        // prefix_max - global_reorder_gap. All submitted callbacks have completed
        // before that watermark is used to publish a selected presentation.
        while !self.finished
            && !self
                .watermark
                .is_some_and(|mark| compare(mark, requested).is_gt())
        {
            let next = self.reader.as_mut().expect("opened reader").next()?;
            let Some((sample, bytes)) = next else {
                self.finished = true;
                break;
            };
            self.stats.compressed_sample_peak_bytes =
                self.stats.compressed_sample_peak_bytes.max(bytes);
            let pts = sample_pts(&sample)?;
            let canonical =
                exact_time_add(exact_time_add(pts, self.first, -1)?, self.interval[0], 1)?;
            // SAFETY: Private retained sample, exact finite timing; native decode
            // starts only after this output timestamp change has completed.
            let status = unsafe { sample.0.set_output_presentation_time_stamp(canonical) };
            if status != 0 {
                return Err(error(
                    NativeStage::FrameSelection,
                    format!("canonical output PTS OSStatus={status}"),
                ));
            }
            if self.decoder.is_none() {
                self.decoder = Some(StreamingDecoder::new(
                    &sample,
                    match self.format {
                        ResidentFormat::Bgra8 => DecodeMode::SdrBgra8,
                        _ => DecodeMode::SdrNv12,
                    },
                )?);
                self.stats.sessions_created = self.stats.sessions_created.saturating_add(1);
            }
            let decoder = self.decoder.as_ref().expect("created decoder");
            decoder.submit(&sample)?;
            self.stats.compressed_samples = self.stats.compressed_samples.saturating_add(1);
            for (buffer, presentation_time) in decoder.take_frames()? {
                if self
                    .frames
                    .iter()
                    .any(|frame| compare(frame.presentation_time, presentation_time).is_eq())
                {
                    return Err(error(
                        NativeStage::Decode,
                        "duplicate native presentation PTS",
                    ));
                }
                // Admission precedes taking ownership into the retained queue.
                // An obsolete candidate can be released immediately; future
                // outputs remain available for exact forward presentation.
                if !compare(presentation_time, requested).is_gt() {
                    if self.frames.iter().any(|frame| {
                        !compare(frame.presentation_time, requested).is_gt()
                            && compare(frame.presentation_time, presentation_time).is_gt()
                    }) {
                        continue;
                    }
                    self.frames
                        .retain(|frame| compare(frame.presentation_time, requested).is_gt());
                }
                let incoming = CVPixelBufferGetDataSize(&buffer.0) as u64;
                let retained = self
                    .frames
                    .iter()
                    .map(|frame| CVPixelBufferGetDataSize(&frame.buffer.0) as u64)
                    .sum::<u64>();
                if incoming == 0
                    || self.frames.len() == 64
                    || retained.saturating_add(incoming) > 512 * 1024 * 1024
                {
                    return Err(error(
                        NativeStage::Decode,
                        "resident output admission count/byte budget or unknown native size",
                    ));
                }
                self.frames.push(HardwareFrame {
                    buffer,
                    format: self.format,
                    presentation_time,
                });
            }
            self.frames
                .sort_by(|a, b| compare(a.presentation_time, b.presentation_time));
            // Retire only frames preceding the best candidate for this request.
            // Future frames are preserved so B-frame lookahead remains available.
            if let Some(index) = self
                .frames
                .iter()
                .rposition(|f| !compare(f.presentation_time, requested).is_gt())
            {
                self.frames.drain(..index);
            }
            let bytes = self
                .frames
                .iter()
                .map(|f| CVPixelBufferGetDataSize(&f.buffer.0) as u64)
                .sum::<u64>();
            if self.frames.len() > 64 || bytes > 512 * 1024 * 1024 {
                return Err(error(
                    NativeStage::Decode,
                    "resident retained output count/byte budget exceeded",
                ));
            }
            self.stats.retained_outputs = self.frames.len();
            self.stats.retained_output_bytes = bytes;
            self.stats.peak_outputs = self.stats.peak_outputs.max(self.frames.len());
            self.stats.peak_output_bytes = self.stats.peak_output_bytes.max(bytes);
            if self
                .maximum_submitted
                .is_none_or(|previous| compare(canonical, previous).is_gt())
            {
                self.maximum_submitted = Some(canonical);
            }
            self.watermark = Some(exact_time_add(
                self.maximum_submitted.expect("submitted sample"),
                self.reorder_gap,
                -1,
            )?);
        }
        let index = self
            .frames
            .iter()
            .rposition(|frame| !compare(frame.presentation_time, requested).is_gt())
            .ok_or_else(|| {
                error(
                    NativeStage::FrameSelection,
                    "no hardware frame at requested presentation time",
                )
            })?;
        self.frames.drain(..index);
        if fingerprint(&self.path)? != self.source {
            return Err(error(
                NativeStage::FormatDescription,
                "resident source changed during hardware decode",
            ));
        }
        let selected = &self.frames[0];
        self.last_selected = Some(selected.presentation_time);
        self.stats.retained_outputs = self.frames.len();
        self.stats.retained_output_bytes = self
            .frames
            .iter()
            .map(|f| CVPixelBufferGetDataSize(&f.buffer.0) as u64)
            .sum();
        Ok(HardwareFrame {
            buffer: crate::videotoolbox::PixelToken(selected.buffer.0.clone()),
            format: selected.format,
            presentation_time: selected.presentation_time,
        })
    }
}
