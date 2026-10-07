#!/usr/bin/env python3
"""Prepare an AUDIO-006 waveform GUI review project; no GUI automation.

Creates a project with a pcm_s16le WAV asset clip whose amplitude changes at the
midpoint, so the timeline waveform bars visibly track loudness. Open the printed
.kronello in the macOS app; the Edit page issues `audio.analyze` automatically and
draws the RMS waveform inside the audio clip.
"""
import argparse
import hashlib
import json
import math
import struct
import subprocess
import wave
from pathlib import Path
from uuid import uuid5, NAMESPACE_URL

ROOT = Path(__file__).resolve().parents[1]
NS = uuid5(NAMESPACE_URL, "kronello-audio006-wave/namespace")


def uid(name):
    return str(uuid5(NS, name))


def rat(n, d=1):
    return {"num": str(n), "den": str(d)}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-directory", type=Path, required=True)
    args = parser.parse_args()
    out = args.output_directory.resolve()
    out.mkdir(parents=True, exist_ok=True)

    audio = out / "wave.wav"
    frames = bytearray()
    for n in range(48000 * 4):
        amp = 12000 if n < 48000 * 2 else 2500
        s = int(amp * math.sin(n * 440 * math.tau / 48000))
        frames += struct.pack("<hh", s, s)
    with wave.open(str(audio), "wb") as wav:
        wav.setnchannels(2)
        wav.setsampwidth(2)
        wav.setframerate(48000)
        wav.writeframes(bytes(frames))

    aid = uid("asset-audio")

    def clip(source, start, end):
        return {
            "id": uid(f"clip-{source['kind']}-{start}"),
            "source_ref": source,
            "timeline_range": {"start": rat(start), "end": rat(end)},
            "source_in": rat(0),
            "time_map": {"kind": "linear", "offset": rat(0), "speed": rat(1)},
            "audio_retime": "reject",
            "properties": [],
            "effects": [],
            "links": [],
        }

    backdrop = clip(
        {
            "kind": "generator",
            "generator": "kronello.solid",
            "version": 1,
            "color": {"space": "srgb", "components": {"r": 0.25, "g": 0.3, "b": 0.35, "alpha": 1.0}},
        },
        0,
        4,
    )
    audio_clip = clip({"kind": "asset", "asset": aid, "stream_index": 0}, 0, 4)

    document = {
        "id": uid("project"),
        "schema_version": 1,
        "semantic_version": 1,
        "name": "AUDIO-006 waveform",
        "compositions": [],
        "curves": [],
        "shapes": [],
        "texts": [],
        "assets": [
            {
                "id": aid,
                "kind": "audio",
                "content_hash": hashlib.sha256(audio.read_bytes()).hexdigest(),
                "locator": {"relative": "wave.wav", "absolute": str(audio)},
                "streams": [
                    {"index": 0, "codec": "pcm_s16le", "time_base": rat(1, 48000), "duration": rat(4)}
                ],
            }
        ],
        "sequences": [
            {
                "id": uid("seq"),
                "extent": {"width": 640.0, "height": 360.0},
                "frame_rate": rat(24),
                "audio_rate": 48000,
                "working_space": "linear_rec709",
                "tracks": [
                    {"id": uid("track-v"), "kind": "video", "clips": [backdrop]},
                    {"id": uid("track-a"), "kind": "audio", "clips": [audio_clip]},
                ],
                "transitions": [],
            }
        ],
    }
    project = out / "wave.kronello"
    binary = ROOT / "apps/macos/Libraries/kronello"
    if not binary.is_file():
        binary = ROOT / "target/debug/kronello"
    result = subprocess.run(
        [str(binary), "--backend", "cpu-reference"],
        input=json.dumps({"operation": "project.create", "project": str(project), "document": document}),
        text=True,
        capture_output=True,
        check=True,
    )
    response = json.loads(result.stdout)
    if response["status"] != "success":
        raise RuntimeError(response)
    print(project)


if __name__ == "__main__":
    main()
