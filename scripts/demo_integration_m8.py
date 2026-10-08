#!/usr/bin/env python3
"""Run INTEGRATION-005 through real CLI/MCP processes, without private storage access.

The M8 finish-quality project exercises retime+freeze (NLE-006), masks
(FX-004), adjustment clips (FX-007), LUTs (COLOR-003), audio chains with
loudness normalization (AUDIO-007/008), proxy switching (PROXY-001) and
tracking data (TRACK-001) over both shared-API transports.
"""
import argparse
from fractions import Fraction
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time
import uuid

from demo_integration_m2 import Demo, ROOT, digest, fraction, rational
from demo_integration_m3 import canonical_bytes
from demo_integration_m7 import Channel

NS = uuid.UUID("8f3a9c21-4e7b-4d5f-9a6c-2b8e0f1a3d4e")


def uid(name):
    return str(uuid.uuid5(NS, name))


PROJECT_ID = uid("project")
SEQ = uid("sequence")
TRACK_V1, TRACK_V2, TRACK_V3, TRACK_V4, TRACK_A = (
    uid(f"track.{n}") for n in ("v1", "v2", "v3", "v4", "audio"))
CLIP_V1, CLIP_V2, CLIP_M, CLIP_ADJ, CLIP_LUT, CLIP_TONE = (
    uid(f"clip.{n}") for n in ("v1", "v2", "masked", "adjust", "lut", "tone"))
MASK_ID = uid("mask")
MASK_PROPS = [uid(f"mask.prop.{n}") for n in ("path", "feather", "expansion", "opacity")]
LUT_ASSET, TRACKING_ASSET, VIDEO_ASSET = (
    uid(f"asset.{n}") for n in ("lut", "tracking", "video"))
EXPOSURE_ID = uid("prop.exposure")
EXPOSURE_OFFSET_ID = uid("prop.exposure_offset")
LUT_PROP_ID, LUT_INTENSITY_ID = uid("prop.lut"), uid("prop.lut_intensity")
EQ_BANDS_ID = uid("prop.eq_bands")
HPF_CUTOFF_ID, HPF_ORDER_ID = uid("prop.hpf_cutoff"), uid("prop.hpf_order")
CEILING_ID, RELEASE_ID = uid("prop.ceiling"), uid("prop.release")

WORK_AREA = (Fraction(1), Fraction(7))
JOB_RATE, MOVIE_RATE = 4, 24
BLUE = (0.10, 0.25, 0.85)
MAGENTA = (0.85, 0.10, 0.85)


def trange(start, end):
    return {"start": rational(start), "end": rational(end)}


def srgb(r, g, b, a=1.0):
    return {"space": "srgb", "components": {"r": r, "g": g, "b": b, "alpha": a}}


def linear_map():
    return {"kind": "linear", "offset": rational(0), "speed": rational(1)}


def clip(cid, start, end, source, effects=None, properties=None):
    return {"id": cid, "source_ref": source,
            "timeline_range": trange(start, end), "source_in": rational(0),
            "time_map": linear_map(), "audio_retime": "reject", "links": [],
            "effects": effects or [], "properties": properties or []}


def timeline(kind, **fields):
    return {"timeline": {kind: fields}}


def scalar_property(pid, key, value):
    return {"id": pid, "descriptor": {"key": key, "version": 1},
            "source": {"kind": "constant", "value": {"kind": "scalar", "value": value}},
            "modifiers": []}


def property_ref(pid, key, kind, value):
    return {"id": pid, "descriptor": {"key": key, "version": 1},
            "source": {"kind": "constant", "value": {"kind": kind, "value": value}},
            "modifiers": []}


def invert_cube():
    text = 'TITLE "invert"\nLUT_3D_SIZE 2\n'
    for b in (0, 1):
        for g in (0, 1):
            for r in (0, 1):
                text += f"{1 - r} {1 - g} {1 - b}\n"
    return text


