"""Pinned native build failures must never become release success."""
import ctypes
import hashlib
import io
import json
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build_ffmpeg_lgpl as build


class NativeBuildTests(unittest.TestCase):
    def test_windows_configure_uses_verified_absolute_msys2_bash(self):
        with tempfile.TemporaryDirectory() as temporary:
            executable = Path(temporary) / "bash.exe"
            executable.write_bytes(b"test executable")
            with patch.dict(build.os.environ, {"KRONELLO_MSYS2_BASH": str(executable)}), patch.object(build.shutil, "which", side_effect=AssertionError("explicit MSYS2 path must override WSL PATH")), patch.object(build.subprocess, "run", return_value=Mock(stdout="MINGW64_NT-10.0-26100\n")) as probe:
                self.assertEqual(build.msys2_bash(), executable.resolve())
                self.assertEqual(probe.call_args.args[0][0], str(executable.resolve()))
            with patch.dict(build.os.environ, {"KRONELLO_MSYS2_BASH": str(executable)}), patch.object(build.subprocess, "run", return_value=Mock(stdout="Linux\n")):
                with self.assertRaisesRegex(ValueError, "MSYS2 bash required"):
                    build.msys2_bash()

    def test_manifest_pins_license_versions_and_no_gpl(self):
        manifest = json.loads(build.MANIFEST.read_text())
        self.assertEqual(manifest["ffmpeg_major"], 9)
        self.assertEqual([d["version"] for d in manifest["dependencies"]], ["9.0.2", "4.2.0", "1.5.4", "1.5.2"])
        for dependency in manifest["dependencies"]:
            self.assertTrue(dependency["url"].startswith("https://"))
            self.assertEqual(len(bytes.fromhex(dependency["sha256"])), 32)
        svt = manifest["dependencies"][1]
        self.assertNotIn("-DEXCLUDE_HASH=ON", svt["cmake"])
        self.assertEqual(svt["platform_cmake"], {"linux": ["-DEXCLUDE_HASH=ON"]})
        flags = manifest["dependencies"][0]["configure"]
        self.assertIn("--enable-shared", flags)
        self.assertIn("--disable-static", flags)
        self.assertIn("--disable-gpl", flags)
        self.assertIn("--disable-nonfree", flags)
        self.assertIn("--disable-autodetect", flags)
        self.assertIn("--disable-network", flags)
        self.assertIn("--enable-libsvtav1", flags)
        self.assertFalse(any("x264" in f or "x265" in f or f in ["--enable-gpl", "--enable-nonfree"] for f in flags))

    def test_offline_source_missing_corrupt_and_verified_cache(self):
        with tempfile.TemporaryDirectory() as temporary:
            cache = Path(temporary)
            entry = {"name": "test", "url": "https://example.invalid/pinned.tar", "sha256": hashlib.sha256(b"verified").hexdigest()}
            with self.assertRaises(ValueError):
                build.source(entry, cache, True)
            path = cache / "pinned.tar"
            path.write_bytes(b"corrupt")
            with self.assertRaises(ValueError):
                build.source(entry, cache, True)
            path.write_bytes(b"verified")
            with patch("urllib.request.urlopen", side_effect=AssertionError("network must not be used")):
                self.assertEqual(build.source(entry, cache, True), path)

    def test_source_hash_mismatch_never_publishes(self):
        with tempfile.TemporaryDirectory() as temporary:
            cache = Path(temporary)
            entry = {"name": "test", "url": "https://example.invalid/pinned.tar", "sha256": hashlib.sha256(b"verified").hexdigest()}
            response = io.BytesIO(b"mismatch")
            response.geturl = lambda: entry["url"]
            with patch("urllib.request.urlopen", return_value=response):
                with self.assertRaises(ValueError):
                    build.source(entry, cache, False)
            self.assertEqual(list(cache.iterdir()), [])

    def test_unsafe_archive_paths_and_links_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            for index, (name, kind) in enumerate([("../escape", tarfile.REGTYPE), ("source/link", tarfile.SYMTYPE)]):
                archive = directory / f"bad{index}.tar"
                with tarfile.open(archive, "w") as tar:
                    member = tarfile.TarInfo(name)
                    member.type = kind
                    member.linkname = "/escape"
                    tar.addfile(member)
                with self.assertRaises(ValueError):
                    build.extract(archive, directory / f"out{index}")

    def test_gpl_nonfree_and_wrong_abi_are_rejected_before_receipt(self):
        with tempfile.TemporaryDirectory() as temporary:
            prefix = Path(temporary)
            for license_text, config, version in [(b"GPL version 3", b"--enable-shared", 61 << 16), (b"LGPL version 2.1", b"--enable-nonfree", 61 << 16), (b"LGPL version 2.1", b"--enable-gpl", 61 << 16), (b"LGPL version 2.1", b"--enable-shared", 60 << 16)]:
                library = Mock()
                library.avutil_license.return_value = license_text
                library.avutil_configuration.return_value = config
                library.avutil_version.return_value = version
                with patch.object(ctypes, "CDLL", return_value=library):
                    with self.assertRaises(ValueError):
                        build.verify(prefix, {})
                self.assertFalse((prefix / "build-receipt.json").exists())

    def test_different_ffmpeg_release_with_same_abi_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            prefix = Path(temporary)
            library = Mock()
            library.avutil_license.return_value = b"LGPL version 2.1 or later"
            library.avutil_configuration.return_value = b"--disable-gpl --disable-nonfree --enable-shared"
            library.avutil_version.return_value = 61 << 16
            library.av_version_info.return_value = b"9.1.0"
            with patch.object(ctypes, "CDLL", return_value=library):
                with self.assertRaisesRegex(ValueError, "pinned FFmpeg version mismatch"):
                    build.verify(prefix, json.loads(build.MANIFEST.read_text()))
            self.assertFalse((prefix / "build-receipt.json").exists())


if __name__ == "__main__":
    unittest.main()
