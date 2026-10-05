#!/usr/bin/env python3
"""Assemble and ad-hoc sign a development app. Does not bundle FFmpeg runtimes."""
import argparse
import plistlib
import re
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PACKAGE = ROOT / "apps/macos"


def run(*args):
    return subprocess.run(list(map(str, args)), cwd=ROOT, check=True, text=True, stdout=subprocess.PIPE).stdout


def assemble(bin_path, output):
    binary = bin_path / "Kronello"
    library = PACKAGE / "Libraries/libkronello_ffi.dylib"
    worker = PACKAGE / "Libraries/kronello"
    resource = bin_path / "Kronello_KronelloDesign.bundle"
    for source in [binary, library, worker, resource]:
        if not source.exists():
            raise RuntimeError(f"Missing build artifact: {source}")
    if not list(resource.rglob("*.ttf")):
        raise RuntimeError("UI font resource bundle is empty; run scripts/fetch_ui_fonts.py and rebuild")
    staging = output.with_name("Kronello.staging.app")
    if staging.exists():
        shutil.rmtree(staging)
    contents = staging / "Contents"
    for folder in ["MacOS", "Frameworks", "Helpers", "Resources"]:
        (contents / folder).mkdir(parents=True, exist_ok=True)
    executable = contents / "MacOS/Kronello"
    shutil.copy2(binary, executable)
    shutil.copy2(library, contents / "Frameworks" / library.name)
    shutil.copy2(worker, contents / "Helpers/kronello")
    shutil.copytree(resource, contents / "Resources" / resource.name)
    license_dir = contents / "Resources/Licenses"
    license_dir.mkdir()
    for path in (ROOT / "third_party/fonts").glob("**/*"):
        if path.is_file():
            destination = license_dir / path.relative_to(ROOT / "third_party/fonts")
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, destination)
    # The UI is Japanese; declare it so AppKit localizes the standard menus.
    (contents / "Resources/ja.lproj").mkdir()
    (contents / "Resources/ja.lproj/InfoPlist.strings").write_text("", encoding="utf-8")
    info = {
        "CFBundleDevelopmentRegion": "ja", "CFBundleLocalizations": ["ja"],
        "CFBundleIdentifier": "dev.kronello.Kronello", "CFBundleName": "Kronello", "CFBundleDisplayName": "Kronello",
        "CFBundleExecutable": "Kronello", "CFBundlePackageType": "APPL", "CFBundleVersion": "1", "CFBundleShortVersionString": "0.0.0",
        "LSMinimumSystemVersion": "14.0", "NSHighResolutionCapable": True, "NSPrincipalClass": "NSApplication",
        "CFBundleDocumentTypes": [{"CFBundleTypeName": "Kronello Project", "CFBundleTypeRole": "Editor", "LSItemContentTypes": ["dev.kronello.project"]}],
        "UTExportedTypeDeclarations": [{"UTTypeIdentifier": "dev.kronello.project", "UTTypeDescription": "Kronello Project",
            "UTTypeConformsTo": ["public.data"], "UTTypeTagSpecification": {"public.filename-extension": ["kronello"]}}],
    }
    with (contents / "Info.plist").open("wb") as file:
        plistlib.dump(info, file)
    paths = re.findall(r"\n\s*path (.*?) \(offset", run("otool", "-l", executable))
    development = str(PACKAGE / "Libraries")
    if development in paths:
        run("install_name_tool", "-delete_rpath", development, executable)
    relative = "@executable_path/../Frameworks"
    if relative not in paths:
        run("install_name_tool", "-add_rpath", relative, executable)
    if "@rpath/libkronello_ffi.dylib" not in run("otool", "-D", contents / "Frameworks" / library.name):
        raise RuntimeError("FFI library install name must be @rpath/libkronello_ffi.dylib")
    for path in [contents / "Frameworks" / library.name, contents / "Helpers/kronello", staging]:
        run("codesign", "--force", "--sign", "-", "--timestamp=none", path)
    run("codesign", "--verify", "--deep", "--strict", staging)
    if output.exists():
        shutil.rmtree(output)
    staging.rename(output)
    return output


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--skip-build", action="store_true", help="Use previously built FFI and Swift artifacts")
    parser.add_argument("--release", action="store_true", help="Build the FFI and helper CLI optimized (use for review and timing)")
    args = parser.parse_args()
    if not args.skip_build:
        run("python3", "scripts/build_ffi.py", *(["--release"] if args.release else []))
        run("python3", "scripts/fetch_ui_fonts.py")
        run("swift", "build", "--package-path", PACKAGE, "-j", "3")
    bin_path = Path(run("swift", "build", "--package-path", PACKAGE, "--show-bin-path").strip())
    output = ROOT / "target/macos/Kronello.app"
    output.parent.mkdir(parents=True, exist_ok=True)
    print(assemble(bin_path, output))


if __name__ == "__main__":
    main()
