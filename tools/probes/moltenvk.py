#!/usr/bin/env python3
"""Build and run the independent native GPU probe with explicit SDK paths.

Does not download, install, bundle, or search third-party application runtimes.
No DXGI/D3D feature level is claimed by a successful compute submission.
"""
import argparse
import json
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--include', required=True, type=Path,
                        help='official SDK directory containing vulkan/vulkan.h')
    parser.add_argument('--library', required=True, type=Path,
                        help='official public-API libMoltenVK.dylib')
    parser.add_argument('--out', required=True, type=Path)
    args = parser.parse_args()
    if sys.platform != 'darwin':
        parser.error('this probe runner currently targets macOS')
    include, library = args.include.resolve(), args.library.resolve()
    if not (include / 'vulkan/vulkan.h').is_file() or not library.is_file():
        parser.error('Vulkan headers or MoltenVK library are missing')
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / 'report.json').unlink(missing_ok=True)
    binary = (args.out / 'moltenvk-probe').resolve()
    source = (Path(__file__).resolve().parents[1] / 'moltenvk_probe.c')
    subprocess.run(['xcrun', 'clang', '-std=c11', '-O2', '-Wall', '-Wextra',
                    '-Werror', '-I', str(include), str(source), '-o', str(binary)], check=True)
    result = subprocess.run([str(binary), str(library)], capture_output=True,
                            text=True, timeout=60)
    (args.out / 'driver.log').write_text(result.stderr)
    if result.returncode:
        raise SystemExit(f'GPU probe failed ({result.returncode}):\n{result.stderr}')
    report = json.loads(result.stdout)
    # A failed profile is useful evidence, independent of successful GPU work.
    # This is a necessary-condition check, not a claim of full DXVK support.
    required = ['VK_EXT_depth_clip_enable', 'VK_KHR_maintenance5',
                'VK_KHR_maintenance6', 'VK_KHR_load_store_op_none',
                'VK_EXT_robustness2', 'VK_EXT_transform_feedback']
    missing = [name for name in required if name not in report['extensions']]
    for name in ['descriptorIndexing', 'scalarBlockLayout', 'shaderInt8', 'shaderInt16', 'shaderInt64']:
        if not report['features'][name]:
            missing.append(name)
    if report['max_push_constants'] < 256:
        missing.append('maxPushConstantsSize >= 256')
    if tuple(map(int, report['device_api'].split('.'))) < (1, 4, 0):
        missing.append('Vulkan >= 1.4')
    report['dxvk3_missing_necessary_conditions'] = missing
    report['dxvk_native_1_9_2_missing_geometry_shader'] = not report['features']['geometryShader']
    report['headers'] = str(include)
    report['library'] = str(library)
    (args.out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(f"GPU: {report['device']}; Vulkan {report['device_api']}")
    print(f"SPIR-V -> Metal compute: {report['compute_words_verified']} words verified")
    print('DXVK 3.x missing necessary conditions: ' + (', '.join(missing) or 'none checked'))
    print(f"Report: {(args.out / 'report.json').resolve()}")


if __name__ == '__main__':
    main()
