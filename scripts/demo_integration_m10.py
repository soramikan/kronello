#!/usr/bin/env python3
"""Run INTEGRATION-007 (M10) through real CLI/MCP processes over the shared API.

The M10 finishing/expansion demo exercises the milestone's six lanes against
one authored project per transport:

- MEDIA-004: chapter markers transfer into a chapter-capable MOV, a single
  render.submit fans out to DNxHR-MOV/GIF/MP3/FLAC legs, incapable containers
  record typed CHAPTERS_DROPPED warnings, and ffprobe verifies each codec.
- FX-008: versioned grain/mosaic/invert clip effects change pixels through
  render.frame, and audio.gate measurably silences the tone clip before the
  audible audio.delay chain drives the elementary audio legs.
- MEDIA-005: a synthetic LibRaw DNG still decodes real pixels through
  asset.thumbnail; BRAW/R3D fixtures hit the typed vendor-SDK boundary.
- IO-001: io.output.list enumerates the four device kinds; headless
  enable/disable stay typed UNSUPPORTED_FEATURE rejects.
- FLOW-004: capture.start submits a bounded synthetic capture job, stop/status
  observe the registered asset, and capture.deck_probe hits the vendor gate.
- Parity: sequence.query / project.export / render.frame agree between the
  CLI and MCP channels.
"""
import argparse
from fractions import Fraction
import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess
import time
import uuid

from demo_integration_m2 import Demo, ROOT, digest, fraction, rational
from demo_integration_m3 import canonical_bytes
from demo_integration_m7 import Channel
from demo_integration_m9 import parity_document

NS = uuid.UUID("4b10a55e-7c3d-4e2f-9a1b-6d8f0c2e5a41")


def uid(name):
    return str(uuid.uuid5(NS, name))


PROJECT_ID = uid("project")
SEQ = uid("sequence")
TRACK_V, TRACK_A = uid("track.v"), uid("track.a")
CLIP_V, CLIP_TONE = uid("clip.video"), uid("clip.tone")
ASSET_V, ASSET_TONE = uid("asset.video"), uid("asset.tone")
ASSET_DNG, ASSET_BRAW, ASSET_R3D = (
    uid("asset.dng"), uid("asset.braw"), uid("asset.r3d"))
MARKER_A, MARKER_B = uid("marker.intro"), uid("marker.second")
GRAIN_PROPS = [uid(f"grain.{n}") for n in ("amount", "size", "mono", "seed")]
MOSAIC_PROPS = [uid(f"mosaic.{n}") for n in ("size", "basis")]
INVERT_PROP = uid("invert.channel")
TINT_PROPS = [uid(f"tint.{n}") for n in ("black", "white", "amount")]
GATE_PROPS = [uid(f"gate.{n}") for n in
              ("threshold", "attack", "release", "hysteresis")]
DELAY_PROPS = [uid(f"delay.{n}") for n in ("ms", "feedback", "wet", "dry")]
CAPTURE_ASSET = uid("asset.capture")

WORK_AREA = (Fraction(0), Fraction(4))
MOVIE_RATE = 24


def trange(start, end):
    return {"start": rational(start), "end": rational(end)}


def srgb(r, g, b, a=1.0):
    return {"space": "srgb", "components": {"r": r, "g": g, "b": b, "alpha": a}}


def linear_map(speed=1):
    return {"kind": "linear", "offset": rational(0), "speed": rational(speed)}


def clip(cid, start, end, source, effects=None, properties=None,
         audio_retime="reject"):
    return {"id": cid, "source_ref": source,
            "timeline_range": trange(start, end), "source_in": rational(0),
            "time_map": linear_map(), "audio_retime": audio_retime,
            "links": [], "effects": effects or [], "properties": properties or []}


def timeline(kind, **fields):
    return {"timeline": {kind: fields}}


def property_ref(pid, key, kind, value):
    return {"id": pid, "descriptor": {"key": key, "version": 1},
            "source": {"kind": "constant", "value": {"kind": kind, "value": value}},
            "modifiers": []}


def scalar_property(pid, key, value):
    return property_ref(pid, key, "scalar", value)


# --- Synthetic camera-RAW fixtures (ports of kronello_testkit::rawmedia) ---

