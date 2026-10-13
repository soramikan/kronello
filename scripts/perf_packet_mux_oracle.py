#!/usr/bin/env python3
"""Verify completed packet-mux trials outside their timed interval."""
import argparse
import json
from pathlib import Path

from perf_export_worker import digest, independent_oracle, percentile, semantic_streams


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--ffmpeg", default="ffmpeg")
    parser.add_argument("--ffprobe", default="ffprobe")
    args = parser.parse_args()
    evidence = json.loads(args.evidence.read_text())
    trials = evidence["trials"]
    assert len(trials) >= 6
    for trial in trials:
        trial["oracle"] = independent_oracle(Path(trial["output"]), args.ffmpeg, args.ffprobe)
    encoded = packets = True
    pairs = sorted({trial["pair"] for trial in trials})
    for pair in pairs:
        group = {t["mode"]: t for t in trials if t["pair"] == pair}
        assert set(group) == {"intermediate", "direct"}
        left, right = [group[mode]["oracle"] for mode in ("intermediate", "direct")]
        assert semantic_streams(left) == semantic_streams(right), "decoded pixels/PCM/timing/layout/priming"
        assert left["snapshot_tags"] == right["snapshot_tags"]
        assert left["chapters"] == right["chapters"]
        packets &= left["streams"] == right["streams"]
        encoded &= left["encoded_sha256"] == right["encoded_sha256"]
    evidence["comparison"] = {
        "all_decoded_pixels_pcm_timing_layout_priming_equal": True,
        "all_snapshot_tags_and_chapters_equal": True,
        "all_packet_payload_equal": packets,
        "all_encoded_bytes_equal": encoded,
    }
    evidence["summary"] = {}
    for mode in ("intermediate", "direct"):
        selected = [t for t in trials if t["mode"] == mode]
        warm = [t["report"]["total_us"] for t in selected[1:]]
        evidence["summary"][mode] = {
            "first_us": selected[0]["report"]["total_us"],
            "subsequent_p50_us": percentile(warm, 0.5),
            "subsequent_p95_us": percentile(warm, 0.95),
            "intermediate_video_bytes": [t["report"]["intermediate_video_bytes"] for t in selected],
        }
    evidence["provenance"] = {
        "source_evidence": str(args.evidence.resolve()),
        "source_evidence_sha256": digest(args.evidence),
        "binary": str(args.binary.resolve()),
        "binary_sha256": digest(args.binary),
        "note": "Same frozen binary, fixed input, alternating pairs. Fresh encoder per trial; shared loaded runtime. First/subsequent is not a persistent encoder. Generator, audio preparation, encode, mux, validation and publication are timed. Independent oracle excluded. Quiet host not guaranteed.",
    }
    args.output.write_text(json.dumps(evidence, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps(evidence["summary"], indent=2))
    print(json.dumps(evidence["comparison"], indent=2))


if __name__ == "__main__":
    main()
