#!/usr/bin/env python3
"""Prepare a GUI review project and reproduce external edits through the shared CLI."""
import argparse
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SHAPE = "adcfcd01-95a0-4292-ba82-61369bf4ac8c"
GROUP = "93e2a4d5-4f44-436c-a036-ea64d330f33d"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-root", type=Path, required=True)
    parser.add_argument("--binary", type=Path, default=ROOT / "apps/macos/Libraries/kronello")
    parser.add_argument("--action", choices=["prepare", "info", "rename", "hide", "delete"], default="prepare")
    args = parser.parse_args()
    output = args.output_root.resolve()
    project = output / "motion.kronello"

    def call(operation, **fields):
        request = {"operation": operation, "project": str(project), **fields}
        result = subprocess.run([str(args.binary.resolve())], input=json.dumps(request), text=True, capture_output=True, check=False)
        response = json.loads(result.stdout)
        if result.returncode or response["status"] != "success":
            raise RuntimeError(response)
        return response["result"]["value"]

    if args.action == "prepare":
        output.mkdir(parents=True)  # Existing review artifacts are never overwritten.
        document = json.loads((ROOT / "examples/m1-demo.project.json").read_text())
        document["id"] = "195938c6-408d-4391-a184-ed68c88c06d3"
        document["name"] = "GUI-001 日本語モーション"
        composition = document["compositions"][0]
        composition["design_extent"] = {"width": 1920, "height": 1080}
        for node in composition["nodes"]:
            node["name"] = "Shape" if node["id"] == SHAPE else "Title"
            node["containment_parent"] = GROUP
            if node["id"] != SHAPE:
                node["transform_parent"] = GROUP
            for prop in node["properties"]:
                key = prop["descriptor"]["key"]
                source = prop["source"]
                if source["kind"] != "constant":
                    continue
                values = {"kronello.shape.size": [600.0, 360.0], "kronello.transform.position": [240.0, 600.0],
                          "kronello.text.font_size": 64.0, "kronello.text.wrap_width": 1200.0, "kronello.text.line_height": 80.0}
                if key in values:
                    source["value"]["value"] = values[key]
        composition["nodes"].insert(0, {"id": GROUP, "name": "Group", "kind": {"kind": "null"},
            "containment_parent": None, "transform_parent": None, "child_order": composition["root_nodes"],
            "active_range": {"start": {"num": "0", "den": "1"}, "end": composition["duration"]}, "properties": []})
        composition["root_nodes"] = [GROUP]
        for key, value in zip(document["curves"][0]["keys"], [[240.0, 180.0], [600.0, 180.0]]):
            key["value"]["value"] = value
        fonts = [{"identity": document["texts"][0]["styles"][0]["font"],
                  "path": str(ROOT / "target/fixtures/external/NotoSansCJKjp-Regular.otf")}]
        (output / "document.json").write_text(json.dumps(document, ensure_ascii=False, indent=2) + "\n")
        (output / "fonts.json").write_text(json.dumps(fonts, indent=2) + "\n")
        print(json.dumps(call("project.create", document=document), ensure_ascii=False))
    elif args.action == "info":
        print(json.dumps(call("project.info"), ensure_ascii=False))
        print(json.dumps(call("history.list", since_revision="0", limit=1000), ensure_ascii=False))
    else:
        from uuid import uuid4
        info = call("project.info")
        document = call("project.export")["document"]
        composition = document["compositions"][0]["id"]
        fields = {"composition": composition, "node": SHAPE}
        if args.action == "rename": fields["name"] = "CLI renamed Shape " + uuid4().hex[:6]
        if args.action == "hide": fields["enabled"] = False
        command = {"rename": "node_rename", "hide": "node_enabled_set", "delete": "node_remove"}[args.action]
        commands = [{command: fields}]
        plan = call("edit.plan", base_revision=info["revision"], commands=commands)
        print(json.dumps(call("edit.apply", base_revision=info["revision"], commands=commands, plan_hash=plan["plan_hash"],
                              session_id=str(uuid4()), idempotency_key=str(uuid4())), ensure_ascii=False))
    print(project)


if __name__ == "__main__":
    main()
