fn main() {
    println!("cargo:rerun-if-changed=native/media.c");
    println!("cargo:rerun-if-changed=native/raw.cpp");
    // Camera RAW support (ADR-0136) is optional: when LibRaw 0.22.2 headers are
    // not found, the crate still builds and every RAW path returns a typed
    // UNSUPPORTED_FEATURE. The vendored build installs libraw_r into the shared
    // KRONELLO_FFMPEG_PREFIX; development machines may use pkg-config.
    println!("cargo::rustc-check-cfg=cfg(kronello_libraw)");
    println!("cargo:rerun-if-env-changed=KRONELLO_FFMPEG_PREFIX");
    println!("cargo:rerun-if-env-changed=KRONELLO_LIBRAW_PREFIX");
    let mut build = cc::Build::new();
    build.file("native/media.c").flag_if_supported("-std=gnu11");
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
        build_libraw();
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
    build_libraw();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-lib=dl");
    }
}

/// Probe LibRaw and compile the raw shim. Search order: explicit
/// KRONELLO_LIBRAW_PREFIX, the vendored KRONELLO_FFMPEG_PREFIX layout, then
/// pkg-config `libraw_r`/`libraw`. Successful probes emit `kronello_libraw`.
fn build_libraw() {
    let mut prefix = std::env::var_os("KRONELLO_LIBRAW_PREFIX")
        .map(std::path::PathBuf::from)
        .filter(|p| p.join("include/libraw/libraw.h").is_file());
    if prefix.is_none() {
        prefix = std::env::var_os("KRONELLO_FFMPEG_PREFIX")
            .map(std::path::PathBuf::from)
            .filter(|p| p.join("include/libraw/libraw.h").is_file());
    }
    if let Some(prefix) = prefix {
        // Vendored builds place runtime DLLs under bin/ on Windows and shared
        // libraries under lib/ elsewhere.
        let lib_dir = if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
            prefix.join("bin")
        } else {
            prefix.join("lib")
        };
        // The vendored Windows LibRaw is a MinGW build: only its C API is
        // ABI-compatible with MSVC, so the shim binds the DLL at runtime from
        // KRONELLO_LIBRAW_BUILD_LIB_DIR instead of linking an import library.
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
            println!("cargo:rustc-link-search=native={}", lib_dir.display());
            println!("cargo:rustc-link-lib=raw_r");
        }
        println!("cargo:rustc-cfg=kronello_libraw");
        println!(
            "cargo:rustc-env=KRONELLO_LIBRAW_BUILD_LIB_DIR={}",
            lib_dir.display()
        );
        compile_raw_shim(std::slice::from_ref(&prefix.join("include")));
        return;
    }
    for name in ["libraw_r", "libraw"] {
        let Ok(lib) = pkg_config::Config::new().cargo_metadata(false).probe(name) else {
            continue;
        };
        for path in &lib.link_paths {
            println!("cargo:rustc-link-search=native={}", path.display());
        }
        println!(
            "cargo:rustc-link-lib={}",
            name.strip_prefix("lib").unwrap_or(name)
        );
        println!("cargo:rustc-cfg=kronello_libraw");
        if let Some(path) = lib.link_paths.first() {
            println!(
                "cargo:rustc-env=KRONELLO_LIBRAW_BUILD_LIB_DIR={}",
                path.display()
            );
        }
        compile_raw_shim(&lib.include_paths);
        return;
    }
    println!(
        "cargo:warning=LibRaw not found; camera RAW decode compiles out as UNSUPPORTED_FEATURE"
    );
}

fn compile_raw_shim(include_paths: &[std::path::PathBuf]) {
    let mut build = cc::Build::new();
    build.cpp(true).file("native/raw.cpp");
    if build.get_compiler().is_like_msvc() {
        build.flag("/EHsc");
    } else {
        build.flag_if_supported("-std=c++17");
    }
    // libraw/libraw.h must resolve via the include root, not libraw/ itself.
    for path in include_paths {
        if path.join("libraw/libraw.h").is_file() {
            build.include(path);
        } else if path.join("libraw.h").is_file() {
            if let Some(parent) = path.parent() {
                build.include(parent);
            }
            build.include(path);
        } else {
            build.include(path);
        }
    }
    build.compile("kronello_raw_shim");
}
