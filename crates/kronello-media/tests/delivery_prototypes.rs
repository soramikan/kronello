#![cfg(feature = "export-prototypes")]
use kronello_media::delivery_prototypes::*;
use kronello_media::{MediaError, MediaRuntime};
use kronello_time::FrameRate;

fn request(output: std::path::PathBuf) -> PacketMuxProbeRequest {
    PacketMuxProbeRequest {
        output,
        pixels: [32, 16],
        frame_rate: FrameRate::new(24, 1).unwrap(),
        frames: 48,
        render_snapshot_hash: "1".repeat(64),
        export_snapshot_hash: "2".repeat(64),
    }
}
fn fill(index: u64, pixels: &mut [u8]) -> Result<(), MediaError> {
    for (n, pixel) in pixels.chunks_exact_mut(4).enumerate() {
        pixel.copy_from_slice(&[
            (n as u64 + index) as u8,
            (n as u64 / 3 + index * 2) as u8,
            (n as u64 / 7 + index * 3) as u8,
            255,
        ]);
    }
    Ok(())
}
#[test]
fn direct_packet_mux_preserves_decoded_frames_audio_timing_and_identity() {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let before = packet_mux_probe(
        &runtime,
        &request(dir.path().join("before.mov")),
        PacketMuxMode::Intermediate,
        &mut fill,
        &mut |_| Ok(()),
    )
    .unwrap();
    let after = packet_mux_probe(
        &runtime,
        &request(dir.path().join("after.mov")),
        PacketMuxMode::Direct,
        &mut fill,
        &mut |_| Ok(()),
    )
    .unwrap();
    assert!(before.intermediate_video_bytes > 0);
    assert_eq!(after.intermediate_video_bytes, 0);
    assert_eq!(before.probe, after.probe);
    assert_eq!(
        std::fs::read(dir.path().join("before.mov")).unwrap(),
        std::fs::read(dir.path().join("after.mov")).unwrap()
    );
    let mut left = runtime.open_video(&dir.path().join("before.mov")).unwrap();
    let mut right = runtime.open_video(&dir.path().join("after.mov")).unwrap();
    for i in 0..48 {
        let time = FrameRate::new(24, 1).unwrap().frame_to_time(i).unwrap();
        let a = left.rgba_at(time).unwrap();
        let b = right.rgba_at(time).unwrap();
        assert_eq!(a, b);
    }
    assert_eq!(
        runtime
            .decode_audio(&dir.path().join("before.mov"), 1)
            .unwrap(),
        runtime
            .decode_audio(&dir.path().join("after.mov"), 1)
            .unwrap()
    );
}
#[test]
fn direct_mux_cancel_and_competing_destination_reclaim_private_stages() {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("canceled.mov");
    let result = packet_mux_probe(
        &runtime,
        &request(output.clone()),
        PacketMuxMode::Direct,
        &mut fill,
        &mut |n| {
            if n == 3 {
                Err(MediaError::InvalidInput("cancel".into()))
            } else {
                Ok(())
            }
        },
    );
    assert!(result.is_err());
    assert!(!output.exists());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    let competing = dir.path().join("competing.mov");
    let target = competing.clone();
    let result = packet_mux_probe(
        &runtime,
        &request(competing.clone()),
        PacketMuxMode::Direct,
        &mut fill,
        &mut |n| {
            if n == 48 {
                std::fs::write(&target, b"keep")?;
            }
            Ok(())
        },
    );
    assert!(result.is_err());
    assert_eq!(std::fs::read(competing).unwrap(), b"keep");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}
