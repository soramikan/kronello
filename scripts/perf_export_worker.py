#!/usr/bin/env python3
"""Measure frozen cases through render.submit and real detached workers.

Case preparation and independent decoding are outside the timed interval.
Only the explicit fresh state roots created here are inspected or canceled.
"""
import argparse
from contextlib import closing
from fractions import Fraction
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import sqlite3
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]


def digest(path):
    with Path(path).open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"),
                      ensure_ascii=False).encode()


def call(binary, backend, env, operation, **payload):
    result = subprocess.run([str(binary), "--backend", backend],
                            input=json.dumps({"operation": operation, **payload}),
                            text=True, capture_output=True, env=env, timeout=600)
    response = json.loads(result.stdout)
    if result.returncode or response.get("status") != "success":
        raise RuntimeError(f"{operation}: {response}; stderr: {result.stderr}")
    return response["result"]["value"]


def read_status(state, job):
    with closing(sqlite3.connect(f"{(state / 'jobs.sqlite3').as_uri()}?mode=ro",
                                uri=True, timeout=0.1)) as db:
        with closing(db.execute("SELECT record FROM jobs WHERE id=?", (job,))) as rows:
            return json.loads(rows.fetchone()[0])


def rss(pid):
    if os.name != "posix":
        return None
    result = subprocess.run(["ps", "-p", str(pid), "-o", "rss="],
                            capture_output=True, text=True, timeout=5)
    return int(result.stdout.strip()) * 1024 if result.stdout.strip() else None


def independent_oracle(path, ffmpeg, ffprobe):
    result = subprocess.run([ffprobe, "-v", "error", "-show_streams",
                             "-show_packets", "-show_format", "-of", "json", str(path)],
                            capture_output=True, text=True, check=True, timeout=600)
    probe = json.loads(result.stdout)
    streams = []
    for stream in probe["streams"]:
        index = stream["index"]
        video = stream["codec_type"] == "video"
        pixel_format = stream.get("pix_fmt")
        command = [ffmpeg, "-v", "error", "-i", str(path), "-map", f"0:{index}"]
        command += (["-fps_mode", "passthrough", "-pix_fmt", pixel_format,
                     "-f", "rawvideo"] if video else
                    ["-c:a", "pcm_f32le", "-f", "f32le"])
        command += ["-"]
        # Stream the oracle bytes rather than retaining a decoded movie.
        with subprocess.Popen(command, stdout=subprocess.PIPE,
                              stderr=subprocess.DEVNULL) as decoder:
            sha = hashlib.sha256()
            count = 0
            while block := decoder.stdout.read(1024 * 1024):
                count += len(block)
                sha.update(block)
            if decoder.wait(timeout=600):
                raise RuntimeError(f"independent decode failed: {path}")
        time_base = Fraction(stream["time_base"])
        packets = [{key: str(Fraction(packet[key]) * time_base)
                    for key in ("pts", "dts", "duration") if key in packet}
                   for packet in probe["packets"] if packet["stream_index"] == index]
        streams.append({"codec_type": stream["codec_type"],
                        "codec": stream["codec_name"], "pixel_format": pixel_format,
                        "sample_rate": stream.get("sample_rate"),
                        "channels": stream.get("channels"),
                        "channel_layout": stream.get("channel_layout"),
                        "decoded_sha256": sha.hexdigest(), "decoded_bytes": count,
                        "packets": packets})
    return {"encoded_sha256": digest(path), "encoded_bytes": path.stat().st_size,
            "streams": streams, "chapters": probe.get("chapters", []),
            "snapshot_tags": {k: v for k, v in probe["format"].get("tags", {}).items()
                              if "snapshot" in k}}


