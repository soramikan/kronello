#!/usr/bin/env python3
"""Create an Edit-page review fixture through the shared CLI, without GPU use."""
import argparse
import copy
import hashlib
import json
import subprocess
import wave
from pathlib import Path
from uuid import uuid4

ROOT = Path(__file__).resolve().parents[1]


def rational(num, den=1):
    from fractions import Fraction
    value = Fraction(num, den)
    return {"num": str(value.numerator), "den": str(value.denominator)}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-root", type=Path, required=True)
    parser.add_argument("--binary", type=Path, default=ROOT / "apps/macos/Libraries/kronello")
    parser.add_argument("--action", choices=["prepare", "move", "info"], default="prepare")
    args = parser.parse_args()
    output = args.output_root.resolve()
    project = output / "edit.kronello"

    def call(operation, **fields):
        request = {"operation": operation, "project": str(project), **fields}
        result = subprocess.run([str(args.binary.resolve())], input=json.dumps(request, sort_keys=True),
                                text=True, capture_output=True, check=True)
        response = json.loads(result.stdout)
        if response["status"] != "success":
            raise RuntimeError(response)
        return response["result"]["value"]

    if args.action == "prepare":
        output.mkdir(parents=True)  # Never overwrite an existing review root.
        video = output / "video.mov"
        subprocess.run(["ffmpeg", "-v", "error", "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=24:duration=5",
                        "-c:v", "mpeg4", "-pix_fmt", "yuv420p", "-an", str(video)], check=True)
        audio = output / "audio.wav"
        with wave.open(str(audio), "wb") as wav:
            wav.setnchannels(1)
            wav.setsampwidth(2)
            wav.setframerate(48000)
            wav.writeframes(b"\x00\x00" * 48000 * 5)
        document = json.loads((ROOT / "examples/m1-demo.project.json").read_text())
        document["id"] = str(uuid4())
        document["name"] = "GUI-003 日本語カット編集"
        composition = document["compositions"][0]

        def asset(path, kind, index, **metadata):
            return {"id": str(uuid4()), "content_hash": hashlib.sha256(path.read_bytes()).hexdigest(), "kind": kind,
                    "locator": {"relative": path.name, "absolute": str(path)},
                    "streams": [{"index": index, "codec": "mpeg4" if kind == "video" else "pcm_s16le",
                                 "time_base": rational(1, 24 if kind == "video" else 48000), "duration": rational(5),
                                 "width": None, "height": None, "pixel_format": None, "color_primaries": None,
                                 "color_transfer": None, "color_matrix": None, "color_range": None, **metadata}]}

        v = asset(video, "video", 0, width=320, height=180, pixel_format="yuv420p")
        a = asset(audio, "audio", 0)
        missing = copy.deepcopy(v)
        missing["id"] = str(uuid4())
        missing["locator"] = {"relative": "missing.mov", "absolute": str(output / "missing.mov")}
        document["assets"] = [v, a, missing]

        def clip(source, start, end):
            return {"id": str(uuid4()), "source_ref": source, "timeline_range": {"start": rational(start), "end": rational(end)},
                    "source_in": rational(0), "time_map": {"kind": "linear", "offset": rational(0), "speed": rational(1)},
                    "links": [], "effects": [], "properties": []}

        sequence = {"id": str(uuid4()), "extent": {"width": 320, "height": 180}, "frame_rate": rational(24),
                    "audio_rate": 48000, "working_space": "linear_rec709", "tracks": [
                        {"id": str(uuid4()), "kind": "video", "clips": [clip({"kind": "asset", "asset": v["id"], "stream_index": 0}, 0, 3),
                           clip({"kind": "asset", "asset": missing["id"], "stream_index": 0}, 4, 5)]},
                        {"id": str(uuid4()), "kind": "video", "clips": [clip({"kind": "composition", "composition": composition["id"]}, 1, 4)]},
                        {"id": str(uuid4()), "kind": "audio", "clips": [clip({"kind": "asset", "asset": a["id"], "stream_index": 0}, 0, 5)]}]}
        document["sequences"] = [sequence]
        (output / "document.json").write_text(json.dumps(document, ensure_ascii=False, indent=2) + "\n")
        fonts = [{"identity": document["texts"][0]["styles"][0]["font"],
                  "path": str(ROOT / "target/fixtures/external/NotoSansCJKjp-Regular.otf")}]
        (output / "fonts.json").write_text(json.dumps(fonts, indent=2) + "\n")
        print(json.dumps(call("project.create", document=document), ensure_ascii=False))
        state = output / "user-state/ui-state"
        state.mkdir(parents=True)
        (state / (document["id"] + ".json")).write_text(json.dumps({
            "page": "edit", "workspace": "standard", "layouts": {}, "sequence": sequence["id"], "clipSelection": sequence["tracks"][1]["clips"][0]["id"],
            "locked": [], "collapsed": [], "zoom": "fit", "panX": 0, "panY": 0, "tool": "select", "bounds": "layout",
            "resolution": "full", "looping": False, "time": rational(0)}))
    elif args.action == "move":
        exported = call("project.export")
        sequence = exported["document"]["sequences"][0]
        commands = [{"timeline": {"clip_move": {"sequence": sequence["id"], "clip": sequence["tracks"][1]["clips"][0]["id"],
                                                 "delta": rational(1, 24), "linked": True}}}]
        plan = call("edit.plan", base_revision=exported["revision"], commands=commands)
        print(json.dumps(call("edit.apply", base_revision=exported["revision"], commands=commands, plan_hash=plan["plan_hash"],
                              session_id=str(uuid4()), idempotency_key=str(uuid4())), ensure_ascii=False))
    else:
        print(json.dumps(call("project.info"), ensure_ascii=False))
        print(json.dumps(call("project.export"), ensure_ascii=False))
    print(project)


if __name__ == "__main__":
    main()
