fn main() {
    println!("cargo:rerun-if-changed=native/demux.m");
    println!("cargo:rerun-if-changed=native/capture.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("native/demux.m")
            .flag("-fobjc-arc")
            .compile("kronello_framebridge_demux");
        cc::Build::new()
            .file("native/capture.m")
            .flag("-fobjc-arc")
            .compile("kronello_framebridge_capture");
        for framework in [
            "AVFoundation",
            "Foundation",
            "CoreMedia",
            "ScreenCaptureKit",
        ] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    }
}
