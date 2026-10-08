#!/usr/bin/env python3
"""Validate owned Vulkan resource transfers, views, copies and queue timestamps."""
import argparse
import ctypes as c
import json
import struct
from pathlib import Path


class TextureDesc(c.Structure):
    _fields_ = [(n, c.c_uint32) for n in ('width', 'height', 'mips', 'layers', 'format', 'bind')]


class ViewDesc(c.Structure):
    _fields_ = [(n, c.c_uint32) for n in ('format', 'mip', 'mips', 'layer', 'layers', 'kind')]


class Copy(c.Structure):
    _fields_ = [(n, c.c_uint32) for n in ('dst_sub', 'src_sub', 'x', 'y', 'left', 'top', 'right', 'bottom')]


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--backend', required=True, type=Path)
    p.add_argument('--moltenvk', required=True, type=Path)
    p.add_argument('--out', required=True, type=Path)
    args = p.parse_args()
    lib = c.CDLL(str(args.backend.resolve()))
    error = c.create_string_buffer(4096)
    vp, sz, u32 = c.c_void_p, c.c_size_t, c.c_uint32
    def fn(name, ret, types):
        f = getattr(lib, name); f.restype = ret; f.argtypes = types; return f
    open_gpu = fn('wire_gpu_open', vp, [c.c_char_p, vp, vp, sz])
    close = fn('wire_gpu_close', None, [vp])
    texture = fn('wire_gpu_texture_create', vp, [vp, c.POINTER(TextureDesc), vp, sz])
    free_texture = fn('wire_gpu_texture_free', None, [vp])
    write = fn('wire_gpu_texture_write', c.c_int, [vp, u32, vp, sz, vp, sz])
    read = fn('wire_gpu_texture_read', c.c_int, [vp, u32, vp, sz, vp, sz])
    view = fn('wire_gpu_image_view_create', vp, [vp, c.POINTER(ViewDesc), vp, sz])
    free_view = fn('wire_gpu_image_view_free', None, [vp])
    clear = fn('wire_gpu_image_view_clear', c.c_int, [vp, c.POINTER(c.c_float), vp, sz])
    clear_depth = fn('wire_gpu_image_view_clear_depth',c.c_int,[vp,u32,c.c_float,u32,vp,sz])
    copy = fn('wire_gpu_texture_copy', c.c_int, [vp, vp, c.POINTER(Copy), vp, sz])
    timestamp = fn('wire_gpu_timestamp', c.c_int, [vp, c.POINTER(c.c_uint64), c.POINTER(c.c_uint64), vp, sz])
    support = fn('wire_gpu_format_support', u32, [vp, u32])
    owned, views = [], []
    gpu = open_gpu(str(args.moltenvk.resolve()).encode(), c.create_string_buffer(312), error, len(error))
    def ok(value):
        if not value: raise RuntimeError(error.value.decode(errors='replace'))
        return value
    def make(d):
        t = ok(texture(gpu, c.byref(d), error, len(error))); owned.append(t); return t
    def put(t, sub, data):
        ok(write(t, sub, c.create_string_buffer(data), len(data), error, len(error)))
    def get(t, sub, size):
        b = c.create_string_buffer(size); ok(read(t, sub, b, size, error, len(error))); return b.raw
    try:
        ok(gpu)
        src = make(TextureDesc(16, 8, 1, 1, 28, 8))
        data = bytes(v for y in range(8) for x in range(16) for v in (x*13, y*27, (x+y)*7, 255))
        put(src, 0, data); assert get(src, 0, len(data)) == data
        dst = make(TextureDesc(32, 16, 1, 1, 28, 0x28))
        zeros = bytes(32*16*4); put(dst, 0, zeros)
        region = Copy(0, 0, 3, 5, 2, 1, 14, 7)
        ok(copy(dst, src, c.byref(region), error, len(error)))
        expected = bytearray(zeros)
        for y in range(6): expected[((y+5)*32+3)*4:((y+5)*32+15)*4] = data[((y+1)*16+2)*4:((y+1)*16+14)*4]
        assert get(dst, 0, len(expected)) == expected
        # Rejected copies preserve the entire destination.
        invalid = Copy(0, 0, 30, 15, 0, 0, 16, 8)
        assert not copy(dst, src, c.byref(invalid), error, len(error))
        assert get(dst, 0, len(expected)) == expected
        rtv = ok(view(dst, c.byref(ViewDesc(28, 0, 1, 0, 1, 1)), error, len(error))); views.append(rtv)
        ok(clear(rtv, (c.c_float*4)(.25, .5, .75, 1), error, len(error)))
        cleared = get(dst, 0, len(expected)); assert cleared == bytes((64, 128, 191, 255))*(32*16)
        depth = make(TextureDesc(16,8,1,1,19,0x48))
        dsv = ok(view(depth,c.byref(ViewDesc(20,0,1,0,1,2)),error,len(error)));views.append(dsv)
        for fmt in (21,22):
            srv = ok(view(depth,c.byref(ViewDesc(fmt,0,1,0,1,0)),error,len(error)));views.append(srv)
        assert not view(depth,c.byref(ViewDesc(28,0,1,0,1,2)),error,len(error))
        for flags, z, stencil, want_z, want_stencil in ((3,.375,91,.375,91),(1,.625,123,.625,91),(2,.9,7,.625,7)):
            ok(clear_depth(dsv,flags,z,stencil,error,len(error)))
            content=get(depth,0,16*8*8)
            assert content==(struct.pack('<fB',want_z,want_stencil)+bytes(3))*(16*8)
        assert not clear_depth(dsv,4,.25,0,error,len(error))
        assert not clear_depth(dsv,1,float('nan'),0,error,len(error))
        assert get(depth,0,16*8*8)==content
        # Integer/SNORM vertex formats also accepted as textures must use the
        # same byte sizes as guest upload/readback. Skip actual GPU rejections.
        format_checks = []
        for fmt, pixel_bytes in ((7,12),(8,12),(12,8),(13,8),(14,8),(17,8),(18,8),
                                 (25,4),(36,4),(37,4),(38,4),(50,2),(51,2),(52,2),(58,2),(59,2)):
            if not support(gpu,fmt)&0x20:
                continue
            t=make(TextureDesc(7,5,1,1,fmt,0))
            content=bytes((i*37+fmt)&255 for i in range(7*5*pixel_bytes))
            put(t,0,content);assert get(t,0,len(content))==content
            format_checks.append(fmt)
        cube = make(TextureDesc(4, 4, 3, 6, 29, 0x108))
        cube_checks = 0
        for layer in range(6):
            for mip in range(3):
                n = (4 >> mip)**2*4; content = bytes((layer*13+mip*31,))*n
                put(cube, layer*3+mip, content); assert get(cube, layer*3+mip, n) == content; cube_checks += n
        srv = ok(view(cube, c.byref(ViewDesc(29, 0, 3, 0, 6, 3)), error, len(error))); views.append(srv)
        volume = make(TextureDesc(4, 2, 3, 8, 28, 0x208))
        volume_checks = 0
        for mip in range(3):
            n = max(1,4 >> mip)*max(1,2 >> mip)*max(1,8 >> mip)*4
            content = bytes((i*29+mip*17)&255 for i in range(n))
            put(volume,mip,content);assert get(volume,mip,n)==content;volume_checks += n
        volume_srv = ok(view(volume,c.byref(ViewDesc(28,0,3,0,1,5)),error,len(error)));views.append(volume_srv)
        a, b, freq = c.c_uint64(), c.c_uint64(), c.c_uint64()
        ok(timestamp(gpu, c.byref(a), c.byref(freq), error, len(error)))
        first_freq = freq.value
        ok(timestamp(gpu, c.byref(b), c.byref(freq), error, len(error)))
        assert freq.value == first_freq and freq.value > 0 and b.value >= a.value
        args.out.mkdir(parents=True, exist_ok=True)
        (args.out/'clear-rgba32x16.bin').write_bytes(cleared)
        (args.out/'report.json').write_text(json.dumps(dict(texture_bytes=len(data), copy_bytes=len(expected), cube_bytes=cube_checks,volume_bytes=volume_checks,
            clear_pixels=32*16, depth_stencil_pixels=16*8, depth_stencil_clear_checks=3,
            typed_texture_formats=format_checks,
            timestamp_frequency=freq.value, timestamps=[a.value, b.value], guest_frame=False), indent=2)+'\n')
        print(f'resource validation passed: {len(data)+len(expected)+cube_checks+volume_checks} bytes, {32*16} clear pixels, real queue timestamps')
    finally:
        for v in reversed(views): free_view(v)
        for t in reversed(owned): free_texture(t)
        if gpu: close(gpu)


if __name__ == '__main__': main()
