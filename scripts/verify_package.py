#!/usr/bin/env python3
"""Relocate and verify the entire macOS package, recording every command result."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import sys

from release_common import (EXECUTABLES, LIBRARIES, ROOT, Runner, check_linkage, clean_environment,
                            inventory, metadata, package_hash, sha256, validate_capabilities, write_json)


def verify_inventory(package):
    manifest_path = package / "package-manifest.json"
    if manifest_path.is_symlink():
        raise ValueError("manifest symlink rejected")
    manifest = json.loads(manifest_path.read_text())
    files = inventory(package)
    if (manifest["schema_version"] != 1 or manifest["platform"] != "darwin"
            or files != manifest["files"] or package_hash(files) != manifest["package_sha256"]):
        raise ValueError("package inventory/hash/platform mismatch")
    native = json.loads((package / "native-dependencies.json").read_text())
    receipt = json.loads((package / "native-build-receipt.json").read_text())
    if (native != receipt["manifest"] or native != json.loads((ROOT / "scripts/native-dependencies.json").read_text())
            or receipt["ffmpeg_version"] != "9.0.2" or receipt["external_versions"] != {"svt-av1": "4.2.0", "dav1d": "1.5.4"}):
        raise ValueError("pinned source manifest mismatch")
    expected = set(EXECUTABLES + [f"lib/{name}" for name in LIBRARIES] + [
        "native-dependencies.json", "native-build-receipt.json", "build-provenance.json",
        "tools/build_ffmpeg_lgpl.py", "tools/macos_release.entitlements.plist",
        "licenses/LICENSE-MIT", "licenses/LICENSE-APACHE"])
    for entry in native["dependencies"]:
        expected.update(f"licenses/{entry['name']}/{name}" for name in entry["license_files"])
    if set(files) != expected:
        raise ValueError("unexpected/missing distribution files")
    license_hashes = {entry["file"]: entry["sha256"] for entry in receipt["licenses"]}
    for name in expected:
        if name.startswith("licenses/") and not name.startswith("licenses/LICENSE-"):
            if files[name]["sha256"] != license_hashes.get(name):
                raise ValueError(f"native license/PATENTS mismatch: {name}")
    return manifest, receipt


def cli_capabilities(binary, runner, env, cwd):
    response = json.loads(runner.run([binary, "--request-json", '{"operation":"capabilities.get"}'], env=env, cwd=cwd))
    if response["status"] != "success" or response["result"]["kind"] != "capabilities":
        raise ValueError("CLI capabilities.get failed")
    return response["result"]["value"]["media"]


def mcp_capabilities(binary, runner, env, cwd):
    requests = [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-11-25", "capabilities": {},
            "clientInfo": {"name": "release-001", "version": "1"}}},
        {"jsonrpc": "2.0", "method": "notifications/initialized"},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "capabilities.get", "arguments": {}}},
    ]
    lines = runner.run_session([binary], requests, env=env, cwd=cwd).splitlines()
    responses = {response["id"]: response for response in map(json.loads, filter(None, lines)) if "id" in response}
    if responses[1]["result"]["protocolVersion"] != "2025-11-25":
        raise ValueError("MCP initialization failed")
    result = responses[2]["result"]
    if result.get("isError", False):
        raise ValueError("MCP capabilities.get failed")
    return result["structuredContent"]["media"]


def verify(args, runner, report):
    if sys.platform != "darwin":
        raise ValueError("macOS verification requires a macOS host")
    package, relocated = args.package.resolve(strict=True), args.relocated.resolve()
    manifest, receipt = verify_inventory(package)
    provenance = json.loads((package / "build-provenance.json").read_text())
    prefix = Path(provenance["native_prefix"]).resolve()
    original_prefix = Path(provenance["original_native_prefix"]).resolve()
    # No source package/build prefix ancestor may supply the relocated runtime.
    if (relocated.exists() or relocated.is_relative_to(package) or package.is_relative_to(relocated)
            or any(relocated.is_relative_to(p) or p.is_relative_to(relocated) for p in [prefix, original_prefix])):
        raise ValueError("fresh relocation destination outside package/native prefix required")
    report.update(revision=manifest["revision"], package_sha256=manifest["package_sha256"],
                  manifest_sha256=sha256(package / "package-manifest.json"),
                  source_package=str(package), relocated=str(relocated),
                  build_provenance=provenance)
    shutil.copytree(package, relocated)
    relocated_manifest, _ = verify_inventory(relocated)
    if relocated_manifest != manifest:
        raise ValueError("relocation changed manifest")
    report["linkage"] = check_linkage(relocated, runner)
    if args.static_only:
        report.update(status="static-passed", acceptance_verified=False)
        return
    if not manifest["signed"]:
        raise ValueError("unsigned assembly cannot pass release verification")
    for relative in [f"lib/{name}" for name in LIBRARIES] + EXECUTABLES:
        runner.run(["codesign", "--verify", "--deep", "--strict", "--verbose=2", relocated / relative])
    work = args.report.resolve().parent / (args.report.stem + "-artifacts")
    work.mkdir()
    env = clean_environment()
    env["KRONELLO_STATE_ROOT"] = str(work / "state")
    cap = cli_capabilities(relocated / "bin/kronello", runner, env, work)
    validate_capabilities(cap, relocated / "lib", False, receipt)
    mcp = mcp_capabilities(relocated / "bin/kronello-mcp", runner, env, work)
    validate_capabilities(mcp, relocated / "lib", False, receipt)
    if cap != mcp:
        raise ValueError("CLI and MCP runtime capabilities differ")
    report["capabilities"] = cap
    roundtrip = json.loads(runner.run([relocated / "tools/release_roundtrip", work / "roundtrip"], env=env, cwd=work))
    if (roundtrip["status"] != "passed" or roundtrip["pcm24_samples_checked"] != 16000
            or roundtrip["video_samples_checked"] != 104448
            or not 0 <= roundtrip["video_max_error_8bit_units"] <= 4):
        raise ValueError("codec roundtrip did not complete")
    validate_capabilities(roundtrip["capabilities"], relocated / "lib", False, receipt)
    report["roundtrip"] = roundtrip
    # Exercise the explicit directory override in a separate process. This is
    # an identical ABI-compatible copy, not a claim about arbitrary third-party builds.
    replacement = work / "replacement-runtime"
    shutil.copytree(relocated / "lib", replacement)
    for name in LIBRARIES:
        runner.run(["codesign", "--force", "--sign", "-", replacement / name])
        runner.run(["codesign", "--verify", "--deep", "--strict", replacement / name])
    env["KRONELLO_FFMPEG_LIB_DIR"] = str(replacement)
    replacement_cap = cli_capabilities(relocated / "bin/kronello", runner, env, work)
    validate_capabilities(replacement_cap, replacement, True, receipt)
    replacement_mcp = mcp_capabilities(relocated / "bin/kronello-mcp", runner, env, work)
    validate_capabilities(replacement_mcp, replacement, True, receipt)
    replacement_roundtrip = json.loads(runner.run([relocated / "tools/release_roundtrip", work / "replacement-roundtrip"], env=env, cwd=work))
    validate_capabilities(replacement_roundtrip["capabilities"], replacement, True, receipt)
    if (replacement_roundtrip["status"] != "passed" or replacement_roundtrip["pcm24_samples_checked"] != 16000
            or replacement_roundtrip["video_samples_checked"] != 104448
            or not 0 <= replacement_roundtrip["video_max_error_8bit_units"] <= 4):
        raise ValueError("replacement roundtrip failed")
    report["replacement"] = {"signing": "ad-hoc", "capabilities": replacement_cap, "roundtrip": replacement_roundtrip}
    env["KRONELLO_FFMPEG_LIB_DIR"] = str(work / "missing-runtime")
    failed = json.loads(runner.run([relocated / "bin/kronello", "--request-json", '{"operation":"capabilities.get"}'], env=env, cwd=work, allowed=(1,)))
    if failed["status"] != "error" or failed["error"]["code"] != "FFMPEG_UNAVAILABLE":
        raise ValueError("invalid override silently fell back")
    env.pop("KRONELLO_FFMPEG_LIB_DIR")
    broken = work / "broken-package"
    shutil.copytree(relocated, broken)
    (broken / "lib/libswresample.7.dylib").unlink()
    failed_bundle = json.loads(runner.run([broken / "bin/kronello", "--request-json", '{"operation":"capabilities.get"}'], env=env, cwd=work, allowed=(1,)))
    if failed_bundle["status"] != "error" or failed_bundle["error"]["code"] != "FFMPEG_UNAVAILABLE":
        raise ValueError("broken package silently fell back to a development runtime")
    report["negative_checks"] = {"invalid_override": failed, "missing_swresample": failed_bundle}
    # After load, codec and replacement tests, the packaged bytes must still
    # equal the signed inventory. Artifacts are always outside the package.
    verify_inventory(relocated)
    report.update(status="passed", acceptance_verified=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--relocated", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--static-only", action="store_true", help="inventory/linkage only; never an acceptance verdict")
    args = parser.parse_args()
    report_path = args.report.resolve()
    if (report_path.exists() or report_path.is_relative_to(args.package.resolve())
            or report_path.is_relative_to(args.relocated.resolve())):
        raise ValueError("fresh report outside both package directories required")
    report_path.parent.mkdir(parents=True, exist_ok=True)
    runner = Runner()
    report = dict(metadata(), status="failed", acceptance_verified=False)
    try:
        verify(args, runner, report)
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        report["error"] = str(error)
        raise
    finally:
        report["commands"] = runner.commands
        write_json(report_path, report)
    print(f"{report['status']}: {report_path}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        print(f"PACKAGE_VERIFICATION_ERROR: {error}", file=sys.stderr)
        sys.exit(1)
