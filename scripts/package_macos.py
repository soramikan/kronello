#!/usr/bin/env python3
"""Build a pinned macOS CLI/MCP runtime package; never copy system FFmpeg."""
import argparse
import json
import os
from pathlib import Path
import shutil
import shlex
import subprocess
import sys
import tarfile

from release_common import (EXECUTABLES, FFMPEG, LIBRARIES, ROOT, Runner, check_linkage,
                            dependencies, inventory, metadata, package_hash, rpaths,
                            sha256, system_dependency, write_json)


def check_prefix(prefix, sources):
    manifest = json.loads((ROOT / "scripts/native-dependencies.json").read_text())
    if json.loads((prefix / "native-dependencies.json").read_text()) != manifest:
        raise ValueError("prefix source manifest differs from the pinned repository manifest")
    receipt = json.loads((prefix / "build-receipt.json").read_text())
    if receipt["manifest"] != manifest or receipt["platform"] != "darwin":
        raise ValueError("pinned macOS build receipt required")
    if receipt["ffmpeg_version"] != "9.0.2":
        raise ValueError("pinned FFmpeg 9.0.2 version required")
    if receipt["external_versions"] != {"svt-av1": "4.2.0", "dav1d": "1.5.4"}:
        raise ValueError("pinned SVT-AV1/dav1d versions required")
    libs = {entry["name"]: entry for entry in receipt["libraries"]}
    if set(libs) != set(FFMPEG):
        raise ValueError("five-library receipt required; rerun build_ffmpeg_lgpl.py --verify-only on a writable prefix")
    for name, major in FFMPEG.items():
        entry = libs[name]
        config = entry["configuration"].split()
        flags = set(manifest["dependencies"][0]["configure"])
        flags.update(manifest["dependencies"][0]["platform_configure"]["darwin"])
        if (entry["version"] >> 16 != major or not entry["license"].startswith("LGPL")
                or not flags.issubset(config) or {"--enable-gpl", "--enable-nonfree"}.intersection(config)):
            raise ValueError(f"non-distribution license/configuration/ABI: {name}")
    hashes = {entry["file"]: entry["sha256"] for entry in receipt["shared_libraries"]}
    for name in LIBRARIES:
        original = (prefix / "lib" / name).resolve(strict=True)
        if original.parent != (prefix / "lib").resolve() or sha256(original) != hashes.get(original.name):
            raise ValueError(f"runtime provenance/hash mismatch: {name}")
    licenses = {entry["file"]: entry["sha256"] for entry in receipt["licenses"]}
    for entry in manifest["dependencies"]:
        archive = sources / entry["url"].rsplit("/", 1)[1]
        if sha256(archive) != entry["sha256"]:
            raise ValueError(f"pinned source archive hash mismatch: {entry['name']}")
        with tarfile.open(archive) as tar:
            members = tar.getmembers()
            roots = {Path(member.name).parts[0] for member in members if Path(member.name).parts}
            if len(roots) != 1:
                raise ValueError("one source archive root required")
            source_root = roots.pop()
            for name in entry["license_files"]:
                member = tar.getmember(f"{source_root}/{name}")
                if not member.isfile():
                    raise ValueError("license must be a regular source archive member")
                with tar.extractfile(member) as stream:
                    if stream.read() != (prefix / "licenses" / entry["name"] / name).read_bytes():
                        raise ValueError(f"license differs from pinned source: {entry['name']}/{name}")
        for name in entry["license_files"]:
            relative = f"licenses/{entry['name']}/{name}"
            if sha256(prefix / relative) != licenses.get(relative):
                raise ValueError(f"license/PATENTS hash mismatch: {relative}")
    return manifest, receipt


