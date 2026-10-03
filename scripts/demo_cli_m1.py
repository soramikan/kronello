#!/usr/bin/env python3
"""M1 CLI の再現可能な headless デモと成果物検証。外部取得は行わない。"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", choices=["gpu", "cpu-reference"], default="gpu")
    parser.add_argument("--binary", type=Path, default=Path("target/debug/kronello"))
    parser.add_argument("--output-root", type=Path, required=True)
    args = parser.parse_args()
    root = args.output_root.resolve()
    root.mkdir()  # Existing artifacts are never overwritten.
    project = root / "demo.kronello"
    document = json.loads(Path("examples/m1-demo.project.json").read_text())

    def call(command, payload):
        request = json.dumps(payload, ensure_ascii=False)
        (root / ("-".join(command) + ".request.json")).write_text(request + "\n")
        result = subprocess.run(
            [str(args.binary.resolve()), "--backend", args.backend, *command],
            input=request, text=True, capture_output=True, check=False,
        )
        (root / ("-".join(command) + ".stdout.json")).write_text(result.stdout)
        (root / ("-".join(command) + ".stderr.log")).write_text(result.stderr)
        response = json.loads(result.stdout)
        assert result.returncode == 0 and response["status"] == "success", response
        return response["result"]["value"]

    created = call(["project", "create"], {"project": str(project), "document": document})
    exported = call(["project", "export"], {"project": str(project)})
    assert exported["document"] == document
    imported = call(["project", "import"], {
        "project": str(project), "base_revision": created["revision"], "document": document,
    })
    assert imported["revision"] == "2"
    input_data = {
        "project": str(project), "composition": document["compositions"][0]["id"],
        "region": {"origin": [0, 0], "extent": [64, 32], "pixels": [64, 32]},
        "fonts": [{"identity": document["texts"][0]["styles"][0]["font"],
                   "path": str(Path("target/fixtures/external/NotoSansCJKjp-Regular.otf").resolve())}],
    }
    sequence = call(["render", "sequence"], {
        "input": input_data,
        "range": {"start": {"num": "0", "den": "1"}, "end": {"num": "1", "den": "1"}},
        "frame_rate": {"num": "4", "den": "1"},
        "output_directory": str(root / "frames"),
    })
    disk = json.loads((root / "frames/sequence.json").read_text())
    assert disk == sequence and len(sequence["frames"]) == 4
    assert len(list((root / "frames").iterdir())) == 13
    expected_backend = "wgpu_rgba16f" if args.backend == "gpu" else "cpu_reference_float32"
    for ordinal, frame in enumerate(sequence["frames"]):
        metadata = frame["metadata"]
        assert metadata["backend"] == expected_backend
        assert metadata["revision"] == "2"
        assert metadata["frame_index"] == str(ordinal)
        assert metadata["sequence_number"] == ordinal
        assert metadata["font_locks"] == [input_data["fonts"][0]["identity"]]
        assert json.loads((root / "frames" / frame["metadata_file"]).read_text()) == metadata
        for kind in ["numeric", "display"]:
            artifact = frame[kind]
            data = (root / "frames" / artifact["name"]).read_bytes()
            assert len(data) == artifact["bytes"]
            assert hashlib.sha256(data).hexdigest() == artifact["sha256"]
    assert len({f["numeric"]["sha256"] for f in sequence["frames"]}) == 4
    print(json.dumps({"status": "verified", "backend": expected_backend,
                      "frames": 4, "output_root": str(root)}, ensure_ascii=False))


if __name__ == "__main__":
    main()
