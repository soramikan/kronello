#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use kronello_framebridge::encoder_prototype::probe;
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 6 {
        return Err("usage: perf_encoder_surface NEW_DIRECTORY WIDTH HEIGHT FRAMES PAIRS".into());
    }
    let root = std::path::Path::new(&args[1]);
    let width = args[2].parse()?;
    let height = args[3].parse()?;
    let frames = args[4].parse()?;
    let pairs: usize = args[5].parse()?;
    if !(3..=32).contains(&pairs) {
        return Err("pairs must be 3..32".into());
    }
    std::fs::create_dir(root)?;
    let mut evidence = String::from(
        "pair,mode,total_ms,prepare_ms,conversion_ms,encode_ms,readback_bytes,frames\n",
    );
    for pair in 0..pairs {
        for gpu in if pair % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        } {
            let mode = if gpu { "gpu" } else { "cpu" };
            let file = root.join(format!("{pair}-{mode}.h264"));
            let m = probe(&file, [width, height], frames, gpu, false, None, false)?;
            evidence.push_str(&format!(
                "{pair},{mode},{},{},{},{},{},{}\n",
                m.total_ms,
                m.prepare_ms,
                m.conversion_ms,
                m.encode_ms,
                m.readback_bytes,
                m.encoded_frames
            ));
            std::fs::write(root.join("evidence.csv"), &evidence)?;
            println!("pair {pair} {mode}: {:.3} ms", m.total_ms);
        }
    }
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("Metal/VideoToolbox prototype requires macOS");
    std::process::exit(1);
}
