#!/usr/bin/env python3
"""Keep measurements and exact digests without duplicating long frame arrays."""
import argparse
import hashlib
import json
from pathlib import Path

from perf_export_worker import canonical, digest, percentile


def compact(value, key=""):
    if isinstance(value, dict):
        return {name: compact(item, name) for name, item in value.items()}
    if isinstance(value, list):
        if len(value) > 64 or (key == "frames" and len(value) > 1) or (key == "first_samples_us" and len(value) > 8):
            encoded = canonical(value)
            result = {"count": len(value), "canonical_bytes": len(encoded),
                    "canonical_sha256": hashlib.sha256(encoded).hexdigest(),
                    "first": compact(value[0]), "last": compact(value[-1])}
            if key == "first_samples_us":
                result.update(p50_us=percentile(value, 0.5), p95_us=percentile(value, 0.95))
            return result
        return [compact(item) for item in value]
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("evidence", type=Path, nargs="+")
    args = parser.parse_args()
    records = []
    for path in args.evidence:
        value = json.loads(path.read_text())
        if not value.get("comparison", {}).get("all_decoded_bits_pts_layout_equal"):
            raise RuntimeError(f"unfinished or failed comparison: {path}")
        records.append({"raw_path": str(path.resolve()), "raw_bytes": path.stat().st_size,
                        "raw_sha256": digest(path), "evidence": compact(value)})
    args.output.write_text(json.dumps({"schema_version": 1, "records": records},
                                     ensure_ascii=False, indent=2) + "\n")


if __name__ == "__main__":
    main()
