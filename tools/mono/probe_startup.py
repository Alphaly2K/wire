#!/usr/bin/env python3
"""Drive actual Unity into the exact-sample finite native Mono startup provider."""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import time
from probe import REPO, sha256


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--game', type=Path, required=True, help='read-only original EXE')
    parser.add_argument('--runtime-output', type=Path, default=REPO / 'wire-out/native-mono-game-classlib')
    parser.add_argument('--output', type=Path, default=REPO / 'wire-out/native-mono-unity-startup-probe')
    parser.add_argument('--wire', type=Path, default=REPO / 'target/release/wire')
    parser.add_argument('--skip-etw', action='store_true', help='explicit diagnostic platform divergence')
    parser.add_argument('--boundary', choices=('root', 'main-thread', 'pre-bindings'), default='root', help='finite diagnostic initialization boundary')
    args = parser.parse_args()
    output = args.output.resolve()
    if not output.is_relative_to(REPO / 'wire-out'):
        parser.error('output must be under the worktree ignored wire-out directory')
    output.mkdir(parents=True, exist_ok=True)
    (output / 'results.json').unlink(missing_ok=True)
    game = args.game.resolve()
    runtime = args.runtime_output.resolve()
    library = runtime / 'build-runtime/mono/mini/.libs/libmonoboehm-2.0.1.dylib'
    guard = runtime / 'probe/noexec_guard.dylib'
    support = runtime / 'probe/classlib/mono/4.5/mscorlib.dll.dylib'
    clone = output / 'runtime'
    if clone.exists():
        parser.error('use a fresh output: existing game copies are never overwritten')
    clone.mkdir()
    # APFS data clones: original game and runtime artifacts remain read-only.
    subprocess.run(['cp', '-cR', str(game.parent) + '/.', str(clone)], check=True)
    managed = clone / 'AliceToPapaADV_1-0-0_Data/Managed'
    if sha256(managed / 'mscorlib.dll') != sha256(runtime / 'probe/classlib/mono/4.5/mscorlib.dll'):
        raise RuntimeError('game/corelib differs from development precompiled support input')
    shutil.copy2(support, managed / support.name)
    executable = output / 'wire'
    shutil.copy2(args.wire.resolve(), executable)
    paths = {'exe': game, 'UnityPlayer': game.parent / 'UnityPlayer.dll',
             'original_mono': game.parent / 'MonoBleedingEdge/EmbedRuntime/mono-2.0-bdwgc.dll',
             'corelib': managed / 'mscorlib.dll', 'native_mono': library,
             'precompiled_corelib_support': support, 'noexec_guard': guard, 'wire': executable}
    inputs = {name: {'path': str(path), 'sha256': sha256(path)} for name, path in paths.items()}
    (output / 'inputs.json').write_text(json.dumps(inputs, indent=2) + '\n')
    env = os.environ.copy()
    for key in tuple(env):
        if key.startswith('MONO_') or key in ('DYLD_INSERT_LIBRARIES', 'DYLD_LIBRARY_PATH'):
            env.pop(key)
    env.update(DYLD_INSERT_LIBRARIES=str(guard), MONO_THREADS_SUSPEND='preemptive')
    command = [str(executable), 'probe64', str(clone / game.name), '--startup',
               '--max-instr', '10000000', '--native-mono-startup-probe', str(library),
               '--api-trace', str(output / 'api.tsv')]
    if args.skip_etw:
        command.append('--native-mono-probe-skip-etw')
    command.extend(['--native-mono-probe-boundary', args.boundary])
    (output / 'command.json').write_text(json.dumps({'command': command, 'cwd': str(clone),
        'environment': {key: env[key] for key in ('DYLD_INSERT_LIBRARIES', 'MONO_THREADS_SUSPEND')},
        'full_game_backend': False, 'timing_is_performance_comparison': False}, indent=2) + '\n')
    start = time.monotonic()
    with (output / 'startup.log').open('w') as stream:
        result = subprocess.run(command, cwd=clone, env=env, stdout=stream, stderr=subprocess.STDOUT, timeout=180)
    content = (output / 'startup.log').read_text()
    original_attach = bool(re.search(r'^(?:entry stage:|TLS process attach:).*mono-2\.0-bdwgc\.dll', content, re.M))
    guard_result = re.search(r'WIRE_NOEXEC_GUARD checks=(\d+) denied=(\d+)', content)
    root_ready = 'native-root-ready=true' in content
    evidence = {'returncode': result.returncode, 'elapsed_seconds': time.monotonic() - start,
        'native_root_ready': root_ready, 'native_main_thread_ready': 'native-main-thread-ready=true' in content,
        'native_pre_bindings_ready': 'native-pre-bindings-ready=true' in content, 'boundary': args.boundary,
        'full_game_backend': False, 'original_mono_attach_observed': original_attach,
        'etw_explicitly_disabled': args.skip_etw, 'native_guard':
        {'checks': int(guard_result[1]), 'denied': int(guard_result[2])} if guard_result else None,
        'timing_is_performance_comparison': False}
    (output / 'results.json').write_text(json.dumps(evidence, indent=2) + '\n')
    print(json.dumps(evidence, indent=2))
    if original_attach:
        raise RuntimeError('native startup provider executed original Mono attach')
    if root_ready and (not guard_result or int(guard_result[1]) == 0 or int(guard_result[2]) != 0):
        raise RuntimeError('native root run lacks successful noexec guard evidence')
    if result.returncode != 0 or not root_ready or (args.boundary != 'root' and not evidence['native_main_thread_ready']) or (args.boundary == 'pre-bindings' and not evidence['native_pre_bindings_ready']):
        raise SystemExit(1)


if __name__ == '__main__':
    main()
