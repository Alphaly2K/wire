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
from .fixtures.pe import pe64
from .fixtures.amd64_basic import fixtures
from .fixtures.amd64_extra import MASK, CONST, additional, gateway_fixture, nls_conversion_fixture
from .measurement import digest, execution as execution_context, launch_protocol, paired_order, run_command, stable_command



def replay(binary, exe, expected, folder, mode, timeout, cache=None, qos_clamp=None,
           stable_launch=False, fixed_executable=False, binary_sha256=None):
    folder.mkdir(parents=True, exist_ok=True)
    if stable_launch:
        folder, exe = folder.resolve(), exe.resolve()
    with execution_context(binary, folder, stable_launch, fixed_executable, binary_sha256) as (program, cwd):
        command = [str(program), 'probe64', str(exe), '--max-instr', str(expected['work'] + 100)]
        if mode != 'timing':
            command.append('--cpu-stats' if mode == 'stats' else '--cpu-diagnostics')
        if expected.get('startup'):
            command.append('--startup')
        if cache:
            command += ['--translation-cache', cache]
        if stable_launch:
            command = stable_command(command)
        if qos_clamp:
            command = [shutil.which('taskpolicy'), '-c', qos_clamp] + command
        result = run_command(command, folder / 'report.txt', timeout, cwd=cwd)
        if fixed_executable:
            result['executable_slot_sha256'] = binary_sha256
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
    p.add_argument('--stable-launch', action='store_true',
                   help='fix guest argv0 and use each run directory as host cwd')
    p.add_argument('--fixed-executable-slot', action='store_true',
                   help='also fix the executable path and cwd across labels; requires --stable-launch')
    args = p.parse_args()
    if args.fixed_executable_slot and not args.stable_launch:
        p.error('--fixed-executable-slot requires --stable-launch')
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
    if args.cases and 'gateway-etw' in args.cases:
        from .fixtures.tracing_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-etw', data, meta))
    if args.cases and 'gateway-token-info' in args.cases:
        from .fixtures.token_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-token-info', data, meta))
    if args.cases and 'gateway-version' in args.cases:
        data, meta = gateway_fixture(args.work, version=True)
        entries.append(('gateway-version', data, meta))
    if args.cases and 'gateway-nls' in args.cases:
        data, meta = gateway_fixture(args.work, nls=True)
        entries.append(('gateway-nls', data, meta))
    if args.cases and 'nls-conversion' in args.cases:
        data, meta = nls_conversion_fixture(args.work)
        entries.append(('nls-conversion', data, meta))
    if args.cases and 'gateway-bstr' in args.cases:
        from .fixtures.bstr_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-bstr', data, meta))
    if args.cases and 'gateway-winrt' in args.cases:
        from .fixtures.winrt_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-winrt', data, meta))
    if args.cases and 'gateway-guid' in args.cases:
        from .fixtures.guid_gateway import fixture
        data, meta = fixture(args.work)
        entries.append(('gateway-guid', data, meta))
    if args.cases and 'gateway-module' in args.cases:
        from .fixtures.module_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-module', data, meta))
    if args.cases and 'gateway-file-type' in args.cases:
        from .fixtures.standard_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-file-type', data, meta))
    if args.cases and 'gateway-path' in args.cases:
        from .fixtures.path_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-path', data, meta))
    if args.cases and 'gateway-metadata' in args.cases:
        from .fixtures.metadata_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-metadata', data, meta))
    if args.cases and 'gateway-std-handle' in args.cases:
        from .fixtures.handle_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-std-handle', data, meta))
    if args.cases and 'gateway-local' in args.cases:
        from .fixtures.local_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-local', data, meta))
    if args.cases and 'gateway-event' in args.cases:
        from .fixtures.kernel_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-event', data, meta))
    if args.cases and 'gateway-wait' in args.cases:
        from .fixtures.wait_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-wait', data, meta))
    if args.cases and 'gateway-delay' in args.cases:
        from .fixtures.delay_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-delay', data, meta))
    if args.cases and 'gateway-timer' in args.cases:
        from .fixtures.timer_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-timer', data, meta))
    if args.cases and 'gateway-vmem' in args.cases:
        from .fixtures.virtual_memory_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-vmem', data, meta))
    if args.cases and 'gateway-heap' in args.cases:
        from .fixtures.heap_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-heap', data, meta))
    if args.cases and 'gateway-local-memory' in args.cases:
        from .fixtures.local_memory_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-local-memory', data, meta))
    if args.cases and 'gateway-system-profile' in args.cases:
        from .fixtures.system_profile_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-system-profile', data, meta))
    if args.cases and 'gateway-calendar' in args.cases:
        from .fixtures.calendar_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-calendar', data, meta))
    if args.cases and 'gateway-multimedia' in args.cases:
        from .fixtures.multimedia_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-multimedia', data, meta))
    if args.cases and 'gateway-inline-sync' in args.cases:
        from .fixtures.inline_sync_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-inline-sync', data, meta))
    if args.cases and 'gateway-critical-section' in args.cases:
        from .fixtures.critical_section_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-critical-section', data, meta))
    if args.cases and 'gateway-thread-properties' in args.cases:
        from .fixtures.thread_properties_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-thread-properties', data, meta))
    if args.cases and 'gateway-thread-control' in args.cases:
        from .fixtures.thread_control_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-thread-control', data, meta))
    if args.cases and 'gateway-com' in args.cases:
        from .fixtures.com_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-com', data, meta))
    if args.cases and 'gateway-guid-create' in args.cases:
        from .fixtures.guid_create_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-guid-create', data, meta))
    if args.cases and 'gateway-processor-count' in args.cases:
        from .fixtures.basic_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-processor-count', data, meta))
    if args.cases and 'gateway-rectangle' in args.cases:
        from .fixtures.geometry_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-rectangle', data, meta))
    if args.cases and 'gateway-cursor' in args.cases:
        from .fixtures.cursor_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-cursor', data, meta))
    if args.cases and 'gateway-images' in args.cases:
        from .fixtures.images_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-images', data, meta))
    if args.cases and 'gateway-display' in args.cases:
        from .fixtures.display_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-display', data, meta))
    if args.cases and 'gateway-gdi-profile' in args.cases:
        from .fixtures.gdi_profile_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-gdi-profile', data, meta))
    if args.cases and 'gateway-messages' in args.cases:
        from .fixtures.messages_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-messages', data, meta))
    if args.cases and 'gateway-window-control' in args.cases:
        from .fixtures.window_control_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-window-control', data, meta))
    if args.cases and 'gateway-window-state' in args.cases:
        from .fixtures.window_state_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-window-state', data, meta))
    if args.cases and 'gateway-window' in args.cases:
        from .fixtures.window_queries_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-window', data, meta))
    if args.cases and 'gateway-classes' in args.cases:
        from .fixtures.classes_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-classes', data, meta))
    if args.cases and 'gateway-desktop' in args.cases:
        from .fixtures.desktop_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-desktop', data, meta))
    for name, ime in [('gateway-raw-input', False), ('gateway-ime', True)]:
        if args.cases and name in args.cases:
            from .fixtures.input_profile_gateway import x64
            data, meta = x64(args.work, ime=ime)
            entries.append((name, data, meta))
    if args.cases and 'gateway-keyboard' in args.cases:
        from .fixtures.keyboard_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-keyboard', data, meta))
    if args.cases and 'gateway-keyboard-map' in args.cases:
        from .fixtures.keyboard_gateway import x64_layout
        data, meta = x64_layout(args.work)
        entries.append(('gateway-keyboard-map', data, meta))
    if args.cases and 'gateway-address-wait' in args.cases:
        from .fixtures.address_wait_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-address-wait', data, meta))
    if args.cases and 'gateway-context' in args.cases:
        from .fixtures.context_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-context', data, meta))
    if args.cases and 'gateway-clock' in args.cases:
        from .fixtures.clock_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-clock', data, meta))
    if args.cases and 'gateway-slist' in args.cases:
        from .fixtures.slist_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-slist', data, meta))
    if args.cases and 'gateway-section' in args.cases:
        from .fixtures.section_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-section', data, meta))
    if args.cases and 'gateway-search' in args.cases:
        from .fixtures.search_gateway import x64
        data, meta = x64(args.work)
        entries.append(('gateway-search', data, meta))
    if args.cases:
        if set(args.cases) - {name for name, _, _ in entries}:
            p.error('unknown fixture name')
        entries = [entry for entry in entries if entry[0] in args.cases]
    manifest = dict(binaries={n: dict(path=str(b), sha256=digest(b)) for n, b in binaries.items()},
                    caches=caches,
                    script_sha256=digest(Path(__file__)),
                    measurement_sha256=digest(Path(__file__).with_name('measurement.py')),
                    mode=args.mode,
                    pairs=args.pairs, requested_work=args.work, qos_clamp=args.qos_clamp,
                    taskpolicy_sha256=digest(Path(shutil.which('taskpolicy'))) if args.qos_clamp else None,
                    inputs={})
    results = {}
    manifest['launch'] = launch_protocol(args.stable_launch, args.fixed_executable_slot)
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
                           caches.get(version), args.qos_clamp, args.stable_launch,
                           args.fixed_executable_slot, manifest['binaries'][version]['sha256'])
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
