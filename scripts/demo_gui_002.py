#!/usr/bin/env python3
"""Create a three-key GUI-002 review fixture through the shared CLI."""
import argparse
import json
import subprocess
from pathlib import Path
from uuid import uuid4

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-root", type=Path, required=True)
    parser.add_argument("--expression-consumer", action="store_true", help="Share Position curve with an expression-driven Size for last-key review")
    parser.add_argument("--binary", type=Path, default=ROOT / "apps/macos/Libraries/kronello")
    args = parser.parse_args()
    binary = args.binary.resolve()
    subprocess.run(["python3", str(ROOT / "scripts/demo_gui_001.py"), "--output-root", str(args.output_root), "--binary", str(binary)], check=True)
    project = (args.output_root / "motion.kronello").resolve()

    def call(operation, **fields):
        request = {"operation": operation, "project": str(project), **fields}
        result = subprocess.run([str(binary)], input=json.dumps(request, sort_keys=True), text=True, capture_output=True, check=True)
        response = json.loads(result.stdout)
        if response["status"] != "success":
            raise RuntimeError(response)
        return response["result"]["value"]

    exported = call("project.export")
    document = exported["document"]
    curve = document["curves"][0]
    curve["keys"] = [
        {"time": {"num": str(i), "den": "1"}, "value": {"kind": "vec2", "value": value},
         "interpolation": {"kind": "cubic", "value": {"control1": [1 / 3, 1 / 3], "control2": [2 / 3, 2 / 3]}}}
        for i, value in zip([0, 2, 4], [[240.0, 180.0], [600.0, 360.0], [960.0, 540.0]])
    ]
    node, prop = next((node, prop) for node in document["compositions"][0]["nodes"] for prop in node["properties"]
                      if prop["source"] == {"kind": "curve", "value": curve["id"]})
    commands = [{"property_source_set": {"object": node["id"], "property": prop["id"], "source": prop["source"], "curve": curve}}]
    if args.expression_consumer:
        expression_id = str(uuid4())
        size = next(prop for prop in node["properties"] if prop["descriptor"]["key"] == "kronello.shape.size")
        commands += [
            {"expression_set": {"expression": {"id": expression_id, "version": 1, "value_type": "vec2", "nodes": [
                {"curve_sample": {"curve": curve["id"], "offset": {"num": "0", "den": "1"}, "value_type": "vec2"}}]}}},
            {"property_source_set": {"object": node["id"], "property": size["id"], "source": {"kind": "expression", "value": expression_id}}},
        ]
    plan = call("edit.plan", base_revision=exported["revision"], commands=commands)
    event = call("edit.apply", base_revision=exported["revision"], commands=commands, plan_hash=plan["plan_hash"],
                 session_id=str(uuid4()), idempotency_key=str(uuid4()))
    print(json.dumps(event, ensure_ascii=False))
    print(project)


if __name__ == "__main__":
    main()
