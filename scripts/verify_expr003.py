#!/usr/bin/env python3
"""EXPR-003 実 CLI/MCP の保存・query・frame 相互運用。外部取得なし。"""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import queue
import shutil
import subprocess
import threading
import uuid


def uid():
    return str(uuid.uuid4())


def scalar(value):
    return {"kind": "scalar", "value": value}


def time(num, den=1):
    return {"num": str(num), "den": str(den)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/debug/kronello"))
    parser.add_argument("--mcp-binary", type=Path, default=Path("target/debug/kronello-mcp"))
    parser.add_argument("--output-root", type=Path, required=True)
    args = parser.parse_args()
    out = args.output_root.resolve()
    out.mkdir(parents=True, exist_ok=False)
    (out / "bin").mkdir()
    for source, name in [(args.binary, "kronello"), (args.mcp_binary, "kronello-mcp")]:
        shutil.copy2(source.resolve(), out / "bin" / name)
    binary, mcp_binary = out / "bin/kronello", out / "bin/kronello-mcp"
    report = {"status": "running", "checks": [], "frames": [], "errors": [],
              "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "mcp_binary_sha256": hashlib.sha256(mcp_binary.read_bytes()).hexdigest()}
    project = str(out / "expr.kronello")
    counter = 0
    rpc_id = 0
    process = None
    log = open(out / "mcp.stderr.log", "w")
    replies = queue.Queue()

    def cli(op, error=None, **payload):
        nonlocal counter
        counter += 1
        request = {"operation": op, **payload}
        completed = subprocess.run([str(binary), "--backend", "cpu-reference"],
                                   input=json.dumps(request), capture_output=True, text=True, timeout=120)
        (out / f"cli-{counter:03}.json").write_text(json.dumps({"request": request, "stdout": completed.stdout,
                                                               "stderr": completed.stderr, "exit": completed.returncode}))
        response = json.loads(completed.stdout)
        if error:
            assert response["status"] == "error" and response["error"]["code"] == error, response
            return response["error"]
        assert completed.returncode == 0 and response["status"] == "success", response
        return response["result"]["value"]

    def rpc(method, params):
        nonlocal rpc_id
        rpc_id += 1
        request = {"jsonrpc": "2.0", "id": rpc_id, "method": method, "params": params}
        process.stdin.write(json.dumps(request) + "\n")
        process.stdin.flush()
        response = json.loads(replies.get(timeout=120))
        assert response["id"] == rpc_id and "error" not in response, response
        (out / f"mcp-{rpc_id:03}.json").write_text(json.dumps({"request": request, "response": response}))
        return response["result"]

    def start_mcp():
        nonlocal process, replies
        replies = queue.Queue()
        process = subprocess.Popen([str(mcp_binary), "--backend", "cpu-reference"], stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=log, text=True)
        stream, mailbox = process.stdout, replies
        def read():
            for line in stream:
                mailbox.put(line)
        threading.Thread(target=read, daemon=True).start()
        rpc("initialize", {"protocolVersion": "2025-11-25", "capabilities": {},
                           "clientInfo": {"name": "expr003-acceptance", "version": "1"}})
        process.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
        process.stdin.flush()

    def stop_mcp():
        if process is not None:
            process.stdin.close()
            assert process.wait(timeout=30) == 0

    def mcp(op, error=None, **payload):
        response = rpc("tools/call", {"name": op, "arguments": payload})
        if error:
            assert response.get("isError") and response["structuredContent"]["error"]["code"] == error, response
            return response["structuredContent"]["error"]
        assert not response.get("isError"), response
        return response["structuredContent"]

    def both(op, **payload):
        left, right = cli(op, **payload), mcp(op, **payload)
        assert left == right, (op, left, right)
        return left

    def edit(expression, revision, key):
        plan = both("edit.plan", project=project, base_revision=str(revision), commands=[
            {"expression_set": {"expression": expression}},
            {"property_source_set": {"object": consumer, "property": opacity,
                "source": {"kind": "expression", "value": expression["id"]}, "curve": None}}])
        payload = {"project": project, "base_revision": plan["base_revision"], "commands": plan["commands"],
                   "plan_hash": plan["plan_hash"], "idempotency_key": key, "session_id": uid()}
        event = mcp("edit.apply", **payload)
        assert cli("edit.apply", **payload) == event
        assert mcp("edit.apply", **payload) == event
        return event["revision"]

    def rejection(expression, revision, code, label):
        before = both("project.export", project=project)
        payload = {"project": project, "base_revision": str(revision), "commands": [{"expression_set": {"expression": expression}}]}
        a, b = cli("edit.plan", error=code, **payload), mcp("edit.plan", error=code, **payload)
        assert a == b and before == both("project.export", project=project)
        report["errors"].append({"case": label, "code": code, "transaction_unchanged": True})

    try:
        start_mcp()
        document = json.loads(Path("examples/m1-demo.project.json").read_text())
        document["id"], document["name"] = uid(), "EXPR003 actual transports"
        c = document["compositions"][0]
        c["nodes"], c["root_nodes"] = c["nodes"][:1], c["root_nodes"][:1]
        document["texts"] = []
        composition, consumer = c["id"], c["nodes"][0]["id"]
        opacity = next(p["id"] for p in c["nodes"][0]["properties"] if p["descriptor"]["key"] == "kronello.opacity")
        source_id, source_property, source_expression = uid(), uid(), uid()
        upstream = copy.deepcopy(c["nodes"][0])
        upstream["id"], upstream["kind"] = source_id, {"kind": "null"}
        upstream["properties"] = [{"id": source_property, "descriptor": {"key": "kronello.opacity", "version": 1},
            "source": {"kind": "expression", "value": source_expression}, "modifiers": []}]
        c["nodes"].append(upstream)
        c["root_nodes"].append(source_id)
        document["expressions"] = [{"id": source_expression, "version": 3, "value_type": "scalar", "nodes": ["time"]}]
        table = {"columns": {"offset": "scalar"}, "rows": [{"offset": scalar(0.1)}]}
        content_hash = hashlib.sha256(json.dumps([1, table], sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        data_id = uid()
        document["expression_data_assets"] = [{"id": data_id, "version": 1, "content_hash": content_hash, "table": table}]
        (out / "fixture.json").write_text(json.dumps(document, ensure_ascii=False, indent=2))
        cli("project.create", project=project, document=document)
        both("project.export", project=project)
        expression = {"id": uid(), "version": 3, "value_type": "scalar", "nodes": ["time",
            {"literal": scalar(0.5)}, {"multiply": {"left": 0, "right": 1}},
            {"property_sample": {"node": source_id, "property": source_property, "value_type": "scalar", "lookback": 2}},
            {"literal": scalar(0.0)}, {"data_asset_cell": {"asset": data_id, "column": "offset", "row": 4, "value_type": "scalar"}},
            {"add": {"left": 3, "right": 5}}]}
        revision = edit(expression, 1, "expr003-past-table")
        query = {"project": project, "composition": composition,
                 "keys": [{"kind": "node", "instance_path": [], "node": consumer, "property": opacity}],
                 "times": [time(1, 2), time(0), time(1, 4), time(1, 2)]}
        sampled = both("property.sample", **query)
        assert sampled["samples"][0]["values"] == [scalar(0.35), scalar(0.1), scalar(0.225), scalar(0.35)], sampled
        report["checks"].append("Dynamic past sampling and typed table query match CLI/MCP at rational times")
        exports = both("project.export", project=project)
        stop_mcp()
        start_mcp()
        assert exports == both("project.export", project=project)
        assert sampled == both("property.sample", **query)
        report["checks"].append("Fresh CLI processes and restarted MCP reopen identical saved data/revision/hash")
        frame_input = {"project": project, "composition": composition,
                       "region": {"origin": [0, 0], "extent": [64, 32], "pixels": [64, 32]}}
        def frames(label):
            memo = {}
            for n in [2, 0, 1, 2]:
                frame = both("render.frame", input=frame_input, time=time(n, 4))
                assert frame["metadata"]["semantic_versions"]["expression"] == 3
                assert len(frame["linear"]) == len(frame["display"]) == 2048
                if n in memo:
                    assert frame == memo[n]
                memo[n] = frame
                if label == "past_table":
                    assert abs(max(p[3] for p in frame["linear"]) - (0.1 + n / 8)) < 1e-6
                report["frames"].append({"case": label, "time": time(n, 4), "pixels": 2048,
                    "all_linear_display_pixels_exact": True, "revision": frame["metadata"]["revision"],
                    "snapshot_hash": frame["metadata"]["snapshot_content_hash"],
                    "pins": frame["metadata"]["semantic_versions"],
                    "linear_sha256": hashlib.sha256(json.dumps(frame["linear"], separators=(",", ":")).encode()).hexdigest()})
        frames("past_table")
        invalid = copy.deepcopy(expression)
        invalid["version"] = 2
        rejection(invalid, revision, "UNSUPPORTED_FEATURE", "legacy_ast_rejects_new_nodes")
        invalid = copy.deepcopy(expression)
        invalid["nodes"][5]["data_asset_cell"]["asset"] = uid()
        rejection(invalid, revision, "EVALUATION_ERROR", "missing_DataAsset")
        noise = {"id": expression["id"], "version": 3, "value_type": "scalar", "nodes": ["time",
            {"continuous_noise": {"seed": 19, "element": 7, "input": 0}}, {"literal": scalar(0.5)},
            {"multiply": {"left": 1, "right": 2}}, {"literal": scalar(0.5)}, {"add": {"left": 3, "right": 4}}]}
        revision = edit(noise, revision, "expr003-noise")
        both("property.sample", **query)
        frames("continuous_noise")
        limited = copy.deepcopy(noise)
        limited["budget"] = {"instructions": 0, "memory_bytes": 1048576, "samples": 64, "nodes": 1024, "dependencies": 64}
        revision = edit(limited, revision, "expr003-budget")
        for op, payload in [("property.sample", query), ("render.frame", {"input": frame_input, "time": time(0)})]:
            a = cli(op, error="EXPRESSION_BUDGET_EXCEEDED", **payload)
            b = mcp(op, error="EXPRESSION_BUDGET_EXCEEDED", **payload)
            assert a == b
        report["errors"].append({"case": "actual_shared_budget", "code": "EXPRESSION_BUDGET_EXCEEDED", "query_and_final_frame": True})
        report["status"] = "pass"
        report["final_revision"] = revision
    except Exception as error:
        report["status"] = "fail"
        report["failure"] = repr(error)
        raise
    finally:
        if process is not None and process.poll() is None:
            stop_mcp()
        log.close()
        (out / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"status": report["status"], "frames": len(report["frames"]),
                      "errors": report["errors"], "report": str(out / "report.json")}, ensure_ascii=False))


if __name__ == "__main__":
    main()
