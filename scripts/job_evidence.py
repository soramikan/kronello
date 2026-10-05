#!/usr/bin/env python3
"""JOB-002 real-process evidence and outer cleanup (also on failed commands)."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import platform
import signal
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def check_lints() -> None:
    workspace = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["lints"]
    native = tomllib.loads((ROOT / "crates/kronello-platform/Cargo.toml").read_text())["lints"]
    assert native["rust"]["unsafe_code"] == "deny"
    native["rust"]["unsafe_code"] = workspace["rust"]["unsafe_code"]
    assert native == workspace, "kronello-platform mirrored workspace lints drifted"


def probe_pid(pid: int) -> bool:
    if sys.platform == "win32":
        import ctypes
        from ctypes import wintypes
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        kernel.OpenProcess.restype = wintypes.HANDLE
        kernel.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
        kernel.CloseHandle.argtypes = [wintypes.HANDLE]
        handle = kernel.OpenProcess(0x100000, False, pid)  # SYNCHRONIZE
        if not handle:
            if ctypes.get_last_error() == 5:
                raise PermissionError(f"cannot inspect worker {pid}")
            return False
        try:
            result = kernel.WaitForSingleObject(handle, 0)
            if result not in (0, 258):
                raise OSError(f"cannot wait on worker {pid}: {result}")
            return result == 258
        finally:
            kernel.CloseHandle(handle)
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False


def worker_command(pid: int) -> str:
    if sys.platform == "win32":
        result = subprocess.run(
            ["powershell", "-NoProfile", "-Command",
             f"(Get-CimInstance Win32_Process -Filter 'ProcessId = {pid}').CommandLine"],
            capture_output=True, text=True, check=True, timeout=20,
        )
    else:
        result = subprocess.run(["ps", "-p", str(pid), "-o", "args="],
                                capture_output=True, text=True, timeout=20)
        if result.returncode != 0 and result.stderr:
            raise RuntimeError(result.stderr.strip())
    return result.stdout.strip()


def adopt_workers() -> None:
    if sys.platform == "linux":
        import ctypes
        libc = ctypes.CDLL(None, use_errno=True)
        libc.prctl.argtypes = [ctypes.c_int, ctypes.c_ulong, ctypes.c_ulong,
                               ctypes.c_ulong, ctypes.c_ulong]
        if libc.prctl(36, 1, 0, 0, 0) != 0:  # PR_SET_CHILD_SUBREAPER
            raise OSError(ctypes.get_errno(), "cannot adopt test-harness workers")


def reap_adopted_pid(pid: int) -> None:
    if sys.platform != "win32":
        try:
            os.waitpid(pid, os.WNOHANG)
        except ChildProcessError:
            pass


def cleanup_workers(registry: Path, scratch: Path) -> dict:
    workers = {}
    errors = []
    if registry.exists():
        for line in registry.read_text().splitlines():
            try:
                row = json.loads(line)
                workers[(row["pid"], row["job"])] = row
            except (ValueError, KeyError, TypeError) as error:
                errors.append(f"invalid registry entry: {error}")
    # A killed test harness may not have captured its last submit response.
    for db_path in scratch.rglob("jobs.sqlite3"):
        try:
            with sqlite3.connect(f"{db_path.as_uri()}?mode=ro", uri=True) as db:
                for (text,) in db.execute("SELECT record FROM jobs"):
                    row = json.loads(text)
                    if row["worker_pid"]:
                        pid = row["worker_pid"]
                        workers[(pid, row["id"])] = {"pid": pid, "job": row["id"]}
        except (sqlite3.Error, ValueError, KeyError, TypeError) as error:
            errors.append(f"cannot discover workers in {db_path}: {error}")
    orphans = []
    for row in workers.values():
        pid = row["pid"]
        try:
            reap_adopted_pid(pid)
            if not probe_pid(pid):
                continue
            command = worker_command(pid)
            # A recycled PID must never be killed. Only exact registered jobs.
            if "--job" not in command or row["job"] not in command:
                continue
            orphans.append({**row, "command": command})
            if sys.platform == "win32":
                subprocess.run(["taskkill", "/PID", str(pid), "/T", "/F"],
                               capture_output=True, check=True, timeout=20)
            else:
                os.kill(pid, signal.SIGKILL)
            deadline = time.monotonic() + 10
            while probe_pid(pid) and time.monotonic() < deadline:
                reap_adopted_pid(pid)
                time.sleep(0.05)
            reap_adopted_pid(pid)
            if probe_pid(pid):
                errors.append(f"worker not reaped: {pid}")
        except Exception as error:
            errors.append(f"pid={pid}: {error}")
    return {"registered_workers": len(workers), "orphans_before_cleanup": orphans,
            "cleanup_errors": errors, "no_orphans": not orphans and not errors}


def run_command(command: list[str], log: Path, env: dict, timeout: int = 600) -> dict:
    start = time.monotonic()
    print("COMMAND:", " ".join(command), flush=True)
    with log.open("w") as output:
        process = subprocess.Popen(command, cwd=ROOT, env=env, stdout=output,
                                   stderr=subprocess.STDOUT,
                                   start_new_session=sys.platform != "win32")
        timed_out = False
        try:
            code = process.wait(timeout=timeout)
        except (subprocess.TimeoutExpired, KeyboardInterrupt):
            timed_out = True
            if sys.platform == "win32":
                subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"],
                               capture_output=True, timeout=20)
            else:
                os.killpg(process.pid, signal.SIGKILL)
            code = process.wait()
    print(log.read_text(errors="replace"), end="", flush=True)
    return {"command": command, "exit": code, "timed_out": timed_out,
            "seconds": round(time.monotonic() - start, 3), "log": log.name}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "target/job-002-evidence")
    parser.add_argument("--check-lints", action="store_true")
    parser.add_argument("--stress", action="store_true")
    parser.add_argument("--cpu-hogs", type=int, default=0)
    parser.add_argument("--warmup-seconds", type=int, default=0)
    parser.add_argument("--raw-stress", action="store_true",
                        help="diagnostic unguarded SQLite baseline (failure is expected on affected SQLite)")
    args = parser.parse_args()
    check_lints()
    if args.check_lints:
        print("platform lint mirror matches workspace (unsafe_code exception only)")
        return 0
    args.output.mkdir(parents=True, exist_ok=True)
    report = {"os": platform.platform(), "machine": platform.machine(),
              "python": sys.version, "commands": [], "load_samples": [],
              "windows_scope": "jobs/platform deterministic payload; full CLI/MCP media port pending"}
    for name, command in [("rustc", ["rustc", "--version", "--verbose"]),
                          ("revision", ["git", "rev-parse", "HEAD"]),
                          ("status", ["git", "status", "--short"])]:
        report[name] = subprocess.check_output(command, cwd=ROOT, text=True).strip()
    registry = args.output.resolve() / "workers.jsonl"
    registry.write_text("")
    hogs = []
    stop = threading.Event()
    def sample_load() -> None:
        while not stop.is_set():
            if hasattr(os, "getloadavg"):
                report["load_samples"].append({"at": time.time(), "load": os.getloadavg()})
            stop.wait(1)
    sampler = threading.Thread(target=sample_load)
    sampler.start()
    failed = False
    with tempfile.TemporaryDirectory(prefix="kronello-job-evidence-") as scratch_name:
        scratch = Path(scratch_name)
        env = {**os.environ, "TMPDIR": str(scratch), "TEMP": str(scratch), "TMP": str(scratch),
               "KRONELLO_TEST_WORKER_REGISTRY": str(registry), "CARGO_BUILD_JOBS": "3"}
        commands = [
            [sys.executable, "-m", "unittest", "scripts.tests.test_job_evidence", "-v"],
            ["cargo", "test", "-p", "kronello-jobs", "--features", "test-worker", "--locked",
             "--test", "state", "--test", "processes", "--", "--nocapture"],
            ["cargo", "clippy", "-p", "kronello-platform", "-p", "kronello-jobs", "--all-targets",
             "--features", "kronello-jobs/test-worker", "--locked", "--", "-D", "warnings"],
        ]
        second_volume = env.get("KRONELLO_JOB_SECOND_VOLUME")
        if sys.platform == "linux" and not second_volume and Path("/dev/shm").is_dir():
            if Path("/dev/shm").stat().st_dev != scratch.stat().st_dev:
                second_volume = "/dev/shm"
                env["KRONELLO_JOB_SECOND_VOLUME"] = second_volume
        report["second_volume"] = second_volume or "pending: no distinct volume supplied"
        if second_volume:
            commands.append(["cargo", "test", "-p", "kronello-jobs", "--features", "test-worker", "--locked",
                             "--test", "processes", "cross_volume_is_typed_when_a_second_volume_is_supplied",
                             "--", "--ignored", "--exact"])
        if sys.platform != "win32":
            commands.extend([
                ["cargo", "test", "-p", "kronello-cli", "--test", "jobs", "--locked", "--", "--test-threads=16"],
                ["cargo", "test", "-p", "kronello-mcp", "--test", "stdio", "--locked",
                 "submitted_job_survives_mcp_eof_and_is_queryable_on_new_connection", "--", "--exact"],
            ])
        if args.raw_stress:
            commands = [["cargo", "build", "-p", "kronello-jobs", "--features", "test-worker",
                         "--bin", "kronello-job-test-worker", "--locked"]]
        try:
            adopt_workers()
            for index, command in enumerate(commands):
                result = run_command(command, args.output / f"command-{index}.log", env)
                report["commands"].append(result)
                failed |= result["exit"] != 0
            if args.raw_stress and not failed:
                command = ["cargo", "run", "-p", "kronello-jobs", "--features", "test-worker",
                           "--bin", "kronello-job-test-worker", "--locked", "--", "raw-stress"]
                for index in range(3):
                    baseline_env = {**env, "KRONELLO_STATE_ROOT": str(scratch / f"raw-{index}")}
                    result = run_command(command, args.output / f"raw-{index}.log", baseline_env, timeout=30)
                    report["commands"].append(result)
                    failed |= result["exit"] != 0
                    if result["timed_out"]:
                        break
            if args.stress and sys.platform != "win32":
                for _ in range(args.cpu_hogs):
                    hogs.append(subprocess.Popen([sys.executable, "-c", "while True: pass"],
                                                 stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
                if args.warmup_seconds:
                    print(f"Warming CPU load for {args.warmup_seconds}s with {args.cpu_hogs} processes", flush=True)
                    stop.wait(args.warmup_seconds)
                results = [None, None]
                command = ["cargo", "test", "-p", "kronello-cli", "--test", "jobs", "--locked", "--", "--test-threads=32"]
                def suite(index: int) -> None:
                    results[index] = run_command(command, args.output / f"stress-{index}.log", env)
                threads = [threading.Thread(target=suite, args=(i,)) for i in range(2)]
                for thread in threads: thread.start()
                for thread in threads: thread.join()
                report["commands"].extend(results)
                failed |= any(result is None or result["exit"] != 0 for result in results)
        except (Exception, KeyboardInterrupt) as error:
            failed = True
            report["runner_error"] = str(error)
        finally:
            for hog in hogs:
                hog.kill()
                hog.wait()
            stop.set()
            sampler.join()
            try:
                report["cleanup"] = cleanup_workers(registry, scratch)
            except Exception as error:
                report["cleanup"] = {"no_orphans": False, "cleanup_errors": [str(error)]}
            failed |= not report["cleanup"]["no_orphans"]
            report["exit"] = int(failed)
            (args.output / "evidence.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"exit": int(failed), "cleanup": report["cleanup"]}), flush=True)
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
