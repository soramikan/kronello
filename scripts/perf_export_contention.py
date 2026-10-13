#!/usr/bin/env python3
"""Compare serial and concurrent independent audio/video workers.

This measures resource competition, not a production within-job overlap path.
Oracles run after both workers finish, outside their resource/timing interval.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import platform
import time

from perf_export_worker import (call, canonical, digest, independent_oracle,
                                percentile, run_trial, semantic_streams)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--video-case", required=True, type=Path)
    parser.add_argument("--audio-case", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--pairs", type=int, default=3)
    args = parser.parse_args()
    if args.pairs < 2:
        parser.error("at least two pairs required")
    binary = args.binary.resolve()
    sha = digest(binary)
    root = args.output.resolve()
    root.mkdir(parents=True, exist_ok=False)
    inputs = {}
    for name, path, backend in (("video", args.video_case, "gpu"),
                                ("audio", args.audio_case, "cpu-reference")):
        case = json.loads(path.read_text())
        project = root / f"{name}.kronello"
        call(binary, backend, dict(os.environ, KRONELLO_STATE_ROOT=str(root / "prepare")),
             "project.create", project=str(project), document=case["document"])
        inputs[name] = (case, project, backend)
    evidence = {"schema_version": 1, "binary_sha256": sha,
                "platform": platform.platform(), "quiet_host": False,
                "cases": {name: {"case": item[0],
                                  "document_sha256": hashlib.sha256(canonical(item[0]["document"])).hexdigest()}
                          for name, item in inputs.items()},
                "notes": ["Independent workers; no within-job production parallelism added.",
                          "Both output oracles run after both workers finish.",
                          "Resource observations are bounded samples, not exit totals; kernel disk I/O excludes cached reads."],
                "trials": []}
    receipt = root / "evidence.json"

    def trial(name, directory):
        case, project, backend = inputs[name]
        return run_trial(binary, case, project, directory, backend, 1800,
                         "ffmpeg", "ffprobe", raster_cache=False, verify_outputs=False)

    for pair in range(args.pairs):
        for mode in (("serial", "concurrent") if pair % 2 == 0 else ("concurrent", "serial")):
            if digest(binary) != sha:
                raise RuntimeError("frozen binary changed")
            directory = root / f"{pair}-{mode}"
            directory.mkdir()
            print(f"pair {pair}: {mode}", flush=True)
            start = time.monotonic()
            if mode == "serial":
                results = {name: trial(name, directory / name) for name in inputs}
            else:
                with ThreadPoolExecutor(max_workers=2) as executor:
                    futures = {name: executor.submit(trial, name, directory / name) for name in inputs}
                    results = {name: future.result() for name, future in futures.items()}
            wall_ms = (time.monotonic() - start) * 1000
            for result in results.values():
                result["oracles"] = [independent_oracle(Path(p), "ffmpeg", "ffprobe")
                                     for p in result["output_paths"]]
            evidence["trials"].append({"pair": pair, "mode": mode,
                                       "both_workers_ms": wall_ms, "workers": results})
            receipt.write_text(json.dumps(evidence, ensure_ascii=False, indent=2) + "\n")
    for pair in range(args.pairs):
        a, b = [t for t in evidence["trials"] if t["pair"] == pair]
        for name in inputs:
            left, right = a["workers"][name], b["workers"][name]
            for key in ("render_snapshot_hash", "export_snapshot_hash", "sample_range"):
                assert left["report"][key] == right["report"][key], key
            for x, y in zip(left["oracles"], right["oracles"], strict=True):
                assert semantic_streams(x) == semantic_streams(y)
                assert x["streams"] == y["streams"]
                assert x["encoded_sha256"] == y["encoded_sha256"]
                assert x["chapters"] == y["chapters"]
                assert x["snapshot_tags"] == y["snapshot_tags"]
    evidence["comparison"] = {"all_decoded_bits_pts_layout_equal": True,
                              "all_packet_and_encoded_bytes_equal": True}
    evidence["summary"] = {}
    for mode in ("serial", "concurrent"):
        values = [t["both_workers_ms"] for t in evidence["trials"] if t["mode"] == mode]
        evidence["summary"][mode] = {"first_ms": values[0],
                                     "subsequent_p50_ms": percentile(values[1:], 0.5),
                                     "subsequent_p95_ms": percentile(values[1:], 0.95)}
    receipt.write_text(json.dumps(evidence, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps(evidence["summary"], indent=2))


if __name__ == "__main__":
    main()