def build_binaries(prefix, runner):
    env = dict(os.environ, CARGO_BUILD_JOBS="3", PKG_CONFIG_PATH=str(prefix / "lib/pkgconfig"),
               PKG_CONFIG_LIBDIR=str(prefix / "lib/pkgconfig"), PKG_CONFIG_ALLOW_SYSTEM_LIBS="1",
               PKG_CONFIG_ALLOW_SYSTEM_CFLAGS="1")
    flags = (env["CARGO_ENCODED_RUSTFLAGS"].split("\x1f") if env.get("CARGO_ENCODED_RUSTFLAGS")
             else shlex.split(env.get("RUSTFLAGS", "")))
    env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(flags + ["-C", "link-arg=-Wl,-headerpad_max_install_names"])
    # cargo's pkg-config build dependency tracks these environment changes.
    commands = [
        ["cargo", "build", "--release", "--locked", "-p", "kronello-cli", "-p", "kronello-mcp", "--message-format=json-render-diagnostics"],
        ["cargo", "build", "--release", "--locked", "-p", "kronello-media", "--example", "release_roundtrip", "--message-format=json-render-diagnostics"],
    ]
    artifacts = {}
    for command in commands:
        output = runner.run(command, cwd=ROOT, env=env, timeout=1800)
        for line in output.splitlines():
            entry = json.loads(line)
            if entry.get("reason") == "compiler-artifact" and entry.get("executable"):
                artifacts[entry["target"]["name"]] = Path(entry["executable"])
    if not {"kronello", "kronello-mcp", "release_roundtrip"}.issubset(artifacts):
        raise ValueError("expected CLI, MCP and media acceptance executables")
    return artifacts


