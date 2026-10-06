#!/usr/bin/env python3
"""SIM-001 copied-binary CLI/MCP persistence and CPU pixel evidence; never builds."""
import argparse
import copy
import hashlib
import json
import platform
import subprocess
from pathlib import Path
from check_gui_007_transports import Evidence, canonical, rational

class SimulationEvidence(Evidence):
    def run_simulation(self, fixture, composition):
        self.project = str(self.output / "simulation.kronello")
        document = json.loads(fixture.read_text())
        document["name"] = "SIM-001 small particles"
        for definition in document["compositions"]:
            # Composition has no name field; label the single root node instead.
            definition["nodes"][0]["name"] = "Particle emitter" if definition["id"] == composition else "Particle shape source"
            definition["design_extent"] = {"width": 320.0, "height": 180.0}
            for node in definition["nodes"]:
                for property in node["properties"]:
                    key = property["descriptor"]["key"]
                    values = {"kronello.shape.size": ("vec2", [12.0, 12.0]),
                              "kronello.transform.position": ("vec2", [0.0, 0.0]),
                              "kronello.simulation.origin": ("vec2", [40.0, 70.0]),
                              "kronello.simulation.velocity": ("vec2", [80.0, 0.0]),
                              "kronello.simulation.jitter": ("vec2", [0.0, 12.0])}
                    if key in values:
                        kind, value = values[key]
                        property["source"] = {"kind": "constant", "value": {"kind": kind, "value": value}}
        document["simulations"][0]["lifetime"] = rational(2)
        document["simulations"][0]["emission_interval"] = rational(2, 5)
        self.success("cli", "project.create", project=self.project, document=document)
        initial = self.export()
        frame = self.frame_simulation("initial", composition, rational(7, 10))
        at_start = self.frame_simulation("start", composition, rational(0))
        self.check("particles_change_actual_pixels", frame["linear"] != at_start["linear"])
        simulation = copy.deepcopy(initial["simulations"][0])
        simulation["seed"] = 18446744073709551615
        stale_base = self.revision
        event, changed = self.edit("simulation_seed", {"simulation_set": {"simulation": simulation}}, "mcp")
        self.check("seed_u64_max_persisted", changed["simulations"][0]["seed"] == 18446744073709551615)
        commands = [{"simulation_set": {"simulation": initial["simulations"][0]}}]
        for transport in ["cli", "mcp"]:
            response = self.call(transport, "edit.plan", project=self.project, base_revision=stale_base, commands=commands)
            self.check(transport + ".stale_origin_rejected", response["status"] == "error" and response["error"]["code"] == "REVISION_CONFLICT")
        self.check("stale_did_not_overwrite", self.export() == changed)
        self.undo("seed", event, initial)
        restored = self.frame_simulation("restored", composition, rational(7, 10))
        self.check("undo_pixels_exact", restored["linear"] == frame["linear"] and restored["display"] == frame["display"])
        self.gpu_frame("initial", composition, rational(7, 10), frame)
        self.gpu_frame("restored", composition, rational(7, 10), restored)
        (self.output / "simulation.project.json").write_bytes(canonical(self.export()) + b"\n")
        (self.output / "identities.json").write_bytes(canonical({"project": self.project, "composition": composition, "simulation": simulation["id"], "revision": self.revision}) + b"\n")
        self.report["composition"] = composition
        self.report["status"] = "passed"
    def frame_simulation(self, name, composition, time):
        fields = {"input": {"project": self.project, "composition": composition,
                  "region": {"origin": [0, 0], "extent": [320, 180], "pixels": [64, 48]}}, "time": time}
        cli = self.success("cli", "render.frame", **fields)
        mcp = self.success("mcp", "render.frame", **fields)
        self.check(name + ".all_pixels_and_metadata_equal", cli == mcp)
        self.check(name + ".fixed_revision", str(cli["metadata"]["revision"]) == self.revision)
        self.report.setdefault("frames", {})[name] = {"sha256": hashlib.sha256(canonical(cli)).hexdigest(), "metadata": cli["metadata"]}
        return cli
    def gpu_frame(self, name, composition, time, reference):
        # Same pinned CLI and request as the CPU leg, rendered through wgpu.
        # Compares every linear channel against the CPU frame at the shared
        # QA-001 tolerance 2^-10 (relative for RGB, absolute for alpha).
        request = {"operation": "render.frame",
                   "input": {"project": self.project, "composition": composition,
                             "region": {"origin": [0, 0], "extent": [320, 180], "pixels": [64, 48]}},
                   "time": time}
        process = subprocess.run([str(self.binary / "kronello"), "--backend", "gpu"],
                                 input=json.dumps(request), text=True, capture_output=True,
                                 env=self.env, timeout=120)
        response = json.loads(process.stdout)
        self.report["requests"].append({"transport": "cli-gpu", "request": request,
                                        "response_sha256": hashlib.sha256(canonical(response)).hexdigest()})
        if response.get("status") != "success":
            self.check(name + ".gpu_backend_available", False)
            return
        value = response["result"]["value"]
        metadata = value["metadata"]
        self.check(name + ".gpu_backend_is_wgpu", metadata.get("backend") == "wgpu_rgba16f")
        tolerance = 2 ** -10
        mismatches = 0
        worst = 0.0
        for expected, actual in zip(reference["linear"], value["linear"]):
            for channel, (e, a) in enumerate(zip(expected, actual)):
                limit = tolerance if channel == 3 else tolerance * max(1.0, abs(e))
                diff = abs(e - a)
                if diff > limit:
                    mismatches += 1
                worst = max(worst, diff)
        self.check(name + ".gpu_pixel_count", len(value["linear"]) == len(reference["linear"]))
        self.check(name + ".gpu_within_cpu_tolerance", mismatches == 0)
        self.report.setdefault("gpu", {"environment": {
            "backend": metadata.get("backend"), "machine": platform.machine(),
            "macos": platform.mac_ver()[0], "python": platform.python_version(),
            "tolerance": {"rgb_absolute": tolerance, "rgb_relative": tolerance,
                          "alpha_absolute": tolerance}}})
        self.report["frames"][name + "_gpu"] = {
            "sha256": hashlib.sha256(canonical(value)).hexdigest(),
            "metadata": metadata, "cpu_comparison": {"mismatches": mismatches, "worst_abs_diff": worst}}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--mcp-binary", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--composition", required=True)
    parser.add_argument("--output-root", type=Path, required=True)
    args = parser.parse_args()
    evidence = SimulationEvidence(args.output_root.resolve(), args.binary.resolve(), args.mcp_binary.resolve())
    try:
        evidence.run_simulation(args.fixture, args.composition)
    finally:
        evidence.close()
    print(json.dumps({"passed": len(evidence.report["checks"]), "project": evidence.project}))
if __name__ == "__main__":
    main()
