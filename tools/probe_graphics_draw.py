#!/usr/bin/env python3
"""Exercise guest draw backend state with original shaders and pixel expectations."""
import argparse
import ctypes as c
import json
import struct
import subprocess
from pathlib import Path
from probe_native_graphics import GpuInfo, ShaderInfo
from probe_graphics_resources import TextureDesc, ViewDesc


class Vertex(c.Structure):
    _fields_=[('buffer',c.c_void_p),('stride',c.c_uint32),('offset',c.c_uint32)]
class Attribute(c.Structure):
    _fields_=[(n,c.c_uint32) for n in ('location','format','slot','offset','input_class','step')]
class Descriptor(c.Structure):
    _fields_=[(n,c.c_uint32) for n in ('set','binding','kind','reserved')]+[('resource',c.c_void_p),('sampler',c.c_uint32*13)]
class Draw(c.Structure):
    _fields_=[(n,c.c_void_p) for n in ('vertex','pixel','target','depth','index')]+[
        ('vertices',Vertex*32),('attributes',c.POINTER(Attribute)),('attribute_count',c.c_size_t),
        ('bindings',c.POINTER(Descriptor)),('binding_count',c.c_size_t)]+[
        (n,c.c_uint32) for n in ('topology','index_format','index_offset','indexed','count','first')]+[
        ('base_vertex',c.c_int32),('instances',c.c_uint32),('first_instance',c.c_uint32),
        ('viewport',c.c_float*6),('scissor',c.c_int32*4),('blend',c.c_uint32*66),
        ('depth_state',c.c_uint32*13),('raster',c.c_uint32*10),('blend_factor',c.c_float*4),
        ('sample_mask',c.c_uint32),('stencil_ref',c.c_uint32)]

