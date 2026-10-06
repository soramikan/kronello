fn main() {
    println!("cargo:rerun-if-changed=native/media.c");
    let mut build = cc::Build::new();
    build.file("native/media.c").flag_if_supported("-std=gnu11");
    println!("cargo:rerun-if-env-changed=KRONELLO_FFMPEG_PREFIX");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // Clang supports the audited shim's typeof declarations with the MSVC ABI.
        build.compiler("clang");
        let prefix = std::path::PathBuf::from(
            std::env::var_os("KRONELLO_FFMPEG_PREFIX")
                .expect("Windows requires KRONELLO_FFMPEG_PREFIX with FFmpeg headers and DLLs"),
        );
        build.include(prefix.join("include"));
        println!(
            "cargo:rustc-env=KRONELLO_FFMPEG_BUILD_LIB_DIR={}",
            prefix.join("bin").display()
        );
        build.compile("kronello_media_shim");
        return;
    }
    for name in [
        "libavutil",
        "libavcodec",
        "libavformat",
        "libswscale",
        "libswresample",
    ] {
        let lib = pkg_config::Config::new()
            .cargo_metadata(false)
            .probe(name)
            .expect("FFmpeg development headers and pkg-config are required");
        for path in lib.include_paths {
            build.include(path);
        }
        if name == "libavutil" {
            let path = lib.link_paths.first().expect("FFmpeg library directory");
            println!(
                "cargo:rustc-env=KRONELLO_FFMPEG_BUILD_LIB_DIR={}",
                path.display()
            );
        }
    }
    build.compile("kronello_media_shim");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-lib=dl");
    }
}
