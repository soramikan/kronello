fn main() {
    println!("cargo:rerun-if-changed=native/demux.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("native/demux.m")
            .flag("-fobjc-arc")
            .compile("kronello_framebridge_demux");
        for framework in ["AVFoundation", "Foundation", "CoreMedia"] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    }
}
