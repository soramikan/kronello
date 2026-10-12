#!/usr/bin/env python3
"""Freeze codec fan-out and stateful audio fixtures from retained source bytes."""
import argparse
from copy import deepcopy
import json
from pathlib import Path
from perf_export_worker import digest
from prepare_export_perf_cases import rational


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    base = json.loads(args.source.read_text())
    base['submit']['render']['range'] = {'start': rational(0), 'end': rational(1)}
    base['expected_frames'] = 24
    base['submit']['outputs'] = []
    base['additional_output_names'] = []
    document = {'format': 'flac', 'profile_version': 1, 'audio': 'document', 'clips': []}
    movie = {'format': 'pro_res_mov', 'profile_version': 3, 'audio': 'document', 'clips': [], 'background': [0, 0, 0]}
    manifest = []

    def save(name, case, output, extra, filenames):
        case['name'] = name
        case['submit']['output'] = output
        case['submit']['outputs'] = [{'output': leg, 'chapters': 'omit'} for leg in extra]
        case['output_name'] = filenames[0]
        case['additional_output_names'] = filenames[1:]
        path = args.output / (name + '.json')
        path.write_text(json.dumps(case, ensure_ascii=False, indent=2) + '\n')
        manifest.append({'path': str(path.resolve()), 'sha256': digest(path)})

    save('pcm-flac-one-second', deepcopy(base), movie, [document], ['movie.mov', 'audio.flac'])
    save('six-audio-codecs', deepcopy(base), movie, [
        document, dict(document, format='mp3'),
        dict(movie, format='av1_mp4', profile_version=1, audio_codec='alac'),
        dict(movie, format='av1_mp4', profile_version=1, audio_codec='aac'),
        dict(movie, format='av1_webm', profile_version=1, audio_codec='opus')],
        ['movie.mov', 'audio.flac', 'audio.mp3', 'alac.mp4', 'aac.mp4', 'opus.webm'])
    case = deepcopy(base)
    case['submit']['render']['range'] = {'start': rational('1/24'), 'end': rational('4/24')}
    case['expected_frames'] = 3
    clip = case['document']['sequences'][0]['tracks'][1]['clips'][0]
    clip['source_in'] = rational(599)
    clip['time_map']['speed'] = rational('3/2')
    clip['audio_retime'] = 'pitch_preserve_v1'
    definitions = [('cutoff_hz', 200.0), ('order', 2.0), ('threshold_db', -30.0),
                   ('ratio', 3.0), ('attack_ms', 4.0), ('release_ms', 30.0), ('makeup_db', 0.0)]
    ids = {key: f'c5c6be97-bf27-4c74-afd6-{number:012d}' for number, (key, _) in enumerate(definitions, 1)}
    clip['properties'] = [{'id': ids[key], 'descriptor': {'key': 'kronello.effect.' + key, 'version': 1},
        'source': {'kind': 'constant', 'value': {'kind': 'scalar', 'value': value}}, 'modifiers': []}
        for key, value in definitions]
    clip['effects'] = [
        {'effect_id': 'kronello.audio.hpf', 'version': 1, 'parameters': {'kind': 'audio_hpf', 'cutoff_hz': ids['cutoff_hz'], 'order': ids['order']}},
        {'effect_id': 'kronello.audio.compressor', 'version': 1, 'parameters': {'kind': 'audio_compressor', **{key: ids[key] for key in ('threshold_db','ratio','attack_ms','release_ms','makeup_db')}}}]
    save('wsola-dsp-nonzero-partial', case, movie, [document,
        dict(movie, format='av1_mp4', profile_version=1, audio_codec='aac'),
        dict(movie, format='av1_webm', profile_version=1, audio_codec='opus')],
        ['movie.mov', 'audio.flac', 'aac.mp4', 'opus.webm'])
    (args.output / 'manifest.json').write_text(json.dumps({'schema_version': 1,
        'source_case_sha256': digest(args.source), 'cases': manifest}, indent=2) + '\n')


if __name__ == '__main__':
    main()
