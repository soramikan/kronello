#!/usr/bin/env python3
"""Exercise a real native FFI process against real CLI/MCP processes."""
import argparse
import ctypes
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
SUFFIX = ".exe" if sys.platform == "win32" else ""
CLI = ROOT / f"target/debug/kronello{SUFFIX}"
MCP = ROOT / f"target/debug/kronello-mcp{SUFFIX}"


def native_child(path):
    name = "kronello_ffi.dll" if sys.platform == "win32" else ("libkronello_ffi.dylib" if sys.platform == "darwin" else "libkronello_ffi.so")
    native = ctypes.CDLL(str(ROOT / "target/debug" / name))
    native.kronello_open.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_size_t]
    native.kronello_open.restype = ctypes.c_uint64
    native.kronello_call.argtypes = [ctypes.c_uint64, ctypes.c_void_p, ctypes.c_size_t]
    native.kronello_call.restype = ctypes.c_uint64
    native.kronello_poll.argtypes = [ctypes.c_uint64]
    native.kronello_poll.restype = ctypes.c_void_p
    native.kronello_free.argtypes = [ctypes.c_void_p]
    native.kronello_close.argtypes = [ctypes.c_uint64]
    native.kronello_audio_prepare.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_bool), ctypes.POINTER(ctypes.c_void_p)]
    native.kronello_audio_prepare.restype = ctypes.c_void_p
    native.kronello_audio_render.argtypes = [ctypes.c_void_p, ctypes.c_int64, ctypes.c_size_t, ctypes.POINTER(ctypes.c_float), ctypes.POINTER(ctypes.c_void_p)]
    native.kronello_audio_render.restype = ctypes.c_bool
    native.kronello_audio_free.argtypes = [ctypes.c_void_p]
    encoded = str(path).encode()
    worker = str(CLI).encode()
    handle = native.kronello_open(encoded, len(encoded), worker, len(worker))
    assert handle

    def wait(request_id):
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            pointer = native.kronello_poll(handle)
            if pointer:
                try:
                    value = json.loads(ctypes.string_at(pointer))
                finally:
                    native.kronello_free(pointer)
                if value.get("request_id") == request_id:
                    return json.loads(value["response_json"])
            time.sleep(0.01)
        raise TimeoutError("native FFI completion")

    print(json.dumps(wait(0)), flush=True)
    for line in sys.stdin:
        if line.strip() == "close":
            native.kronello_close(handle)
            # Stay alive while the other process verifies asynchronous release.
            print('"closed"', flush=True)
            continue
        value = json.loads(line)
        if "audio" in value:
            encoded = json.dumps(value["audio"]).encode()
            audible, error = ctypes.c_bool(), ctypes.c_void_p()
            resource = native.kronello_audio_prepare(encoded, len(encoded), ctypes.byref(audible), ctypes.byref(error))
            if error.value:
                try:
                    response = json.loads(ctypes.string_at(error.value))
                finally:
                    native.kronello_free(error.value)
                print(json.dumps({"error": response}), flush=True)
                continue
            assert resource
            try:
                buffer = (ctypes.c_float * 512)()
                assert native.kronello_audio_render(resource, 0, 256, buffer, ctypes.byref(error))
                assert not error.value
                print(json.dumps({"has_audio": audible.value, "peak": max(map(abs, buffer))}), flush=True)
            finally:
                native.kronello_audio_free(resource)
            continue
        encoded = line.encode()
        request_id = native.kronello_call(handle, encoded, len(encoded))
        assert request_id
        print(json.dumps(wait(request_id)), flush=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--child", type=Path)
    args = parser.parse_args()
    if args.child:
        native_child(args.child)
        return
    evidence = ROOT / "target/ffi-002-evidence"
    evidence.mkdir(parents=True, exist_ok=True)
    report = {"platform": platform.platform(), "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True, cwd=ROOT).strip(), "checks": []}
    state = tempfile.TemporaryDirectory(prefix="kronello-ffi-state-")
    env = dict(os.environ, KRONELLO_STATE_ROOT=state.name)
    home = Path(os.environ.get("USERPROFILE") or os.environ["HOME"]).resolve()
    base = ROOT if ROOT.is_relative_to(home) else home
    fixture = tempfile.TemporaryDirectory(prefix="Dropbox - ffi-002-", dir=base)
    path = Path(fixture.name) / "safe.kronello"
    children = []
    request = {"operation": "project.info", "project": str(path)}

    def cli(value):
        result = subprocess.run([str(CLI), "--backend", "cpu-reference"], input=json.dumps(value), text=True, capture_output=True, env=env)
        response = json.loads(result.stdout)
        report["checks"].append({"entry": "cli", "operation": value["operation"], "exit": result.returncode, "response": response})
        return response

    def mcp():
        messages = [
            {"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "ffi-002", "version": "1"}}},
            {"jsonrpc": "2.0", "method": "notifications/initialized"},
            {"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "project.info", "arguments": {"project": str(path)}}},
        ]
        process = subprocess.Popen([str(MCP), "--backend", "cpu-reference"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=env)
        try:
            process.stdin.write(json.dumps(messages[0]) + "\n")
            process.stdin.flush()
            initialization = json.loads(process.stdout.readline())
            assert initialization["id"] == 0, initialization
            process.stdin.write("\n".join(map(json.dumps, messages[1:])) + "\n")
            process.stdin.flush()
            value = json.loads(process.stdout.readline())
            assert value["id"] == 1, value
            response = value["result"]
            process.stdin.close()
            exit_code = process.wait(timeout=20)
            report["checks"].append({"entry": "mcp", "exit": exit_code, "response": response})
            assert exit_code == 0
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=20)
        return response

    def child():
        process = subprocess.Popen([sys.executable, __file__, "--child", str(path)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, env=env)
        children.append(process)
        return process, json.loads(process.stdout.readline())

    def send(process, value):
        process.stdin.write(value + "\n")
        process.stdin.flush()
        return json.loads(process.stdout.readline())

    def unlocked():
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            response = cli(request)
            if response["status"] == "success":
                return
            assert response["error"]["code"] == "PROJECT_LOCKED", response
            time.sleep(0.02)
        raise TimeoutError("native close/kill failed to release project")

    try:
        document = json.loads((ROOT / "examples/ffi-preview.project.json").read_text())
        sequence = json.loads((ROOT / "examples/nle-001.project.json").read_text())["sequences"][0]
        sequence["id"] = "00830000-0000-4000-8000-000000000001"
        track = sequence["tracks"][0]
        track["kind"] = "audio"
        clip = track["clips"][0]
        clip["source_ref"] = {"kind": "generator", "generator": "kronello.audio.tone440", "version": 1, "color": {"space": "srgb", "components": {"r": 0, "g": 0, "b": 0, "alpha": 1}}}
        clip["timeline_range"] = {"start": {"num": "0", "den": "1"}, "end": {"num": "3", "den": "1"}}
        track["clips"] = [clip]
        sequence["tracks"] = [track]
        document["sequences"] = [sequence]
        assert cli({"operation": "project.create", "project": str(path), "document": document})["status"] == "success"
        holder, opened = child()
        assert opened["status"] == "success", opened
        assert opened["result"]["value"]["open_mode"] == "safe", opened
        assert send(holder, json.dumps(request))["status"] == "success"
        document["name"] = "Safe session edit"
        edit = {"operation": "project.import", "project": str(path), "base_revision": "1", "document": document}
        assert send(holder, json.dumps(edit))["result"]["value"]["revision"] == "2"
        assert send(holder, json.dumps(request))["result"]["value"]["revision"] == "2"
        report["checks"].append({"scenario": "native_shared_edit_and_query", "status": "passed"})
        audio_request = {"project": str(path), "target": {"kind": "sequence", "sequence": sequence["id"]}, "expected_revision": "2"}
        prepared = send(holder, json.dumps({"audio": audio_request}))
        assert prepared["has_audio"] and prepared["peak"] > 0.1, prepared
        audio_request["expected_revision"] = "1"
        assert send(holder, json.dumps({"audio": audio_request}))["error"]["code"] == "REVISION_CONFLICT"
        report["checks"].append({"scenario": "safe_session_binary_audio_capture_render_and_revision_fence", "status": "passed", "peak": prepared["peak"]})
        output = Path(state.name) / "safe-render-job"
        render = {"input": {"project": str(path), "composition": document["compositions"][0]["id"], "region": {"origin": [0, 0], "extent": [64, 32], "pixels": [8, 4]}},
                  "range": {"start": {"num": "0", "den": "1"}, "end": {"num": "1", "den": "1"}},
                  "frame_rate": {"num": "3", "den": "1"}, "output_directory": str(output)}
        submitted = send(holder, json.dumps({"operation": "render.submit", "render": render, "expected_revision": "2"}))
        assert submitted["status"] == "success", submitted
        job_id = submitted["result"]["value"]["id"]
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            job = cli({"operation": "job.get", "job": job_id})["result"]["value"]
            if job["status"] == "succeeded":
                break
            assert job["status"] in {"queued", "running"}, job
            time.sleep(0.05)
        assert output.is_dir(), job
        report["checks"].append({"scenario": "safe_session_detached_render_submit", "status": "passed"})
        assert cli(request)["error"]["code"] == "PROJECT_LOCKED"
        assert mcp()["structuredContent"]["error"]["code"] == "PROJECT_LOCKED"
        blocked, failed = child()
        assert failed["error"]["code"] == "PROJECT_LOCKED"
        assert send(holder, "close") == "closed"
        unlocked()
        report["checks"].append({"scenario": "close_while_native_process_remains_alive", "status": "passed"})
        # The failed GUI session must not silently adopt a transient open.
        assert send(blocked, json.dumps(request))["error"]["code"] == "PROJECT_LOCKED"
        report["checks"].append({"scenario": "failed_open_stays_failed_after_holder_close", "status": "passed"})
        holder, opened = child()
        assert opened["status"] == "success"
        assert cli(request)["error"]["code"] == "PROJECT_LOCKED"
        holder.kill()
        holder.wait(timeout=20)
        unlocked()
        report["checks"].append({"scenario": "native_process_kill_releases_lock", "status": "passed"})
        report["status"] = "passed"
    except BaseException:
        report["status"] = "failed"
        raise
    finally:
        for process in children:
            if process.poll() is None:
                process.kill()
            process.wait(timeout=20)
        (evidence / "evidence.json").write_text(json.dumps(report, indent=2) + "\n")
        fixture.cleanup()
        state.cleanup()


if __name__ == "__main__":
    main()