def rewrite(path, prefix, runner, library, build_prefix):
    anchor = "@loader_path" if library else "@executable_path/../lib"
    for dependency in set(dependencies(runner.run(["otool", "-L", "-arch", "all", path]))):
        if system_dependency(dependency):
            continue
        # Resolve only names from the declared build prefix or @rpath. Never
        # turn a Homebrew/system development dependency into a bundled one.
        if any(dependency.startswith(str(base / "lib") + "/") for base in [prefix, build_prefix]):
            source = prefix / "lib" / Path(dependency).name
        elif dependency.startswith("@rpath/"):
            source = prefix / "lib" / dependency[len("@rpath/"):]
        else:
            raise ValueError(f"unexpected input dependency: {path}: {dependency}")
        matching = [name for name in LIBRARIES if (prefix / "lib" / name).resolve() == source.resolve()]
        if len(matching) != 1:
            raise ValueError(f"unbundled input dependency: {dependency}")
        runner.run(["install_name_tool", "-change", dependency, f"{anchor}/{matching[0]}", path])
    if library:
        runner.run(["install_name_tool", "-id", f"@rpath/{path.name}", path])
    for old in set(rpaths(runner.run(["otool", "-l", "-arch", "all", path]))):
        runner.run(["install_name_tool", "-delete_rpath", old, path])
    runner.run(["install_name_tool", "-add_rpath", anchor, path])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--prefix", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--sources", type=Path, default=ROOT / "target/native/downloads", help="pinned source archives, used to verify original license texts")
    parser.add_argument("--unsigned", action="store_true", help="assembly only; verification will reject this candidate")
    parser.add_argument("--sign-identity", default="-", help="ad-hoc '-' or Developer ID Application identity in the host keychain")
    args = parser.parse_args()
    if sys.platform != "darwin":
        raise ValueError("macOS packaging requires a macOS host")
    prefix, output = args.prefix.resolve(strict=True), args.output.resolve()
    if output.exists() or output.is_relative_to(prefix) or prefix.is_relative_to(output):
        raise ValueError("fresh output independent of native prefix required")
    manifest, receipt = check_prefix(prefix, args.sources.resolve(strict=True))
    configuration = shlex.split(receipt["libraries"][0]["configuration"])
    build_prefixes = [Path(flag.split("=", 1)[1]).resolve() for flag in configuration if flag.startswith("--prefix=")]
    if len(build_prefixes) != 1:
        raise ValueError("one native build prefix required in configuration")
    build_prefix = build_prefixes[0]
    runner = Runner()
    revision = runner.run(["git", "rev-parse", "HEAD"], cwd=ROOT).strip()
    status = runner.run(["git", "status", "--porcelain"], cwd=ROOT)
    diff = runner.run(["git", "diff", "--binary", "HEAD"], cwd=ROOT)
    source_names = runner.run(["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"], cwd=ROOT).split("\0")
    source_files = {name: sha256(ROOT / name) for name in source_names if name and (ROOT / name).is_file()}
    artifacts = build_binaries(prefix, runner)
    output.mkdir(parents=True)
    for directory in ["bin", "lib", "tools", "licenses"]:
        (output / directory).mkdir()
    for relative in EXECUTABLES:
        shutil.copy2(artifacts[Path(relative).name], output / relative)
    for name in LIBRARIES:
        shutil.copy2((prefix / "lib" / name).resolve(), output / "lib" / name)
    for entry in manifest["dependencies"]:
        destination = output / "licenses" / entry["name"]
        destination.mkdir()
        for name in entry["license_files"]:
            shutil.copy2(prefix / "licenses" / entry["name"] / name, destination / name)
    for name in ["LICENSE-MIT", "LICENSE-APACHE"]:
        shutil.copy2(ROOT / name, output / "licenses" / name)
    shutil.copy2(ROOT / "scripts/native-dependencies.json", output / "native-dependencies.json")
    shutil.copy2(prefix / "build-receipt.json", output / "native-build-receipt.json")
    # Preserve the exact recipes used to produce the bundled runtime.
    shutil.copy2(ROOT / "scripts/build_ffmpeg_lgpl.py", output / "tools/build_ffmpeg_lgpl.py")
    shutil.copy2(ROOT / "scripts/macos_release.entitlements.plist", output / "tools/macos_release.entitlements.plist")
    for name in LIBRARIES:
        rewrite(output / "lib" / name, prefix, runner, True, build_prefix)
    for relative in EXECUTABLES:
        rewrite(output / relative, prefix, runner, False, build_prefix)
    linkage = check_linkage(output, runner)
    if not args.unsigned:
        # Sign inside out after all install-name/rpath changes. Ad-hoc signing
        # is a local relocation check, not Developer ID/Gatekeeper approval.
        for relative in [f"lib/{name}" for name in LIBRARIES] + EXECUTABLES:
            command = ["codesign", "--force", "--sign", args.sign_identity]
            if args.sign_identity != "-":
                command.append("--timestamp")
                if relative in EXECUTABLES:
                    command += ["--options", "runtime", "--entitlements", output / "tools/macos_release.entitlements.plist"]
            runner.run([*command, output / relative])
            runner.run(["codesign", "--verify", "--deep", "--strict", "--verbose=2", output / relative])
    final_names = runner.run(["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"], cwd=ROOT).split("\0")
    if (final_names != source_names or runner.run(["git", "rev-parse", "HEAD"], cwd=ROOT).strip() != revision
            or runner.run(["git", "status", "--porcelain"], cwd=ROOT) != status
            or any(sha256(ROOT / name) != digest for name, digest in source_files.items())):
        raise ValueError("source files changed during packaging; rerun on a stable revision")
    import hashlib
    provenance = dict(metadata(), revision=revision, status=status,
                      tracked_diff_sha256=hashlib.sha256(diff.encode()).hexdigest(),
                      source_files=source_files, sign_identity=args.sign_identity,
                      native_prefix=str(prefix), original_native_prefix=str(build_prefix),
                      signed=not args.unsigned, commands=runner.commands, linkage=linkage)
    write_json(output / "build-provenance.json", provenance)
    files = inventory(output)
    write_json(output / "package-manifest.json", {"schema_version": 1, "platform": "darwin",
               "revision": revision, "signed": not args.unsigned, "files": files,
               "package_sha256": package_hash(files)})
    print(f"package_sha256={package_hash(files)} {output}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        print(f"PACKAGE_ERROR: {error}", file=sys.stderr)
        sys.exit(1)