def parity_document(document):
    # The two channels build sibling projects whose semantics must be
    # identical while their on-disk media locators and job records differ by
    # construction; parity compares content with locators reduced to file
    # names and worker job ids dropped.
    doc = json.loads(json.dumps(document))
    for asset in doc.get("assets", []):
        locator = asset.get("locator") or {}
        if locator.get("relative"):
            locator["relative"] = Path(locator["relative"]).name
        if locator.get("absolute"):
            locator["absolute"] = Path(locator["absolute"]).name
    for link in doc.get("proxies", []):
        link["job"] = None
    return doc


class M8Channel(Channel):
    """Channel bound to this demo's sequence; render inputs carry LUTs."""

    def __init__(self, demo, transport):
        super().__init__(demo, transport)
        self.path = demo.output / f"m8-integration.{transport}.kronello"

    def render_input(self):
        return {"project": str(self.path),
                "target": {"kind": "sequence", "sequence": SEQ},
                "region": {"origin": [0, 0], "extent": [320, 180],
                           "pixels": list(self.demo.pixels)},
                "fonts": [], "luts": self.luts}

    def query(self):
        return self.call("sequence.query", project=str(self.path), sequence=SEQ)


class IntegrationM8(Demo):
    def __init__(self, args):
        super().__init__(args)
        self.pixels = [320, 180]
        self.cli_channel = M8Channel(self, "cli")
        self.mcp_channel = M8Channel(self, "mcp")
        self.project = self.cli_channel.path

    def px(self, frame, x, y):
        return frame["linear"][y * self.pixels[0] + x]

    def probe(self, frame, points=((280, 90), (100, 60), (200, 120))):
        # testsrc2 keeps parts of the frame static across neighbouring
        # frames; a small point set stays stable under hold maps while still
        # distinguishing two different source frames.
        return [self.px(frame, x, y) for x, y in points]

    def video_source(self, path):
        subprocess.run(
            ["ffmpeg", "-v", "error", "-f", "lavfi",
             "-i", "testsrc2=size=320x180:rate=24:duration=6",
             "-c:v", "mpeg4", "-pix_fmt", "yuv420p", "-an", str(path)],
            check=True)
        # Author the exact muxed stream metadata; proxy validation compares
        # durations rationals, not seconds.
        probe = json.loads(subprocess.run(
            ["ffprobe", "-v", "error", "-select_streams", "v:0",
             "-show_entries", "stream=codec_name,time_base,duration_ts,width,height,pix_fmt",
             "-of", "json", str(path)],
            capture_output=True, text=True, check=True).stdout)["streams"][0]
        num, den = probe["time_base"].split("/")
        duration = Fraction(int(probe["duration_ts"]), int(den))
        return {"index": 0, "codec": probe["codec_name"],
                "time_base": {"num": num, "den": den},
                "duration": rational(duration),
                "width": probe["width"], "height": probe["height"],
                "pixel_format": probe["pix_fmt"]}

    def build(self, ch):
        tag = ch.transport
        ch.luts = []
        # Real video fixture beside the project; deterministic test pattern.
        media = self.output / f"{tag}-media"
        media.mkdir()
        video = media / "source.mov"
        stream = self.video_source(video)
        document = {"id": PROJECT_ID, "schema_version": 1, "semantic_version": 1,
                    "name": "INTEGRATION-005 M8 仕上げ", "compositions": [], "curves": [],
                    "assets": [{
                        "id": VIDEO_ASSET, "kind": "video",
                        "content_hash": hashlib.sha256(video.read_bytes()).hexdigest(),
                        "locator": {"relative": video.name, "absolute": str(video)},
                        "streams": [stream]}]}
        created = ch.call("project.create", project=str(ch.path), document=document)
        ch.revision = str(created["revision"])
        self.check(f"{tag}.project.create", created["project_id"] == PROJECT_ID)
        ch.edit("sequence.create", sequence={
            "id": SEQ, "extent": {"width": 320, "height": 180},
            "frame_rate": rational(24), "audio_rate": 48000,
            "working_space": "linear_rec709",
            "tracks": [{"id": TRACK_V1, "kind": "video", "clips": []},
                       {"id": TRACK_V2, "kind": "video", "clips": []},
                       {"id": TRACK_V3, "kind": "video", "clips": []},
                       {"id": TRACK_V4, "kind": "video", "clips": []},
                       {"id": TRACK_A, "kind": "audio", "clips": []}]})
        # V1: real media for retime/freeze; V2: masked solid; V3: adjustment;
        # V4: LUT clip; A1: processed tone.
        ch.edit("clip.place", sequence=SEQ, track=TRACK_V1,
                clip=clip(CLIP_V1, 0, 6, {"kind": "asset", "asset": VIDEO_ASSET,
                                          "stream_index": 0}))
        ch.edit("clip.place", sequence=SEQ, track=TRACK_V1,
                clip=clip(CLIP_V2, 6, 12, {"kind": "asset", "asset": VIDEO_ASSET,
                                           "stream_index": 0}))
        ch.edit("clip.place", sequence=SEQ, track=TRACK_V2,
                clip=clip(CLIP_M, 0, 12, {"kind": "generator",
                                          "generator": "kronello.solid", "version": 1,
                                          "color": srgb(*BLUE)}))
        ch.edit("clip.place", sequence=SEQ, track=TRACK_A,
                clip=clip(CLIP_TONE, 0, 12, {"kind": "generator",
                                             "generator": "kronello.audio.tone440",
                                             "version": 1, "color": srgb(0, 0, 0)}))
        # FX-007: an adjustment clip applies its authored effects to the lower
        # composite; audio tracks reject it with INVALID_CLIP.
        ch.edit("clip.place", sequence=SEQ, track=TRACK_V3,
                clip=clip(CLIP_ADJ, 6, 12, {"kind": "adjustment"},
                          effects=[{"effect_id": "kronello.color.exposure", "version": 1,
                                    "parameters": {"kind": "color_exposure",
                                                   "exposure": EXPOSURE_ID,
                                                   "offset": EXPOSURE_OFFSET_ID}}],
                          properties=[scalar_property(EXPOSURE_ID, "kronello.effect.exposure", -1.0),
                                      scalar_property(EXPOSURE_OFFSET_ID,
                                                      "kronello.effect.exposure_offset", 0.0)]))
        ch.call("clip.place", expected_error="INVALID_CLIP",
                project=str(ch.path), base_revision=ch.revision,
                session_id=ch.session, idempotency_key=ch.key("adj-reject"),
                sequence=SEQ, track=TRACK_A,
                clip=clip(uid("clip.adj.audio"), 14, 16, {"kind": "adjustment"}))
        # NLE-005: lock V1, prove TRACK_LOCKED, unlock, then target tracks.
        ch.apply(timeline("track_state_set", sequence=SEQ, track=TRACK_V1,
                          state={"visible": True, "muted": False, "locked": True}))
        ch.call("clip.place", expected_error="TRACK_LOCKED",
                project=str(ch.path), base_revision=ch.revision,
                session_id=ch.session, idempotency_key=ch.key("locked-reject"),
                sequence=SEQ, track=TRACK_V1,
                clip=clip(uid("clip.locked"), 12, 13, {"kind": "generator",
                                                      "generator": "kronello.solid",
                                                      "version": 1, "color": srgb(1, 1, 1)}))
        ch.apply(timeline("track_state_set", sequence=SEQ, track=TRACK_V1,
                          state={"visible": True, "muted": False, "locked": False}))
        ch.apply(timeline("sequence_targets_set", sequence=SEQ,
                          targets={"video": TRACK_V1, "audio": TRACK_A}))
        # NLE-006: freeze V1's clip at 3s (right part holds the source frame)
        # and retime the second clip with a ramp-hold-ramp piecewise map.
        ch.apply(timeline("clip_freeze", sequence=SEQ, clip=CLIP_V1, at=rational(3)))
        ch.apply(timeline("clip_time_set", sequence=SEQ, clip=CLIP_V2,
                          source_in=rational(0),
                          time_map={"kind": "piecewise_linear", "points": [
                              {"parent": rational(0), "local": rational(0)},
                              {"parent": rational(2), "local": rational(1)},
                              {"parent": rational(4), "local": rational(1)},
                              {"parent": rational(6), "local": rational(3)}]},
                          audio_retime="reject"))
        # FX-004: a left-half mask on the blue clip.
        path_value = {"segments": [
            {"kind": "move_to", "value": [0.0, 0.0]},
            {"kind": "line_to", "value": [160.0, 0.0]},
            {"kind": "line_to", "value": [160.0, 180.0]},
            {"kind": "line_to", "value": [0.0, 180.0]},
            {"kind": "close"}]}
        ch.apply(timeline("clip_masks_set", sequence=SEQ, clip=CLIP_M, masks=[{
            "id": MASK_ID, "path": MASK_PROPS[0], "mode": "add",
            "feather": MASK_PROPS[1], "expansion": MASK_PROPS[2],
            "opacity": MASK_PROPS[3], "invert": False, "closed": True}],
            properties=[
                property_ref(MASK_PROPS[0], "kronello.mask.path", "path", path_value),
                property_ref(MASK_PROPS[1], "kronello.mask.feather", "scalar", 0.0),
                property_ref(MASK_PROPS[2], "kronello.mask.expansion", "scalar", 0.0),
                property_ref(MASK_PROPS[3], "kronello.mask.opacity", "scalar", 1.0)]))
        # COLOR-003: import an inverting lattice, attach it to the magenta clip.
        cube = media / "invert.cube"
        cube.write_text(invert_cube())
        ch.edit("lut.import", path=str(cube), asset=LUT_ASSET)
        ch.luts = [{"hash": hashlib.sha256(cube.read_bytes()).hexdigest(),
                    "path": str(cube)}]
        ch.edit("clip.place", sequence=SEQ, track=TRACK_V4,
                clip=clip(CLIP_LUT, 10, 12, {"kind": "generator",
                                            "generator": "kronello.solid", "version": 1,
                                            "color": srgb(*MAGENTA)},
                          effects=[{"effect_id": "kronello.color.lut", "version": 1,
                                    "parameters": {"kind": "color_lut", "lut": LUT_PROP_ID,
                                                   "intensity": LUT_INTENSITY_ID}}],
                          properties=[
                              property_ref(LUT_PROP_ID, "kronello.effect.lut",
                                           "asset_ref", LUT_ASSET),
                              property_ref(LUT_INTENSITY_ID, "kronello.effect.intensity",
                                           "scalar", 1.0)]))
        # AUDIO-007/008: EQ + high-pass + limiter on the tone clip.
        eq_table = {"columns": {"kind": "enum", "freq_hz": "scalar",
                                "gain_db": "scalar", "q": "scalar"},
                    "rows": [{"kind": {"kind": "enum", "value": "peak"},
                              "freq_hz": {"kind": "scalar", "value": 1000.0},
                              "gain_db": {"kind": "scalar", "value": -6.0},
                              "q": {"kind": "scalar", "value": 1.0}}]}
        ch.apply(timeline("clip_set_effects", sequence=SEQ, clip=CLIP_TONE,
                          properties=[
                              property_ref(EQ_BANDS_ID, "kronello.effect.eq_bands",
                                           "data_table", eq_table),
                              scalar_property(HPF_CUTOFF_ID, "kronello.effect.cutoff_hz", 200.0),
                              scalar_property(HPF_ORDER_ID, "kronello.effect.order", 2.0),
                              scalar_property(CEILING_ID, "kronello.effect.ceiling_db", -1.0),
                              scalar_property(RELEASE_ID, "kronello.effect.release_ms", 50.0)],
                          effects=[
                              {"effect_id": "kronello.audio.eq", "version": 1,
                               "parameters": {"kind": "audio_eq", "bands": EQ_BANDS_ID}},
                              {"effect_id": "kronello.audio.hpf", "version": 1,
                               "parameters": {"kind": "audio_hpf",
                                              "cutoff_hz": HPF_CUTOFF_ID,
                                              "order": HPF_ORDER_ID}},
                              {"effect_id": "kronello.audio.limiter", "version": 1,
                               "parameters": {"kind": "audio_limiter",
                                              "ceiling_db": CEILING_ID,
                                              "release_ms": RELEASE_ID}}]))
        # AUDIO-008: loudness measure then normalize toward -23 LUFS.
        measured = ch.call("audio.loudness", project=str(ch.path),
                           base_revision=ch.revision,
                           input={"kind": "clip", "sequence": SEQ, "clip": CLIP_TONE})
        self.check(f"{tag}.audio.loudness",
                   measured.get("integrated_lufs") is not None and
                   measured["frames"] > 0, lufs=measured.get("integrated_lufs"))
        normalized = ch.call("audio.normalize", project=str(ch.path),
                             base_revision=ch.revision, session_id=ch.session,
                             idempotency_key=ch.key("audio.normalize"),
                             sequence=SEQ, clip=CLIP_TONE, target_lufs=-23.0)
        ch.revision = str(normalized["event"]["revision"])
        self.check(f"{tag}.audio.normalize",
                   abs(normalized["measured_lufs"] - measured["integrated_lufs"]) < 0.1 and
                   abs(normalized["gain_linear"] - 10 ** (normalized["gain_db"] / 20)) < 1e-6,
                   gain_db=normalized["gain_db"])
        again = ch.call("audio.loudness", project=str(ch.path),
                        base_revision=ch.revision,
                        input={"kind": "clip", "sequence": SEQ, "clip": CLIP_TONE})
        self.check(f"{tag}.audio.normalized_lufs",
                   abs(again["integrated_lufs"] - (-23.0)) < 0.5,
                   lufs=again["integrated_lufs"])
        # TRACK-001: one deterministic point track commits a data asset.
        tracked = ch.call("track.analyze", project=str(ch.path),
                          base_revision=ch.revision, id=TRACKING_ASSET,
                          asset=VIDEO_ASSET, stream_index=0, mode="points",
                          seeds=[{"x": 0.5, "y": 0.5, "template_radius": 4,
                                  "search_radius": 6}],
                          range=trange(0, Fraction(4, 24)),
                          idempotency_key=ch.key("track.analyze"))
        ch.revision = str(tracked["revision"])
        # PROXY-001: submit, wait, then the link reports ready.
        submitted = ch.call("proxy.generate", project=str(ch.path),
                            expected_revision=ch.revision, assets=[VIDEO_ASSET],
                            scale=0.5)
        self.check(f"{tag}.proxy.submit", len(submitted["jobs"]) == 1,
                   jobs=len(submitted["jobs"]))
        proxy_job = ch.wait_job(submitted["jobs"][0])
        self.check(f"{tag}.proxy.job", proxy_job["status"] == "succeeded",
                   error=proxy_job.get("error"))
        # The worker commits the link against the live revision; re-read it.
        info = ch.call("project.info", project=str(ch.path))
        ch.revision = str(info["revision"])
        status = ch.call("proxy.status", project=str(ch.path))
        self.check(f"{tag}.proxy.ready",
                   len(status["proxies"]) == 1 and
                   status["proxies"][0]["state"] == "ready" and
                   status["proxies"][0]["link"]["original"] == VIDEO_ASSET,
                   proxies=status["proxies"])
        # NLE-005 authored switch: disabling the masked clip removes it from
        # the composite while keeping the placement.
        masked = ch.frame(2)
        ch.apply(timeline("clip_enable_set", sequence=SEQ, clip=CLIP_M, enabled=False))
        unmasked = ch.frame(2)
        self.check(f"{tag}.nle.clip_disable",
                   self.px(masked, 40, 90) != self.px(unmasked, 40, 90),
                   before=self.px(masked, 40, 90), after=self.px(unmasked, 40, 90))
        ch.apply(timeline("clip_enable_set", sequence=SEQ, clip=CLIP_M, enabled=True))
        self.structure(ch)
        self.pixel_checks(ch)

    def structure(self, ch):
        tag = ch.transport
        result = ch.query()
        self.check(f"{tag}.revision", result["revision"] == ch.revision)
        sequence = result["sequence"]
        tracks = {t["id"]: t for t in sequence["tracks"]}
        self.check(f"{tag}.tracks",
                   [t["kind"] for t in sequence["tracks"]] ==
                   ["video", "video", "video", "video", "audio"])
        self.check(f"{tag}.track.unlocked",
                   tracks[TRACK_V1]["state"]["locked"] is False)
        self.check(f"{tag}.targets",
                   sequence["targets"] == {"video": TRACK_V1, "audio": TRACK_A},
                   targets=sequence["targets"])
        v1 = {c["id"]: c for c in tracks[TRACK_V1]["clips"]}
        frozen = next(c for c in v1.values()
                      if c["timeline_range"]["start"] == rational(3))
        self.check(f"{tag}.freeze.map",
                   frozen["time_map"]["kind"] == "piecewise_linear" and
                   frozen["source_in"] == rational(3) and
                   [p["local"] for p in frozen["time_map"]["points"]] ==
                   [rational(0)] * 2, map=frozen["time_map"])
        self.check(f"{tag}.retime.map",
                   v1[CLIP_V2]["time_map"]["points"] == [
                       {"parent": rational(0), "local": rational(0)},
                       {"parent": rational(2), "local": rational(1)},
                       {"parent": rational(4), "local": rational(1)},
                       {"parent": rational(6), "local": rational(3)}],
                   map=v1[CLIP_V2]["time_map"])
        masked = tracks[TRACK_V2]["clips"][0]
        self.check(f"{tag}.mask.stored",
                   masked.get("enabled", True) is True and
                   len(masked["masks"]) == 1 and
                   masked["masks"][0]["id"] == MASK_ID)
        kinds = {c["clip"]["id"]: c["kind"] for c in result["clips"]}
        self.check(f"{tag}.adjustment.kind", kinds[CLIP_ADJ] == "adjustment",
                   kind=kinds[CLIP_ADJ])
        document = ch.call("project.export", project=str(ch.path))["document"]
        self.check(f"{tag}.tracking.asset",
                   len(document.get("tracking_data_assets", [])) == 1,
                   count=len(document.get("tracking_data_assets", [])))
        self.check(f"{tag}.proxy.persisted",
                   len(document.get("proxies", [])) == 1)

    def pixel_checks(self, ch):
        tag = ch.transport
        # Mask: left half is authored blue; the right half shows the video.
        frame = ch.frame(2)
        left, right = self.px(frame, 40, 90), self.px(frame, 280, 90)
        self.check(f"{tag}.mask.pixels",
                   left[2] > left[0] + 0.15 and left != right,
                   left=left, right=right)
        # Freeze: frames inside the hold are the identical source frame.
        held_a, held_b, before = ch.frame(4), ch.frame(5), ch.frame(2)
        self.check(f"{tag}.freeze.pixels",
                   self.probe(held_a) == self.probe(held_b) and
                   self.probe(before) != self.probe(held_a),
                   held=self.probe(held_a), before=self.probe(before))
        # Retime hold: parents 2 and 3 inside CLIP_V2 both sample local 1.
        hold_a, hold_b, ramp = ch.frame(8), ch.frame(9), ch.frame(Fraction(13, 2))
        self.check(f"{tag}.retime.hold",
                   self.probe(hold_a) == self.probe(hold_b) and
                   self.probe(ramp) != self.probe(hold_a))
        # Adjustment: -1 EV halves the lower composite in linear premult
        # (a brightening exposure would leave the SDR delivery gamut and the
        # movie job is required to succeed below).
        adjusted = ch.frame(8)
        ch.apply(timeline("clip_enable_set", sequence=SEQ, clip=CLIP_ADJ,
                          enabled=False))
        plain = ch.frame(8)
        ch.apply(timeline("clip_enable_set", sequence=SEQ, clip=CLIP_ADJ,
                          enabled=True))
        ratio = [a / b for a, b in zip(self.px(adjusted, 280, 90)[:3],
                                       self.px(plain, 280, 90)[:3]) if b > 0.02]
        self.check(f"{tag}.adjustment.exposure",
                   ratio and all(abs(v - 0.5) < 0.08 for v in ratio), ratio=ratio)
        # LUT: the magenta solid inverts through the size-2 lattice.
        luted = ch.frame(10)
        expected = [0.3079, 0.9900, 0.3079]
        self.check(f"{tag}.lut.pixels",
                   all(abs(p - e) < 0.03 for p, e in
                       zip(self.px(luted, 160, 90)[:3], expected)),
                   pixel=self.px(luted, 160, 90), expected=expected)
        # Without the lattice the same render is a typed failure.
        missing = dict(ch.render_input())
        missing["luts"] = []
        ch.call("render.frame", expected_error="LUT_INPUT_MISSING",
                input=missing, time=rational(10))
        # Proxy preview input is preview-only; file-writing paths reject it.
        preview = dict(ch.render_input())
        preview["media_proxies"] = "prefer"
        proxied = ch.call("render.frame", input=preview, time=rational(2))
        self.check(f"{tag}.proxy.preview",
                   proxied["metadata"]["region"]["pixels"] == self.pixels)
        ch.call("render.submit", expected_error="UNSUPPORTED_FEATURE",
                render={"input": preview, "range": trange(*WORK_AREA),
                        "frame_rate": rational(JOB_RATE),
                        "output_directory": str(self.output / f"{tag}-prefer")},
                output={"format": "image_sequence"})

    def jobs(self, ch):
        tag = ch.transport
        frames_dest = self.output / f"{tag}-frames"
        frames_job = ch.wait_job(ch.submit(frames_dest, JOB_RATE,
                                           {"format": "image_sequence"}))
        self.check(f"{tag}.job.frames.succeeded",
                   frames_job["status"] == "succeeded" and
                   frames_job["completed_frames"] == 24 and
                   frames_job["result"]["validated"] is True,
                   error=frames_job.get("error"))
        manifest = json.loads((frames_dest / "sequence.json").read_text())
        times = [fraction(f["metadata"]["time"]) for f in manifest["frames"]]
        self.check(f"{tag}.job.frames.range",
                   manifest["range"] == trange(*WORK_AREA) and
                   times == [1 + Fraction(k, JOB_RATE) for k in range(24)])
        ch.frame_manifest = manifest
        movie_dest = self.output / f"{tag}-movie.mov"
        movie_job = ch.wait_job(ch.submit(
            movie_dest, MOVIE_RATE,
            {"format": "pro_res_mov", "profile_version": 3,
             "audio": "document", "clips": [], "background": [0, 0, 0]}))
        self.check(f"{tag}.job.movie.succeeded",
                   movie_job["status"] == "succeeded" and
                   movie_job["completed_frames"] == 144 and
                   movie_job["result"]["validated"] is True,
                   error=movie_job.get("error"))
        probe = json.loads(subprocess.run(
            ["ffprobe", "-v", "error", "-show_streams", "-of", "json",
             str(movie_dest)], capture_output=True, text=True,
            env=self.env, timeout=60).stdout)
        streams = {s["codec_type"]: s for s in probe["streams"]}
        audio = streams.get("audio", {})
        self.check(f"{tag}.job.movie.streams",
                   streams.get("video", {}).get("codec_name") == "prores" and
                   audio.get("codec_name") == "pcm_s24le" and
                   audio.get("sample_rate") == "48000",
                   streams=[{k: s.get("codec_name") for k, s in streams.items()}])
        completed = subprocess.run(
            ["ffmpeg", "-v", "info", "-i", str(movie_dest), "-af", "astats",
             "-f", "null", "-"], capture_output=True, text=True,
            env=self.env, timeout=120)
        match = re.search(r"RMS level dB:\s*(-?[\d.]+|-inf)", completed.stderr)
        rms = float(match.group(1)) if match else None
        self.check(f"{tag}.job.movie.audio_level", rms is not None and rms > -40,
                   rms_db=rms)

    def run(self):
        self.fonts = []
        self.compositions = [{"id": uid("missing-composition")}]
        self.start_mcp()
        capabilities = self.tool("capabilities.get")
        self.check("capabilities.features",
                   {"clip_effects", "generator_clip", "clip_split", "ripple",
                    "movie_delivery_v1"} <= set(capabilities["features"]))
        for ch in (self.cli_channel, self.mcp_channel):
            self.build(ch)
        cli_q = self.cli_channel.query()
        mcp_q = self.mcp_channel.query()
        self.check("parity.sequence.query",
                   canonical_bytes(cli_q) == canonical_bytes(mcp_q),
                   cli_sha256=digest(canonical_bytes(cli_q)))
        cli_doc = self.cli_channel.call("project.export",
                                        project=str(self.cli_channel.path))
        mcp_doc = self.mcp_channel.call("project.export",
                                        project=str(self.mcp_channel.path))
        self.check("parity.project.export",
                   cli_doc["revision"] == mcp_doc["revision"] and
                   canonical_bytes(parity_document(cli_doc["document"])) ==
                   canonical_bytes(parity_document(mcp_doc["document"])),
                   revision=cli_doc["revision"])
        for t in (Fraction(2), Fraction(8), Fraction(10)):
            self.check(f"parity.frame.{t}",
                       self.cli_channel.frame(t)["linear"] ==
                       self.mcp_channel.frame(t)["linear"])
        for ch in (self.cli_channel, self.mcp_channel):
            self.jobs(ch)
        cli_hashes = [f["numeric"]["sha256"]
                      for f in self.cli_channel.frame_manifest["frames"]]
        mcp_hashes = [f["numeric"]["sha256"]
                      for f in self.mcp_channel.frame_manifest["frames"]]
        self.check("parity.job.frames.hashes", cli_hashes == mcp_hashes)
        manifest = {
            "schema_version": 1,
            "projects": {"cli": str(self.cli_channel.path),
                         "mcp": str(self.mcp_channel.path)},
            "revision": self.cli_channel.revision,
            "project_id": PROJECT_ID, "sequence": SEQ,
            "tracks": {"v1": TRACK_V1, "v2": TRACK_V2, "v3": TRACK_V3,
                       "v4": TRACK_V4, "audio": TRACK_A},
            "clips": {"v1": CLIP_V1, "v2": CLIP_V2, "masked": CLIP_M,
                      "adjustment": CLIP_ADJ, "lut": CLIP_LUT, "tone": CLIP_TONE},
            "assets": {"video": VIDEO_ASSET, "lut": LUT_ASSET,
                       "tracking": TRACKING_ASSET},
            "work_area": [1, 7],
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
    demo = IntegrationM8(args)
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
