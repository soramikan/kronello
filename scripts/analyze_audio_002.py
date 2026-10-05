#!/usr/bin/env python3
"""Report real-engine/Metal-submission evidence; never infer physical scanout."""
import argparse
import json
from collections import defaultdict
from fractions import Fraction
from pathlib import Path

def analyze(path):
    records = [json.loads(line) for line in path.read_text().splitlines()]
    events = [r for r in records if r["record"] == "event"]
    frames = [r for r in records if r["record"] == "presentation"]
    if not frames or any(r["master"] != "audio_device" for r in frames):
        raise ValueError("Real audio-device presentation evidence required")
    epochs = defaultdict(list)
    offsets = []
    grid_offsets = []
    rates = set()
    for r in frames:
        num, den = int(r["fps_num"]), int(r["fps_den"])
        rates.add((num, den))
        expected = int(r["audio_sample"]) * num // (48000 * den)
        if expected != int(r["expected_frame"]) or int(r["offset_frames"]) != int(r["frame"]) - expected:
            raise ValueError("Inconsistent exact audio clock/frame arithmetic")
        epochs[r["epoch"]].append(int(r["callback_sample"]))
        grid_offsets.append(abs(int(r["offset_frames"])) * Fraction(den * 1000, num))
        offsets.append(abs(Fraction(int(r["frame"]) * den * 1000, num) - Fraction(int(r["audio_sample"]) * 1000, 48000)))
    duration = sum(Fraction(max(samples)-min(samples), 48000) for samples in epochs.values())
    if duration < 30:
        raise ValueError(f"Only {float(duration):.3f} seconds of observed live callback clock; require >=30")
    stop = next(r for r in events if r["name"] == "stop")
    resume = next(r for r in events if r["name"] == "resume")
    finish = next(r for r in events if r["name"] == "finish")
    if stop["sample"] != resume["sample"]:
        raise ValueError("Stop/resume sample mismatch")
    if len(rates) != 1:
        raise ValueError("One frame rate required per trace")
    num, den = next(iter(rates))
    seeks = [r for r in events if r["name"] == "seek"]
    if len(seeks) < 2 or any(int(r["sample"]) != int(r["frame"]) * den * 48000 // num for r in seeks):
        raise ValueError("Missing or inexact seeks")
    return {"trace": str(path), "measurement": "real_audio_engine_metal_submission",
            "fps": f"{num}/{den}", "observed_device_seconds": float(duration), "presentations": len(frames),
            "max_av_offset_frames": max(abs(int(r["offset_frames"])) for r in frames),
            "max_av_offset_ms": float(max(offsets)), "underruns": max(int(r["underruns"]) for r in frames + [finish]),
            "max_frame_grid_offset_ms": float(max(grid_offsets)),
            "missing_frames": max(int(r["missing_frames"]) for r in frames + [finish]), "seek_count": len(seeks),
            "stop_resume_sample_exact": True, "physical_scanout_or_loopback": "not_measured"}

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("traces", type=Path, nargs="+")
    args = parser.parse_args()
    reports = [analyze(path) for path in args.traces]
    print(json.dumps(reports, indent=2))

if __name__ == "__main__":
    main()
