#!/usr/bin/env python3
"""Exercise receipt reconciliation in an explicitly owned benchmark state."""
import argparse
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import time

from perf_export_worker import call, canonical, digest, rss


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    evidence = json.loads(args.evidence.read_text())
    trial = evidence["trials"][-1]
    state = args.evidence.resolve().parent / f"{trial['pair']}-{trial['mode']}" / "state"
    if digest(args.binary) != evidence["binary_sha256"][trial["mode"]]:
        raise RuntimeError("binary differs from measured worker")
    env = dict(os.environ, KRONELLO_STATE_ROOT=str(state))
    original = call(args.binary.resolve(), evidence["backend"], env, "job.get", job=trial["job"])
    if original["status"] != "succeeded" or rss(original["worker_pid"]) is not None:
        raise RuntimeError("benchmark worker has not exited successfully")
    # Simulate a crash between rename and the success record. Only this
    # benchmark's already exited job is modified; artifacts stay untouched.
    with closing(sqlite3.connect(state / "jobs.sqlite3")) as db:
        raw = db.execute("SELECT record FROM jobs WHERE id=?", (trial["job"],)).fetchone()[0]
        record = json.loads(raw)
        record["status"] = "interrupted"
        with db:
            db.execute("UPDATE jobs SET record=? WHERE id=?", (json.dumps(record), trial["job"]))
    start = time.monotonic()
    try:
        resumed = call(args.binary.resolve(), evidence["backend"], env, "job.resume", job=trial["job"])
        restored = call(args.binary.resolve(), evidence["backend"], env, "job.get", job=trial["job"])
        assert resumed["status"] == restored["status"] == "succeeded"
        assert restored["attempt"] == original["attempt"]
        assert restored["result"] == original["result"]
        assert restored["completed_frames"] == original["total_frames"]
    except BaseException:
        with closing(sqlite3.connect(state / "jobs.sqlite3")) as db, db:
            db.execute("UPDATE jobs SET record=? WHERE id=?", (raw, trial["job"]))
        raise
    result = {"evidence_sha256": digest(args.evidence), "binary_sha256": digest(args.binary),
              "job": trial["job"], "frame_count": len(restored["result"]["report"]["frames"]),
              "result_canonical_sha256": hashlib.sha256(canonical(original["result"])).hexdigest(),
              "recovered_result_identical": True, "no_new_attempt": True,
              "resume_and_hydrate_ms": (time.monotonic() - start) * 1000}
    args.output.write_text(json.dumps(result, indent=2) + "\n")


if __name__ == "__main__":
    main()
