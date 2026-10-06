#!/usr/bin/env python3
"""Build the Rust cdylib/CLI and prepare the local SwiftPM link directory."""
import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--release", action="store_true")
    args = parser.parse_args()
    subprocess.run(["python3", "scripts/generate_swift_api.py", "--check"], cwd=ROOT, check=True)
    env = dict(os.environ)
    env["CARGO_BUILD_JOBS"] = "3"
    command = ["cargo", "build", "-p", "kronello-ffi", "-p", "kronello-cli", "--locked", "--message-format=json-render-diagnostics"]
    if args.release:
        command.append("--release")
        # Rust 1.95's Mach-O debug stripping can misalign the LINKEDIT string
        # pool, which Xcode 27 rejects. Keep the final FFI dylib unstripped;
        # release optimization and the CLI/dependency profiles are unchanged.
        if sys.platform == "darwin":
            command.extend(["--config", 'profile.release.package.kronello-ffi.strip="none"'])
    result = subprocess.run(command, cwd=ROOT, env=env, check=True, text=True, stdout=subprocess.PIPE)
    artifacts = {}
    for line in result.stdout.splitlines():
        entry = json.loads(line)
        if entry.get("reason") == "compiler-artifact":
            target = entry["target"]["name"]
            if target == "kronello_ffi":
                artifacts["library"] = next(Path(p) for p in entry["filenames"] if p.endswith(".dylib"))
            if entry.get("executable") and target == "kronello":
                artifacts["cli"] = Path(entry["executable"])
    destination = ROOT / "apps/macos/Libraries"
    destination.mkdir(parents=True, exist_ok=True)
    for source in artifacts.values():
        shutil.copy2(source, destination / source.name)
    if set(artifacts) != {"library", "cli"}:
        raise SystemExit("Expected the Rust macOS cdylib and CLI artifacts")
    print(destination)

if __name__ == "__main__":
    main()
