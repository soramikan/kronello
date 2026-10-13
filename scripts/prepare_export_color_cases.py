#!/usr/bin/env python3
"""Derive fixed fan-out cases from retained PERF-006 fixtures."""
import argparse
from copy import deepcopy
import json
from pathlib import Path
from prepare_export_perf_cases import rational
from perf_export_worker import digest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--audio-source', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    base = json.loads(args.source.read_text())
    records = []

    def save(name, case, outputs):
        case['name'] = name
        case['submit']['outputs'] = outputs
        if case['submit']['output']['format'] == 'av1_mp4':
            case['output_name'] = 'movie.mp4'
        case['additional_output_names'] = [f'additional-{i}.' +
            ('mxf' if leg['output']['format'] == 'dnx_mxf' else
             'gif' if leg['output']['format'] == 'gif' else
             'webm' if leg['output']['format'] == 'av1_webm' else
             'mp4' if leg['output']['format'] == 'av1_mp4' else 'mov')
            for i, leg in enumerate(outputs)]
        path = args.output / (name + '.json')
        path.write_text(json.dumps(case, ensure_ascii=False, indent=2) + '\n')
        records.append({'path': str(path.resolve()), 'sha256': digest(path)})

    movie = deepcopy(base['submit']['output'])
    save('4k-single-direct', deepcopy(base), [])
    case = deepcopy(base)
    case['document']['sequences'][0]['markers'] = [{
        'id': 'd6f69017-2caa-4b5c-9fc8-b8bda5ef2c92',
        'time': rational('1/2'), 'color': 'green', 'role': 'chapter',
        'title': 'PERF-010 chapter', 'comment': 'authored annotation'}]
    case['submit']['chapters'] = 'transfer'
    save('4k-duplicate-chapters', case, [
        {'output': deepcopy(movie), 'chapters': 'omit'},
        {'output': deepcopy(movie), 'chapters': 'transfer'}])
    other = deepcopy(movie)
    other['background'] = [0.125, 0.25, 0.5]
    save('4k-distinct-background', deepcopy(base), [{'output': other, 'chapters': 'omit'}])
    case = deepcopy(base)
    case['document']['assets'] = []
    case['sources'] = []
    case['document']['sequences'][0]['tracks'][0]['clips'][0]['source_ref'] = {
        'kind': 'generator', 'generator': 'kronello.solid', 'version': 1,
        'color': {'space': 'linear_rec709', 'components': {'r': 0.125, 'g': 0.25, 'b': 0.5, 'alpha': 0.5}}}
    save('4k-transparent-background', deepcopy(case), [{'output': other, 'chapters': 'omit'}])
    case['document']['sequences'][0]['working_space'] = 'linear_rec2020'
    case['document']['sequences'][0]['tracks'][0]['clips'][0]['source_ref']['color'] = {
        'space': 'linear_rec2020', 'components': {'r': 3.0, 'g': 1.5, 'b': 0.25, 'alpha': 0.5}}
    case['submit']['render']['input']['region']['pixels'] = [640, 360]
    case['submit']['render']['range']['end'] = rational('2/3')
    case['expected_frames'] = 4
    case['submit']['render']['input']['profile']['working_space'] = 'linear_rec2020'
    case['submit']['render']['input']['profile']['hdr'] = {'transfer': 'pq'}
    hdr = dict(movie, format='pro_res_hdr_mov', profile_version=1, transfer='pq')
    case['submit']['output'] = hdr
    sdr = dict(movie, format='pro_res_sdr_from_hdr_mov', profile_version=1)
    for transfer in ('pq', 'hlg'):
        hdr_case = deepcopy(case)
        hdr_case['submit']['render']['input']['profile']['hdr']['transfer'] = transfer
        hdr_case['submit']['output']['transfer'] = transfer
        save('hdr-' + transfer + '-and-sdr', hdr_case, [
            {'output': dict(hdr, transfer=transfer), 'chapters': 'omit'},
            {'output': deepcopy(sdr), 'chapters': 'omit'},
            {'output': deepcopy(sdr), 'chapters': 'omit'}])
    dnx = dict(movie, format='dnx_mov', profile_version=1, dnx_profile='dnxhr_hqx')
    case = deepcopy(base)
    case['submit']['output'] = dnx
    save('4k-dnx-mov-mxf', case, [{'output': dict(dnx, format='dnx_mxf'), 'chapters': 'omit'}])
    case = deepcopy(base)
    case['submit']['render']['range']['end'] = rational('2/3')
    case['expected_frames'] = 4
    save('4k-prores-dnx-distinct-encoders', case, [{'output': deepcopy(dnx), 'chapters': 'omit'}])
    case = deepcopy(base)
    case['submit']['render']['input']['region']['pixels'] = [640, 360]
    case['submit']['render']['range']['end'] = rational('2/3')
    case['expected_frames'] = 4
    save('gif-shared-color', case, [{'output': {'format': 'gif', 'profile_version': 1, 'background': [0, 0, 0]}, 'chapters': 'omit'}])
    case = json.loads(args.audio_source.read_text())
    save('document-audio-silence-video-share', case, [{'output': deepcopy(movie), 'chapters': 'omit'}])
    case = deepcopy(base)
    case['submit']['render']['input']['region']['pixels'] = [1920, 1080]
    case['submit']['render']['range']['end'] = rational('1')
    case['expected_frames'] = 12
    av1 = dict(movie, format='av1_mp4', profile_version=1)
    case['submit']['output'] = av1
    save('1080-av1-duplicate', case, [{'output': deepcopy(av1), 'chapters': 'omit'}])
    save('1080-av1-mixed-audio-containers', deepcopy(case), [
        {'output': dict(av1, audio_codec='aac'), 'chapters': 'omit'},
        {'output': dict(av1, format='av1_webm', audio_codec='opus'), 'chapters': 'omit'}])
    case = deepcopy(base)
    case['submit']['render']['input']['region']['pixels'] = [640, 360]
    case['submit']['render']['range']['end'] = rational('2/3')
    case['expected_frames'] = 4
    h264 = dict(movie, format='h264_mov', profile_version=1)
    case['submit']['output'] = h264
    save('h264-mixed-audio', case, [{'output': dict(h264, audio_codec='aac'), 'chapters': 'omit'}])
    (args.output / 'manifest.json').write_text(json.dumps({
        'schema_version': 1, 'source_sha256': digest(args.source),
        'audio_source_sha256': digest(args.audio_source), 'cases': records}, indent=2) + '\n')


if __name__ == '__main__':
    main()
