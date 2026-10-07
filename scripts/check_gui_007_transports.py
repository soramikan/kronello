#!/usr/bin/env python3
"""Reproduce GUI-007 shared CLI/MCP contracts without building or using the GUI."""
import argparse
import copy
import hashlib
import json
import os
import queue
import shutil
import subprocess
import threading
from pathlib import Path
from uuid import uuid4

ROOT = Path(__file__).resolve().parents[1]
def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
def rational(n, d=1):
    return {"num": str(n), "den": str(d)}

class Evidence:
    def __init__(self, output, cli_binary=None, mcp_binary=None):
        self.output = output
        output.mkdir(parents=True, exist_ok=False)
        binary = output / "bin"
        binary.mkdir()
        for name, source in [("kronello", cli_binary or ROOT / "apps/macos/Libraries/kronello"), ("kronello-mcp", mcp_binary or ROOT / "target/debug/kronello-mcp")]:
            shutil.copy2(source, binary / name)
        self.binary = binary
        self.env = dict(os.environ, KRONELLO_STATE_ROOT=str(output / "state"))
        self.report = {"checks": [], "requests": [], "binaries": {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in binary.iterdir()}}
        self.log = (output / "mcp.stderr.log").open("w")
        self.mcp = subprocess.Popen([str(binary / "kronello-mcp"), "--backend", "cpu-reference"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.log, text=True, env=self.env)
        self.lines = queue.Queue()
        threading.Thread(target=lambda: [self.lines.put(line) for line in self.mcp.stdout], daemon=True).start()
        self.rpc_id = 0
        self.rpc("initialize", {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "gui007-evidence", "version": "1"}})
        self.mcp.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
        self.mcp.stdin.flush()
        self.session = str(uuid4())
    def check(self, name, condition):
        self.report["checks"].append({"name": name, "passed": bool(condition)})
        if not condition:
            raise AssertionError(name)
    def rpc(self, method, params):
        self.rpc_id += 1
        self.mcp.stdin.write(json.dumps({"jsonrpc": "2.0", "id": self.rpc_id, "method": method, "params": params}) + "\n")
        self.mcp.stdin.flush()
        response = json.loads(self.lines.get(timeout=120))
        assert response.get("id") == self.rpc_id and "error" not in response, response
        return response["result"]
    def call(self, transport, operation, **fields):
        request = {"operation": operation, **fields}
        if transport == "cli":
            process = subprocess.run([str(self.binary / "kronello"), "--backend", "cpu-reference"], input=json.dumps(request), text=True, capture_output=True, env=self.env, timeout=120)
            response = json.loads(process.stdout)
        else:
            result = self.rpc("tools/call", {"name": operation, "arguments": fields})
            response = result["structuredContent"]
            assert response == json.loads(result["content"][0]["text"])
            response = {"status": "error", **response} if result["isError"] else {"status": "success", "result": {"value": response}}
        entry = {"transport": transport, "request": request, "response": response}
        if operation == "render.frame" and response.get("status") == "success":
            entry = {"transport": transport, "request": request, "response_sha256": hashlib.sha256(canonical(response)).hexdigest(), "metadata": response["result"]["value"]["metadata"]}
        self.report["requests"].append(entry)
        return response
    def success(self, transport, operation, **fields):
        response = self.call(transport, operation, **fields)
        assert response["status"] == "success", response
        return response["result"]["value"]
    def export(self):
        a = self.success("cli", "project.export", project=self.project)
        b = self.success("mcp", "project.export", project=self.project)
        self.check("saved_export_parity_revision_" + a["revision"], a == b)
        self.revision = a["revision"]
        return a["document"]
    def edit(self, name, command, transport):
        fields = {"project": self.project, "base_revision": self.revision, "commands": [command]}
        a = self.success("cli", "edit.plan", **fields)
        b = self.success("mcp", "edit.plan", **fields)
        self.check(name + ".plan_parity", a == b)
        apply = dict(fields, plan_hash=a["plan_hash"], session_id=self.session, idempotency_key=str(uuid4()))
        event = self.success(transport, "edit.apply", **apply)
        replay = self.success("mcp" if transport == "cli" else "cli", "edit.apply", **apply)
        self.check(name + ".cross_transport_idempotent_retry", event == replay)
        document = self.export()
        self.check(name + ".one_event", int(self.revision) == int(fields["base_revision"]) + 1)
        return event, document
    def undo(self, name, event, expected_document):
        self.success("mcp", "edit.undo", project=self.project, base_revision=self.revision, session_id=self.session, idempotency_key=str(uuid4()), event_id=event["id"])
        self.check(name + ".undo_restores_document", self.export() == expected_document)
    def frame(self, name, time):
        fields = {"input": {"project": self.project, "target": {"kind": "sequence", "sequence": self.sequence}, "region": {"origin": [0, 0], "extent": [640, 480], "pixels": [64, 48]}, "fonts": []}, "time": time}
        a = self.success("cli", "render.frame", **fields)
        b = self.success("mcp", "render.frame", **fields)
        self.check(name + ".fixed_snapshot_cpu_frame_parity", a == b)
        self.check(name + ".revision_fence", str(a["metadata"]["revision"]) == self.revision)
        self.report.setdefault("frames", {})[name] = {"sha256": hashlib.sha256(canonical(a)).hexdigest(), "metadata": a["metadata"]}
        return a
    def run(self):
        subprocess.run(["python3", str(ROOT / "scripts/demo_gui_007.py"), "--lightweight", "--output-root", str(self.output / "fixture")], check=True, env=self.env)
        ids = json.loads((self.output / "fixture/identities.json").read_text())
        self.project, self.sequence = ids["project"], ids["sequence"]
        self.export()
        def timeline(kind, **fields):
            return {"timeline": {kind: {"sequence": self.sequence, **fields}}}
        def clip(document):
            return document["sequences"][0]["tracks"][1]["clips"][0]
        def prop(key, kind, value):
            return {"id": str(uuid4()), "descriptor": {"key": key, "version": 1}, "source": {"kind": "constant", "value": {"kind": kind, "value": value}}, "modifiers": []}
        document = self.export()
        before = copy.deepcopy(document)
        original = self.frame("original", rational(0))
        forward_last = self.frame("forward_last", rational(71, 24))
        opacity = prop("kronello.opacity", "scalar", 0.5)
        blend = prop("kronello.blend_mode", "enum", "multiply")
        sigma = prop("kronello.effect.sigma", "scalar", 2)
        parameters = [opacity, blend, sigma]
        effects = [{"effect_id": "kronello.gaussian_blur", "version": 2, "parameters": {"kind": "gaussian_blur", "sigma": sigma["id"]}}]
        event, document = self.edit("opacity_blend_effects", timeline("clip_set_effects", clip=ids["video_clip"], properties=parameters, effects=effects), "cli")
        changed = self.frame("opacity_blend_effects", rational(0))
        self.check("compositing_changes_actual_pixels", original["linear"] != changed["linear"])
        self.undo("opacity_blend_effects", event, before)
        for mode in ["normal", "multiply", "screen"]:
            before_mode = self.export()
            mode_property = prop("kronello.blend_mode", "enum", mode)
            mode_event, mode_document = self.edit("blend_" + mode, timeline("clip_set_effects", clip=ids["video_clip"], properties=[mode_property], effects=[]), "mcp")
            self.check("blend_" + mode + ".saved_enum", clip(mode_document)["properties"][0]["source"]["value"]["value"] == mode)
            self.frame("blend_" + mode, rational(0))
            self.undo("blend_" + mode, mode_event, before_mode)
        before_opacity = self.export()
        opacity_event, opacity_document = self.edit("opacity_only", timeline("clip_set_effects", clip=ids["video_clip"], properties=[opacity], effects=[]), "cli")
        opacity_frame = self.frame("opacity_only", rational(0))
        self.check("opacity_changes_actual_pixels", original["linear"] != opacity_frame["linear"])
        self.undo("opacity_only", opacity_event, before_opacity)
        shadow_sigma = prop("kronello.effect.sigma", "scalar", 2)
        shadow_offset = prop("kronello.effect.offset", "vec2", [8, 8])
        shadow_color = prop("kronello.effect.color", "color", {"space": "srgb", "components": {"r": 0, "g": 0, "b": 0, "alpha": 1}})
        shadow_opacity = prop("kronello.effect.opacity", "scalar", 0.5)
        shadow = {"effect_id": "kronello.drop_shadow", "version": 2, "parameters": {"kind": "drop_shadow", "sigma": shadow_sigma["id"], "offset": shadow_offset["id"], "color": shadow_color["id"], "opacity": shadow_opacity["id"]}}
        before_shadow = self.export()
        shadow_event, _ = self.edit("blur_and_shadow", timeline("clip_set_effects", clip=ids["video_clip"], properties=[sigma, shadow_sigma, shadow_offset, shadow_color, shadow_opacity], effects=effects + [shadow]), "mcp")
        self.frame("blur_and_shadow", rational(0))
        self.undo("blur_and_shadow", shadow_event, before_shadow)
        command = timeline("clip_time_set", clip=ids["video_clip"], source_in=rational(1, 24), time_map={"kind": "linear", "offset": rational(0), "speed": rational(1, 2)}, audio_retime="resample_v1", reverse_sampling=None)
        before = self.export()
        event, document = self.edit("fractional_source_speed", command, "mcp")
        self.check("exact_rational_saved", clip(document)["source_in"] == rational(1, 24) and clip(document)["time_map"]["speed"] == rational(1, 2))
        self.frame("fractional_source_speed", rational(1))
        self.undo("fractional_source_speed", event, before)
        before = self.export()
        event, document = self.edit("reverse", timeline("clip_time_set", clip=ids["video_clip"], source_in=rational(3), time_map={"kind": "linear", "offset": rational(0), "speed": rational(1)}, audio_retime="reverse_resample_v1", reverse_sampling="reverse_grid_v1"), "cli")
        self.check("reverse_policy_saved", clip(document)["reverse_sampling"] == "reverse_grid_v1")
        first_reverse = self.frame("reverse_first", rational(0))
        last_reverse = self.frame("reverse_last", rational(71, 24))
        self.check("reverse_first_last_actual_pixels_differ", first_reverse["linear"] != last_reverse["linear"])
        self.check("reverse_matches_forward_endpoint_pixels", first_reverse["linear"] == forward_last["linear"] and last_reverse["linear"] == original["linear"])
        self.undo("reverse", event, before)
        before = self.export()
        stale = {"project": self.project, "base_revision": self.revision, "commands": [timeline("track_state_set", track=ids["tracks"][1], state={"visible": False, "muted": False})]}
        stale_plan = self.success("cli", "edit.plan", **stale)
        event, document = self.edit("track_mute", timeline("track_state_set", track=ids["tracks"][2], state={"visible": True, "muted": True}), "mcp")
        attempt = dict(stale, plan_hash=stale_plan["plan_hash"], session_id=self.session, idempotency_key=str(uuid4()))
        a = self.call("cli", "edit.apply", **attempt)
        b = self.call("mcp", "edit.apply", **attempt)
        self.check("stale_both_transports_typed_revision_conflict", a["error"]["code"] == b["error"]["code"] == "REVISION_CONFLICT")
        self.check("stale_does_not_mutate", self.export() == document)
        self.undo("track_mute", event, before)
        event2, document2 = self.edit("explicit_fresh_retry_track_visible", stale["commands"][0], "cli")
        hidden = self.frame("hidden", rational(0))
        self.check("track_visibility_changes_actual_pixels", original["linear"] != hidden["linear"])
        self.undo("track_visible", event2, before)
        self.check("all_operations_restore_authored_document", self.export() == before)
        self.frame("restored", rational(0))
    def close(self):
        self.mcp.stdin.close()
        self.mcp.wait(timeout=30)
        self.log.close()
        (self.output / "report.json").write_bytes(canonical(self.report) + b"\n")

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-root", required=True, type=Path)
    args = parser.parse_args()
    evidence = Evidence(args.output_root.resolve())
    try:
        evidence.run()
    finally:
        evidence.close()
    print(json.dumps({"passed": len(evidence.report["checks"]), "report": str(evidence.output / "report.json")}))
if __name__ == "__main__":
    main()
