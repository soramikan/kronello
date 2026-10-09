"""Shared, fail-closed macOS distribution inventory and Mach-O checks."""
import hashlib
import json
import os
from pathlib import Path
import platform
import stat
import subprocess
import threading
from datetime import datetime, timezone

ROOT = Path(__file__).resolve().parents[1]
FFMPEG = {"avutil": 61, "avcodec": 63, "avformat": 63, "swscale": 10, "swresample": 7}
LIBRARIES = [f"lib{name}.{major}.dylib" for name, major in FFMPEG.items()] + [
    "libSvtAv1Enc.4.dylib", "libdav1d.7.dylib", "libopus.0.dylib", "libraw_r.25.dylib"]
EXECUTABLES = ["bin/kronello", "bin/kronello-mcp", "tools/release_roundtrip"]


def sha256(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def write_json(path, value):
    Path(path).write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def metadata():
    return {"platform": platform.platform(), "machine": platform.machine(),
            "utc": datetime.now(timezone.utc).isoformat()}


class Runner:
    def __init__(self):
        self.commands = []

    def run(self, command, *, cwd=None, env=None, input=None, allowed=(0,), timeout=180):
        command = list(map(str, command))
        cwd = str(Path(cwd).resolve()) if cwd else os.getcwd()
        try:
            result = subprocess.run(command, cwd=cwd, env=env, input=input, text=True,
                                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout)
        except (OSError, subprocess.SubprocessError) as error:
            def decoded(value):
                return value.decode(errors="replace") if isinstance(value, bytes) else value or ""
            self.commands.append({"command": command, "cwd": cwd,
                                  "exit_code": None, "error": str(error),
                                  "stdout": decoded(getattr(error, "stdout", None)),
                                  "stderr": decoded(getattr(error, "stderr", None))})
            raise
        self.commands.append({"command": command, "cwd": cwd,
                              "exit_code": result.returncode, "stdout": result.stdout,
                              "stderr": result.stderr})
        if result.returncode not in allowed:
            raise ValueError(f"command failed ({result.returncode}): {command}: {result.stderr[-2000:]}")
        return result.stdout

    def run_session(self, command, requests, *, cwd=None, env=None, timeout=180):
        """Write JSON-RPC lines and keep stdin open until every request id has a response.

        Closing stdin early would cancel in-flight requests (stdio EOF is a cancellation).
        """
        command = list(map(str, command))
        cwd = str(Path(cwd).resolve()) if cwd else os.getcwd()
        pending = {r["id"] for r in requests if "id" in r}
        lines = []
        process = subprocess.Popen(command, cwd=cwd, env=env, text=True, stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        timer = threading.Timer(timeout, process.kill)
        timer.start()
        try:
            for request in requests:
                process.stdin.write(json.dumps(request) + "\n")
            process.stdin.flush()
            while pending:
                line = process.stdout.readline()
                if not line:
                    break
                lines.append(line)
                message = json.loads(line)
                pending.discard(message.get("id"))
            process.stdin.close()
            rest, stderr = process.communicate()
            lines.append(rest)
        finally:
            timer.cancel()
        stdout = "".join(lines)
        self.commands.append({"command": command, "cwd": cwd, "exit_code": process.returncode,
                              "stdout": stdout, "stderr": stderr})
        if process.returncode != 0 or pending:
            raise ValueError(f"session failed ({process.returncode}, missing {sorted(pending)}): {command}: {stderr[-2000:]}")
        return stdout


def clean_environment():
    return {key: value for key, value in os.environ.items()
            if not key.startswith(("DYLD_", "KRONELLO_", "PKG_CONFIG_"))}


def inventory(root):
    files = {}
    for path in sorted(Path(root).rglob("*")):
        if path.is_symlink():
            raise ValueError(f"package symlink rejected: {path}")
        if path.is_file() and path.relative_to(root).as_posix() != "package-manifest.json":
            files[path.relative_to(root).as_posix()] = {
                "sha256": sha256(path), "size": path.stat().st_size,
                "mode": stat.S_IMODE(path.stat().st_mode)}
    return files


def package_hash(files):
    return hashlib.sha256(json.dumps(files, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def dependencies(output):
    # Fat binaries have an additional header per architecture.
    return [line.strip().split(" (compatibility version", 1)[0]
            for line in output.splitlines() if line[:1].isspace() and " (compatibility version" in line]


def rpaths(output):
    lines = output.splitlines()
    paths = []
    for index, line in enumerate(lines):
        if line.strip() == "cmd LC_RPATH":
            for following in lines[index + 1:index + 5]:
                if following.strip().startswith("path "):
                    paths.append(following.strip()[5:].split(" (offset ", 1)[0])
                    break
            else:
                raise ValueError("malformed LC_RPATH")
    return paths


def system_dependency(name):
    return name.startswith(("/usr/lib/", "/System/Library/Frameworks/")) and ".." not in Path(name).parts


def check_linkage(root, runner):
    root = Path(root).resolve()
    expected = set(EXECUTABLES + [f"lib/{name}" for name in LIBRARIES])
    observed = set()
    reports = {}
    for path in sorted(root.rglob("*")):
        if not path.is_file():
            continue
        with path.open("rb") as stream:
            magic = stream.read(4)
        if magic not in (b"\xcf\xfa\xed\xfe", b"\xfe\xed\xfa\xcf", b"\xca\xfe\xba\xbe", b"\xca\xfe\xba\xbf"):
            if path.relative_to(root).as_posix() in expected:
                raise ValueError(f"Mach-O required: {path}")
            continue
        relative = path.relative_to(root).as_posix()
        if relative not in expected:
            raise ValueError(f"unexpected Mach-O: {relative}")
        observed.add(relative)
        architectures = runner.run(["lipo", "-archs", path]).split()
        if platform.machine() not in architectures:
            raise ValueError(f"host architecture missing: {relative}: {architectures}")
        deps = dependencies(runner.run(["otool", "-L", "-arch", "all", path]))
        paths = rpaths(runner.run(["otool", "-l", "-arch", "all", path]))
        is_library = relative.startswith("lib/")
        anchor = "@loader_path" if is_library else "@executable_path/../lib"
        if set(paths) != {anchor}:
            raise ValueError(f"unexpected rpaths: {relative}: {paths}")
        for dep in deps:
            if is_library and dep == f"@rpath/{path.name}":
                continue
            if system_dependency(dep):
                continue
            if dep not in {f"{anchor}/{name}" for name in LIBRARIES}:
                raise ValueError(f"external/development dependency: {relative}: {dep}")
            if not (root / "lib" / dep.rsplit("/", 1)[-1]).is_file():
                raise ValueError(f"missing bundled dependency: {dep}")
        if is_library:
            ids = [line.strip() for line in runner.run(["otool", "-D", "-arch", "all", path]).splitlines()
                   if line.startswith("@") or line.startswith("/") and not line.endswith(":")]
            if not ids or set(ids) != {f"@rpath/{path.name}"}:
                raise ValueError(f"unexpected install name: {relative}: {ids}")
        reports[relative] = {"dependencies": deps, "rpaths": paths, "architectures": architectures}
    if observed != expected:
        raise ValueError(f"missing Mach-O files: {sorted(expected - observed)}")
    return reports


def validate_capabilities(cap, directory, substituted, receipt):
    if Path(cap["library_directory"]).resolve() != Path(directory).resolve() or cap["substituted"] != substituted:
        raise ValueError("runtime loaded the wrong directory/substitution status")
    if not cap["distribution_eligible"] or cap["development_only"] or cap["ffmpeg_version"] != "9.0.2":
        raise ValueError("pinned LGPL FFmpeg 9.0.2 required")
    libs = {entry["name"]: entry for entry in cap["libraries"]}
    pinned = {entry["name"]: entry for entry in receipt["libraries"]}
    if len(cap["libraries"]) != 5 or set(libs) != set(FFMPEG):
        raise ValueError("all five FFmpeg libraries required")
    for name, major in FFMPEG.items():
        entry = libs[name]
        config = entry["configuration"].split()
        if (entry["version"] >> 16 != major or not entry["license"].startswith("LGPL")
                or any(flag in config for flag in ["--enable-gpl", "--enable-nonfree"])
                or not {"--disable-gpl", "--disable-nonfree", "--enable-shared", "--disable-static", "--disable-autodetect"}.issubset(config)
                or entry["version"] != pinned[name]["version"]
                or entry["configuration"] != pinned[name]["configuration"]):
            raise ValueError(f"license/configuration/ABI mismatch: {name}")
    for name, direction in [("libsvtav1", "encoder"), ("prores_ks", "encoder"), ("pcm_s24le", "encoder"), ("libdav1d", "decoder")]:
        if not any(c["name"] == name and c[direction] for c in cap["codecs"]):
            raise ValueError(f"required codec missing: {name}")
