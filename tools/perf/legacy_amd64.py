#!/usr/bin/env python3
"""Alternate frozen AMD64 interpreters on deterministic PEs or a real startup.

Timing runs never enable CPU statistics or sampling. Synthetic PEs are generated
locally, contain no third-party code, and must return identical checksums and
retired counts. Real startup is timer/thread dependent; report its state rather
than pretending two runs follow an identical guest path.
"""
import argparse
import json
import os
from pathlib import Path
import re
import statistics
import struct
import subprocess
import time


from .fixtures.pe import pe64
from .fixtures.amd64_basic import fixtures


def run(command, cwd, folder, sample_seconds=0, player_log=None):
    folder.mkdir(parents=True, exist_ok=True)
    with (folder / 'report.txt').open('w') as report:
        start = time.perf_counter()
        process = subprocess.Popen(command, cwd=cwd, stdout=report, stderr=subprocess.STDOUT)
        sampler_status = None
        if sample_seconds:
            sampler = subprocess.run(['/usr/bin/sample', str(process.pid), str(sample_seconds), '1',
                                      '-mayDie', '-fullPaths', '-file', str(folder / 'sample.txt')],
                                     stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
                                     timeout=sample_seconds + 30)
            (folder / 'sampler.txt').write_text(sampler.stdout)
            sampler_status = sampler.returncode
        _, status, usage = os.wait4(process.pid, 0)
        process.returncode = os.waitstatus_to_exitcode(status)
        result = dict(command=command, cwd=str(cwd), wall_s=time.perf_counter() - start,
                      cpu_s=usage.ru_utime + usage.ru_stime, exit_code=process.returncode)
    text = (folder / 'report.txt').read_text()
    returned = re.search(r'entry returned RAX=(0x[0-9a-f]+); retired=(\d+)', text)
    state = re.search(r'^phase=(.*); retired=(\d+); budget work=(\d+); RIP=(0x[0-9a-f]+); API calls=(\d+)',
                      text, re.M)
    if returned:
        result.update(rax=returned[1], retired=int(returned[2]))
    elif state:
        result.update(phase=state[1], retired=int(state[2]), work=int(state[3]),
                      rip=state[4], api_calls=int(state[5]))
    else:
        raise RuntimeError(f'missing execution result: {folder}/report.txt')
    result['process_mips'] = result['retired'] / result['wall_s'] / 1e6
    if sample_seconds:
        result['sampler_status'] = sampler_status
        if sampler_status:
            raise RuntimeError(f'sampling failed: {folder}/sampler.txt')
    (folder / 'timing.json').write_text(json.dumps(result, indent=2) + '\n')
    if player_log:
        (folder / 'Player.log').write_bytes((Path(cwd) / player_log).read_bytes())
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('mode', choices=['matrix', 'game', 'capture'])
    p.add_argument('--before', type=Path)
    p.add_argument('--cached', type=Path)
    p.add_argument('--after', type=Path, default=Path('target/release/wire'))
    p.add_argument('--out', type=Path, required=True)
    p.add_argument('--pairs', type=int, default=3)
    p.add_argument('--work', type=int, default=50_000_000)
    p.add_argument('--cwd', type=Path)
    p.add_argument('--exe', default='AliceToPapaADV_1-0-0.exe')
    p.add_argument('--graphics-backend', type=Path)
    p.add_argument('--moltenvk', type=Path)
    p.add_argument('--window', action='store_true', help='include the real AppKit/GPU Present path')
    p.add_argument('--sample-seconds', type=int, default=8)
    p.add_argument('--expected-error', help='real startup must stop at this exact probe64 error instead of its budget')
    p.add_argument('--skip-coverage', action='store_true', help='omit the separate statistics replay')
    p.add_argument('--player-log', type=Path, help='preserve this log from the runtime directory after each replay')
    args = p.parse_args()
    if args.pairs < 1 or args.work < 1 or args.sample_seconds < 1:
        p.error('pairs, work and sample duration must be positive')
    if args.mode != 'capture' and not args.before:
        p.error('--before is required for comparisons')
    if args.mode != 'matrix' and not args.cwd:
        p.error('--cwd is required for real startup')
    if bool(args.graphics_backend) != bool(args.moltenvk):
        p.error('graphics backend and MoltenVK must be provided together')
    args.out = args.out.resolve(); args.out.mkdir(parents=True, exist_ok=True)
    versions = {n: str(v.resolve()) for n in ['before', 'cached', 'after'] if (v := getattr(args, n))}
    inputs = []
    if args.mode == 'matrix':
        for name, data in fixtures(args.work):
            path = args.out / f'{name}.exe'; path.write_bytes(data)
            inputs.append((name, str(path), args.out))
    else:
        inputs = [('alice-startup', args.exe, args.cwd.resolve())]
    results = {}
    for name, exe, cwd in inputs:
        runs = []
        extra = [] if args.mode == 'matrix' else ['--startup']
        if args.window:
            extra += ['--window']
        if args.graphics_backend:
            extra += ['--graphics-backend', str(args.graphics_backend.resolve()),
                      '--moltenvk', str(args.moltenvk.resolve())]
        def command(version):
            return [versions[version], 'probe64', exe, '--max-instr', str(args.work + 100 if args.mode == 'matrix' else args.work)] + extra
        if args.mode == 'capture':
            run(command('after'), cwd, args.out / name, args.sample_seconds, args.player_log)
            continue
        for pair in range(args.pairs):
            order = list(versions) if pair % 2 == 0 else list(reversed(versions))
            for version in order:
                r = run(command(version), cwd, args.out / name / f'pair-{pair}-{version}', player_log=args.player_log)
                r.update(pair=pair, version=version)
                if args.mode == 'matrix':
                    if r['exit_code'] or not r.get('rax'):
                        raise RuntimeError(f'synthetic execution failed: {name} {version}')
                    if runs and (r['rax'], r['retired']) != (runs[0]['rax'], runs[0]['retired']):
                        raise RuntimeError(f'guest state/count mismatch: {name} {version}')
                else:
                    text = (args.out / name / f'pair-{pair}-{version}' / 'report.txt').read_text()
                    expected = ('probe64: ' + args.expected_error) if args.expected_error else None
                    reached = (expected in text.splitlines()) if expected else r.get('work') == args.work
                    if r['exit_code'] != 1 or not reached:
                        raise RuntimeError(f'startup did not reach requested boundary: {name} {version}')
                runs.append(r)
                print(f'{name} pair {pair} {version}: {r["wall_s"]:.3f}s, '
                      f'{r["process_mips"]:.3f} process MIPS', flush=True)
        medians = {version: {key: statistics.median(r[key] for r in runs if r['version'] == version)
                             for key in ['wall_s', 'cpu_s', 'process_mips']}
                   for version in versions}
        results[name] = dict(medians=medians, runs=runs)
        (args.out / 'compare.json').write_text(json.dumps(results, indent=2) + '\n')
        # Coverage is a separate untimed diagnostic, never mixed into AB timing.
        if not args.skip_coverage:
            run(command('after') + ['--cpu-stats'], cwd, args.out / name / 'coverage', player_log=args.player_log)
        print(name, json.dumps(medians), flush=True)


if __name__ == '__main__':
    main()