def _tiff_single_ifd(entries, pixels):
    """Single-IFD little-endian TIFF; values > 4 bytes live after the IFD."""
    by_tag = {tag: (typ, count, value) for tag, typ, count, value in entries}
    records = sorted((tag, typ, count, value)
                     for tag, (typ, count, value) in by_tag.items())
    count = len(records)
    data_base = 8 + 2 + count * 12 + 4
    extra = bytearray()
    body = bytearray()
    for tag, typ, cnt, value in records:
        if tag == 273:  # StripOffsets: patched to the appended pixel strip.
            value = struct.pack("<I", data_base + len(extra))
        elif len(value) > 4:
            value = struct.pack("<I", data_base + len(extra))
            extra += value
            if len(extra) % 2:
                extra += b"\x00"
        else:
            value = value.ljust(4, b"\x00")
        body += struct.pack("<HHI", tag, typ, cnt) + value
    return (b"II" + struct.pack("<H", 42) + struct.pack("<I", 8)
            + struct.pack("<H", count) + bytes(body) + b"\x00\x00\x00\x00"
            + bytes(extra) + pixels)


def dng_bayer16(width, height, seed=0):
    """Deterministic uncompressed 16-bit RGGB Bayer DNG (testkit port)."""
    samples = [((i * 97 + seed * 977 + 512) & 0x3fff)
               for i in range(width * height)]
    pixels = b"".join(struct.pack("<H", s) for s in samples)
    srat = lambda n, d: struct.pack("<iI", n, d)
    rat = lambda n, d: struct.pack("<II", n, d)
    cm = b"".join(srat(1 if i % 4 == 0 else 0, 1) for i in range(9))
    entries = [
        (254, 4, 1, struct.pack("<I", 0)),
        (256, 4, 1, struct.pack("<I", width)),
        (257, 4, 1, struct.pack("<I", height)),
        (258, 3, 1, struct.pack("<H", 16)),
        (259, 3, 1, struct.pack("<H", 1)),
        (262, 3, 1, struct.pack("<H", 32803)),  # CFA
        (271, 2, 9, b"Kronello\x00"),
        (272, 2, 14, b"Synthetic DNG\x00"),
        (273, 4, 1, b""),  # StripOffsets (patched)
        (274, 3, 1, struct.pack("<H", 1)),
        (277, 3, 1, struct.pack("<H", 1)),
        (278, 4, 1, struct.pack("<I", height)),
        (279, 4, 1, struct.pack("<I", width * height * 2)),
        (284, 3, 1, struct.pack("<H", 1)),
        (33421, 3, 2, struct.pack("<HH", 2, 2)),
        (33422, 1, 4, bytes([0, 1, 1, 2])),  # RGGB
        (50706, 1, 4, bytes([1, 4, 0, 0])),  # DNGVersion
        (50707, 1, 4, bytes([1, 1, 0, 0])),
        (50708, 2, 18, b"Kronello Synthetic\x00"),
        (50710, 1, 3, bytes([0, 1, 2])),     # CFAPlaneColor
        (50711, 3, 1, struct.pack("<H", 1)),
        (50714, 4, 1, struct.pack("<I", 0)),       # BlackLevel
        (50717, 3, 1, struct.pack("<H", 16383)),   # WhiteLevel
        (50721, 10, 9, cm),                        # ColorMatrix1
        (50728, 5, 3, rat(1, 1) + rat(1, 1) + rat(1, 1)),  # AsShotNeutral
        (50778, 3, 1, struct.pack("<H", 21)),  # CalibrationIlluminant1 D65
        (50779, 3, 1, struct.pack("<H", 21)),
    ]
    return _tiff_single_ifd(entries, pixels)


def braw_fixture():
    """Minimal BRAW marker: a PK zip local header naming cam.braw."""
    return (b"PK\x03\x04" + bytes(22)
            + struct.pack("<H", 8) + struct.pack("<H", 0) + b"cam.braw")


def r3d_fixture():
    """Minimal R3D marker: the RED2 magic followed by a padded header."""
    return b"RED2" + bytes(508)


def stream_entry(probe):
    num, den = probe["time_base"].split("/")
    return {"index": 0, "codec": probe["codec_name"],
            "time_base": {"num": num, "den": den},
            "duration": rational(Fraction(int(probe["duration_ts"]), int(den)))}


