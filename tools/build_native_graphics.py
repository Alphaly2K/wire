#!/usr/bin/env python3
"""Build wire's optional host shader/Vulkan library from explicit external SDKs.

No download, installation, game-directory mutation or application bundling.
Use the fixed dependency revisions documented in d3d11-dxvk-moltenvk-design.md.
"""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import subprocess


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--vulkan-include', required=True, type=Path)
    p.add_argument('--dxbc-source', required=True, type=Path)
    p.add_argument('--dxbc-library', required=True, type=Path)
    p.add_argument('--out', required=True, type=Path)
    args = p.parse_args()
    if platform.system() != 'Darwin' or platform.machine() != 'arm64':
        p.error('this build route currently targets macOS ARM64')
    root = Path(__file__).resolve().parent.parent
    include, source, library, out = [v.resolve() for v in
        (args.vulkan_include, args.dxbc_source, args.dxbc_library, args.out)]
    for path in [include / 'vulkan/vulkan.h', source / 'dxbc/dxbc_api.h', library]:
        if not path.is_file():
            p.error(f'missing dependency: {path}')
    out.parent.mkdir(parents=True, exist_ok=True)
    args = ['xcrun', 'clang++', '-std=c++17', '-O2', '-Wall', '-Wextra', '-Werror',
            '-Wno-missing-field-initializers', '-Wno-unused-private-field',
            '-fvisibility=hidden', '-I', str(include), '-I', str(source),
            '-dynamiclib', str(root / 'src/platform/native_graphics.cpp'), str(library),
            '-o', str(out)]
    subprocess.run(args, check=True)
    # These hashes identify the inputs supplied by the caller; they do not
    # certify a revision. Use unchanged upstream archives/checked checkouts.
    inputs = [root / 'src/platform/native_graphics.cpp', root / 'src/platform/native_graphics.h', library]
    manifest = dict(target='macOS ARM64', command=args,
                    inputs={str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in inputs},
                    output_sha256=hashlib.sha256(out.read_bytes()).hexdigest())
    out.with_suffix(out.suffix + '.build.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(out)


if __name__ == '__main__':
    main()
