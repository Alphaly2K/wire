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
        # Typeless keeps raw bytes, but every image view must select a type.
        # sRGB clears convert linear values; copies never apply that conversion.
        typeless_checks = 0
        for resource_format, typed_formats in ((27, (28,29,30,31,32)), (90, (87,91))):
            t = make(TextureDesc(8,4,3,2,resource_format,0x28))
            assert support(gpu,resource_format) == 0x320
            assert not view(t,c.byref(ViewDesc(resource_format,0,1,0,1,0)),error,len(error))
            assert not view(t,c.byref(ViewDesc(87 if resource_format==27 else 28,0,1,0,1,0)),error,len(error))
            for fmt in typed_formats:
                srv = ok(view(t,c.byref(ViewDesc(fmt,0,3,0,2,0)),error,len(error))); views.append(srv)
            for layer in range(2):
                for mip in range(3):
                    n=max(1,8>>mip)*max(1,4>>mip)*4
                    content=bytes((i*31+mip*17+layer*41)&255 for i in range(n))
                    put(t,layer*3+mip,content);assert get(t,layer*3+mip,n)==content
                    typeless_checks += n
            for fmt, pixel in ((typed_formats[0],bytes((64,128,191,255))),
                               (typed_formats[1],bytes((137,188,225,255)))):
                rtv = ok(view(t,c.byref(ViewDesc(fmt,1,1,1,1,1)),error,len(error))); views.append(rtv)
                ok(clear(rtv,(c.c_float*4)(.25,.5,.75,1),error,len(error)))
                if resource_format==90:pixel=bytes((pixel[2],pixel[1],pixel[0],pixel[3]))
                assert get(t,4,4*2*4)==pixel*8
                typed=make(TextureDesc(4,2,1,1,fmt,0))
                raw_copy=Copy(0,4,0,0,0,0,4,2)
                ok(copy(typed,t,c.byref(raw_copy),error,len(error)))
                assert get(typed,0,4*2*4)==pixel*8
                typeless_checks += 64
        depth32=make(TextureDesc(8,4,2,2,39,0x48))
        dsv32=ok(view(depth32,c.byref(ViewDesc(40,1,1,1,1,2)),error,len(error)));views.append(dsv32)
        srv32=ok(view(depth32,c.byref(ViewDesc(41,0,2,0,2,0)),error,len(error)));views.append(srv32)
        for fmt,kind in ((39,0),(42,0),(43,0),(41,2)):
            assert not view(depth32,c.byref(ViewDesc(fmt,0,1,0,1,kind)),error,len(error))
        ok(clear_depth(dsv32,3,.375,7,error,len(error)))
        content=get(depth32,3,4*2*4);assert content==struct.pack('<f',.375)*8
        ok(clear_depth(dsv32,2,float('nan'),7,error,len(error)))
        ok(clear_depth(dsv32,0,.625,7,error,len(error)))
        assert get(depth32,3,len(content))==content
        assert not clear_depth(dsv32,3,float('nan'),7,error,len(error))
        assert get(depth32,3,len(content))==content
        typed32=make(TextureDesc(4,2,1,1,40,0x40))
        raw_copy=Copy(0,3,0,0,0,0,4,2)
        ok(copy(typed32,depth32,c.byref(raw_copy),error,len(error)))
        assert get(typed32,0,len(content))==content
        for value,want in ((1.5,1.),(-.5,0.),(float('inf'),1.),(-float('inf'),0.)):
            ok(clear_depth(dsv32,3,value,7,error,len(error)))
            assert get(depth32,3,len(content))==struct.pack('<f',want)*8
        color32=make(TextureDesc(4,2,1,1,39,8))
        content=bytes((i*47)&255 for i in range(32))
        put(color32,0,content);assert get(color32,0,32)==content
        for fmt in (41,42,43):
            srv=ok(view(color32,c.byref(ViewDesc(fmt,0,1,0,1,0)),error,len(error)));views.append(srv)
        typeless_checks += 96
        color16=make(TextureDesc(128,1,1,1,53,0x28))
        content=bytes((i*19)&255 for i in range(256));put(color16,0,content)
        assert get(color16,0,256)==content
        for fmt in (54,56,57,58,59):
            srv=ok(view(color16,c.byref(ViewDesc(fmt,0,1,0,1,0)),error,len(error)));views.append(srv)
        rtv16=ok(view(color16,c.byref(ViewDesc(56,0,1,0,1,1)),error,len(error)));views.append(rtv16)
        ok(clear(rtv16,(c.c_float*4)(.5,.25,.75,1),error,len(error)))
        assert get(color16,0,256)==struct.pack('<H',32768)*128
        depth16=make(TextureDesc(8,4,1,1,53,0x48))
        dsv16=ok(view(depth16,c.byref(ViewDesc(55,0,1,0,1,2)),error,len(error)));views.append(dsv16)
        srv16=ok(view(depth16,c.byref(ViewDesc(56,0,1,0,1,0)),error,len(error)));views.append(srv16)
        for fmt,kind in ((53,0),(54,0),(57,0),(58,0),(59,0),(56,2)):
            assert not view(depth16,c.byref(ViewDesc(fmt,0,1,0,1,kind)),error,len(error))
        ok(clear_depth(dsv16,3,1.5,7,error,len(error)))
        assert get(depth16,0,64)==b'\xff\xff'*32
        typeless_checks += 576
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
            typeless_texture_formats=[27,39,53,90],typeless_checked_bytes=typeless_checks,
            r32_depth_clear_checks=8,
            timestamp_frequency=freq.value, timestamps=[a.value, b.value], guest_frame=False), indent=2)+'\n')
        print(f'resource validation passed: {len(data)+len(expected)+cube_checks+volume_checks} bytes, {32*16} clear pixels, real queue timestamps')
    finally:
        for v in reversed(views): free_view(v)
        for t in reversed(owned): free_texture(t)
        if gpu: close(gpu)


if __name__ == '__main__': main()
