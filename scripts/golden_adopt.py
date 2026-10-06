#!/usr/bin/env python3
"""Adopt a reviewed, clean-revision platform GPU golden candidate."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import struct
import subprocess
import tempfile
import math

ROOT = Path(__file__).resolve().parents[1]
BASELINE = Path("tests/golden/apple-silicon-metal")
PER_FILE = 256 * 1024
TOTAL = 3 * 1024 * 1024


def require(condition, message):
    if not condition:
        raise ValueError(message)


def git(root, *args):
    return subprocess.check_output(["git", *args], cwd=root, text=True).strip()


def load(path):
    return json.loads(path.read_bytes())


def validate(root, candidate, baseline_path=BASELINE):
    require(not git(root, "status", "--porcelain", "--untracked-files=all"),
            "working tree is dirty; commit code changes before adoption")
    require(candidate.is_dir() and not candidate.is_symlink(), "candidate directory missing or symlinked")
    provenance = load(candidate / "provenance.json")
    require(provenance["revision"] == git(root, "rev-parse", "HEAD"), "candidate revision differs from HEAD")
    require(provenance["status"] == "", "candidate was generated from a dirty tree")
    environment = load(candidate / "environment.json")
    profiles = {"apple-silicon-metal": ("aarch64-apple-darwin", "Metal"),
                "linux-vulkan": ("x86_64-unknown-linux-gnu", "Vulkan"),
                "windows-dx12": ("x86_64-pc-windows-msvc", "Dx12")}
    require(baseline_path.name in profiles, "unsupported baseline profile")
    target, backend = profiles[baseline_path.name]
    require(environment["target"] == target and environment["adapter"]["backend"] == backend,
            "candidate platform differs from requested baseline")
    manifest = load(candidate / "manifest.json")
    adoption = load(candidate / "adoption.json")
    report = load(candidate.parent / "report.json")
    scenes = manifest["scenes"]
    ids = [s["id"] for s in scenes]
    catalog = load(root / baseline_path / "scenes.json")
    require(ids and ids == catalog["scene_ids"] and len(set(ids)) == len(ids), "zero or mismatched scenes")
    require(all(re.fullmatch(r"[a-zA-Z0-9-]+", name) for name in ids), "unsafe scene id")
    require(manifest["catalog"] == catalog, "catalog settings differ")
    require(report["status"] == "candidate-only; no baseline comparison" and
            report["test_count"] == 1 and report["scene_count"] == len(scenes) and
            report["frame_count"] == len(scenes) and report["candidate_may_be_adopted"] is True and
            report["provenance"] == provenance, "candidate run did not finish successfully")
    require([s["scene"] for s in report["scenes"]] == ids and
            all(s["result"] == "candidate; CPU oracle validated" for s in report["scenes"]),
            "candidate lacks CPU-oracle validation")
    require(adoption["schema_version"] == 1 and manifest["comparison_version"] == 1,
            "unsupported adoption or tolerance version")
    for field in ("comparison_version", "rgb_absolute", "rgb_relative", "alpha_absolute"):
        require(adoption[field] == manifest[field], "tolerance metadata differs")
    require(all(manifest[field] == 2 ** -10 for field in ("rgb_absolute", "rgb_relative", "alpha_absolute")),
            "tolerance must remain 2^-10")
    require(adoption["environment"] == environment and adoption["provenance"] == provenance,
            "provenance metadata differs")
    fields = ("id", "size", "sample_id", "samples_per_frame", "time", "working_space", "alpha")
    require(adoption["scene_settings"] == [{key: s[key] for key in fields} for s in scenes],
            "scene settings differ")
    expected = {"manifest.json", "environment.json", "provenance.json"}
    for scene in scenes:
        name = scene["id"]
        require(scene["sample_id"] == "frame-0", "unsupported frame id")
        expected.update((f"{name}/frame-0.rgba16f", f"{name}/frame-0.png"))
        if scene["output_transform"] is not None:
            expected.add(f"{name}/external-srgb-straight.rgba16f")
    require(set(adoption["files"]) == expected, "missing or unexpected artifacts in adoption manifest")
    require(not any(p.is_symlink() for p in candidate.rglob("*")), "symlinked artifact")
    require({p.relative_to(candidate).as_posix() for p in candidate.rglob("*") if p.is_file()} ==
            expected | {"adoption.json"}, "missing or unexpected candidate files")
    for name, record in adoption["files"].items():
        data = (candidate / name).read_bytes()
        require(len(data) == record["bytes"] and hashlib.sha256(data).hexdigest() == record["sha256"],
                f"artifact hash or size differs: {name}")
    for scene in scenes:
        width, height = scene["size"]
        require(type(width) is int and type(height) is int and width > 0 and height > 0, "invalid dimensions")
        data = (candidate / scene["id"] / "frame-0.rgba16f").read_bytes()
        require(len(data) == width * height * 8, "frame dimensions differ")
        for pixel in struct.iter_unpack("<4e", data):
            require(all(math.isfinite(v) for v in pixel), "non-finite pixel")
            require(0 <= pixel[3] <= 1 and (pixel[3] != 0 or pixel[:3] == (0, 0, 0)),
                    "invalid premultiplied pixel")
    files = [p for p in candidate.rglob("*") if p.is_file()]
    preserved = [root / baseline_path / name for name in ("README.md", "scenes.json")]
    fixtures = [p for p in (root / "tests/fixtures").rglob("*") if p.is_file()]
    other_golden = [p for p in (root / "tests/golden").rglob("*")
                    if p.is_file() and not p.is_relative_to(root / baseline_path)]
    all_files = files + preserved + fixtures + other_golden
    require(all(p.stat().st_size <= PER_FILE for p in all_files), "fixture exceeds 256 KiB")
    total = sum(p.stat().st_size for p in all_files)
    require(total <= TOTAL, "fixtures plus platform goldens exceed 3 MiB")
    return adoption, {"scene_count": len(scenes), "candidate_bytes": sum(p.stat().st_size for p in files),
                      "fixture_total_bytes": total, "largest_file_bytes": max(p.stat().st_size for p in all_files)}


def adopt(root, candidate, baseline_path=BASELINE):
    adoption, sizes = validate(root, candidate, baseline_path)
    print(json.dumps({"adoption_manifest": adoption, "sizes": sizes}, indent=2), flush=True)
    baseline = root / baseline_path
    # Validate everything before mutation, then publish a complete directory.
    with tempfile.TemporaryDirectory(prefix="adopt.", dir=root / "target/golden") as temp:
        stage = Path(temp) / "baseline"
        shutil.copytree(candidate, stage)
        for name in ("README.md", "scenes.json"):
            shutil.copy2(baseline / name, stage / name)
        # Recheck bytes after copying, including the recorded manifest.
        for source in candidate.rglob("*"):
            if source.is_file():
                require(source.read_bytes() == (stage / source.relative_to(candidate)).read_bytes(),
                        "candidate changed while copying")
        validate(root, candidate, baseline_path)
        backup = Path(temp) / "previous"
        baseline.rename(backup)
        try:
            stage.rename(baseline)
        except BaseException:
            backup.rename(baseline)
            raise
    print(json.dumps({"status": "adopted", "revision": adoption["provenance"]["revision"], **sizes}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("candidate", type=Path, help="target/golden/run.*/candidate from a successful UPDATE")
    parser.add_argument("--profile", choices=("apple-silicon-metal", "linux-vulkan", "windows-dx12"), default="apple-silicon-metal")
    args = parser.parse_args()
    candidate = args.candidate.resolve()
    require(candidate.is_relative_to(ROOT / "target/golden") and candidate.name == "candidate",
            "candidate must be beneath target/golden and named candidate")
    adopt(ROOT, candidate, Path("tests/golden") / args.profile)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, KeyError, TypeError, subprocess.CalledProcessError) as error:
        raise SystemExit(f"golden adoption failed: {error}") from error
