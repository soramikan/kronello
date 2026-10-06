#!/usr/bin/env python3
"""Fetch the pinned UI fonts into the KronelloDesign resources.

Reads third_party/fonts/fonts.json, downloads each font, and verifies its size
and SHA-256 before writing it to
apps/macos/Sources/KronelloDesign/Resources/Fonts/. Existing files with the
right hash are kept. Exits non-zero on any mismatch; never falls back to
another font.

Usage: fetch_ui_fonts.py [--check]   (--check only verifies the files on disk)
"""

from __future__ import annotations

import hashlib
import json
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "third_party" / "fonts" / "fonts.json"
DEST = ROOT / "apps" / "macos" / "Sources" / "KronelloDesign" / "Resources" / "Fonts"


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def main() -> int:
    check_only = "--check" in sys.argv
    fonts = json.loads(MANIFEST.read_text())["fonts"]
    DEST.mkdir(parents=True, exist_ok=True)
    failed = False
    for font in fonts:
        path = DEST / font["file"]
        if path.exists():
            data = path.read_bytes()
            if len(data) == font["bytes"] and digest(data) == font["sha256"]:
                print(f"ok      {font['file']}")
                continue
            if check_only:
                print(f"BAD     {font['file']} (size or hash mismatch)", file=sys.stderr)
                failed = True
                continue
        elif check_only:
            print(f"MISSING {font['file']}", file=sys.stderr)
            failed = True
            continue
        with urllib.request.urlopen(font["url"], timeout=120) as resp:
            data = resp.read()
        if len(data) != font["bytes"] or digest(data) != font["sha256"]:
            print(f"BAD     {font['file']}: got {len(data)} bytes, sha256 {digest(data)}", file=sys.stderr)
            failed = True
            continue
        path.write_bytes(data)
        print(f"fetched {font['file']}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
