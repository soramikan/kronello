#!/usr/bin/env python3
"""Stage-1 demo plus an explicit portrait variant and CLI/MCP/GUI evidence."""
import argparse
import copy
import json
import os
from pathlib import Path
import struct
import uuid

from demo_integration_m2 import Demo, ROOT, fraction, rational

HEADLINE = "日本語の字幕\n背景帯が追従"
PORTRAIT_LINES = "日本語の\n字幕\n背景帯が\n追従"
TIMES = [rational(0), rational(2, 5), rational(1), rational(77, 10)]


def canonical(value):
    # Swift JSONSerialization and Rust may write integral floats differently.
    # Keep array order, every field and every finite numeric value unchanged.
    if isinstance(value, float) and value.is_integer():
        return int(value)
    if isinstance(value, dict):
        return {k: canonical(v) for k, v in value.items()}
    if isinstance(value, list):
        return [canonical(v) for v in value]
    return value


def canonical_bytes(value):
    return (json.dumps(canonical(value), ensure_ascii=False, sort_keys=True,
                       separators=(",", ":"), allow_nan=False) + "\n").encode()


class Stage2(Demo):
    def load_inputs(self):
        document, definition = super().load_inputs()
        # Extend the stage-1 edition BEFORE publication, keeping one definition
        # and the original authoring content, inputs and duration policy intact.
        namespace = uuid.UUID("9803d7fb-b1bb-4ffc-9f97-6427c7b98dcb")
        ids = set()
        def collect(value):
            if isinstance(value, dict):
                if "id" in value:
                    ids.add(value["id"])
                for v in value.values():
                    collect(v)
            elif isinstance(value, list):
                for v in value:
                    collect(v)
        authoring = document["compositions"][2]
        bundle = [authoring, *document["shapes"], *document["texts"]]
        collect(bundle)
        self.remap = {old: str(uuid.uuid5(namespace, old)) for old in ids}
        def mapped(value):
            if isinstance(value, dict):
                return {self.remap.get(k, k): mapped(v) for k, v in value.items()}
            if isinstance(value, list):
                return [mapped(v) for v in value]
            return self.remap.get(value, value) if isinstance(value, str) else value
        vertical = mapped(authoring)
        vertical["design_extent"] = {"width": 180, "height": 320}
        for node in vertical["nodes"]:
            node["name"] = "Portrait band" if node["kind"]["kind"] == "shape" else "Portrait headline"
            if node["kind"]["kind"] == "text":
                for prop in node["properties"]:
                    if prop["descriptor"]["key"] == "kronello.text.wrap_width":
                        prop["source"]["value"]["value"] = 48
                    if prop["descriptor"]["key"] == "kronello.transform.position":
                        prop["source"]["value"]["value"] = [32, 220]
        document["compositions"].append(vertical)
        document["shapes"] += mapped(copy.deepcopy(document["shapes"]))
        document["texts"] += mapped(copy.deepcopy(document["texts"]))
        self.portrait = copy.deepcopy(document["compositions"][0])
        self.portrait.update(id=str(uuid.uuid5(namespace, "portrait-destination")),
                             design_extent={"width": 180, "height": 320})
        document["compositions"].append(self.portrait)
        constraints = mapped(definition["constraints"])
        constraints["max_lines"] = {self.remap["955f3ce9-88f6-4025-978a-6d42e5c08d31"]: 4}
        definition["variants"] = {"portrait": {
            "composition_ref": vertical["id"],
            "targets": {name: mapped(value["target"]) for name, value in definition["public_inputs"].items()},
            "constraints": constraints, "content_hash": ""}}
        self.definition = definition
        (self.output / "input.project.json").write_bytes(canonical_bytes(document))
        (self.output / "input.definition.json").write_bytes(canonical_bytes(definition))
        return document, definition

    def run(self):
        super().run()
        self.run_stage2()

    def run_stage2(self):
        self.report["status"] = "running"
        # Stage 1 deliberately leaves an overflow input. Restore its verified
        # two-line pose so the exact SAME project can be opened in the app.
        self.edit("template.set_input", instance=self.instances[0], name="headline",
                  value={"kind": "string", "value": HEADLINE})
        exported = self.cli("project.export", project=str(self.project))
        base = next(i for i in exported["document"]["template_instances"] if i["id"] == self.instances[0])
        portrait_instance = {**base, "id": str(uuid.uuid4()), "variant": "portrait"}
        placement = str(uuid.uuid4())
        self.edit("template.instantiate", composition=self.portrait["id"], node=placement,
                  index=0, instance=portrait_instance)
        exported = self.cli("project.export", project=str(self.project))
        definitions = exported["document"]["templates"]
        self.check("variants.single_definition", len(definitions) == 1 and
                   definitions[0]["id"] == base["definition_ref"] == portrait_instance["definition_ref"])
        cases = []
        for name, comp, instance, expected_size in (
                ("landscape", self.compositions[0], base, [258, 36]),
                ("portrait", self.portrait, portrait_instance, [56, 68])):
            binding = (self.definition["variants"]["portrait"]["constraints"] if name == "portrait"
                       else self.definition["constraints"])["bands"][0]
            for at in TIMES:
                request = {"composition": comp["id"], "expand_instances": True,
                           "evaluation": {"time": at, "fonts": self.fonts}}
                cli = self.cli("scene.query", project=str(self.project), **request)
                mcp = self.tool("scene.query", project=str(self.project), **request)
                stamp = f"{name}-{at['num']}-{at['den']}"
                (self.output / (stamp + ".cli.json")).write_bytes(canonical_bytes(cli))
                (self.output / (stamp + ".mcp.json")).write_bytes(canonical_bytes(mcp))
                self.check(stamp + ".canonical_parity", canonical_bytes(cli) == canonical_bytes(mcp))
                nodes = {n["key"]["node"]: n for n in cli["nodes"] if n["key"]["instance_path"] == [instance["id"]]}
                band, label = nodes[binding["band_node"]]["evaluated"], nodes[binding["text_node"]]["evaluated"]
                size = band["properties"][binding["size_property"]]["value"]
                self.check(stamp + ".layout_size", size == expected_size, actual=size, expected=expected_size)
                y = 220 if name == "portrait" else 120
                width = 48 if name == "portrait" else 250
                height = 64 if name == "portrait" else 32
                self.check(stamp + ".layout_bounds", label["bounds"]["layout_bounds"] ==
                           {"min": [32, y], "max": [32 + width, y + height]} and
                           band["bounds"]["layout_bounds"] ==
                           {"min": [28, y - 2], "max": [36 + width, y + height + 2]})
                self.check(stamp + ".text", label["text"] == HEADLINE)
                for node in nodes.values():
                    for stage, bounds in node["evaluated"]["bounds"].items():
                        if bounds is not None:
                            self.check(stamp + ".inside." + node["key"]["node"] + "." + stage,
                                       all(0 <= bounds["min"][axis] <= bounds["max"][axis] <=
                                           comp["design_extent"][dimension] for axis, dimension in enumerate(("width", "height"))), bounds=bounds)
                cases.append({"name": stamp, "request": request, "instance": instance["id"],
                              "placement": next(n["key"]["node"] for n in cli["nodes"] if n["key"]["instance_path"] == [] and
                                                n["kind"].get("value", {}).get("id") == instance["id"]),
                              "cli": stamp + ".cli.json", "mcp": stamp + ".mcp.json"})
        previews = []
        for text in (HEADLINE, PORTRAIT_LINES):
            proposed = copy.deepcopy(portrait_instance)
            proposed["inputs"]["headline"] = {"kind": "string", "value": text}
            result = self.cli("template.preview", project=str(self.project), instance=proposed,
                              time=rational(1), fonts=self.fonts)
            self.check("portrait.preview.no_overflow", result.get("diagnostic") is None, diagnostic=result.get("diagnostic"))
            previews.append(result)
        def bounds(preview):
            return [(n["key"], n["evaluated"]["bounds"], n["evaluated"]["layout_bounds"])
                    for n in preview["nodes"]]
        self.check("portrait.wrapped_equals_explicit_lines", bounds(previews[0]) == bounds(previews[1]),
                   expected_lines=PORTRAIT_LINES.splitlines())
        (self.output / "portrait-wrapping.json").write_bytes(canonical_bytes(previews))
        final_export = self.cli("project.export", project=str(self.project))
        (self.output / "project.export.json").write_bytes(canonical_bytes(final_export))
        (self.output / "fonts.json").write_bytes(canonical_bytes(self.fonts))
        manifest = {"schema_version": 1, "project": str(self.project), "revision": self.revision,
                    "fonts": self.fonts, "cases": cases, "stage1_frames": str(self.project.parent / "fixed-frames/sequence.json"),
                    "render_backend": self.args.backend}
        if self.args.render_variants:
            for name, comp, pixels in (("landscape", self.compositions[0], [3840, 2160]),
                                       ("portrait", self.portrait, [1080, 1920])):
                render = {"input": {"project": str(self.project), "composition": comp["id"], "fonts": self.fonts,
                           "region": {"origin": [0, 0], "extent": [comp["design_extent"]["width"], comp["design_extent"]["height"]], "pixels": pixels}},
                          "range": {"start": rational(0), "end": rational(8)}, "frame_rate": rational(1, 7),
                          "output_directory": str(self.output / (name + "-frames"))}
                job = self.wait_job(self.tool("render.submit", render=render, output={"format": "image_sequence"}))
                self.check(name + ".render.succeeded", job["status"] == "succeeded", job=job)
                sequence = json.loads((Path(job["destination"]) / "sequence.json").read_text())
                expected_backend = "wgpu_rgba16f" if self.args.backend == "gpu" else "cpu_reference_float32"
                self.check(name + ".render.metadata", job["completed_frames"] == 2 and len(sequence["frames"]) == 2 and
                           all(f["metadata"]["backend"] == expected_backend and
                               f["metadata"]["region"]["pixels"] == pixels and
                               fraction(f["metadata"]["time"]) == i * 7 for i, f in enumerate(sequence["frames"])))
                for i, frame in enumerate(sequence["frames"]):
                    data = (Path(job["destination"]) / frame["display"]["name"]).read_bytes()
                    self.check(f"{name}.render.{i}.png", data[:8] == b"\x89PNG\r\n\x1a\n" and
                               list(struct.unpack(">II", data[16:24])) == pixels)
        (self.output / "gui-evidence.json").write_bytes(canonical_bytes(manifest))
        self.report.update(status="verified", gui_evidence=str(self.output / "gui-evidence.json"))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-directory", required=True, type=Path)
    parser.add_argument("--binary-dir", type=Path, default=Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "debug")
    parser.add_argument("--backend", choices=("gpu", "cpu-reference"), default="gpu")
    parser.add_argument("--resolution", choices=("4k", "small"), default="4k")
    parser.add_argument("--state-root", type=Path)
    parser.add_argument("--render-variants", action="store_true", help="Also render 4K landscape and 1080x1920 portrait jobs (0/7s, 2 frames each)")
    args = parser.parse_args()
    demo = Stage2(args)
    try:
        demo.run()
    except Exception as error:
        demo.report.update(status="failed", error=str(error))
        raise
    finally:
        demo.finish()
    print(json.dumps({"status": demo.report["status"], "evidence": demo.report["gui_evidence"]}))


if __name__ == "__main__":
    main()
