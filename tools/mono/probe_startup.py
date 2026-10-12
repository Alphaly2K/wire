#!/usr/bin/env python3
"""Drive actual Unity into the exact-sample finite native Mono startup provider."""
import argparse
from contextlib import ExitStack
import csv
import fcntl
import json
import os
from pathlib import Path
import re
import resource
import shutil
import subprocess
import tempfile
import time
from probe import REPO, sha256


def link_input(source, destination, cleanup):
    """Borrow an existing input without retaining a second copy after the run."""
    source = source.resolve(strict=True)
    destination.symlink_to(source, target_is_directory=source.is_dir())
    cleanup.callback(destination.unlink, missing_ok=True)
    return destination


def snapshot_player_logs(sandbox, output):
    users = sandbox / 'Users'
    if users.is_dir():
        for name in ('Player.log', 'Player-prev.log'):
            for log in users.rglob(name):
                destination = output / 'guest-logs' / log.relative_to(users)
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(log, destination)


def reset_player_logs(sandbox):
    # A failure before Unity startup must not inherit the preceding run's log.
    users = sandbox / 'Users'
    if users.is_dir():
        for name in ('Player.log', 'Player-prev.log'):
            for log in users.rglob(name):
                log.unlink()


def main():
    # Input links and the small frozen wire executable retire even on timeout,
    # failed evidence checks or interruption. The game directory is reused.
    with ExitStack() as cleanup:
        run(cleanup)


