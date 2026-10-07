#!/usr/bin/env python3
"""REPEAT-001 実 CLI/MCP 操作・expand・Noise・Metal 同等性。"""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import queue
import subprocess
import shutil
import sys
import threading
import uuid


def fresh():
    return str(uuid.uuid4())


def prop(key, kind, value):
    return {"id": fresh(), "descriptor": {"key": key, "version": 1},
            "source": {"kind": "constant", "value": {"kind": kind, "value": value}}, "modifiers": []}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/debug/kronello"))
    parser.add_argument("--mcp-binary", type=Path, default=Path("target/debug/kronello-mcp"))
    parser.add_argument("--output-root", type=Path, required=True)
    args = parser.parse_args()
    out = args.output_root.resolve()
    out.mkdir(parents=True, exist_ok=False)
    binary, mcp_binary = args.binary.resolve(), args.mcp_binary.resolve()
    (out / "bin").mkdir()
    shutil.copy2(binary, out / "bin/kronello")
    shutil.copy2(mcp_binary, out / "bin/kronello-mcp")
    binary, mcp_binary = out / "bin/kronello", out / "bin/kronello-mcp"
    project = str(out / "repeat.kronello")
    report = {"status": "running", "project": project, "checks": [], "frames": [],
              "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "mcp_binary_sha256": hashlib.sha256(mcp_binary.read_bytes()).hexdigest()}
    if sys.platform == "darwin":
        hardware = json.loads(subprocess.check_output(["system_profiler", "SPDisplaysDataType", "-json"], text=True))
        report["gpu_host"] = [{key: device[key] for key in ["_name", "sppci_model", "spdisplays_metal", "spdisplays_mtlgpufamilysupport"] if key in device}
                              for device in hardware["SPDisplaysDataType"]]
    counter = 0

    def cli(op, backend="cpu-reference", expect_error=None, **payload):
        nonlocal counter
        counter += 1
        prefix = out / f"{counter:03}-{op}-{backend}"
        request = {"operation": op, **payload}
        Path(str(prefix) + ".request.json").write_text(json.dumps(request, ensure_ascii=False) + "\n")
        completed = subprocess.run([str(binary), "--backend", backend], input=json.dumps(request),
                                   capture_output=True, text=True, timeout=120)
        Path(str(prefix) + ".stdout.json").write_text(completed.stdout)
        Path(str(prefix) + ".stderr.log").write_text(completed.stderr)
        result = json.loads(completed.stdout)
        if expect_error:
            assert result["status"] == "error" and result["error"]["code"] == expect_error, result
            return result["error"]
        assert completed.returncode == 0 and result["status"] == "success", result
        return result["result"]["value"]

    log = open(out / "mcp.stderr.log", "w")
    process = subprocess.Popen([str(mcp_binary), "--backend", "cpu-reference"], stdin=subprocess.PIPE,
                               stdout=subprocess.PIPE, stderr=log, text=True)
    replies = queue.Queue()
    def read_replies():
        for line in process.stdout:
            replies.put(line)
    threading.Thread(target=read_replies, daemon=True).start()
    request_id = 0

    def rpc(method, params):
        nonlocal request_id
        request_id += 1
        request = {"jsonrpc": "2.0", "id": request_id, "method": method, "params": params}
        process.stdin.write(json.dumps(request) + "\n")
        process.stdin.flush()
        response = json.loads(replies.get(timeout=120))
        assert response["id"] == request_id and "error" not in response, response
        (out / f"mcp-{request_id:03}.json").write_text(json.dumps({"request": request, "response": response}, ensure_ascii=False) + "\n")
        return response["result"]

    def mcp(op, expect_error=None, **payload):
        result = rpc("tools/call", {"name": op, "arguments": payload})
        if expect_error:
            assert result.get("isError") and result["structuredContent"]["error"]["code"] == expect_error, result
            return result["structuredContent"]["error"]
        assert not result.get("isError"), result
        return result["structuredContent"]

    def shared_plan(op, **payload):
        left, right = cli(op, **payload), mcp(op, **payload)
        assert left == right, (op, left, right)
        return left

    def apply(plan, key):
        payload = {"project": project, "base_revision": plan["base_revision"], "commands": plan["commands"],
                   "plan_hash": plan["plan_hash"], "idempotency_key": key, "session_id": fresh()}
        applied = mcp("edit.apply", **payload)
        assert cli("edit.apply", **payload) == applied
        return applied

    def compare_frame(time, tag):
        payload = {"input": {"project": project, "composition": composition, "fonts": fonts,
                   "region": {"origin": [0, 0], "extent": [64, 64], "pixels": [64, 64]}}, "time": time}
        cpu, remote = cli("render.frame", **payload), mcp("render.frame", **payload)
        assert cpu == remote, tag
        gpu = cli("render.frame", backend="gpu", **payload)
        assert gpu["metadata"]["backend"] == "wgpu_rgba16f", gpu["metadata"]
        for key in ["revision", "snapshot_content_hash", "semantic_versions", "time"]:
            assert cpu["metadata"][key] == gpu["metadata"][key], (key, cpu["metadata"], gpu["metadata"])
        maxima = {}
        for surface in ["linear", "display"]:
            assert len(cpu[surface]) == len(gpu[surface]) == 4096
            error = max(abs(a - b) for left, right in zip(cpu[surface], gpu[surface]) for a, b in zip(left, right))
            assert error <= 0.002, (tag, surface, error)
            maxima[surface] = error
        record = {"tag": tag, "time": time, "revision": cpu["metadata"]["revision"],
                  "snapshot_content_hash": cpu["metadata"]["snapshot_content_hash"], "pixels": 4096,
                  "cpu_cli_mcp_exact": True, "cpu_metal_max_abs_includes_alpha": True, "cpu_metal_max_abs": maxima,
                  "backend": gpu["metadata"]["backend"], "gpu_semantic_versions": gpu["metadata"]["semantic_versions"],
                  "alpha_area": sum(p[3] for p in cpu["linear"])}
        report["frames"].append(record)
        return cpu

    try:
        rpc("initialize", {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "repeat001-acceptance", "version": "1"}})
        process.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
        process.stdin.flush()
        for fixture, tag, times in [
            ("examples/m5-repeat.project.json", "shared-noise", [(0, 1), (1, 2), (3, 4), (1, 2)]),
            ("examples/m5-repeat-template.project.json", "nested-template", [(1, 5), (1, 2), (4, 1), (39, 5)]),
        ]:
            document = json.loads(Path(fixture).read_text())
            project = str(out / (tag + ".kronello"))
            composition = document["compositions"][-1]["id"]
            fonts = []
            if document.get("texts"):
                identity = document["texts"][0]["styles"][0]["font"]
                fonts = [{"identity": identity, "path": str(Path("target/fixtures/external/NotoSansCJKjp-Regular.otf").resolve())}]
            cli("project.create", project=project, document=document)
            original = shared_plan("project.export", project=project)
            frames = [compare_frame({"num": str(n), "den": str(d)}, tag + "-before-" + str(n) + "-" + str(d)) for n, d in times]
            assert any(pixel[3] > 0 for frame in frames for pixel in frame["linear"])
            repeater = document["repeaters"][0]
            instance = repeater["instances"][min(1, len(repeater["instances"]) - 1)]
            if tag == "shared-noise":
                # The regular Property command addresses an authored stable instance placement.
                position = instance["properties"][0]
                command = {"property_source_set": {"object": instance["placement"], "property": position["id"], "source": {"kind": "constant", "value": {"kind": "vec2", "value": [30, 0]}}, "curve": None}}
                plan = shared_plan("edit.plan", project=project, base_revision=original["revision"], commands=[command])
                event = apply(plan, "repeat-control")
                moved = compare_frame({"num": "1", "den": "2"}, "per-instance-move")
                assert moved["linear"] != frames[1]["linear"]
                current = shared_plan("project.export", project=project)
                payload = {"project": project, "base_revision": current["revision"], "event_id": event["id"], "session_id": fresh(), "idempotency_key": "undo-control"}
                undone = mcp("edit.undo", **payload)
                assert cli("edit.undo", **payload) == undone
                assert shared_plan("project.export", project=project)["document"] == original["document"]
                report["checks"].append("stable instance property edits through identical CLI/MCP plans, durable replay and Undo")
            current = shared_plan("project.export", project=project)
            command = {"repeater_expand": {"repeater": repeater["id"], "instance": instance["id"], "expansion_id": fresh()}}
            plan = shared_plan("edit.plan", project=project, base_revision=current["revision"], commands=[command])
            event = apply(plan, tag + "-expand")
            expanded = shared_plan("project.export", project=project)
            after = expanded["document"]
            assert after["compositions"][:len(document["compositions"])] == document["compositions"]
            for key in ["templates", "template_instances"]:
                assert after.get(key, []) == document.get(key, [])
            expanded_instance = after["repeaters"][0]["instances"][min(1, len(repeater["instances"]) - 1)]
            assert expanded_instance["id"] == instance["id"] and expanded_instance["seed"] == instance["seed"]
            assert expanded_instance["expanded_source"]["noise_aliases"]
            for (n, d), before in zip(times, frames):
                result = compare_frame({"num": str(n), "den": str(d)}, tag + "-expanded-" + str(n) + "-" + str(d))
                assert result["linear"] == before["linear"] and result["display"] == before["display"], "expand preserves pixels and Noise at arbitrary time"
            report["checks"].append(tag + ": source/template/instance ID/seed unchanged and exact arbitrary-time pixels after explicit expand")
            payload = {"project": project, "base_revision": expanded["revision"], "event_id": event["id"], "session_id": fresh(), "idempotency_key": tag + "-undo-expand"}
            undone = mcp("edit.undo", **payload)
            assert cli("edit.undo", **payload) == undone
            assert shared_plan("project.export", project=project)["document"] == original["document"]
            bad = copy.deepcopy(repeater)
            bad["source"]["root"] = fresh()
            command = {"repeater_set": {"repeater": bad}}
            revision = cli("project.export", project=project)["revision"]
            payload = {"project": project, "base_revision": revision, "commands": [command]}
            left = cli("edit.plan", expect_error="REPEATER_SOURCE", **payload)
            right = mcp("edit.plan", expect_error="REPEATER_SOURCE", **payload)
            assert left == right
            assert cli("project.export", project=project)["revision"] == revision
            report["checks"].append(tag + ": identical typed refusal without mutation and full expand Undo")
        report["binary_sha256_after"] = hashlib.sha256(binary.read_bytes()).hexdigest()
        report["mcp_binary_sha256_after"] = hashlib.sha256(mcp_binary.read_bytes()).hexdigest()
        assert report["binary_sha256"] == report["binary_sha256_after"]
        assert report["mcp_binary_sha256"] == report["mcp_binary_sha256_after"]
        report["status"] = "pass"
    except Exception as error:
        report["status"], report["failure"] = "failed", str(error)
        raise
    finally:
        (out / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
        process.stdin.close()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.terminate()
            process.wait(timeout=10)
        log.close()


if __name__ == "__main__":
    main()
