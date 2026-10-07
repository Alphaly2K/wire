#!/usr/bin/env python3
"""Compile embedded DXBC with an explicitly supplied native dxbc-spirv tool.

Reads the isolated game image only. Output is a shader-translation diagnostic,
not evidence of D3D device creation, GPU execution, or game rendering.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess


def containers(data):
    """Check container/chunk bounds before passing any candidate to upstream."""
    offset = 0
    while (offset := data.find(b'DXBC', offset)) >= 0:
        start = offset
        offset += 4
        if start + 32 > len(data):
            continue
        version, size, count = struct.unpack_from('<III', data, start + 20)
        if (version != 1 or not 0 < count <= 128 or
                not 32 + 4 * count <= size <= 16 * 1024 * 1024 or
                start + size > len(data)):
            continue
        chunks, spans = [], []
        for i in range(count):
            relative, = struct.unpack_from('<I', data, start + 32 + 4 * i)
            if relative < 32 + 4 * count or relative + 8 > size:
                break
            length, = struct.unpack_from('<I', data, start + relative + 4)
            end = relative + 8 + length
            if end > size or any(relative < b and end > a for a, b in spans):
                break
            spans.append((relative, end))
            tag = data[start + relative:start + relative + 4]
            if tag in (b'SHDR', b'SHEX') and length >= 8:
                chunks.append(tag.decode('ascii'))
        else:
            if len(chunks) == 1:
                yield start, data[start:start + size]


def spirv_info(data):
    if len(data) < 20 or len(data) % 4:
        raise ValueError('invalid SPIR-V size')
    words = struct.unpack('<' + 'I' * (len(data) // 4), data)
    if words[0] != 0x07230203 or words[4] != 0:
        raise ValueError('invalid SPIR-V header')
    offset, stages, capabilities = 5, [], []
    while offset < len(words):
        count, opcode = words[offset] >> 16, words[offset] & 0xffff
        if not count or offset + count > len(words):
            raise ValueError('invalid SPIR-V instruction bounds')
        if opcode == 15 and count >= 4:
            stages.append(words[offset + 1])
        if opcode == 17 and count == 2:
            capabilities.append(words[offset + 1])
        offset += count
    if not stages:
        raise ValueError('SPIR-V entry point missing')
    return dict(bytes=len(data), version=f'{(words[1] >> 16) & 255}.{(words[1] >> 8) & 255}',
                execution_models=stages, capabilities=capabilities)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--compiler', required=True, type=Path)
    parser.add_argument('--input', required=True, type=Path)
    parser.add_argument('--out', required=True, type=Path)
    args = parser.parse_args()
    compiler, source = args.compiler.resolve(), args.input.resolve()
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / 'report.json').unlink(missing_ok=True)
    data = source.read_bytes()
    rows, seen = [], set()
    for offset, container in containers(data):
        digest = hashlib.sha256(container).hexdigest()
        if digest in seen:
            continue
        seen.add(digest)
        dxbc, spv = [(args.out / (digest + suffix)).resolve() for suffix in ('.dxbc', '.spv')]
        dxbc.write_bytes(container)
        # Remove this probe's stale output, so a failed run cannot reuse it.
        spv.unlink(missing_ok=True)
        result = subprocess.run([str(compiler), str(dxbc), '--spv', str(spv), '--no-debug'],
                                text=True, capture_output=True, timeout=30)
        (args.out / (digest + '.log')).write_text(result.stdout + result.stderr)
        row = dict(offset=offset, dxbc_bytes=len(container), sha256=digest,
                   returncode=result.returncode)
        if result.returncode == 0:
            row['spirv'] = spirv_info(spv.read_bytes())
        rows.append(row)
    translated = sum('spirv' in r for r in rows)
    report = dict(input=str(source), input_sha256=hashlib.sha256(data).hexdigest(),
                  compiler=str(compiler), unique_containers=len(rows), translated=translated,
                  gpu_executed=False, guest_connected=False, shaders=rows)
    (args.out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(f'Native DXBC -> SPIR-V: {translated}/{len(rows)} embedded shaders translated')
    print(f'Report: {(args.out / "report.json").resolve()}')
    if not rows or translated != len(rows):
        raise SystemExit(1)


if __name__ == '__main__':
    main()
