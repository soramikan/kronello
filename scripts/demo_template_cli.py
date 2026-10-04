#!/usr/bin/env python3
"""Reproduce TEMPLATE-001 using the shared CLI and explicit CPU rendering."""
import argparse
import json
from pathlib import Path
import subprocess
import uuid

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output-directory', required=True, type=Path)
    parser.add_argument('--binary', type=Path)
    args = parser.parse_args()
    binary = args.binary.resolve() if args.binary else ROOT / 'target/debug/kronello'
    if not args.binary:
        subprocess.run(['cargo', 'build', '-p', 'kronello-cli', '--locked'], cwd=ROOT, check=True)
    output = args.output_directory.resolve()
    output.mkdir(parents=True, exist_ok=False)
    document = json.loads((ROOT / 'examples/template-001.project.json').read_text())
    definition = json.loads((ROOT / 'examples/template-001.definition.json').read_text())
    project = output / 'lower-third.kronello'
    session = str(uuid.uuid4())
    instance = str(uuid.uuid4())
    revision = '1'
    results = []

    def call(command, payload, expected_error=None):
        result = subprocess.run([str(binary), '--backend', 'cpu-reference', *command],
                                input=json.dumps(payload, ensure_ascii=False), text=True,
                                capture_output=True, cwd=ROOT)
        response = json.loads(result.stdout)
        if expected_error:
            assert result.returncode != 0 and response['error']['code'] == expected_error, response
        else:
            assert result.returncode == 0 and response['status'] == 'success', response
        results.append({'command': command, 'response': response})
        return response

    def edit(verb, **payload):
        nonlocal revision
        response = call(['template', verb], {
            'project': str(project), 'base_revision': revision, 'session_id': session,
            'idempotency_key': verb + '-' + revision, **payload,
        })
        revision = str(response['result']['value']['revision'])

    call(['project', 'create'], {'project': str(project), 'document': document})
    edit('define', definition=definition)
    edit('instantiate', composition=document['compositions'][0]['id'], node=str(uuid.uuid4()), index=0,
         instance={'id': instance, 'definition_ref': definition['id'], 'version': definition['version'],
                   'duration': {'num': '5', 'den': '1'}, 'inputs': {}})
    edit('set_duration', instance=instance, duration={'num': '8', 'den': '1'})
    edit('set_input', instance=instance, name='headline', value={'kind': 'string', 'value': '日本語日本語'})
    call(['project', 'export'], {'project': str(project)})
    font = document['texts'][0]['styles'][0]['font']
    import sys
    sys.path.insert(0, str(ROOT / 'scripts'))
    from fixtures import external_fixture_dir
    font_path = external_fixture_dir() / 'NotoSansCJKjp-Regular.otf'
    render_input = {'project': str(project), 'composition': document['compositions'][0]['id'],
                    'region': {'origin': [0, 0], 'extent': [64, 32], 'pixels': [64, 32]},
                    'fonts': [{'identity': font, 'path': str(font_path)}]}
    request = {'input': render_input, 'range': {'start': {'num': '0', 'den': '1'},
                                              'end': {'num': '2', 'den': '1'}},
               'frame_rate': {'num': '1', 'den': '1'}, 'output_directory': str(output / 'frames')}
    call(['render', 'sequence'], request)
    edit('set_input', instance=instance, name='headline', value={'kind': 'string', 'value': '一\n二\n三'})
    request['output_directory'] = str(output / 'overflow-frames')
    call(['render', 'sequence'], request, 'TEMPLATE_OVERFLOW')
    assert not (output / 'overflow-frames').exists()
    (output / 'results.json').write_text(json.dumps(results, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps({'project': str(project), 'results': str(output / 'results.json'),
                      'frames': str(output / 'frames'), 'overflow': 'TEMPLATE_OVERFLOW'}, ensure_ascii=False))


if __name__ == '__main__':
    main()
