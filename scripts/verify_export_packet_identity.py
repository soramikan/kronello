#!/usr/bin/env python3
"""Audit retained A/B files without altering their original timing receipts."""
import argparse
from fractions import Fraction
import hashlib
import json
from pathlib import Path
import subprocess
from perf_export_worker import canonical, digest


def vt_packet_hash(packet):
    data = b''.join(bytes.fromhex(row.split(':', 1)[1].split('  ')[0].strip())
                    for row in packet['data'].strip().splitlines())
    normalized = bytearray()
    omitted = 0
    while data:
        if len(data) < 4:
            raise RuntimeError('truncated length-prefixed H.264 packet')
        length = int.from_bytes(data[:4], 'big')
        nal, data = data[4:4 + length], data[4 + length:]
        if len(nal) != length or not nal:
            raise RuntimeError('invalid H.264 NAL length')
        # Only this observed VideoToolbox private unregistered-user-data SEI
        # is operational. Retain all slices, SPS/PPS and every other SEI.
        rbsp = nal.replace(b'\x00\x00\x03', b'\x00\x00')
        if (len(rbsp) == 49 and rbsp[:3] == b'\x06\x05\x2d'
                and rbsp[3:19] == bytes.fromhex('47564adc5c4c433f94efc5113cd143a8')
                and rbsp[-1] == 0x80):
            omitted += 1
        else:
            normalized.extend(length.to_bytes(4, 'big'))
            normalized.extend(nal)
    return hashlib.sha256(normalized).hexdigest(), omitted


def packet_oracle(path, ffprobe, normalize_vt=False):
    command = [ffprobe, '-v', 'error', '-show_streams',
        '-show_packets', '-show_data_hash', 'sha256', '-show_chapters', '-show_format',
        '-of', 'json']
    if normalize_vt:
        command.append('-show_data')
    probe = json.loads(subprocess.check_output(command + [str(path)], timeout=600))
    streams = []
    for stream in probe['streams']:
        time_base = Fraction(stream['time_base'])
        packets = []
        for packet in probe['packets']:
            if packet['stream_index'] != stream['index']:
                continue
            record = {key: str(Fraction(packet[key]) * time_base)
                      for key in ('pts', 'dts', 'duration') if key in packet}
            record['data_hash'] = packet['data_hash']
            if normalize_vt and stream['codec_type'] == 'video':
                record['data_hash'], record['omitted_vt_private_sei_count'] = vt_packet_hash(packet)
            record['side_data'] = packet.get('side_data_list', [])
            packets.append(record)
        streams.append({key: stream.get(key) for key in ('codec_type', 'codec_name',
            'pix_fmt', 'width', 'height', 'sample_rate', 'channels', 'channel_layout',
            'color_primaries', 'color_transfer', 'color_space', 'color_range',
            'initial_padding', 'extradata_hash')} | {'packets': packets})
    return {'streams': streams, 'chapters': probe.get('chapters', []),
            'snapshot_tags': {k: v for k, v in probe['format'].get('tags', {}).items() if 'snapshot' in k}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--ffprobe', default='ffprobe')
    parser.add_argument('--normalize-vt-private-sei', action='store_true')
    parser.add_argument('evidence', type=Path, nargs='+')
    args = parser.parse_args()
    records = []
    for source in args.evidence:
        receipt = json.loads(source.read_text())
        if not receipt.get('comparison', {}).get('all_decoded_bits_pts_layout_equal'):
            raise RuntimeError(f'incomplete comparison: {source}')
        trials = []
        for trial in receipt['trials']:
            outputs = []
            for output, original in zip(trial['report']['outputs'], trial['oracles'], strict=True):
                path = Path(output['output'])
                if digest(path) != original['encoded_sha256']:
                    raise RuntimeError(f'retained file changed: {path}')
                normalize_vt = (args.normalize_vt_private_sei and
                    output.get('video', {}).get('encoder') == 'h264_videotoolbox')
                oracle = packet_oracle(path, args.ffprobe, normalize_vt)
                if digest(path) != original['encoded_sha256']:
                    raise RuntimeError(f'retained file changed during probe: {path}')
                outputs.append({'path': str(path), 'encoded_sha256': original['encoded_sha256'],
                    'packet_metadata_sha256': hashlib.sha256(canonical(oracle)).hexdigest(),
                    'videotoolbox_private_sei_normalization': normalize_vt,
                    'packet_counts': [len(s['packets']) for s in oracle['streams']]})
            trials.append({'pair': trial['pair'], 'mode': trial['mode'], 'outputs': outputs})
        for pair in sorted({t['pair'] for t in trials}):
            before, after = [next(t for t in trials if t['pair'] == pair and t['mode'] == mode)
                             for mode in ('before', 'after')]
            for left, right in zip(before['outputs'], after['outputs'], strict=True):
                if left['packet_metadata_sha256'] != right['packet_metadata_sha256']:
                    raise RuntimeError(f'packet payload/time/color/chapters mismatch: {source}, {pair}')
        records.append({'raw_evidence': str(source.resolve()), 'raw_sha256': digest(source),
                        'all_declared_packet_comparisons_equal': True,
                        'all_packet_payload_time_color_chapters_equal': not any(
                            output['videotoolbox_private_sei_normalization']
                            for trial in trials for output in trial['outputs']),
                        'trials': trials})
    args.output.write_text(json.dumps({'schema_version': 1,
        'videotoolbox_private_sei_normalization_enabled': args.normalize_vt_private_sei,
        'records': records}, indent=2) + '\n')
    print(f'verified {len(records)} retained comparisons')


if __name__ == '__main__':
    main()
