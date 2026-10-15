#!/usr/bin/env python3
"""Replay original deterministic PE64 fixtures on frozen interpreters, serially.

Timing excludes counters and sampling. Preserve every AB/BA round, CPU time,
peak RSS, binary/input hashes, exact work/retired and algebraic checksums.
"""
import argparse
import json
from pathlib import Path
import re
import shutil
import statistics
import struct
import sys

sys.dont_write_bytecode = True
from amd64_perf import fixtures, pe64
from perf.measurement import digest, paired_order, run_command

MASK = (1 << 64) - 1
CONST = 0x1122334455667788


def additional(name, work):
    code = bytearray()

    def emit(value):
        code.extend(bytes.fromhex(value))

    def imm(prefix, value, size=8):
        emit(prefix + int(value).to_bytes(size, 'little').hex())

    if name == 'sse-register':
        n = max(1, work // 6)
        imm('48b8', 1); emit('66480f6ec0')
        imm('48bb', CONST); emit('66480f6ecb')
        imm('41bf', n, 4); loop = len(code)
        emit('660fd4c1 660fd4c1 660ffbc1 660f6fd0')
        expected = (1 + n * CONST) & MASK; retired = total = 6 * n + 7
    elif name == 'sse-gpr-transfer':
        n = max(1, work // 6)
        imm('48b8', 1); imm('41bf', n, 4); loop = len(code)
        emit('66480f6ec0 66480f7ec3 4883c001 4831c3')
        expected = n + 1; retired = total = 6 * n + 3
    elif name == 'shift-register':
        n = max(1, work // 7)
        imm('48b8', 1); imm('48bb', CONST)
        imm('41bf', n, 4); loop = len(code)
        emit('48c1c00d 48c1c80d 48d1e3 48d1eb 4801d8')
        expected = (1 + n * CONST) & MASK; retired = total = 7 * n + 4
    else:
        assert name == 'rep-memory'
        n = max(1, work // 2055)
        imm('48b8', CONST); imm('41bf', n, 4); loop = len(code)
        emit('488dbc2400d0ffff'); imm('b9', 1024, 4); emit('f348ab')
        emit('488db42400d0ffff 488dbc2400a0ffff')
        imm('b9', 1024, 4); emit('f348a5')
        expected = 4 * CONST & MASK; retired = 9 * n + 7; total = 2055 * n + 7
    emit('49ffcf')
    at = len(code); emit('0f85' + struct.pack('<i', loop - (at + 6)).hex())
    if name == 'sse-register':
        emit('66480f7ed0')
    if name == 'rep-memory':
        emit('488b842400a0ffff 48038424f8bfffff 4803842400d0ffff 48038424f8efffff')
    emit('c3')
    return pe64(code), dict(rax=hex(expected), retired=retired, work=total,
                           bytes_written=16384 * n if name == 'rep-memory' else 0)


def gateway_fixture(work, version=False, nls=False):
    n = max(1, work // 5)
    code = bytearray.fromhex('4156 4157 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4531f6 4883ec28')
    loop = len(code)
    call = len(code); code += bytes.fromhex('ff15') + struct.pack('<i', 0x1190 - (0x1000 + call + 6))
    code += bytes.fromhex('4901c6 41ffcf')
    at = len(code); code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89f0 4883c428 415f 415e c3')
    code.extend(bytes(512 - len(code)))
    struct.pack_into('<I', code, 0x100, 0x1180)
    struct.pack_into('<I', code, 0x10c, 0x11c0)
    struct.pack_into('<I', code, 0x110, 0x1190)
    struct.pack_into('<Q', code, 0x180, 0x11d0)
    code[0x1c0:0x1cd] = b'kernel32.dll\0'
    symbol = b'GetACP\0' if nls else b'GetVersion\0' if version else b'GetCurrentThreadId\0'
    code[0x1d2:0x1d2 + len(symbol)] = symbol
    image = bytearray(pe64(code))
    struct.pack_into('<II', image, 0x98 + 112 + 8, 0x1100, 40)
    value = 932 if nls else 0x4a65000a if version else 1
    return image, dict(rax=hex(n * value & MASK), retired=4 * n + 10, work=5 * n + 10, api_calls=n, startup=True)


def nls_conversion_fixture(work):
    n = max(1, work // 12)
    code = bytearray.fromhex('4156 4157 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4531f6 4883ec38')
    loop = len(code)
    code += bytes.fromhex('488d442430 4889442420 c744242802000000 b9a4030000 31d2')
    at = len(code)
    code += bytes.fromhex('4c8d05') + struct.pack('<i', 0x11f0 - (0x1000 + at + 7))
    code += bytes.fromhex('41b9ffffffff')
    at = len(code)
    code += bytes.fromhex('ff15') + struct.pack('<i', 0x1190 - (0x1000 + at + 6))
    code += bytes.fromhex('4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('8b442430 4c01f0 4883c438 415f 415e c3')
    code.extend(bytes(512 - len(code)))
    for offset, value in [(0x100, 0x1180), (0x10c, 0x11c0), (0x110, 0x1190)]:
        struct.pack_into('<I', code, offset, value)
    struct.pack_into('<Q', code, 0x180, 0x11d0)
    code[0x1c0:0x1cd] = b'kernel32.dll\0'
    symbol = b'MultiByteToWideChar\0'
    code[0x1d2:0x1d2 + len(symbol)] = symbol
    code[0x1f0:0x1f2] = b'A\0'
    image = bytearray(pe64(code))
    struct.pack_into('<II', image, 0x98 + 112 + 8, 0x1100, 40)
    return image, dict(rax=hex(65 + 2 * n), retired=11 * n + 11, work=12 * n + 11, api_calls=n, startup=True)


def replay(binary, exe, expected, folder, mode, timeout, cache=None, qos_clamp=None):
    folder.mkdir(parents=True, exist_ok=True)
    command = [str(binary), 'probe64', str(exe), '--max-instr', str(expected['work'] + 100)]
    if mode != 'timing':
        command.append('--cpu-stats' if mode == 'stats' else '--cpu-diagnostics')
    if expected.get('startup'):
        command.append('--startup')
    if cache:
        command += ['--translation-cache', cache]
    if qos_clamp:
        command = [shutil.which('taskpolicy'), '-c', qos_clamp] + command
    result = run_command(command, folder / 'report.txt', timeout)
    report = (folder / 'report.txt').read_text()
    returned = re.search(r'entry returned RAX=(0x[0-9a-f]+); retired=(\d+)', report)
    execution = re.search(r'AMD64 execution: retired=(\d+) work=(\d+)', report)
    if returned and execution:
        result.update(rax=returned[1], retired=int(returned[2]), work=int(execution[2]))
    if expected.get('api_calls'):
        match = re.search(r'phase=.*; API calls=(\d+)', report)
        result['api_calls'] = int(match[1]) if match else None
    result['validated'] = (result['exit_code'] == 0 and returned is not None
                           and result.get('retired') == expected['retired']
                           and result.get('work') == expected['work']
                           and (expected.get('rax') is None
                                or int(result['rax'], 16) == int(expected['rax'], 16))
                           and (expected.get('api_calls') is None or result.get('api_calls') == expected['api_calls']))
    (folder / 'run.json').write_text(json.dumps(result, indent=2) + '\n')
    if not result['validated']:
        raise RuntimeError(f'fixture failed: {folder}\n{report}')
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--binary', action='append', required=True, help='NAME=FROZEN_PATH')
    p.add_argument('--out', type=Path, required=True)
    p.add_argument('--cache', action='append', default=[], help='BINARY_NAME=LAYOUT')
    p.add_argument('--work', type=int, default=100_000_000)
    p.add_argument('--pairs', type=int, default=3)
    p.add_argument('--cases', nargs='+')
    p.add_argument('--mode', choices=['timing', 'stats', 'diagnostics'], default='timing')
    p.add_argument('--timeout', type=float, default=120)
    p.add_argument('--qos-clamp', choices=['utility', 'background', 'maintenance'],
                   help='optional macOS taskpolicy clamp applied equally to every binary')
    args = p.parse_args()
    if args.work < 1 or args.pairs < 1 or args.timeout <= 0:
        p.error('work, pairs and timeout must be positive')
    if args.qos_clamp and sys.platform != 'darwin':
        p.error('--qos-clamp requires macOS taskpolicy')
    if args.qos_clamp and shutil.which('taskpolicy') is None:
        p.error('taskpolicy is unavailable')
    binaries = {}
    caches = dict(item.split('=', 1) for item in args.cache)
    for item in args.binary:
        name, path = item.split('=', 1)
        if not re.fullmatch(r'[a-zA-Z0-9_-]+', name) or name in binaries:
            p.error('binary names must be unique simple identifiers')
        binaries[name] = Path(path).resolve(strict=True)
    args.out = args.out.resolve(); args.out.mkdir(parents=True, exist_ok=True)
    entries = []
    per_iteration = {'integer': 9, 'memory': 8, 'simd-mixed': 8,
                     'large-code': 16386, 'indirect-call': 9}
    for name, data in fixtures(args.work):
        per = per_iteration[name]
        total = max(1, args.work // per) * per + (9 if name == 'indirect-call' else 8)
        entries.append((name, data, dict(work=total, retired=total, rax=None)))
    for name in ['sse-register', 'sse-gpr-transfer', 'shift-register', 'rep-memory']:
        data, meta = additional(name, args.work)
        entries.append((name, data, meta))
    if args.cases and 'gateway-thread-id' in args.cases:
        data, meta = gateway_fixture(args.work)
        entries.append(('gateway-thread-id', data, meta))
    if args.cases and 'gateway-version' in args.cases:
        data, meta = gateway_fixture(args.work, version=True)
        entries.append(('gateway-version', data, meta))
    if args.cases and 'gateway-nls' in args.cases:
        data, meta = gateway_fixture(args.work, nls=True)
        entries.append(('gateway-nls', data, meta))
    if args.cases and 'nls-conversion' in args.cases:
        data, meta = nls_conversion_fixture(args.work)
        entries.append(('nls-conversion', data, meta))
    if args.cases and 'gateway-guid' in args.cases:
        from perf.fixtures.guid_gateway import fixture
        data, meta = fixture(args.work)
        entries.append(('gateway-guid', data, meta))
    if args.cases and 'gateway-module' in args.cases:
        from perf.fixtures.module_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-module', data, meta))
    if args.cases and 'gateway-file-type' in args.cases:
        from perf.fixtures.standard_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-file-type', data, meta))
    if args.cases and 'gateway-path' in args.cases:
        from perf.fixtures.path_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-path', data, meta))
    if args.cases and 'gateway-metadata' in args.cases:
        from perf.fixtures.metadata_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-metadata', data, meta))
    if args.cases and 'gateway-std-handle' in args.cases:
        from perf.fixtures.handle_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-std-handle', data, meta))
    if args.cases and 'gateway-section' in args.cases:
        from perf.fixtures.section_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-section', data, meta))
    if args.cases and 'gateway-search' in args.cases:
        from perf.fixtures.search_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-search', data, meta))
    if args.cases:
        if set(args.cases) - {name for name, _, _ in entries}:
            p.error('unknown fixture name')
        entries = [entry for entry in entries if entry[0] in args.cases]
    manifest = dict(binaries={n: dict(path=str(b), sha256=digest(b)) for n, b in binaries.items()},
                    caches=caches,
                    script_sha256=digest(Path(__file__)),
                    measurement_sha256=digest(Path(__file__).parent / 'perf' / 'measurement.py'),
                    mode=args.mode,
                    pairs=args.pairs, requested_work=args.work, qos_clamp=args.qos_clamp,
                    taskpolicy_sha256=digest(Path(shutil.which('taskpolicy'))) if args.qos_clamp else None,
                    inputs={})
    results = {}
    for name, data, expected in entries:
        exe = args.out / f'{name}.exe'; exe.write_bytes(data)
        manifest['inputs'][name] = dict(sha256=digest(exe), **expected)
        (args.out / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
        runs = []
        for pair in range(args.pairs):
            order = paired_order(binaries, pair)
            for version in order:
                r = replay(binaries[version], exe, expected,
                           args.out / name / f'pair-{pair}-{version}', args.mode, args.timeout,
                           caches.get(version), args.qos_clamp)
                if runs and (r['rax'], r['retired'], r['work']) != (
                        runs[0]['rax'], runs[0]['retired'], runs[0]['work']):
                    raise RuntimeError(f'guest state differs: {name} {version}')
                r.update(pair=pair, version=version); runs.append(r)
                print(f'{name} pair={pair} {version}: wall={r["wall_s"]:.4f} '
                      f'cpu={r["cpu_s"]:.4f} RSS={r["peak_rss_bytes"]} validated', flush=True)
            results[name] = dict(runs=runs, medians={v: {
                key: statistics.median(r[key] for r in runs if r['version'] == v)
                for key in ['wall_s', 'cpu_s', 'peak_rss_bytes']} for v in binaries})
            (args.out / 'compare.json').write_text(json.dumps(results, indent=2) + '\n')


if __name__ == '__main__':
    main()
