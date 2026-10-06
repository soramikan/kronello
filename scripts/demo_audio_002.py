#!/usr/bin/env python3
"""Prepare local, bounded tone + animated video fixtures for the host harness."""
import argparse
import copy
import json
import subprocess
import uuid
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def time(n, d=1):
    return {"num": str(n), "den": str(d)}

def prepare(output, cli):
    output.mkdir(parents=True, exist_ok=False)
    template = json.loads((ROOT / "examples/ffi-preview.project.json").read_text())
    manifest = []
    for num, den in [(24, 1), (24000, 1001), (30000, 1001)]:
        document = copy.deepcopy(template)
        document["id"] = str(uuid.uuid4())
        document["name"] = f"AUDIO-002 {num}/{den}"
        composition = document["compositions"][0]
        composition["duration"] = time(180)
        composition["edit_rate"] = time(num, den)
        for node in composition["nodes"]:
            node["active_range"] = {"start": time(0), "end": time(180)}
        # Motion remains visible over the measurement rather than holding at 1 s.
        document["curves"][0]["keys"][1]["time"] = time(180)
        sequence = str(uuid.uuid4())
        def clip(source):
            return {"id": str(uuid.uuid4()), "source_ref": source,
                    "timeline_range": {"start": time(0), "end": time(180)}, "source_in": time(0),
                    "time_map": {"kind": "linear", "offset": time(0), "speed": time(1)},
                    "links": [], "properties": [], "effects": []}
        document["sequences"] = [{"id": sequence, "extent": {"width": 64, "height": 32},
            "frame_rate": time(num, den), "audio_rate": 48000, "working_space": "linear_rec709",
            "tracks": [{"id": str(uuid.uuid4()), "kind": "video", "clips": [clip({"kind": "composition", "composition": composition["id"]})]},
                       {"id": str(uuid.uuid4()), "kind": "audio", "clips": [clip({"kind": "generator", "generator": "kronello.audio.tone440", "version": 1})]}],
            "transitions": []}]
        project = output / f"audio-{num}-{den}.kronello"
        result = subprocess.run([str(cli), "--backend", "cpu-reference"], input=json.dumps({"operation": "project.create", "project": str(project), "document": document}), text=True, capture_output=True)
        if result.returncode:
            raise RuntimeError(result.stdout + result.stderr)
        revision = json.loads(result.stdout)["result"]["value"]["revision"]
        query = subprocess.run([str(cli), "--backend", "cpu-reference"], input=json.dumps({"operation": "sequence.query", "project": str(project), "sequence": sequence}), text=True, capture_output=True)
        if query.returncode:
            raise RuntimeError(query.stdout + query.stderr)
        entry = {"project": str(project), "sequence": sequence, "fps_num": num, "fps_den": den, "revision": revision,
                 "trace": str(output / f"audio-{num}-{den}.jsonl")}
        manifest.append(entry)
        print(f'swift run --package-path apps/macos -j 3 KronelloAudioHarness "{project}" {sequence} {num} {den} {revision} "{entry["trace"]}"')
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-root", type=Path, required=True)
    parser.add_argument("--cli", type=Path, default=ROOT / "apps/macos/Libraries/kronello")
    args = parser.parse_args()
    prepare(args.output_root.resolve(), args.cli.resolve())

if __name__ == "__main__":
    main()
