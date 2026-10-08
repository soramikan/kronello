#!/usr/bin/env python3
"""Run INTEGRATION-006 through real CLI/MCP processes, without private storage access.

The M9 editing-workflow project exercises multicam creation/switching
(NLE-007), shared three-point insert/overwrite (GUI-011), scene detection and
application (AI-002), stabilization (TRACK-002), optical-flow retime
interpolation (TRACK-003), pitch-preserving audio retime plus multichannel
export (AUDIO-010), media bins (FLOW-002) and preset-driven batch export
(FLOW-003) over both shared-API transports.
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

NS = uuid.UUID("9d7c5e31-2a48-4f6b-b1c0-3e8d9f5a7b2c")


def uid(name):
    return str(uuid.uuid5(NS, name))


PROJECT_ID = uid("project")
SEQ = uid("sequence")
TRACK_V1, TRACK_V2, TRACK_A = (uid(f"track.{n}") for n in ("v1", "v2", "audio"))
CLIP_MC, CLIP_V2, CLIP_TONE, CLIP_FLOW = (
    uid(f"clip.{n}") for n in ("multicam", "v2", "tone", "flow"))
CLIP_INSERTED, CLIP_OVERWRITTEN = uid("clip.inserted"), uid("clip.overwritten")
ASSET_A, ASSET_B = uid("asset.angle-a"), uid("asset.angle-b")
MULTICAM_ID, ANGLE_A, ANGLE_B = (
    uid("multicam"), uid("angle.a"), uid("angle.b"))
TRACKING_ASSET = uid("asset.tracking")
ASSET_TONE = uid("asset.tone")
BIN_ID, PRESET_ID = uid("bin"), uid("preset.export")
STAB_PROPS = [uid(f"stab.{n}") for n in (
    "tracking", "smoothing", "displacement", "rotation", "crop", "border",
    "fill", "sampling")]

WORK_AREA = (Fraction(0), Fraction(4))
MOVIE_RATE = 24


def trange(start, end):
    return {"start": rational(start), "end": rational(end)}


def srgb(r, g, b, a=1.0):
    return {"space": "srgb", "components": {"r": r, "g": g, "b": b, "alpha": a}}


def linear_map(speed=1):
    return {"kind": "linear", "offset": rational(0), "speed": rational(speed)}


def clip(cid, start, end, source, effects=None, properties=None,
         audio_retime="reject", time_map=None):
    return {"id": cid, "source_ref": source,
            "timeline_range": trange(start, end), "source_in": rational(0),
            "time_map": time_map or linear_map(), "audio_retime": audio_retime,
            "links": [], "effects": effects or [], "properties": properties or []}


def timeline(kind, **fields):
    return {"timeline": {kind: fields}}


def property_ref(pid, key, kind, value):
    return {"id": pid, "descriptor": {"key": key, "version": 1},
            "source": {"kind": "constant", "value": {"kind": kind, "value": value}},
            "modifiers": []}


def scalar_property(pid, key, value):
    return property_ref(pid, key, "scalar", value)


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


class M9Channel(Channel):
    """Channel bound to this demo's sequence."""

    def __init__(self, demo, transport):
        super().__init__(demo, transport)
        self.path = demo.output / f"m9-integration.{transport}.kronello"

    def call(self, operation, expected_error=None, **payload):
        if self.transport == "cli":
            return self.demo.cli(operation, expected_error, **payload)
        # Demo.tool's `name` parameter collides with request fields named
        # `name` (multicam.create); call the tool through rpc directly with
        # the same parity checks.
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


