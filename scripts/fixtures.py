#!/usr/bin/env python3
"""Generate and validate QA-001 fixtures without third-party Python packages."""
import argparse
from fractions import Fraction
import hashlib
import io
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import sys
import wave

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "tests/fixtures/manifest.json"
EXTERNAL_FIXTURE_DIR_ENV = "KRONELLO_FIXTURE_EXTERNAL_DIR"
RATES = ["24/1", "25/1", "30/1", "30000/1001", "60000/1001"]
VFR_INDICES = [0, 1, 3, 6, 10, 15]
# One quantized 1 kHz sine period at 48 kHz; no runtime floating-point synthesis.
SINE = [0,2139,4240,6270,8192,9974,11585,12998,14189,15137,15826,16244,
        16384,16244,15826,15137,14189,12998,11585,9974,8192,6270,4240,2139,
        0,-2139,-4240,-6270,-8192,-9974,-11585,-12998,-14189,-15137,-15826,-16244,
        -16384,-16244,-15826,-15137,-14189,-12998,-11585,-9974,-8192,-6270,-4240,-2139]


def encoded(value):
    return (json.dumps(value, ensure_ascii=False, indent=2) + "\n").encode()


def external_fixture_dir():
    return Path(os.environ[EXTERNAL_FIXTURE_DIR_ENV]) if EXTERNAL_FIXTURE_DIR_ENV in os.environ else ROOT / "target/fixtures/external"


def rational(value):
    return {"num": str(value.numerator), "den": str(value.denominator)}


def parse_rational(value):
    require(isinstance(value, dict) and set(value) == {"num", "den"}, "invalid rational object")
    for field in ("num", "den"):
        require(isinstance(value[field], str) and
                re.fullmatch(r"0|-?[1-9][0-9]*", value[field]) is not None,
                "invalid rational decimal string")
    num, den = int(value["num"]), int(value["den"])
    require(den > 0, "nonpositive rational denominator")
    result = Fraction(num, den)
    require(result.numerator == num and result.denominator == den, "unnormalized rational")
    return result


def validate_timing(value):
    require(value["schema_version"] == 1, "unsupported timing schema")
    require([entry["rate"] for entry in value["cfr"]] == RATES, "wrong timing rates")
    for entry in value["cfr"]:
        require([parse_rational(t) for t in entry["frame_times"]] ==
                [Fraction(i) / Fraction(entry["rate"]) for i in range(6)], "wrong CFR timing")
        require(entry["long_frame_index"] == 864000 and
                parse_rational(entry["long_frame_time"]) == Fraction(864000) / Fraction(entry["rate"]),
                "wrong long-frame timing")
    require(parse_rational(value["vfr"]["time_base"]) == Fraction(1, 30), "wrong VFR time base")
    require([parse_rational(t) for t in value["vfr"]["frame_times"]] ==
            [Fraction(i, 30) for i in VFR_INDICES], "wrong VFR timing")


