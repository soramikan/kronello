#!/usr/bin/env python3
"""Supplement SwiftPM with direct compiler checks in restricted worker sandboxes.

This is not a swift build/test acceptance verdict. It compiles the shipped
sources and runs the same throwing checks that the XCTest target wraps.
"""
import argparse
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PACKAGE = ROOT / "apps/macos"

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--swiftc", default=shutil.which("swiftc"))
    parser.add_argument("--sdk", required=True)
    args = parser.parse_args()
    output = PACKAGE / ".build/direct"
    output.mkdir(parents=True, exist_ok=True)
    libraries = PACKAGE / "Libraries"
    common = [args.swiftc, "-swift-version", "5", "-target", "arm64-apple-macosx14.0",
              "-sdk", args.sdk, "-module-cache-path", str(output / "ModuleCache"),
              "-I", str(PACKAGE / "Sources/CKronelloFFI/include")]
    core_sources = sorted((PACKAGE / "Sources/KronelloCore").glob("*.swift"))
    subprocess.run(common + ["-parse-as-library", "-emit-module", "-emit-library", "-module-name", "KronelloCore",
                   "-emit-module-path", str(output / "KronelloCore.swiftmodule"),
                   "-L", str(libraries), "-lkronello_ffi", "-Xlinker", "-rpath", "-Xlinker", str(libraries),
                   "-o", str(output / "libKronelloCore.dylib")] + [str(p) for p in core_sources], check=True)
    consumer = common + ["-I", str(output), "-L", str(output), "-lKronelloCore",
                         "-Xlinker", "-rpath", "-Xlinker", str(output)]
    for target in ["KronelloPreviewHarness", "KronelloJSONBenchmark"]:
        subprocess.run(consumer + ["-o", str(output / target), str(PACKAGE / f"Sources/{target}/main.swift")], check=True)
    runner = output / "SmokeRunner.swift"
    runner.write_text("""import Foundation
@main struct SmokeRunner {
    @MainActor static func main() async throws {
        let checks = CoreChecks()
        try await checks.verifySharedRevisionEventAndCLIIdempotency()
        print("PASS shared Swift/CLI revision and exact Event replay")
        try checks.verifyGeneratedTypesPreserveUnknownProjectAndRationalStrings()
        print("PASS schema unknown fields and exact integer/rational strings")
        try await checks.verifyRawDuplicateRejectionAndClosedHandle()
        print("PASS raw duplicate rejection and closed session")
        try await checks.verifyRawArbitraryPrecisionResponse()
        print("PASS arbitrary-precision raw response framing")
    }
}
""")
    subprocess.run(consumer + ["-parse-as-library", "-o", str(output / "SmokeRunner"),
                   str(PACKAGE / "Tests/KronelloCoreTests/CoreChecks.swift"), str(runner)], check=True)
    subprocess.run([str(output / "SmokeRunner")], check=True)
    subprocess.run([str(output / "KronelloJSONBenchmark"),
                    str(ROOT / "examples/ffi-json-benchmark.request.json")], check=True)

if __name__ == "__main__":
    main()
