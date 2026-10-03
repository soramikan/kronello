#!/usr/bin/env python3
"""CPU regression checks for adoption gates; synthetic pixels are not GPU evidence."""
import contextlib
import hashlib
import io
import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch

import golden_adopt as golden


class AdoptionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.baseline = self.root / golden.BASELINE
        self.baseline.mkdir(parents=True)
        self.catalog = {"scene_ids": ["test-scene"]}
        self.write(self.baseline / "scenes.json", self.catalog)
        (self.baseline / "README.md").write_text("synthetic baseline")
        self.candidate = self.root / "target/golden/run.test/candidate"
        self.candidate.mkdir(parents=True)
        self.scene = {"id": "test-scene", "size": [1, 1], "sample_id": "frame-0",
                      "samples_per_frame": 1, "time": {"num": "0", "den": "1"},
                      "working_space": "LinearRec709", "alpha": "premultiplied",
                      "output_transform": None}
        self.manifest = {"scenes": [self.scene], "catalog": self.catalog, "comparison_version": 1,
                         "rgb_absolute": 2 ** -10, "rgb_relative": 2 ** -10, "alpha_absolute": 2 ** -10}
        self.environment = {"target": "aarch64-apple-darwin", "adapter": {"backend": "Metal", "name": "M1"},
                            "hardware": {"model": "arbitrary"}, "os": "arbitrary"}
        self.provenance = {"revision": "test-head", "status": ""}
        self.write(self.candidate / "manifest.json", self.manifest)
        self.write(self.candidate / "environment.json", self.environment)
        self.write(self.candidate / "provenance.json", self.provenance)
        frame = self.candidate / "test-scene"
        frame.mkdir()
        (frame / "frame-0.rgba16f").write_bytes(struct.pack("<4e", 0.5, 0, 0, 1))
        (frame / "frame-0.png").write_bytes(b"synthetic display artifact")
        self.report = {"test_count": 1, "scene_count": 1, "frame_count": 1,
                       "status": "candidate-only; no baseline comparison", "candidate_may_be_adopted": True,
                       "provenance": self.provenance,
                       "scenes": [{"scene": "test-scene", "result": "candidate; CPU oracle validated"}]}
        self.write(self.candidate.parent / "report.json", self.report)
        self.seal()
        self.git = patch.object(golden, "git", side_effect=lambda root, *args: "" if args[0] == "status" else "test-head")
        self.git_mock = self.git.start()
        self.addCleanup(self.git.stop)

    @staticmethod
    def write(path, value):
        path.write_text(json.dumps(value))

    def seal(self):
        self.adoption = {"schema_version": 1, "scene_settings": [{key: self.scene[key] for key in
                         ("id", "size", "sample_id", "samples_per_frame", "time", "working_space", "alpha")}],
                         "environment": self.environment, "provenance": self.provenance}
        for key in ("comparison_version", "rgb_absolute", "rgb_relative", "alpha_absolute"):
            self.adoption[key] = self.manifest[key]
        self.adoption["files"] = {p.relative_to(self.candidate).as_posix():
                                   {"sha256": hashlib.sha256(p.read_bytes()).hexdigest(), "bytes": p.stat().st_size}
                                   for p in self.candidate.rglob("*") if p.is_file() and p.name != "adoption.json"}
        self.write(self.candidate / "adoption.json", self.adoption)

    def test_adopt_preserves_catalog_and_publishes_hash_manifest(self):
        with contextlib.redirect_stdout(io.StringIO()) as log:
            golden.adopt(self.root, self.candidate)
        self.assertIn('"status": "adopted"', log.getvalue())
        self.assertEqual(golden.load(self.baseline / "adoption.json"), self.adoption)
        self.assertEqual(golden.load(self.baseline / "scenes.json"), self.catalog)
        self.assertEqual((self.baseline / "README.md").read_text(), "synthetic baseline")
        self.assertEqual((self.baseline / "test-scene/frame-0.rgba16f").read_bytes(), struct.pack("<4e", 0.5, 0, 0, 1))

    def test_publish_failure_restores_previous_baseline(self):
        original = Path.rename

        def fail_publish(path, target):
            if path.name == "baseline":
                raise OSError("injected publish failure")
            return original(path, target)

        with patch.object(Path, "rename", fail_publish), contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaisesRegex(OSError, "injected publish failure"):
                golden.adopt(self.root, self.candidate)
        self.assertEqual((self.baseline / "README.md").read_text(), "synthetic baseline")
        self.assertEqual(golden.load(self.baseline / "scenes.json"), self.catalog)
        self.assertFalse((self.baseline / "manifest.json").exists())

    def test_dirty_tree_is_rejected_before_mutation(self):
        self.git_mock.side_effect = lambda root, *args: "?? untracked-input"
        with self.assertRaisesRegex(ValueError, "working tree is dirty"):
            golden.adopt(self.root, self.candidate)
        self.assertFalse((self.baseline / "manifest.json").exists())

    def test_dirty_candidate_and_revision_mismatch(self):
        for key, value, message in (("status", " M shader", "dirty tree"), ("revision", "old-head", "differs from HEAD")):
            with self.subTest(key=key):
                self.write(self.candidate / "provenance.json", {**self.provenance, key: value})
                with self.assertRaisesRegex(ValueError, message):
                    golden.validate(self.root, self.candidate)
        self.write(self.candidate / "provenance.json", self.provenance)

    def test_missing_or_corrupt_baseline_artifact(self):
        path = self.candidate / "test-scene/frame-0.rgba16f"
        path.write_bytes(b"corrupt")
        with self.assertRaisesRegex(ValueError, "artifact hash or size differs"):
            golden.validate(self.root, self.candidate)
        path.unlink()
        with self.assertRaisesRegex(ValueError, "missing or unexpected candidate files"):
            golden.validate(self.root, self.candidate)

    def test_nonfinite_and_invalid_alpha_even_with_matching_hashes(self):
        for pixel in ((float("nan"), 0, 0, 1), (float("inf"), 0, 0, 1), (0, 0, 0, 1.5), (1, 0, 0, 0)):
            with self.subTest(pixel=pixel):
                (self.candidate / "test-scene/frame-0.rgba16f").write_bytes(struct.pack("<4e", *pixel))
                self.seal()
                with self.assertRaisesRegex(ValueError, "non-finite|premultiplied"):
                    golden.validate(self.root, self.candidate)

    def test_wrong_backend_and_non_native_target(self):
        for target, backend in (("x86_64-apple-darwin", "Metal"), ("aarch64-apple-darwin", "Vulkan")):
            with self.subTest(target=target, backend=backend):
                self.write(self.candidate / "environment.json", {"target": target, "adapter": {"backend": backend}})
                with self.assertRaisesRegex(ValueError, "Apple Silicon"):
                    golden.validate(self.root, self.candidate)

    def test_zero_scenes_and_unsuccessful_run(self):
        self.write(self.candidate / "manifest.json", {**self.manifest, "scenes": []})
        with self.assertRaisesRegex(ValueError, "zero or mismatched scenes"):
            golden.validate(self.root, self.candidate)
        self.write(self.candidate / "manifest.json", self.manifest)
        self.write(self.candidate.parent / "report.json", {**self.report, "frame_count": 0})
        with self.assertRaisesRegex(ValueError, "did not finish"):
            golden.validate(self.root, self.candidate)

    def test_unsafe_paths_and_size_limits(self):
        self.adoption["files"]["../escape"] = {"bytes": 0, "sha256": ""}
        self.write(self.candidate / "adoption.json", self.adoption)
        with self.assertRaisesRegex(ValueError, "unexpected artifacts"):
            golden.validate(self.root, self.candidate)
        self.seal()
        with patch.object(golden, "PER_FILE", 1), self.assertRaisesRegex(ValueError, "256 KiB"):
            golden.validate(self.root, self.candidate)
        with patch.object(golden, "TOTAL", 1), self.assertRaisesRegex(ValueError, "1 MiB"):
            golden.validate(self.root, self.candidate)

    def test_model_os_adapter_name_are_metadata_only(self):
        self.environment.update({"os": "future OS", "hardware": {"model": "future model"}})
        self.environment["adapter"]["name"] = "future adapter"
        self.write(self.candidate / "environment.json", self.environment)
        self.seal()
        golden.validate(self.root, self.candidate)


if __name__ == "__main__":
    unittest.main()
