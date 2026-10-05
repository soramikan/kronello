#!/usr/bin/env python3
"""Direct compiler GUI checks, independent of SwiftPM and native window execution."""
import argparse
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PACKAGE = ROOT / "apps/macos"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--swiftc", default=shutil.which("swiftc"))
    parser.add_argument("--sdk", default=None)
    parser.add_argument("--run-checks", action="store_true")
    parser.add_argument("--skip-modules", action="store_true", help="Reuse modules from a preceding direct check")
    parser.add_argument("--disable-plugin-sandbox", action="store_true", help="Avoid a nested compiler subprocess sandbox; the caller sandbox still applies")
    args = parser.parse_args()
    sdk = args.sdk or subprocess.check_output(["xcrun", "--show-sdk-path"], text=True).strip()
    output = PACKAGE / ".build/gui-direct"
    output.mkdir(parents=True, exist_ok=True)
    common = [args.swiftc, "-swift-version", "5", "-target", "arm64-apple-macosx14.0", "-sdk", sdk,
              "-module-cache-path", str(output / "ModuleCache"), "-I", str(PACKAGE / "Sources/CKronelloFFI/include"),
              "-I", str(output), "-L", str(output), "-L", str(PACKAGE / "Libraries"),
              "-Xlinker", "-rpath", "-Xlinker", str(output), "-Xlinker", "-rpath", "-Xlinker", str(PACKAGE / "Libraries")]
    if args.disable_plugin_sandbox:
        common += ["-disable-sandbox"]
    # Only the direct check uses this accessor. SwiftPM uses its own generated Bundle.module.
    accessor = output / "Resources.swift"
    accessor.write_text("import Foundation\nextension Bundle { static let module = Bundle(path: "
                        + '"' + str(PACKAGE / "Sources/KronelloDesign/Resources") + '"' + ")! }\n")
    modules = [("KronelloCore", ["-lkronello_ffi"]), ("KronelloDesign", []), ("KronelloAppModel", ["-lKronelloCore", "-lKronelloDesign"])]
    for module, links in ([] if args.skip_modules else modules):
        sources = sorted((PACKAGE / f"Sources/{module}").rglob("*.swift"))
        if module == "KronelloDesign":
            sources.append(accessor)
        subprocess.run(common + ["-parse-as-library", "-enable-testing", "-emit-module", "-emit-library", "-module-name", module,
                                "-emit-module-path", str(output / f"{module}.swiftmodule"),
                                "-o", str(output / f"lib{module}.dylib")] + links + [str(p) for p in sources], check=True)
    for module in ["Kronello", "KronelloDesignGallery"]:
        flags = ["-typecheck", "-module-name", module]
        if module == "Kronello":
            flags += ["-parse-as-library"]
        subprocess.run(common + flags + [str(p) for p in sorted((PACKAGE / f"Sources/{module}").glob("*.swift"))], check=True)
    developer = Path(sdk).parent.parent
    subprocess.run(common + ["-typecheck", "-module-name", "KronelloAppModelTests",
                            "-F", str(developer / "Library/Frameworks"), "-I", str(developer / "usr/lib")]
                   + [str(p) for p in sorted((PACKAGE / "Tests/KronelloAppModelTests").glob("*.swift"))], check=True)
    if args.run_checks:
        runner = output / "GUIRunner.swift"
        runner.write_text(r'''import Foundation
import Darwin
@main struct Runner {
    @MainActor static func main() async {
        setbuf(stdout, nil)
        do { try await GUIChecks().runAll(); try await MotionChecks().runAll() }
        catch { fputs("GUI checks failed: \(error)\n", stderr); exit(1) }
    }
}
''')
        subprocess.run(common + ["-parse-as-library", "-lKronelloAppModel", "-lKronelloCore", "-lKronelloDesign", "-o", str(output / "GUIRunner"),
                                str(PACKAGE / "Tests/KronelloAppModelTests/GUIChecks.swift"), str(PACKAGE / "Tests/KronelloAppModelTests/MotionChecks.swift"), str(runner)], check=True)
        subprocess.run([str(output / "GUIRunner")], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
