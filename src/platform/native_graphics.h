/* Host-only ABI, never mapped into the guest. SPDX-License-Identifier: LGPL-2.1-only */
#pragma once
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
#define WIRE_GPU_EXPORT __attribute__((visibility("default")))
/* Context calls are serialized by the owner. Free every shader before closing
   its context; these are trusted host pointers, not guest-visible handles. */
typedef struct WireGpuInfo {
  char name[256];
  uint32_t vendor, device, api_version, reserved;
  uint64_t dedicated_video, dedicated_system, shared_system;
  uint8_t uuid[16];
} WireGpuInfo;
typedef struct WireBinding {
  uint32_t set, binding, descriptor_type, count;
} WireBinding;
typedef struct WireShaderInfo {
  const uint32_t *words;
  size_t word_count;
  const WireBinding *bindings;
  size_t binding_count;
  uint32_t stage, push_bytes;
} WireShaderInfo;
/* GS lowered to compute. I/O data is planar in declaration order, packed
   32-bit words. built_in=UINT32_MAX denotes a location/component input.
   array_size is the number of scalars including the per-vertex dimension.
   No pointers or Vulkan handles are part of the data buffers. */
typedef struct WireGeometryIo {
  uint32_t location, component, built_in, scalar_type, array_size, word_offset;
  uint32_t vector_size, array_dimensions;
} WireGeometryIo;
typedef struct WireGeometryInfo {
  const WireGeometryIo *inputs, *outputs;
  size_t input_count, output_count;
  uint32_t input_vertices, output_primitive, max_vertices, instances;
  uint32_t input_words, vertex_words, invocation_words, reserved;
} WireGeometryInfo;
typedef struct WireBufferBinding {
  uint32_t set, binding;
  void *buffer;
} WireBufferBinding;
typedef struct WireTextureDesc { uint32_t width,height,mips,layers,format,bind; } WireTextureDesc;
WIRE_GPU_EXPORT void *wire_gpu_texture_create(void *gpu, const WireTextureDesc *desc, char *error, size_t capacity);
WIRE_GPU_EXPORT void wire_gpu_texture_free(void *texture);
typedef struct WireTextureCopy {uint32_t dst_sub,src_sub,x,y,left,top,right,bottom;} WireTextureCopy;
WIRE_GPU_EXPORT int wire_gpu_texture_copy(void *dst,void *src,const WireTextureCopy *region,char *error,size_t capacity);
WIRE_GPU_EXPORT int wire_gpu_texture_write(void *texture, uint32_t subresource, const uint8_t *data,
    size_t size, char *error, size_t capacity);
WIRE_GPU_EXPORT int wire_gpu_texture_read(void *texture, uint32_t subresource, uint8_t *data,
    size_t size, char *error, size_t capacity);
typedef struct WireImageViewDesc {uint32_t format,mip,mips,layer,layers,kind;} WireImageViewDesc;
WIRE_GPU_EXPORT void *wire_gpu_image_view_create(void *texture,const WireImageViewDesc *desc,char *error,size_t capacity);
WIRE_GPU_EXPORT void wire_gpu_image_view_free(void *view);
WIRE_GPU_EXPORT int wire_gpu_image_view_clear(void *view,const float *rgba,char *error,size_t capacity);
/* Owned host buffers: usage bit 1=CBV, 2=storage, 4=vertex, 8=index.
   Calls are serialized, and every submitted operation waits for completion.
   Buffer handles retain their context in the Rust owner. Initial data is copied. */
WIRE_GPU_EXPORT void *wire_gpu_buffer_create(void *gpu, const uint8_t *data, size_t size,
    uint32_t usage, char *error, size_t capacity);
WIRE_GPU_EXPORT void wire_gpu_buffer_free(void *buffer);
WIRE_GPU_EXPORT int wire_gpu_buffer_write(void *buffer, size_t offset, const uint8_t *data,
    size_t size, char *error, size_t capacity);
