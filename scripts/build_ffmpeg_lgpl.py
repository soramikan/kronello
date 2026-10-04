#!/usr/bin/env python3
"""Build pinned, replaceable LGPL FFmpeg shared libraries (never system FFmpeg)."""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "scripts/native-dependencies.json"


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
            with urllib.request.urlopen(entry["url"], timeout=60) as response:
                if not response.geturl().startswith("https://"):
                    raise ValueError("HTTPS redirect required")
                with temporary.open("wb") as out:
                    shutil.copyfileobj(response, out)
            if sha256(temporary) != entry["sha256"]:
                raise ValueError(f"source hash mismatch: {entry['name']}")
            temporary.replace(path)
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
    names = [("avutil", 61), ("avcodec", 63), ("avformat", 63), ("swscale", 10)]
    libraries = []
    for name, major in names:
        filename = f"lib{name}.{major}.dylib" if sys.platform == "darwin" else f"lib{name}.so.{major}"
        path = prefix / "lib" / filename
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
        libraries.append({"name": name, "version": version_fn(), "license": license_text, "configuration": configuration})
    probe = subprocess.check_output([str(prefix / "bin/ffmpeg"), "-hide_banner", "-encoders"], text=True, stderr=subprocess.STDOUT)
    if "libsvtav1" not in probe or "prores_ks" not in probe:
        raise ValueError("required AV1 and ProRes encoders missing")
    decoders = subprocess.check_output([str(prefix / "bin/ffmpeg"), "-hide_banner", "-decoders"], text=True, stderr=subprocess.STDOUT)
    if "libdav1d" not in decoders:
        raise ValueError("required AV1 software decoder missing")
    shared = sorted(p for p in (prefix / "lib").iterdir() if p.is_file() and not p.is_symlink() and (".so" in p.name or p.suffix == ".dylib"))
    if not shared or any(p.suffix == ".a" for p in (prefix / "lib").iterdir()):
        raise ValueError("shared libraries only required")
    receipt = {"schema_version": 1, "manifest": manifest, "libraries": libraries,
               "shared_libraries": [{"file": p.name, "sha256": sha256(p)} for p in shared],
               "platform": sys.platform, "machine": os.uname().machine}
    (prefix / "build-receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(f"verified LGPL shared libraries, AV1 and ProRes: {prefix}")


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
    cache = ROOT / "target/native/downloads"
    cache.mkdir(parents=True, exist_ok=True)
    sources = {entry["name"]: source(entry, cache, args.offline) for entry in manifest["dependencies"]}
    work = ROOT / "target/native/build"
    if work.exists():
        shutil.rmtree(work)
    work.mkdir()
    entries = {entry["name"]: entry for entry in manifest["dependencies"]}
    svt, ffmpeg, dav1d = entries["svt-av1"], entries["ffmpeg"], entries["dav1d"]
    svt_source = extract(sources["svt-av1"], work / "svt-source")
    ffmpeg_source = extract(sources["ffmpeg"], work / "ffmpeg-source")
    dav1d_source = extract(sources["dav1d"], work / "dav1d-source")
    dav1d_build = work / "dav1d-build"
    run(["meson", "setup", dav1d_build, dav1d_source, *dav1d["meson"], f"--prefix={prefix}", "--libdir=lib"])
    run(["meson", "compile", "-C", dav1d_build, "-j", args.jobs])
    run(["meson", "install", "-C", dav1d_build])
    svt_build = work / "svt-build"
    run(["cmake", "-S", svt_source, "-B", svt_build, *svt["cmake"], *svt.get("platform_cmake", {}).get(sys.platform, []), f"-DCMAKE_INSTALL_PREFIX={prefix}", "-DCMAKE_INSTALL_LIBDIR=lib"])
    run(["cmake", "--build", svt_build, "--parallel", args.jobs])
    run(["cmake", "--install", svt_build])
    env = dict(os.environ, PKG_CONFIG_PATH=str(prefix / "lib/pkgconfig"), PKG_CONFIG_LIBDIR=str(prefix / "lib/pkgconfig"))
    ffmpeg_build = work / "ffmpeg-build"
    ffmpeg_build.mkdir()
    flags = [*ffmpeg["configure"], f"--prefix={prefix}", f"--extra-ldflags=-Wl,-rpath,{prefix / 'lib'}"]
    if sys.platform == "darwin":
        flags += [*ffmpeg["platform_configure"]["darwin"], f"--install-name-dir={prefix / 'lib'}"]
    run([ffmpeg_source / "configure", *flags], cwd=ffmpeg_build, env=env)
    run(["make", f"-j{args.jobs}"], cwd=ffmpeg_build, env=env)
    run(["make", "install"], cwd=ffmpeg_build, env=env)
    licenses = prefix / "licenses"
    licenses.mkdir()
    for entry, source_dir in [(ffmpeg, ffmpeg_source), (svt, svt_source), (dav1d, dav1d_source)]:
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