def canonical_data():
    text = {"schema_version": 1, "normalization": "none", "cases": [
        {"id": "combining", "text": "か\u3099", "codepoints": [0x304B, 0x3099]},
        {"id": "multi-glyph-grapheme", "text": "x\u3099", "codepoints": [0x78, 0x3099]},
        {"id": "ligature", "text": "office"},
        {"id": "ivs", "text": "葛\U000E0100", "codepoints": [0x845B, 0xE0100]},
        {"id": "emoji", "text": "👩\u200d💻", "codepoints": [0x1F469, 0x200D, 0x1F4BB]},
        {"id": "variants", "text": "髙﨑"},
        {"id": "kinsoku", "text": "「日本語」、句読点。"},
        {"id": "ruby", "text": "漢字", "ruby": "かんじ"},
        {"id": "vertical", "text": "縦書き１２３。", "direction": "vertical-rl"},
    ]}
    times = {"schema_version": 1, "cfr": [
        {"rate": rate, "frame_times": [rational(Fraction(i) / Fraction(rate)) for i in range(6)],
         "long_frame_index": 864000, "long_frame_time": rational(Fraction(864000) / Fraction(rate))}
        for rate in RATES],
        "vfr": {"time_base": rational(Fraction(1, 30)), "frame_times": [rational(Fraction(i, 30)) for i in VFR_INDICES]}}
    out = io.BytesIO()
    with wave.open(out, "wb") as wav:
        wav.setnchannels(2); wav.setsampwidth(2); wav.setframerate(48000)
        wav.writeframes(b"".join(struct.pack("<hh", SINE[i % 48], 0 if i < 2400 else SINE[i % 48]) for i in range(4800)))
    pixels = [(0., 0., 0., 0.), (.5, 0., 0., .5), (4., 2., -.125, 1.), (1/65536, 0., 0., 1/65536)]
    return {"japanese.json": encoded(text), "timing.json": encoded(times),
            "alpha.pam": b"P7\nWIDTH 4\nHEIGHT 1\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n" + bytes([255,0,0,0, 255,0,0,128, 0,255,0,255, 0,0,255,1]),
            "linear-hdr.rgba16f": b"".join(struct.pack("<4e", *p) for p in pixels),
            "sine-48k-stereo.wav": out.getvalue()}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def load_manifest():
    manifest = json.loads(MANIFEST.read_text())
    require(manifest["schema_version"] == 1, "unsupported manifest schema")
    ids = [entry["id"] for entry in manifest["fixtures"]]
    require(len(ids) == len(set(ids)), "duplicate fixture IDs")
    paths = [entry["path"] for entry in manifest["fixtures"]]
    require(len(paths) == len(set(paths)), "duplicate fixture paths")
    for entry in manifest["fixtures"]:
        require(entry["storage"] in {"bundled", "generated", "external"}, "unknown storage")
        require(entry["license"] in {"MIT OR Apache-2.0", "OFL-1.1", "CC0-1.0"}, "unapproved license")
        relative = Path(entry["path"])
        require(not relative.is_absolute() and ".." not in relative.parts, "unsafe fixture path")
        require(entry.get("source") and entry.get("purpose"), "missing ledger fields")
        if entry["storage"] != "generated":
            h = entry["sha256"]
            require(len(h) == 64 and all(c in "0123456789abcdef" for c in h), "invalid SHA-256")
            require(entry["bytes"] > 0, "invalid fixture size")
        if entry["storage"] == "external":
            require(entry["url"].startswith("https://"), "HTTPS source required")
            require(entry["revision"] in entry["url"], "source revision not pinned in URL")
    return manifest


def require(condition, message):
    if not condition:
        raise ValueError(message)


def run(command):
    return subprocess.check_output(command, text=True, stderr=subprocess.PIPE)


def media_command(entry, output):
    recipe = entry["recipe"]
    common = ["ffmpeg", "-hide_banner", "-loglevel", "error", "-y"]
    if recipe == "cfr":
        args = ["-f", "lavfi", "-i", f"testsrc2=size=16x16:rate={entry['rate']}", "-frames:v", "6"]
        codec = ["-c:v", "rawvideo", "-pix_fmt", "yuv420p", "-f", "nut"]
    elif recipe == "vfr":
        args = ["-f", "lavfi", "-i", "testsrc2=size=16x16:rate=30", "-vf",
                "select='eq(n,0)+eq(n,1)+eq(n,3)+eq(n,6)+eq(n,10)+eq(n,15)'",
                "-frames:v", "6", "-fps_mode", "vfr"]
        codec = ["-c:v", "rawvideo", "-pix_fmt", "yuv420p", "-f", "nut"]
    elif recipe == "bframes":
        args = ["-f", "lavfi", "-i", "testsrc2=size=16x16:rate=24", "-frames:v", "6"]
        codec = ["-c:v", "mpeg4", "-bf", "2", "-g", "12", "-q:v", "2", "-pix_fmt", "yuv420p", "-f", "nut"]
    elif recipe in {"pq", "hlg"}:
        transfer = "smpte2084" if recipe == "pq" else "arib-std-b67"
        args = ["-f", "lavfi", "-i", "nullsrc=size=16x16:rate=24,format=yuv420p10le,"
                "geq=lum=64+876*X/(W-1):cb=512:cr=512,"
                f"setparams=color_primaries=bt2020:color_trc={transfer}:colorspace=bt2020nc:range=limited",
                "-frames:v", "2"]
        codec = ["-c:v", "ffv1", "-level", "3", "-f", "matroska"]
    else:
        raise ValueError(f"unknown recipe: {recipe}")
    return common + args + ["-threads", "1", "-fflags", "+bitexact", "-flags:v", "+bitexact"] + codec + [str(output)]


