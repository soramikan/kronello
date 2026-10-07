#!/usr/bin/env python3
"""Run INTEGRATION-003 through real CLI/MCP processes, without private storage access."""
import argparse
from fractions import Fraction
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import time
import uuid

from demo_integration_m2 import Demo, ROOT, digest, fraction, rational
from demo_integration_m3 import canonical_bytes
from fixtures import external_fixture_dir

# Deterministic identities keep the CLI and MCP builds byte-identical.
NS = uuid.UUID("a6f4c2d1-9e3b-4a58-8c72-1d0e5b6f7a89")


def uid(name):
    return str(uuid.uuid5(NS, name))


PROJECT_ID = uid("project")
SEQ = uid("sequence")
TRACK_V, TRACK_A, TRACK_C = uid("track.video"), uid("track.audio"), uid("track.caption")
CLIP_A, CLIP_B, CLIP_B2, CLIP_C, CLIP_TONE = (uid(f"clip.{n}") for n in
                                            ("a", "b", "b2", "c", "tone"))
CUES = [(uid(f"caption.{i}"), uid(f"caption.clip.{i}")) for i in (1, 2, 3)]
MARKER_SEQ, MARKER_CLIP = uid("marker.sequence"), uid("marker.clip")
EXPOSURE_ID = "f0000000-0010-4100-8000-000000000005"
EXPOSURE_OFFSET_ID = "f0000000-0010-4100-8000-000000000006"
WORK_AREA = (Fraction(1), Fraction(7))
JOB_RATE, MOVIE_RATE = 4, 24

COLOR_A = (0.8, 0.1, 0.1)
COLOR_B = (0.15, 0.6, 0.2)
COLOR_C = (0.15, 0.25, 0.8)

SRT = (
    "1\r\n"
    "00:00:00,500 --> 00:00:02,000\r\n"
    "最初の字幕です\r\n"
    "\r\n"
    "2\r\n"
    "00:00:04,250 --> 00:00:05,750\r\n"
    "ワイプの <b>途中</b> です\r\n"
    "\r\n"
    "3\r\n"
    "00:00:08,000 --> 00:00:09,500\r\n"
    "色補正クリップの字幕\r\n"
)
EDITED_CUE1 = "編集済みの最初の字幕"
EXPECTED_SRT = (
    "1\r\n"
    "00:00:00,500 --> 00:00:02,000\r\n"
    f"{EDITED_CUE1}\r\n"
    "\r\n"
    "2\r\n"
    "00:00:04,250 --> 00:00:05,500\r\n"
    "ワイプの <b>途中</b> です\r\n"
    "\r\n"
    "3\r\n"
    "00:00:08,000 --> 00:00:09,500\r\n"
    "色補正クリップの字幕\r\n"
)


def trange(start, end):
    return {"start": rational(start), "end": rational(end)}


def srgb(r, g, b, a=1.0):
    return {"space": "srgb", "components": {"r": r, "g": g, "b": b, "alpha": a}}


def linear_map():
    return {"kind": "linear", "offset": rational(0), "speed": rational(1)}


def solid_clip(cid, start, end, color):
    return {"id": cid,
            "source_ref": {"kind": "generator", "generator": "kronello.solid",
                           "version": 1, "color": srgb(*color)},
            "timeline_range": trange(start, end), "source_in": rational(0),
            "time_map": linear_map(), "audio_retime": "reject",
            "links": [], "effects": []}


def tone_clip(cid, start, end):
    return {"id": cid,
            "source_ref": {"kind": "generator", "generator": "kronello.audio.tone440",
                           "version": 1, "color": srgb(0, 0, 0)},
            "timeline_range": trange(start, end), "source_in": rational(0),
            "time_map": linear_map(), "audio_retime": "reject",
            "links": [], "effects": []}


def timeline(kind, **fields):
    return {"timeline": {kind: fields}}


def scalar_property(property_id, key, value):
    return {"id": property_id, "descriptor": {"key": key, "version": 1},
            "source": {"kind": "constant", "value": {"kind": "scalar", "value": value}},
            "modifiers": []}


