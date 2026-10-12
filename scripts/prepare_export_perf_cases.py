#!/usr/bin/env python3
"""Create fixed export benchmark inputs once; retain and hash their bytes."""
import argparse
from fractions import Fraction
import json
from pathlib import Path
import subprocess

from perf_export_worker import digest

ROOT = Path(__file__).resolve().parents[1]
# These are allocated fixture identities, preserved across every trial.
ASSET = "543d322d-b358-435a-bc7d-69b59670b36a"
TRACK = "92f8c045-eed0-47db-9a2a-7409d2a22d69"
CLIP = "8cac0384-ae67-49db-bf39-785b5eb51fe7"


def rational(value):
    value = Fraction(value)
    return {"num": str(value.numerator), "den": str(value.denominator)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--ffmpeg", default="ffmpeg")
    parser.add_argument("--ffprobe", default="ffprobe")
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    source = output / "source-4k.mov"
    # Moving deterministic bars without nondeterministic timestamps or audio.
    command = [args.ffmpeg, "-v", "error", "-f", "lavfi", "-i",
               "testsrc2=size=3840x2160:rate=24:duration=2",
               "-c:v", "mpeg4", "-q:v", "4", "-threads", "1",
               "-pix_fmt", "yuv420p", "-color_primaries", "bt709",
               "-color_trc", "bt709", "-colorspace", "bt709", "-color_range", "tv",
               "-fflags", "+bitexact", "-flags:v", "+bitexact",
               "-map_metadata", "-1", str(source)]
    subprocess.run(command, check=True, timeout=600)
    probe = json.loads(subprocess.check_output(
        [args.ffprobe, "-v", "error", "-show_streams", "-of", "json", str(source)]))
    stream = probe["streams"][0]
    metadata = {"index": 0, "codec": stream["codec_name"],
                "time_base": rational(stream["time_base"]),
                "duration": rational(int(stream["duration_ts"]) * Fraction(stream["time_base"])),
                "start_time": rational(int(stream.get("start_pts", 0)) * Fraction(stream["time_base"])),
                "width": stream["width"], "height": stream["height"],
                "pixel_format": stream["pix_fmt"],
                "color_primaries": stream.get("color_primaries"),
                "color_transfer": stream.get("color_transfer"),
                "color_matrix": stream.get("color_space"), "color_range": stream.get("color_range")}
    document = json.loads((ROOT / "examples/perf-codec.project.json").read_text())
    document["name"] = "PERF-006 fixed real 4K video"
    sequence = document["sequences"][0]
    sequence["extent"] = {"width": 3840.0, "height": 2160.0}
    sequence["tracks"] = [{"id": TRACK, "kind": "video", "clips": [{
        "id": CLIP, "source_ref": {"kind": "asset", "asset": ASSET, "stream_index": 0},
        "timeline_range": {"start": rational(0), "end": rational(2)},
        "source_in": rational(0),
        "time_map": {"kind": "linear", "offset": rational(0), "speed": rational(1)},
        "links": [], "effects": [], "properties": []}]}]
    source_hash = digest(source)
    document["assets"] = [{"id": ASSET, "kind": "video", "content_hash": source_hash,
                           "locator": {"relative": source.name, "absolute": str(source)},
                           "streams": [metadata]}]
    cases = []
    for samples in (0, 3, 8):
        profile = {"working_space": "linear_rec709", "flatten_tolerance_px": 0.02}
        if samples:
            profile["temporal"] = {
            "frame_rate": rational(24), "shutter_angle": rational(180),
            "shutter_phase": rational(Fraction(-1, 4)),
            "samples": samples, "cut_policy": "avoid_crossing"}
        # One output frame is sufficient for exact conversion/evaluation counts.
        submit = {"render": {"input": {"target": {"kind": "sequence", "sequence": sequence["id"]},
                                         "region": {"origin": [0, 0], "extent": [3840, 2160],
                                                    "pixels": [3840, 2160]},
                                         "profile": profile, "fonts": []},
                              "range": {"start": rational(Fraction(1, 2)),
                                        "end": rational(Fraction(13, 24))},
                              "frame_rate": rational(24)},
                  "output": {"format": "pro_res_mov", "profile_version": 3,
                             "audio": "silence", "clips": [], "background": [0, 0, 0]},
                  "chapters": "omit"}
        case = {"schema_version": 1, "name": f"4k-video-samples{samples}",
                "document": document, "submit": submit,
                "sources": [{"path": str(source), "sha256": source_hash}],
                "fixture_generation": command, "expected_frames": 1}
        path = output / f"samples{samples}.json"
        path.write_text(json.dumps(case, ensure_ascii=False, indent=2) + "\n")
        cases.append({"path": str(path), "sha256": digest(path)})
    (output / "manifest.json").write_text(json.dumps({
        "schema_version": 1, "cases": cases, "source_sha256": source_hash,
        "fixture_generation": command, "probe": probe}, indent=2) + "\n")


if __name__ == "__main__":
    main()
