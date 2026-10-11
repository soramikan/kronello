#!/usr/bin/env python3
"""Measure release native request turnaround with/without subscriptions."""
import argparse
import copy
import ctypes
import json
import os
from pathlib import Path
import platform
import subprocess
import tempfile
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--samples", type=int, default=101)
    parser.add_argument("--interval-ms", type=float, default=0, help="Delay between requests; 16.667 exercises periodic polls")
    parser.add_argument("--library", type=Path, default=ROOT / "target/release/libkronello_ffi.dylib")
    args = parser.parse_args()
    assert args.samples > 0 and args.interval_ms >= 0
    native = ctypes.CDLL(str(args.library))
    for name, types, result in [
        ("open", [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_size_t], ctypes.c_uint64),
        ("call", [ctypes.c_uint64, ctypes.c_void_p, ctypes.c_size_t], ctypes.c_uint64),
        ("subscribe", [ctypes.c_uint64, ctypes.c_bool], ctypes.c_uint64),
        ("poll", [ctypes.c_uint64], ctypes.c_void_p),
        ("free", [ctypes.c_void_p], None),
        ("close", [ctypes.c_uint64], None),
    ]:
        fn = getattr(native, "kronello_" + name)
        fn.argtypes, fn.restype = types, result
    report = {"platform": platform.platform(), "revision": subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "build": "release; working tree", "cases": []}
    with tempfile.TemporaryDirectory(prefix="kronello-perf-") as directory:
        os.environ.setdefault("KRONELLO_STATE_ROOT", str(Path(directory) / "state"))
        for count in [100, 1000, 5000]:
            document = json.loads((ROOT / "examples/nle-001.project.json").read_text())
            sequence = document["sequences"][0]
            track = sequence["tracks"][0]
            clip = copy.deepcopy(track["clips"][0])
            clip["source_ref"] = {"kind": "generator", "generator": "kronello.solid", "version": 1,
                "color": {"space": "srgb", "components": {"r": 0, "g": 0, "b": 0, "alpha": 1}}}
            clip["properties"] = []
            track["clips"] = []
            for index in range(count):
                item = copy.deepcopy(clip)
                item["id"] = str(uuid.UUID(int=index + 1))
                item["timeline_range"] = {"start": {"num": str(index), "den": "1"},
                    "end": {"num": str(index + 1), "den": "1"}}
                track["clips"].append(item)
            sequence["tracks"] = [track]
            path = Path(directory) / f"{count}.kronello"
            creation = subprocess.run([str(ROOT / "target/release/kronello"), "--backend", "cpu-reference"],
                input=json.dumps({"operation": "project.create", "project": str(path), "document": document}),
                text=True, capture_output=True, check=True)
            assert json.loads(creation.stdout)["status"] == "success", creation.stdout
            for subscribed in [False, True]:
                encoded = str(path).encode()
                handle = native.kronello_open(encoded, len(encoded), None, 0)
                assert handle

                def wait(request):
                    deadline = time.monotonic() + 30
                    while time.monotonic() < deadline:
                        pointer = native.kronello_poll(handle)
                        if pointer:
                            try:
                                value = json.loads(ctypes.string_at(pointer))
                            finally:
                                native.kronello_free(pointer)
                            if value.get("request_id") == request:
                                return json.loads(value["response_json"])
                        time.sleep(0.0001)
                    raise TimeoutError("FFI completion")

                assert wait(0)["status"] == "success"
                wait(native.kronello_subscribe(handle, subscribed))
                # A cheap typed failure isolates work queued behind notification
                # polling. It performs no document read or GPU presentation.
                payload = b'{}'
                samples = []
                for _ in range(args.samples + 1):
                    start = time.perf_counter_ns()
                    request = native.kronello_call(handle, payload, len(payload))
                    assert request
                    assert wait(request)["status"] == "error"
                    samples.append((time.perf_counter_ns() - start) / 1e6)
                    time.sleep(args.interval_ms / 1000)
                native.kronello_close(handle)
                # Close is asynchronous; allow the last notification query to
                # release SQLite before removing the fixture directory.
                time.sleep(0.5)
                cold_ms, samples = samples[0], samples[1:]
                ordered = sorted(samples)
                case = {"clips": count, "subscribed": subscribed, "samples_ms": samples,
                    "cold_ms": cold_ms,
                    "interval_ms": args.interval_ms, "mean_ms": sum(samples) / len(samples), "max_ms": max(samples),
                    "p50_ms": ordered[len(ordered) // 2], "p95_ms": ordered[int((len(ordered) - 1) * .95)]}
                report["cases"].append(case)
                print(json.dumps({k: v for k, v in case.items() if k != "samples_ms"}), flush=True)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
