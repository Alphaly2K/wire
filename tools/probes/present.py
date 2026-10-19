#!/usr/bin/env python3
"""Exercise public Metal/Vulkan presentation on the main thread (not a game frame)."""
import argparse
import ctypes as c
import json
from pathlib import Path
import subprocess

from .abi import TextureDesc, ViewDesc


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--backend', required=True, type=Path)
    parser.add_argument('--moltenvk', required=True, type=Path)
    parser.add_argument('--out', required=True, type=Path)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    root = Path(__file__).resolve().parents[2]
    ui_path = args.out.resolve() / 'libwire_present_probe.dylib'
    subprocess.run(['xcrun', 'clang', '-fobjc-arc', '-mmacosx-version-min=11.0',
                    '-Wall', '-Wextra', '-Werror', '-dynamiclib',
                    str(root / 'src/platform/macos.m'), '-framework', 'AppKit',
                    '-framework', 'QuartzCore', '-o', str(ui_path)], check=True)
    ui, gpu_lib = c.CDLL(str(ui_path)), c.CDLL(str(args.backend.resolve()))
    vp, u32, sz = c.c_void_p, c.c_uint32, c.c_size_t
    error = c.create_string_buffer(4096)

    def fn(lib, name, ret, types):
        f = getattr(lib, name)
        f.restype, f.argtypes = ret, types
        return f

    init = fn(ui, 'wire_ui_init', c.c_int, [])
    window = fn(ui, 'wire_ui_window', None, [u32, c.c_char_p, u32, u32, c.c_int])
    layer_get = fn(ui, 'wire_ui_metal_layer', vp, [u32])
    layer_free = fn(ui, 'wire_ui_release_metal_layer', None, [vp])
    remove = fn(ui, 'wire_ui_remove', None, [u32])
    poll = fn(ui, 'wire_ui_poll', c.c_int, [vp])
    shutdown = fn(ui, 'wire_ui_shutdown', None, [])
    open_gpu = fn(gpu_lib, 'wire_gpu_open', vp, [c.c_char_p, vp, vp, sz])
    close = fn(gpu_lib, 'wire_gpu_close', None, [vp])
    create_texture = fn(gpu_lib, 'wire_gpu_texture_create', vp, [vp, c.POINTER(TextureDesc), vp, sz])
    free_texture = fn(gpu_lib, 'wire_gpu_texture_free', None, [vp])
    write = fn(gpu_lib, 'wire_gpu_texture_write', c.c_int, [vp, u32, vp, sz, vp, sz])
    read = fn(gpu_lib, 'wire_gpu_texture_read', c.c_int, [vp, u32, vp, sz, vp, sz])
    create_surface = fn(gpu_lib, 'wire_gpu_surface_create', vp, [vp, vp, vp, sz])
    free_surface = fn(gpu_lib, 'wire_gpu_surface_free', None, [vp])
    present = fn(gpu_lib, 'wire_gpu_surface_present', c.c_int, [vp, vp, u32, u32, u32, vp, sz])
    create_view = fn(gpu_lib,'wire_gpu_image_view_create',vp,[vp,c.POINTER(ViewDesc),vp,sz])
    free_view = fn(gpu_lib,'wire_gpu_image_view_free',None,[vp])
    clear = fn(gpu_lib,'wire_gpu_image_view_clear',c.c_int,[vp,c.POINTER(c.c_float),vp,sz])

    def ok(value):
        if not value:
            raise RuntimeError(error.value.decode(errors='replace'))
        return value

    gpu = other_gpu = surface = layer = 0
    textures = []
    assert init(), 'AppKit main-thread initialization failed'
    try:
        info = c.create_string_buffer(312)
        gpu = ok(open_gpu(str(args.moltenvk.resolve()).encode(), info, error, len(error)))
        window(1, b'wire public Vulkan present validation', 256, 128, 1)
        layer = ok(layer_get(1))
        surface = ok(create_surface(gpu, layer, error, len(error)))
        frames, checked, alias_pixels = 0, 0, 0
        for fmt in (28, 29, 87, 91):
            texture = ok(create_texture(gpu, c.byref(TextureDesc(128, 64, 1, 1, fmt, 0x420)), error, len(error)))
            textures.append(texture)
            if fmt in (29,91):
                alias = ok(create_view(texture,c.byref(ViewDesc(28 if fmt==29 else 87,0,1,0,1,1)),error,len(error)))
                try:
                    ok(clear(alias,(c.c_float*4)(.25,.5,.75,1),error,len(error)))
                    output=c.create_string_buffer(128*64*4)
                    ok(read(texture,0,output,len(output),error,len(error)))
                    expected=bytes((64,128,191,255) if fmt==29 else (191,128,64,255))*8192
                    assert output.raw==expected, 'UNORM alias clear changed encoded values'
                    alias_pixels+=8192
                    assert not create_view(texture,c.byref(ViewDesc(87 if fmt==29 else 28,0,1,0,1,1)),error,len(error))
                finally:
                    free_view(alias)
            data = bytes(v for y in range(64) for x in range(128)
                         for v in ((x*2, y*4, 71, 255) if fmt in (28,29) else (71, y*4, x*2, 255)))
            ok(write(texture, 0, c.create_string_buffer(data), len(data), error, len(error)))
            output = c.create_string_buffer(len(data))
            ok(read(texture, 0, output, len(data), error, len(error)))
            assert output.raw == data
            checked += len(data)
            for width, height, interval in ((256, 128, 0), (256, 128, 1), (192, 96, 1)):
                window(1, b'wire public Vulkan present validation', width, height, 1)
                temporary = ok(layer_get(1))
                layer_free(temporary)
                ok(present(surface, texture, width, height, interval, error, len(error)))
                frames += 1
                event = c.create_string_buffer(16)
                for _ in range(64):
                    if not poll(event):
                        break
            assert not present(surface, texture, 0, 96, 0, error, len(error))
            assert not present(surface, texture, 192, 96, 2, error, len(error))
        other_gpu = ok(open_gpu(str(args.moltenvk.resolve()).encode(), info, error, len(error)))
        alien = ok(create_texture(other_gpu, c.byref(TextureDesc(16, 16, 1, 1, 28, 0x20)), error, len(error)))
        try:
            assert not present(surface, alien, 192, 96, 0, error, len(error))
        finally:
            free_texture(alien)
        # Outstanding surface/layer survives native window removal until teardown.
        remove(1)
        report = dict(native_queue_presents=frames, source_bytes_verified=checked,
                      formats=[28, 29, 87, 91], resize=True, cross_device_rejected=True,
                      invalid_size_sync_rejected=True, guest_frame=False,
                      unorm_alias_clear_pixels_verified=alias_pixels,
                      cpu_readback_in_present=False)
        (args.out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
        print(f'public Vulkan presentation passed: {frames} submits, resize, RGBA/BGRA, device/lifetime checks')
    finally:
        if surface:
            free_surface(surface)
        if layer:
            layer_free(layer)
        for texture in reversed(textures):
            free_texture(texture)
        if other_gpu:
            close(other_gpu)
        if gpu:
            close(gpu)
        shutdown()


if __name__ == '__main__':
    main()
