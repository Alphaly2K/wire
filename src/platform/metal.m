// Public Metal compute rasterizer. Buffers contain checked host snapshots only.
#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#include <stdint.h>
#include <string.h>
@interface WireRaster : NSObject
@property id<MTLDevice> device;
@property id<MTLCommandQueue> queue;
@property id<MTLComputePipelineState> pipeline;
@property id<MTLBuffer> triangles,params,source,dest,depth,program;
@end
@implementation WireRaster
@end
static void error_text(char *out,unsigned cap,NSString *text) {
    if(cap) {strncpy(out,text.UTF8String?:"Metal unavailable",cap-1);out[cap-1]=0;}
}
void *wire_metal_create(const char *source,unsigned size,char *error,unsigned cap) {
    @autoreleasepool {
        WireRaster *r=[WireRaster new];r.device=MTLCreateSystemDefaultDevice();
        if(!r.device) {error_text(error,cap,@"No Metal device");return NULL;}
        NSError *e=nil;
        MTLCompileOptions *options=[MTLCompileOptions new];options.fastMathEnabled=NO;
        NSString *text=[[NSString alloc] initWithBytes:source length:size encoding:NSUTF8StringEncoding];
        id<MTLLibrary> library=[r.device newLibraryWithSource:text options:options error:&e];
        if(!library) {error_text(error,cap,e.localizedDescription);return NULL;}
        r.pipeline=[r.device newComputePipelineStateWithFunction:[library newFunctionWithName:@"raster"] error:&e];
        r.queue=[r.device newCommandQueue];
        if(!r.pipeline || !r.queue) {error_text(error,cap,e.localizedDescription);return NULL;}
        return (__bridge_retained void *)r;
    }
}
void wire_metal_destroy(void *ctx) {
    if(ctx) { @autoreleasepool { __unused id owner=CFBridgingRelease(ctx); } }
}
static id<MTLBuffer> buffer(WireRaster *r,id<MTLBuffer> old,const void *data,size_t size) {
    if(!old || old.length<size) old=[r.device newBufferWithLength:MAX(size,4u) options:MTLResourceStorageModeShared];
    if(old && size) memcpy(old.contents,data,size);
    return old;
}
int wire_metal_draw(void *ctx,const float *triangles,unsigned count,const uint32_t *params,
    const uint32_t *source,unsigned source_count,uint32_t *dest,unsigned dest_count,
    uint32_t *depth,unsigned depth_count,const uint32_t *program,unsigned program_count,char *error,unsigned cap) {
    @autoreleasepool {
        WireRaster *r=(__bridge WireRaster *)ctx;
        if(params[7]!=count || (uint64_t)params[0]*params[1]!=dest_count
            || (uint64_t)params[2]*params[3]>source_count
            || (params[199] && (uint64_t)params[6]*params[1]>depth_count)) return 0;
        if(!program_count || program[0]>96 || (program[0] && program_count!=137+program[0]*8)) return 0;
        if(program[0] && (uint64_t)program[1]*program[2]+program[7]>source_count) return 0;
        r.program=buffer(r,r.program,program,(size_t)program_count*4);
        r.triangles=buffer(r,r.triangles,triangles,(size_t)count*144);
        r.params=buffer(r,r.params,params,800);
        r.source=buffer(r,r.source,source,(size_t)source_count*4);
        r.dest=buffer(r,r.dest,dest,(size_t)dest_count*4);
        r.depth=buffer(r,r.depth,depth,(size_t)depth_count*4);
        if(!r.triangles || !r.params || !r.source || !r.dest || !r.depth || !r.program) {
            error_text(error,cap,@"Metal buffer allocation failed");return 0;
        }
        id<MTLCommandBuffer> command=[r.queue commandBuffer];
        id<MTLComputeCommandEncoder> encoder=[command computeCommandEncoder];
        if(!command || !encoder) {error_text(error,cap,@"Metal command allocation failed");return 0;}
        [encoder setComputePipelineState:r.pipeline];
        [encoder setBuffer:r.triangles offset:0 atIndex:0];
        [encoder setBuffer:r.params offset:0 atIndex:1];
        [encoder setBuffer:r.source offset:0 atIndex:2];
        [encoder setBuffer:r.dest offset:0 atIndex:3];
        [encoder setBuffer:r.depth offset:0 atIndex:4];
        [encoder setBuffer:r.program offset:0 atIndex:5];
        [encoder dispatchThreadgroups:MTLSizeMake((params[0]+15)/16,(params[1]+15)/16,1)
            threadsPerThreadgroup:MTLSizeMake(16,16,1)];
        [encoder endEncoding];[command commit];[command waitUntilCompleted];
        if(command.status!=MTLCommandBufferStatusCompleted) {
            error_text(error,cap,command.error.localizedDescription);return 0;
        }
        memcpy(dest,r.dest.contents,(size_t)dest_count*4);
        if(depth_count) memcpy(depth,r.depth.contents,(size_t)depth_count*4);
        return 1;
    }
}
