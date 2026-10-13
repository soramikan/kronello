fn main() {
    println!("cargo:rerun-if-changed=native/demux.m");
    println!("cargo:rerun-if-changed=native/output.m");
    println!("cargo:rerun-if-changed=native/capture.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        if std::env::var_os("CARGO_FEATURE_ENCODER_PROTOTYPES").is_some() {
            println!("cargo:rerun-if-changed=native/encoder_probe.m");
            cc::Build::new()
                .file("native/encoder_probe.m")
                .flag("-fobjc-arc")
                .compile("kronello_encoder_probe");
            for framework in ["CoreVideo", "VideoToolbox"] {
                println!("cargo:rustc-link-lib=framework={framework}");
            }
        }
        cc::Build::new()
            .file("native/demux.m")
            .flag("-fobjc-arc")
            .compile("kronello_framebridge_demux");
        cc::Build::new()
            .file("native/output.m")
            .flag("-fobjc-arc")
            .compile("kronello_framebridge_output");
        cc::Build::new()
            .file("native/capture.m")
            .flag("-fobjc-arc")
            .compile("kronello_framebridge_capture");
        for framework in [
            "AVFoundation",
            "Foundation",
            "CoreMedia",
            "Metal",
            "ScreenCaptureKit",
        ] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    }
}
