#!/usr/bin/env python3
"""Create an isolated lightweight color-curve GUI review fixture without builds."""
import argparse
import hashlib
import json
import shutil
import subprocess
from pathlib import Path
from uuid import uuid4
ROOT = Path(__file__).resolve().parents[1]
def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output-root', type=Path, required=True)
    output = parser.parse_args().output_root.resolve()
    output.mkdir(parents=True, exist_ok=False)
    binary = output / 'kronello'
    shutil.copy2(ROOT / 'apps/macos/Libraries/kronello', binary)
    document = json.loads((ROOT / 'examples/m5-text-matte.project.json').read_text())
    document['id'] = str(uuid4()); document['name'] = 'GUI-007 Color Curve'
    comp = document['compositions'][0]
    comp['design_extent'] = {'width': 640.0, 'height': 480.0}
    node = comp['nodes'][0]
    node['name'] = 'Color rectangle'
    comp['nodes'] = [node]; comp['root_nodes'] = [node['id']]
    curve = str(uuid4())
    fill = next(p for p in node['properties'] if p['descriptor']['key'] == 'kronello.fill_color')
    fill['source'] = {'kind': 'curve', 'value': curve}
    position = next(p for p in node['properties'] if p['descriptor']['key'] == 'kronello.transform.position')
    position['source'] = {'kind': 'constant', 'value': {'kind': 'vec2', 'value': [200.0, 180.0]}}
    def color(r, g, b):
        return {'kind': 'color', 'value': {'space': 'srgb', 'components': {'r': r, 'g': g, 'b': b, 'alpha': 1.0}}}
    keys = [{'time': {'num': '0', 'den': '1'}, 'value': color(1, 0, 0), 'interpolation': {'kind': 'hold'}},
            {'time': {'num': '1', 'den': '1'}, 'value': color(0, 0, 1), 'interpolation': {'kind': 'cubic', 'value': {'control1': [0.2, 0.3], 'control2': [0.7, 0.8]}}}]
    document['curves'] = [{'id': curve, 'value_type': 'color', 'interpolation_version': 1, 'keys': keys}]
    document['texts'] = []; document['assets'] = []; document['sequences'] = []
    document.pop('mattes', None)
    project = output / 'color.kronello'
    def call(request):
        result = subprocess.run([str(binary), '--backend', 'cpu-reference'], input=json.dumps(request), text=True, capture_output=True, check=True)
        response = json.loads(result.stdout)
        assert response['status'] == 'success', response
        return response['result']['value']
    call({'operation': 'project.create', 'project': str(project), 'document': document})
    expected = {'project': str(project), 'composition': comp['id'], 'node': node['id'], 'property': fill['id'], 'curve': curve, 'key_count': 2, 'keys': keys,
                'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest()}
    (output / 'document.json').write_text(json.dumps(document, ensure_ascii=False, indent=2)+'\n')
    (output / 'identities.json').write_text(json.dumps(expected, ensure_ascii=False, indent=2)+'\n')
    for second in [0, 1]:
        frame = call({'operation': 'render.frame', 'input': {'project': str(project), 'composition': comp['id'], 'fonts': [], 'region': {'origin': [0,0], 'extent': [640,480], 'pixels': [64,48]}}, 'time': {'num': str(second), 'den': '1'}})
        (output / ('frame-'+str(second)+'.json')).write_text(json.dumps({'metadata': frame['metadata'], 'linear_sha256': hashlib.sha256(json.dumps(frame['linear']).encode()).hexdigest()} ,indent=2)+'\n')
    print(project)
if __name__ == '__main__':
    main()
