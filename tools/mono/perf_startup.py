#!/usr/bin/env python3
"""Frozen serial AB/BA comparison of an exact finite native Mono startup boundary."""
import argparse
import json
from pathlib import Path
import statistics
import subprocess
import sys
from probe import REPO, sha256


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--before', type=Path, required=True)
    parser.add_argument('--after', type=Path, required=True)
    parser.add_argument('--game', type=Path, required=True)
    parser.add_argument('--runtime-output', type=Path, required=True)
    parser.add_argument('--graphics-backend', type=Path, required=True)
    parser.add_argument('--moltenvk', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--pairs', type=int, default=3)
    parser.add_argument('--boundary', default='assembly-lookup')
    parser.add_argument('--max-instr', type=int, default=500000000)
    args = parser.parse_args()
    output = args.output.resolve()
    if not output.is_relative_to(REPO / 'wire-out') or output.exists() or not 1 <= args.pairs <= 10:
        parser.error('use a fresh ignored wire-out output and 1..10 pairs')
    output.mkdir(parents=True)
    binaries = {key: path.resolve() for key, path in [('before', args.before), ('after', args.after)]}
    hashes = {key: sha256(path) for key, path in binaries.items()}
    inputs = None
    identity = None
    rows = []
    keys = ('pending_boundary', 'native_classlib_invokes', 'native_metadata_queries',
            'native_metadata_handles', 'native_images_opened', 'native_assembly_image_getters',
            'native_assembly_name_parses', 'native_assembly_name_hits',
            'native_attribute_objects', 'native_attribute_field_reads', 'native_attribute_arrays',
            'native_error_completions', 'native_external_strong_handles')
    for pair in range(args.pairs):
        for key in (('before', 'after') if pair % 2 == 0 else ('after', 'before')):
            if sha256(binaries[key]) != hashes[key]:
                raise RuntimeError('frozen executable changed')
            folder = output / f'pair-{pair + 1}-{key}'
            command = [sys.executable, str(REPO / 'tools/mono/probe_startup.py'),
                       '--game', str(args.game.resolve()), '--runtime-output', str(args.runtime_output.resolve()),
                       '--wire', str(binaries[key]), '--skip-etw', '--boundary', args.boundary,
                       '--max-instr', str(args.max_instr), '--graphics-backend', str(args.graphics_backend.resolve()),
                       '--moltenvk', str(args.moltenvk.resolve()), '--output', str(folder)]
            with (output / f'pair-{pair + 1}-{key}.log').open('w') as log:
                subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, check=True)
            evidence = json.loads((folder / 'results.json').read_text())
            external = {name: value['sha256'] for name, value in json.loads((folder / 'inputs.json').read_text()).items() if name != 'wire'}
            current = {name: evidence[name] for name in keys}
            if evidence['full_game_backend'] or evidence['returncode'] != 0:
                raise RuntimeError('not a successful finite diagnostic run')
            if inputs is None:
                inputs, identity = external, current
            if inputs != external or current != identity:
                raise RuntimeError('inputs or exact initialization counts/endpoint changed; do not compare timing')
            row = {'pair': pair + 1, 'configuration': key, 'cpu_seconds': evidence['cpu_user_seconds'] + evidence['cpu_system_seconds'],
                   'wall_seconds': evidence['elapsed_seconds'], 'evidence': str(folder / 'results.json')}
            rows.append(row)
            (output / 'samples.json').write_text(json.dumps(rows, indent=2) + '\n')
            print(json.dumps(row), flush=True)
    medians = {key: {metric: statistics.median(row[metric] for row in rows if row['configuration'] == key)
                     for metric in ('cpu_seconds', 'wall_seconds')} for key in binaries}
    result = {'scope': f"finite native Mono initialization boundary={args.boundary}, pending={identity['pending_boundary']}; not complete game startup or FPS",
              'full_game_backend': False, 'binary_hashes': hashes, 'external_input_hashes': inputs,
              'matched_initialization': identity, 'samples': rows, 'medians': medians,
              'cpu_speedup': medians['before']['cpu_seconds'] / medians['after']['cpu_seconds']}
    (output / 'results.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({'medians': medians, 'cpu_speedup': result['cpu_speedup']}), flush=True)


if __name__ == '__main__':
    main()
