#!/usr/bin/env python3
"""Exercise the host-only C ABI with actual Unity shaders and Vulkan pipelines."""
import argparse
import ctypes as c
import hashlib
import json
import struct
import subprocess
from pathlib import Path
from probe_dxbc import containers


class GpuInfo(c.Structure):
    _fields_ = [('name', c.c_char * 256), ('vendor', c.c_uint32), ('device', c.c_uint32),
                ('api', c.c_uint32), ('reserved', c.c_uint32),
                ('video', c.c_uint64), ('system', c.c_uint64), ('shared', c.c_uint64),
                ('uuid', c.c_uint8 * 16)]


class Binding(c.Structure):
    _fields_ = [(name, c.c_uint32) for name in ['set', 'binding', 'type', 'count']]


class ShaderInfo(c.Structure):
    _fields_ = [('words', c.POINTER(c.c_uint32)), ('word_count', c.c_size_t),
                ('bindings', c.POINTER(Binding)), ('binding_count', c.c_size_t),
                ('stage', c.c_uint32), ('push_bytes', c.c_uint32)]


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ['backend', 'moltenvk', 'input', 'out']:
        p.add_argument('--' + name, required=True, type=Path)
    p.add_argument('--validator', type=Path, help='optional spirv-val executable')
    args = p.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / 'report.json').unlink(missing_ok=True)
    lib = c.CDLL(str(args.backend.resolve()))
    lib.wire_gpu_abi.restype = c.c_uint32
    if lib.wire_gpu_abi() != 1:
        raise SystemExit('native graphics ABI mismatch')
    lib.wire_gpu_open.argtypes = [c.c_char_p, c.POINTER(GpuInfo), c.c_void_p, c.c_size_t]
    lib.wire_gpu_open.restype = c.c_void_p
    lib.wire_gpu_close.argtypes = [c.c_void_p]
    lib.wire_gpu_compile.argtypes = [c.c_void_p, c.c_void_p, c.c_size_t, c.c_uint32, c.c_void_p, c.c_size_t]
    lib.wire_gpu_compile.restype = c.c_void_p
    lib.wire_gpu_shader_info.argtypes = [c.c_void_p, c.POINTER(ShaderInfo)]
    lib.wire_gpu_shader_info.restype = c.c_int
    lib.wire_gpu_shader_free.argtypes = [c.c_void_p]
    lib.wire_gpu_pipeline.argtypes = [c.c_void_p] * 5 + [c.c_size_t]
    lib.wire_gpu_pipeline.restype = c.c_int
    lib.wire_gpu_blit_rgba64.argtypes = [c.c_void_p] * 4 + [c.c_size_t, c.c_void_p, c.c_size_t, c.c_void_p, c.c_size_t]
    lib.wire_gpu_blit_rgba64.restype = c.c_int
    error = c.create_string_buffer(4096)
    info = GpuInfo()
    context = lib.wire_gpu_open(str(args.moltenvk.resolve()).encode(), c.byref(info), error, len(error))
    if not context:
        raise SystemExit(error.value.decode(errors='replace'))
    shaders, rows, pipelines, by_hash = [], [], [], {}
    rejected = []
    try:
        # C ABI must reject invalid data and retain a usable context afterwards.
        bad = c.create_string_buffer(b'DXBC' + b'\0' * 28)
        if lib.wire_gpu_compile(context, bad, 32, 1, error, len(error)):
            raise AssertionError('invalid DXBC accepted')
        input_bytes = args.input.read_bytes()
        samples = list(containers(input_bytes))
        if not samples:
            raise AssertionError('no DXBC fixtures found')
        payload = samples[0][1]
        def invalid(name, data, stage=2):
            blob = c.create_string_buffer(bytes(data))
            handle = lib.wire_gpu_compile(context, blob, len(data), stage, error, len(error))
            if handle:
                lib.wire_gpu_shader_free(handle)
                raise AssertionError(f'{name}: invalid DXBC accepted')
            rejected.append(dict(case=name, error=error.value.decode(errors='replace')))
        invalid('truncated_container', payload[:-1])
        invalid('unsupported_stage', payload, 8)
        bad = bytearray(payload)
        struct.pack_into('<I', bad, 32, len(bad) - 4)
        invalid('chunk_outside_container', bad)
        bad = bytearray(payload)
        struct.pack_into('<I', bad, 36, struct.unpack_from('<I', bad, 32)[0])
        invalid('overlapping_chunks', bad)
        # A failed device open must not overwrite adapter output metadata.
        unopened = GpuInfo()
        c.memset(c.byref(unopened), 0x5a, c.sizeof(unopened))
        if lib.wire_gpu_open(b'/nonexistent/wire-moltenvk.dylib', c.byref(unopened), error, len(error)):
            raise AssertionError('missing native library accepted')
        if bytes(unopened) != b'\x5a' * c.sizeof(unopened):
            raise AssertionError('failed open modified output')
        for offset, payload in containers(input_bytes):
            # SHDR/SHEX header contains the actual shader stage.
            count, = struct.unpack_from('<I', payload, 28)
            kind = None
            for i in range(count):
                chunk, = struct.unpack_from('<I', payload, 32 + 4 * i)
                if payload[chunk:chunk+4] in (b'SHDR', b'SHEX'):
                    token, = struct.unpack_from('<I', payload, chunk + 8)
                    kind = {0: 2, 1: 1, 5: 4}.get(token >> 16)
            if kind is None:
                raise AssertionError('unsupported sample shader stage')
            blob = c.create_string_buffer(payload)
            handle = lib.wire_gpu_compile(context, blob, len(payload), kind, error, len(error))
            row = dict(offset=offset, sha256=hashlib.sha256(payload).hexdigest(), stage=kind)
            if not handle:
                row['error'] = error.value.decode(errors='replace')
            else:
                shaders.append((kind, handle))
                by_hash[row['sha256']] = handle
                view = ShaderInfo()
                if not lib.wire_gpu_shader_info(handle, c.byref(view)):
                    raise AssertionError('shader metadata missing')
                row['spirv_words'] = view.word_count
                (args.out / (row['sha256'] + '.spv')).write_bytes(
                    c.string_at(view.words, view.word_count * 4))
                if args.validator:
                    result = subprocess.run([str(args.validator.resolve()), '--target-env', 'vulkan1.3',
                                             str(args.out / (row['sha256'] + '.spv'))],
                                            capture_output=True, text=True, timeout=30)
                    row['validation'] = result.returncode == 0
                    if result.returncode:
                        row['error'] = result.stdout + result.stderr
                row['bindings'] = [dict(set=b.set, binding=b.binding, type=b.type, count=b.count)
                                   for b in view.bindings[:view.binding_count]]
                row['push_bytes'] = view.push_bytes
            rows.append(row)
        vertices = [(i, h) for i, (k, h) in enumerate(shaders) if k == 1]
        pixels = [(i, h) for i, (k, h) in enumerate(shaders) if k == 2]
        # Pair each VS with each PS to exercise interface/layout compatibility.
        for vi, vertex in vertices:
            for pi, pixel in pixels:
                ok = lib.wire_gpu_pipeline(context, vertex, pixel, None, error, len(error))
                pipelines.append(dict(vertex=vi, pixel=pi, ok=bool(ok),
                                      error='' if ok else error.value.decode(errors='replace'),
                                      incompatible=not ok and error.value == b'shader I/O location mismatch'))
        for i, (kind, shader) in enumerate(shaders):
            if kind == 4:
                ok = lib.wire_gpu_pipeline(context, None, None, shader, error, len(error))
                pipelines.append(dict(compute=i, ok=bool(ok),
                                      error='' if ok else error.value.decode(errors='replace')))
        # The raw C ABI also refuses to mix two live GPU contexts.
        second_info = GpuInfo()
        second = lib.wire_gpu_open(str(args.moltenvk.resolve()).encode(), c.byref(second_info), error, len(error))
        if not second:
            raise AssertionError('second GPU context missing')
        try:
            compute = next(h for k, h in shaders if k == 4)
            if lib.wire_gpu_pipeline(second, None, None, compute, error, len(error)):
                raise AssertionError('cross-context shader accepted')
            if error.value != b'shader belongs to another GPU':
                raise AssertionError(error.value)
        finally:
            lib.wire_gpu_close(second)
        vertex = next(h for digest, h in by_hash.items() if digest.startswith('30e1f7ac'))
        pixel = next(h for digest, h in by_hash.items() if digest.startswith('aaaff3e4'))
        source = bytes(v for y in range(64) for x in range(64) for v in (x * 4, y * 4, (x ^ y) * 4, 255))
        source_buffer = c.create_string_buffer(source)
        output_buffer = c.create_string_buffer(b'\x55' * len(source), len(source))
        if lib.wire_gpu_blit_rgba64(context, vertex, pixel, source_buffer, len(source),
                                   output_buffer, len(source) - 1, error, len(error)):
            raise AssertionError('short readback buffer accepted')
        if output_buffer.raw != b'\x55' * len(source):
            raise AssertionError('failed draw modified readback output')
        if not lib.wire_gpu_blit_rgba64(context, vertex, pixel, source_buffer, len(source),
                                       output_buffer, len(source), error, len(error)):
            raise AssertionError(error.value.decode(errors='replace'))
        output = output_buffer.raw
        mismatches = sum(a != b for a, b in zip(source, output))
        if mismatches:
            raise AssertionError(f'GPU texture blit: {mismatches} channel mismatches')
        (args.out / 'offscreen-rgba64.bin').write_bytes(output)
        # Self-contained PNG writer: no extra imaging dependency for this probe.
        import zlib
        def png_chunk(tag, data):
            return struct.pack('>I', len(data)) + tag + data + struct.pack('>I', zlib.crc32(tag + data))
        png = b'\x89PNG\r\n\x1a\n' + png_chunk(b'IHDR', struct.pack('>IIBBBBB', 64, 64, 8, 6, 0, 0, 0))
        png += png_chunk(b'IDAT', zlib.compress(b''.join(b'\0' + output[y*256:(y+1)*256] for y in range(64))))
        (args.out / 'offscreen-rgba64.png').write_bytes(png + png_chunk(b'IEND', b''))
        report = dict(device=info.name.decode(), shaders=rows, pipelines=pipelines,
                      rejected_inputs=rejected, cross_context_rejected=True,
                      gpu_draw_submitted=True, guest_connected=False,
                      offscreen_pixels=4096, offscreen_channel_mismatches=mismatches,
                      input_sha256=hashlib.sha256(input_bytes).hexdigest())
        (args.out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
        print(f"Native shader library: {sum('spirv_words' in r for r in rows)}/{len(rows)} compiled")
        print(f"Actual Vulkan pipelines: {sum(r['ok'] for r in pipelines)} created; "
              f"{sum(r.get('incompatible', False) for r in pipelines)} incompatible pairs rejected")
        print(f"Actual Unity shader Draw + GPU readback: 4096 pixels, {mismatches} mismatched channels")
        for r in rows + pipelines:
            if r.get('error'):
                print(r)
        covered = set()
        for r in pipelines:
            if r['ok']:
                covered.update(r[k] for k in ['vertex', 'pixel', 'compute'] if k in r)
        if (any('error' in r for r in rows) or covered != set(range(len(shaders))) or
                not all(r['ok'] or r.get('incompatible', False) for r in pipelines)):
            raise SystemExit(1)
    finally:
        for _, shader in shaders:
            lib.wire_gpu_shader_free(shader)
        lib.wire_gpu_close(context)


if __name__ == '__main__':
    main()
