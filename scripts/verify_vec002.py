#!/usr/bin/env python3
"""VEC-002 実 CLI/MCP 操作と Metal の画素同等性。外部取得は行わない。"""
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
    project = str(out / "vector.kronello")
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
        payload = {"input": {"project": project, "composition": composition,
                   "region": {"origin": [0, 0], "extent": [640, 320], "pixels": [256, 128]}}, "time": time}
        cpu, remote = cli("render.frame", **payload), mcp("render.frame", **payload)
        assert cpu == remote, tag
        gpu = cli("render.frame", backend="gpu", **payload)
        assert gpu["metadata"]["backend"] == "wgpu_rgba16f", gpu["metadata"]
        for key in ["revision", "snapshot_content_hash", "semantic_versions", "time"]:
            assert cpu["metadata"][key] == gpu["metadata"][key], (key, cpu["metadata"], gpu["metadata"])
        maxima = {}
        for surface in ["linear", "display"]:
            assert len(cpu[surface]) == len(gpu[surface]) == 32768
            assert all(left[3] == right[3] for left, right in zip(cpu[surface], gpu[surface])), (tag, surface, "alpha")
            error = max(abs(a - b) for left, right in zip(cpu[surface], gpu[surface]) for a, b in zip(left, right))
            assert error <= 0.001, (tag, surface, error)
            maxima[surface] = error
        record = {"tag": tag, "time": time, "revision": cpu["metadata"]["revision"],
                  "snapshot_content_hash": cpu["metadata"]["snapshot_content_hash"], "pixels": 32768,
                  "cpu_cli_mcp_exact": True, "cpu_metal_alpha_exact": True, "cpu_metal_max_abs": maxima,
                  "backend": gpu["metadata"]["backend"], "gpu_semantic_versions": gpu["metadata"]["semantic_versions"],
                  "alpha_area": sum(p[3] for p in cpu["linear"])}
        report["frames"].append(record)
        return cpu

    try:
        rpc("initialize", {"protocolVersion": "2025-11-25", "capabilities": {},
                           "clientInfo": {"name": "vec002-acceptance", "version": "1"}})
        process.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
        process.stdin.flush()
        document = json.loads(Path("examples/ffi-preview.project.json").read_text())
        document["id"], document["name"] = fresh(), "M5 vector acceptance"
        c = document["compositions"][0]
        prototype = copy.deepcopy(c["nodes"][0])
        prototype.update({"properties": [], "child_order": [], "containment_parent": None, "transform_parent": None})
        c.update({"nodes": [], "root_nodes": [], "design_extent": {"width": 640, "height": 320}})
        document.update({"curves": [], "shapes": [], "texts": []})
        composition = c["id"]
        created = cli("project.create", project=project, document=document)
        source_svg = "<svg><path d='M50 50C80 30 120 30 150 50L150 150L50 150Z' fill='#ef4444'/><path d='M360 80L540 80L540 240L360 240Z' fill='#22c55e'/></svg>"
        inspected = shared_plan("svg.inspect", svg=source_svg)
        assert inspected["unsupported"] == [] and inspected["external_references"] == []
        exported_svg = shared_plan("svg.export", paths=inspected["paths"])
        assert shared_plan("svg.inspect", svg=exported_svg["svg"]) == inspected
        report["checks"].append("Actual CLI/MCP SVG inspect/export roundtrip is identical")
        targets = []
        for i, name in enumerate(["Morph curve", "Trim closed loop"]):
            node = copy.deepcopy(prototype)
            shape = fresh()
            node.update({"id": fresh(), "name": name, "kind": {"kind": "shape", "value": {"content_ref": shape}}})
            targets.append({"shape": shape, "path_property": fresh(), "fill_property": fresh(), "node": node, "index": i})
        plan = shared_plan("svg.import_plan", project=project, base_revision=created["revision"],
                           composition=composition, svg=source_svg, targets=targets)
        apply(plan, "vec002-svg-import")
        assert cli("project.export", project=project) == mcp("project.export", project=project)
        report["checks"].append("SVG import plan/apply persists with exact CLI/MCP plan and idempotent replay")
        imported = compare_frame({"num": "0", "den": "1"}, "imported-svg")
        current = cli("project.export", project=project)
        shapes = {s["id"]: copy.deepcopy(s) for s in current["document"]["shapes"]}
        commands = []
        def insert(target, property):
            commands.append({"node_property_insert": {"composition": composition, "node": target["node"]["id"], "property": property}})
        target_svg = "<svg><path d='M50 50C100 10 200 10 250 50L250 150L50 150Z' fill='#ef4444'/></svg>"
        target_path = shared_plan("svg.inspect", svg=target_svg)["paths"][0]["path"]
        progress = prop("kronello.shape.morph_progress", "scalar", 0)
        destination = prop("kronello.shape.morph_target", "path", target_path)
        insert(targets[0], progress)
        insert(targets[0], destination)
        curve = {"id": fresh(), "value_type": "scalar", "interpolation_version": 1,
                 "keys": [{"time": {"num": str(i), "den": "1"}, "value": {"kind": "scalar", "value": i},
                           "interpolation": {"kind": "linear"}} for i in [0, 1]]}
        commands.append({"property_source_set": {"object": targets[0]["node"]["id"], "property": progress["id"],
                         "source": {"kind": "curve", "value": curve["id"]}, "curve": curve}})
        morph = shapes[targets[0]["shape"]]
        morph["geometry"] = {"kind": "morph_path", "value": {"from": targets[0]["path_property"], "to": destination["id"], "progress": progress["id"]}}
        commands.append({"shape_set": {"shape": morph}})
        trim_ids = []
        for key, value in [("trim_start", 0.19), ("trim_end", 0.81), ("trim_offset", -0.4)]:
            property = prop("kronello.shape." + key, "scalar", value)
            insert(targets[1], property)
            trim_ids.append(property["id"])
        trim = shapes[targets[1]["shape"]]
        trim["geometry"] = {"kind": "trimmed_path", "value": dict(zip(["path", "start", "end", "offset"], [targets[1]["path_property"], *trim_ids]))}
        commands.append({"shape_set": {"shape": trim}})
        plan = shared_plan("edit.plan", project=project, base_revision=current["revision"], commands=commands)
        event = apply(plan, "vec002-path-operations")
        final = cli("project.export", project=project)
        assert final == mcp("project.export", project=project)
        (out / "fixture.project.json").write_text(json.dumps(final["document"], ensure_ascii=False, indent=2) + "\n")
        report["fixture"] = {"composition": composition, "morph_node": targets[0]["node"]["id"],
                             "trim_node": targets[1]["node"]["id"], "progress_property": progress["id"],
                             "trim_properties": trim_ids, "revision": final["revision"]}
        images = []
        for num, den in [(0, 1), (1, 2), (1, 1), (1, 2)]:
            images.append(compare_frame({"num": str(num), "den": str(den)}, f"morph-trim-{num}-{den}"))
        assert images[1] == images[3], "arbitrary render order must be history independent"
        assert images[0]["linear"] != images[1]["linear"] != images[2]["linear"]
        report["checks"].append("Animated curved morph and negative-offset seam trim have exact fixed-snapshot CLI/MCP pixels; Metal matches every pixel")
        report["checks"].append("Arbitrary time order 0,1/2,1,1/2 repeats identical results")
        before = cli("project.export", project=project)
        invalid_path = shared_plan("svg.inspect", svg="<svg><path d='M0 0L1 1'/></svg>")["paths"][0]["path"]
        invalid_edit = {"project": project, "base_revision": before["revision"], "commands": [{"property_source_set": {
            "object": targets[0]["node"]["id"], "property": destination["id"],
            "source": {"kind": "constant", "value": {"kind": "path", "value": invalid_path}}, "curve": None}}]}
        assert cli("edit.plan", **invalid_edit, expect_error="PATH_MORPH_CORRESPONDENCE") == mcp("edit.plan", **invalid_edit, expect_error="PATH_MORPH_CORRESPONDENCE")
        refused = shared_plan("svg.inspect", svg="<svg><image href='https://invalid.example/never-fetched.svg'/></svg>")
        assert refused["external_references"] == ["https://invalid.example/never-fetched.svg"]
        external_import = {"project": project, "base_revision": before["revision"], "composition": composition,
            "svg": "<svg><image href='https://invalid.example/never-fetched.svg'/></svg>", "targets": []}
        assert cli("svg.import_plan", **external_import, expect_error="UNSUPPORTED_FEATURE") == mcp("svg.import_plan", **external_import, expect_error="UNSUPPORTED_FEATURE")
        assert cli("project.export", project=project) == before
        report["checks"].append("Correspondence/external-reference refusal is typed and leaves the document unchanged")
        # Undo the whole path-operation transaction and compare the imported SVG frame.
        cli("edit.undo", project=project, base_revision=before["revision"], event_id=event["id"],
            session_id=fresh(), idempotency_key="vec002-undo")
        undone = compare_frame({"num": "0", "den": "1"}, "undo-path-operations")
        assert undone["linear"] == imported["linear"] and undone["display"] == imported["display"]
        report["checks"].append("Real CLI Undo restores imported SVG pixels, with MCP and Metal parity")
        # Leave the final vector fixture as a separate editable project for GUI verification.
        gui_project = str(out / "vector-gui.kronello")
        cli("project.create", project=gui_project, document=final["document"])
        report["gui_project"] = gui_project
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