def parity_query(result):
    # A mid-capture stop publishes a partial recording whose byte length —
    # like `parity_document`'s locators — is environmental, not semantic; the
    # deterministic contract is registration and availability.
    q = json.loads(json.dumps(result))
    for entry in q.get("asset_status", []):
        if entry.get("asset") == CAPTURE_ASSET:
            entry["size_bytes"] = None
    return q


def parity_capture(document):
    # Same environmental normalization for the exported document: the partial
    # capture's content hash and probed duration depend on when the stop
    # marker landed.
    doc = parity_document(document)
    for asset in doc.get("assets", []):
        if asset.get("id") == CAPTURE_ASSET:
            asset["content_hash"] = None
            for stream in asset.get("streams", []):
                stream["duration"] = None
    return doc


class M10Channel(Channel):
    """Channel bound to this demo's sequence; `call` avoids the Demo.tool
    `name` collision (io.output.enable takes a request field named `name`)."""

    def __init__(self, demo, transport):
        super().__init__(demo, transport)
        self.path = demo.output / f"m10-integration.{transport}.kronello"

    def call(self, operation, expected_error=None, **payload):
        if self.transport == "cli":
            return self.demo.cli(operation, expected_error, **payload)
        result = self.demo.rpc("tools/call",
                               {"name": operation, "arguments": payload})
        response = result["structuredContent"]
        if json.loads(result["content"][0]["text"]) != response:
            raise RuntimeError("MCP text/structuredContent mismatch")
        if expected_error:
            self.demo.check(f"mcp.{operation}:{expected_error}",
                            result["isError"] and
                            response.get("error", {}).get("code") == expected_error)
            return response["error"]
        if result["isError"]:
            raise RuntimeError(f"MCP {operation}: {response}")
        return response

    def render_input(self):
        return {"project": str(self.path),
                "target": {"kind": "sequence", "sequence": SEQ},
                "region": {"origin": [0, 0], "extent": [320, 180],
                           "pixels": list(self.demo.pixels)},
                "fonts": []}

    def query(self):
        return self.call("sequence.query", project=str(self.path), sequence=SEQ)


