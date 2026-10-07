"""Distribution checks reject partial scans, tampering and development dependencies."""
import copy
import json
import platform
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import release_common as common
import verify_package as verify


class FakeOtool:
    def __init__(self, bad_dependency=None, bad_rpath=None):
        self.commands = []
        self.bad_dependency = bad_dependency
        self.bad_rpath = bad_rpath

    def run(self, command):
        self.commands.append(command)
        path = Path(command[-1])
        library = path.parent.name == "lib"
        anchor = "@loader_path" if library else "@executable_path/../lib"
        if command[0] == "lipo":
            return platform.machine()
        if command[1] == "-L":
            deps = [f"@rpath/{path.name}"] if library else []
            deps.append("/usr/lib/libSystem.B.dylib")
            if path.name == self.bad_dependency:
                deps.append("/opt/homebrew/lib/libx264.dylib")
            return str(path) + ":\n" + "".join(f"\t{dep} (compatibility version 1.0.0, current version 1.0.0)\n" for dep in deps)
        if command[1] == "-l":
            if path.name == self.bad_rpath:
                anchor = "/opt/homebrew/lib"
            return f"Load command 1\n          cmd LC_RPATH\n      cmdsize 40\n         path {anchor} (offset 12)\n"
        if command[1] == "-D":
            return f"{path}:\n@rpath/{path.name}\n"
        raise AssertionError(command)


