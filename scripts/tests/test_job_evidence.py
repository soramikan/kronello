"""Failure-path checks for JOB-002's outer process cleanup."""
import contextlib
import io
import json
import os
from pathlib import Path
import sys
import sqlite3
import subprocess
import tempfile
import time
import unittest
from unittest.mock import patch

from scripts import job_evidence


class JobEvidenceTests(unittest.TestCase):
    def test_bad_registry_and_database_do_not_skip_registered_workers(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            registry = root / "workers.jsonl"
            registry.write_text("broken JSON\n[]\n" + json.dumps({"pid": 123, "job": "known-job", "detach_mode": "in_parent_job"}) + "\n")
            (root / "jobs.sqlite3").write_bytes(b"broken database")
            opened = []
            connect = sqlite3.connect
            def tracked_connect(*args, **kwargs):
                db = connect(*args, **kwargs)
                opened.append(db)
                return db
            with patch.object(job_evidence, "probe_pid", return_value=False) as probe:
                with patch.object(job_evidence, "reap_adopted_pid"):
                    with patch.object(job_evidence.sqlite3, "connect", side_effect=tracked_connect):
                        result = job_evidence.cleanup_workers(registry, root)
            probe.assert_called_once_with(123)
            self.assertFalse(result["no_orphans"])
            self.assertEqual(len(result["cleanup_errors"]), 3)
            self.assertEqual(result["detach_modes"][0]["detach_mode"], "in_parent_job")
            self.assertEqual(len(opened), 1)
            with self.assertRaises(sqlite3.ProgrammingError):
                opened[0].execute("SELECT 1")

    def test_database_handles_close_on_success_and_bad_record(self):
        for valid in [False, True]:
            with self.subTest(valid=valid), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                path = root / "jobs.sqlite3"
                with contextlib.closing(sqlite3.connect(path)) as db:
                    db.execute("CREATE TABLE jobs(record TEXT)")
                    record = json.dumps({"id": "known-job", "worker_pid": 123}) if valid else "bad JSON"
                    db.execute("INSERT INTO jobs VALUES (?)", [record])
                    db.commit()
                log = root / "jobs" / "known-job" / "worker.log"
                log.parent.mkdir(parents=True)
                log.write_text('worker launch detach_mode: "in_parent_job"\n')
                opened = []
                connect = sqlite3.connect
                def tracked_connect(*args, **kwargs):
                    db = connect(*args, **kwargs)
                    opened.append(db)
                    return db
                with patch.object(job_evidence, "probe_pid", return_value=False):
                    with patch.object(job_evidence, "reap_adopted_pid"):
                        with patch.object(job_evidence.sqlite3, "connect", side_effect=tracked_connect):
                            result = job_evidence.cleanup_workers(root / "missing-registry", root)
                self.assertEqual(result["no_orphans"], valid)
                if valid:
                    self.assertEqual(result["detach_modes"][0]["detach_mode"], "in_parent_job")
                self.assertEqual(len(opened), 1)
                with self.assertRaises(sqlite3.ProgrammingError):
                    opened[0].execute("SELECT 1")
                # Exercise deletion before GC, with the closed connection
                # retained in opened; Windows rejects this if a handle leaked.
                path.unlink()

    def test_command_timeout_kills_and_waits_for_child(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            pid_file = root / "pid"
            command = [sys.executable, "-c",
                       "import os,time; from pathlib import Path; "
                       f"Path({str(pid_file)!r}).write_text(str(os.getpid())); time.sleep(60)"]
            with contextlib.redirect_stdout(io.StringIO()):
                result = job_evidence.run_command(command, root / "timeout.log", os.environ.copy(), timeout=2)
            self.assertTrue(result["timed_out"])
            self.assertNotEqual(result["exit"], 0)
            self.assertFalse(job_evidence.probe_pid(int(pid_file.read_text())))

    @unittest.skipUnless(sys.platform == "linux", "Linux subreaper requires Linux CI")
    def test_linux_cleanup_reaps_worker_after_harness_is_killed(self):
        job_evidence.adopt_workers()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            pid_file = root / "worker-pid"
            registry = root / "workers.jsonl"
            child_code = "import time; time.sleep(60)"
            parent_code = (
                "import subprocess,sys,time; from pathlib import Path; "
                f"c=subprocess.Popen([sys.executable,'-c',{child_code!r},'--job','known-job']); "
                f"Path({str(pid_file)!r}).write_text(str(c.pid)); time.sleep(60)"
            )
            parent = subprocess.Popen([sys.executable, "-c", parent_code])
            try:
                deadline = time.monotonic() + 10
                while not pid_file.exists() and time.monotonic() < deadline:
                    time.sleep(0.01)
                pid = int(pid_file.read_text())
                registry.write_text(json.dumps({"pid": pid, "job": "known-job"}) + "\n")
                parent.kill()
                parent.wait()
                result = job_evidence.cleanup_workers(registry, root)
                self.assertEqual(len(result["orphans_before_cleanup"]), 1)
                self.assertEqual(result["cleanup_errors"], [])
                self.assertFalse(result["no_orphans"])
                self.assertFalse(job_evidence.probe_pid(pid))
                with self.assertRaises(ChildProcessError):
                    os.waitpid(pid, os.WNOHANG)
            finally:
                parent.kill()
                parent.wait()
                if pid_file.exists():
                    registry.write_text(json.dumps({"pid": int(pid_file.read_text()), "job": "known-job"}) + "\n")
                    job_evidence.cleanup_workers(registry, root)


if __name__ == "__main__":
    unittest.main()