def run(cleanup):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--game', type=Path, required=True, help='read-only original EXE')
    parser.add_argument('--runtime-output', type=Path, default=REPO / 'wire-out/native-mono-m1')
    parser.add_argument('--output', type=Path, default=REPO / 'wire-out/native-mono-unity-startup-probe')
    parser.add_argument('--registration-reference', type=Path, default=REPO / 'wire-out/native-mono-probe-state/registration-reference.tsv', help='frozen original Unity registration trace')
    parser.add_argument('--wire', type=Path, default=REPO / 'target/release/wire')
    parser.add_argument('--sandbox', type=Path, default=REPO / 'wire-out/native-mono-probe-state/runtime', help='reuse one existing isolated game directory; never copy the game per probe')
    parser.add_argument('--graphics-backend', type=Path, help='existing native graphics backend')
    parser.add_argument('--moltenvk', type=Path, help='existing MoltenVK paired with graphics backend')
    parser.add_argument('--window', action='store_true', help='show actual guest windows through the native presenter')
    parser.add_argument('--capture-frame', action='store_true', help='capture an actual Present to frame.ppm; enables the native presenter')
    parser.add_argument('--managed-trace', action='store_true', help='debug-only native managed call trace; timing is not a performance result')
    parser.add_argument('--bridge-audit', action='store_true', help='diagnostic metadata/icall lookup inventory; no game methods are executed by the audit')
    parser.add_argument('--max-instr', type=int, default=30000000, help='bounded guest work budget')
    parser.add_argument('--skip-etw', action='store_true', help='explicit diagnostic platform divergence')
    parser.add_argument('--boundary', choices=('root', 'main-thread', 'pre-bindings', 'bindings', 'profiler', 'pre-images', 'images', 'metadata', 'strings', 'managed-types', 'classlib', 'logging', 'exceptions', 'synchronization', 'subsystems', 'subsystem-map', 'imgui-cache', 'attributes', 'attribute-arrays', 'assembly-lookup', 'objects', 'curves', 'fields', 'components', 'assets'), default='root', help='finite diagnostic initialization boundary')
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
    sandbox = args.sandbox.resolve()
    if not sandbox.is_relative_to(REPO / 'wire-out') or sandbox == game.parent or not sandbox.is_dir():
        parser.error('--sandbox must be an existing isolated game directory under wire-out')
    for path in (game, library, guard, support, args.wire):
        if not path.is_file():
            parser.error(f'required input is missing: {path}')
    lock = cleanup.enter_context((sandbox.parent / 'startup.lock').open('a'))
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        parser.error('another startup probe is using this game sandbox')
    if sha256(sandbox / game.name) != sha256(game) or sha256(sandbox / 'UnityPlayer.dll') != sha256(game.parent / 'UnityPlayer.dll'):
        parser.error('reused sandbox does not match the requested original game')
    clone = link_input(sandbox, output / 'runtime', cleanup)
    managed = sandbox / 'AliceToPapaADV_1-0-0_Data/Managed'
    if sha256(managed / 'mscorlib.dll') != sha256(runtime / 'probe/classlib/mono/4.5/mscorlib.dll'):
        raise RuntimeError('game/corelib differs from development precompiled support input')
    support_destination = managed / support.name
    if support_destination.resolve() == support.resolve():
        parser.error('precompiled support input must be outside the writable game sandbox')
    support_destination.unlink(missing_ok=True)
    link_input(support, support_destination, cleanup)
    # Freeze only this small executable; a Cargo rebuild may replace its input.
    # The snapshot is ephemeral, and each probe retains its exact input digest.
    snapshot = Path(cleanup.enter_context(tempfile.TemporaryDirectory(prefix='wire-snapshot-', dir=sandbox.parent))) / 'wire'
    shutil.copy2(args.wire.resolve(), snapshot)
    executable = link_input(snapshot, output / 'wire', cleanup)
    paths = {'exe': game, 'UnityPlayer': game.parent / 'UnityPlayer.dll',
             'original_mono': game.parent / 'MonoBleedingEdge/EmbedRuntime/mono-2.0-bdwgc.dll',
             'corelib': managed / 'mscorlib.dll', 'native_mono': library,
             'precompiled_corelib_support': support, 'noexec_guard': guard, 'wire': executable}
    if args.graphics_backend:
        for name, path in [('graphics_backend', args.graphics_backend.resolve()), ('moltenvk', args.moltenvk.resolve())]:
            frozen = output / (name + '.dylib')
            link_input(path, frozen, cleanup)
            paths[name] = frozen
    inputs = {name: {'path': str(path), 'sha256': sha256(path)} for name, path in paths.items()}
    (output / 'inputs.json').write_text(json.dumps(inputs, indent=2) + '\n')
    env = os.environ.copy()
    for key in tuple(env):
        if key.startswith('MONO_') or key in ('DYLD_INSERT_LIBRARIES', 'DYLD_LIBRARY_PATH'):
            env.pop(key)
    env.pop('WIRE_NATIVE_MONO_MANAGED_TRACE', None)
    env.pop('WIRE_NATIVE_MONO_BRIDGE_AUDIT', None)
    if args.bridge_audit:
        env['WIRE_NATIVE_MONO_BRIDGE_AUDIT'] = str(output / 'bridge-audit')
    if args.managed_trace:
        env['WIRE_NATIVE_MONO_MANAGED_TRACE'] = '1'
    env.update(DYLD_INSERT_LIBRARIES=str(guard), MONO_THREADS_SUSPEND='preemptive')
    if args.boundary in ('bindings', 'profiler', 'pre-images', 'images', 'metadata', 'strings', 'managed-types', 'classlib', 'logging', 'exceptions', 'synchronization', 'subsystems', 'subsystem-map', 'imgui-cache', 'attributes', 'attribute-arrays', 'assembly-lookup', 'objects', 'curves', 'fields', 'components', 'assets'):
        reference = args.registration_reference.resolve()
        link_input(reference, output / 'registration-reference.tsv', cleanup)
        inputs['registration_reference'] = {'path': str(reference), 'sha256': sha256(reference)}
        (output / 'inputs.json').write_text(json.dumps(inputs, indent=2) + '\n')
    command = [str(executable), 'probe64', str(clone / game.name), '--startup',
               '--max-instr', str(args.max_instr), '--native-mono-startup-probe', str(library),
               '--api-trace', str(output / 'api.tsv')]
    if args.graphics_backend:
        command.extend(['--graphics-backend', str(paths['graphics_backend']), '--moltenvk', str(paths['moltenvk'])])
    if args.window:
        command.append('--window')
    if args.capture_frame:
        command.extend(['--capture-frame', str(output / 'frame.ppm')])
    if args.skip_etw:
        command.append('--native-mono-probe-skip-etw')
    command.extend(['--native-mono-probe-boundary', args.boundary])
    (output / 'command.json').write_text(json.dumps({'command': command, 'cwd': str(clone),
        'game_sandbox': str(sandbox),
        'storage_policy': 'one reused sandbox; ephemeral symlinks and wire snapshot; logs retained',
        'environment': {key: env[key] for key in ('DYLD_INSERT_LIBRARIES', 'MONO_THREADS_SUSPEND', 'WIRE_NATIVE_MONO_MANAGED_TRACE', 'WIRE_NATIVE_MONO_BRIDGE_AUDIT') if key in env},
        'full_game_backend': False, 'timing_is_performance_comparison': False,
        'native_presenter_requested': args.window or args.capture_frame,
        'present_capture_requested': args.capture_frame}, indent=2) + '\n')
    reset_player_logs(sandbox)
    start = time.monotonic()
    cleanup.callback(snapshot_player_logs, sandbox, output)
    with (output / 'startup.log').open('w') as stream:
        usage_before = resource.getrusage(resource.RUSAGE_CHILDREN)
        result = subprocess.run(command, cwd=clone, env=env, stdout=stream, stderr=subprocess.STDOUT, timeout=180)
        usage_after = resource.getrusage(resource.RUSAGE_CHILDREN)
    content = (output / 'startup.log').read_text()
    original_attach = bool(re.search(r'^(?:entry stage:|TLS process attach:).*mono-2\.0-bdwgc\.dll', content, re.M))
    guard_result = re.search(r'WIRE_NOEXEC_GUARD checks=(\d+) denied=(\d+)', content)
    root_ready = 'native-root-ready=true' in content
    def count_marker(name):
        match = re.search(r'^' + re.escape(name) + r'=(\d+)$', content, re.M)
        return int(match[1]) if match else 0
    registry_match = None
    if args.boundary in ('bindings', 'profiler', 'pre-images', 'images', 'metadata', 'strings', 'managed-types', 'classlib', 'logging', 'exceptions', 'synchronization', 'subsystems', 'subsystem-map', 'imgui-cache', 'attributes', 'attribute-arrays', 'assembly-lookup', 'objects', 'curves', 'fields', 'components', 'assets') and (output / 'api.tsv.mono-bindings.tsv').exists():
        with (output / 'registration-reference.tsv').open() as stream:
            original = {bytes.fromhex(row['name_bytes_hex']): int(row['guest_target'], 16) for row in csv.DictReader(stream, delimiter='\t')}
        with (output / 'api.tsv.mono-bindings.tsv').open() as stream:
            rows = list(csv.DictReader(stream, delimiter='\t'))
        current = {row['name'].encode(): int(row['guest_target'], 16) for row in rows}
        registry_match = len(rows) == len(original) == 10824 and current == original and all(row['full_game_abi_admitted'] == '0' for row in rows)
    evidence = {'returncode': result.returncode, 'elapsed_seconds': time.monotonic() - start,
        'cpu_user_seconds': usage_after.ru_utime - usage_before.ru_utime,
        'cpu_system_seconds': usage_after.ru_stime - usage_before.ru_stime,
        'pending_boundary': (re.findall(r'^pending boundary ([^:]+):', content, re.M) or [None])[-1],
        'native_root_ready': root_ready,
        'native_gc_scope_disables': count_marker('native-gc-scope-disables'),
        'native_gc_scope_enables': count_marker('native-gc-scope-enables'),
        'native_gc_scope_unwound': count_marker('native-gc-scope-unwound'),
        'native_gc_scope_depth': count_marker('native-gc-scope-depth'),
        'native_main_thread_ready': 'native-main-thread-ready=true' in content,
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
        'native_external_strong_handles': count_marker('native-external-strong-handles'),
        'native_external_weak_handles': count_marker('native-external-weak-handles'),
        'native_external_handle_views_retired': 'native-external-handle-views-retired=true' in content,
        'native_external_handles_released': 'native-external-handles-released=true' in content,
        'native_empty_arrays': count_marker('native-empty-arrays'),
        'native_array_gc_verifications': count_marker('native-array-gc-verifications'),
        'native_array_roots_released': 'native-array-roots-released=true' in content,
        'native_array_views_retired': 'native-array-views-retired=true' in content,
        'native_input_asset_constructors': count_marker('native-input-asset-constructors'),
        'native_serialization_callback_invokes': count_marker('native-serialization-callback-invokes'),
        'native_static_managed_invokes': count_marker('native-static-managed-invokes'),
        'native_static_byref_output_writes': count_marker('native-static-byref-output-writes'),
        'native_instance_managed_invokes': count_marker('native-instance-managed-invokes'),
        'native_instance_byref_output_writes': count_marker('native-instance-byref-output-writes'),
        'native_virtual_method_resolutions': count_marker('native-virtual-method-resolutions'),
        'native_input_profiler_marker_calls': count_marker('native-input-profiler-marker-calls'),
        'native_input_action_constructors': count_marker('native-input-action-constructors'),
        'native_input_map_constructors': count_marker('native-input-map-constructors'),
        'native_reference_arrays': count_marker('native-reference-arrays'),
        'native_borrowed_typed_arrays': count_marker('native-borrowed-typed-arrays'),
        'native_typed_array_refreshes': count_marker('native-typed-array-refreshes'),
        'native_value_arrays': count_marker('native-value-arrays'),
        'native_value_array_gc_verifications': count_marker('native-value-array-gc-verifications'),
        'native_value_array_scalar_writes': count_marker('native-value-array-scalar-writes'),
        'native_value_unbox_calls': count_marker('native-value-unbox-calls'),
        'native_value_payloads': count_marker('native-value-payloads'),
        'native_reference_array_writes': count_marker('native-reference-array-writes'),
        'native_reference_array_gc_verifications': count_marker('native-reference-array-gc-verifications'),
        'native_reference_array_views_retired': 'native-reference-array-views-retired=true' in content,
        'native_component_constructors': count_marker('native-component-constructors'),
        'native_instance_field_refreshes': count_marker('native-instance-field-refreshes'),
        'native_instance_field_cells': count_marker('native-instance-field-cells'),
        'native_instance_field_writes': count_marker('native-instance-field-writes'),
        'native_instance_reference_writes': count_marker('native-instance-reference-writes'),
        'native_instance_field_gc_verifications': count_marker('native-instance-field-gc-verifications'),
        'native_instance_field_views_retired': 'native-instance-field-views-retired=true' in content,
        'native_borrowed_object_roots': count_marker('native-borrowed-object-roots'),
        'native_borrowed_object_gc_verifications': count_marker('native-borrowed-object-gc-verifications'),
        'native_color_curves_ready': 'native-color-curves-ready=true' in content,
        'native_color_curves_graph_verifications': count_marker('native-color-curves-graph-verifications'),
        'native_color_curves_cleaned': 'native-color-curves-cleaned=true' in content,
        'native_ordinary_objects_ready': 'native-ordinary-objects-ready=true' in content,
        'native_boxed_value_objects': count_marker('native-boxed-value-objects'),
        'native_allocated_objects': count_marker('native-allocated-objects'),
        'native_allocated_object_gc_verifications': count_marker('native-allocated-object-gc-verifications'),
        'native_allocated_object_views_retired': 'native-allocated-object-views-retired=true' in content,
        'native_unity_cached_pointer_syncs': count_marker('native-unity-cached-pointer-syncs'),
        'native_allocated_object_roots_released': 'native-allocated-object-roots-released=true' in content,
        'native_assembly_lookup_ready': 'native-assembly-lookup-ready=true' in content,
        'native_assembly_name_parses': count_marker('native-assembly-name-parses'),
        'native_assembly_name_lookups': count_marker('native-assembly-name-lookups'),
        'native_assembly_name_hits': count_marker('native-assembly-name-hits'),
        'native_assembly_name_frees': count_marker('native-assembly-name-frees'),
        'native_assembly_names_retired': 'native-assembly-names-retired=true' in content,
        'native_attribute_arrays_ready': 'native-attribute-arrays-ready=true' in content,
        'native_attribute_arrays': count_marker('native-attribute-arrays'),
        'native_attribute_array_gc_verifications': count_marker('native-attribute-array-gc-verifications'),
        'native_attribute_array_views_retired': 'native-attribute-array-views-retired=true' in content,
        'native_error_completions': count_marker('native-error-completions'),
        'native_error_observations': count_marker('native-error-observations'),
        'native_error_snapshots_retired': 'native-error-snapshots-retired=true' in content,
        'native_attributes_ready': 'native-attributes-ready=true' in content,
        'native_attribute_objects': count_marker('native-attribute-objects'),
        'native_attribute_field_reads': count_marker('native-attribute-field-reads'),
        'native_attribute_gc_verifications': count_marker('native-attribute-gc-verifications'),
        'native_attribute_resources_released': 'native-attribute-resources-released=true' in content,
        'native_imgui_ready': 'native-imgui-ready=true' in content,
        'native_subsystem_map_ready': 'native-subsystem-map-ready=true' in content,
        'native_subsystems_ready': 'native-subsystems-ready=true' in content,
        'native_subsystem_analytics_calls': count_marker('native-subsystem-analytics-calls'),
        'native_resources_find_calls': count_marker('native-resources-find-calls'),
        'native_object_find_by_type_calls': count_marker('native-object-find-by-type-calls'),
        'native_scriptable_factory_calls': count_marker('native-scriptable-factory-calls'),
        'native_engine_scalar_calls': count_marker('native-engine-scalar-calls'),
        'native_component_state_calls': count_marker('native-component-state-calls'),
        'native_component_query_calls': count_marker('native-component-query-calls'),
        'native_ui_layout_calls': count_marker('native-ui-layout-calls'),
        'native_graphics_format_calls': count_marker('native-graphics-format-calls'),
        'native_engine_object_getter_calls': count_marker('native-engine-object-getter-calls'),
        'native_profiler_recorder_calls': count_marker('native-profiler-recorder-calls'),
        'native_profiler_recorders_created': count_marker('native-profiler-recorders-created'),
        'native_profiler_recorders_released': count_marker('native-profiler-recorders-released'),
        'native_profiler_recorders_live': count_marker('native-profiler-recorders-live'),
        'native_profiler_recorder_finalizers_queued': count_marker('native-profiler-recorder-finalizers-queued'),
        'native_profiler_recorder_finalizers_drained': count_marker('native-profiler-recorder-finalizers-drained'),
        'native_profiler_recorders_closed': 'native-profiler-recorders-closed=true' in content,
        'native_json_deserialize_calls': count_marker('native-json-deserialize-calls'),
        'native_profiler_marker_calls': count_marker('native-profiler-marker-calls'),
        'native_profiler_sample_begins': count_marker('native-profiler-sample-begins'),
        'native_profiler_sample_ends': count_marker('native-profiler-sample-ends'),
        'native_profiler_sample_depth': count_marker('native-profiler-sample-depth'),
        'native_unsafe_buffer_allocations': count_marker('native-unsafe-buffer-allocations'),
        'native_unsafe_buffer_frees': count_marker('native-unsafe-buffer-frees'),
        'native_unsafe_buffer_memsets': count_marker('native-unsafe-buffer-memsets'),
        'native_unsafe_buffer_copies': count_marker('native-unsafe-buffer-copies'),
        'native_unsafe_buffer_comparisons': count_marker('native-unsafe-buffer-comparisons'),
        'native_input_event_updates': len(re.findall(r'^NATIVE_MONO_INPUT_EVENTS:', content, re.M)),
        'native_unsafe_buffer_cleanup_frees': count_marker('native-unsafe-buffer-cleanup-frees'),
        'native_unsafe_buffer_live': count_marker('native-unsafe-buffer-live'),
        'native_unsafe_buffer_retained_bytes': count_marker('native-unsafe-buffer-retained-bytes'),
        'native_unsafe_buffer_closed': 'native-unsafe-buffer-closed=true' in content,
        'native_unsafe_buffer_finalizer_queued': count_marker('native-unsafe-buffer-finalizer-queued'),
        'native_unsafe_buffer_finalizer_drained': count_marker('native-unsafe-buffer-finalizer-drained'),
        'native_unsafe_buffer_finalizer_pending': count_marker('native-unsafe-buffer-finalizer-pending'),
        'native_shared_static_calls': count_marker('native-shared-static-calls'),
        'native_shared_static_reused': count_marker('native-shared-static-reused'),
        'native_shared_static_live': count_marker('native-shared-static-live'),
        'native_shared_static_bytes': count_marker('native-shared-static-bytes'),
        'native_shared_static_closed': 'native-shared-static-closed=true' in content,
        'native_job_reflection_calls': count_marker('native-job-reflection-calls'),
        'native_worker_tls_calls': count_marker('native-worker-tls-calls'),
        'native_worker_tls_threads': count_marker('native-worker-tls-threads'),
        'native_worker_tls_attaches': count_marker('native-worker-tls-attaches'),
        'native_worker_tls_detaches': count_marker('native-worker-tls-detaches'),
        'native_worker_tls_live': count_marker('native-worker-tls-live'),
        'native_worker_tls_gc_verifications': count_marker('native-worker-tls-gc-verifications'),
        'native_worker_tls_closed': 'native-worker-tls-closed=true' in content,
        'native_debug_log_calls': count_marker('native-debug-log-calls'),
        'native_synchronization_ready': 'native-synchronization-ready=true' in content,
        'native_exceptions_ready': 'native-exceptions-ready=true' in content,
        'native_logging_routes_verified': 4 if 'actual managed null/empty/Unicode/Console routes=4' in content else 0,
        'native_logging_ready': 'native-logging-ready=true' in content,
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
    graphics = re.search(r'^Win64 native GPU draws: (\d+); presents: (\d+)$', content, re.M)
    evidence['gpu_draws'] = int(graphics[1]) if graphics else 0
    evidence['gpu_presents'] = int(graphics[2]) if graphics else 0
    frame = output / 'frame.ppm'
    evidence['present_frame'] = {'path': str(frame), 'sha256': sha256(frame)} if frame.is_file() else None
    (output / 'results.json').write_text(json.dumps(evidence, indent=2) + '\n')
    print(json.dumps(evidence, indent=2))
    if original_attach:
        raise RuntimeError('native startup provider executed original Mono attach')
    if root_ready and (not guard_result or int(guard_result[1]) == 0 or int(guard_result[2]) != 0):
        raise RuntimeError('native root run lacks successful noexec guard evidence')
    if args.boundary in ('bindings', 'profiler', 'pre-images', 'images', 'metadata', 'strings', 'managed-types', 'classlib', 'logging', 'exceptions', 'synchronization', 'subsystems', 'subsystem-map', 'imgui-cache', 'attributes', 'attribute-arrays', 'assembly-lookup', 'objects', 'curves', 'fields', 'components', 'assets') and (registry_match is not True or evidence['actual_unity_scalar_callbacks_verified'] != 2 or evidence['native_scalar_adapters'] != 2):
        raise RuntimeError('actual Unity bindings/callback verification incomplete or registry differs from original')
    if args.boundary in ('profiler', 'pre-images', 'images', 'metadata', 'strings', 'managed-types', 'classlib', 'logging', 'exceptions', 'synchronization', 'subsystems', 'subsystem-map', 'imgui-cache', 'attributes', 'attribute-arrays', 'assembly-lookup', 'objects', 'curves', 'fields', 'components', 'assets') and (not evidence['native_profiler_ready'] or evidence['actual_gc_event_mask'] & 4065 != 4065 or evidence['actual_shutdown_callbacks'] != 1):
        raise RuntimeError('actual native GC/shutdown evidence incomplete')
    if args.boundary in ('pre-images', 'images', 'metadata', 'strings', 'managed-types', 'classlib', 'logging', 'exceptions', 'synchronization', 'subsystems', 'subsystem-map', 'imgui-cache', 'attributes', 'attribute-arrays', 'assembly-lookup', 'objects', 'curves', 'fields', 'components', 'assets') and (not evidence['native_pre_images_ready'] or not evidence['actual_unitytls_leaf_verified']):
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
    if args.boundary in ('logging', 'exceptions', 'synchronization', 'subsystems', 'subsystem-map', 'imgui-cache', 'attributes', 'attribute-arrays', 'assembly-lookup', 'objects', 'curves', 'fields', 'components', 'assets') and (not evidence['native_logging_ready'] or evidence['native_logging_routes_verified'] != 4):
        raise RuntimeError('native UnityLogWriter initialization evidence incomplete')
    if args.boundary in ('exceptions', 'synchronization', 'subsystems', 'subsystem-map', 'imgui-cache', 'attributes', 'attribute-arrays', 'assembly-lookup', 'objects', 'curves', 'fields', 'components', 'assets') and not evidence['native_exceptions_ready']:
        raise RuntimeError('actual Unity exception catcher registration incomplete')
    if args.boundary in ('synchronization', 'subsystems', 'subsystem-map', 'imgui-cache', 'attributes', 'attribute-arrays', 'assembly-lookup', 'objects', 'curves', 'fields', 'components', 'assets') and (evidence['native_empty_arrays'] == 0 or evidence['native_array_gc_verifications'] < evidence['native_empty_arrays'] or not evidence['native_array_roots_released'] or not evidence['native_array_views_retired'] or evidence['native_external_strong_handles'] == 0 or not evidence['native_external_handles_released'] or not evidence['native_external_handle_views_retired']):
        raise RuntimeError('actual startup array/strong handle lifecycle verification incomplete')
    if args.boundary in ('synchronization', 'subsystems', 'subsystem-map', 'imgui-cache', 'attributes', 'attribute-arrays', 'assembly-lookup', 'objects', 'curves', 'fields', 'components', 'assets') and not evidence['native_synchronization_ready']:
        raise RuntimeError(f"actual Unity synchronization context initialization incomplete; pending={evidence['pending_boundary']} steps={evidence['native_classlib_invokes']}")
    if args.boundary in ('subsystems', 'subsystem-map', 'imgui-cache', 'attributes', 'attribute-arrays', 'assembly-lookup', 'objects', 'curves', 'fields', 'components', 'assets') and not evidence['native_subsystems_ready']:
        raise RuntimeError('actual subsystem descriptor initialization incomplete')
    if args.boundary in ('subsystem-map', 'imgui-cache', 'attributes', 'attribute-arrays', 'assembly-lookup', 'objects', 'curves', 'fields', 'components', 'assets') and (not evidence['native_subsystem_map_ready'] or 'NATIVE_MONO_SUBSYSTEM_MAP_VERIFIED:' not in content):
        raise RuntimeError('actual subsystem scripting map callback incomplete')
    if args.boundary in ('imgui-cache', 'attributes', 'attribute-arrays', 'assembly-lookup', 'objects', 'curves', 'fields', 'components', 'assets') and (not evidence['native_imgui_ready'] or 'NATIVE_MONO_IMGUI_CACHE_VERIFIED:' not in content):
        raise RuntimeError('actual IMGUI cache initialization incomplete')
    if args.boundary in ('attributes', 'attribute-arrays', 'assembly-lookup', 'objects', 'curves', 'fields', 'components', 'assets') and (not evidence['native_attributes_ready'] or evidence['native_attribute_objects'] == 0 or evidence['native_attribute_field_reads'] == 0 or evidence['native_attribute_gc_verifications'] < evidence['native_attribute_objects'] or not evidence['native_attribute_resources_released']):
        raise RuntimeError('actual custom-attribute construction/field/GC lifecycle incomplete')
    if args.boundary in ('attribute-arrays', 'assembly-lookup', 'objects', 'curves', 'fields', 'components', 'assets') and (not evidence['native_attribute_arrays_ready'] or evidence['native_attribute_arrays'] == 0 or evidence['native_attribute_array_gc_verifications'] < evidence['native_attribute_arrays'] or not evidence['native_attribute_array_views_retired'] or evidence['native_error_completions'] == 0 or evidence['native_error_observations'] == 0 or not evidence['native_error_snapshots_retired']):
        raise RuntimeError('actual attribute array/error snapshot/GC lifecycle incomplete')
    if args.boundary in ('assembly-lookup', 'objects', 'curves', 'fields', 'components', 'assets') and (not evidence['native_assembly_lookup_ready'] or evidence['native_assembly_name_hits'] == 0 or evidence['native_assembly_name_frees'] == 0 or not evidence['native_assembly_names_retired']):
        raise RuntimeError('actual parsed assembly lookup lifecycle incomplete')
    if args.boundary in ('objects', 'curves', 'fields', 'components', 'assets') and (not evidence['native_ordinary_objects_ready'] or evidence['native_allocated_objects'] == 0 or evidence['native_allocated_object_gc_verifications'] < evidence['native_allocated_objects'] or not evidence['native_allocated_object_roots_released'] or not evidence['native_allocated_object_views_retired']):
        raise RuntimeError('actual checked object allocation/GC/root lifecycle incomplete')
    if args.boundary == 'curves' and ('native-color-curves-ready=true' not in content or count_marker('native-color-curves-graph-verifications') < 3 or 'native-color-curves-cleaned=true' not in content):
        raise RuntimeError('actual ColorCurves constructor/GC/owner cleanup incomplete')
    if args.boundary == 'fields' and (evidence['native_instance_field_cells'] != 73 or evidence['native_instance_field_gc_verifications'] < 73 or evidence['native_instance_field_writes'] == 0 or evidence['native_instance_reference_writes'] != 8 or not evidence['native_instance_field_views_retired'] or evidence['native_borrowed_object_roots'] != 24 or evidence['native_borrowed_object_gc_verifications'] < 24 or evidence['native_color_curves_graph_verifications'] < 3 or not evidence['native_color_curves_cleaned']):
        raise RuntimeError('actual initialized-field deserialization/reference barrier/GC lifecycle incomplete')
    if args.boundary == 'components' and (evidence['native_component_constructors'] == 0 or evidence['native_instance_field_gc_verifications'] < evidence['native_instance_field_cells'] or not evidence['native_instance_field_views_retired'] or evidence['native_borrowed_object_gc_verifications'] < evidence['native_borrowed_object_roots']):
        raise RuntimeError('actual component constructor/field/GC lifecycle incomplete')
    if evidence['native_profiler_sample_begins'] != evidence['native_profiler_sample_ends'] or evidence['native_profiler_sample_depth'] != 0:
        raise RuntimeError('native profiler samples did not close in owner order')
    if evidence['native_profiler_recorders_created'] and (
            evidence['native_profiler_recorders_created'] != evidence['native_profiler_recorders_released']
            or evidence['native_profiler_recorders_live'] != 0
            or evidence['native_profiler_recorder_finalizers_queued'] != evidence['native_profiler_recorder_finalizers_drained']
            or not evidence['native_profiler_recorders_closed']):
        raise RuntimeError('native profiler recorder references/finalizers did not retire after VM cleanup')
    if evidence['native_unsafe_buffer_allocations'] and (evidence['native_unsafe_buffer_frees'] != evidence['native_unsafe_buffer_allocations'] or evidence['native_unsafe_buffer_live'] != 0 or evidence['native_unsafe_buffer_retained_bytes'] != 0 or not evidence['native_unsafe_buffer_closed']):
        raise RuntimeError('native private buffers did not retire guest records and actual storage after VM cleanup')
    if evidence['native_gc_scope_depth'] != 0:
        raise SystemExit('native Unity GC scopes were not restored before diagnostic verification')
    if args.boundary == 'assets' and (evidence['native_input_asset_constructors'] != 1 or evidence['native_instance_field_gc_verifications'] < evidence['native_instance_field_cells'] or not evidence['native_instance_field_views_retired'] or evidence['native_borrowed_object_gc_verifications'] < evidence['native_borrowed_object_roots']):
        raise RuntimeError('actual InputActionAsset constructor/fields/GC lifecycle incomplete')
    if evidence['native_reference_arrays'] and (evidence['native_reference_array_gc_verifications'] < evidence['native_reference_arrays'] or not evidence['native_reference_array_views_retired']):
        raise RuntimeError('owned reference arrays did not pass GC verification and retirement')
    if evidence['native_value_arrays'] and (evidence['native_value_array_gc_verifications'] < evidence['native_value_arrays'] or not evidence['native_reference_array_views_retired']):
        raise RuntimeError('owned value arrays did not pass GC verification and retirement')
    expected_steps = {'classlib': 1, 'logging': 2, 'exceptions': 3, 'synchronization': 5, 'subsystems': 6, 'subsystem-map': 7, 'imgui-cache': 8, 'attributes': 8, 'attribute-arrays': 8, 'assembly-lookup': 8, 'objects': 8, 'curves': 8, 'fields': 8, 'components': 8, 'assets': 8}.get(args.boundary)
    if expected_steps is not None and evidence['native_classlib_invokes'] != expected_steps:
        raise RuntimeError('actual ordered managed startup steps incomplete')
    if result.returncode != 0 or not root_ready or (args.boundary != 'root' and not evidence['native_main_thread_ready']) or (args.boundary in ('pre-bindings','bindings','profiler','pre-images','images','metadata','strings','managed-types','classlib','logging','exceptions','synchronization', 'subsystems', 'subsystem-map', 'imgui-cache', 'attributes', 'attribute-arrays', 'assembly-lookup', 'objects', 'curves', 'fields', 'components', 'assets') and not evidence['native_pre_bindings_ready']) or (args.boundary in ('bindings','profiler','pre-images','images','metadata','strings','managed-types','classlib','logging','exceptions','synchronization', 'subsystems', 'subsystem-map', 'imgui-cache', 'attributes', 'attribute-arrays', 'assembly-lookup', 'objects', 'curves', 'fields', 'components', 'assets') and not evidence['native_bindings_ready']):
        raise SystemExit(1)


if __name__ == '__main__':
    main()