WIRE_GPU_EXPORT int wire_gpu_buffer_read(void *buffer, size_t offset, uint8_t *data,
    size_t size, char *error, size_t capacity);
WIRE_GPU_EXPORT void *wire_gpu_buffer_view_create(void *buffer,uint32_t format,size_t offset,size_t size,char *error,size_t capacity);
WIRE_GPU_EXPORT void wire_gpu_buffer_view_free(void *view);
WIRE_GPU_EXPORT uint32_t wire_gpu_abi(void);
/* Native format capabilities, intersected with the bridge's implemented scope.
   DXGI format in, D3D11_FORMAT_SUPPORT mask out; zero means unsupported. */
WIRE_GPU_EXPORT uint32_t wire_gpu_format_support(void *gpu, uint32_t format);
/* Fence-completed queue timestamp. Frequency derives from timestampPeriod. */
WIRE_GPU_EXPORT int wire_gpu_timestamp(void *gpu, uint64_t *ticks, uint64_t *frequency,
    char *error, size_t capacity);
WIRE_GPU_EXPORT void *wire_gpu_open(const char *library, WireGpuInfo *info, char *error, size_t capacity);
WIRE_GPU_EXPORT void wire_gpu_close(void *gpu);
WIRE_GPU_EXPORT void *wire_gpu_compile(void *gpu, const uint8_t *data, size_t size,
                                     uint32_t stage, char *error, size_t capacity);
WIRE_GPU_EXPORT int wire_gpu_shader_info(void *shader, WireShaderInfo *info);
WIRE_GPU_EXPORT void wire_gpu_shader_free(void *shader);
WIRE_GPU_EXPORT int wire_gpu_geometry_info(void *shader, WireGeometryInfo *info);
WIRE_GPU_EXPORT void *wire_gpu_geometry_vertex(void *gpu, void *geometry, char *error, size_t capacity);
/* stage=8 is a GS compute expansion, not a native Vulkan geometry shader.
   Each output invocation has a count word and max_vertices+1 records;
   each record has a start-of-strip word followed by vertex_words payload.
   The extra record is an overflow sink. Over-emission fails atomically.
   Optional read-only CBV/SRV buffer descriptors must match shader reflection;
   UAV writes, image, sampler, texel-buffer and push-data are not supported yet. */
WIRE_GPU_EXPORT int wire_gpu_geometry_dispatch(void *gpu, void *shader,
    const uint32_t *input, size_t input_words, uint32_t primitives,
    const WireBufferBinding *bindings, size_t binding_count,
    uint32_t *output, size_t output_words, char *error, size_t capacity);
/* Rasterize expanded stream-0 payload/list indices through a generated
   forwarding VS and the real DXBC PS. Initial 64x64 diagnostic scope:
   position + basic scalar/vector outputs, resource-free PS, no depth/blend. */
WIRE_GPU_EXPORT int wire_gpu_geometry_draw_rgba64(void *gpu, void *geometry, void *pixel,
    const uint32_t *vertices, size_t vertex_words, const uint32_t *indices, size_t index_count,
    uint8_t *output, size_t output_size, char *error, size_t capacity);
/* Validate actual graphics or compute pipeline creation; no draw is submitted. */
WIRE_GPU_EXPORT int wire_gpu_pipeline(void *gpu, void *vertex, void *pixel, void *compute,
                                    char *error, size_t capacity);
/* Bounded 64x64 RGBA8 offscreen draw/readback with position/UV VS and t0/s0 PS.
   Source/output each hold 16384 bytes; output is committed only after the fence. */
WIRE_GPU_EXPORT int wire_gpu_blit_rgba64(void *gpu, void *vertex, void *pixel,
                                      const uint8_t *source, size_t size,
                                      uint8_t *output, size_t output_size,
                                      char *error, size_t capacity);
#ifdef __cplusplus
}
#endif
