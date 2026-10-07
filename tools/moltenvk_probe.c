/* Standalone host-GPU diagnostic. No guest pointers or CPU code enter Vulkan.
 * Build against separately supplied official Vulkan headers; see the runner.
 * SPDX-License-Identifier: LGPL-2.1-only
 */
#define VK_NO_PROTOTYPES
#include <vulkan/vulkan.h>
#include <dlfcn.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define WORDS 4096u
#define SALT 0x6b8b4567u
#define CHECK(expr) do { VkResult r_ = (expr); if (r_ != VK_SUCCESS) { \
  fprintf(stderr, "%s: VkResult=%d\n", #expr, r_); goto cleanup; } } while (0)
#define LOAD_GLOBAL(name) PFN_##name name = (PFN_##name)get_proc(NULL, #name); \
  if (!name) { fprintf(stderr, "missing %s\n", #name); goto cleanup; }
#define LOAD_INSTANCE(name) PFN_##name name = (PFN_##name)get_proc(instance, #name); \
  if (!name) { fprintf(stderr, "missing %s\n", #name); goto cleanup; }
#define LOAD_DEVICE(name) PFN_##name name = (PFN_##name)get_device_proc(device, #name); \
  if (!name) { fprintf(stderr, "missing %s\n", #name); goto cleanup; }

/* Independently authored SPIR-V 1.0: local_size_x=64;
 * layout(set=0,binding=0,std430) buffer Out { uint data[]; };
 * data[gl_GlobalInvocationID.x] = gl_GlobalInvocationID.x ^ SALT;
 * BufferBlock/Uniform is the Vulkan 1.0 storage-buffer representation.
 * The probe dispatches exactly WORDS invocations, so no out-of-bounds work.
 */
#define OP(n, code) (((uint32_t)(n) << 16) | (code))
static const uint32_t shader[] = {
  0x07230203, 0x00010000, 0, 22, 0,
  OP(2,17),1,                       /* Capability Shader */
  OP(3,14),0,1,                     /* Logical GLSL450 */
  OP(6,15),5,14,0x6e69616d,0,6,     /* GLCompute %main "main" %gid */
  OP(6,16),14,17,64,1,1,            /* LocalSize */
  OP(4,71),6,11,28,                 /* gid BuiltIn GlobalInvocationId */
  OP(4,71),7,6,4,                   /* ArrayStride 4 */
  OP(3,71),8,3,                     /* BufferBlock */
  OP(5,72),8,0,35,0,                /* member Offset 0 */
  OP(4,71),10,34,0,                 /* DescriptorSet 0 */
  OP(4,71),10,33,0,                 /* Binding 0 */
  OP(2,19),1,                       /* void */
  OP(3,33),2,1,                     /* fn()->void */
  OP(4,21),3,32,0,                  /* uint */
  OP(4,23),4,3,3,                   /* uvec3 */
  OP(4,32),5,1,4,                   /* ptr Input uvec3 */
  OP(4,59),5,6,1,                   /* gid */
  OP(3,29),7,3,                     /* runtime array uint */
  OP(3,30),8,7,                     /* block */
  OP(4,32),9,2,8,                   /* ptr Uniform block */
  OP(4,59),9,10,2,                  /* out */
  OP(4,32),11,2,3,                  /* ptr Uniform uint */
  OP(4,43),3,12,0,                  /* zero */
  OP(4,43),3,13,SALT,
  OP(5,54),1,14,0,2,                /* main */
  OP(2,248),15,
  OP(4,61),4,16,6,                  /* load gid */
  OP(5,81),3,17,16,0,               /* gid.x */
  OP(5,198),3,18,17,13,             /* xor */
  OP(6,65),11,19,10,12,17,          /* &data[gid.x] */
  OP(3,62),19,18,
  OP(1,253), OP(1,56),
};

static int has_extension(const VkExtensionProperties *ext, uint32_t count,
                         const char *name) {
  for (uint32_t i = 0; i < count; i++)
    if (!strcmp(ext[i].extensionName, name)) return 1;
  return 0;
}

static void json_string(const char *value) {
  putchar('"');
  for (const unsigned char *p = (const unsigned char *)value; *p; p++) {
    if (*p == '"' || *p == '\\') { putchar('\\'); putchar(*p); }
    else if (*p < 32) printf("\\u%04x", *p);
    else putchar(*p);
  }
  putchar('"');
}

int main(int argc, char **argv) {
  if (argc != 2) {
    fprintf(stderr, "usage: moltenvk-probe /absolute/path/libMoltenVK.dylib\n");
    return 2;
  }
  int status = 1, submitted = 0;
  void *library = NULL, *mapped = NULL;
  VkInstance instance = VK_NULL_HANDLE;
  VkDevice device = VK_NULL_HANDLE;
  VkBuffer buffer = VK_NULL_HANDLE;
  VkDeviceMemory memory = VK_NULL_HANDLE;
  VkDescriptorSetLayout set_layout = VK_NULL_HANDLE;
  VkDescriptorPool descriptor_pool = VK_NULL_HANDLE;
  VkPipelineLayout pipeline_layout = VK_NULL_HANDLE;
  VkShaderModule module = VK_NULL_HANDLE;
  VkPipeline pipeline = VK_NULL_HANDLE;
  VkCommandPool command_pool = VK_NULL_HANDLE;
  VkFence fence = VK_NULL_HANDLE;
  VkPhysicalDevice *physical = NULL;
  VkExtensionProperties *instance_ext = NULL, *device_ext = NULL;
  VkQueueFamilyProperties *queues = NULL;
  /* Cleanup entry points are available even after partial setup failure. */
  PFN_vkDestroyInstance destroy_instance = NULL;
  PFN_vkDestroyDevice destroy_device = NULL;
  PFN_vkDeviceWaitIdle wait_idle = NULL;
  PFN_vkDestroyBuffer destroy_buffer = NULL;
  PFN_vkFreeMemory free_memory = NULL;
  PFN_vkUnmapMemory unmap_memory = NULL;
  PFN_vkDestroyDescriptorSetLayout destroy_set_layout = NULL;
  PFN_vkDestroyDescriptorPool destroy_descriptor_pool = NULL;
  PFN_vkDestroyPipelineLayout destroy_pipeline_layout = NULL;
  PFN_vkDestroyShaderModule destroy_module = NULL;
  PFN_vkDestroyPipeline destroy_pipeline = NULL;
  PFN_vkDestroyCommandPool destroy_command_pool = NULL;
  PFN_vkDestroyFence destroy_fence = NULL;

  library = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
  if (!library) { fprintf(stderr, "dlopen: %s\n", dlerror()); goto cleanup; }
  PFN_vkGetInstanceProcAddr get_proc =
    (PFN_vkGetInstanceProcAddr)dlsym(library, "vkGetInstanceProcAddr");
  if (!get_proc) { fprintf(stderr, "missing vkGetInstanceProcAddr\n"); goto cleanup; }
  LOAD_GLOBAL(vkEnumerateInstanceVersion);
  LOAD_GLOBAL(vkEnumerateInstanceExtensionProperties);
  LOAD_GLOBAL(vkCreateInstance);
  uint32_t api_version = 0, count = 0;
  CHECK(vkEnumerateInstanceVersion(&api_version));
  if (api_version < VK_API_VERSION_1_2) {
    fprintf(stderr, "probe requires Vulkan 1.2 for feature reporting\n"); goto cleanup;
  }
  CHECK(vkEnumerateInstanceExtensionProperties(NULL, &count, NULL));
  instance_ext = calloc(count, sizeof(*instance_ext));
  if (!instance_ext) goto cleanup;
  CHECK(vkEnumerateInstanceExtensionProperties(NULL, &count, instance_ext));
  const char *instance_extensions[] = { VK_KHR_PORTABILITY_ENUMERATION_EXTENSION_NAME };
  int portability = has_extension(instance_ext, count, instance_extensions[0]);
  /* MoltenVK caps its advertised physical-device version to the requested
   * instance version. Request the highest header-known loader version to
   * avoid mistaking that cap for a hardware/driver limitation. */
  uint32_t requested_api = api_version < VK_API_VERSION_1_4 ? api_version : VK_API_VERSION_1_4;
  VkApplicationInfo app = { .sType = VK_STRUCTURE_TYPE_APPLICATION_INFO,
    .pApplicationName = "wire host GPU probe", .apiVersion = requested_api };
  VkInstanceCreateInfo ici = { .sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
    .pApplicationInfo = &app,
    .flags = portability ? VK_INSTANCE_CREATE_ENUMERATE_PORTABILITY_BIT_KHR : 0,
    .enabledExtensionCount = portability ? 1u : 0u,
    .ppEnabledExtensionNames = portability ? instance_extensions : NULL };
  CHECK(vkCreateInstance(&ici, NULL, &instance));
  destroy_instance = (PFN_vkDestroyInstance)get_proc(instance, "vkDestroyInstance");
  if (!destroy_instance) goto cleanup;
  LOAD_INSTANCE(vkEnumeratePhysicalDevices);
  LOAD_INSTANCE(vkGetPhysicalDeviceProperties);
  LOAD_INSTANCE(vkGetPhysicalDeviceFeatures2);
  LOAD_INSTANCE(vkGetPhysicalDeviceMemoryProperties);
  LOAD_INSTANCE(vkGetPhysicalDeviceQueueFamilyProperties);
  LOAD_INSTANCE(vkEnumerateDeviceExtensionProperties);
  LOAD_INSTANCE(vkCreateDevice);
  LOAD_INSTANCE(vkGetDeviceProcAddr);
  PFN_vkGetDeviceProcAddr get_device_proc = vkGetDeviceProcAddr;
  CHECK(vkEnumeratePhysicalDevices(instance, &count, NULL));
  if (!count) { fprintf(stderr, "no Vulkan physical device\n"); goto cleanup; }
  physical = calloc(count, sizeof(*physical));
  if (!physical) goto cleanup;
  CHECK(vkEnumeratePhysicalDevices(instance, &count, physical));
  /* Probe the first real device; report its name, never invent an adapter. */
  VkPhysicalDevice gpu = physical[0];
  VkPhysicalDeviceProperties properties;
  vkGetPhysicalDeviceProperties(gpu, &properties);
  if (properties.apiVersion < VK_API_VERSION_1_2) goto cleanup;
  CHECK(vkEnumerateDeviceExtensionProperties(gpu, NULL, &count, NULL));
  uint32_t device_extension_count = count;
  device_ext = calloc(count, sizeof(*device_ext));
  if (!device_ext) goto cleanup;
  CHECK(vkEnumerateDeviceExtensionProperties(gpu, NULL, &device_extension_count, device_ext));
  VkPhysicalDeviceVulkan12Features v12 = {
    .sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES };
  VkPhysicalDeviceFeatures2 features = {
    .sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2, .pNext = &v12 };
  vkGetPhysicalDeviceFeatures2(gpu, &features);
  VkPhysicalDeviceMemoryProperties memory_properties;
  vkGetPhysicalDeviceMemoryProperties(gpu, &memory_properties);
  vkGetPhysicalDeviceQueueFamilyProperties(gpu, &count, NULL);
  queues = calloc(count, sizeof(*queues));
  if (!queues) goto cleanup;
  vkGetPhysicalDeviceQueueFamilyProperties(gpu, &count, queues);
  uint32_t family = UINT32_MAX;
  for (uint32_t i = 0; i < count; i++)
    if (queues[i].queueCount && (queues[i].queueFlags & VK_QUEUE_COMPUTE_BIT)) { family = i; break; }
  if (family == UINT32_MAX) { fprintf(stderr, "no compute queue\n"); goto cleanup; }
  const char *device_extensions[] = { "VK_KHR_portability_subset" };
  int subset = has_extension(device_ext, device_extension_count, device_extensions[0]);
  float priority = 1.0f;
  VkDeviceQueueCreateInfo qci = { .sType = VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO,
    .queueFamilyIndex = family, .queueCount = 1, .pQueuePriorities = &priority };
  /* Enable no optional GPU features; this is not a D3D feature-level claim. */
  VkDeviceCreateInfo dci = { .sType = VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO,
    .queueCreateInfoCount = 1, .pQueueCreateInfos = &qci,
    .enabledExtensionCount = subset ? 1u : 0u,
    .ppEnabledExtensionNames = subset ? device_extensions : NULL };
  CHECK(vkCreateDevice(gpu, &dci, NULL, &device));
#define CLEANUP_FN(variable, name) variable = (PFN_##name)get_device_proc(device, #name); \
  if (!variable) goto cleanup
  CLEANUP_FN(destroy_device, vkDestroyDevice);
  CLEANUP_FN(wait_idle, vkDeviceWaitIdle);
  CLEANUP_FN(destroy_buffer, vkDestroyBuffer);
  CLEANUP_FN(free_memory, vkFreeMemory);
  CLEANUP_FN(unmap_memory, vkUnmapMemory);
  CLEANUP_FN(destroy_set_layout, vkDestroyDescriptorSetLayout);
  CLEANUP_FN(destroy_descriptor_pool, vkDestroyDescriptorPool);
  CLEANUP_FN(destroy_pipeline_layout, vkDestroyPipelineLayout);
  CLEANUP_FN(destroy_module, vkDestroyShaderModule);
  CLEANUP_FN(destroy_pipeline, vkDestroyPipeline);
  CLEANUP_FN(destroy_command_pool, vkDestroyCommandPool);
  CLEANUP_FN(destroy_fence, vkDestroyFence);
  LOAD_DEVICE(vkCreateBuffer);
  LOAD_DEVICE(vkGetBufferMemoryRequirements);
  LOAD_DEVICE(vkAllocateMemory);
  LOAD_DEVICE(vkBindBufferMemory);
  LOAD_DEVICE(vkMapMemory);
  LOAD_DEVICE(vkFlushMappedMemoryRanges);
  LOAD_DEVICE(vkInvalidateMappedMemoryRanges);
  LOAD_DEVICE(vkCreateDescriptorSetLayout);
  LOAD_DEVICE(vkCreateDescriptorPool);
  LOAD_DEVICE(vkAllocateDescriptorSets);
  LOAD_DEVICE(vkUpdateDescriptorSets);
  LOAD_DEVICE(vkCreatePipelineLayout);
  LOAD_DEVICE(vkCreateShaderModule);
  LOAD_DEVICE(vkCreateComputePipelines);
  LOAD_DEVICE(vkCreateCommandPool);
  LOAD_DEVICE(vkAllocateCommandBuffers);
  LOAD_DEVICE(vkBeginCommandBuffer);
  LOAD_DEVICE(vkCmdBindPipeline);
  LOAD_DEVICE(vkCmdBindDescriptorSets);
  LOAD_DEVICE(vkCmdDispatch);
  LOAD_DEVICE(vkCmdPipelineBarrier);
  LOAD_DEVICE(vkEndCommandBuffer);
  LOAD_DEVICE(vkCreateFence);
  LOAD_DEVICE(vkGetDeviceQueue);
  LOAD_DEVICE(vkQueueSubmit);
  LOAD_DEVICE(vkWaitForFences);
  VkBufferCreateInfo bci = { .sType = VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,
    .size = WORDS * sizeof(uint32_t), .usage = VK_BUFFER_USAGE_STORAGE_BUFFER_BIT,
    .sharingMode = VK_SHARING_MODE_EXCLUSIVE };
  CHECK(vkCreateBuffer(device, &bci, NULL, &buffer));
  VkMemoryRequirements requirements;
  vkGetBufferMemoryRequirements(device, buffer, &requirements);
  uint32_t memory_type = UINT32_MAX;
  for (uint32_t i = 0; i < memory_properties.memoryTypeCount; i++) {
    VkMemoryPropertyFlags flags = memory_properties.memoryTypes[i].propertyFlags;
    if ((requirements.memoryTypeBits & (1u << i)) && (flags & VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT)) {
      memory_type = i;
      if (flags & VK_MEMORY_PROPERTY_HOST_COHERENT_BIT) break;
    }
  }
  if (memory_type == UINT32_MAX) { fprintf(stderr, "no host-visible buffer memory\n"); goto cleanup; }
  VkMemoryAllocateInfo mai = { .sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
    .allocationSize = requirements.size, .memoryTypeIndex = memory_type };
  CHECK(vkAllocateMemory(device, &mai, NULL, &memory));
  CHECK(vkBindBufferMemory(device, buffer, memory, 0));
  CHECK(vkMapMemory(device, memory, 0, VK_WHOLE_SIZE, 0, &mapped));
  memset(mapped, 0xa5, WORDS * sizeof(uint32_t));
  int coherent = !!(memory_properties.memoryTypes[memory_type].propertyFlags & VK_MEMORY_PROPERTY_HOST_COHERENT_BIT);
  VkMappedMemoryRange range = { .sType = VK_STRUCTURE_TYPE_MAPPED_MEMORY_RANGE,
    .memory = memory, .offset = 0, .size = VK_WHOLE_SIZE };
  if (!coherent) CHECK(vkFlushMappedMemoryRanges(device, 1, &range));
  VkDescriptorSetLayoutBinding binding = { .binding = 0,
    .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, .descriptorCount = 1,
    .stageFlags = VK_SHADER_STAGE_COMPUTE_BIT };
  VkDescriptorSetLayoutCreateInfo slci = { .sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
    .bindingCount = 1, .pBindings = &binding };
  CHECK(vkCreateDescriptorSetLayout(device, &slci, NULL, &set_layout));
  VkDescriptorPoolSize pool_size = { VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, 1 };
  VkDescriptorPoolCreateInfo dpci = { .sType = VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO,
    .maxSets = 1, .poolSizeCount = 1, .pPoolSizes = &pool_size };
  CHECK(vkCreateDescriptorPool(device, &dpci, NULL, &descriptor_pool));
  VkDescriptorSetAllocateInfo dsai = { .sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO,
    .descriptorPool = descriptor_pool, .descriptorSetCount = 1, .pSetLayouts = &set_layout };
  VkDescriptorSet set;
  CHECK(vkAllocateDescriptorSets(device, &dsai, &set));
  VkDescriptorBufferInfo dbi = { buffer, 0, WORDS * sizeof(uint32_t) };
  VkWriteDescriptorSet write = { .sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
    .dstSet = set, .dstBinding = 0, .descriptorCount = 1,
    .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, .pBufferInfo = &dbi };
  vkUpdateDescriptorSets(device, 1, &write, 0, NULL);
  VkPipelineLayoutCreateInfo plci = { .sType = VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
    .setLayoutCount = 1, .pSetLayouts = &set_layout };
  CHECK(vkCreatePipelineLayout(device, &plci, NULL, &pipeline_layout));
  VkShaderModuleCreateInfo smci = { .sType = VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO,
    .codeSize = sizeof(shader), .pCode = shader };
  CHECK(vkCreateShaderModule(device, &smci, NULL, &module));
  VkComputePipelineCreateInfo cpci = { .sType = VK_STRUCTURE_TYPE_COMPUTE_PIPELINE_CREATE_INFO,
    .stage = { .sType = VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
      .stage = VK_SHADER_STAGE_COMPUTE_BIT, .module = module, .pName = "main" },
    .layout = pipeline_layout };
  CHECK(vkCreateComputePipelines(device, VK_NULL_HANDLE, 1, &cpci, NULL, &pipeline));
  VkCommandPoolCreateInfo pci = { .sType = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO,
    .queueFamilyIndex = family };
  CHECK(vkCreateCommandPool(device, &pci, NULL, &command_pool));
  VkCommandBufferAllocateInfo cbai = { .sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,
    .commandPool = command_pool, .level = VK_COMMAND_BUFFER_LEVEL_PRIMARY, .commandBufferCount = 1 };
  VkCommandBuffer command;
  CHECK(vkAllocateCommandBuffers(device, &cbai, &command));
  VkCommandBufferBeginInfo cbbi = { .sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO,
    .flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT };
  CHECK(vkBeginCommandBuffer(command, &cbbi));
  vkCmdBindPipeline(command, VK_PIPELINE_BIND_POINT_COMPUTE, pipeline);
  vkCmdBindDescriptorSets(command, VK_PIPELINE_BIND_POINT_COMPUTE, pipeline_layout, 0, 1, &set, 0, NULL);
  vkCmdDispatch(command, WORDS / 64, 1, 1);
  VkBufferMemoryBarrier barrier = { .sType = VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER,
    .srcAccessMask = VK_ACCESS_SHADER_WRITE_BIT, .dstAccessMask = VK_ACCESS_HOST_READ_BIT,
    .srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED, .dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED,
    .buffer = buffer, .offset = 0, .size = VK_WHOLE_SIZE };
  vkCmdPipelineBarrier(command, VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT, VK_PIPELINE_STAGE_HOST_BIT,
    0, 0, NULL, 1, &barrier, 0, NULL);
  CHECK(vkEndCommandBuffer(command));
  VkFenceCreateInfo fci = { .sType = VK_STRUCTURE_TYPE_FENCE_CREATE_INFO };
  CHECK(vkCreateFence(device, &fci, NULL, &fence));
  VkQueue queue;
  vkGetDeviceQueue(device, family, 0, &queue);
  VkSubmitInfo submit = { .sType = VK_STRUCTURE_TYPE_SUBMIT_INFO,
    .commandBufferCount = 1, .pCommandBuffers = &command };
  CHECK(vkQueueSubmit(queue, 1, &submit, fence));
  submitted = 1;
  CHECK(vkWaitForFences(device, 1, &fence, VK_TRUE, UINT64_C(10000000000)));
  if (!coherent) CHECK(vkInvalidateMappedMemoryRanges(device, 1, &range));
  for (uint32_t i = 0; i < WORDS; i++) {
    uint32_t actual = ((const uint32_t *)mapped)[i];
    if (actual != (i ^ SALT)) {
      fprintf(stderr, "GPU mismatch at %u: %08x != %08x\n", i, actual, i ^ SALT);
      goto cleanup;
    }
  }
  printf("{\n  \"device\": "); json_string(properties.deviceName);
  printf(",\n  \"loader_api\": \"%u.%u.%u\",\n  \"device_api\": \"%u.%u.%u\",\n",
    VK_API_VERSION_MAJOR(api_version), VK_API_VERSION_MINOR(api_version), VK_API_VERSION_PATCH(api_version),
    VK_API_VERSION_MAJOR(properties.apiVersion), VK_API_VERSION_MINOR(properties.apiVersion), VK_API_VERSION_PATCH(properties.apiVersion));
  printf("  \"vendor_id\": %u,\n  \"device_id\": %u,\n  \"max_push_constants\": %u,\n",
    properties.vendorID, properties.deviceID, properties.limits.maxPushConstantsSize);
  printf("  \"features\": {\n");
#define FEATURE(name) printf("    \"%s\": %s,\n", #name, features.features.name ? "true" : "false")
  FEATURE(geometryShader); FEATURE(tessellationShader); FEATURE(shaderCullDistance);
  FEATURE(logicOp); FEATURE(shaderInt16); FEATURE(shaderInt64); FEATURE(shaderFloat64);
  printf("    \"descriptorIndexing\": %s,\n    \"scalarBlockLayout\": %s,\n    \"shaderInt8\": %s\n  },\n",
    v12.descriptorIndexing ? "true" : "false", v12.scalarBlockLayout ? "true" : "false", v12.shaderInt8 ? "true" : "false");
  printf("  \"extensions\": [");
  for (uint32_t i = 0; i < device_extension_count; i++) {
    if (i) printf(", ");
    json_string(device_ext[i].extensionName);
  }
  printf("],\n  \"compute_words_verified\": %u,\n  \"guest_cpu_jit\": false,\n  \"guest_connected\": false\n}\n", WORDS);
  status = 0;
cleanup:
  if (device && submitted && wait_idle) wait_idle(device);
  if (fence && destroy_fence) destroy_fence(device, fence, NULL);
  if (command_pool && destroy_command_pool) destroy_command_pool(device, command_pool, NULL);
  if (pipeline && destroy_pipeline) destroy_pipeline(device, pipeline, NULL);
  if (module && destroy_module) destroy_module(device, module, NULL);
  if (pipeline_layout && destroy_pipeline_layout) destroy_pipeline_layout(device, pipeline_layout, NULL);
  if (descriptor_pool && destroy_descriptor_pool) destroy_descriptor_pool(device, descriptor_pool, NULL);
  if (set_layout && destroy_set_layout) destroy_set_layout(device, set_layout, NULL);
  if (mapped && unmap_memory) unmap_memory(device, memory);
  if (buffer && destroy_buffer) destroy_buffer(device, buffer, NULL);
  if (memory && free_memory) free_memory(device, memory, NULL);
  if (device && destroy_device) destroy_device(device, NULL);
  if (instance && destroy_instance) destroy_instance(instance, NULL);
  if (library) dlclose(library);
  free(queues); free(device_ext); free(instance_ext); free(physical);
  return status;
}
