#!/usr/bin/env python3
"""Drive actual Unity into the exact-sample finite native Mono startup provider."""
import argparse
import csv
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
    parser.add_argument('--registration-reference', type=Path, default=REPO / 'wire-out/native-mono-unity-callback-trace/summary/registered-internal-calls.tsv', help='frozen original Unity registration trace')
    parser.add_argument('--wire', type=Path, default=REPO / 'target/release/wire')
    parser.add_argument('--graphics-backend', type=Path, help='existing native graphics backend')
    parser.add_argument('--moltenvk', type=Path, help='existing MoltenVK paired with graphics backend')
    parser.add_argument('--managed-trace', action='store_true', help='debug-only native managed call trace; timing is not a performance result')
    parser.add_argument('--max-instr', type=int, default=30000000, help='bounded guest work budget')
    parser.add_argument('--skip-etw', action='store_true', help='explicit diagnostic platform divergence')
    parser.add_argument('--boundary', choices=('root', 'main-thread', 'pre-bindings', 'bindings', 'profiler', 'pre-images', 'images', 'metadata', 'strings', 'managed-types', 'classlib'), default='root', help='finite diagnostic initialization boundary')
    args = parser.parse_args()
    if bool(args.graphics_backend) != bool(args.moltenvk):
        parser.error("--graphics-backend and --moltenvk must be paired")
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
    if args.graphics_backend:
        for name, path in [('graphics_backend', args.graphics_backend.resolve()), ('moltenvk', args.moltenvk.resolve())]:
            frozen = output / (name + '.dylib')
            shutil.copy2(path, frozen)
            paths[name] = frozen
    inputs = {name: {'path': str(path), 'sha256': sha256(path)} for name, path in paths.items()}
    (output / 'inputs.json').write_text(json.dumps(inputs, indent=2) + '\n')
    env = os.environ.copy()
    for key in tuple(env):
        if key.startswith('MONO_') or key in ('DYLD_INSERT_LIBRARIES', 'DYLD_LIBRARY_PATH'):
            env.pop(key)
    env.pop('WIRE_NATIVE_MONO_MANAGED_TRACE', None)
    if args.managed_trace:
        env['WIRE_NATIVE_MONO_MANAGED_TRACE'] = '1'
    env.update(DYLD_INSERT_LIBRARIES=str(guard), MONO_THREADS_SUSPEND='preemptive')
    if args.boundary in ('bindings', 'profiler', 'pre-images', 'images', 'metadata', 'strings', 'managed-types', 'classlib'):
        reference = args.registration_reference.resolve()
        shutil.copy2(reference, output / 'registration-reference.tsv')
        inputs['registration_reference'] = {'path': str(reference), 'sha256': sha256(reference)}
        (output / 'inputs.json').write_text(json.dumps(inputs, indent=2) + '\n')
    command = [str(executable), 'probe64', str(clone / game.name), '--startup',
               '--max-instr', str(args.max_instr), '--native-mono-startup-probe', str(library),
               '--api-trace', str(output / 'api.tsv')]
    if args.graphics_backend:
        command.extend(['--graphics-backend', str(paths['graphics_backend']), '--moltenvk', str(paths['moltenvk'])])
    if args.skip_etw:
        command.append('--native-mono-probe-skip-etw')
    command.extend(['--native-mono-probe-boundary', args.boundary])
    (output / 'command.json').write_text(json.dumps({'command': command, 'cwd': str(clone),
        'environment': {key: env[key] for key in ('DYLD_INSERT_LIBRARIES', 'MONO_THREADS_SUSPEND', 'WIRE_NATIVE_MONO_MANAGED_TRACE') if key in env},
        'full_game_backend': False, 'timing_is_performance_comparison': False}, indent=2) + '\n')
    start = time.monotonic()
    with (output / 'startup.log').open('w') as stream:
        result = subprocess.run(command, cwd=clone, env=env, stdout=stream, stderr=subprocess.STDOUT, timeout=180)
    content = (output / 'startup.log').read_text()
    original_attach = bool(re.search(r'^(?:entry stage:|TLS process attach:).*mono-2\.0-bdwgc\.dll', content, re.M))
    guard_result = re.search(r'WIRE_NOEXEC_GUARD checks=(\d+) denied=(\d+)', content)
    root_ready = 'native-root-ready=true' in content
    def count_marker(name):
        match = re.search(r'^' + re.escape(name) + r'=(\d+)$', content, re.M)
        return int(match[1]) if match else 0
    registry_match = None
    if args.boundary in ('bindings', 'profiler', 'pre-images', 'images', 'metadata', 'strings', 'managed-types', 'classlib') and (output / 'api.tsv.mono-bindings.tsv').exists():
        with (output / 'registration-reference.tsv').open() as stream:
            original = {bytes.fromhex(row['name_bytes_hex']): int(row['guest_target'], 16) for row in csv.DictReader(stream, delimiter='\t')}
        with (output / 'api.tsv.mono-bindings.tsv').open() as stream:
            rows = list(csv.DictReader(stream, delimiter='\t'))
        current = {row['name'].encode(): int(row['guest_target'], 16) for row in rows}
        registry_match = len(rows) == len(original) == 10824 and current == original and all(row['full_game_abi_admitted'] == '0' for row in rows)
    evidence = {'returncode': result.returncode, 'elapsed_seconds': time.monotonic() - start,
        'native_root_ready': root_ready, 'native_main_thread_ready': 'native-main-thread-ready=true' in content,
        'native_pre_bindings_ready': 'native-pre-bindings-ready=true' in content, 'native_bindings_ready': 'native-bindings-ready=true' in content, 'boundary': args.boundary, 'native_profiler_ready': 'native-profiler-ready=true' in content,
        'provider_bindings': count_marker('provider-bindings'), 'native_scalar_adapters': count_marker('native-scalar-adapters'),
        'actual_unity_scalar_callbacks_verified': count_marker('actual-unity-scalar-callbacks-verified'),
        'actual_gc_event_mask': count_marker('actual-gc-event-mask'), 'actual_shutdown_callbacks': count_marker('actual-shutdown-callbacks'),
        'original_registration_table_matches': registry_match,
        'native_pre_images_ready': 'native-pre-images-ready=true' in content,
        'actual_unitytls_leaf_verified': 'actual-unitytls-leaf-verified=true' in content,
        'native_images_ready': 'native-images-ready=true' in content,
        'native_images_opened': count_marker('native-images-opened'),
        'native_assemblies_loaded': count_marker('native-assemblies-loaded'),
        'guest_image_refs_closed': count_marker('guest-image-refs-closed'),
        'native_assembly_image_getters': count_marker('native-assembly-image-getters'),
        'native_image_registry_retired': 'native-image-registry-retired=true' in content,
        'native_metadata_ready': 'native-metadata-ready=true' in content,
        'native_metadata_queries': count_marker('native-metadata-queries'),
        'native_metadata_handles': count_marker('native-metadata-handles'),
        'native_metadata_retired': 'native-metadata-retired=true' in content,
        'native_classlib_ready': 'native-classlib-ready=true' in content,
        'native_classlib_invokes': count_marker('native-classlib-invokes'),
        'native_managed_types_ready': 'native-managed-types-ready=true' in content,
        'native_managed_type_invokes': count_marker('native-managed-type-invokes'),
        'native_reflection_types': count_marker('native-reflection-types'),
        'native_reflection_gc_verifications': count_marker('native-reflection-gc-verifications'),
        'native_reflection_roots_released': 'native-reflection-roots-released=true' in content,
        'native_reflection_views_retired': 'native-reflection-views-retired=true' in content,
        'native_strings_ready': 'native-strings-ready=true' in content,
        'native_strings': count_marker('native-strings'),
        'native_string_gc_verifications': count_marker('native-string-gc-verifications'),
        'native_string_roots_released': 'native-string-roots-released=true' in content,
        'native_string_views_retired': 'native-string-views-retired=true' in content,
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
    if args.boundary in ('bindings', 'profiler', 'pre-images', 'images', 'metadata', 'strings', 'managed-types', 'classlib') and (registry_match is not True or evidence['actual_unity_scalar_callbacks_verified'] != 2 or evidence['native_scalar_adapters'] != 2):
        raise RuntimeError('actual Unity bindings/callback verification incomplete or registry differs from original')
    if args.boundary in ('profiler', 'pre-images', 'images', 'metadata', 'strings', 'managed-types', 'classlib') and (not evidence['native_profiler_ready'] or evidence['actual_gc_event_mask'] & 4065 != 4065 or evidence['actual_shutdown_callbacks'] != 1):
        raise RuntimeError('actual native GC/shutdown evidence incomplete')
    if args.boundary in ('pre-images', 'images', 'metadata', 'strings', 'managed-types', 'classlib') and (not evidence['native_pre_images_ready'] or not evidence['actual_unitytls_leaf_verified']):
        raise RuntimeError('actual UnityTLS/managed-image boundary evidence incomplete')
    if args.boundary == 'images' and (not evidence['native_images_ready'] or not evidence['native_image_registry_retired'] or [evidence[key] for key in ('native_images_opened', 'native_assemblies_loaded', 'guest_image_refs_closed', 'native_assembly_image_getters')] != [82, 82, 82, 82]):
        raise RuntimeError('actual native image/assembly cycles incomplete')
    if args.boundary == 'metadata' and (not evidence['native_metadata_ready'] or not evidence['native_metadata_retired'] or evidence['native_metadata_queries'] == 0):
        raise RuntimeError('actual typed metadata evidence incomplete')
    if args.boundary == 'strings' and (not evidence['native_strings_ready'] or evidence['native_strings'] == 0 or evidence['native_string_gc_verifications'] < evidence['native_strings'] or not evidence['native_string_roots_released'] or not evidence['native_string_views_retired']):
        raise RuntimeError('actual owned string/GC evidence incomplete')
    if args.boundary == 'managed-types' and (not evidence['native_managed_types_ready'] or evidence['native_managed_type_invokes'] == 0 or evidence['native_reflection_gc_verifications'] < evidence['native_reflection_types'] or not evidence['native_reflection_roots_released'] or not evidence['native_reflection_views_retired']):
        raise RuntimeError('actual managed type invoke/reflection GC evidence incomplete')
    if args.boundary == 'classlib' and (not evidence['native_classlib_ready'] or evidence['native_classlib_invokes'] != 1):
        raise RuntimeError('actual native classlib initialization evidence incomplete')
    if result.returncode != 0 or not root_ready or (args.boundary != 'root' and not evidence['native_main_thread_ready']) or (args.boundary in ('pre-bindings','bindings','profiler','pre-images','images','metadata','strings','managed-types','classlib') and not evidence['native_pre_bindings_ready']) or (args.boundary in ('bindings','profiler','pre-images','images','metadata','strings','managed-types','classlib') and not evidence['native_bindings_ready']):
        raise SystemExit(1)


if __name__ == '__main__':
    main()
