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
WIRE_GPU_EXPORT uint32_t wire_gpu_abi(void);
WIRE_GPU_EXPORT void *wire_gpu_open(const char *library, WireGpuInfo *info, char *error, size_t capacity);
WIRE_GPU_EXPORT void wire_gpu_close(void *gpu);
WIRE_GPU_EXPORT void *wire_gpu_compile(void *gpu, const uint8_t *data, size_t size,
                                     uint32_t stage, char *error, size_t capacity);
WIRE_GPU_EXPORT int wire_gpu_shader_info(void *shader, WireShaderInfo *info);
WIRE_GPU_EXPORT void wire_gpu_shader_free(void *shader);
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
