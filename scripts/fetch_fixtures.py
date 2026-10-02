#!/usr/bin/env python3
"""Fetch only manifest-pinned HTTPS fixtures; fail on missing or incorrect bytes."""
import argparse
import hashlib
import os
from pathlib import Path
import sys
import tempfile
import urllib.request
from fixtures import load_manifest, require, external_fixture_dir


class HTTPSOnly(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        require(newurl.startswith("https://"), "non-HTTPS redirect rejected")
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def verify(path, entry):
    hasher = hashlib.sha256()
    count = 0
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            count += len(chunk); hasher.update(chunk)
    return count == entry["bytes"] and hasher.hexdigest() == entry["sha256"]


def fetch(entry, output, offline=False):
    destination = output / Path(entry["path"]).name
    if destination.is_file() and verify(destination, entry):
        return destination
    require(not offline, f"missing or corrupt offline fixture: {entry['id']}")
    output.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=output, delete=False) as target:
            temporary = Path(target.name)
            request = urllib.request.Request(entry["url"], headers={"User-Agent": "kronello-fixture-fetch/1"})
            with urllib.request.build_opener(HTTPSOnly()).open(request, timeout=60) as response:
                require(response.geturl().startswith("https://"), "non-HTTPS response")
                count = 0
                while chunk := response.read(1024 * 1024):
                    count += len(chunk)
                    require(count <= entry["bytes"], f"oversize download: {entry['id']}")
                    target.write(chunk)
        require(verify(temporary, entry), f"SHA-256/size mismatch: {entry['id']}")
        os.replace(temporary, destination)
        temporary = None
        return destination
    finally:
        if temporary is not None: temporary.unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=external_fixture_dir())
    parser.add_argument("--offline", action="store_true")
    args = parser.parse_args()
    try:
        entries = [entry for entry in load_manifest()["fixtures"] if entry["storage"] == "external"]
        require(entries, "no external fixtures configured")
        for entry in entries:
            print(f"verified {entry['id']}: {fetch(entry, args.output, args.offline)}")
    except (OSError, ValueError, KeyError) as error:
        print(f"fixture fetch failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