class IntegrationM10(Demo):
    def __init__(self, args):
        super().__init__(args)
        self.pixels = [320, 180]
        self.cli_channel = M10Channel(self, "cli")
        self.mcp_channel = M10Channel(self, "mcp")
        self.project = self.cli_channel.path

    def px(self, frame, x, y):
        return frame["linear"][y * self.pixels[0] + x]

    def ffprobe(self, path, *entries):
        args = ["ffprobe", "-v", "error", "-show_streams", "-of", "json",
                str(path)]
        if "-show_chapters" in entries:
            args = ["ffprobe", "-v", "error", "-show_streams",
                    "-show_chapters", "-of", "json", str(path)]
        return json.loads(subprocess.run(
            args, capture_output=True, text=True, env=self.env,
            timeout=60, check=True).stdout)

    def video_source(self, path, *sources):
        inputs = []
        for source in sources:
            inputs += ["-f", "lavfi", "-i", source]
        cmd = ["ffmpeg", "-v", "error", *inputs]
        if len(sources) > 1:
            cmd += ["-filter_complex", "[0:v][1:v]concat=n=2:v=1[out]"]
        cmd += ["-map", "0:v" if len(sources) == 1 else "[out]",
                "-c:v", "mpeg4", "-pix_fmt", "yuv420p",
                "-video_track_timescale", "24", "-an", str(path)]
        subprocess.run(cmd, check=True, env=self.env, capture_output=True)
        probe = json.loads(subprocess.run(
            ["ffprobe", "-v", "error", "-select_streams", "v:0",
             "-show_entries",
             "stream=codec_name,time_base,duration_ts,width,height,pix_fmt",
             "-of", "json", str(path)],
            capture_output=True, text=True, check=True).stdout)["streams"][0]
        entry = stream_entry(probe)
        entry.update(width=probe["width"], height=probe["height"],
                     pixel_format=probe["pix_fmt"])
        return entry

    def media_fixtures(self, media):
        """Deterministic media + camera-RAW fixtures; returns stream metadata."""
        video = media / "source.mov"
        stream_v = self.video_source(video, "testsrc2=size=320x180:rate=24:duration=4")
        tone = media / "tone.mov"
        subprocess.run(["ffmpeg", "-v", "error", "-f", "lavfi", "-i",
                        "sine=frequency=440:sample_rate=48000:duration=4",
                        "-ac", "2", "-c:a", "pcm_s24le", "-vn", str(tone)],
                       check=True, env=self.env, capture_output=True)
        tone_probe = json.loads(subprocess.run(
            ["ffprobe", "-v", "error", "-select_streams", "a:0",
             "-show_entries", "stream=codec_name,time_base,duration_ts",
             "-of", "json", str(tone)],
            capture_output=True, text=True, check=True).stdout)["streams"][0]
        stream_t = stream_entry(tone_probe)
        dng = media / "still.dng"
        dng.write_bytes(dng_bayer16(32, 24))
        braw = media / "clip.braw"
        braw.write_bytes(braw_fixture())
        r3d = media / "clip.r3d"
        r3d.write_bytes(r3d_fixture())
        return {
            ASSET_V: (video, "video", [stream_v]),
            ASSET_TONE: (tone, "audio", [stream_t]),
            ASSET_DNG: (dng, "image", [{
                "index": 0, "codec": "dng",
                "time_base": {"num": "1", "den": "1"},
                "width": 32, "height": 24, "pixel_format": "bayer16",
                "color_primaries": "bt709", "color_transfer": "linear",
                "color_matrix": "gbr", "color_range": "pc"}]),
            ASSET_BRAW: (braw, "video", [{
                "index": 0, "codec": "braw",
                "time_base": {"num": "1", "den": "24"},
                "width": 320, "height": 180}]),
            ASSET_R3D: (r3d, "video", [{
                "index": 0, "codec": "r3d",
                "time_base": {"num": "1", "den": "24"},
                "width": 320, "height": 180}]),
        }

    def build(self, ch):
        tag = ch.transport
        media = self.output / f"{tag}-media"
        media.mkdir()
        assets = self.media_fixtures(media)
        document = {"id": PROJECT_ID, "schema_version": 1, "semantic_version": 1,
                    "name": "INTEGRATION-007 M10 仕上げと拡張",
                    "compositions": [], "curves": [],
                    "assets": [
                        {"id": asset_id, "kind": kind,
                         "content_hash": hashlib.sha256(
                             path.read_bytes()).hexdigest(),
                         "locator": {"relative": path.name,
                                     "absolute": str(path)},
                         "streams": streams}
                        for asset_id, (path, kind, streams) in assets.items()]}
        created = ch.call("project.create", project=str(ch.path),
                          document=document)
        ch.revision = str(created["revision"])
        self.check(f"{tag}.project.create",
                   created["project_id"] == PROJECT_ID)
        ch.edit("sequence.create", sequence={
            "id": SEQ, "extent": {"width": 320, "height": 180},
            "frame_rate": rational(MOVIE_RATE), "audio_rate": 48000,
            "working_space": "linear_rec709",
            "tracks": [{"id": TRACK_V, "kind": "video", "clips": []},
                       {"id": TRACK_A, "kind": "audio", "clips": []}],
            "markers": []})
        ch.edit("clip.place", sequence=SEQ, track=TRACK_V,
                clip=clip(CLIP_V, 0, 4, {"kind": "asset", "asset": ASSET_V,
                                         "stream_index": 0}))
        ch.edit("clip.place", sequence=SEQ, track=TRACK_A,
                clip=clip(CLIP_TONE, 0, 4,
                          {"kind": "asset", "asset": ASSET_TONE,
                           "stream_index": 0}))
        # MEDIA-004: chapter-role markers become container chapters; the
        # standard marker and comment never transfer.
        ch.apply(timeline("marker_set", sequence=SEQ, marker={
                          "id": MARKER_A, "time": rational(0),
                          "color": "green", "role": "chapter",
                          "title": "Intro"}),
                 timeline("marker_set", sequence=SEQ, marker={
                          "id": MARKER_B, "time": rational(2),
                          "color": "red", "role": "chapter",
                          "title": "後半"}),
                 timeline("marker_set", sequence=SEQ, marker={
                          "id": uid("marker.note"), "time": rational(1),
                          "color": "blue", "comment": "annotation only"}))
        self.effects(ch)
        self.delivery(ch)
        self.io_and_capture(ch)
        self.raw_boundary(ch)
        # The capture worker registered its asset via a shared edit; pull the
        # final revision before later mutations.
        info = ch.call("project.info", project=str(ch.path))
        ch.revision = str(info["revision"])

    def effects(self, ch):
        tag = ch.transport
        baseline = ch.frame(1)
        # FX-008: versioned grain + mosaic + invert on the video clip; the
        # deterministic seed makes pixels repeatable while visibly changing.
        ch.apply(timeline("clip_set_effects", sequence=SEQ, clip=CLIP_V,
                          properties=[
                              scalar_property(GRAIN_PROPS[0],
                                              "kronello.effect.grain_amount",
                                              0.5),
                              scalar_property(GRAIN_PROPS[1],
                                              "kronello.effect.grain_size",
                                              2.0),
                              property_ref(GRAIN_PROPS[2],
                                           "kronello.effect.monochrome",
                                           "bool", True),
                              scalar_property(GRAIN_PROPS[3],
                                              "kronello.effect.seed", 7.0),
                              scalar_property(MOSAIC_PROPS[0],
                                              "kronello.effect.block_size",
                                              8.0),
                              property_ref(MOSAIC_PROPS[1],
                                           "kronello.effect.mosaic_basis",
                                           "enum", "center"),
                              property_ref(INVERT_PROP,
                                           "kronello.effect.invert_channel",
                                           "enum", "rgb")],
                          effects=[
                              {"effect_id": "kronello.grain", "version": 1,
                               "parameters": {"kind": "grain",
                                              "amount": GRAIN_PROPS[0],
                                              "size": GRAIN_PROPS[1],
                                              "monochrome": GRAIN_PROPS[2],
                                              "seed": GRAIN_PROPS[3]}},
                              {"effect_id": "kronello.mosaic", "version": 1,
                               "parameters": {"kind": "mosaic",
                                              "block_size": MOSAIC_PROPS[0],
                                              "basis": MOSAIC_PROPS[1]}},
                              {"effect_id": "kronello.invert", "version": 1,
                               "parameters": {"kind": "invert",
                                              "channel": INVERT_PROP}}]))
        effected = ch.frame(1)
        again = ch.frame(1)
        self.check(f"{tag}.fx008.pixels",
                   effected["linear"] != baseline["linear"],
                   baseline=self.px(baseline, 160, 90),
                   effected=self.px(effected, 160, 90))
        self.check(f"{tag}.fx008.deterministic",
                   effected["linear"] == again["linear"])
        # The delivery job below re-encodes through the strict SDR path, which
        # rejects out-of-gamut samples — grain deliberately perturbs beyond
        # [0,1], so the export stack carries the in-gamut FX-008 effects.
        ch.apply(timeline("clip_set_effects", sequence=SEQ, clip=CLIP_V,
                          properties=[
                              scalar_property(MOSAIC_PROPS[0],
                                              "kronello.effect.block_size",
                                              8.0),
                              property_ref(MOSAIC_PROPS[1],
                                           "kronello.effect.mosaic_basis",
                                           "enum", "center"),
                              property_ref(INVERT_PROP,
                                           "kronello.effect.invert_channel",
                                           "enum", "rgb"),
                              property_ref(TINT_PROPS[0],
                                           "kronello.effect.map_black",
                                           "color", srgb(0.0, 0.0, 0.2)),
                              property_ref(TINT_PROPS[1],
                                           "kronello.effect.map_white",
                                           "color", srgb(1.0, 0.9, 0.8)),
                              scalar_property(TINT_PROPS[2],
                                              "kronello.effect.tint_amount",
                                              0.25)],
                          effects=[
                              {"effect_id": "kronello.mosaic", "version": 1,
                               "parameters": {"kind": "mosaic",
                                              "block_size": MOSAIC_PROPS[0],
                                              "basis": MOSAIC_PROPS[1]}},
                              {"effect_id": "kronello.invert", "version": 1,
                               "parameters": {"kind": "invert",
                                              "channel": INVERT_PROP}},
                              {"effect_id": "kronello.tint", "version": 1,
                               "parameters": {"kind": "tint",
                                              "map_black": TINT_PROPS[0],
                                              "map_white": TINT_PROPS[1],
                                              "amount": TINT_PROPS[2]}}]))
        # FX-008 audio: a closed gate silences the tone measurably, then the
        # audible delay chain drives the elementary audio legs below.
        measured = ch.call("audio.loudness", project=str(ch.path),
                           base_revision=ch.revision,
                           input={"kind": "clip", "sequence": SEQ,
                                  "clip": CLIP_TONE})
        self.check(f"{tag}.audio.baseline",
                   measured.get("integrated_lufs") is not None,
                   lufs=measured.get("integrated_lufs"))
        ch.apply(timeline("clip_set_effects", sequence=SEQ, clip=CLIP_TONE,
                          properties=[
                              scalar_property(GATE_PROPS[0],
                                              "kronello.effect.threshold_db",
                                              0.0),
                              scalar_property(GATE_PROPS[1],
                                              "kronello.effect.attack_ms", 1.0),
                              scalar_property(GATE_PROPS[2],
                                              "kronello.effect.release_ms",
                                              50.0),
                              scalar_property(GATE_PROPS[3],
                                              "kronello.effect.hysteresis_db",
                                              6.0)],
                          effects=[{"effect_id": "kronello.audio.gate",
                                    "version": 1,
                                    "parameters": {
                                        "kind": "audio_gate",
                                        "threshold_db": GATE_PROPS[0],
                                        "attack_ms": GATE_PROPS[1],
                                        "release_ms": GATE_PROPS[2],
                                        "hysteresis_db": GATE_PROPS[3]}}]))
        gated = ch.call("audio.loudness", project=str(ch.path),
                        base_revision=ch.revision,
                        input={"kind": "clip", "sequence": SEQ,
                               "clip": CLIP_TONE})
        gated_lufs = gated.get("integrated_lufs")
        self.check(f"{tag}.fx008.gate",
                   gated["frames"] > 0 and (gated_lufs is None or
                   gated_lufs < measured["integrated_lufs"] - 20),
                   baseline_lufs=measured["integrated_lufs"],
                   gated_lufs=gated_lufs)
        ch.apply(timeline("clip_set_effects", sequence=SEQ, clip=CLIP_TONE,
                          properties=[
                              scalar_property(DELAY_PROPS[0],
                                              "kronello.effect.delay_ms",
                                              50.0),
                              scalar_property(DELAY_PROPS[1],
                                              "kronello.effect.feedback_db",
                                              -12.0),
                              scalar_property(DELAY_PROPS[2],
                                              "kronello.effect.wet", 0.3),
                              scalar_property(DELAY_PROPS[3],
                                              "kronello.effect.dry", 1.0)],
                          effects=[{"effect_id": "kronello.audio.delay",
                                    "version": 1,
                                    "parameters": {
                                        "kind": "audio_delay",
                                        "delay_ms": DELAY_PROPS[0],
                                        "feedback_db": DELAY_PROPS[1],
                                        "wet": DELAY_PROPS[2],
                                        "dry": DELAY_PROPS[3]}}]))

    def delivery(self, ch):
        tag = ch.transport
        out = self.output / f"{tag}-delivery"
        out.mkdir()
        movie = out / "movie.mov"
        submitted = ch.call("render.submit",
                            render={"input": ch.render_input(),
                                    "range": trange(*WORK_AREA),
                                    "frame_rate": rational(MOVIE_RATE),
                                    "output_directory": str(movie)},
                            output={"format": "pro_res_mov",
                                    "profile_version": 3, "audio": "document",
                                    "clips": [], "background": [0, 0, 0]},
                            chapters="transfer",
                            outputs=[
                                {"destination": str(out / "movie-dnx.mov"),
                                 "output": {"format": "dnx_mov",
                                            "profile_version": 1,
                                            "dnx_profile": "dnxhr_hq",
                                            "audio": "document", "clips": [],
                                            "background": [0, 0, 0]}},
                                {"destination": str(out / "movie.gif"),
                                 "output": {"format": "gif",
                                            "profile_version": 1,
                                            "background": [0, 0, 0]}},
                                {"destination": str(out / "sound.mp3"),
                                 "output": {"format": "mp3",
                                            "profile_version": 1,
                                            "audio": "document", "clips": []}},
                                {"destination": str(out / "sound.flac"),
                                 "output": {"format": "flac",
                                            "profile_version": 1,
                                            "audio": "document",
                                            "clips": []}}])
        job = ch.wait_job(submitted)
        self.check(f"{tag}.delivery.job", job["status"] == "succeeded",
                   error=job.get("error"))
        probe = self.ffprobe(movie, "-show_chapters")
        streams = {s["codec_type"]: s for s in probe["streams"]}
        titles = [c.get("tags", {}).get("title") for c in
                  probe.get("chapters", [])]
        self.check(f"{tag}.delivery.chapters",
                   streams.get("video", {}).get("codec_name") == "prores" and
                   titles == ["Intro", "後半"],
                   titles=titles,
                   streams=[s.get("codec_name") for s in probe["streams"]])
        legs = {
            "movie-dnx.mov": ("dnxhd", "video"),
            "movie.gif": ("gif", "video"),
            "sound.mp3": ("mp3", "audio"),
            "sound.flac": ("flac", "audio"),
        }
        for name, (codec, kind) in legs.items():
            leg_probe = self.ffprobe(out / name)
            found = {s["codec_type"]: s.get("codec_name")
                     for s in leg_probe["streams"]}
            self.check(f"{tag}.delivery.{name}",
                       found.get(kind) == codec, streams=found)
        result = job.get("result") or {}
        reports = (result.get("report") or {}).get("outputs") or []
        dropped = {Path(r["output"]).name
                   for r in reports
                   for w in r.get("warnings", [])
                   if w.get("code") == "CHAPTERS_DROPPED"}
        self.check(f"{tag}.delivery.chapters_dropped",
                   {"movie.gif", "sound.mp3", "sound.flac"} <= dropped,
                   dropped=sorted(dropped))

    def io_and_capture(self, ch):
        tag = ch.transport
        # IO-001: detection is honest and separate from activation; headless
        # enable/disable are typed UNSUPPORTED_FEATURE rejects (ADR-0134).
        outputs = ch.call("io.output.list")
        kinds = {d.get("kind") for d in outputs.get("devices", [])}
        self.check(f"{tag}.io.output.list",
                   {"ref_monitor", "syphon", "sdi", "ndi"} <= kinds,
                   kinds=sorted(kinds))
        for verb in ("enable", "disable"):
            ch.call(f"io.output.{verb}", expected_error="UNSUPPORTED_FEATURE",
                    kind="syphon")
        # FLOW-004: a bounded synthetic capture records, stops and publishes
        # the registered asset; deck ingest stays the typed vendor boundary.
        # max_frames bounds the session while job.get progress gates the stop
        # marker so the worker has at least one recorded frame.
        capture = ch.call("capture.start", project=str(ch.path),
                          source={"kind": "synthetic"},
                          format={"width": 16, "height": 8,
                                  "frame_rate": {"num": "24", "den": "1"},
                                  "codec": "pro_res", "color": "bt709"},
                          asset=CAPTURE_ASSET,
                          idempotency_key=ch.key("capture"),
                          max_frames=96)
        self.check(f"{tag}.capture.submitted", capture["id"] is not None,
                   job=capture["id"])
        deadline = time.monotonic() + 120
        while True:
            progress = ch.call("job.get", job=capture["id"])
            if progress["status"] in ("succeeded", "failed", "cancelled"):
                break
            if progress.get("completed_frames", 0) > 0:
                break
            if time.monotonic() >= deadline:
                raise RuntimeError(f"capture produced no frames: {progress}")
            time.sleep(0.25)
        stopped = ch.call("capture.stop", project=str(ch.path),
                          job=capture["id"])
        self.check(f"{tag}.capture.stop", stopped["id"] == capture["id"])
        job = ch.wait_job(capture)
        self.check(f"{tag}.capture.job", job["status"] == "succeeded",
                   error=job.get("error"))
        status = ch.call("capture.status", project=str(ch.path))
        sessions = status.get("sessions", [])
        entry = next((s for s in sessions if s["job"]["id"] == capture["id"]),
                     None)
        self.check(f"{tag}.capture.status",
                   entry is not None and entry.get("asset_registered") is True,
                   session=entry)
        ch.call("capture.deck_probe", expected_error="UNSUPPORTED_FEATURE")

    def raw_boundary(self, ch):
        tag = ch.transport
        # MEDIA-005: the LibRaw still path decodes real pixels; proprietary
        # camera formats stay typed vendor-SDK rejects (ADR-0136).
        thumb = ch.call("asset.thumbnail", project=str(ch.path),
                        asset=ASSET_DNG, max_size=64)
        self.check(f"{tag}.raw.dng",
                   thumb["width"] > 0 and
                   len(thumb["rgba"]) == thumb["width"] * thumb["height"] * 4,
                   width=thumb.get("width"), height=thumb.get("height"))
        for asset in (ASSET_BRAW, ASSET_R3D):
            ch.call("asset.thumbnail", expected_error="UNSUPPORTED_FEATURE",
                    project=str(ch.path), asset=asset, max_size=64)

    def run(self):
        self.fonts = []
        self.compositions = [{"id": uid("missing-composition")}]
        self.start_mcp()
        capabilities = self.tool("capabilities.get")
        self.check("capabilities.features",
                   {"clip_effects", "movie_delivery_v1"} <=
                   set(capabilities["features"]))
        for ch in (self.cli_channel, self.mcp_channel):
            self.build(ch)
        cli_q = self.cli_channel.query()
        mcp_q = self.mcp_channel.query()
        self.check("parity.sequence.query",
                   canonical_bytes(parity_query(cli_q)) ==
                   canonical_bytes(parity_query(mcp_q)),
                   cli_sha256=digest(canonical_bytes(parity_query(cli_q))))
        cli_doc = self.cli_channel.call("project.export",
                                        project=str(self.cli_channel.path))
        mcp_doc = self.mcp_channel.call("project.export",
                                        project=str(self.mcp_channel.path))
        self.check("parity.project.export",
                   cli_doc["revision"] == mcp_doc["revision"] and
                   canonical_bytes(parity_capture(cli_doc["document"])) ==
                   canonical_bytes(parity_capture(mcp_doc["document"])),
                   revision=cli_doc["revision"])
        for t in (Fraction(1), Fraction(3)):
            self.check(f"parity.frame.{t}",
                       self.cli_channel.frame(t)["linear"] ==
                       self.mcp_channel.frame(t)["linear"])
        manifest = {
            "schema_version": 1,
            "projects": {"cli": str(self.cli_channel.path),
                         "mcp": str(self.mcp_channel.path)},
            "revision": self.cli_channel.revision,
            "project_id": PROJECT_ID, "sequence": SEQ,
            "tracks": {"video": TRACK_V, "audio": TRACK_A},
            "clips": {"video": CLIP_V, "tone": CLIP_TONE},
            "assets": {"video": ASSET_V, "tone": ASSET_TONE,
                       "dng": ASSET_DNG, "braw": ASSET_BRAW,
                       "r3d": ASSET_R3D, "capture": CAPTURE_ASSET},
            "work_area": [0, 4],
            "render_backend": self.args.backend,
            "pixels": self.pixels,
        }
        (self.output / "gui-evidence.json").write_bytes(canonical_bytes(manifest))
        (self.output / "project.export.json").write_bytes(canonical_bytes(cli_doc))
        self.report.update(status="verified",
                           gui_evidence=str(self.output / "gui-evidence.json"))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-directory", required=True, type=Path)
    parser.add_argument("--binary-dir", type=Path,
                        default=Path(os.environ.get("CARGO_TARGET_DIR",
                                                    ROOT / "target")) / "debug")
    parser.add_argument("--backend", choices=("gpu", "cpu-reference"),
                        default="gpu")
    parser.add_argument("--resolution", choices=("4k", "small"), default="small")
    parser.add_argument("--state-root", type=Path)
    args = parser.parse_args()
    demo = IntegrationM10(args)
    try:
        demo.run()
    except Exception as error:
        demo.report.update(status="failed", error=str(error))
        raise
    finally:
        demo.finish()
    print(json.dumps({"status": demo.report["status"],
                      "checks": len(demo.report["checks"])}))


if __name__ == "__main__":
    main()
