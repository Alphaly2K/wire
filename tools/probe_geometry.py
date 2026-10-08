#!/usr/bin/env python3
"""Check GS compute expansion against independent emission/strip expectations.

Fixtures are original shaders produced by tests/native_graphics/geometry_fixtures.cpp,
not extracted game code. This is an offscreen backend test, not guest rendering.
"""
import argparse
import ctypes as c
import json
import struct
import subprocess
from pathlib import Path
from probe_native_graphics import GpuInfo, ShaderInfo


class Io(c.Structure):
    _fields_ = [(n, c.c_uint32) for n in
                ['location', 'component', 'built_in', 'scalar_type', 'array_size', 'word_offset',
                 'vector_size', 'array_dimensions']]


class Geometry(c.Structure):
    _fields_ = [('inputs', c.POINTER(Io)), ('outputs', c.POINTER(Io)),
                ('input_count', c.c_size_t), ('output_count', c.c_size_t)] + [
                    (n, c.c_uint32) for n in ['input_vertices', 'output_primitive', 'max_vertices',
                    'instances', 'input_words', 'vertex_words', 'invocation_words', 'reserved']]


class BufferBinding(c.Structure):
    _fields_ = [('set', c.c_uint32), ('binding', c.c_uint32), ('buffer', c.c_void_p)]


