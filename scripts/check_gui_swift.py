#!/usr/bin/env python3
"""Direct compiler GUI checks, independent of SwiftPM and native window execution."""
import argparse
import re
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PACKAGE = ROOT / "apps/macos"


def check_focus_ownership():
    components = PACKAGE / "Sources/KronelloDesign/Components"
    source = (components / "FocusRing.swift").read_text()
    if re.search(r"@Environment\s*\(\s*\\\.isFocused\s*\)", source):
        raise AssertionError("Focus rings must use the control's own focus, never inherited ancestor isFocused")
    for path in components.glob("*.swift"):
        if re.search(r"\.krFocusRing\(\s*\)", path.read_text()):
            raise AssertionError(f"{path.name}: a control must explicitly supply focus or own it via krControlFocusRing")
    print("PASS focus-ring source regression: no inherited ancestor focus; controls own focus explicitly", flush=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--swiftc", default=shutil.which("swiftc"))
    parser.add_argument("--sdk", default=None)
    parser.add_argument("--clang", default=None)
    parser.add_argument("--run-checks", action="store_true")
    parser.add_argument("--skip-modules", action="store_true", help="Reuse modules from a preceding direct check")
    parser.add_argument("--disable-plugin-sandbox", action="store_true", help="Avoid a nested compiler subprocess sandbox; the caller sandbox still applies")
    args = parser.parse_args()
    check_focus_ownership()
    sdk = args.sdk or subprocess.check_output(["xcrun", "--show-sdk-path"], text=True).strip()
    output = PACKAGE / ".build/gui-direct"
    output.mkdir(parents=True, exist_ok=True)
    common = [args.swiftc, "-swift-version", "5", "-target", "arm64-apple-macosx14.0", "-sdk", sdk,
              "-module-cache-path", str(output / "ModuleCache"), "-I", str(PACKAGE / "Sources/CKronelloFFI/include"),
              "-I", str(output), "-L", str(output), "-L", str(PACKAGE / "Libraries"),
              "-Xlinker", "-rpath", "-Xlinker", str(output), "-Xlinker", "-rpath", "-Xlinker", str(PACKAGE / "Libraries")]
    if args.disable_plugin_sandbox:
        common += ["-disable-sandbox"]
    native = output / "playback.o"
    clang = args.clang or str(Path(args.swiftc).with_name("clang"))
    subprocess.run([clang, "-std=c11", "-Wall", "-Wextra", "-Werror", "-isysroot", sdk,
                    "-target", "arm64-apple-macosx14.0", "-c", str(PACKAGE / "Sources/CKronelloFFI/playback.c"),
                    "-o", str(native)], check=True)
    if args.run_checks:
        native_checks = output / "PlaybackNativeChecks"
        subprocess.run([clang, "-std=c11", "-Wall", "-Wextra", "-Werror", "-isysroot", sdk,
                        "-target", "arm64-apple-macosx14.0", "-framework", "AudioToolbox",
                        str(PACKAGE / "Tests/PlaybackNativeChecks.c"), "-o", str(native_checks)], check=True)
        subprocess.run([str(native_checks)], check=True)
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
                                "-o", str(output / f"lib{module}.dylib")] + links + ([str(native)] if module == "KronelloCore" else []) + [str(p) for p in sources], check=True)
    for module in ["Kronello", "KronelloDesignGallery", "KronelloAudioHarness"]:
        flags = ["-typecheck", "-module-name", module]
        if module == "Kronello":
            flags += ["-parse-as-library"]
        subprocess.run(common + flags + [str(p) for p in sorted((PACKAGE / f"Sources/{module}").glob("*.swift"))], check=True)
        if module == "KronelloAudioHarness" and args.run_checks:
            subprocess.run(common + ["-lKronelloAppModel", "-lKronelloCore", "-lKronelloDesign",
                            "-o", str(output / "KronelloAudioHarness")]
                           + [str(p) for p in sorted((PACKAGE / f"Sources/{module}").glob("*.swift"))], check=True)
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
        do { try await GUIChecks().runAll(); try await MotionChecks().runAll(); try await PlaybackChecks().runAll(); try await EditChecks().runAll() }
        catch { fputs("GUI checks failed: \(error)\n", stderr); exit(1) }
    }
}
''')
        subprocess.run(common + ["-parse-as-library", "-lKronelloAppModel", "-lKronelloCore", "-lKronelloDesign", "-o", str(output / "GUIRunner"),
                                str(PACKAGE / "Tests/KronelloAppModelTests/GUIChecks.swift"), str(PACKAGE / "Tests/KronelloAppModelTests/MotionChecks.swift"),
                                str(PACKAGE / "Tests/KronelloAppModelTests/PlaybackChecks.swift"), str(PACKAGE / "Tests/KronelloAppModelTests/EditChecks.swift"), str(runner)], check=True)
        subprocess.run([str(output / "GUIRunner")], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
