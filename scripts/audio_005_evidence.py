#!/usr/bin/env python3
"""Record AUDIO-005 AAC-LC / Opus delivery audio acceptance, fail closed."""
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]


def main():
    directory = ROOT / "target/m5-acceptance/audio-005"
    directory.mkdir(parents=True, exist_ok=True)
    report = {"schema_version": 1, "task": "AUDIO-005", "platform": platform.platform(),
              "machine": platform.machine(), "python": sys.version,
              "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
              "runtime": os.environ.get("KRONELLO_FFMPEG_LIB_DIR"), "commands": []}
    commands = [
        ["cargo", "test", "-p", "kronello-media", "--test", "audio5", "--locked", "--", "--nocapture"],
        ["cargo", "test", "-p", "kronello-service", "--test", "media", "--locked", "--", "--nocapture"],
        ["cargo", "test", "-p", "kronello-service", "--locked", "export_profiles", "--", "--nocapture"],
        ["cargo", "test", "-p", "kronello-media", "--test", "audio", "--locked", "--", "--nocapture"],
        ["cargo", "test", "-p", "kronello-media", "--test", "profiles", "--locked", "--", "--nocapture"],
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
            # A mistyped test filter must never count as process evidence. A
            # package-scoped filter prints "0 passed" for every non-matching
            # test binary, so only flag runs where no test passed at all.
            ran_any = any(int(n) > 0 for n in re.findall(r"(\d+) passed", result.stdout))
            empty_test = command[1:2] == ["test"] and not ran_any
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
