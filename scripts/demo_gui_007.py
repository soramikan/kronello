#!/usr/bin/env python3
"""Prepare a persistent shared-API GUI-007 review project; no GUI automation."""
import argparse
import copy
import hashlib
import json
import math
import shutil
import struct
import subprocess
import wave
from pathlib import Path
from uuid import uuid4

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-root", type=Path, required=True)
    parser.add_argument("--lightweight", action="store_true")
    args = parser.parse_args()
    output = args.output_root.resolve()
    output.mkdir(parents=True, exist_ok=False)
    document = json.loads((ROOT / "examples/m5-text-matte.project.json").read_text())
    document["id"] = str(uuid4())
    document["name"] = "GUI-007 編集操作確認"
    comp = document["compositions"][0]
    if args.lightweight:
        moving = comp["nodes"][0]
        curve_id = str(uuid4())
        document["curves"].append({"id": curve_id, "value_type": "vec2", "interpolation_version": 1, "keys": [
            {"time": {"num": "0", "den": "1"}, "value": {"kind": "vec2", "value": [160.0, 180.0]}, "interpolation": {"kind": "linear"}},
            {"time": {"num": "3", "den": "1"}, "value": {"kind": "vec2", "value": [480.0, 180.0]}, "interpolation": {"kind": "linear"}}]})
        for prop in moving["properties"]:
            if prop["descriptor"]["key"] == "kronello.transform.position":
                prop["source"] = {"kind": "curve", "value": curve_id}
        background = copy.deepcopy(moving)
        background["id"] = str(uuid4())
        background["name"] = "Static background"
        shape = copy.deepcopy(document["shapes"][0])
        shape["id"] = str(uuid4())
        background["kind"]["value"]["content_ref"] = shape["id"]
        ids = {prop["id"]: str(uuid4()) for prop in background["properties"]}
        for prop in background["properties"]:
            prop["id"] = ids[prop["id"]]
            key = prop["descriptor"]["key"]
            if key == "kronello.transform.position":
                prop["source"] = {"kind": "constant", "value": {"kind": "vec2", "value": [260, 180]}}
            elif key == "kronello.fill_color":
                prop["source"]["value"]["value"]["components"].update(r=0.3, g=0.65, b=0.4)
        shape["geometry"]["value"] = {key: ids.get(value, value) for key, value in shape["geometry"]["value"].items()}
        shape["fill"]["color"] = ids[shape["fill"]["color"]]
        comp["nodes"] = [background, moving]
        comp["root_nodes"] = [background["id"], moving["id"]]
        comp["design_extent"] = {"width": 640.0, "height": 480.0}
        document["shapes"].append(shape)
        document["texts"] = []
        document.pop("mattes", None)
    rational = lambda n, d=1: {"num": str(n), "den": str(d)}
    audio = output / "tone.wav"
    with wave.open(str(audio), "wb") as wav:
        wav.setnchannels(2)
        wav.setsampwidth(2)
        wav.setframerate(48000)
        wav.writeframes(b"".join(struct.pack("<hh", *([int(6500 * math.sin(n * 440 * math.tau / 48000))] * 2)) for n in range(48000 * 3)))
    aid = str(uuid4())
    document["assets"] = [{"id": aid, "kind": "audio", "content_hash": hashlib.sha256(audio.read_bytes()).hexdigest(),
        "locator": {"relative": audio.name, "absolute": str(audio)}, "streams": [{"index": 0, "codec": "pcm_s16le",
        "time_base": rational(1,48000), "duration": rational(3)}]}]
    def clip(source):
        return {"id": str(uuid4()), "source_ref": source, "timeline_range": {"start": rational(0), "end": rational(3)},
            "source_in": rational(0), "time_map": {"kind": "linear", "offset": rational(0), "speed": rational(1)},
            "audio_retime": "reject", "properties": [], "effects": [], "links": []}
    backdrop = clip({"kind": "generator", "generator": "kronello.solid", "version": 1,
        "color": {"space": "srgb", "components": {"r": 0.2, "g": 0.3, "b": 0.45, "alpha": 1.0}}})
    video = clip({"kind": "composition", "composition": comp["id"]})
    audio_clip = clip({"kind": "asset", "asset": aid, "stream_index": 0})
    sequence = {"id": str(uuid4()), "extent": comp["design_extent"], "frame_rate": comp["edit_rate"],
        "audio_rate": 48000, "working_space": "linear_rec709", "tracks": [
        {"id": str(uuid4()), "kind": "video", "clips": [backdrop]},
        {"id": str(uuid4()), "kind": "video", "clips": [video]},
        {"id": str(uuid4()), "kind": "audio", "clips": [audio_clip]}], "transitions": []}
    document["sequences"] = [sequence]
    project = output / "edit.kronello"
    (output / "document.json").write_text(json.dumps(document, ensure_ascii=False, indent=2) + "\n")
    fonts = ROOT / "target/m5-acceptance/gui/fonts.json"
    if fonts.is_file():
        shutil.copy2(fonts, output / "fonts.json")
    result = subprocess.run([str(ROOT / "apps/macos/Libraries/kronello"), "--backend", "cpu-reference"],
        input=json.dumps({"operation": "project.create", "project": str(project), "document": document}), text=True, capture_output=True, check=True)
    response = json.loads(result.stdout)
    if response["status"] != "success":
        raise RuntimeError(response)
    (output / "identities.json").write_text(json.dumps({"project": str(project), "sequence": sequence["id"],
        "video_clip": video["id"], "audio_clip": audio_clip["id"], "tracks": [track["id"] for track in sequence["tracks"]]}, indent=2) + "\n")
    print(project)


if __name__ == "__main__":
    main()