def probe_media(entry, path):
    probe = json.loads(run(["ffprobe", "-v", "error", "-show_streams", "-show_frames", "-of", "json", str(path)]))
    require(len(probe["streams"]) == 1, f"{entry['id']}: expected one stream")
    stream = probe["streams"][0]
    frames = probe["frames"]
    require(stream["width"] == stream["height"] == 16, "wrong fixture dimensions")
    recipe = entry["recipe"]
    require(len(frames) == (2 if recipe in {"pq", "hlg"} else 6), "wrong frame count")
    if recipe in {"cfr", "vfr", "bframes"}:
        require(stream["codec_name"] == ("mpeg4" if recipe == "bframes" else "rawvideo") and stream["pix_fmt"] == "yuv420p", "wrong codec/pixel format")
        times = [Fraction(frame["pts"]) * Fraction(stream["time_base"]) for frame in frames]
        expected = [Fraction(i) / Fraction(entry["rate"]) for i in range(6)] if recipe in {"cfr", "bframes"} else [Fraction(i, 30) for i in VFR_INDICES]
        if recipe == "bframes":
            expected = [Fraction(i + 1, 24) for i in range(6)]
        require(times == expected, f"{entry['id']}: timestamps differ: {times}")
        if recipe == "bframes":
            require(any(f.get("pict_type") == "B" for f in frames), "B-frames required")
            require(any(f.get("pkt_dts") != f["pts"] for f in frames), "reordered decode/presentation timestamps required")
        if recipe == "cfr":
            require(Fraction(stream["r_frame_rate"]) == Fraction(entry["rate"]), "wrong frame rate")
    else:
        require(stream["codec_name"] == "ffv1" and stream["pix_fmt"] == "yuv420p10le", "wrong HDR codec/pixel format")
        require(stream.get("color_primaries") == "bt2020" and stream.get("color_space") == "bt2020nc", "missing HDR primaries/matrix")
        require(stream.get("color_transfer") == ("smpte2084" if recipe == "pq" else "arib-std-b67"), "missing HDR transfer")
        require(stream.get("color_range") == "tv", "wrong HDR range")
    # Decode to catch truncated/corrupt payloads; metadata alone is insufficient.
    run(["ffmpeg", "-v", "error", "-xerror", "-i", str(path), "-f", "null", "-"])
    return probe


def generate(output):
    manifest = load_manifest()
    output.mkdir(parents=True, exist_ok=True)
    bundled = output / "data"; bundled.mkdir(exist_ok=True)
    for name, data in canonical_data().items():
        (bundled / name).write_bytes(data)
    media = output / "media"; media.mkdir(exist_ok=True)
    receipt = {"schema_version": 1, "ffmpeg": run(["ffmpeg", "-version"]),
               "ffprobe": run(["ffprobe", "-version"]), "fixtures": []}
    for entry in manifest["fixtures"]:
        if entry["storage"] == "generated":
            path = media / Path(entry["path"]).name
            command = media_command(entry, path)
            run(command)
            probe = probe_media(entry, path)
            data = path.read_bytes()
            receipt["fixtures"].append({"id": entry["id"], "sha256": digest(data), "bytes": len(data), "command": command, "probe": probe})
    (output / "receipt.json").write_bytes(encoded(receipt))
    print(f"generated and decoded {len(receipt['fixtures'])} media fixtures in {output}")


