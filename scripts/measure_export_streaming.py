#!/usr/bin/env python3
"""Run one export acceptance binary and record kernel RSS/I/O counters."""
import argparse
import json
from pathlib import Path
import resource
import os
import signal
import subprocess
import sys
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--test", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--file-limit-bytes", type=int)
    args = parser.parse_args()
    command = [str(args.binary.resolve()), args.test, "--ignored", "--exact", "--nocapture"]
    before = resource.getrusage(resource.RUSAGE_CHILDREN)
    start = time.monotonic()
    environment = os.environ.copy()
    prepare = None
    if args.file_limit_bytes is not None:
        if args.file_limit_bytes <= 0:
            parser.error("--file-limit-bytes must be positive")
        environment["KRONELLO_EXPORT_TEST_FILE_LIMIT"] = str(args.file_limit_bytes)

        def prepare():
            resource.setrlimit(resource.RLIMIT_FSIZE, (args.file_limit_bytes, args.file_limit_bytes))
            signal.signal(signal.SIGXFSZ, signal.SIG_IGN)

    result = subprocess.run(command, check=False, env=environment, preexec_fn=prepare)
    after = resource.getrusage(resource.RUSAGE_CHILDREN)
    record = {
        "command": command,
        "platform": sys.platform,
        "exit_code": result.returncode,
        "file_limit_bytes": args.file_limit_bytes,
        "elapsed_seconds": time.monotonic() - start,
        "peak_rss_bytes": int(after.ru_maxrss * (1 if sys.platform == "darwin" else 1024)),
        "user_seconds": after.ru_utime - before.ru_utime,
        "system_seconds": after.ru_stime - before.ru_stime,
        "input_blocks": after.ru_inblock - before.ru_inblock,
        "output_blocks": after.ru_oublock - before.ru_oublock,
        "io_note": "Kernel block counters; cached reads/writes may be zero. Not logical byte counts.",
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(record, indent=2) + "\n")
    print(json.dumps(record, indent=2))
    return result.returncode


if __name__ == "__main__":
    sys.exit(main())
