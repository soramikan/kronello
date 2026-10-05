//! Distribution acceptance using only the relocated runtime, without a GPU.
use kronello_audio::{AudioBuffer, AudioClip, AudioSources, ClippingPolicy, Gain, mix};
use kronello_media::{EncodeCodec, EncodeFrame, EncodeRequest, ExecutionKind, MediaRuntime};
use kronello_model::AssetId;
use kronello_time::{Rational, TimeRange};

fn check_video_pixels(frame: &kronello_render::DecodedVideoFrame, rgb: &[u8]) -> (f64, usize) {
    assert_eq!(frame.color_primaries, "bt709");
    assert_eq!(frame.color_transfer, "bt709");
    assert_eq!(frame.color_matrix, "bt709");
    assert_eq!(frame.color_range, "tv");
    let [r, g, b] = [0, 1, 2].map(|i| f64::from(rgb[i]));
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    // Independent BT.709 limited-range reference in 8-bit sample units.
    // Uniform frames avoid chroma subsampling edge ambiguity. The fixed
    // tolerance includes codec loss and integer color-conversion rounding.
    let expected = [
        16.0 + 219.0 * y / 255.0,
        128.0 + 112.0 * (b - y) / (0.9278 * 255.0),
        128.0 + 112.0 * (r - y) / (0.7874 * 255.0),
    ];
    let pixels = (frame.width * frame.height) as usize;
    let (lengths, bytes_per_sample) = match frame.pixel_format.as_str() {
        "yuv420p" => ([pixels, pixels / 4, pixels / 4], 1),
        "yuv422p10le" => ([pixels, pixels / 2, pixels / 2], 2),
        other => panic!("unexpected decoded pixel format: {other}"),
    };
    assert_eq!(
        frame.pixels.len(),
        lengths.iter().sum::<usize>() * bytes_per_sample
    );
    let mut offset = 0;
    let mut max_error = 0.0_f64;
    for (length, expected) in lengths.into_iter().zip(expected) {
        let end = offset + length * bytes_per_sample;
        for sample in frame.pixels[offset..end].chunks_exact(bytes_per_sample) {
            let actual = if bytes_per_sample == 1 {
                f64::from(sample[0])
            } else {
                f64::from(u16::from_le_bytes([sample[0], sample[1]])) / 4.0
            };
            let error = (actual - expected).abs();
            assert!(error <= 4.0, "decoded sample error {error} exceeds 4/255");
            max_error = max_error.max(error);
        }
        offset = end;
    }
    (max_error, lengths.iter().sum())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = MediaRuntime::load()?;
    runtime.capabilities().verify_distribution()?;
    let output = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .ok_or("fresh output directory argument required")?;
    std::fs::create_dir(&output)?;
    let step = Rational::new(1, 24)?;
    let frames = (0..4)
        .map(|i| EncodeFrame {
            pts: Rational::new(i, 24).unwrap(),
            rgba: [40 + 20 * i as u8, 100, 180 - 20 * i as u8, 255].repeat(64 * 64),
        })
        .collect::<Vec<_>>();
    let mut reports = Vec::new();
    let mut video_samples_checked = 0;
    let mut video_max_error = 0.0_f64;
    for (codec, name) in [
        (EncodeCodec::Av1, "av1.mp4"),
        (EncodeCodec::ProRes, "prores.mov"),
    ] {
        let request = EncodeRequest {
            output: output.join(name),
            codec,
            width: 64,
            height: 64,
            time_base: step,
        };
        let report = runtime.encode_video(&request, &frames)?;
        assert_eq!(report.execution, ExecutionKind::Software);
        assert_eq!(
            report.encoder.as_deref(),
            Some(if codec == EncodeCodec::Av1 {
                "libsvtav1"
            } else {
                "prores_ks"
            })
        );
        let mut decoder = runtime.open_video(&request.output)?;
        for i in [3, 0, 2, 1, 3] {
            let frame = decoder.decode_at(Rational::new(i, 24)?)?;
            assert_eq!(frame.pts, Rational::new(i, 24)?);
            assert_eq!((frame.width, frame.height), (64, 64));
            assert_eq!(frame.end, Rational::new(i + 1, 24)?);
            let (error, samples) = check_video_pixels(&frame, &frames[i as usize].rgba);
            video_samples_checked += samples;
            video_max_error = video_max_error.max(error);
        }
        let probe = runtime.probe(&request.output)?;
        assert_eq!(probe.streams.len(), 1);
        assert_eq!(
            probe.streams[0].codec,
            if codec == EncodeCodec::Av1 {
                "av1"
            } else {
                "prores"
            }
        );
        assert_eq!(probe.streams[0].start, Some(Rational::ZERO));
        assert_eq!(probe.streams[0].duration, Some(Rational::new(1, 6)?));
        reports.push(
            serde_json::json!({"encode":report,"decode":decoder.path_report(),"probe":probe}),
        );
    }
    let id = AssetId::new();
    let samples = (0..8000)
        .map(|i| {
            let value = ((i % 257) as f32 - 128.0) / 256.0;
            [value, -value]
        })
        .collect::<Vec<_>>();
    let sources = AudioSources::from([((id, 0), AudioBuffer::new(samples.clone())?)]);
    let range = TimeRange::new(Rational::ZERO, Rational::new(1, 6)?)?;
    let bus = mix(
        &[AudioClip {
            asset: id,
            stream_index: 0,
            placement: range,
            source_in: Rational::ZERO,
            gain: Gain::UNITY,
        }],
        &sources,
        range,
    )?;
    let pcm = output.join("pcm24.mov");
    let audio_report = runtime.encode_audio(&bus, ClippingPolicy::Reject, &pcm)?;
    assert_eq!(audio_report.frames, 8000);
    let mux = output.join("prores-pcm24.mov");
    let probe = runtime.mux_av(
        &output.join("prores.mov"),
        &pcm,
        &mux,
        &"1".repeat(64),
        &"2".repeat(64),
    )?;
    probe.verify_av()?;
    let audio_stream = probe
        .streams
        .iter()
        .find(|s| s.codec == "pcm_s24le")
        .ok_or("missing PCM24")?;
    let decoded = runtime.decode_audio(&mux, audio_stream.index)?;
    assert_eq!(decoded.source_start, Rational::ZERO);
    assert_eq!(decoded.buffer.frames().len(), 8000);
    for (actual, expected) in decoded
        .buffer
        .frames()
        .iter()
        .flatten()
        .zip(samples.iter().flatten())
    {
        assert!((actual - expected).abs() <= 1.0 / 8388608.0);
    }
    let mut decoder = runtime.open_video(&mux)?;
    for i in 0..4 {
        let frame = decoder.decode_at(Rational::new(i, 24)?)?;
        assert_eq!(frame.pts, Rational::new(i, 24)?);
        let (error, samples) = check_video_pixels(&frame, &frames[i as usize].rgba);
        video_samples_checked += samples;
        video_max_error = video_max_error.max(error);
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "capabilities": runtime.capabilities(), "video": reports,
            "audio": audio_report, "mux": probe, "pcm24_samples_checked": 16000,
            "video_samples_checked": video_samples_checked,
            "video_max_error_8bit_units": video_max_error,
            "status": "passed"
        }))?
    );
    Ok(())
}
