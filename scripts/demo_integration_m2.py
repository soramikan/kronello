#!/usr/bin/env python3
"""Run INTEGRATION-001 through real CLI/MCP processes, without private storage access."""
import argparse
from fractions import Fraction
import hashlib
import json
import math
import os
from pathlib import Path
import queue
import struct
import subprocess
import threading
import time
import uuid

from fixtures import external_fixture_dir

ROOT = Path(__file__).resolve().parents[1]


def rational(num, den=1):
    value = Fraction(num, den)
    return {"num": str(value.numerator), "den": str(value.denominator)}


def fraction(value):
    return Fraction(int(value["num"]), int(value["den"]))


def digest(data):
    return hashlib.sha256(data).hexdigest()


def summarize(value):
    if isinstance(value, dict):
        return {key: summarize(item) for key, item in value.items()}
    if isinstance(value, list):
        if len(value) > 256:
            return {"count": len(value), "sha256": digest(json.dumps(value, sort_keys=True).encode())}
        return [summarize(item) for item in value]
    return value


class Demo:
    def __init__(self, args):
        self.args = args
        self.output = args.output_directory.resolve()
        self.output.mkdir(parents=True, exist_ok=False)
        self.state = (args.state_root or self.output / "state").resolve()
        # An explicit fresh state root prevents user-state access and old job interference.
        self.state.mkdir(parents=True, exist_ok=False)
        self.env = dict(os.environ, KRONELLO_STATE_ROOT=str(self.state))
        for key in ("KRONELLO_TEST_JOB_GATE", "KRONELLO_TEST_JOB_CORRUPT_OUTPUT",
                    "KRONELLO_TEST_ADAPTER_UNAVAILABLE", "KRONELLO_JOB_SLOTS",
                    "KRONELLO_JOB_HEARTBEAT_MS", "KRONELLO_JOB_TIMEOUT_MS",
                    "KRONELLO_JOB_RETENTION_SECONDS"):
            self.env.pop(key, None)
        self.binary = args.binary_dir.resolve() / "kronello"
        self.mcp_binary = args.binary_dir.resolve() / "kronello-mcp"
        self.project = self.output / "lower-third.kronello"
        self.revision = "1"
        self.session = str(uuid.uuid4())
        self.report = {"schema_version": 1, "backend": args.backend,
                       "resolution": args.resolution, "state_root": str(self.state),
                       "requests": [], "checks": [], "status": "running"}
        self.mcp = None
        self.mcp_log = None
        self.rpc_id = 0

    def check(self, name, condition, **evidence):
        self.report["checks"].append({"name": name, "passed": bool(condition), **evidence})
        if not condition:
            raise RuntimeError(f"check failed: {name}: {evidence}")

    def cli(self, operation, expected_error=None, **payload):
        request = {"operation": operation, **payload}
        entry = {"transport": "cli", "request": request, "response": None}
        self.report["requests"].append(entry)
        started = time.monotonic()
        completed = subprocess.run([str(self.binary), "--backend", self.args.backend],
                                   input=json.dumps(request, ensure_ascii=False), text=True,
                                   capture_output=True, env=self.env, cwd=ROOT, timeout=600)
        entry.update(exit=completed.returncode, stderr=completed.stderr,
                     seconds=time.monotonic() - started)
        response = json.loads(completed.stdout)
        entry["response"] = summarize(response)
        if expected_error:
            self.check(operation + ":" + expected_error,
                       completed.returncode != 0 and response.get("error", {}).get("code") == expected_error)
            return response["error"]
        if completed.returncode != 0 or response.get("status") != "success":
            raise RuntimeError(f"{operation}: {response}")
        return response["result"]["value"]

    def edit(self, operation, **payload):
        result = self.cli(operation, project=str(self.project), base_revision=self.revision,
                          session_id=self.session, idempotency_key=operation + ":" + self.revision,
                          **payload)
        self.revision = str(result["revision"])
        return result

    def start_mcp(self):
        self.mcp_log = (self.output / "mcp.stderr.log").open("w")
        self.mcp = subprocess.Popen([str(self.mcp_binary), "--backend", self.args.backend],
                                    stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                    stderr=self.mcp_log, text=True, env=self.env, cwd=ROOT)
        self.lines = queue.Queue()

        def read():
            for line in self.mcp.stdout:
                self.lines.put(line)
            self.lines.put(None)

        self.reader = threading.Thread(target=read, daemon=True)
        self.reader.start()
        result = self.rpc("initialize", {"protocolVersion": "2025-11-25", "capabilities": {},
                                        "clientInfo": {"name": "integration-001", "version": "1"}})
        self.check("mcp.initialize", result["protocolVersion"] == "2025-11-25")
        notification = {"jsonrpc": "2.0", "method": "notifications/initialized"}
        self.mcp.stdin.write(json.dumps(notification) + "\n")
        self.mcp.stdin.flush()
        self.report["requests"].append({"transport": "mcp", "request": notification,
                                       "response": None})
        tools = self.rpc("tools/list", {})["tools"]
        names = {tool["name"] for tool in tools}
        self.check("mcp.tools.list", {"scene.query", "property.sample", "capabilities.get",
                                     "render.submit", "job.get"} <= names, count=len(names))
        self.tool("scene.query", expected_error="INVALID_REQUEST", composition=self.compositions[0]["id"])

    def rpc(self, method, params):
        self.rpc_id += 1
        request = {"jsonrpc": "2.0", "id": self.rpc_id, "method": method, "params": params}
        entry = {"transport": "mcp", "request": request, "response": None}
        self.report["requests"].append(entry)
        self.mcp.stdin.write(json.dumps(request, ensure_ascii=False) + "\n")
        self.mcp.stdin.flush()
        line = self.lines.get(timeout=600)
        if line is None:
            raise RuntimeError("MCP exited before responding")
        response = json.loads(line)
        entry["response"] = summarize(response)
        if response.get("id") != self.rpc_id or "error" in response:
            raise RuntimeError(f"MCP protocol error: {response}")
        return response["result"]

    def tool(self, name, expected_error=None, **arguments):
        result = self.rpc("tools/call", {"name": name, "arguments": arguments})
        response = result["structuredContent"]
        if json.loads(result["content"][0]["text"]) != response:
            raise RuntimeError("MCP text/structuredContent mismatch")
        if expected_error:
            self.check("mcp." + name + ":" + expected_error,
                       result["isError"] and response.get("error", {}).get("code") == expected_error)
            return response["error"]
        if result["isError"]:
            raise RuntimeError(f"MCP {name}: {response}")
        return response

    def render_input(self, composition=None, pixels=(320, 180)):
        target = {"composition": composition} if composition else {
            "target": {"kind": "sequence", "sequence": self.sequence}}
        return {"project": str(self.project), **target,
                "region": {"origin": [0, 0], "extent": [320, 180], "pixels": list(pixels)},
                "fonts": self.fonts}

    def frame(self, at):
        return self.cli("render.frame", input=self.render_input(), time=rational(at))

    def inspect(self, ordinal, text, color, duration):
        composition = self.compositions[ordinal]["id"]
        result = self.tool("scene.query", project=str(self.project), composition=composition,
                           expand_instances=True,
                           evaluation={"time": rational(0), "fonts": self.fonts})
        placement = result["nodes"][0]
        self.check(f"instance.{ordinal}.duration", fraction(placement["active_range"]["end"]) == duration)
        band = next(n["evaluated"] for n in result["nodes"] if n["key"]["node"] == self.band["id"])
        label = next(n["evaluated"] for n in result["nodes"] if n["key"]["node"] == self.label["id"])
        self.check(f"instance.{ordinal}.text", label["text"] == text, text=label["text"])
        key = {"kind": "node", "instance_path": [self.instances[ordinal]],
               "node": self.band["id"], "property": self.color_property}
        samples = self.tool("property.sample", project=str(self.project), composition=composition,
                            keys=[key, {**key, "property": self.size_property}],
                            times=[rational(0), rational(1)], fonts=self.fonts)
        self.check(f"instance.{ordinal}.color", samples["samples"][0]["values"] == [color, color])
        size = band["properties"][self.size_property]["value"]
        bounds = label["layout_bounds"]
        transform = label["world_transform"]
        corners = [(x, y) for x in (bounds["min"][0], bounds["max"][0])
                   for y in (bounds["min"][1], bounds["max"][1])]
        world = [[row[0] * x + row[1] * y + row[2] for row in transform] for x, y in corners]
        minimum = [min(p[i] for p in world) for i in (0, 1)]
        maximum = [max(p[i] for p in world) for i in (0, 1)]
        expected = [maximum[i] - minimum[i] + 2 * self.padding[i] for i in (0, 1)]
        position = band["properties"][self.position_property]["value"]
        self.check(f"instance.{ordinal}.band_follows_text",
                   all(abs(a - b) < 1e-8 for a, b in zip(size, expected)) and
                   all(abs(position[i] - minimum[i] + self.padding[i]) < 1e-8 for i in (0, 1)) and
                   samples["samples"][1]["values"] == [{"kind": "vec2", "value": size}] * 2,
                   size=size, position=position, text_bounds=bounds)
        self.check(f"instance.{ordinal}.shadow", len(band["effects"]) == 1 and
                   band["effects"][0]["DropShadow"]["offset"] == [6, 6])
        return {"band": band, "label": label, "placement": placement,
                "samples": samples["samples"]}

    def shadow(self, frame, inspected, name):
        band = inspected["band"]
        size = band["properties"][self.size_property]["value"]
        position = band["properties"][self.position_property]["value"]
        x = math.floor(position[0] + size[0] - 2)
        y = math.ceil(position[1] + size[1] + 2)
        # The shadow is an integer translation with sigma=0. This probe is
        # outside the source band and inside its translated opaque interior.
        def pixel(px, py):
            return frame["linear"][py * 320 + px]
        shadow = pixel(x, y)
        source = pixel(x - 6, y - 6)
        clear = pixel(x, y + 12)
        self.check(name, abs(source[3] - 1) < 1e-3 and
                   abs(shadow[3] - source[3] * 0.5 * 0.6) < 1e-3 and
                   max(abs(v) for v in shadow[:3]) < 1e-6 and clear == [0, 0, 0, 0],
                   probe=[x, y], source=source, shadow=shadow, no_shadow=clear)

    def wait_job(self, job):
        deadline = time.monotonic() + 3600
        while True:
            result = self.tool("job.get", job=job["id"])
            if result["status"] not in ("queued", "running"):
                return result
            if time.monotonic() >= deadline:
                raise RuntimeError(f"job timeout: {job['id']}")
            time.sleep(0.5)

    def validate_sequence(self, job, expected_frame):
        destination = Path(job["destination"])
        sequence = json.loads((destination / "sequence.json").read_text())
        expected_backend = "wgpu_rgba16f" if self.args.backend == "gpu" else "cpu_reference_float32"
        pixels = [3840, 2160] if self.args.resolution == "4k" else [320, 180]
        self.check("job.succeeded", job["status"] == "succeeded" and
                   job["completed_frames"] == 2 and job["result"]["validated"] is True)
        self.check("job.fixed_snapshot", job["revision"] == expected_frame["metadata"]["revision"] and
                   job["snapshot_hash"] == expected_frame["metadata"]["snapshot_content_hash"] and
                   self.revision != job["revision"], submitted_revision=job["revision"],
                   live_revision=self.revision, snapshot_hash=job["snapshot_hash"])
        self.check("job.frame_count", len(sequence["frames"]) == 2)
        for i, frame in enumerate(sequence["frames"]):
            metadata = frame["metadata"]
            self.check(f"job.frame.{i}.metadata", metadata["region"]["pixels"] == pixels and
                       metadata["backend"] == expected_backend and
                       metadata["revision"] == job["revision"] and
                       metadata["snapshot_content_hash"] == job["snapshot_hash"] and
                       fraction(metadata["time"]) == i * 8 and
                       metadata["target"] == {"kind": "sequence", "sequence": self.sequence})
            self.check(f"job.frame.{i}.metadata_file",
                       json.loads((destination / frame["metadata_file"]).read_text()) == metadata)
            for kind in ("numeric", "display"):
                artifact = frame[kind]
                data = (destination / artifact["name"]).read_bytes()
                self.check(f"job.frame.{i}.{kind}.hash",
                           len(data) == artifact["bytes"] and digest(data) == artifact["sha256"])
                if kind == "display":
                    width, height, depth, color = struct.unpack(">IIBB", data[16:26])
                    self.check(f"job.frame.{i}.png", data[:8] == b"\x89PNG\r\n\x1a\n" and
                               [width, height] == pixels and depth == 16 and color == 6)
                else:
                    self.check(f"job.frame.{i}.rgba16f", len(data) == pixels[0] * pixels[1] * 8)
                    baseline = expected_frame if i == 0 else self.b_frame
                    if self.args.resolution == "small":
                        numeric = b"".join(struct.pack("<e", v) for pixel in baseline["linear"] for v in pixel)
                        self.check(f"job.frame.{i}.frozen_pixels", data == numeric)
                    else:
                        # Sample an opaque band interior below the text ink.
                        # This proves fixed visual content as well as revision;
                        # the post-submit one-line text has no band at A's probe.
                        band = self.final_instances[i]["band"]["properties"]
                        position = band[self.position_property]["value"]
                        size = band[self.size_property]["value"]
                        x, y = math.ceil(position[0] + 10), math.floor(position[1] + size[1] - 1)
                        scale = pixels[0] // 320
                        offset = ((y * scale + scale // 2) * pixels[0] + x * scale + scale // 2) * 8
                        actual = struct.unpack_from("<4e", data, offset)
                        expected = baseline["linear"][y * 320 + x]
                        self.check(f"job.frame.{i}.frozen_pixel_probe",
                                   abs(expected[3] - 1) < 1e-3 and
                                   all(abs(a - b) < 1e-3 for a, b in zip(actual, expected)),
                                   design_pixel=[x, y], expected=expected, actual=actual)
        first = destination / sequence["frames"][0]["display"]["name"]
        # The LGPL build need not include the PNG decoder: dimensions come
        # from every PNG IHDR above, not ffprobe's optional decoded width.
        command = ["ffprobe", "-v", "error", "-show_streams", "-show_packets", "-of", "json", str(first)]
        completed = subprocess.run(command, capture_output=True, text=True, env=self.env, timeout=30)
        probe = json.loads(completed.stdout)
        self.report["requests"].append({"transport": "inspection", "command": command,
                                       "exit": completed.returncode, "response": probe,
                                       "stderr": completed.stderr})
        stream = probe["streams"][0]
        self.check("job.ffprobe_codec", completed.returncode == 0 and stream["codec_name"] == "png" and
                   len(probe["packets"]) == 1 and int(probe["packets"][0]["size"]) == first.stat().st_size,
                   codec=stream["codec_name"], reported_dimensions=[stream["width"], stream["height"]])

    def run(self):
        document = json.loads((ROOT / "examples/integration-001.project.json").read_text())
        definition = json.loads((ROOT / "examples/integration-001.definition.json").read_text())
        self.compositions = document["compositions"][:2]
        self.band, self.label = document["compositions"][2]["nodes"]
        binding = definition["constraints"]["bands"][0]
        self.size_property = binding["size_property"]
        self.position_property = binding["position_property"]
        self.color_property = definition["public_inputs"]["accent"]["target"]["property"]["property"]
        self.padding = binding["padding"]
        font = document["texts"][0]["styles"][0]["font"]
        self.fonts = [{"identity": font, "path": str(external_fixture_dir().resolve() / "NotoSansCJKjp-Regular.otf")}]
        self.check("font.lock", digest(Path(self.fonts[0]["path"]).read_bytes()) == font["sha256"])
        created = self.cli("project.create", project=str(self.project), document=document)
        self.revision = created["revision"]
        self.edit("template.define", definition=definition)
        self.instances = [str(uuid.uuid4()), str(uuid.uuid4())]
        a_text, b_text = "日本語", "別の字幕"
        a_color = definition["public_inputs"]["accent"]["default"]
        b_color = {"kind": "color", "value": {"space": "srgb", "components": {
            "r": 0.05, "g": 0.2, "b": 0.9, "alpha": 1.0}}}
        for i, (text, color, duration) in enumerate(((a_text, a_color, 5), (b_text, b_color, 6))):
            self.edit("template.instantiate", composition=self.compositions[i]["id"],
                      node=str(uuid.uuid4()), index=0,
                      instance={"id": self.instances[i], "definition_ref": definition["id"],
                                "version": definition["version"], "duration": rational(duration),
                                "inputs": {"headline": {"kind": "string", "value": text}, "accent": color}})
        self.sequence, track = str(uuid.uuid4()), str(uuid.uuid4())
        self.edit("sequence.create", sequence={"id": self.sequence,
                  "extent": {"width": 320, "height": 180}, "frame_rate": rational(24),
                  "audio_rate": 48000, "working_space": "linear_rec709",
                  "tracks": [{"id": track, "kind": "video", "clips": []}]})
        for i, (start, end) in enumerate(((0, 8), (8, 14))):
            self.edit("clip.place", sequence=self.sequence, track=track,
                      clip={"id": str(uuid.uuid4()), "source_ref": {"kind": "composition",
                            "composition": self.compositions[i]["id"]},
                            "timeline_range": {"start": rational(start), "end": rational(end)},
                            "source_in": rational(0), "time_map": {"kind": "linear", "offset": rational(0),
                            "speed": rational(1)}, "audio_retime": "reject", "links": [], "effects": []})
        self.start_mcp()
        capabilities = self.tool("capabilities.get")
        self.check("capabilities.shadow", "kronello.drop_shadow" in capabilities["effects"])
        before_a = self.inspect(0, a_text, a_color, 5)
        before_b = self.inspect(1, b_text, b_color, 6)
        before_frame = self.frame(0)
        self.b_frame = self.frame(8)
        self.shadow(before_frame, before_a, "shadow.5_seconds")
        self.edit("template_instance.retime", instance=self.instances[0], duration=rational(8))
        longer = "日本語の字幕\n背景帯が追従"
        changed_color = {"kind": "color", "value": {"space": "srgb", "components": {
            "r": 0.9, "g": 0.5, "b": 0.05, "alpha": 1.0}}}
        self.edit("template.set_input", instance=self.instances[0], name="headline",
                  value={"kind": "string", "value": longer})
        self.edit("template.set_input", instance=self.instances[0], name="accent", value=changed_color)
        after_a = self.inspect(0, longer, changed_color, 8)
        after_b = self.inspect(1, b_text, b_color, 6)
        self.final_instances = [after_a, after_b]
        frozen_frame = self.frame(0)
        after_b_frame = self.frame(8)
        self.check("independence.evaluated_values", before_b == after_b)
        self.check("independence.pixels", self.b_frame["linear"] == after_b_frame["linear"])
        self.check("band.longer_text", after_a["band"]["properties"][self.size_property]["value"][1] >
                   before_a["band"]["properties"][self.size_property]["value"][1])
        self.shadow(frozen_frame, after_a, "shadow.8_seconds")
        for name, value, duration in (("before", before_a, 5), ("after", after_a, 8)):
            points = value["placement"]["kind"]["value"]["local_time_map"]["points"]
            expected = [(0, 0), (Fraction(2, 5), Fraction(2, 5)),
                        (Fraction(duration) - Fraction(3, 10), Fraction(47, 10)), (duration, 5)]
            actual = [(fraction(p["parent"]), fraction(p["local"])) for p in points]
            self.check("protected_intervals." + name, actual == expected,
                       points=points, middle=str(Fraction(duration) - Fraction(7, 10)))
        exported = self.tool("project.export", project=str(self.project))
        self.check("one_directional.authoring", exported["document"]["texts"] == document["texts"] and
                   exported["document"]["compositions"][2] == document["compositions"][2])
        # Two absolute samples at 0s (A) and 8s (B) span the 14-second sequence.
        # Image sequences are an ADR-0050 output profile, with retained alpha.
        pixels = (3840, 2160) if self.args.resolution == "4k" else (320, 180)
        render = {"input": self.render_input(pixels=pixels),
                  "range": {"start": rational(0), "end": rational(14)},
                  "frame_rate": rational(1, 8), "output_directory": str(self.output / "fixed-frames")}
        submitted = self.tool("render.submit", render=render, output={"format": "image_sequence"})
        self.edit("template.set_input", instance=self.instances[0], name="headline",
                  value={"kind": "string", "value": "投入後に変更"})
        live = self.frame(0)
        self.check("job.post_submit_edit", live["linear"] != frozen_frame["linear"])
        finished = self.wait_job(submitted)
        if finished["status"] != "succeeded":
            raise RuntimeError(f"job failed: {finished}")
        self.validate_sequence(finished, frozen_frame)
        self.edit("template.set_input", instance=self.instances[0], name="headline",
                  value={"kind": "string", "value": "長すぎる字幕" * 40})
        overflow = {**render, "input": self.render_input(),
                    "output_directory": str(self.output / "overflow-frames")}
        failed = self.wait_job(self.tool("render.submit", render=overflow))
        self.check("overflow.typed_error", failed["status"] == "failed" and
                   failed["error"]["code"] == "TEMPLATE_OVERFLOW", error=failed.get("error"))
        self.check("overflow.no_published_output", not Path(overflow["output_directory"]).exists())
        self.report["job"] = finished
        self.report["overflow_job"] = failed
        self.report["status"] = "verified"

    def finish(self):
        try:
            if self.mcp:
                self.mcp.stdin.close()
                code = self.mcp.wait(timeout=30)
                self.reader.join(timeout=30)
                self.mcp_log.close()
                self.check("mcp.clean_exit", code == 0 and self.lines.get(timeout=5) is None, exit=code)
        except Exception as error:
            self.report["status"] = "failed"
            self.report["shutdown_error"] = str(error)
            raise
        finally:
            (self.output / "report.json").write_text(
                json.dumps(self.report, ensure_ascii=False, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-directory", required=True, type=Path)
    default_dir = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "debug"
    parser.add_argument("--binary-dir", type=Path, default=default_dir)
    parser.add_argument("--backend", choices=("gpu", "cpu-reference"), default="gpu")
    parser.add_argument("--resolution", choices=("4k", "small"), default="4k")
    parser.add_argument("--state-root", type=Path)
    args = parser.parse_args()
    demo = Demo(args)
    try:
        demo.run()
    except Exception as error:
        demo.report["status"] = "failed"
        demo.report["error"] = str(error)
        raise
    finally:
        demo.finish()
    print(json.dumps({"status": demo.report["status"], "checks": len(demo.report["checks"]),
                      "report": str(demo.output / "report.json")}, ensure_ascii=False))


if __name__ == "__main__":
    main()