def caption_style(font):
    return {"font": font, "size": 12.0, "fill": srgb(1, 1, 1),
            "outline": {"color": srgb(0, 0, 0), "width": 1.0}}


class Channel:
    """One transport-bound project: identical requests over CLI or MCP."""

    def __init__(self, demo, transport):
        self.demo, self.transport = demo, transport
        self.path = demo.output / f"m7-integration.{transport}.kronello"
        self.revision = "1"
        self.session = str(uuid.uuid4())
        self.counter = 0
        self.caption_docs = {}

    def call(self, operation, expected_error=None, **payload):
        if self.transport == "cli":
            return self.demo.cli(operation, expected_error, **payload)
        return self.demo.tool(operation, expected_error, **payload)

    def key(self, tag):
        self.counter += 1
        return f"m7:{self.transport}:{self.counter}:{tag}"

    def edit(self, operation, **payload):
        result = self.call(operation, project=str(self.path), base_revision=self.revision,
                           session_id=self.session, idempotency_key=self.key(operation),
                           **payload)
        self.revision = str(result["revision"])
        return result

    def apply(self, *commands):
        commands = list(commands)
        planned = self.call("edit.plan", project=str(self.path),
                            base_revision=self.revision, commands=commands)
        result = self.call("edit.apply", project=str(self.path),
                           base_revision=self.revision, session_id=self.session,
                           idempotency_key=self.key("edit.apply"),
                           plan_hash=planned["plan_hash"], commands=commands)
        self.revision = str(result["revision"])
        return result

    def render_input(self):
        return {"project": str(self.path),
                "target": {"kind": "sequence", "sequence": SEQ},
                "region": {"origin": [0, 0], "extent": [320, 180],
                           "pixels": list(self.demo.pixels)},
                "fonts": self.demo.fonts}

    def frame(self, at):
        return self.call("render.frame", input=self.render_input(), time=rational(at))

    def query(self):
        return self.call("sequence.query", project=str(self.path), sequence=SEQ)

    def submit(self, destination, rate, output):
        return self.call("render.submit",
                         render={"input": self.render_input(), "range": trange(*WORK_AREA),
                                 "frame_rate": rational(rate),
                                 "output_directory": str(destination)},
                         output=output)

    def wait_job(self, job):
        deadline = time.monotonic() + 3600
        while True:
            result = self.call("job.get", job=job["id"])
            if result["status"] not in ("queued", "running"):
                return result
            if time.monotonic() >= deadline:
                raise RuntimeError(f"job timeout: {job['id']}")
            time.sleep(0.5)