def run_trial(binary, case, project, directory, backend, timeout, ffmpeg, ffprobe, diagnostics=True):
    for source in case.get("sources", []):
        if digest(source["path"]) != source["sha256"]:
            raise RuntimeError(f"fixture hash mismatch: {source['path']}")
    directory.mkdir()
    state = directory / "state"
    state.mkdir()
    env = dict(os.environ, KRONELLO_STATE_ROOT=str(state),
               KRONELLO_EXPORT_DIAGNOSTICS="1" if diagnostics else "0",
               KRONELLO_RASTER_CACHE_ROOT=str(directory.parent.parent /
                                              f"cache-{directory.parent.name}-{directory.name}"))
    for key in tuple(env):
        if key.startswith("KRONELLO_TEST_") or key.startswith("KRONELLO_JOB_"):
            env.pop(key)
    request = json.loads(json.dumps(case["submit"]))
    request["render"]["input"]["project"] = str(project)
    paths = [directory / case.get("output_name", "movie.mov")]
    request["render"]["output_directory"] = str(paths[0])
    for index, leg in enumerate(request.get("outputs", [])):
        path = directory / case["additional_output_names"][index]
        leg["destination"] = str(path)
        paths.append(path)
    start = time.monotonic()
    submitted = call(binary, backend, env, "render.submit", **request)
    job = submitted["id"]
    pid = submitted["worker_pid"]
    max_rss = 0
    observed = []
    rss_windows = {}
    next_rss = start
    try:
        while True:
            record = read_status(state, job)
            pid = record.get("worker_pid") or pid
            now = time.monotonic()
            if not observed or observed[-1]["completed"] != record["completed_frames"]:
                observed.append({"elapsed_ms": (now - start) * 1000,
                                 "completed": record["completed_frames"]})
            if record["status"] not in ("queued", "running"):
                break
            if now - start > timeout:
                raise TimeoutError(f"worker deadline exceeded: {job}")
            if now >= next_rss:
                value = rss(pid) if pid else None
                if value is not None:
                    max_rss = max(max_rss, value)
                    # Fixed progress buckets keep long-run observation bounded.
                    bucket = min(32, record["completed_frames"] * 32 //
                                 max(1, record["total_frames"]))
                    point = {"elapsed_ms": (now - start) * 1000, "bytes": value,
                             "completed": record["completed_frames"]}
                    window = rss_windows.setdefault(bucket, {
                        "first": point, "last": point, "min_bytes": value,
                        "max_bytes": value, "samples": 0})
                    window.update(last=point, min_bytes=min(window["min_bytes"], value),
                                  max_bytes=max(window["max_bytes"], value),
                                  samples=window["samples"] + 1)
                next_rss = now + 0.1
            time.sleep(0.02)
        terminal = call(binary, backend, env, "job.get", job=job)
        succeeded_ms = (time.monotonic() - start) * 1000
        if terminal["status"] != "succeeded":
            raise RuntimeError(f"worker failed: {terminal}")
        report = terminal["result"]["report"]
        assert terminal["completed_frames"] == terminal["total_frames"]
        assert terminal["total_frames"] == case["expected_frames"]
        assert len(report["frames"]) == case.get("expected_rendered_frames", case["expected_frames"])
        worker = None
        if diagnostics:
            performance_path = state / "jobs" / job / "performance-attempt-0.json"
            # The success commit precedes diagnostic file emission and process exit.
            deadline = time.monotonic() + 30
            while True:
                try:
                    worker = json.loads(performance_path.read_text())
                    break
                except (FileNotFoundError, json.JSONDecodeError):
                    if time.monotonic() > deadline:
                        raise RuntimeError(f"worker evidence missing: {performance_path}")
                    time.sleep(0.02)
        return {"succeeded_ms": succeeded_ms, "submit_to_worker_evidence_ms":
                (time.monotonic() - start) * 1000,
                "worker_pid": pid, "job": job,
                "worker": worker, "report": report, "progress": observed,
                "sampled_worker_peak_rss_bytes": max_rss or None,
                "sampled_worker_rss_progress_windows": rss_windows,
                "oracles": [independent_oracle(p, ffmpeg, ffprobe) for p in paths]}
    except BaseException:
        call(binary, backend, env, "job.cancel", job=job)
        raise


def percentile(values, fraction):
    return sorted(values)[max(0, math.ceil(len(values) * fraction) - 1)]


def capability_provenance(value):
    # The command registry is inspectable from the API schema; avoid repeating
    # it in every performance case while retaining its exact digest.
    return {key: value[key] for key in ("api_schema_version", "engine_version", "semantic_versions", "backends", "media", "export_profiles") if key in value} | {
        "complete_capabilities_sha256": hashlib.sha256(canonical(value)).hexdigest()
    }


