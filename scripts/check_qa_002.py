#!/usr/bin/env python3
"""Run independent GUI/CLI/MCP edit checks and write one auditable evidence report."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--swiftc", required=True)
    parser.add_argument("--disable-plugin-sandbox", action="store_true")
    parser.add_argument("--output", type=Path, default=ROOT / "target/qa-002")
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    # Never reuse a successful report from an earlier run after a failed check.
    for name in ["gui.json", "ime.json", "cli-mcp.json", "report.json"]:
        (output / name).unlink(missing_ok=True)
    env = dict(os.environ, CARGO_BUILD_JOBS="3", KRONELLO_QA_EVIDENCE_DIR=str(output))
    env["KRONELLO_STATE_ROOT"] = str(output / "state")
    commands = [
        ["python3", "scripts/build_ffi.py"],
        ["cargo", "build", "-p", "kronello-mcp", "--locked"],
        ["python3", "scripts/check_gui_swift.py", "--swiftc", args.swiftc, "--run-checks"],
        ["cargo", "test", "-p", "kronello-cli", "--test", "qa_equivalence", "--locked", "--", "--nocapture"],
    ]
    if args.disable_plugin_sandbox:
        commands[2].append("--disable-plugin-sandbox")
    for command in commands:
        subprocess.run(command, cwd=ROOT, env=env, check=True)
    native = json.loads((output / "cli-mcp.json").read_text())
    ime = json.loads((output / "ime.json").read_text())
    if not native["gui_compared"]:
        raise AssertionError("All three independent paths must be compared")
    sources = [
        "tests/qa-002/scenarios.json", "crates/kronello-cli/tests/qa_equivalence.rs",
        "apps/macos/Tests/KronelloAppModelTests/QAChecks.swift",
        "apps/macos/Sources/KronelloDesign/Components/CommittedTextInput.swift",
        "apps/macos/Sources/KronelloDesign/Components/TextField.swift",
        "apps/macos/Sources/KronelloDesign/Components/NumberField.swift",
        "scripts/check_gui_swift.py", "scripts/check_qa_002.py",
    ]
    report = {
        "task": "QA-002", "format": 1, "head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "source_sha256": {p: hashlib.sha256((ROOT / p).read_bytes()).hexdigest() for p in sources},
        "commands": commands, "independent_projects": True,
        "equivalence": {"paths": ["GUI app-model / real FFI session", "CLI process", "MCP stdio process"],
                        "all_snapshots_equal": True, "canonicalization": "sorted object keys; unchanged array order and UTF-8 strings; known finite numbers normalized via f64; no Unicode normalization",
                        "steps": [{k: step[k] for k in ["name", "action", "revision", "event_count", "canonical_document_sha256"]} for step in native["cli"]["steps"]]},
        "ime": ime["checks"],
        "pending_host": ["SwiftPM XCTest runner", "macOS app build/launch", "Kotoeri typing, conversion, candidate selection, Escape, Return, blur, Dark/Light"],
    }
    (output / "report.json").write_text(json.dumps(report, ensure_ascii=False, sort_keys=True, indent=2) + "\n")
    print(output / "report.json")


if __name__ == "__main__":
    main()