def check(external, generated):
    manifest = load_manifest()
    total = 0
    for entry in manifest["fixtures"]:
        storage = entry["storage"]
        if storage == "generated":
            continue
        path = ROOT / entry["path"] if storage == "bundled" else external / Path(entry["path"]).name
        data = path.read_bytes()  # Missing required fixtures are errors, never skips.
        require(len(data) == entry["bytes"] and digest(data) == entry["sha256"], f"hash/size mismatch: {entry['id']}")
        if storage == "bundled":
            total += len(data)
            require(len(data) <= manifest["limits"]["bundled_file_bytes"], "oversize bundled fixture")
    require(total <= manifest["limits"]["bundled_total_bytes"], "bundled fixture budget exceeded")
    for name, expected in canonical_data().items():
        require((ROOT / "tests/fixtures/data" / name).read_bytes() == expected, f"noncanonical fixture: {name}")
    validate_timing(json.loads((ROOT / "tests/fixtures/data/timing.json").read_text()))
    ledger = (ROOT / "tests/fixtures/LEDGER.md").read_text()
    for entry in manifest["fixtures"]:
        require(f"`{entry['id']}`" in ledger, f"missing ledger entry: {entry['id']}")
    scenes = json.loads((ROOT / "tests/golden/scenes.json").read_text())
    require(scenes["schema_version"] == 1 and scenes["scenes"], "missing scenes")
    scene_ids = set()
    fixture_ids = {entry["id"] for entry in manifest["fixtures"]}
    for scene in scenes["scenes"]:
        require(scene["id"] not in scene_ids, "duplicate scene ID"); scene_ids.add(scene["id"])
        require(set(scene["fixtures"]) <= fixture_ids, "unknown scene fixture")
        require(scene["expected"] and scene["operation"], "missing scene expectation")
        require(scene["descriptor"]["samples_per_frame"] > 0, "invalid scene sample count")
        parse_rational(scene["descriptor"]["time"])
        if scene["operation"] == "linear-interpolation":
            parse_rational(scene["input"]["fraction"])
        elif scene["operation"] == "half-open-interval":
            for value in scene["input"]["interval"] + scene["input"]["times"]:
                parse_rational(value)
    if generated is not None:
        receipt = json.loads((generated / "receipt.json").read_text())
        entries = {item["id"]: item for item in receipt["fixtures"]}
        required = [item for item in manifest["fixtures"] if item["storage"] == "generated"]
        require(len(entries) == len(receipt["fixtures"]) and set(entries) == {item["id"] for item in required}, "invalid receipt coverage")
        for entry in required:
            path = generated / "media" / Path(entry["path"]).name
            data = path.read_bytes(); record = entries[entry["id"]]
            require(digest(data) == record["sha256"] and len(data) == record["bytes"], "generated receipt mismatch")
            probe_media(entry, path)
        for name, expected in canonical_data().items():
            require((generated / "data" / name).read_bytes() == expected, f"regeneration mismatch: {name}")
    print(f"validated {len(manifest['fixtures'])} fixture entries and {len(scene_ids)} scenes; bundled bytes={total}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    g = sub.add_parser("generate"); g.add_argument("--output", type=Path, default=ROOT / "target/fixtures/generated")
    c = sub.add_parser("check"); c.add_argument("--external", type=Path, default=external_fixture_dir())
    c.add_argument("--generated", type=Path)
    args = parser.parse_args()
    try:
        if args.command == "generate": generate(args.output)
        else: check(args.external, args.generated)
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        print(f"fixture failure: {error}", file=sys.stderr)
        if isinstance(error, subprocess.CalledProcessError): print(error.stderr, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
