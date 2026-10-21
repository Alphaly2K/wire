#!/usr/bin/env python3
"""Build and run original macOS input callbacks on a private view."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', required=True, type=Path)
    args = parser.parse_args()
    if sys.platform != 'darwin':
        parser.error('this probe requires macOS/AppKit')
    root = Path(__file__).resolve().parents[1]
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    report = out / 'report.json'
    report.unlink(missing_ok=True)
    binary = out / 'input-probe'
    commands = [
        ['clang', '-fobjc-arc', '-mmacosx-version-min=11.0', '-Wall', '-Wextra',
         '-Werror', str(root / 'tools/probes/input.m'), '-framework', 'AppKit',
         '-framework', 'QuartzCore', '-o', str(binary)],
        [str(binary)],
    ]
    for name, command in zip(('build', 'run'), commands):
        result = subprocess.run(command, cwd=root, stdout=subprocess.PIPE,
                                stderr=subprocess.STDOUT, text=True, check=False)
        (out / f'{name}.txt').write_text(result.stdout)
        print(result.stdout, end='')
        if result.returncode:
            raise SystemExit(f'{name} failed: {result.returncode}')
    sources = ['src/platform/macos.m', 'tools/probes/input.m', 'tools/probe_input.py']
    report.write_text(json.dumps({
        'passed': True, 'commands': commands,
        'source_sha256': {name: hashlib.sha256((root / name).read_bytes()).hexdigest()
                          for name in sources},
        'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
    }, indent=2) + '\n')


if __name__ == '__main__':
    main()
