"""Negative-path checks for pinned downloads and fixture validation."""
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import fetch_fixtures
import fixtures


class FetchTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.output = Path(self.directory.name)
        self.data = b"verified fixture"
        self.entry = {"id": "test", "path": "external/test.bin", "bytes": len(self.data),
                      "sha256": hashlib.sha256(self.data).hexdigest(),
                      "url": "https://example.invalid/pinned/test.bin"}

    def opener(self, data):
        response = io.BytesIO(data)
        response.geturl = lambda: self.entry["url"]
        mock = unittest.mock.Mock()
        mock.open.return_value = response
        return mock

    def test_verified_cache_requires_no_network(self):
        (self.output / "test.bin").write_bytes(self.data)
        with patch("urllib.request.build_opener", side_effect=AssertionError("unexpected network")):
            self.assertEqual(fetch_fixtures.fetch(self.entry, self.output), self.output / "test.bin")

    def test_missing_and_corrupt_offline_cache_fail(self):
        with self.assertRaises(ValueError):
            fetch_fixtures.fetch(self.entry, self.output, offline=True)
        (self.output / "test.bin").write_bytes(b"bad")
        with self.assertRaises(ValueError):
            fetch_fixtures.fetch(self.entry, self.output, offline=True)

    def test_bad_hash_or_size_never_replaces_destination(self):
        destination = self.output / "test.bin"
        destination.write_bytes(b"old")
        for bad in [b"x" * len(self.data), b"short", self.data + b"oversize"]:
            with patch("urllib.request.build_opener", return_value=self.opener(bad)):
                with self.assertRaises(ValueError):
                    fetch_fixtures.fetch(self.entry, self.output)
            self.assertEqual(destination.read_bytes(), b"old")
            self.assertEqual(list(self.output.iterdir()), [destination])

    def test_network_failure_is_not_skipped(self):
        with patch("urllib.request.build_opener", side_effect=OSError("unavailable")):
            with self.assertRaises(OSError):
                fetch_fixtures.fetch(self.entry, self.output)
        self.assertEqual(list(self.output.iterdir()), [])

    def test_successful_download_publishes_verified_bytes(self):
        with patch("urllib.request.build_opener", return_value=self.opener(self.data)):
            path = fetch_fixtures.fetch(self.entry, self.output)
        self.assertEqual(path.read_bytes(), self.data)
        self.assertEqual(list(self.output.iterdir()), [path])

    def test_redirect_downgrade_is_rejected(self):
        with self.assertRaises(ValueError):
            fetch_fixtures.HTTPSOnly().redirect_request(None, None, 302, "", {}, "http://example.invalid")


class ValidationTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        shutil.copytree(fixtures.ROOT / "tests", self.root / "tests")
        self.manifest = self.root / "tests/fixtures/manifest.json"
        # Avoid depending on a previously downloaded font for negative-path unit tests.
        data = json.loads(self.manifest.read_text())
        data["fixtures"] = [entry for entry in data["fixtures"] if entry["storage"] != "external"]
        self.manifest.write_bytes(fixtures.encoded(data))
        scenes = self.root / "tests/golden/scenes.json"
        s = json.loads(scenes.read_text())
        for scene in s["scenes"]:
            scene["fixtures"] = [id for id in scene["fixtures"] if id != "noto-sans-cjk-jp"]
        scenes.write_bytes(fixtures.encoded(s))
        for name, replacement in [("ROOT", self.root), ("MANIFEST", self.manifest)]:
            patcher = patch.object(fixtures, name, replacement)
            patcher.start(); self.addCleanup(patcher.stop)

    def test_corrupt_and_missing_bundled_data_fail(self):
        path = self.root / "tests/fixtures/data/alpha.pam"
        path.write_bytes(b"corrupt")
        with self.assertRaises(ValueError):
            fixtures.check(self.root, None)
        path.unlink()
        with self.assertRaises(OSError):
            fixtures.check(self.root, None)

    def test_duplicate_manifest_ids_fail(self):
        data = json.loads(self.manifest.read_text())
        data["fixtures"].append(data["fixtures"][0])
        self.manifest.write_bytes(fixtures.encoded(data))
        with self.assertRaises(ValueError): fixtures.load_manifest()

    def test_unknown_scene_fixture_and_zero_scenes_fail(self):
        scenes = self.root / "tests/golden/scenes.json"
        data = json.loads(scenes.read_text())
        data["scenes"][0]["fixtures"].append("missing-fixture")
        scenes.write_bytes(fixtures.encoded(data))
        with self.assertRaises(ValueError): fixtures.check(self.root, None)
        data["scenes"] = []
        scenes.write_bytes(fixtures.encoded(data))
        with self.assertRaises(ValueError): fixtures.check(self.root, None)

    def test_external_directory_defaults_and_environment_override(self):
        data = json.loads(self.manifest.read_text())
        entry = {"id": "small-external", "storage": "external", "path": "external/test.bin",
                 "bytes": 3, "sha256": hashlib.sha256(b"abc").hexdigest(),
                 "url": "https://example.invalid/pinned/test.bin", "revision": "pinned",
                 "source": "unit test", "purpose": "directory selection", "license": "OFL-1.1"}
        data["fixtures"].append(entry)
        self.manifest.write_bytes(fixtures.encoded(data))
        ledger = self.root / "tests/fixtures/LEDGER.md"
        ledger.write_text(ledger.read_text() + "\n`small-external`\n")
        default = self.root / "target/fixtures/external"
        override = self.root / "custom-external"
        for path in [default, override]:
            path.mkdir(parents=True)
            (path / "test.bin").write_bytes(b"abc")
        for external in [None, override]:
            with patch.dict(os.environ), patch.object(sys, "argv", ["fetch_fixtures.py", "--offline"]):
                os.environ.pop(fixtures.EXTERNAL_FIXTURE_DIR_ENV, None)
                if external is not None:
                    (default / "test.bin").unlink()
                    os.environ[fixtures.EXTERNAL_FIXTURE_DIR_ENV] = str(external)
                self.assertEqual(fixtures.external_fixture_dir(), external or default)
                self.assertEqual(fetch_fixtures.main(), 0)
                with patch.object(sys, "argv", ["fixtures.py", "check"]):
                    self.assertEqual(fixtures.main(), 0)

    def test_timing_decimal_string_rationals_and_exact_expectations(self):
        value = json.loads(fixtures.canonical_data()["timing.json"])
        self.assertEqual(value["cfr"][0]["frame_times"][1], {"num": "1", "den": "24"})
        fixtures.validate_timing(value)
        value["cfr"][0]["frame_times"][1] = {"num": "1", "den": "25"}
        with self.assertRaises(ValueError): fixtures.validate_timing(value)

    def test_invalid_rational_encodings_fail(self):
        for value in [[1, 24], {"num": 1, "den": "24"}, {"num": "01", "den": "24"},
                      {"num": "+1", "den": "24"}, {"num": "-0", "den": "1"},
                      {"num": "1", "den": "0"}, {"num": "1", "den": "-24"},
                      {"num": "2", "den": "48"}, {"num": "0", "den": "24"},
                      {"num": "1", "den": "24", "extra": "0"}]:
            with self.subTest(value=value), self.assertRaises(ValueError): fixtures.parse_rational(value)

    def test_scene_rational_fields_require_decimal_string_objects(self):
        path = self.root / "tests/golden/scenes.json"
        original = json.loads(path.read_text())
        for index, field, item in [(0, "descriptor", "time"), (0, "input", "fraction"),
                                   (1, "input", "interval"), (1, "input", "times")]:
            data = json.loads(json.dumps(original))
            if item in {"interval", "times"}:
                data["scenes"][index][field][item][0] = [0, 1]
            else:
                data["scenes"][index][field][item] = [0, 1]
            path.write_bytes(fixtures.encoded(data))
            with self.assertRaises(ValueError): fixtures.check(self.root, None)


if __name__ == "__main__":
    unittest.main()