class ReleasePackageTests(unittest.TestCase):
    def macho_tree(self, root):
        for relative in common.EXECUTABLES + [f"lib/{name}" for name in common.LIBRARIES]:
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"\xcf\xfa\xed\xfe")

    def test_all_eleven_machos_are_scanned_including_five_ffmpeg_libraries(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.macho_tree(root)
            runner = FakeOtool()
            result = common.check_linkage(root, runner)
            self.assertEqual(len(result), 11)
            self.assertEqual(len([c for c in runner.commands if c[1] == "-l"]), 11)
            self.assertTrue(all(c[2:4] == ["-arch", "all"] for c in runner.commands if c[0] == "otool"))
            for name in common.FFMPEG:
                self.assertIn(f"lib/lib{name}.{common.FFMPEG[name]}.dylib", result)

    def test_swresample_rpath_and_dav1d_development_link_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.macho_tree(root)
            for runner in [FakeOtool(bad_rpath="libswresample.7.dylib"), FakeOtool(bad_dependency="libdav1d.7.dylib")]:
                with self.assertRaises(ValueError):
                    common.check_linkage(root, runner)

    def test_missing_non_macho_and_unexpected_macho_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.macho_tree(root)
            missing = root / "lib/libswscale.10.dylib"
            missing.unlink()
            with self.assertRaises(ValueError):
                common.check_linkage(root, FakeOtool())
            missing.write_bytes(b"not a Mach-O")
            with self.assertRaises(ValueError):
                common.check_linkage(root, FakeOtool())
            missing.write_bytes(b"\xcf\xfa\xed\xfe")
            (root / "bin/ffmpeg").write_bytes(b"\xcf\xfa\xed\xfe")
            with self.assertRaises(ValueError):
                common.check_linkage(root, FakeOtool())

    def capabilities(self):
        libraries = [{"name": name, "version": major << 16, "license": "LGPL version 2.1 or later",
                      "configuration": "--disable-gpl --disable-nonfree --enable-shared --disable-static --disable-autodetect"}
                     for name, major in common.FFMPEG.items()]
        return {"library_directory": "/runtime", "substituted": False, "distribution_eligible": True,
                "development_only": False, "ffmpeg_version": "9.0.2", "libraries": libraries,
                "codecs": [{"name": name, "encoder": encoder, "decoder": not encoder}
                           for name, encoder in [("libsvtav1", True), ("prores_ks", True), ("pcm_s24le", True), ("libdav1d", False)]]}

    def test_all_five_loaded_library_abis_and_licenses_are_required(self):
        cap = self.capabilities()
        receipt = {"libraries": copy.deepcopy(cap["libraries"])}
        common.validate_capabilities(cap, Path("/runtime"), False, receipt)
        for change in ["missing", "abi", "gpl", "nonfree", "license", "codec", "directory", "substitution"]:
            broken = copy.deepcopy(cap)
            if change == "missing":
                broken["libraries"].pop()
            elif change == "abi":
                broken["libraries"][-1]["version"] = 6 << 16
            elif change in ["gpl", "nonfree"]:
                broken["libraries"][-1]["configuration"] += f" --enable-{change}"
            elif change == "license":
                broken["libraries"][-1]["license"] = "GPL version 3"
            elif change == "codec":
                broken["codecs"].pop()
            elif change == "directory":
                broken["library_directory"] = "/original-build/lib"
            else:
                broken["substituted"] = True
            with self.subTest(change=change), self.assertRaises(ValueError):
                common.validate_capabilities(broken, Path("/runtime"), False, receipt)

    def test_inventory_hash_covers_bytes_modes_and_rejects_symlinks(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            path = root / "license"
            path.write_text("original")
            before = common.package_hash(common.inventory(root))
            path.write_text("modified")
            self.assertNotEqual(before, common.package_hash(common.inventory(root)))
            before = common.package_hash(common.inventory(root))
            path.chmod(0o700)
            self.assertNotEqual(before, common.package_hash(common.inventory(root)))
            (root / "outside").symlink_to(path)
            with self.assertRaises(ValueError):
                common.inventory(root)

    def test_fat_otool_headers_and_multiple_rpaths_are_parsed(self):
        output = "/binary (architecture arm64):\n\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0)\n/binary (architecture x86_64):\n\t@executable_path/../lib/libavutil.61.dylib (compatibility version 1.0.0)\n"
        self.assertEqual(common.dependencies(output), ["/usr/lib/libSystem.B.dylib", "@executable_path/../lib/libavutil.61.dylib"])
        self.assertFalse(common.system_dependency("/usr/lib/../../opt/homebrew/lib/x.dylib"))
        self.assertEqual(common.rpaths("cmd LC_RPATH\ncmdsize 40\npath @loader_path (offset 12)\ncmd LC_RPATH\ncmdsize 60\npath /old/prefix (offset 12)"), ["@loader_path", "/old/prefix"])

    def test_failed_command_retains_exit_code_and_output(self):
        runner = common.Runner()
        with self.assertRaises(ValueError):
            runner.run([sys.executable, "-c", "import sys; print('failure'); sys.exit(17)"])
        self.assertEqual(runner.commands[0]["exit_code"], 17)
        self.assertIn("failure", runner.commands[0]["stdout"])
        self.assertEqual(runner.commands[0]["cwd"], str(Path.cwd()))

    def test_timeout_retains_partial_output_and_cwd(self):
        runner = common.Runner()
        error = subprocess.TimeoutExpired(["codec-probe"], 3, output=b"partial stdout", stderr=b"partial stderr")
        with patch.object(subprocess, "run", side_effect=error):
            with self.assertRaises(subprocess.TimeoutExpired):
                runner.run(["codec-probe"], cwd=Path.cwd(), timeout=3)
        record = runner.commands[0]
        self.assertIsNone(record["exit_code"])
        self.assertEqual(record["stdout"], "partial stdout")
        self.assertEqual(record["stderr"], "partial stderr")
        self.assertEqual(record["cwd"], str(Path.cwd()))

    def test_unsigned_and_static_candidates_cannot_claim_acceptance(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            package = root / "package"
            package.mkdir()
            (package / "package-manifest.json").write_text("{}")
            common.write_json(package / "build-provenance.json", {"native_prefix": str(root / "prefix"), "original_native_prefix": str(root / "prefix")})
            manifest = {"revision": "test", "package_sha256": "test", "signed": False}
            args = type("Args", (), {"package": package, "relocated": root / "relocated", "report": root / "report.json", "static_only": False})()
            with patch.object(verify.sys, "platform", "darwin"), patch.object(verify, "verify_inventory", return_value=(manifest, {})), patch.object(verify, "check_linkage", return_value={}):
                with self.assertRaisesRegex(ValueError, "unsigned"):
                    verify.verify(args, FakeOtool(), {})
                args.relocated = root / "static-relocated"
                args.static_only = True
                report = {}
                verify.verify(args, FakeOtool(), report)
                self.assertEqual(report["status"], "static-passed")
                self.assertIs(report["acceptance_verified"], False)


if __name__ == "__main__":
    unittest.main()
