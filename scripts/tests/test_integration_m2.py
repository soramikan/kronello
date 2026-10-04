"""Binary-only CPU acceptance for the INTEGRATION-001 driver."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class IntegrationM2(unittest.TestCase):
    def run_demo(self, resolution):
        target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
        profile = "release" if resolution == "4k" else "debug"
        binary_dir = Path(os.environ.get("KRONELLO_INTEGRATION_BINARY_DIR", target / profile))
        with tempfile.TemporaryDirectory(prefix="kronello-integration-001-") as temporary:
            output = Path(temporary) / "demo"
            state = Path(temporary) / "state"
            completed = subprocess.run([
                "python3", str(ROOT / "scripts/demo_integration_m2.py"),
                "--binary-dir", str(binary_dir), "--backend", "cpu-reference",
                "--resolution", resolution, "--output-directory", str(output),
                "--state-root", str(state),
            ], cwd=ROOT, capture_output=True, text=True, timeout=1200)
            report = json.loads((output / "report.json").read_text())
            self.assertEqual(completed.returncode, 0, completed.stderr + json.dumps(report))
            self.assertEqual(report["status"], "verified")
            self.assertEqual(report["backend"], "cpu-reference")
            self.assertEqual(report["state_root"], str(state))
            checks = {check["name"]: check["passed"] for check in report["checks"]}
            for name in ("independence.evaluated_values", "independence.pixels",
                         "protected_intervals.before", "protected_intervals.after",
                         "shadow.5_seconds", "shadow.8_seconds", "job.fixed_snapshot",
                         "job.ffprobe_codec", "overflow.typed_error", "overflow.no_published_output"):
                self.assertTrue(checks[name], name)
            self.assertTrue(all(checks.values()))
            self.assertEqual(report["job"]["completed_frames"], 2)
            print(json.dumps({"resolution": resolution, "checks": len(checks),
                              "job_seconds": (report["job"]["finished_at_ms"] -
                                              report["job"]["submitted_at_ms"]) / 1000}), flush=True)

    def test_small_cpu_reference(self):
        self.run_demo("small")

    def test_4k_cpu_reference(self):
        self.run_demo("4k")


if __name__ == "__main__":
    unittest.main()
