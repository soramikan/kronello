#!/usr/bin/env python3
"""Build pinned, replaceable LGPL FFmpeg shared libraries (never system FFmpeg)."""
import argparse
import ctypes
import hashlib
import json
import os
import platform
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "scripts/native-dependencies.json"
if not MANIFEST.is_file():
    MANIFEST = ROOT / "native-dependencies.json"


def msys2_bash():
    # Windows CreateProcess searches System32 before PATH for a bare executable
    # name, which can select the WSL launcher even inside an MSYS2 shell.
    candidate = os.environ.get("KRONELLO_MSYS2_BASH") or shutil.which("bash.exe")
    if not candidate:
        raise ValueError("set KRONELLO_MSYS2_BASH to the absolute MSYS2 bash.exe path")
    executable = Path(candidate).resolve()
    if not executable.is_file():
        raise ValueError(f"MSYS2 bash executable does not exist: {executable}")
    probe = subprocess.run([str(executable), "-c", "uname -s"], check=True,
                           capture_output=True, text=True)
    if not probe.stdout.strip().startswith(("MSYS_NT-", "MINGW32_NT-", "MINGW64_NT-", "UCRT64_NT-", "CLANG64_NT-")):
        raise ValueError(f"MSYS2 bash required, found {probe.stdout.strip()!r}")
    return executable


def msys2_posix(bash, path):
    """Translate a Windows path for MSYS2 argv (D:/a/b -> /d/a/b)."""
    output = subprocess.run([str(bash), "-c", 'cygpath -u "$1"', "-", str(path)],
                            check=True, capture_output=True, text=True)
    return output.stdout.strip()


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def source(entry, cache, offline):
    path = cache / entry["url"].rsplit("/", 1)[1]
    if not path.exists() or sha256(path) != entry["sha256"]:
        if offline:
            raise ValueError(f"missing/hash-mismatched offline source: {path}")
        temporary = path.with_suffix(path.suffix + ".part")
        try:
            error = None
            for _ in range(3):
                try:
                    with urllib.request.urlopen(entry["url"], timeout=60) as response:
                        if not response.geturl().startswith("https://"):
                            raise ValueError("HTTPS redirect required")
                        with temporary.open("wb") as out:
                            shutil.copyfileobj(response, out)
                except (OSError, ValueError) as attempt:
                    error = attempt
                    continue
                if sha256(temporary) != entry["sha256"]:
                    error = ValueError(f"source hash mismatch: {entry['name']}")
                    continue
                temporary.replace(path)
                break
            else:
                raise error
        finally:
            temporary.unlink(missing_ok=True)
    return path


def extract(archive, destination):
    destination.mkdir()
    with tarfile.open(archive) as tar:
        # Python 3.10 also supports manual member verification. Upstream
        # symlinks/hardlinks are rejected rather than trusted during extraction.
        for member in tar.getmembers():
            path = Path(member.name)
            if path.is_absolute() or ".." in path.parts or member.issym() or member.islnk() or not (member.isdir() or member.isfile()):
                raise ValueError(f"unsafe archive member: {member.name}")
        tar.extractall(destination, filter="data")
    roots = list(destination.iterdir())
    if len(roots) != 1 or not roots[0].is_dir():
        raise ValueError("one source root required")
    return roots[0]


def run(command, cwd=None, env=None):
    print("+", " ".join(map(str, command)), flush=True)
    subprocess.run(list(map(str, command)), cwd=cwd, env=env, check=True)


