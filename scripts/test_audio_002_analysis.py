#!/usr/bin/env python3
"""Synthetic trace validation only; does not run or verify realtime playback."""
import copy
import json
import tempfile
import unittest
from pathlib import Path
from analyze_audio_002 import analyze

class EvidenceChecks(unittest.TestCase):
    def trace(self):
        frames = []
        for sample in [0, 31 * 48000]:
            expected = sample * 24000 // (48000 * 1001)
            frames.append({"record": "presentation", "master": "audio_device", "epoch": "1",
                "fps_num": "24000", "fps_den": "1001", "audio_sample": str(sample),
                "callback_sample": str(sample), "frame": str(expected + 2), "expected_frame": str(expected),
                "offset_frames": "2", "underruns": "1", "missing_frames": "128"})
        events = [{"record": "event", "name": "stop", "sample": "1000"},
                  {"record": "event", "name": "resume", "sample": "1000"},
                  {"record": "event", "name": "finish", "sample": "1488000", "underruns": "3", "missing_frames": "384"}]
        events += [{"record": "event", "name": "seek", "frame": str(f), "sample": str(f * 1001 * 48000 // 24000)} for f in [137, 777]]
        return frames + events
    def report(self, records):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "synthetic.jsonl"
            path.write_text("".join(json.dumps(r) + "\n" for r in records))
            return analyze(path)
    def test_ntsc_offset_and_final_underruns(self):
        report = self.report(self.trace())
        self.assertEqual(report["observed_device_seconds"], 31)
        self.assertEqual(report["max_av_offset_frames"], 2)
        self.assertAlmostEqual(report["max_av_offset_ms"], 2 * 1001 * 1000 / 24000)
        self.assertEqual(report["underruns"], 3)
        self.assertEqual(report["missing_frames"], 384)
        self.assertEqual(report["physical_scanout_or_loopback"], "not_measured")
    def test_rejects_fallback_short_run_bad_grid_seek_and_resume(self):
        for row, key, value in [(0, "master", "host_clock"), (1, "callback_sample", "48000"),
                                (1, "expected_frame", "9"), (3, "sample", "1001"), (5, "sample", "2")]:
            records = copy.deepcopy(self.trace()); records[row][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                self.report(records)
    def test_offset_includes_subframe_phase_even_when_frame_grid_matches(self):
        records = self.trace()
        for record in records[:2]:
            record["frame"] = record["expected_frame"]; record["offset_frames"] = "0"
        report = self.report(records)
        self.assertEqual(report["max_av_offset_frames"], 0)
        self.assertEqual(report["max_frame_grid_offset_ms"], 0)
        self.assertAlmostEqual(report["max_av_offset_ms"], abs(743 * 1001 * 1000 / 24000 - 31000))

if __name__ == "__main__":
    unittest.main()