def bits(f):
    return struct.unpack('<I', struct.pack('<f', f))[0]


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ['backend', 'moltenvk', 'fixtures', 'out', 'validator']:
        p.add_argument('--' + name, required=True, type=Path)
    args = p.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / 'report.json').unlink(missing_ok=True)
    lib = c.CDLL(str(args.backend.resolve()))
    lib.wire_gpu_open.argtypes = [c.c_char_p, c.POINTER(GpuInfo), c.c_void_p, c.c_size_t]
    lib.wire_gpu_open.restype = c.c_void_p
    lib.wire_gpu_close.argtypes = [c.c_void_p]
    lib.wire_gpu_compile.argtypes = [c.c_void_p, c.c_void_p, c.c_size_t, c.c_uint32, c.c_void_p, c.c_size_t]
    lib.wire_gpu_compile.restype = c.c_void_p
    lib.wire_gpu_shader_free.argtypes = [c.c_void_p]
    lib.wire_gpu_shader_info.argtypes = [c.c_void_p, c.POINTER(ShaderInfo)]
    lib.wire_gpu_geometry_info.argtypes = [c.c_void_p, c.POINTER(Geometry)]
    lib.wire_gpu_geometry_vertex.argtypes = [c.c_void_p,c.c_void_p,c.c_void_p,c.c_size_t]
    lib.wire_gpu_geometry_vertex.restype = c.c_void_p
    lib.wire_gpu_geometry_dispatch.argtypes = [c.c_void_p, c.c_void_p, c.c_void_p, c.c_size_t,
        c.c_uint32, c.POINTER(BufferBinding), c.c_size_t, c.c_void_p, c.c_size_t, c.c_void_p, c.c_size_t]
    lib.wire_gpu_geometry_draw_rgba64.argtypes = [c.c_void_p]*4 + [c.c_size_t,c.c_void_p,
        c.c_size_t,c.c_void_p,c.c_size_t,c.c_void_p,c.c_size_t]
    lib.wire_gpu_buffer_create.argtypes = [c.c_void_p,c.c_void_p,c.c_size_t,c.c_uint32,c.c_void_p,c.c_size_t]
    lib.wire_gpu_buffer_create.restype = c.c_void_p
    lib.wire_gpu_buffer_free.argtypes = [c.c_void_p]
    lib.wire_gpu_buffer_write.argtypes = [c.c_void_p,c.c_size_t,c.c_void_p,c.c_size_t,c.c_void_p,c.c_size_t]
    lib.wire_gpu_buffer_read.argtypes = [c.c_void_p,c.c_size_t,c.c_void_p,c.c_size_t,c.c_void_p,c.c_size_t]
    error = c.create_string_buffer(4096)
    info = GpuInfo()
    context = lib.wire_gpu_open(str(args.moltenvk.resolve()).encode(), c.byref(info), error, len(error))
    if not context:
        raise AssertionError(error.value.decode())
    rows = []
    try:
        for path in sorted(args.fixtures.glob('*.dxbc')):
            if '.ps.' in path.name or '.vs.' in path.name:
                continue
            blob = path.read_bytes()
            handle = lib.wire_gpu_compile(context, c.create_string_buffer(blob), len(blob), 8, error, len(error))
            if not handle:
                raise AssertionError(f'{path.name}: {error.value.decode()}')
            buffer = None
            try:
                view, g = ShaderInfo(), Geometry()
                assert lib.wire_gpu_shader_info(handle, c.byref(view))
                assert lib.wire_gpu_geometry_info(handle, c.byref(g))
                spv = args.out / (path.stem + '.spv')
                spv.write_bytes(c.string_at(view.words, view.word_count * 4))
                subprocess.run([str(args.validator), '--target-env', 'vulkan1.3', str(spv)], check=True)
                assert g.input_words == g.input_vertices * 4
                assert g.vertex_words == 6
                inputs = list(g.inputs[:g.input_count])
                outputs = list(g.outputs[:g.output_count])
                row = dict(shader=path.name, spirv_valid=True, instances=g.instances,
                           input_vertices=g.input_vertices, output_primitive=g.output_primitive,
                           cases=[], compared_words=0)
                bindings = None
                if path.stem == 'constant-buffer':
                    data = (c.c_uint32 * 4)(42,17,3,99)
                    assert not lib.wire_gpu_buffer_create(context,data,15,1,error,len(error))
                    buffer = lib.wire_gpu_buffer_create(context,data,16,1,error,len(error))
                    assert buffer, error.value.decode()
                    patch = (c.c_uint32 * 1)(77)
                    assert not lib.wire_gpu_buffer_write(buffer,14,patch,4,error,len(error))
                    read = (c.c_uint32 * 4)()
                    assert lib.wire_gpu_buffer_read(buffer,0,read,16,error,len(error))
                    assert list(read) == [42,17,3,99]
                    assert lib.wire_gpu_buffer_write(buffer,0,patch,4,error,len(error))
                    bindings = (BufferBinding * 1)(BufferBinding(3,0,buffer))
                for primitives in [1, 2, 7, 127]:
                    # Different source words in every invocation and every vertex;
                    # float patterns include signed zero and non-integer values.
                    positions = [[bits((pid + 1) * 0.125), bits((v + 1) * -0.25),
                                  bits(-0.0 if v % 2 else 0.5), bits(1.0)]
                                 for pid in range(primitives) for v in range(g.input_vertices)]
                    src = (c.c_uint32 * (primitives * g.input_words))()
                    for pid in range(primitives):
                        for io in inputs:
                            assert io.built_in == 0
                            for v in range(g.input_vertices):
                                width = io.array_size // g.input_vertices
                                for j in range(width):
                                    src[pid*g.input_words+io.word_offset+v*width+j] = positions[pid*g.input_vertices+v][j]
                    size = primitives * g.instances * g.invocation_words
                    dst = (c.c_uint32 * (size + 8))(*([0x5a5a5a5a] * (size + 8)))
                    def dispatch(count=primitives, src_size=len(src), dst_size=size):
                        return lib.wire_gpu_geometry_dispatch(context, handle, src, src_size,
                            count, bindings, 1 if bindings is not None else 0, dst, dst_size, error, len(error))
                    if bindings is not None:
                        assert not lib.wire_gpu_geometry_dispatch(context,handle,src,len(src),primitives,None,0,dst,size,error,len(error))
                        assert list(dst) == [0x5a5a5a5a]*len(dst)
                    assert not dispatch(dst_size=size-1), 'short output accepted'
                    assert list(dst) == [0x5a5a5a5a] * len(dst), 'failed call overwrote output'
                    assert not dispatch(src_size=len(src)-1), 'short input accepted'
                    assert list(dst) == [0x5a5a5a5a] * len(dst), 'failed call overwrote output'
                    ok = dispatch()
                    if path.stem == 'overflow':
                        assert not ok and b'more than declared' in error.value
                        assert list(dst) == [0x5a5a5a5a] * len(dst)
                        row['cases'].append(dict(primitives=primitives, overflow_rejected=True))
                        continue
                    assert ok, error.value.decode()
                    for pid in range(primitives):
                        for instance in range(g.instances):
                            base = (pid*g.instances+instance)*g.invocation_words
                            count = 0 if path.stem == 'conditional' and pid % 2 == 0 else g.input_vertices
                            assert dst[base] == count, (path.name, pid, dst[base], count)
                            for v in range(count):
                                start = base+1+v*(g.vertex_words+1)
                                expected_cut = int(v == 0 or ('cut' in path.stem and v == 2))
                                assert dst[start] == expected_cut
                                expected = [None]*g.vertex_words
                                for io in outputs:
                                    if io.built_in == 0:
                                        assert io.array_size == 4
                                        values = [0]*4 if path.stem == 'out-of-range' else positions[pid*g.input_vertices+v]
                                    else:
                                        assert io.location == 1 and io.component == 0 and io.array_size == 2
                                        values = [pid, 77 if buffer is not None else instance]
                                    expected[io.word_offset:io.word_offset+io.array_size] = values
                                assert list(dst[start+1:start+1+g.vertex_words]) == expected, (path.name, pid, v)
                                row['compared_words'] += g.vertex_words+1
                            # Unemitted/sink records stay zero. Host output guard stays intact.
                            assert all(x == 0 for x in dst[base+1+count*(g.vertex_words+1):base+g.invocation_words])
                    assert list(dst[size:]) == [0x5a5a5a5a]*8
                    row['cases'].append(dict(primitives=primitives, matched=True))
                if path.stem == 'triangle':
                    ps_blob = (args.fixtures / 'color.ps.dxbc').read_bytes()
                    ps = lib.wire_gpu_compile(context, c.create_string_buffer(ps_blob), len(ps_blob), 2, error, len(error))
                    assert ps, error.value.decode()
                    try:
                        forwarding = lib.wire_gpu_geometry_vertex(context,handle,error,len(error))
                        assert forwarding, error.value.decode()
                        try:
                            vs_view = ShaderInfo(); assert lib.wire_gpu_shader_info(forwarding,c.byref(vs_view))
                            vs_spv = args.out / 'forwarding.vs.spv'
                            vs_spv.write_bytes(c.string_at(vs_view.words,vs_view.word_count*4))
                            subprocess.run([str(args.validator), '--target-env', 'vulkan1.3', str(vs_spv)], check=True)
                        finally:
                            lib.wire_gpu_shader_free(forwarding)
                        ps_view = ShaderInfo(); assert lib.wire_gpu_shader_info(ps, c.byref(ps_view))
                        ps_spv = args.out / 'color.ps.spv'
                        ps_spv.write_bytes(c.string_at(ps_view.words, ps_view.word_count*4))
                        subprocess.run([str(args.validator), '--target-env', 'vulkan1.3', str(ps_spv)], check=True)
                        screen = [-1.,-1.,0.,1., 3.,-1.,0.,1., -1.,3.,0.,1.]
                        src = (c.c_uint32 * 12)(*(bits(f) for f in screen))
                        dst = (c.c_uint32 * g.invocation_words)()
                        assert lib.wire_gpu_geometry_dispatch(context,handle,src,len(src),1,None,0,dst,len(dst),error,len(error)), error.value.decode()
                        payload = [dst[1+v*7+1+j] for v in range(3) for j in range(6)]
                        vertex = (c.c_uint32 * len(payload))(*payload)
                        indices = (c.c_uint32 * 3)(0,1,2)
                        rgba = c.create_string_buffer(b'\x5a'*(64*64*4))
                        def draw(size=len(rgba)-1):
                            return lib.wire_gpu_geometry_draw_rgba64(context,handle,ps,vertex,len(vertex),indices,
                                len(indices),rgba,size,error,len(error))
                        assert not draw(size=16383)
                        assert rgba.raw[:-1] == b'\x5a'*16384
                        indices[2] = 3
                        assert not draw() and b'outside vertex buffer' in error.value
                        assert rgba.raw[:-1] == b'\x5a'*16384
                        indices[2] = 2
                        assert draw(), error.value.decode()
                        expected = bytes([64,128,191,255])*4096
                        assert rgba.raw[:-1] == expected
                        (args.out / 'geometry-rgba64.bin').write_bytes(rgba.raw[:-1])
                        row['draw_pixels_matched'] = 4096
                    finally:
                        lib.wire_gpu_shader_free(ps)
                rows.append(row)
            finally:
                if buffer:
                    lib.wire_gpu_buffer_free(buffer)
                lib.wire_gpu_shader_free(handle)
    finally:
        lib.wire_gpu_close(context)
    assert len(rows) == 11, 'missing fixtures'
    report = dict(adapter=info.name.decode(), guest_connected=False, shaders=rows,
                  compared_words=sum(r['compared_words'] for r in rows))
    (args.out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(dict(shaders=len(rows), compared_words=report['compared_words'], matched=True)))


if __name__ == '__main__':
    main()
