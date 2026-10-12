#!/usr/bin/env python3
"""Measure control gates and physical worker exit in fresh benchmark states."""
import argparse
from contextlib import closing
import json
import os
from pathlib import Path
import signal
import sqlite3
import subprocess
import time

from perf_export_worker import call, digest, independent_oracle, read_status


def process(pid):
    result = subprocess.run(["ps", "-p", str(pid), "-o", "stat=", "-o", "args="],
                            capture_output=True, text=True, timeout=5)
    value = result.stdout.strip()
    return value if value and not value.startswith("Z") else None


def trial(binary, case_path, directory, action, marker):
    case = json.loads(case_path.read_text())
    for source in case.get("sources", []):
        assert digest(source["path"]) == source["sha256"]
    directory.mkdir()
    state = directory / "state"
    env = dict(os.environ, KRONELLO_STATE_ROOT=str(state), KRONELLO_EXPORT_DIAGNOSTICS="1",
               KRONELLO_JOB_HEARTBEAT_MS="100", KRONELLO_JOB_TIMEOUT_MS="3000",
               KRONELLO_MEDIA_TIMING="1", KRONELLO_PROGRESS_INTERVAL_MS="200",
               KRONELLO_RASTER_CACHE_ROOT=str(directory.parent / f"cache-{directory.name}"))
    for key in tuple(env):
        if key.startswith("KRONELLO_TEST_"):
            env.pop(key)
    project = directory / "input.kronello"
    call(binary, "gpu", env, "project.create", project=str(project), document=case["document"])
    request = json.loads(json.dumps(case["submit"]))
    request["render"]["input"]["project"] = str(project)
    destination = directory / case.get("output_name", "movie.mov")
    request["render"]["output_directory"] = str(destination)
    job = call(binary, "gpu", env, "render.submit", **request)["id"]
    deadline = time.monotonic() + 120
    observed = None
    while time.monotonic() < deadline:
        record = read_status(state, job)
        pid = record.get("worker_pid")
        log = state / "jobs" / job / "worker.log"
        if record["status"] not in ("queued", "running"):
            raise RuntimeError(f"job ended before the requested native marker: {record['status']}")
        if record["status"] == "running" and pid:
            if marker == "video" and log.exists() and "video_source: decode=" in log.read_text():
                observed = {"marker": "first video source converted"}
            if marker == "audio":
                sizes = [p.stat().st_size for stage in directory.glob(".kronello-job-*")
                         for p in stage.rglob("*") if p.is_file() and p.name != "owner.json"]
                if sizes and max(sizes) >= 65536:
                    observed = {"marker": "audio spool growing", "bytes": max(sizes)}
            if observed:
                command = process(pid)
                if command and str(binary) in command and job in command:
                    break
        time.sleep(0.01)
    else:
        call(binary, "gpu", env, "job.cancel", job=job)
        raise TimeoutError("native marker was not reached")
    start = time.monotonic()
    if action == "cancel":
        call(binary, "gpu", env, "job.cancel", job=job)
    elif action == "lost-lease":
        # Only this fresh, explicitly identified benchmark record is changed.
        with closing(sqlite3.connect(state / "jobs.sqlite3")) as db, db:
            value = json.loads(db.execute("SELECT record FROM jobs WHERE id=?", (job,)).fetchone()[0])
            assert value["worker_pid"] == pid and value["status"] == "running"
            value["status"] = "interrupted"
            value["worker_pid"] = None
            db.execute("UPDATE jobs SET record=? WHERE id=?", (json.dumps(value), job))
    else:
        # PID and exact --job ownership were verified immediately above.
        os.kill(pid, signal.SIGKILL)
    requested_ms = (time.monotonic() - start) * 1000
    while process(pid):
        if time.monotonic() - start > 120:
            raise TimeoutError("owned worker did not stop")
        time.sleep(0.01)
    exited_ms = (time.monotonic() - start) * 1000
    assert not destination.exists(), "interrupted/canceled output was published"
    deadline = time.monotonic() + 10
    while True:
        terminal = call(binary, "gpu", env, "job.get", job=job)
        if terminal["status"] not in ("queued", "running"):
            break
        if time.monotonic() > deadline:
            raise TimeoutError("dead worker was not recovered")
        time.sleep(0.05)
    assert terminal["status"] == ("canceled" if action == "cancel" else "interrupted")
    resumed = None
    if action != "cancel":
        call(binary, "gpu", env, "job.resume", job=job)
        deadline = time.monotonic() + 120
        while True:
            restored = read_status(state, job)
            if restored["status"] not in ("queued", "running"):
                break
            if time.monotonic() > deadline:
                raise TimeoutError("resumed worker did not finish")
            time.sleep(0.02)
        assert restored["status"] == "succeeded" and restored["attempt"] == 1
        restored = call(binary, "gpu", env, "job.get", job=job)
        assert restored["completed_frames"] == restored["total_frames"]
        resumed = {"attempt": 1, "status": restored["status"],
                   "render_snapshot_hash": restored["result"]["report"]["render_snapshot_hash"],
                   "oracle": independent_oracle(destination, "ffmpeg", "ffprobe")}
    performance = state / "jobs" / job / "performance-attempt-0.json"
    return {"binary_sha256": digest(binary), "case_sha256": digest(case_path),
            "action": action, "observed_work": observed, "signal_api_ms": requested_ms,
            "signal_to_worker_exit_ms": exited_ms, "terminal_status": terminal["status"],
            "control_interval_ms": 50, "heartbeat_interval_ms": 100, "heartbeat_timeout_ms": 3000,
            "worker_diagnostics": json.loads(performance.read_text()) if performance.exists() else None,
            "resumed": resumed}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", type=Path, required=True)
    parser.add_argument("--after", type=Path, required=True)
    parser.add_argument("--audio-case", type=Path, required=True)
    parser.add_argument("--video-case", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--resume", action="store_true")
    args = parser.parse_args()
    args.output = args.output.resolve()
    if args.resume:
        evidence = json.loads((args.output / "evidence.json").read_text())
    else:
        args.output.mkdir(parents=True, exist_ok=False)
        evidence = {"schema_version": 1, "trials": [], "note": "Fresh owned states only; heartbeat policy is explicit. Signal API and physical process exit are separate."}
    scenarios = [("cancel", "audio", args.audio_case), ("cancel", "video", args.video_case),
                 ("lost-lease", "video", args.video_case), ("kill", "video", args.video_case)]
    for action, marker, case in scenarios:
        for mode, binary in [("before", args.before), ("after", args.after)]:
            existing = [item for item in evidence["trials"] if item["action"] == action and item["mode"] == mode
                        and ("audio" if item["observed_work"]["marker"].startswith("audio") else "video") == marker]
            if existing:
                assert existing[0]["binary_sha256"] == digest(binary)
                assert existing[0]["case_sha256"] == digest(case)
                continue
            print(action, marker, mode, flush=True)
            directory = args.output / f"{action}-{marker}-{mode}"
            if directory.exists():
                db_path = directory / "state" / "jobs.sqlite3"
                if db_path.exists():
                    with closing(sqlite3.connect(db_path)) as db:
                        records = [json.loads(row[0]) for row in db.execute("SELECT record FROM jobs")]
                    if any(record["status"] in ("queued", "running") for record in records):
                        raise RuntimeError("previous owned trial is still live; resume its observation first")
                suffix = 1
                while directory.with_name(directory.name + f"-retry{suffix}").exists():
                    suffix += 1
                directory = directory.with_name(directory.name + f"-retry{suffix}")
            result = trial(binary.resolve(), case.resolve(), directory, action, marker)
            evidence["trials"].append({"mode": mode, **result})
            (args.output / "evidence.json").write_text(json.dumps(evidence, indent=2) + "\n")


if __name__ == "__main__":
    main()
