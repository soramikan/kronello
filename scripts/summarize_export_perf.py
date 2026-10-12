#!/usr/bin/env python3
"""Keep measurements and exact digests without duplicating long frame arrays."""
import argparse
import hashlib
import json
from pathlib import Path

from perf_export_worker import canonical, digest


def compact(value):
    if isinstance(value, dict):
        return {key: compact(item) for key, item in value.items()}
    if isinstance(value, list):
        if len(value) > 64:
            encoded = canonical(value)
            return {"count": len(value), "canonical_bytes": len(encoded),
                    "canonical_sha256": hashlib.sha256(encoded).hexdigest(),
                    "first": compact(value[0]), "last": compact(value[-1])}
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