class IntegrationM9(Demo):
    def __init__(self, args):
        super().__init__(args)
        self.pixels = [320, 180]
        self.cli_channel = M9Channel(self, "cli")
        self.mcp_channel = M9Channel(self, "mcp")
        self.project = self.cli_channel.path

    def px(self, frame, x, y):
        return frame["linear"][y * self.pixels[0] + x]

    def video_source(self, path, first, second=None, split=4, total=8):
        """Deterministic angle fixture; `second` concatenates a hard cut.
        `first`/`second` are complete lavfi source specs."""
        inputs = ["-f", "lavfi", "-i", first]
        if second:
            inputs += ["-f", "lavfi", "-i", second]
        cmd = ["ffmpeg", "-v", "error", *inputs]
        if second:
            cmd += ["-filter_complex", "[0:v][1:v]concat=n=2:v=1[v]",
                    "-map", "[v]"]
        else:
            cmd += ["-map", "0:v"]
        # scene.detect budgets against stream.time_base; pin the track
        # timescale to the frame rate so the estimate equals the frame count.
        cmd += ["-c:v", "mpeg4", "-pix_fmt", "yuv420p",
                "-video_track_timescale", "24", "-an", str(path)]
        subprocess.run(cmd, check=True)
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
        media = self.output / f"{tag}-media"
        media.mkdir()
        # Angle A carries a hard cut at 4s for scene detection; angle B is a
        # visually distinct pattern for multicam switching checks.
        video_a = media / "angle-a.mov"
        video_b = media / "angle-b.mov"
        stream_a = self.video_source(
            video_a, "testsrc2=size=320x180:rate=24:duration=4",
            "color=c=red:size=320x180:rate=24:duration=4")
        stream_b = self.video_source(
            video_b, "smptebars=size=320x180:rate=24:duration=8")
        # AUDIO-010 source: a real 5.1 tone so the retimed clip exercises
        # multichannel decode, stereo fold-down and WSOLA pitch preservation.
        tone = media / "tone.mov"
        subprocess.run(["ffmpeg", "-v", "error", "-f", "lavfi", "-i",
                        "sine=frequency=440:sample_rate=48000:duration=8",
                        "-ac", "6", "-c:a", "pcm_s24le", "-vn", str(tone)],
                       check=True)
        tone_probe = json.loads(subprocess.run(
            ["ffprobe", "-v", "error", "-select_streams", "a:0",
             "-show_entries", "stream=codec_name,time_base,duration_ts",
             "-of", "json", str(tone)],
            capture_output=True, text=True, check=True).stdout)["streams"][0]
        num, den = tone_probe["time_base"].split("/")
        stream_t = {"index": 0, "codec": tone_probe["codec_name"],
                    "time_base": {"num": num, "den": den},
                    "duration": rational(Fraction(int(tone_probe["duration_ts"]),
                                                  int(den)))}
        document = {"id": PROJECT_ID, "schema_version": 1, "semantic_version": 1,
                    "name": "INTEGRATION-006 M9 編集ワークフロー",
                    "compositions": [], "curves": [],
                    "assets": [
                        {"id": ASSET_A, "kind": "video",
                         "content_hash": hashlib.sha256(video_a.read_bytes()).hexdigest(),
                         "locator": {"relative": video_a.name, "absolute": str(video_a)},
                         "streams": [stream_a]},
                        {"id": ASSET_B, "kind": "video",
                         "content_hash": hashlib.sha256(video_b.read_bytes()).hexdigest(),
                         "locator": {"relative": video_b.name, "absolute": str(video_b)},
                         "streams": [stream_b]},
                        {"id": ASSET_TONE, "kind": "audio",
                         "content_hash": hashlib.sha256(tone.read_bytes()).hexdigest(),
                         "locator": {"relative": tone.name, "absolute": str(tone)},
                         "streams": [stream_t]}]}
        created = ch.call("project.create", project=str(ch.path), document=document)
        ch.revision = str(created["revision"])
        self.check(f"{tag}.project.create", created["project_id"] == PROJECT_ID)
        # NLE-007: the multicam group precedes clip placement (manual sync,
        # zero offsets); audio sync against video-only angles fails typed.
        ch.edit("multicam.create", multicam=MULTICAM_ID, name="Camera Group",
                sync="manual",
                angles=[{"id": ANGLE_A, "asset": ASSET_A, "stream_index": 0},
                        {"id": ANGLE_B, "asset": ASSET_B, "stream_index": 0}],
                offsets={ANGLE_A: rational(0), ANGLE_B: rational(0)})
        ch.call("multicam.create", expected_error="MULTICAM_SYNC_FAILED",
                project=str(ch.path), base_revision=ch.revision,
                session_id=ch.session, idempotency_key=ch.key("mc-audio-fail"),
                multicam=uid("multicam.bad"), sync="audio",
                angles=[{"id": uid("angle.bad-a"), "asset": ASSET_A,
                         "stream_index": 0},
                        {"id": uid("angle.bad-b"), "asset": ASSET_B,
                         "stream_index": 0}])
        ch.edit("sequence.create", sequence={
            "id": SEQ, "extent": {"width": 320, "height": 180},
            "frame_rate": rational(24), "audio_rate": 48000,
            "working_space": "linear_rec709",
            "tracks": [{"id": TRACK_V1, "kind": "video", "clips": []},
                       {"id": TRACK_V2, "kind": "video", "clips": []},
                       {"id": TRACK_A, "kind": "audio", "clips": []}]})
        # The multicam clip sources angle A across the whole 8s asset.
        ch.edit("clip.place", sequence=SEQ, track=TRACK_V1,
                clip=clip(CLIP_MC, 0, 8, {"kind": "multicam",
                                          "multicam": MULTICAM_ID,
                                          "angle": ANGLE_A}))
        frame_a = ch.frame(1)
        ch.edit("clip.angle_switch", sequence=SEQ, clip=CLIP_MC, angle=ANGLE_B)
        frame_b = ch.frame(1)
        self.check(f"{tag}.multicam.angle_switch",
                   self.px(frame_a, 160, 90) != self.px(frame_b, 160, 90),
                   angle_a=self.px(frame_a, 160, 90), angle_b=self.px(frame_b, 160, 90))
        ch.call("clip.angle_switch", expected_error="SOURCE_MISSING",
                project=str(ch.path), base_revision=ch.revision,
                session_id=ch.session, idempotency_key=ch.key("bad-angle"),
                sequence=SEQ, clip=CLIP_MC, angle=uid("angle.missing"))
        # TRACK-002: switch back to angle A, track it, attach
        # kronello.stabilize, then prove the bound source is enforced by
        # switching to the untracked angle B.
        ch.edit("clip.angle_switch", sequence=SEQ, clip=CLIP_MC, angle=ANGLE_A)
        tracked = ch.call("track.analyze", project=str(ch.path),
                          base_revision=ch.revision, id=TRACKING_ASSET,
                          asset=ASSET_A, stream_index=0, mode="points",
                          seeds=[{"x": 0.5, "y": 0.5, "template_radius": 4,
                                  "search_radius": 6}],
                          range=trange(0, 8),
                          idempotency_key=ch.key("track.analyze"))
        ch.revision = str(tracked["revision"])
        ch.apply(timeline("clip_set_effects", sequence=SEQ, clip=CLIP_MC,
                          properties=[
                              property_ref(STAB_PROPS[0], "kronello.effect.tracking",
                                           "asset_ref", TRACKING_ASSET),
                              scalar_property(STAB_PROPS[1],
                                              "kronello.effect.smoothing_radius", 1.0),
                              scalar_property(STAB_PROPS[2],
                                              "kronello.effect.max_displacement", 64.0),
                              property_ref(STAB_PROPS[3],
                                           "kronello.effect.max_rotation",
                                           "angle", 5.0),
                              scalar_property(STAB_PROPS[4],
                                              "kronello.effect.max_crop", 0.25),
                              property_ref(STAB_PROPS[5], "kronello.effect.border",
                                           "enum", "replicate"),
                              property_ref(STAB_PROPS[6],
                                           "kronello.effect.fill_color", "color",
                                           srgb(0, 0, 0, 0)),
                              property_ref(STAB_PROPS[7],
                                           "kronello.effect.sampling", "enum",
                                           "bilinear")],
                          effects=[{"effect_id": "kronello.stabilize", "version": 1,
                                    "parameters": {"kind": "stabilize",
                                                   "tracking": STAB_PROPS[0],
                                                   "smoothing_radius": STAB_PROPS[1],
                                                   "max_displacement": STAB_PROPS[2],
                                                   "max_rotation": STAB_PROPS[3],
                                                   "max_crop": STAB_PROPS[4],
                                                   "border": STAB_PROPS[5],
                                                   "fill_color": STAB_PROPS[6],
                                                   "sampling": STAB_PROPS[7]}}]))
        stabilized = ch.frame(1)
        self.check(f"{tag}.stabilize.render",
                   stabilized["metadata"]["region"]["pixels"] == self.pixels)
        ch.edit("clip.angle_switch", sequence=SEQ, clip=CLIP_MC, angle=ANGLE_B)
        ch.call("render.frame", expected_error="TRACKING_DATA_STALE",
                input=ch.render_input(), time=rational(1))
        ch.edit("clip.angle_switch", sequence=SEQ, clip=CLIP_MC, angle=ANGLE_A)
        # V2 sources angle A directly so scene.apply can map boundaries; a
        # flow-retimed clip and the retimed tone cover TRACK-003/AUDIO-010.
        ch.edit("clip.place", sequence=SEQ, track=TRACK_V2,
                clip=clip(CLIP_V2, 0, 8, {"kind": "asset", "asset": ASSET_A,
                                          "stream_index": 0}))
        flow_map = {"kind": "piecewise_linear",
                    "points": [{"parent": rational(0), "local": rational(0)},
                               {"parent": rational(2), "local": rational(1, 2)},
                               {"parent": rational(4), "local": rational(1)}],
                    "interpolation": {"mode": "optical_flow",
                                      "block_radius": 2, "search_radius": 4,
                                      "levels": 2,
                                      "confidence_floor": rational(1, 4),
                                      "max_low_confidence": rational(1, 2)}}
        ch.edit("clip.place", sequence=SEQ, track=TRACK_V2,
                clip=clip(CLIP_FLOW, 10, 14, {"kind": "asset", "asset": ASSET_B,
                                              "stream_index": 0},
                          time_map=flow_map))
        # t=11.25 maps to source 0.3125s — halfway between 24fps frames, so the
        # optical-flow synthesis path actually runs.
        interpolated = ch.frame(Fraction(45, 4))
        self.check(f"{tag}.flow.render",
                   interpolated["metadata"]["region"]["pixels"] == self.pixels)
        ch.edit("clip.place", sequence=SEQ, track=TRACK_A,
                clip=clip(uid("clip.tone"), 0, 4,
                          {"kind": "asset", "asset": ASSET_TONE,
                           "stream_index": 0},
                          audio_retime="pitch_preserve_v1",
                          time_map=linear_map(2)))
        self.scene_and_edits(ch)
        self.media_and_export(ch)

    def scene_and_edits(self, ch):
        tag = ch.transport
        # AI-002: the fixed-input job commits a SceneBoundaryAsset on success;
        # the authored hard cut at 4s is found deterministically.
        submitted = ch.call("scene.detect", project=str(ch.path),
                            expected_revision=ch.revision,
                            asset=ASSET_A, stream_index=0,
                            range=trange(0, 8))
        scene_job = ch.wait_job(submitted)
        self.check(f"{tag}.scene.job", scene_job["status"] == "succeeded",
                   error=scene_job.get("error"))
        info = ch.call("project.info", project=str(ch.path))
        ch.revision = str(info["revision"])
        document = ch.call("project.export", project=str(ch.path))["document"]
        scene_assets = document.get("scene_boundary_assets", [])
        self.check(f"{tag}.scene.asset", len(scene_assets) == 1,
                   count=len(scene_assets))
        scene_asset = scene_assets[0]["id"]
        boundary_times = [fraction(b["time"]) for b in scene_assets[0]["boundaries"]]
        self.check(f"{tag}.scene.cut",
                   any(abs(t - 4) <= Fraction(1, 8) for t in boundary_times),
                   boundaries=[str(t) for t in boundary_times])
        ch.edit("scene.apply", scene_asset=scene_asset, sequence=SEQ,
                mode="markers", clip=CLIP_V2)
        result = ch.query()["sequence"]
        markers = result.get("markers", [])
        self.check(f"{tag}.scene.markers",
                   any(abs(fraction(m["time"]) - 4) <= Fraction(1, 8)
                       for m in markers),
                   markers=markers)
        ch.edit("scene.apply", scene_asset=scene_asset, sequence=SEQ,
                mode="split", clip=CLIP_V2)
        tracks = {t["id"]: t for t in ch.query()["sequence"]["tracks"]}
        v2_clips = sorted(tracks[TRACK_V2]["clips"],
                          key=lambda c: fraction(c["timeline_range"]["start"]))
        self.check(f"{tag}.scene.split",
                   len(v2_clips) >= 2 and
                   v2_clips[0]["timeline_range"]["end"] ==
                   v2_clips[1]["timeline_range"]["start"],
                   clips=len(v2_clips))
        # GUI-011: shared three-point insert ripples content at a clip
        # boundary (straddling insertions are typed rejections), then
        # overwrite replaces covered content mid-clip.
        ch.edit("edit.insert", sequence=SEQ, clip=CLIP_INSERTED,
                source={"kind": "asset", "asset": ASSET_B, "stream_index": 0},
                source_range=trange(0, 1), at=rational(4), track=TRACK_V2)
        ch.edit("edit.overwrite", sequence=SEQ, clip=CLIP_OVERWRITTEN,
                source={"kind": "asset", "asset": ASSET_B, "stream_index": 0},
                source_range=trange(0, 2), at=rational(6), track=TRACK_V2,
                split_tail=uid("clip.tail"))
        tracks = {t["id"]: t for t in ch.query()["sequence"]["tracks"]}
        v2 = {c["id"]: c for c in tracks[TRACK_V2]["clips"]}
        inserted = v2.get(CLIP_INSERTED)
        overwritten = v2.get(CLIP_OVERWRITTEN)
        self.check(f"{tag}.edit.insert",
                   inserted is not None and
                   inserted["timeline_range"] == trange(4, 5) and
                   inserted["source_ref"].get("asset") == ASSET_B,
                   clip=inserted)
        self.check(f"{tag}.edit.overwrite",
                   overwritten is not None and
                   overwritten["timeline_range"] == trange(6, 8),
                   clip=overwritten)
        # A locked destination rejects the shared placement path atomically.
        ch.apply(timeline("track_state_set", sequence=SEQ, track=TRACK_V2,
                          state={"visible": True, "muted": False, "locked": True}))
        ch.call("edit.insert", expected_error="TRACK_LOCKED",
                project=str(ch.path), base_revision=ch.revision,
                session_id=ch.session, idempotency_key=ch.key("locked-insert"),
                sequence=SEQ, clip=uid("clip.locked"),
                source={"kind": "asset", "asset": ASSET_B, "stream_index": 0},
                source_range=trange(0, 1), at=rational(9), track=TRACK_V2)
        ch.apply(timeline("track_state_set", sequence=SEQ, track=TRACK_V2,
                          state={"visible": True, "muted": False, "locked": False}))

    def media_and_export(self, ch):
        tag = ch.transport
        # FLOW-002: bins are document-owned membership lists.
        ch.apply({"bin_create": {"bin": {"id": BIN_ID, "name": "素材",
                                          "assets": []}}},
                 {"bin_assign": {"bin": BIN_ID, "assets": [ASSET_A, ASSET_B]}})
        media = ch.call("media.query", project=str(ch.path))
        bins = {b["id"]: b for b in media.get("bins", [])}
        self.check(f"{tag}.media.bins",
                   BIN_ID in bins and bins[BIN_ID]["assets"] == [ASSET_A, ASSET_B],
                   bins=list(bins))
        self.check(f"{tag}.media.assets",
                   all(a["availability"] == "present_unverified"
                       for a in media["assets"]),
                   assets=[a.get("availability") for a in media["assets"]])
        thumb = ch.call("asset.thumbnail", project=str(ch.path),
                        asset=ASSET_B, max_size=64)
        self.check(f"{tag}.media.thumbnail",
                   thumb["width"] > 0 and len(thumb["rgba"]) ==
                   thumb["width"] * thumb["height"] * 4,
                   width=thumb.get("width"), height=thumb.get("height"))
        # FLOW-003: a stored preset plus an inline submission queue together.
        preset = {"version": 1, "id": PRESET_ID, "name": "ProRes 5.1",
                  "target": {"kind": "sequence", "sequence": SEQ},
                  "range": trange(*WORK_AREA), "frame_rate": rational(MOVIE_RATE),
                  "region": {"origin": [0, 0], "extent": [320, 180],
                             "pixels": list(self.pixels)},
                  "output": {"format": "pro_res_mov", "profile_version": 3,
                             "audio": "document", "audio_layout": 1551,
                             "clips": [], "background": [0, 0, 0]}}
        ch.apply({"export_preset_save": {"preset": preset}})
        movie_dest = self.output / f"{tag}-movie.mov"
        frames_dest = self.output / f"{tag}-frames"
        movie_key = ch.key("preset-movie")
        batch = ch.call("export.batch", items=[
            {"idempotency_key": movie_key, "preset": PRESET_ID,
             "project": str(ch.path), "destination": str(movie_dest)},
            {"idempotency_key": ch.key("inline-frames"),
             "submission": {
                 "render": {"input": ch.render_input(),
                            "range": trange(*WORK_AREA),
                            "frame_rate": rational(4),
                            "output_directory": str(frames_dest)},
                 "output": {"format": "image_sequence"}}}],
            failure_policy="continue")
        outcomes = [item["outcome"] for item in batch["items"]]
        self.check(f"{tag}.export.batch", outcomes == ["submitted", "submitted"],
                   outcomes=outcomes)
        movie_job = ch.wait_job(batch["items"][0]["job"])
        frames_job = ch.wait_job(batch["items"][1]["job"])
        self.check(f"{tag}.job.movie.succeeded",
                   movie_job["status"] == "succeeded", error=movie_job.get("error"))
        self.check(f"{tag}.job.frames.succeeded",
                   frames_job["status"] == "succeeded" and
                   frames_job["result"]["validated"] is True,
                   error=frames_job.get("error"))
        # Batch replay: the same submission key returns its original job.
        replay = ch.call("export.batch", items=[
            {"idempotency_key": movie_key, "preset": PRESET_ID,
             "project": str(ch.path), "destination": str(movie_dest)}],
            failure_policy="continue")
        self.check(f"{tag}.export.batch.replay",
                   replay["items"][0]["outcome"] == "replayed",
                   outcome=replay["items"][0]["outcome"])
        probe = json.loads(subprocess.run(
            ["ffprobe", "-v", "error", "-show_streams", "-of", "json",
             str(movie_dest)], capture_output=True, text=True,
            env=self.env, timeout=60).stdout)
        streams = {s["codec_type"]: s for s in probe["streams"]}
        audio = streams.get("audio", {})
        # AUDIO-010: the document audio renders through WSOLA retime into the
        # pinned 5.1 layout (1551 = FL|FR|C|LFE|SL|SR mask bits).
        self.check(f"{tag}.job.movie.streams",
                   streams.get("video", {}).get("codec_name") == "prores" and
                   audio.get("codec_name") == "pcm_s24le" and
                   audio.get("channels") == 6,
                   streams=[{k: s.get("codec_name") for k, s in streams.items()},
                            audio.get("channels")])

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
        for t in (Fraction(1), Fraction(3), Fraction(11)):
            self.check(f"parity.frame.{t}",
                       self.cli_channel.frame(t)["linear"] ==
                       self.mcp_channel.frame(t)["linear"])
        manifest = {
            "schema_version": 1,
            "projects": {"cli": str(self.cli_channel.path),
                         "mcp": str(self.mcp_channel.path)},
            "revision": self.cli_channel.revision,
            "project_id": PROJECT_ID, "sequence": SEQ,
            "tracks": {"v1": TRACK_V1, "v2": TRACK_V2, "audio": TRACK_A},
            "clips": {"multicam": CLIP_MC, "v2": CLIP_V2,
                      "inserted": CLIP_INSERTED, "overwritten": CLIP_OVERWRITTEN},
            "multicam": MULTICAM_ID, "angles": {"a": ANGLE_A, "b": ANGLE_B},
            "assets": {"angle_a": ASSET_A, "angle_b": ASSET_B,
                       "tracking": TRACKING_ASSET},
            "bin": BIN_ID, "export_preset": PRESET_ID,
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
    demo = IntegrationM9(args)
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