def frame_stage_summary(report):
    stage = report.get("performance", {}).get("stages", {}).get("render_tiles_and_conversion", {})
    values = stage.get("first_samples_us", [])
    if not values:
        return None
    result = {"first_frame_render_us": values[0], "sampled_frames": len(values),
              "note": "Same-worker host rendering including tile conversion; GPU/runtime creation excluded; first frame includes cold shaders."}
    if len(values) > 1:
        result.update(warm_frame_render_p50_us=percentile(values[1:], 0.5),
                      warm_frame_render_p95_us=percentile(values[1:], 0.95))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", type=Path, required=True)
    parser.add_argument("--after", type=Path, required=True)
    parser.add_argument("--case", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--backend", default="gpu")
    parser.add_argument("--pairs", type=int, default=9)
    parser.add_argument("--timeout", type=float, default=1800)
    parser.add_argument("--before-diagnostics", choices=["enabled", "disabled"], default="enabled")
    parser.add_argument("--after-diagnostics", choices=["enabled", "disabled"], default="enabled")
    parser.add_argument("--ffmpeg", default="ffmpeg")
    parser.add_argument("--ffprobe", default="ffprobe")
    args = parser.parse_args()
    if args.pairs < 2:
        parser.error("at least two pairs are required for initial/subsequent trials")
    case = json.loads(args.case.read_text())
    for source in case.get("sources", []):
        if digest(source["path"]) != source["sha256"]:
            raise RuntimeError(f"fixture hash mismatch: {source['path']}")
    binaries = {"before": args.before.resolve(), "after": args.after.resolve()}
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    project = args.output / "input.kronello"
    env = dict(os.environ, KRONELLO_STATE_ROOT=str(args.output / "prepare-state"))
    capabilities = {key: capability_provenance(call(binary, args.backend, env, "capabilities.get"))
                    for key, binary in binaries.items()}
    call(binaries["before"], args.backend, env, "project.create",
         project=str(project), document=case["document"])
    evidence = {"schema_version": 1, "case_sha256": digest(args.case),
                "document_sha256": hashlib.sha256(canonical(case["document"])).hexdigest(),
                "binary_sha256": {key: digest(binary) for key, binary in binaries.items()},
                "platform": platform.platform(), "machine": platform.machine(),
                "capabilities": capabilities, "backend": args.backend,
                "diagnostics_policy": {"before": args.before_diagnostics, "after": args.after_diagnostics},
                "quiet_host": False, "case": case, "trials": [],
                "notes": ["Each trial launches a fresh real worker; first/subsequent trials are not a warm persistent GPU process.",
                          "Wall includes CLI freeze/submit, queue, worker and terminal result loading; independent oracle excluded.",
                          "RSS is sampled from the worker every 100ms, not kernel maximum RSS, payload or GPU memory.",
                          "Stage times overlap; native conversion/send/receive/packet-write and GPU host preparation/submit/wait are separately observed when supported by this binary."]}
    receipt = args.output / "evidence.json"
    for pair in range(args.pairs):
        order = ["before", "after"] if pair % 2 == 0 else ["after", "before"]
        for mode in order:
            if digest(binaries[mode]) != evidence["binary_sha256"][mode]:
                raise RuntimeError("benchmark binary changed during measurement")
            print(f"pair {pair}: {mode}", flush=True)
            result = run_trial(binaries[mode], case, project,
                               args.output / f"{pair}-{mode}", args.backend,
                               args.timeout, args.ffmpeg, args.ffprobe,
                               diagnostics=(args.before_diagnostics if mode == "before"
                                            else args.after_diagnostics) == "enabled")
            if digest(binaries[mode]) != evidence["binary_sha256"][mode]:
                raise RuntimeError("benchmark binary changed during measurement")
            evidence["trials"].append({"pair": pair, "mode": mode, **result})
            receipt.write_text(json.dumps(evidence, ensure_ascii=False, indent=2) + "\n")
    evidence["summary"] = {}
    for mode in binaries:
        trials = [t for t in evidence["trials"] if t["mode"] == mode]
        warm = [t["succeeded_ms"] for t in trials[1:]]
        evidence["summary"][mode] = {"first_ms": trials[0]["succeeded_ms"],
                                     "frame_stage": frame_stage_summary(trials[-1]["report"]),
                                     "subsequent_p50_ms": percentile(warm, 0.5),
                                     "subsequent_p95_ms": percentile(warm, 0.95)}
    pairs = [[t for t in evidence["trials"] if t["pair"] == pair]
             for pair in range(args.pairs)]
    for pair in pairs:
        for identity in ("render_snapshot_hash", "export_snapshot_hash"):
            assert pair[0]["report"][identity] == pair[1]["report"][identity], identity
        assert pair[0]["report"]["sample_range"] == pair[1]["report"]["sample_range"]
        for left, right in zip(pair[0]["oracles"], pair[1]["oracles"], strict=True):
            assert left["streams"] == right["streams"], "decoded bits / PTS / layout"
            assert left["snapshot_tags"] == right["snapshot_tags"], "container identity"
    evidence["comparison"] = {"all_decoded_bits_pts_layout_equal": True,
                              "all_snapshot_hashes_equal": True,
                              "all_encoded_bytes_equal": all(
                                  left["encoded_sha256"] == right["encoded_sha256"]
                                  for pair in pairs for left, right in
                                  zip(pair[0]["oracles"], pair[1]["oracles"], strict=True))}
    receipt.write_text(json.dumps(evidence, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps(evidence["summary"], indent=2))


if __name__ == "__main__":
    main()