def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ('backend','moltenvk','fixtures','out','validator'):
        p.add_argument('--'+name,required=True,type=Path)
    args=p.parse_args();args.out.mkdir(parents=True,exist_ok=True)
    (args.out/'report.json').unlink(missing_ok=True)
    assert c.sizeof(Draw)==1040 and Draw.viewport.offset==620 and c.sizeof(Descriptor)==80
    lib=c.CDLL(str(args.backend.resolve()));error=c.create_string_buffer(4096)
    vp,sz,u32=c.c_void_p,c.c_size_t,c.c_uint32
    def fn(name,ret,types):
        f=getattr(lib,name);f.restype=ret;f.argtypes=types;return f
    open_gpu=fn('wire_gpu_open',vp,[c.c_char_p,vp,vp,sz]);close=fn('wire_gpu_close',None,[vp])
    compile_shader=fn('wire_gpu_compile',vp,[vp,vp,sz,u32,vp,sz]);shader_info=fn('wire_gpu_shader_info',c.c_int,[vp,c.POINTER(ShaderInfo)])
    free_shader=fn('wire_gpu_shader_free',None,[vp]);buffer=fn('wire_gpu_buffer_create',vp,[vp,vp,sz,u32,vp,sz])
    free_buffer=fn('wire_gpu_buffer_free',None,[vp]);write_buffer=fn('wire_gpu_buffer_write',c.c_int,[vp,sz,vp,sz,vp,sz])
    texture=fn('wire_gpu_texture_create',vp,[vp,c.POINTER(TextureDesc),vp,sz]);free_texture=fn('wire_gpu_texture_free',None,[vp])
    read=fn('wire_gpu_texture_read',c.c_int,[vp,u32,vp,sz,vp,sz]);view=fn('wire_gpu_image_view_create',vp,[vp,c.POINTER(ViewDesc),vp,sz]);free_view=fn('wire_gpu_image_view_free',None,[vp])
    clear=fn('wire_gpu_image_view_clear',c.c_int,[vp,c.POINTER(c.c_float),vp,sz]);clear_depth=fn('wire_gpu_image_view_clear_depth',c.c_int,[vp,u32,c.c_float,u32,vp,sz])
    submit=fn('wire_gpu_draw',c.c_int,[vp,c.POINTER(Draw),vp,sz]);gpu=None;shaders=[];buffers=[];textures=[];views=[]
    def ok(v):
        if not v:raise RuntimeError(error.value.decode(errors='replace'))
        return v
    def make_buffer(data,usage):
        b=ok(buffer(gpu,c.create_string_buffer(data),len(data),usage,error,len(error)));buffers.append(b);return b
    def put(b,data):ok(write_buffer(b,0,c.create_string_buffer(data),len(data),error,len(error)))
    def make_texture(d):
        t=ok(texture(gpu,c.byref(d),error,len(error)));textures.append(t);return t
    def make_view(t,d):
        v=ok(view(t,c.byref(d),error,len(error)));views.append(v);return v
    def get(t,size):
        b=c.create_string_buffer(size);ok(read(t,0,b,size,error,len(error)));return b.raw
    try:
        gpu=ok(open_gpu(str(args.moltenvk.resolve()).encode(),c.byref(GpuInfo()),error,len(error)))
        for name,stage in [('offset.vs.dxbc',1),('constant-color.ps.dxbc',2)]:
            data=(args.fixtures/name).read_bytes();s=ok(compile_shader(gpu,c.create_string_buffer(data),len(data),stage,error,len(error)));shaders.append(s)
            info=ShaderInfo();ok(shader_info(s,c.byref(info)))
            spv=args.out/(name+'.spv');spv.write_bytes(c.string_at(info.words,info.word_count*4))
            subprocess.run([str(args.validator),'--target-env','vulkan1.3',str(spv)],check=True)
        width,height=32,24
        target=make_texture(TextureDesc(width,height,1,1,28,0x20));rtv=make_view(target,ViewDesc(28,0,1,0,1,1))
        depth=make_texture(TextureDesc(width,height,1,1,19,0x40));dsv=make_view(depth,ViewDesc(20,0,1,0,1,2))
        positions=[(-.5,.5,.5,1),(.5,.5,.5,1),(.5,-.5,.5,1),(-.5,-.5,.5,1)]
        data=bytes(16)+b''.join(bytes(8)+struct.pack('<4f',*v)+bytes(8) for v in positions)
        vb=make_buffer(data,4);ib=make_buffer(struct.pack('<8H',77,88,1,2,3,1,3,4),8)
        vc=make_buffer(struct.pack('<4f',0,0,0,0),1);pc=make_buffer(struct.pack('<4f',.25,.5,.75,1),1)
        attrs=(Attribute*1)(Attribute(0,2,3,8,0,0));bindings=(Descriptor*2)(Descriptor(0,0,0,0,vc),Descriptor(1,0,0,0,pc))
        d=Draw(vertex=shaders[0],pixel=shaders[1],target=rtv,depth=dsv,index=ib,attributes=attrs,attribute_count=1,bindings=bindings,binding_count=2,
            topology=4,index_format=57,index_offset=2,indexed=1,count=6,first=1,base_vertex=-1,instances=1,
            viewport=(c.c_float*6)(0,0,width,height,0,1),depth_state=(c.c_uint32*13)(1,1,2,1,0xffff,1,1,3,8,1,1,3,8),
            raster=(c.c_uint32*10)(3,1,0,0,0,0x80000000,1,0,1,0),blend_factor=(c.c_float*4)(1,1,1,1),sample_mask=0xffffffff,stencil_ref=9)
        d.vertices[3]=Vertex(vb,32,16);d.blend[9]=15
        background=bytes((32,64,128,255));color=bytes((64,128,191,255));checks=0
        def reset():
            ok(clear(rtv,(c.c_float*4)(.125,.25,.5,1),error,len(error)))
            ok(clear_depth(dsv,3,.75,5,error,len(error)))
        def draw():ok(submit(gpu,c.byref(d),error,len(error)))
        def expected(rect,pixel=color):
            left,top,right,bottom=rect
            return b''.join(pixel if left<=x<right and top<=y<bottom else background for y in range(height) for x in range(width))
        reset();draw();content=get(target,width*height*4);assert content==expected((8,6,24,18));checks+=1
        want_depth=b''.join((struct.pack('<fB',.5,9)+bytes(3)) if 8<=x<24 and 6<=y<18 else (struct.pack('<fB',.75,5)+bytes(3)) for y in range(height) for x in range(width))
        assert get(depth,width*height*8)==want_depth;checks+=1
        # Viewport and scissor must retain prior content outside the draw region.
        reset();d.raster[7]=1;d.scissor[:]=(10,8,18,13);draw();assert get(target,width*height*4)==expected((10,8,18,13));checks+=1
        d.raster[7]=0;d.viewport[:]=(4,2,16,12,0,1);reset();draw();assert get(target,width*height*4)==expected((8,5,16,11));checks+=1
        d.viewport[:]=(0,0,width,height,0,1)
        # Explicit LESS test rejects geometry behind existing depth; no clear.
        reset();d.depth_state[2]=5;draw();assert get(target,width*height*4)==background*(width*height);checks+=1
        d.depth_state[2]=2
        # Source-alpha blend uses the existing attachment, including alpha channel.
        reset();put(pc,struct.pack('<4f',.25,.5,.75,.5));d.blend[2:10]=(1,5,6,1,2,1,1,15);draw()
        output=get(target,width*height*4);wanted=expected((8,6,24,18),bytes((48,96,160,128)))
        assert all(abs(a-b)<=1 for a,b in zip(output,wanted));checks+=1
        d.blend[2]=0;put(pc,struct.pack('<4f',.25,.5,.75,1))
        # D3D clockwise front-facing quad survives BACK cull, not FRONT cull.
        reset();d.raster[1]=3;draw();assert get(target,width*height*4)==expected((8,6,24,18));checks+=1
        reset();d.raster[1]=2;draw();assert get(target,width*height*4)==background*(width*height);checks+=1
        d.raster[1]=1
        # Reject bad state/ranges before any GPU write, retaining both attachments.
        rejected=[]
        for field,value in [('count',1000000),('index_format',28),('base_vertex',-20),('attribute_count',33),('binding_count',0)]:
            before=get(target,width*height*4),get(depth,width*height*8);old=getattr(d,field);setattr(d,field,value)
            assert not submit(gpu,c.byref(d),error,len(error));setattr(d,field,old)
            assert before==(get(target,width*height*4),get(depth,width*height*8));rejected.append(field)
        d.raster[3]=1;before=get(target,width*height*4);assert not submit(gpu,c.byref(d),error,len(error));d.raster[3]=0
        assert get(target,width*height*4)==before;rejected.append('depth_bias')
        args.out.joinpath('report.json').write_text(json.dumps(dict(draw_checks=checks,pixels_per_check=width*height,
            rejected_preserve_pixels=rejected,guest_frame=False),indent=2)+'\n')
        print(f'draw validation passed: {checks} checks, {width*height} pixels each, {len(rejected)} atomic rejections')
    finally:
        for v in reversed(views):free_view(v)
        for t in reversed(textures):free_texture(t)
        for b in reversed(buffers):free_buffer(b)
        for s in reversed(shaders):free_shader(s)
        if gpu:close(gpu)

if __name__=='__main__':main()
