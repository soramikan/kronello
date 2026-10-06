#!/usr/bin/env python3
"""Observe a live release history harness without claiming logical bytes are I/O."""
import argparse
import ctypes
import json
import hashlib
import os
import platform
import subprocess
import sqlite3
import threading
import time
from pathlib import Path


def observer(pid):
    if platform.system() == "Darwin":
        # Xcode SDK sys/resource.h, rusage_info_v4 (RUSAGE_INFO_V4 == 4).
        fields = "user_time system_time pkg_idle_wkups interrupt_wkups pageins wired_size resident_size phys_footprint proc_start_abstime proc_exit_abstime child_user_time child_system_time child_pkg_idle_wkups child_interrupt_wkups child_pageins child_elapsed_abstime diskio_bytesread diskio_byteswritten cpu_time_qos_default cpu_time_qos_maintenance cpu_time_qos_background cpu_time_qos_utility cpu_time_qos_legacy cpu_time_qos_user_initiated cpu_time_qos_user_interactive billed_system_time serviced_system_time logical_writes lifetime_max_phys_footprint instructions cycles billed_energy serviced_energy interval_max_phys_footprint runnable_time".split()
        class Usage(ctypes.Structure):
            _fields_ = [("uuid", ctypes.c_uint8 * 16)] + [(name, ctypes.c_uint64) for name in fields]
        library = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
        library.proc_pid_rusage.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_void_p]
        library.proc_pid_rusage.restype = ctypes.c_int
        def read():
            usage = Usage()
            if library.proc_pid_rusage(pid, 4, ctypes.byref(usage)):
                raise OSError(ctypes.get_errno(), "proc_pid_rusage")
            return {"physical_read_bytes": usage.diskio_bytesread,
                    "physical_write_bytes": usage.diskio_byteswritten,
                    "resident_bytes": usage.resident_size,
                    "physical_footprint_bytes": usage.phys_footprint}
        return read
    if platform.system() == "Linux":
        def read():
            io = dict(line.split(": ") for line in Path(f"/proc/{pid}/io").read_text().splitlines())
            status = Path(f"/proc/{pid}/status").read_text().splitlines()
            rss = next(int(line.split()[1]) * 1024 for line in status if line.startswith("VmRSS:"))
            return {"physical_read_bytes": int(io["read_bytes"]),
                    "physical_write_bytes": int(io["write_bytes"]), "resident_bytes": rss}
        return read
    raise RuntimeError("OS process physical I/O observer not implemented on this platform")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary")
    parser.add_argument("--build", action="store_true")
    parser.add_argument("--kind", choices=["history", "render", "temporal"], default="history")
    parser.add_argument("--font", default="target/fixtures/external/NotoSansCJKjp-Regular.otf")
    parser.add_argument("--project", default="target/m3-acceptance/integration-metal/lower-third.kronello")
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    output = Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=False)
    source_files = [Path("Cargo.toml"), Path("Cargo.lock"), Path("rust-toolchain.toml"), Path(__file__)]
    for crate in Path("crates").iterdir():
        source_files.extend(path for directory in (crate / "src", crate / "native") if directory.exists() for path in directory.rglob("*") if path.is_file() and path.suffix in (".rs", ".c", ".h", ".m", ".mm", ".wgsl", ".metal"))
        source_files.extend(path for path in (crate / "Cargo.toml", crate / "build.rs") if path.is_file())
    source_files.extend(Path("crates/kronello-service/examples").glob("perf_001_*.rs"))
    source_hashes = {str(path): hashlib.sha256(path.read_bytes()).hexdigest() for path in sorted(source_files)}
    source_id = hashlib.sha256(json.dumps(source_hashes, sort_keys=True).encode()).hexdigest()
    (output / "source-manifest.json").write_text(json.dumps(source_hashes, indent=2) + "\n")
    if args.build:
        environment = dict(os.environ, KRONELLO_PERF_SOURCE_ID=source_id)
        with (output / "build.log").open("w") as log:
            subprocess.run(["cargo", "build", "--release", "-p", "kronello-service", "--example", f"perf_001_{args.kind}", "--locked"], check=True, env=environment, stdout=log, stderr=subprocess.STDOUT)
    if any(hashlib.sha256(path.read_bytes()).hexdigest() != source_hashes[str(path)] for path in source_files):
        raise RuntimeError("production source changed during release build")
    stderr = (output / "stderr.log").open("w")
    command = [str(Path(args.binary or f"target/release/examples/perf_001_{args.kind}").resolve()), str(Path(args.project).resolve()), str(output / "workload")]
    if args.kind == "render":
        command.append(str(Path(args.font).resolve()))
    child = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=stderr, text=True)
    read = observer(child.pid)
    phases = {}
    current = None
    baseline = None
    stop = threading.Event()
    guard = threading.Lock()
    errors = []
    def sample():
        while not stop.wait(0.01):
            try:
                values = read()
                with guard:
                    if current is not None:
                        for field in ("resident_bytes", "physical_footprint_bytes"):
                            if field in values:
                                key = "sampled_peak_" + field
                                phases[current][key] = max(phases[current].get(key, 0), values[field])
            except OSError as error:
                if child.poll() is None:
                    errors.append(str(error))
                return
    thread = threading.Thread(target=sample, daemon=True)
    thread.start()
    report = None
    completed_cases = {}
    try:
        for line in child.stdout:
            value = json.loads(line)
            if value["event"] == "case_report":
                completed_cases[value["name"]] = value["measurements"]
                (output / "completed-cases.json").write_text(json.dumps(completed_cases, indent=2) + "\n")
                continue
            if value["event"] == "phase_start":
                with guard:
                    current = value["phase"]
                    phases[current] = {}
                    baseline = read()
            elif value["event"] == "phase_end":
                with guard:
                    ending = read()
                    phases[current].update({field: ending[field] - baseline[field] for field in ("physical_read_bytes", "physical_write_bytes")})
                    current = None
                print(f"completed {value['phase']}", flush=True)
            elif value["event"] == "report":
                report = value
                continue
            child.stdin.write("continue\n")
            child.stdin.flush()
        if child.wait() != 0 or errors or report is None:
            raise RuntimeError(f"benchmark failed: exit={child.returncode}, observer_errors={errors}; see {output / 'stderr.log'}")
    finally:
        stop.set()
        thread.join()
        if child.poll() is None:
            child.kill()
            child.wait()
        stderr.close()
    if any(hashlib.sha256(path.read_bytes()).hexdigest() != source_hashes[str(path)] for path in source_files):
        raise RuntimeError("production/harness source changed during benchmark; measurements not accepted")
    if report.get("compiled_source_id") != source_id:
        raise RuntimeError("binary source identity does not match manifest; rebuild with --build")
    binary = Path(command[0])
    report["binary_sha256"] = hashlib.sha256(binary.read_bytes()).hexdigest()
    report["source_manifest_sha256"] = source_id
    if args.kind == "history":
        # Post-run readonly SQL payload accounting is separate from child I/O.
        database = output / "workload" / "history.kronello"
        with sqlite3.connect(f"file:{database}?mode=ro", uri=True) as connection:
            snapshots = connection.execute("SELECT count(*), coalesce(sum(length(CAST(document AS BLOB))),0) FROM snapshots").fetchone()
            events = connection.execute("SELECT mutations, inverse FROM events").fetchall()
        root_mutation_bytes = 0
        root_events = 0
        for mutations, _ in events:
            decoded = json.loads(mutations)
            if any(item.get("path") == [] for item in decoded):
                root_events += 1
                root_mutation_bytes += len(mutations.encode())
        report["sqlite_payload_accounting"] = {
            "snapshot_count": snapshots[0], "snapshot_document_bytes": snapshots[1],
            "event_count": len(events),
            "mutations_bytes": sum(len(item[0].encode()) for item in events),
            "inverse_bytes": sum(len(item[1].encode()) for item in events),
            "root_replacement_event_count": root_events,
            "root_replacement_mutations_bytes": root_mutation_bytes,
            "scope": "stored UTF-8 payloads, not physical I/O or full database pages",
        }
    report.update({"os": platform.platform(), "os_process_measurements": phases,
                   "sampling_interval_seconds": 0.01,
                   "memory_measurement": "sampled OS RSS/physical footprint; not exact allocation peak",
                   "physical_io_scope": "per-process OS diskio counters; cached reads can legitimately be zero; no SQLite VFS syscall attribution",
                   "sqlite_vfs_bytes": None,
                   "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
                   "dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], text=True).strip())})
    (output / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(output / "report.json")


if __name__ == "__main__":
    main()
