#!/usr/bin/env python3
"""Create GUI-004 review fixtures only through shared CLI commands (CPU explicit)."""
import argparse
import copy
import json
import subprocess
from pathlib import Path
from uuid import uuid4

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-root", type=Path, required=True)
    parser.add_argument("--binary", type=Path, default=ROOT / "apps/macos/Libraries/kronello")
    args = parser.parse_args()
    output = args.output_root.resolve()
    output.mkdir(parents=True)  # Reject reuse: no fixture or output is overwritten.
    binary = args.binary.resolve()

    def call(project, operation, **fields):
        request = {"operation": operation, **fields}
        if operation not in ["render.explain", "capabilities.get", "job.list", "job.get", "job.cancel"]:
            request["project"] = str(project)
        result = subprocess.run([str(binary), "--backend", "cpu-reference"],
                                input=json.dumps(request, sort_keys=True),
                                text=True, capture_output=True, check=False)
        response = json.loads(result.stdout)
        if result.returncode or response["status"] != "success":
            raise RuntimeError(response)
        return response["result"]["value"]

    def edit(project, commands):
        base = call(project, "project.info")["revision"]
        plan = call(project, "edit.plan", base_revision=base, commands=commands)
        return call(project, "edit.apply", base_revision=base, commands=commands, plan_hash=plan["plan_hash"],
                    session_id=str(uuid4()), idempotency_key=str(uuid4()))

    document = json.loads((ROOT / "examples/template-002.project.json").read_text())
    definition = json.loads((ROOT / "examples/template-002.definition.json").read_text())
    # Make the portrait variant overflow using its real max_lines constraint.
    portrait = next(c for c in document["compositions"] if c["id"] == definition["variants"]["portrait"]["composition_ref"])
    for node in portrait["nodes"]:
        for prop in node["properties"]:
            if prop["descriptor"]["key"] == "kronello.text.wrap_width":
                prop["source"] = {"kind": "constant", "value": {"kind": "scalar", "value": 8.0}}
    for key in definition["variants"]["portrait"]["constraints"]["max_lines"]:
        definition["variants"]["portrait"]["constraints"]["max_lines"][key] = 1
    fonts = [{"identity": document["texts"][0]["styles"][0]["font"],
              "path": str(ROOT / "target/fixtures/external/NotoSansCJKjp-Regular.otf")}]
    (output / "fonts.json").write_text(json.dumps(fonts, indent=2) + "\n")
    template = output / "template.kronello"
    call(template, "project.create", document=document)
    edit(template, [{"template": {"define": {"definition": definition}}}])
    instance = {"id": str(uuid4()), "definition_ref": definition["id"], "version": definition["version"],
                "duration": {"num": "8", "den": "1"}, "inputs": {}}
    edit(template, [{"template": {"instantiate": {"composition": document["compositions"][0]["id"], "node": str(uuid4()), "index": 0, "instance": instance}}}])
    new = copy.deepcopy(definition)
    new["id"] = str(uuid4()); new["version"] = "1.1.0"; new["duration_policy"]["middle_mode"] = "hold"
    edit(template, [{"template": {"define": {"definition": new}}}])
    previews = {}
    for variant in [None, "portrait"]:
        candidate = {**instance, **({"variant": variant} if variant else {})}
        previews[variant or "base"] = call(template, "template.preview", instance=candidate, time={"num": "0", "den": "1"}, fonts=fonts)
    if previews["base"]["diagnostic"] is not None or previews["portrait"]["diagnostic"]["code"] != "TEMPLATE_OVERFLOW":
        raise RuntimeError("Fixture must have one fitting variant and one typed overflow")
    (output / "variant-results.json").write_text(json.dumps(previews, ensure_ascii=False, indent=2) + "\n")
    call(template, "template.migration_plan", base_revision=call(template, "project.info")["revision"], instance=instance["id"], definition=new["id"], time={"num": "0", "den": "1"}, fonts=fonts)
    for name, typed_error in [("export-ready", False), ("export-error", True)]:
        path = output / (name + ".kronello")
        project = json.loads((ROOT / "examples/m1-demo.project.json").read_text())
        project["id"] = str(uuid4()); project["name"] = name
        c = project["compositions"][0]
        c["duration"] = {"num": "1", "den": "8"}
        if not typed_error:
            c["nodes"] = [n for n in c["nodes"] if n["kind"]["kind"] != "text"]
            c["root_nodes"] = [n["id"] for n in c["nodes"]]
            project["texts"] = []
        call(path, "project.create", document=project)
        result = call(path, "render.explain", input={"project": str(path), "composition": c["id"], "region": {"origin": [0, 0], "extent": [64, 32], "pixels": [64, 32]}, "fonts": []}, time={"num": "0", "den": "1"})
        if bool(result["diagnostics"]) != typed_error:
            raise RuntimeError("Expected ready export and FONT_MISSING error fixtures")
        (output / (name + "-inspection.json")).write_text(json.dumps(result, indent=2) + "\n")
    (output / "review.json").write_text(json.dumps({"template": str(template), "placement": instance["id"], "new_edition": new["id"], "font_inputs": str(output / "fonts.json"), "ready": str(output / "export-ready.kronello"), "error": str(output / "export-error.kronello")}, indent=2) + "\n")
    print(output / "review.json")


if __name__ == "__main__":
    main()