def verify(prefix, manifest):
    names = [("avutil", 61), ("avcodec", 63), ("avformat", 63), ("swscale", 10), ("swresample", 7)]
    libraries = []
    runtime_dir = prefix / ("bin" if sys.platform == "win32" else "lib")
    dll_cookie = os.add_dll_directory(str(runtime_dir)) if sys.platform == "win32" else None
    ffmpeg_version = None
    for name, major in names:
        filename = f"{name}-{major}.dll" if sys.platform == "win32" else (f"lib{name}.{major}.dylib" if sys.platform == "darwin" else f"lib{name}.so.{major}")
        path = runtime_dir / filename
        lib = ctypes.CDLL(str(path))
        license_fn = getattr(lib, name + "_license")
        license_fn.restype = ctypes.c_char_p
        configuration_fn = getattr(lib, name + "_configuration")
        configuration_fn.restype = ctypes.c_char_p
        license_text = license_fn().decode()
        configuration = configuration_fn().decode()
        version_fn = getattr(lib, name + "_version")
        version_fn.restype = ctypes.c_uint
        if not license_text.startswith("LGPL") or "--enable-gpl" in configuration or "--enable-nonfree" in configuration or version_fn() >> 16 != major:
            raise ValueError(f"distribution license/ABI rejected: {name}: {license_text}")
        if name == "avutil":
            info = lib.av_version_info
            info.restype = ctypes.c_char_p
            ffmpeg_version = info().decode()
            expected = next(d["version"] for d in manifest["dependencies"] if d["name"] == "ffmpeg")
            if ffmpeg_version != expected:
                raise ValueError(f"pinned FFmpeg version mismatch: {ffmpeg_version}")
        libraries.append({"name": name, "version": version_fn(), "license": license_text, "configuration": configuration})
    probe = subprocess.check_output([str(prefix / "bin/ffmpeg"), "-hide_banner", "-encoders"], text=True, stderr=subprocess.STDOUT)
    if any(name not in probe for name in ["libsvtav1", "prores_ks", "pcm_s24le", "alac", "aac", "libopus", "libmp3lame"]):
        raise ValueError("required AV1, ProRes and audio encoders missing")
    decoders = subprocess.check_output([str(prefix / "bin/ffmpeg"), "-hide_banner", "-decoders"], text=True, stderr=subprocess.STDOUT)
    if "libdav1d" not in decoders:
        raise ValueError("required AV1 software decoder missing")
    external_versions = {}
    for dependency, filename, symbol in [
        ("svt-av1", "libSvtAv1Enc.4.dylib" if sys.platform == "darwin" else "libSvtAv1Enc.so.4", "svt_av1_get_version"),
        ("dav1d", "libdav1d.7.dylib" if sys.platform == "darwin" else "libdav1d.so.7", "dav1d_version"),
        ("opus", "libopus.0.dylib" if sys.platform == "darwin" else "libopus.so.0", "opus_get_version_string"),
    ]:
        if sys.platform == "win32":
            matches = list(runtime_dir.glob(f"*{dependency}*.dll" if dependency != "svt-av1" else "*SvtAv1Enc*.dll"))
            if len(matches) != 1:
                raise ValueError(f"one pinned dependency DLL required: {dependency}")
            filename = matches[0].name
        library = ctypes.CDLL(str(runtime_dir / filename))
        version = getattr(library, symbol)
        version.restype = ctypes.c_char_p
        actual = version().decode().removeprefix("v").split()[-1]
        expected = next(d["version"] for d in manifest["dependencies"] if d["name"] == dependency)
        if actual != expected:
            raise ValueError(f"pinned dependency version mismatch: {dependency}: {actual}")
        external_versions[dependency] = actual
    # LibRaw reports "0.22.2-Release" so compare the leading version field; the
    # versioned real file is what CDLL must open, not the dev symlinks.
    pattern = "*raw*.dll" if sys.platform == "win32" else ("libraw_r.*.dylib" if sys.platform == "darwin" else "libraw_r.so.*")
    matches = [p for p in runtime_dir.glob(pattern) if p.is_file() and not p.is_symlink()]
    if len(matches) != 1:
        raise ValueError(f"one pinned shared LibRaw required: {pattern}: {[p.name for p in matches]}")
    library = ctypes.CDLL(str(matches[0]))
    libraw_version = library.libraw_version
    libraw_version.restype = ctypes.c_char_p
    actual = libraw_version().decode().split("-")[0]
    expected = next(d["version"] for d in manifest["dependencies"] if d["name"] == "libraw")
    if actual != expected:
        raise ValueError(f"pinned dependency version mismatch: libraw: {actual}")
    external_versions["libraw"] = actual
    shared = sorted(p for p in runtime_dir.iterdir() if p.is_file() and not p.is_symlink() and (".so" in p.name or p.suffix in {".dylib", ".dll"}))
    if not shared or any(p.suffix == ".a" and not p.name.endswith(".dll.a") for p in (prefix / "lib").iterdir()):
        raise ValueError("shared libraries only required")
    license_files = [prefix / "licenses" / entry["name"] / name
                     for entry in manifest["dependencies"] for name in entry["license_files"]]
    receipt = {"schema_version": 1, "manifest": manifest, "libraries": libraries, "ffmpeg_version": ffmpeg_version,
               "external_versions": external_versions,
               "licenses": [{"file": str(p.relative_to(prefix)), "sha256": sha256(p)} for p in license_files],
               "shared_libraries": [{"file": p.name, "sha256": sha256(p)} for p in shared],
               "platform": sys.platform, "machine": platform.machine()}
    (prefix / "build-receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    if dll_cookie is not None:
        dll_cookie.close()
    print(f"verified five LGPL shared libraries, AV1, ProRes and delivery audio: {prefix}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--prefix", type=Path, default=ROOT / "target/native/ffmpeg-lgpl")
    parser.add_argument("--jobs", type=int, default=os.cpu_count() or 2)
    parser.add_argument("--offline", action="store_true")
    parser.add_argument("--verify-only", action="store_true")
    args = parser.parse_args()
    manifest = json.loads(MANIFEST.read_text())
    prefix = args.prefix.resolve()
    if args.verify_only:
        verify(prefix, manifest)
        return
    if args.jobs < 1 or prefix.exists():
        raise ValueError("positive jobs and a fresh output prefix required")
    bash = msys2_bash() if sys.platform == "win32" else None
    cache = ROOT / "target/native/downloads"
    cache.mkdir(parents=True, exist_ok=True)
    sources = {entry["name"]: source(entry, cache, args.offline) for entry in manifest["dependencies"]}
    work = ROOT / "target/native/build"
    if work.exists():
        shutil.rmtree(work)
    work.mkdir()
    entries = {entry["name"]: entry for entry in manifest["dependencies"]}
    svt, ffmpeg, dav1d, opus = entries["svt-av1"], entries["ffmpeg"], entries["dav1d"], entries["opus"]
    lame, libraw = entries["lame"], entries["libraw"]
    svt_source = extract(sources["svt-av1"], work / "svt-source")
    ffmpeg_source = extract(sources["ffmpeg"], work / "ffmpeg-source")
    dav1d_source = extract(sources["dav1d"], work / "dav1d-source")
    opus_source = extract(sources["opus"], work / "opus-source")
    lame_source = extract(sources["lame"], work / "lame-source")
    if sys.platform == "win32":
        # LAME 3.100's include/libmp3lame.sym still exports the deprecated API
        # (lame_init_old, lame_decode_*, ...), but include/lame.h hardcodes
        # DEPRECATED_OR_OBSOLETE_CODE_REMOVED to 1, which makes those entry
        # points static. The generated MinGW .def must resolve every listed
        # symbol or ld fails ("cannot export lame_init_old"), so keep the
        # deprecated entry points compiled on Windows.
        lame_header = lame_source / "include" / "lame.h"
        needle = "#define DEPRECATED_OR_OBSOLETE_CODE_REMOVED 1"
        text = lame_header.read_text(encoding="utf-8")
        if text.count(needle) != 1:
            raise ValueError("pinned lame.h DEPRECATED_OR_OBSOLETE_CODE_REMOVED define missing")
        lame_header.write_text(text.replace(needle, "#define DEPRECATED_OR_OBSOLETE_CODE_REMOVED 0"), encoding="utf-8")
    libraw_source = extract(sources["libraw"], work / "libraw-source")
    dav1d_build = work / "dav1d-build"
    run(["meson", "setup", dav1d_build, dav1d_source, *dav1d["meson"], f"--prefix={prefix}", "--libdir=lib"])
    run(["meson", "compile", "-C", dav1d_build, "-j", args.jobs])
    run(["meson", "install", "-C", dav1d_build])
    opus_build = work / "opus-build"
    opus_build.mkdir()
    # Autoconf splits its auxiliary-file candidates on ':' — a DOS-style
    # argv[0] (D:/...) makes every candidate unreadable, so pass the MSYS path.
    configure = msys2_posix(bash, opus_source / "configure") if bash else (opus_source / "configure").as_posix()
    run([*([bash] if bash else []), configure,
         *opus["configure"], *opus.get("platform_configure", {}).get(sys.platform, []),
         f"--prefix={prefix}", "--libdir=" + str(prefix / "lib")], cwd=opus_build)
    run(["make", f"-j{args.jobs}"], cwd=opus_build)
    run(["make", "install"], cwd=opus_build)
    svt_build = work / "svt-build"
    run(["cmake", *(["-G", "Ninja"] if sys.platform == "win32" else []), "-S", svt_source, "-B", svt_build, *svt["cmake"], *svt.get("platform_cmake", {}).get(sys.platform, []), f"-DCMAKE_INSTALL_PREFIX={prefix}", "-DCMAKE_INSTALL_LIBDIR=lib"])
    run(["cmake", "--build", svt_build, "--parallel", args.jobs])
    run(["cmake", "--install", svt_build])
    # LAME must be installed before FFmpeg configure so pkg-config exposes libmp3lame.
    lame_build = work / "lame-build"
    lame_build.mkdir()
    configure = msys2_posix(bash, lame_source / "configure") if bash else (lame_source / "configure").as_posix()
    run([*([bash] if bash else []), configure,
         *lame["configure"], *lame.get("platform_configure", {}).get(sys.platform, []),
         f"--prefix={prefix}", "--libdir=" + str(prefix / "lib")], cwd=lame_build)
    run(["make", f"-j{args.jobs}"], cwd=lame_build)
    run(["make", "install"], cwd=lame_build)
    env = dict(os.environ, PKG_CONFIG_PATH=str(prefix / "lib/pkgconfig"), PKG_CONFIG_LIBDIR=str(prefix / "lib/pkgconfig"))
    ffmpeg_build = work / "ffmpeg-build"
    ffmpeg_build.mkdir()
    flags = [*ffmpeg["configure"], f"--prefix={prefix.as_posix()}"]
    # LAME ships no pkg-config file; FFmpeg's configure probes lame/lame.h and
    # -lmp3lame, so expose the vendored prefix explicitly on every platform.
    # MSYS2 configure scripts need /d/a/... paths — a DOS-style -L/-I is split
    # on ':' or loses its backslashes under shell evaluation.
    extra_include = msys2_posix(bash, prefix / "include") if bash else (prefix / "include").as_posix()
    extra_lib = msys2_posix(bash, prefix / "lib") if bash else (prefix / "lib").as_posix()
    flags += [f"--extra-cflags=-I{extra_include}", f"--extra-ldflags=-L{extra_lib}"]
    if sys.platform == "win32":
        flags += ["--target-os=mingw32", "--arch=x86_64", "--cc=gcc", "--cxx=g++"]
    else:
        flags += [f"--extra-ldflags=-Wl,-rpath,{prefix / 'lib'}"]
    if sys.platform == "darwin":
        flags += [*ffmpeg["platform_configure"]["darwin"], f"--install-name-dir={prefix / 'lib'}"]
    run([*([bash] if bash else []), (ffmpeg_source / "configure").as_posix(), *flags], cwd=ffmpeg_build, env=env)
    run(["make", f"-j{args.jobs}"], cwd=ffmpeg_build, env=env)
    run(["make", "install"], cwd=ffmpeg_build, env=env)
    libraw_build = work / "libraw-build"
    libraw_build.mkdir()
    configure = msys2_posix(bash, libraw_source / "configure") if bash else (libraw_source / "configure").as_posix()
    run([*([bash] if bash else []), configure,
         *libraw["configure"], *libraw.get("platform_configure", {}).get(sys.platform, []),
         f"--prefix={prefix}", "--libdir=" + str(prefix / "lib")], cwd=libraw_build, env=env)
    run(["make", f"-j{args.jobs}"], cwd=libraw_build, env=env)
    run(["make", "install"], cwd=libraw_build, env=env)
    if sys.platform == "win32":
        # Copy only the MinGW runtime DLLs into the explicit runtime directory;
        # loading the finished runtime never depends on MSYS being on PATH.
        for name in ["libwinpthread-1.dll", "libgcc_s_seh-1.dll", "libstdc++-6.dll"]:
            dependency = next((Path(part) / name for part in os.environ.get("PATH", "").split(os.pathsep) if (Path(part) / name).is_file()), None)
            if dependency is None:
                raise ValueError(f"missing MinGW runtime DLL: {name}")
            shutil.copy2(dependency, prefix / "bin" / name)
    licenses = prefix / "licenses"
    licenses.mkdir()
    for entry, source_dir in [(ffmpeg, ffmpeg_source), (svt, svt_source), (dav1d, dav1d_source), (opus, opus_source), (lame, lame_source), (libraw, libraw_source)]:
        destination = licenses / entry["name"]
        destination.mkdir()
        for name in entry["license_files"]:
            shutil.copy2(source_dir / name, destination / name)
    shutil.copy2(MANIFEST, prefix / MANIFEST.name)
    verify(prefix, manifest)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"FFMPEG_BUILD_ERROR: {error}", file=sys.stderr)
        sys.exit(1)
