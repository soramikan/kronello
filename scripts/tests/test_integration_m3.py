"""Opt-in stage-2 binary parity and explicit portrait re-layout expectations."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from demo_integration_m3 import canonical_bytes


class CanonicalEvidence(unittest.TestCase):
    def test_numeric_spelling_and_order_only(self):
        self.assertEqual(canonical_bytes({"b": [1.0, 0.125], "a": 2}),
                         canonical_bytes({"a": 2.0, "b": [1, 0.125]}))
        self.assertNotEqual(canonical_bytes([1, 2]), canonical_bytes([2, 1]))
        self.assertNotEqual(canonical_bytes(0.125), canonical_bytes(0.126))


@unittest.skipUnless(os.environ.get("KRONELLO_INTEGRATION_TESTS") == "1",
                     "set KRONELLO_INTEGRATION_TESTS=1 after building CLI/MCP and fetching fonts")
class IntegrationM3(unittest.TestCase):
    def test_small_cpu_reference_parity_and_portrait(self):
        binary = os.environ.get("KRONELLO_INTEGRATION_BINARY_DIR",
                                str(Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "debug"))
        with tempfile.TemporaryDirectory(prefix="kronello-integration-002-") as folder:
            output = Path(folder) / "demo"
            completed = subprocess.run([sys.executable, str(ROOT / "scripts/demo_integration_m3.py"),
                "--backend", "cpu-reference", "--resolution", "small", "--binary-dir", binary,
                "--output-directory", str(output)], cwd=ROOT, capture_output=True, text=True, timeout=1200)
            report = json.loads((output / "report.json").read_text())
            self.assertEqual(completed.returncode, 0, completed.stderr + json.dumps(report))
            self.assertEqual(report["status"], "verified")
            checks = {c["name"]: c["passed"] for c in report["checks"]}
            self.assertTrue(all(checks.values()))
            for name in ("variants.single_definition", "portrait.wrapped_equals_explicit_lines",
                         "portrait.preview.no_overflow", "overflow.typed_error"):
                self.assertTrue(checks[name])
            manifest = json.loads((output / "gui-evidence.json").read_text())
            self.assertEqual(len(manifest["cases"]), 8)
            for case in manifest["cases"]:
                self.assertEqual((output / case["cli"]).read_bytes(), (output / case["mcp"]).read_bytes())
                self.assertTrue(checks[case["name"] + ".layout_size"])
            print(f"PASS stage-2: {len(report['checks'])} checks, 8 canonical CLI/MCP pairs")


if __name__ == "__main__":
    unittest.main()
