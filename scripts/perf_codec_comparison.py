#!/usr/bin/env python3
"""Compare fixed codec benchmark binaries with independent streaming oracles."""
import argparse
import json
from pathlib import Path
import subprocess

from perf_export_worker import digest, independent_oracle, percentile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", type=Path, required=True)
    parser.add_argument("--after", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--ffmpeg", default="ffmpeg")
    parser.add_argument("--ffprobe", default="ffprobe")
    args = parser.parse_args()
    root = args.output.resolve()
    root.mkdir(parents=True, exist_ok=False)
    binaries = {"before": args.before.resolve(), "after": args.after.resolve()}
    evidence = {"schema_version": 1, "quiet_host": False,
                "scope": "synthetic tiles, audio/video encode, mux/probe/publication; runtime load and independent oracle excluded",
                "binary_sha256": {key: digest(path) for key, path in binaries.items()},
                "notes": ["Batch order alternates; trials inside each nine-trial batch are not paired alternately.",
                          "Each batch has initial1/subsequent8; summaries use nearest-rank percentiles.",
                          "Executable hashes identify artifacts, not their codec settings; retain the source diff separately."],
                "batches": []}
    for batch, mode in enumerate(["before", "after", "after", "before"]):
        output = root / f"{batch}-{mode}"
        raw = root / f"{batch}-{mode}.json"
        binary = binaries[mode]
        assert digest(binary) == evidence["binary_sha256"][mode]
        with raw.open("w") as stdout, (root / f"{batch}-{mode}.log").open("w") as stderr:
            subprocess.run([str(binary), str(output)], stdout=stdout, stderr=stderr,
                           check=True, timeout=1800)
        report = json.loads(raw.read_text())
        assert report["binary_sha256"] == digest(binary) == evidence["binary_sha256"][mode]
        for case in report["cases"]:
            for trial in case["trials"]:
                trial["oracle"] = independent_oracle(Path(trial["output"]),
                                                      args.ffmpeg, args.ffprobe)
            warm = [t["ms"] for t in case["trials"][1:]]
            case["nearest_rank_summary"] = {"initial_ms": case["trials"][0]["ms"],
                                            "warm_p50_ms": percentile(warm, 0.5),
                                            "warm_p95_ms": percentile(warm, 0.95)}
        evidence["batches"].append({"batch": batch, "mode": mode, "report": report})
        (root / "evidence.json").write_text(json.dumps(evidence, indent=2) + "\n")
        print(f"completed {batch}: {mode}", flush=True)
    comparisons = []
    for a, b in [(0, 1), (2, 3)]:
        left = evidence["batches"][a]["report"]
        right = evidence["batches"][b]["report"]
        assert left["fixture_sha256"] == right["fixture_sha256"]
        for lc, rc in zip(left["cases"], right["cases"], strict=True):
            assert lc["case"] == rc["case"]
            for lt, rt in zip(lc["trials"], rc["trials"], strict=True):
                for key in ["render_snapshot_hash", "export_snapshot_hash", "sample_range",
                            "backend_calls", "checkpoints"]:
                    assert lt[key] == rt[key], (lc["case"], key)
                assert lt["oracle"]["streams"] == rt["oracle"]["streams"], lc["case"]
                assert lt["oracle"]["snapshot_tags"] == rt["oracle"]["snapshot_tags"]
            comparisons.append({"batches": [a, b], "case": lc["case"],
                                "all_decoded_bits_pts_layout_equal": True,
                                "all_snapshot_hashes_equal": True,
                                "all_encoded_bytes_equal": all(
                                    lt["encoded_sha256"] == rt["encoded_sha256"]
                                    for lt, rt in zip(lc["trials"], rc["trials"], strict=True))})
    evidence["comparisons"] = comparisons
    (root / "evidence.json").write_text(json.dumps(evidence, indent=2) + "\n")
    print(json.dumps(comparisons, indent=2))


if __name__ == "__main__":
    main()
