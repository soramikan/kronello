//! Explicit host acceptance: no ignored test or automatic hardware fallback.
use kronello_media::{EncodeCodec, EncodeFrame, EncodeRequest, MediaRuntime};
use kronello_time::Rational;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = MediaRuntime::load()?;
    let output = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .ok_or("output directory argument required")?;
    std::fs::create_dir(&output)?;
    let step = Rational::new(1, 24)?;
    let frames = (0..4)
        .map(|i| EncodeFrame {
            pts: Rational::new(i, 24).unwrap(),
            rgba: [40_u8, 100, 180, 255].repeat(64 * 64),
        })
        .collect::<Vec<_>>();
    let mut reports = Vec::new();
    for codec in [EncodeCodec::H264, EncodeCodec::Hevc] {
        let request = EncodeRequest {
            output: output.join(format!("{codec:?}.mp4")),
            codec,
            width: 64,
            height: 64,
            time_base: step,
        };
        let report = runtime.encode_video(&request, &frames)?;
        assert_eq!(report.execution, kronello_media::ExecutionKind::Hardware);
        let mut decoder = runtime.open_video(&request.output)?;
        for i in 0..4 {
            let frame = decoder.decode_at(Rational::new(i, 24)?)?;
            assert_eq!(frame.pts, Rational::new(i, 24)?);
            assert_eq!((frame.width, frame.height), (64, 64));
        }
        reports.push(serde_json::json!({"encode":report,"decode":decoder.path_report()}));
    }
    println!("{}", serde_json::to_string_pretty(&reports)?);
    Ok(())
}
