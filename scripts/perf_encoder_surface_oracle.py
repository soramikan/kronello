#!/usr/bin/env python3
"""Decode fixed Metal/VideoToolbox trials outside the timed interval."""
import argparse
import csv
import json
import re
from pathlib import Path
from perf_export_worker import digest, independent_oracle, percentile, semantic_streams


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--directory', type=Path, required=True)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    trials = list(csv.DictReader((args.directory / 'evidence.csv').open()))
    assert len(trials) >= 6
    for trial in trials:
        for key in ('total_ms', 'prepare_ms', 'conversion_ms', 'encode_ms'):
            trial[key] = float(trial[key])
        for key in ('pair', 'readback_bytes', 'frames'):
            trial[key] = int(trial[key])
        file = args.directory / f"{trial['pair']}-{trial['mode']}.h264"
        trial['oracle'] = independent_oracle(file, 'ffmpeg', 'ffprobe')
        stream, = trial['oracle']['streams']
        assert stream['codec'] == 'h264' and stream['pixel_format'] == 'yuv420p'
        assert stream['color_range'] == 'tv'
        assert all(stream[k] == 'bt709' for k in ('color_primaries', 'color_transfer', 'color_matrix'))
        assert len(stream['packets']) == trial['frames']
        assert stream['decoded_bytes'] == stream['width'] * stream['height'] * 3 // 2 * trial['frames']
        expected = 0 if trial['mode'] == 'gpu' else stream['width'] * stream['height'] * 8 * trial['frames']
        assert trial['readback_bytes'] == expected
    comparisons = []
    for pair in sorted({t['pair'] for t in trials}):
        cpu, = [t for t in trials if t['pair'] == pair and t['mode'] == 'cpu']
        gpu, = [t for t in trials if t['pair'] == pair and t['mode'] == 'gpu']
        assert semantic_streams(cpu['oracle']) == semantic_streams(gpu['oracle']), f'decoded/time/color pair {pair}'
        nals = [re.split(rb'\x00\x00\x00?\x01', (args.directory / f'{pair}-{mode}.h264').read_bytes())[1:] for mode in ('cpu', 'gpu')]
        assert len(nals[0]) == len(nals[1])
        differences = []
        for index, (left, right) in enumerate(zip(*nals)):
            if left == right:
                continue
            # Retain and identify the actual VT private SEI differences. Do not
            # rewrite files or label compressed payloads byte-identical.
            prefix = bytes.fromhex('06052d47564adc5c4c433f94efc5113cd143a8')
            assert left.startswith(prefix) and right.startswith(prefix)
            assert len(left) == len(right) == 53
            differences.append({'nal': index, 'cpu_hex': left.hex(), 'gpu_hex': right.hex()})
        comparisons.append({'pair': pair, 'all_picture_and_parameter_nals_equal': True,
                            'private_sei_differences': differences, 'decoded_time_color_equal': True,
                            'packet_payload_equal': cpu['oracle']['streams'] == gpu['oracle']['streams'],
                            'encoded_bytes_equal': cpu['oracle']['encoded_sha256'] == gpu['oracle']['encoded_sha256']})
    summary = {}
    for mode in ('cpu', 'gpu'):
        selected = [t for t in trials if t['mode'] == mode]
        warm = selected[1:]
        summary[mode] = {'first_ms': selected[0]['total_ms'],
                         'warm_p50_ms': percentile([t['total_ms'] for t in warm], 0.5),
                         'warm_p95_ms': percentile([t['total_ms'] for t in warm], 0.95),
                         'warm_conversion_p50_ms': percentile([t['conversion_ms'] for t in warm], 0.5)}
    evidence = {'trials': trials, 'comparisons': comparisons, 'summary': summary,
                'provenance': {'binary_sha256': digest(args.binary),
                               'source_evidence_sha256': digest(args.directory / 'evidence.csv'),
                               'scope': 'Same binary, generated Metal RGBA16F fixture, SDR premultiplied black matte, H.264 hardware encoder, 24 fps. Fresh native objects per trial. Compile/test excluded. Independent decode excluded. Annex B has no authored container timestamps; native callback checks every exact 1/24 PTS and duration. Not the product renderer or worker.'}}
    args.output.write_text(json.dumps(evidence, indent=2) + '\n')
    print(json.dumps(summary, indent=2))
    print(json.dumps(comparisons, indent=2))


if __name__ == '__main__':
    main()