class IntegrationM7(Demo):
    def __init__(self, args):
        super().__init__(args)
        # A 320x180 design extent; the raster grid follows --resolution.
        self.pixels = [3840, 2160] if args.resolution == "4k" else [320, 180]
        self.sx, self.sy = self.pixels[0] / 320, self.pixels[1] / 180
        self.cli_channel = Channel(self, "cli")
        self.mcp_channel = Channel(self, "mcp")
        self.project = self.cli_channel.path

    def px(self, frame, x, y):
        return frame["linear"][int(y * self.sy) * self.pixels[0] + int(x * self.sx)]

    def build(self, ch):
        tag = ch.transport
        document = {"id": PROJECT_ID, "schema_version": 1, "semantic_version": 1,
                    "name": "INTEGRATION-003 カット・字幕・色補正", "compositions": [], "curves": []}
        created = ch.call("project.create", project=str(ch.path), document=document)
        ch.revision = str(created["revision"])
        self.check(f"{tag}.project.create", created["project_id"] == PROJECT_ID,
                   project_id=created["project_id"])
        ch.edit("sequence.create", sequence={
            "id": SEQ, "extent": {"width": 320, "height": 180},
            "frame_rate": rational(24), "audio_rate": 48000,
            "working_space": "linear_rec709",
            "tracks": [{"id": TRACK_V, "kind": "video", "clips": []},
                       {"id": TRACK_A, "kind": "audio", "clips": []}]})
        ch.edit("clip.place", sequence=SEQ, track=TRACK_V, clip=solid_clip(CLIP_A, 0, 5, COLOR_A))
        ch.edit("clip.place", sequence=SEQ, track=TRACK_V, clip=solid_clip(CLIP_B, 5, 10, COLOR_B))
        ch.edit("clip.place", sequence=SEQ, track=TRACK_V, clip=solid_clip(CLIP_C, 10, 14, COLOR_C))
        ch.edit("clip.place", sequence=SEQ, track=TRACK_A, clip=tone_clip(CLIP_TONE, 0, 10))
        # Cut editing: trim, split, slip, then a wipe transition (needs its
        # overlap in the same transaction), then a ripple delete.
        ch.edit("clip.trim", sequence=SEQ, clip=CLIP_C, range=trange(10, 13))
        ch.apply(timeline("clip_split", sequence=SEQ, clip=CLIP_B,
                          time=rational(7), right_clip=CLIP_B2))
        ch.apply(timeline("clip_slip", sequence=SEQ, clip=CLIP_B2,
                          delta=rational(1), linked=False))
        slip = ch.query()
        slipped = next(c["clip"] for c in slip["clips"] if c["clip"]["id"] == CLIP_B2)
        # Split anchors B2's source window at 2; the +1 slip moves it to 3.
        self.check(f"{tag}.edit.slip", fraction(slipped["source_in"]) == 3,
                   source_in=slipped["source_in"])
        ch.apply(timeline("clip_stretch", sequence=SEQ, clip=CLIP_B, range=trange(4, 7)),
                 timeline("transition_set", sequence=SEQ, transition={
                     "outgoing": CLIP_A, "incoming": CLIP_B, "range": trange(4, 5),
                     "kind": "wipe", "params": {"wipe": {"direction": "left"}},
                     "version": 1}))
        ch.apply(timeline("ripple_delete", sequence=SEQ, tracks=[TRACK_V],
                          range=trange(7, 10), linked=False))
        # Captions: plan shows the authored commands, import applies them.
        plan_request = {"project": str(ch.path), "base_revision": ch.revision,
                        "sequence": SEQ, "track": TRACK_C, "format": "srt",
                        "content": SRT, "style": caption_style(self.font),
                        "cue_ids": [{"caption": caption, "clip": clip}
                                    for caption, clip in CUES]}
        plan = ch.call("captions.import_plan", **plan_request)
        caption_sets = [c["caption_set"]["caption"] for c in plan["commands"]
                        if "caption_set" in c]
        self.check(f"{tag}.captions.plan",
                   len(plan["commands"]) == 7 and len(caption_sets) == 3 and
                   plan["commands"][0]["timeline"]["track_append"]["track"]["kind"] == "caption",
                   commands=len(plan["commands"]))
        ch.caption_docs = {doc["id"]: doc for doc in caption_sets}
        imported = ch.call("captions.import", plan=plan_request, session_id=ch.session,
                           idempotency_key=ch.key("captions.import"))
        ch.revision = str(imported["revision"])
        # Caption-track editing: rewrite cue 1 text and trim cue 2's interval.
        edited = dict(ch.caption_docs[CUES[0][0]])
        edited["text"] = EDITED_CUE1
        ch.apply({"caption_set": {"caption": edited}})
        ch.edit("clip.trim", sequence=SEQ, clip=CUES[1][1],
                range=trange(Fraction(17, 4), Fraction(11, 2)))
        # Markers and the In/Out work area.
        ch.apply(timeline("marker_set", sequence=SEQ, marker={
                          "id": MARKER_SEQ, "time": rational(2), "color": "green",
                          "comment": "first cut"}),
                 timeline("marker_set", sequence=SEQ, clip=CLIP_A, marker={
                          "id": MARKER_CLIP, "time": rational(1), "color": "blue",
                          "comment": "clip note"}))
        ch.apply(timeline("marker_move", sequence=SEQ, marker=MARKER_SEQ, time=rational(3)))
        ch.apply(timeline("work_area_set", sequence=SEQ, work_area=trange(*WORK_AREA)))
        # Color correction: baseline pixels first, then exposure +1 on clip C.
        baseline = ch.frame(Fraction(35, 4))
        ch.apply(timeline("clip_set_effects", sequence=SEQ, clip=CLIP_C,
                          properties=[scalar_property(EXPOSURE_ID, "kronello.effect.exposure", 1.0),
                                      scalar_property(EXPOSURE_OFFSET_ID,
                                                      "kronello.effect.exposure_offset", 0.0)],
                          effects=[{"effect_id": "kronello.color.exposure", "version": 1,
                                    "parameters": {"kind": "color_exposure",
                                                   "exposure": EXPOSURE_ID,
                                                   "offset": EXPOSURE_OFFSET_ID}}]))
        corrected = ch.frame(Fraction(35, 4))
        before = self.px(baseline, 160, 20)
        after = self.px(corrected, 160, 20)
        self.check(f"{tag}.color.exposure_pixels",
                   after[3] == before[3] and all(
                       abs(a - b * 2) < 1e-3 for a, b in zip(after[:3], before[:3])),
                   before=before, after=after)
        self.check(f"{tag}.color.differs", before != after)
        self.structure(ch)
        self.caption_checks(ch)
        self.pixel_checks(ch)

    def structure(self, ch):
        tag = ch.transport
        result = ch.query()
        self.check(f"{tag}.revision", result["revision"] == ch.revision,
                   revision=result["revision"])
        sequence = result["sequence"]
        tracks = {t["id"]: t for t in sequence["tracks"]}
        self.check(f"{tag}.timeline.tracks",
                   [t["kind"] for t in sequence["tracks"]] == ["video", "audio", "caption"],
                   kinds=[t["kind"] for t in sequence["tracks"]])
        clips = {c["id"]: c for c in tracks[TRACK_V]["clips"]}
        expected = {CLIP_A: (0, 5), CLIP_B: (4, 7), CLIP_C: (7, 10)}
        ranges = {cid: (fraction(c["timeline_range"]["start"]),
                        fraction(c["timeline_range"]["end"]))
                  for cid, c in clips.items()}
        self.check(f"{tag}.timeline.cut_edits", ranges == expected,
                   ranges={k: [str(v) for v in value] for k, value in ranges.items()})
        self.check(f"{tag}.timeline.ripple", CLIP_B2 not in clips)
        transitions = sequence["transitions"]
        self.check(f"{tag}.timeline.transition",
                   len(transitions) == 1 and transitions[0]["outgoing"] == CLIP_A and
                   transitions[0]["incoming"] == CLIP_B and
                   transitions[0]["kind"] == "wipe" and
                   transitions[0]["params"] == {"wipe": {"direction": "left"}} and
                   fraction(transitions[0]["range"]["start"]) == 4 and
                   fraction(transitions[0]["range"]["end"]) == 5,
                   transitions=transitions)
        self.check(f"{tag}.timeline.markers",
                   sequence["markers"] == [{"id": MARKER_SEQ, "time": rational(3),
                                            "color": "green", "comment": "first cut"}] and
                   clips[CLIP_A]["markers"] == [{"id": MARKER_CLIP, "time": rational(1),
                                                 "color": "blue", "comment": "clip note"}],
                   markers=sequence["markers"])
        self.check(f"{tag}.timeline.work_area",
                   sequence["work_area"] == trange(*WORK_AREA),
                   work_area=sequence["work_area"])
        effects = clips[CLIP_C]["effects"]
        self.check(f"{tag}.color.effect_stored",
                   effects == [{"effect_id": "kronello.color.exposure", "version": 1,
                                "parameters": {"kind": "color_exposure",
                                               "exposure": EXPOSURE_ID,
                                               "offset": EXPOSURE_OFFSET_ID}}],
                   effects=effects)
        audio = tracks[TRACK_A]["clips"]
        self.check(f"{tag}.audio.clip",
                   len(audio) == 1 and audio[0]["source_ref"]["generator"] == "kronello.audio.tone440")
        caption_ranges = sorted((fraction(c["timeline_range"]["start"]),
                                 fraction(c["timeline_range"]["end"]))
                                for c in tracks[TRACK_C]["clips"])
        self.check(f"{tag}.captions.clips",
                   caption_ranges == [(Fraction(1, 2), Fraction(2)),
                                      (Fraction(17, 4), Fraction(11, 2)),
                                      (Fraction(8), Fraction(19, 2))],
                   caption_ranges=[[str(v) for v in r] for r in caption_ranges])
        self.check(f"{tag}.captions.kinds",
                   [c["kind"] for c in result["clips"] if c["track"] == TRACK_C] ==
                   ["caption"] * 3)

    def caption_checks(self, ch):
        tag = ch.transport
        exported = ch.call("captions.export", project=str(ch.path), sequence=SEQ, format="srt")
        ch.srt = exported["content"]
        self.check(f"{tag}.captions.round_trip", exported["content"] == EXPECTED_SRT,
                   content=exported["content"])
        vtt = ch.call("captions.export", project=str(ch.path), sequence=SEQ, format="vtt")
        self.check(f"{tag}.captions.vtt",
                   vtt["content"].startswith("WEBVTT\n\n") and
                   "00:00:04.250 --> 00:00:05.500" in vtt["content"] and
                   EDITED_CUE1 in vtt["content"], content=vtt["content"])

    def pixel_checks(self, ch):
        tag = ch.transport
        # Reference solid colors from probe pixels outside every caption cue.
        color_a = self.px(ch.frame(3), 160, 20)
        color_b = self.px(ch.frame(6), 160, 20)
        wipe = ch.frame(Fraction(9, 2))
        self.check(f"{tag}.transition.wipe_pixels",
                   self.px(wipe, 80, 20) == color_b and self.px(wipe, 240, 20) == color_a,
                   left=self.px(wipe, 80, 20), right=self.px(wipe, 240, 20),
                   expected=[color_b, color_a])
        # Burned-in captions: pixels inside a cue differ from the cue-free
        # frame of the same clip; ink lands in the bottom safe-area band.
        for name, t_cue, t_plain in (
                ("cue1", Fraction(1), Fraction(5, 2)),
                ("cue2", Fraction(5), Fraction(6)),
                ("cue3", Fraction(35, 4), Fraction(39, 4))):
            with_cue, plain = ch.frame(t_cue), ch.frame(t_plain)
            rows = [i // self.pixels[0] for i, (a, b) in
                    enumerate(zip(with_cue["linear"], plain["linear"])) if a != b]
            ink = [p for p in with_cue["linear"] if min(p[:3]) > 0.9]
            plain_ink = [p for p in plain["linear"] if min(p[:3]) > 0.9]
            self.check(f"{tag}.captions.burned.{name}",
                       len(rows) > 30 and rows and
                       min(rows) >= int(120 * self.sy) and len(ink) > len(plain_ink) + 20,
                       diff_pixels=len(rows), first_row=min(rows) if rows else None,
                       last_row=max(rows) if rows else None)

    def jobs(self, ch):
        tag = ch.transport
        frames_dest = self.output / f"{tag}-frames"
        frames_job = ch.wait_job(ch.submit(frames_dest, JOB_RATE, {"format": "image_sequence"}))
        self.check(f"{tag}.job.frames.succeeded",
                   frames_job["status"] == "succeeded" and
                   frames_job["completed_frames"] == 24 and
                   frames_job["result"]["validated"] is True,
                   status=frames_job["status"], error=frames_job.get("error"))
        manifest = json.loads((frames_dest / "sequence.json").read_text())
        times = [fraction(f["metadata"]["time"]) for f in manifest["frames"]]
        expected = [1 + Fraction(k, JOB_RATE) for k in range(24)]
        self.check(f"{tag}.job.frames.work_area_range",
                   manifest["range"] == trange(*WORK_AREA) and times == expected,
                   frame_count=len(times), first=str(times[0]) if times else None,
                   last=str(times[-1]) if times else None)
        expected_backend = ("wgpu_rgba16f" if self.args.backend == "gpu"
                            else "cpu_reference_float32")
        for i, frame in enumerate(manifest["frames"]):
            metadata = frame["metadata"]
            self.check(f"{tag}.job.frames.{i}.metadata",
                       metadata["region"]["pixels"] == self.pixels and
                       metadata["backend"] == expected_backend and
                       metadata["target"] == {"kind": "sequence", "sequence": SEQ})
            numeric = (frames_dest / frame["numeric"]["name"]).read_bytes()
            self.check(f"{tag}.job.frames.{i}.rgba16f",
                       len(numeric) == frame["numeric"]["bytes"] and
                       digest(numeric) == frame["numeric"]["sha256"] and
                       len(numeric) == self.pixels[0] * self.pixels[1] * 8)
            png = (frames_dest / frame["display"]["name"]).read_bytes()
            self.check(f"{tag}.job.frames.{i}.png",
                       png[:8] == b"\x89PNG\r\n\x1a\n" and
                       list(struct.unpack(">II", png[16:24])) == self.pixels and
                       digest(png) == frame["display"]["sha256"])
        # Job output equals an interactive render of the same time.
        probe = ch.frame(1)
        numeric = (frames_dest / manifest["frames"][0]["numeric"]["name"]).read_bytes()
        expected_numeric = b"".join(struct.pack("<e", v) for p in probe["linear"] for v in p)
        self.check(f"{tag}.job.frames.pixel_match", numeric == expected_numeric)
        ch.frame_manifest = manifest
        sidecar_dest = self.output / f"{tag}-captions.srt"
        sidecar_job = ch.wait_job(ch.submit(
            sidecar_dest, JOB_RATE,
            {"format": "caption_sidecar", "sequence": SEQ, "caption_format": "srt"}))
        self.check(f"{tag}.job.sidecar.succeeded", sidecar_job["status"] == "succeeded",
                   error=sidecar_job.get("error"))
        sidecar = sidecar_dest.read_bytes()
        self.check(f"{tag}.job.sidecar.content", sidecar == ch.srt.encode())
        movie_dest = self.output / f"{tag}-movie.mov"
        movie_job = ch.wait_job(ch.submit(
            movie_dest, MOVIE_RATE,
            {"format": "pro_res_mov", "profile_version": 3, "audio": "document",
             "clips": [], "background": [0, 0, 0]}))
        self.check(f"{tag}.job.movie.succeeded",
                   movie_job["status"] == "succeeded" and
                   movie_job["completed_frames"] == 144 and
                   movie_job["result"]["validated"] is True,
                   status=movie_job["status"], error=movie_job.get("error"))
        command = ["ffprobe", "-v", "error", "-show_streams", "-of", "json", str(movie_dest)]
        completed = subprocess.run(command, capture_output=True, text=True,
                                   env=self.env, timeout=60)
        probe = json.loads(completed.stdout)
        self.report["requests"].append({"transport": "inspection", "command": command,
                                       "exit": completed.returncode,
                                       "stderr": completed.stderr})
        streams = {s["codec_type"]: s for s in probe["streams"]}
        audio = streams.get("audio", {})
        self.check(f"{tag}.job.movie.streams",
                   streams.get("video", {}).get("codec_name") == "prores" and
                   [streams["video"]["width"], streams["video"]["height"]] == self.pixels and
                   audio.get("codec_name") == "pcm_s24le" and
                   audio.get("sample_rate") == "48000" and
                   abs(float(audio.get("duration", 0)) - 6) < 0.2,
                   streams=[{k: s.get("codec_name") for k, s in streams.items()}])
        command = ["ffmpeg", "-v", "info", "-i", str(movie_dest),
                   "-af", "astats", "-f", "null", "-"]
        completed = subprocess.run(command, capture_output=True, text=True,
                                   env=self.env, timeout=120)
        self.report["requests"].append({"transport": "inspection", "command": command,
                                       "exit": completed.returncode,
                                       "stderr": completed.stderr[-4000:]})
        match = re.search(r"RMS level dB:\s*(-?[\d.]+|-inf)", completed.stderr)
        rms = float(match.group(1)) if match else None
        self.check(f"{tag}.job.movie.audio_level", rms is not None and rms > -40,
                   rms_db=rms)

    def run(self):
        font_path = external_fixture_dir().resolve() / "NotoSansCJKjp-Regular.otf"
        pinned = self.cli("font.pin", path=str(font_path))
        self.font = pinned
        self.check("font.pin.sha256",
                   digest(font_path.read_bytes()) == pinned["sha256"])
        self.fonts = [{"identity": pinned, "path": str(font_path)}]
        # scene.query without a project is a typed rejection; it also proves the
        # MCP session is live before any project exists.
        self.compositions = [{"id": uid("missing-composition")}]
        self.start_mcp()
        capabilities = self.tool("capabilities.get")
        self.check("capabilities.features",
                   {"captions_v1", "caption_sidecar_v1", "clip_effects",
                    "audio_generator_v1", "document_audio", "movie_delivery_v1",
                    "generator_clip", "clip_split", "ripple"} <=
                   set(capabilities["features"]))
        mcp_font = self.mcp_channel.call("font.pin", path=str(font_path))
        self.check("parity.font.pin", mcp_font == pinned)
        for ch in (self.cli_channel, self.mcp_channel):
            self.build(ch)
        cli_q = self.cli_channel.query()
        mcp_q = self.mcp_channel.query()
        self.check("parity.sequence.query",
                   canonical_bytes(cli_q) == canonical_bytes(mcp_q),
                   cli_sha256=digest(canonical_bytes(cli_q)),
                   mcp_sha256=digest(canonical_bytes(mcp_q)))
        cli_doc = self.cli_channel.call("project.export", project=str(self.cli_channel.path))
        mcp_doc = self.mcp_channel.call("project.export", project=str(self.mcp_channel.path))
        self.check("parity.project.export",
                   cli_doc["revision"] == mcp_doc["revision"] and
                   canonical_bytes(cli_doc["document"]) == canonical_bytes(mcp_doc["document"]),
                   revision=cli_doc["revision"])
        for t in (Fraction(9, 2), Fraction(35, 4), Fraction(1)):
            self.check(f"parity.frame.{t}",
                       self.cli_channel.frame(t)["linear"] == self.mcp_channel.frame(t)["linear"])
        self.check("parity.captions.export",
                   self.cli_channel.srt == self.mcp_channel.srt)
        for ch in (self.cli_channel, self.mcp_channel):
            self.jobs(ch)
        cli_hashes = [f["numeric"]["sha256"] for f in self.cli_channel.frame_manifest["frames"]]
        mcp_hashes = [f["numeric"]["sha256"] for f in self.mcp_channel.frame_manifest["frames"]]
        self.check("parity.job.frames.hashes", cli_hashes == mcp_hashes,
                   frames=len(cli_hashes))
        self.check("parity.job.sidecar",
                   (self.output / "cli-captions.srt").read_bytes() ==
                   (self.output / "mcp-captions.srt").read_bytes())
        manifest = {
            "schema_version": 1,
            "projects": {t: str(ch.path) for t, ch in
                         (("cli", self.cli_channel), ("mcp", self.mcp_channel))},
            "revision": self.cli_channel.revision,
            "project_id": PROJECT_ID, "sequence": SEQ,
            "tracks": {"video": TRACK_V, "audio": TRACK_A, "caption": TRACK_C},
            "clips": {"a": CLIP_A, "b": CLIP_B, "c": CLIP_C, "tone": CLIP_TONE},
            "cue_clips": [clip for _, clip in CUES],
            "transition": {"outgoing": CLIP_A, "incoming": CLIP_B,
                           "range": [4, 5], "kind": "wipe", "direction": "left"},
            "markers": {"sequence": MARKER_SEQ, "clip": MARKER_CLIP},
            "work_area": [1, 7],
            "effect": {"clip": CLIP_C, "effect_id": "kronello.color.exposure",
                       "exposure": 1.0},
            "cue_texts": [EDITED_CUE1, "ワイプの <b>途中</b> です", "色補正クリップの字幕"],
            "cue_times": [[0.5, 2.0], [4.25, 5.5], [8.0, 9.5]],
            "fonts": self.fonts,
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
                        default=Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "debug")
    parser.add_argument("--backend", choices=("gpu", "cpu-reference"), default="gpu")
    parser.add_argument("--resolution", choices=("4k", "small"), default="small")
    parser.add_argument("--state-root", type=Path)
    args = parser.parse_args()
    demo = IntegrationM7(args)
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
