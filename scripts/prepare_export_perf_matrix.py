#!/usr/bin/env python3
"""Prepare remaining worker cases with persisted, preallocated identities."""
import argparse
from copy import deepcopy
from fractions import Fraction
import json
import math
from pathlib import Path
import struct
import wave

from perf_export_worker import digest
from prepare_export_perf_cases import rational

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--video-case", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--font", type=Path, required=True)
    parser.add_argument("--audio-seconds", type=int, default=601)
    parser.add_argument("--metadata-frames", type=int, default=10000)
    args = parser.parse_args()
    if args.audio_seconds < 2 or args.metadata_frames < 1:
        parser.error("positive metadata count and at least two audio seconds required")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    identities = json.loads((ROOT / "examples/perf-export-identities.json").read_text())
    ids = identities["roles"]
    base = json.loads(args.video_case.read_text())
    cases = []

    def save(name, case):
        case["name"] = name
        path = output / f"{name}.json"
        path.write_text(json.dumps(case, ensure_ascii=False, indent=2) + "\n")
        cases.append({"path": str(path), "sha256": digest(path)})

    # Independent placements of the same asset, at different source clocks.
    case = deepcopy(base)
    clips = case["document"]["sequences"][0]["tracks"][0]["clips"]
    first = clips[0]
    first["timeline_range"]["end"] = rational(1)
    second = deepcopy(first)
    second["id"] = ids["alternate_clip"]
    second["time_map"]["speed"] = rational(2)
    case["document"]["sequences"][0]["tracks"].append({
        "id": ids["alternate_track"], "kind": "video", "clips": [second]})
    save("same-source-independent-speed", case)

    # Many authored placements plus real Japanese captions and a blur halo.
    case = deepcopy(base)
    m1 = json.loads((ROOT / "examples/m1-demo.project.json").read_text())
    font = m1["texts"][0]["styles"][0]["font"]
    if digest(args.font) != font["sha256"]:
        raise RuntimeError("fixed Noto font hash mismatch")
    document = m1
    sequence = deepcopy(case["document"]["sequences"][0])
    sequence["extent"] = {"width": 64.0, "height": 32.0}
    template = deepcopy(sequence["tracks"][0]["clips"][0])
    template["source_ref"] = {"kind": "composition", "composition": m1["compositions"][0]["id"]}
    template["effects"] = [{"effect_id": "kronello.gaussian_blur", "version": 1,
                             "parameters": {"kind": "gaussian_blur", "sigma": ids["blur_sigma"]}}]
    template["properties"] = [{"id": ids["blur_sigma"],
                                "descriptor": {"key": "kronello.effect.sigma", "version": 1},
                                "source": {"kind": "constant", "value": {"kind": "scalar", "value": 0.5}},
                                "modifiers": []}]
    clips = []
    for number, identity in enumerate(identities["clip_ids"]):
        clip = deepcopy(template)
        clip["id"] = identity
        # Each authored parameter has its own preallocated stable identity.
        sigma = identities["blur_property_ids"][number]
        clip["properties"][0]["id"] = sigma
        clip["effects"][0]["parameters"]["sigma"] = sigma
        clip["timeline_range"] = {"start": rational(Fraction(number, 24)),
                                  "end": rational(Fraction(number + 1, 24))}
        clips.append(clip)
    sequence["tracks"][0]["clips"] = clips
    document["sequences"] = [sequence]
    document["captions"] = [{"id": ids["caption"], "version": 1,
                             "text": "日本語の字幕\n書き出しの性能計測", "style": {
                                 "font": font, "size": 2.0,
                                 "fill": {"space": "srgb", "components": {
                                     "r": 1, "g": 1, "b": 1, "alpha": 1}}},
                             "placement": {"anchor": "bottom_center",
                                           "safe_area_inset": [rational(Fraction(1, 20))] * 2,
                                           "offset": [rational(0)] * 2}}]
    cue = deepcopy(template)
    cue.update(id=ids["caption_clip"], source_ref={"kind": "caption", "caption": ids["caption"]},
               timeline_range={"start": rational(0), "end": rational(2)}, effects=[], properties=[])
    sequence["tracks"].append({"id": ids["caption_track"], "kind": "caption", "clips": [cue]})
    case["document"] = document
    case["sources"] = [{"path": str(args.font.resolve()), "sha256": font["sha256"]}]
    case["submit"]["render"]["input"]["region"] = {"origin": [0, 0], "extent": [64, 32],
                                                      "pixels": [1920, 1080]}
    case["submit"]["render"]["input"]["fonts"] = [{"identity": font, "path": str(args.font.resolve())}]
    # A half-second run makes repeated validation/static preparation observable.
    case["submit"]["render"]["range"]["end"] = rational(1)
    case["expected_frames"] = 12
    save("3000-clips-japanese-captions-blur", case)

    # A long source with a short authored excerpt near its end.
    audio = output / "long-tone.wav"
    block = b"".join(struct.pack("<hh", sample, sample) for sample in
                     (int(3000 * math.sin(2 * math.pi * 440 * n / 48000)) for n in range(48000)))
    with wave.open(str(audio), "wb") as writer:
        writer.setnchannels(2)
        writer.setsampwidth(2)
        writer.setframerate(48000)
        for _ in range(args.audio_seconds):
            writer.writeframesraw(block)
    audio_hash = digest(audio)
    case = deepcopy(base)
    document = case["document"]
    document["assets"].append({"id": ids["audio_asset"], "kind": "audio",
                               "content_hash": audio_hash,
                               "locator": {"relative": audio.name, "absolute": str(audio)},
                               "streams": [{"index": 0, "codec": "pcm_s16le",
                                            "time_base": rational(Fraction(1, 48000)),
                                            "duration": rational(args.audio_seconds),
                                            "start_time": rational(0), "width": None, "height": None,
                                            "pixel_format": None, "color_primaries": None,
                                            "color_transfer": None, "color_matrix": None,
                                            "color_range": None}]})
    clip = deepcopy(document["sequences"][0]["tracks"][0]["clips"][0])
    clip.update(id=ids["audio_clip"], source_ref={"kind": "asset", "asset": ids["audio_asset"], "stream_index": 0},
                source_in=rational(args.audio_seconds - 1),
                timeline_range={"start": rational(0), "end": rational(1)})
    document["sequences"][0]["tracks"].append({"id": ids["audio_track"], "kind": "audio", "clips": [clip]})
    case["sources"].append({"path": str(audio), "sha256": audio_hash})
    case["submit"]["output"]["audio"] = "document"
    case["submit"]["render"]["input"]["region"]["pixels"] = [640, 360]
    case["submit"]["outputs"] = [{"output": {"format": "flac", "profile_version": 1,
                                               "audio": "document", "clips": []}, "chapters": "omit"}]
    case["additional_output_names"] = ["audio.flac"]
    save("long-source-short-excerpt-mov-flac", case)
    case = deepcopy(case)
    case["submit"]["output"] = {"format": "flac", "profile_version": 1, "audio": "document", "clips": []}
    case["submit"].pop("outputs")
    case.pop("additional_output_names")
    case["output_name"] = "audio.flac"
    case["expected_rendered_frames"] = 0
    save("long-source-short-excerpt-audio-only", case)

    # Cheap CPU rendering exposes the unbounded legacy metadata/receipt cost.
    case = deepcopy(base)
    case["document"]["assets"] = []
    case["document"]["sequences"][0]["tracks"] = []
    case["sources"] = []
    case["submit"]["render"]["input"]["region"]["pixels"] = [2, 2]
    case["submit"]["render"]["frame_rate"] = rational(100)
    case["submit"]["render"]["range"] = {"start": rational(0),
                                             "end": rational(Fraction(args.metadata_frames, 100))}
    case["submit"]["output"] = {"format": "gif", "profile_version": 1, "background": [0, 0, 0]}
    case["output_name"] = "movie.gif"
    case["expected_frames"] = args.metadata_frames
    case["recommended_backend"] = "cpu-reference"
    save("long-metadata-gif", case)
    (output / "manifest.json").write_text(json.dumps({"schema_version": 1, "cases": cases,
                                                       "identities_sha256": digest(ROOT / "examples/perf-export-identities.json")},
                                                      indent=2) + "\n")


if __name__ == "__main__":
    main()
