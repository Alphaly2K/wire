#!/usr/bin/env python3
"""Verify the Rust pointer-output proof against original no-JIT Mono fixtures.

Existing class libraries/support are read-only inputs. An optional CoreModule
is scanned through native metadata without invoking any engine/game method.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess

from probe import DEFAULT_SDK, FIXTURES, REPO, execute


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=REPO / 'wire-out/native-mono-m1')
    parser.add_argument('--fixtures-root', type=Path)
    parser.add_argument('--core', type=Path)
    parser.add_argument('--sdk', type=Path, default=DEFAULT_SDK)
    parser.add_argument('--incremental', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    fixtures = (args.fixtures_root or root / 'fixtures').resolve()
    if not fixtures.is_relative_to(REPO / 'wire-out') or fixtures.is_relative_to(root / 'probe'):
        parser.error('fixtures-root must be under wire-out and separate from game probe')
    output = fixtures / 'scalar-output-probe'
    output.mkdir(parents=True, exist_ok=True)
    sdk = args.sdk.resolve()
    env = {k: v for k, v in os.environ.items() if not k.startswith('MONO_')}
    env.pop('DYLD_INSERT_LIBRARIES', None)
    classlib = fixtures / 'classlib/mono/4.5'
    assembly = output / 'ScalarOutputsProbe.dll'
    records = [execute([sdk / 'bin/mono', sdk / 'lib/mono/4.5/mcs.exe',
                        '-unsafe', '-optimize-', '-target:library', '-nostdlib',
                        '-r:' + str(classlib / 'mscorlib.dll'), '-out:' + str(assembly),
                        FIXTURES / 'ScalarOutputsProbe.cs'], env, output, 'compile-il')]
    aot_env = dict(env, MONO_PATH=str(classlib))
    records.append(execute([root / 'build-aot-tool/mono/mini/mono-boehm', '--aot=interp', assembly],
                           aot_env, output, 'compile-support'))
    result = subprocess.run(['cargo', 'test', '--features', 'native-mono-backend', '--lib',
                             '--no-run', '--message-format=json'], cwd=REPO, env=env,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, check=True)
    (output / 'build.log').write_text(result.stderr)
    artifacts = [json.loads(line) for line in result.stdout.splitlines() if line.startswith('{')]
    tests = [item['executable'] for item in artifacts if item.get('reason') == 'compiler-artifact'
             and item.get('profile', {}).get('test') and item.get('executable')
             and item.get('target', {}).get('kind') == ['lib']]
    if len(tests) != 1:
        raise RuntimeError('expected one native backend library test binary')
    runtime = root / 'build-runtime/mono/mini/.libs'
    env.update(WIRE_MONO_OUTPUT_LIBRARY=str(runtime / 'libmonoboehm-2.0.1.dylib'),
               WIRE_MONO_OUTPUT_CLASSLIB=str(fixtures / 'classlib'),
               WIRE_MONO_OUTPUT_FIXTURE=str(assembly), MONO_PATH=str(classlib),
               DYLD_LIBRARY_PATH=str(runtime),
               DYLD_INSERT_LIBRARIES=str(fixtures / 'noexec_guard.dylib'),
               MONO_THREADS_SUSPEND='preemptive')
    if args.core:
        env['WIRE_MONO_OUTPUT_CORE'] = str(args.core.resolve())
    else:
        env.pop('WIRE_MONO_OUTPUT_CORE', None)
    if args.incremental:
        env['WIRE_MONO_UNITY_GC_PROFILE'] = '1'
    else:
        env.pop('WIRE_MONO_UNITY_GC_PROFILE', None)
    records.append(execute([tests[0], 'actual_native_metadata_and_il_use_private_output_cells',
                            '--ignored', '--test-threads=1', '--nocapture'],
                           env, output, 'native-incremental' if args.incremental else 'native'))
    (output / 'results.json').write_text(json.dumps(records, indent=2) + '\n')


if __name__ == '__main__':
    main()
