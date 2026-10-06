#!/usr/bin/env python3
"""Record full media and actual kronello worker process acceptance, fail closed."""
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]


def main():
    directory = ROOT / "target/media-003-evidence"
    directory.mkdir(parents=True, exist_ok=True)
    report = {"schema_version": 1, "platform": platform.platform(),
              "machine": platform.machine(), "python": sys.version,
              "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
              "runtime": os.environ.get("KRONELLO_FFMPEG_LIB_DIR"), "commands": []}
    commands = [
        ["rustc", "--version", "--verbose"],
        ["cargo", "run", "-p", "kronello-media", "--example", "capabilities", "--locked", "--", "--verify-distribution"],
        ["cargo", "run", "-p", "kronello-media", "--example", "release_roundtrip", "--locked", "--", str(directory / f"roundtrip-{time.time_ns()}")],
        ["cargo", "test", "-p", "kronello-cli", "--test", "jobs", "--locked", "cli_exit_detaches_worker_and_preserves_project_bytes_and_mtime", "--", "--exact", "--nocapture"],
        ["cargo", "test", "-p", "kronello-mcp", "--test", "stdio", "--locked", "submitted_job_survives_mcp_eof_and_is_queryable_on_new_connection", "--", "--exact", "--nocapture"],
        ["cargo", "test", "-p", "kronello-mcp", "--test", "stdio", "--locked", "versions_negotiate_and_registry_schemas_are_self_contained", "--", "--exact", "--nocapture"],
    ]
    failed = True
    try:
        failed = False
        for index, command in enumerate(commands):
            print("+", " ".join(command), flush=True)
            started = time.monotonic()
            result = subprocess.run(command, cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            log = directory / f"{index:02}.log"
            log.write_text(result.stdout, encoding="utf-8")
            print(result.stdout, flush=True)
            # A mistyped exact test filter must never count as process evidence.
            empty_test = command[1:2] == ["test"] and "1 passed" not in result.stdout
            exit_code = result.returncode or int(empty_test)
            report["commands"].append({"command": command, "exit": exit_code,
                                       "seconds": time.monotonic() - started, "log": log.name})
            failed |= exit_code != 0
            if failed:
                break
    except BaseException:
        failed = True
        raise
    finally:
        report["status"] = "failed" if failed else "passed"
        (directory / "evidence.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    return int(failed)


if __name__ == "__main__":
    sys.exit(main())
