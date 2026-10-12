//! Fixed-input alternating native packet mux experiment. Independent decoder
//! and packet/time oracle is run separately, outside these timed exports.
use kronello_media::delivery_prototypes::*;
use kronello_media::{MediaError, MediaRuntime};
use kronello_time::FrameRate;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if cfg!(debug_assertions) {
        return Err("use --release".into());
    }
    let args: Vec<_> = std::env::args().collect();
    let root = PathBuf::from(args.get(1).ok_or("new output directory")?);
    let width: u32 = args.get(2).ok_or("width")?.parse()?;
    let height: u32 = args.get(3).ok_or("height")?.parse()?;
    let frames: u64 = args.get(4).ok_or("frames")?.parse()?;
    let pairs: u64 = args.get(5).ok_or("pairs >= 3")?.parse()?;
    if !(3..=32).contains(&pairs) {
        return Err("pairs must be 3..=32".into());
    }
    std::fs::create_dir(&root)?;
    let runtime = MediaRuntime::load()?;
    let fixture = json!({"schema_version":1,"pixels":[width,height],"frames":frames,"frame_rate":{"num":"24","den":"1"},"rgba":"((x/4+i)%256,(y/4+i*2)%256,((x+y)/8+i*3)%256,255)","audio":"48k stereo PCM24 silence","chapters":[]});
    let identity = format!("{:x}", Sha256::digest(serde_json::to_vec(&fixture)?));
    let mut evidence = json!({"schema_version":1,"fixture":fixture,"fixture_sha256":identity,"capabilities":runtime.capabilities(),"trials":[]});
    for pair in 0..pairs {
        let order = if pair % 2 == 0 {
            [PacketMuxMode::Intermediate, PacketMuxMode::Direct]
        } else {
            [PacketMuxMode::Direct, PacketMuxMode::Intermediate]
        };
        for mode in order {
            let name = match mode {
                PacketMuxMode::Intermediate => "intermediate",
                PacketMuxMode::Direct => "direct",
            };
            let request = PacketMuxProbeRequest {
                output: root.join(format!("{pair}-{name}.mov")),
                pixels: [width, height],
                frame_rate: FrameRate::new(24, 1)?,
                frames,
                render_snapshot_hash: identity.clone(),
                export_snapshot_hash: identity.clone(),
            };
            let mut fill = |i: u64, buffer: &mut [u8]| -> Result<(), MediaError> {
                for (n, pixel) in buffer.chunks_exact_mut(4).enumerate() {
                    let x = n % width as usize;
                    let y = n / width as usize;
                    pixel.copy_from_slice(&[
                        ((x / 4) as u64 + i) as u8,
                        ((y / 4) as u64 + i * 2) as u8,
                        (((x + y) / 8) as u64 + i * 3) as u8,
                        255,
                    ]);
                }
                Ok(())
            };
            let report = packet_mux_probe(&runtime, &request, mode, &mut fill, &mut |_| Ok(()))?;
            eprintln!("pair {pair} {name}: {} us", report.total_us);
            evidence["trials"]
                .as_array_mut()
                .unwrap()
                .push(json!({"pair":pair,"mode":name,"output":request.output,"report":report}));
            std::fs::write(
                root.join("evidence.json"),
                serde_json::to_vec_pretty(&evidence)?,
            )?;
        }
    }
    Ok(())
}
