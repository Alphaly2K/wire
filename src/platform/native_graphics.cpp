// Independently implemented host Vulkan/COM boundary for wire.
// dxbc-spirv is linked separately under its MIT license; no upstream source copied here.
// SPDX-License-Identifier: LGPL-2.1-only
#define VK_NO_PROTOTYPES
#define VK_USE_PLATFORM_METAL_EXT
#include <vulkan/vulkan.h>
#include "native_graphics.h"
#include "native_geometry.h"
#include "dxbc/dxbc_api.h"
#include "spirv/spirv_builder.h"
#include "ir/passes/ir_pass_lower_io.h"
#include "util/util_log.h"
#include <dlfcn.h>
#include <algorithm>
#include <array>
#include <cstring>
#include <cmath>
#include <limits>
#include <map>
#include <memory>
#include <stdexcept>
#include <string>
#include <vector>

static_assert(sizeof(WireGpuInfo) == 312, "host adapter ABI");
static_assert(sizeof(WireBinding) == 16, "host descriptor ABI");
static_assert(sizeof(WireShaderInfo) == 40, "host shader ABI (64-bit host only)");
static_assert(sizeof(WireGeometryIo) == 32, "geometry I/O ABI");
static_assert(sizeof(WireGeometryInfo) == 64, "geometry ABI (64-bit host only)");
static_assert(sizeof(WireBufferBinding) == 16, "buffer binding ABI (64-bit host only)");
static_assert(sizeof(WireDraw)==1040 && offsetof(WireDraw,viewport)==620,"draw ABI");
static_assert(sizeof(WireDrawBinding)==80 && sizeof(WireVertexAttribute)==24,"draw binding ABI");

namespace {
using namespace dxbc_spv;
void error_text(char *out, size_t capacity, const char *text) {
  if (out && capacity) {
    size_t n = std::min(capacity - 1, std::strlen(text));
    std::memcpy(out, text, n); out[n] = 0;
  }
}
void check(VkResult result, const char *operation) {
  if (result != VK_SUCCESS)
    throw std::runtime_error(std::string(operation) + ": VkResult=" + std::to_string(result));
}
template<class T> T symbol(PFN_vkGetInstanceProcAddr get, VkInstance instance, const char *name) {
  auto p = reinterpret_cast<T>(get(instance, name));
  if (!p) throw std::runtime_error(std::string("missing ") + name);
  return p;
}
struct Context {
  void *library = nullptr;
  VkInstance instance = VK_NULL_HANDLE;
  VkPhysicalDevice physical = VK_NULL_HANDLE;
  VkDevice device = VK_NULL_HANDLE;
  VkQueue queue = VK_NULL_HANDLE;
  uint32_t family = 0;
  uint32_t timestamp_bits = 0;
  VkPhysicalDeviceMemoryProperties memory = {};
  VkPhysicalDeviceProperties properties = {};
  VkPhysicalDeviceFloatControlsProperties floats = {
    VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FLOAT_CONTROLS_PROPERTIES, nullptr };
  PFN_vkDestroyInstance destroy_instance = nullptr;
  PFN_vkDestroyDevice destroy_device = nullptr;
  PFN_vkDeviceWaitIdle wait_idle = nullptr;
  PFN_vkGetDeviceProcAddr get_device = nullptr;
  PFN_vkGetInstanceProcAddr get_instance = nullptr;
  bool presentation = false;
  bool maintenance4 = false;
  VkPhysicalDeviceFeatures features = {};
  PFN_vkGetPhysicalDeviceFormatProperties format_properties = nullptr;
  ~Context() {
    if (device && wait_idle) wait_idle(device);
    if (device && destroy_device) destroy_device(device, nullptr);
    if (instance && destroy_instance) destroy_instance(instance, nullptr);
    if (library) dlclose(library);
  }
  template<class T> T fn(const char *name) const {
    auto p = reinterpret_cast<T>(get_device(device, name));
    if (!p) throw std::runtime_error(std::string("missing ") + name);
    return p;
  }
  template<class T> T instance_fn(const char *name) const {
    return symbol<T>(get_instance, instance, name);
  }
};
bool extension(const std::vector<VkExtensionProperties>& extensions, const char *name) {
  for (const auto& e : extensions) if (!std::strcmp(e.extensionName, name)) return true;
  return false;
}
std::unique_ptr<Context> open_gpu(const char *path, WireGpuInfo& info) {
  if (!path) throw std::runtime_error("missing MoltenVK path");
  auto c = std::make_unique<Context>();
  c->library = dlopen(path, RTLD_NOW | RTLD_LOCAL);
  if (!c->library) throw std::runtime_error(dlerror());
  auto get = reinterpret_cast<PFN_vkGetInstanceProcAddr>(dlsym(c->library, "vkGetInstanceProcAddr"));
  if (!get) throw std::runtime_error("missing vkGetInstanceProcAddr");
  c->get_instance = get;
  auto version_fn = symbol<PFN_vkEnumerateInstanceVersion>(get, nullptr, "vkEnumerateInstanceVersion");
  uint32_t version = 0, count = 0;
  check(version_fn(&version), "instance version");
  if (version < VK_API_VERSION_1_3) throw std::runtime_error("SPIR-V 1.6 backend requires Vulkan 1.3");
  auto ie = symbol<PFN_vkEnumerateInstanceExtensionProperties>(get, nullptr, "vkEnumerateInstanceExtensionProperties");
  check(ie(nullptr, &count, nullptr), "instance extensions");
  if (count > 4096) throw std::runtime_error("instance extension capacity");
  std::vector<VkExtensionProperties> extensions(count);
  check(ie(nullptr, &count, extensions.data()), "instance extensions"); extensions.resize(count);
  const char *portability = VK_KHR_PORTABILITY_ENUMERATION_EXTENSION_NAME;
  bool portable = extension(extensions, portability);
  VkApplicationInfo app = { VK_STRUCTURE_TYPE_APPLICATION_INFO, nullptr, "wire native graphics",
    1, "wire", 1, std::min(version, uint32_t(VK_API_VERSION_1_4)) };
  VkInstanceCreateInfo ci = { VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO };
  ci.pApplicationInfo = &app;
  ci.flags = portable ? VK_INSTANCE_CREATE_ENUMERATE_PORTABILITY_BIT_KHR : 0;
  std::vector<const char*> instance_extensions;
  if(portable) instance_extensions.push_back(portability);
  bool surface = extension(extensions,VK_KHR_SURFACE_EXTENSION_NAME) && extension(extensions,VK_EXT_METAL_SURFACE_EXTENSION_NAME);
  if(surface) {instance_extensions.push_back(VK_KHR_SURFACE_EXTENSION_NAME);instance_extensions.push_back(VK_EXT_METAL_SURFACE_EXTENSION_NAME);}
  ci.enabledExtensionCount = uint32_t(instance_extensions.size()); ci.ppEnabledExtensionNames = instance_extensions.data();
  auto create = symbol<PFN_vkCreateInstance>(get, nullptr, "vkCreateInstance");
  check(create(&ci, nullptr, &c->instance), "create instance");
  c->destroy_instance = symbol<PFN_vkDestroyInstance>(get, c->instance, "vkDestroyInstance");
  auto devices = symbol<PFN_vkEnumeratePhysicalDevices>(get, c->instance, "vkEnumeratePhysicalDevices");
  check(devices(c->instance, &count, nullptr), "physical devices");
  if (!count || count > 64) throw std::runtime_error("no usable physical device");
  std::vector<VkPhysicalDevice> physical(count);
  check(devices(c->instance, &count, physical.data()), "physical devices");
  c->physical = physical[0];
  c->format_properties = symbol<PFN_vkGetPhysicalDeviceFormatProperties>(get, c->instance, "vkGetPhysicalDeviceFormatProperties");
  VkPhysicalDeviceIDProperties id = { VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ID_PROPERTIES };
  c->floats.pNext = &id;
  VkPhysicalDeviceProperties2 properties = { VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2, &c->floats };
  symbol<PFN_vkGetPhysicalDeviceProperties2>(get, c->instance, "vkGetPhysicalDeviceProperties2")(c->physical, &properties);
  c->properties = properties.properties; c->floats.pNext = nullptr;
  if (c->properties.apiVersion < VK_API_VERSION_1_3) throw std::runtime_error("device Vulkan < 1.3");
  VkPhysicalDeviceVulkan13Features v13 = { VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_3_FEATURES };
  VkPhysicalDeviceVulkan12Features v12 = { VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES, &v13 };
  VkPhysicalDeviceFeatures2 features = { VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2, &v12 };
  symbol<PFN_vkGetPhysicalDeviceFeatures2>(get, c->instance, "vkGetPhysicalDeviceFeatures2")(c->physical, &features);
  if (!v12.bufferDeviceAddress || !v12.vulkanMemoryModel || !v12.vulkanMemoryModelDeviceScope)
    throw std::runtime_error("shader compiler needs bufferDeviceAddress / Vulkan memory model and device scope");
  auto de = symbol<PFN_vkEnumerateDeviceExtensionProperties>(get, c->instance, "vkEnumerateDeviceExtensionProperties");
  check(de(c->physical, nullptr, &count, nullptr), "device extensions");
  if (count > 4096) throw std::runtime_error("device extension capacity");
  extensions.resize(count);
  check(de(c->physical, nullptr, &count, extensions.data()), "device extensions"); extensions.resize(count);
  const char *subset = "VK_KHR_portability_subset";
  bool has_subset = extension(extensions, subset);
  auto queues = symbol<PFN_vkGetPhysicalDeviceQueueFamilyProperties>(get, c->instance, "vkGetPhysicalDeviceQueueFamilyProperties");
  queues(c->physical, &count, nullptr);
  if (count > 256) throw std::runtime_error("queue family capacity");
  std::vector<VkQueueFamilyProperties> queue_info(count); queues(c->physical, &count, queue_info.data());
  uint32_t family = UINT32_MAX;
  for (uint32_t i = 0; i < count; i++) if (queue_info[i].queueCount &&
      (queue_info[i].queueFlags & (VK_QUEUE_GRAPHICS_BIT | VK_QUEUE_COMPUTE_BIT)) ==
      (VK_QUEUE_GRAPHICS_BIT | VK_QUEUE_COMPUTE_BIT)) { family = i; break; }
  if (family == UINT32_MAX) throw std::runtime_error("no graphics/compute queue");
  c->family = family;
  c->timestamp_bits = queue_info[family].timestampValidBits;
  VkPhysicalDeviceVulkan12Features enabled = { VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES };
  VkPhysicalDeviceVulkan13Features enabled13 = { VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_3_FEATURES };
  enabled.pNext = &enabled13;enabled13.maintenance4 = v13.maintenance4;c->maintenance4 = v13.maintenance4;
  enabled.bufferDeviceAddress = VK_TRUE; enabled.vulkanMemoryModel = VK_TRUE;
  enabled.vulkanMemoryModelDeviceScope = VK_TRUE;
  enabled.shaderOutputViewportIndex = v12.shaderOutputViewportIndex;
  enabled.shaderOutputLayer = v12.shaderOutputLayer;
  VkPhysicalDeviceFeatures core = {};
  core.robustBufferAccess = features.features.robustBufferAccess;
  core.shaderStorageImageWriteWithoutFormat = features.features.shaderStorageImageWriteWithoutFormat;
  core.shaderStorageImageReadWithoutFormat = features.features.shaderStorageImageReadWithoutFormat;
  core.shaderClipDistance = features.features.shaderClipDistance;
  core.shaderInt64 = features.features.shaderInt64;
  core.multiViewport = features.features.multiViewport;
  core.depthClamp = features.features.depthClamp;
  core.depthBiasClamp = features.features.depthBiasClamp;
  core.fillModeNonSolid = features.features.fillModeNonSolid;
  core.samplerAnisotropy = features.features.samplerAnisotropy;
  c->features = core;
  float priority = 1;
  VkDeviceQueueCreateInfo q = { VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO };
  q.queueFamilyIndex = family; q.queueCount = 1; q.pQueuePriorities = &priority;
  VkDeviceCreateInfo d = { VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO, &enabled };
  d.pEnabledFeatures = &core;
  d.queueCreateInfoCount = 1; d.pQueueCreateInfos = &q;
  std::vector<const char*> device_extensions;
  if(has_subset)device_extensions.push_back(subset);
  c->presentation=surface && extension(extensions,VK_KHR_SWAPCHAIN_EXTENSION_NAME);
  if(c->presentation)device_extensions.push_back(VK_KHR_SWAPCHAIN_EXTENSION_NAME);
  d.enabledExtensionCount = uint32_t(device_extensions.size()); d.ppEnabledExtensionNames = device_extensions.data();
  check(symbol<PFN_vkCreateDevice>(get, c->instance, "vkCreateDevice")(c->physical, &d, nullptr, &c->device), "create device");
  c->get_device = symbol<PFN_vkGetDeviceProcAddr>(get, c->instance, "vkGetDeviceProcAddr");
  c->destroy_device = c->fn<PFN_vkDestroyDevice>("vkDestroyDevice");
  c->wait_idle = c->fn<PFN_vkDeviceWaitIdle>("vkDeviceWaitIdle");
  c->fn<PFN_vkGetDeviceQueue>("vkGetDeviceQueue")(c->device, family, 0, &c->queue);
  VkPhysicalDeviceMemoryProperties memory;
  symbol<PFN_vkGetPhysicalDeviceMemoryProperties>(get, c->instance, "vkGetPhysicalDeviceMemoryProperties")(c->physical, &memory);
  c->memory = memory;
  info = {}; std::memcpy(info.name, c->properties.deviceName, sizeof(info.name)); info.name[255] = 0;
  info.vendor = c->properties.vendorID; info.device = c->properties.deviceID; info.api_version = c->properties.apiVersion;
  std::memcpy(info.uuid, id.deviceUUID, 16);
  for (uint32_t i = 0; i < memory.memoryHeapCount; i++) {
    bool host = false;
    for (uint32_t j = 0; j < memory.memoryTypeCount; j++)
      host |= memory.memoryTypes[j].heapIndex == i && (memory.memoryTypes[j].propertyFlags & VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT);
    if (host) info.shared_system += memory.memoryHeaps[i].size;
    else if (memory.memoryHeaps[i].flags & VK_MEMORY_HEAP_DEVICE_LOCAL_BIT) info.dedicated_video += memory.memoryHeaps[i].size;
  }
  return c;
}

struct Shader {
  Context *owner = nullptr;
  std::vector<uint32_t> words;
  std::vector<WireBinding> bindings;
  uint32_t stage = 0, push_bytes = 0;
  bool sampled_rgba2d = true;
  std::map<std::pair<uint32_t,uint32_t>,std::vector<uint32_t>> inputs, outputs;
  wire_geometry::Geometry geometry;
};
VkFormat dxgi_format(uint32_t f) {
  switch (f) {
    case 2: return VK_FORMAT_R32G32B32A32_SFLOAT;
    case 3: return VK_FORMAT_R32G32B32A32_UINT;
    case 4: return VK_FORMAT_R32G32B32A32_SINT;
    case 6: return VK_FORMAT_R32G32B32_SFLOAT;
    case 7:return VK_FORMAT_R32G32B32_UINT;
    case 8:return VK_FORMAT_R32G32B32_SINT;
    case 10: return VK_FORMAT_R16G16B16A16_SFLOAT;
    case 11: return VK_FORMAT_R16G16B16A16_UNORM;
    case 12:return VK_FORMAT_R16G16B16A16_UINT;
    case 13:return VK_FORMAT_R16G16B16A16_SNORM;
    case 14:return VK_FORMAT_R16G16B16A16_SINT;
    case 16: return VK_FORMAT_R32G32_SFLOAT;
    case 17:return VK_FORMAT_R32G32_UINT;
    case 18:return VK_FORMAT_R32G32_SINT;
    case 19:case 20:case 21:case 22:return VK_FORMAT_D32_SFLOAT_S8_UINT;
    case 24: return VK_FORMAT_A2B10G10R10_UNORM_PACK32;
    case 25:return VK_FORMAT_A2B10G10R10_UINT_PACK32;
    case 26: return VK_FORMAT_B10G11R11_UFLOAT_PACK32;
    case 28: return VK_FORMAT_R8G8B8A8_UNORM;
    case 29: return VK_FORMAT_R8G8B8A8_SRGB;
    case 30: return VK_FORMAT_R8G8B8A8_UINT;
    case 31: return VK_FORMAT_R8G8B8A8_SNORM;
    case 32: return VK_FORMAT_R8G8B8A8_SINT;
    case 34: return VK_FORMAT_R16G16_SFLOAT;
    case 35: return VK_FORMAT_R16G16_UNORM;
    case 36:return VK_FORMAT_R16G16_UINT;
    case 37:return VK_FORMAT_R16G16_SNORM;
    case 38:return VK_FORMAT_R16G16_SINT;
    case 40: return VK_FORMAT_D32_SFLOAT;
    case 41: return VK_FORMAT_R32_SFLOAT;
    case 42: return VK_FORMAT_R32_UINT;
    case 43: return VK_FORMAT_R32_SINT;
    case 45: return VK_FORMAT_D24_UNORM_S8_UINT;
    case 49: return VK_FORMAT_R8G8_UNORM;
    case 50:return VK_FORMAT_R8G8_UINT;
    case 51:return VK_FORMAT_R8G8_SNORM;
    case 52:return VK_FORMAT_R8G8_SINT;
    case 54: return VK_FORMAT_R16_SFLOAT;
    case 55: return VK_FORMAT_D16_UNORM;
    case 56: return VK_FORMAT_R16_UNORM;
    case 57: return VK_FORMAT_R16_UINT;
    case 58:return VK_FORMAT_R16_SNORM;
    case 59:return VK_FORMAT_R16_SINT;
    case 61: return VK_FORMAT_R8_UNORM;
    case 62: return VK_FORMAT_R8_UINT;
    case 63: return VK_FORMAT_R8_SNORM;
    case 64: return VK_FORMAT_R8_SINT;
    case 71: return VK_FORMAT_BC1_RGBA_UNORM_BLOCK;
    case 72: return VK_FORMAT_BC1_RGBA_SRGB_BLOCK;
    case 74: return VK_FORMAT_BC2_UNORM_BLOCK;
    case 75: return VK_FORMAT_BC2_SRGB_BLOCK;
    case 77: return VK_FORMAT_BC3_UNORM_BLOCK;
    case 78: return VK_FORMAT_BC3_SRGB_BLOCK;
    case 80: return VK_FORMAT_BC4_UNORM_BLOCK;
    case 83: return VK_FORMAT_BC5_UNORM_BLOCK;
    case 87: return VK_FORMAT_B8G8R8A8_UNORM;
    case 91: return VK_FORMAT_B8G8R8A8_SRGB;
    default: return VK_FORMAT_UNDEFINED;
  }
}
struct Mapping : spirv::ResourceMapping {
  uint32_t set;
  explicit Mapping(uint32_t stage) : set(stage == 1 ? 0 : stage == 2 ? 1 : stage == 4 ? 2 : 3) { }
  spirv::DescriptorBinding mapDescriptor(ir::ScalarType type, uint32_t space, uint32_t reg) override {
    if (space == 1 && set == 3 && type == ir::ScalarType::eUav && reg < 2)
      return {3,166+reg};
    if (space) throw std::runtime_error("SM5.1 register spaces unsupported");
    uint32_t base = 0, limit = 0;
    switch (type) {
      case ir::ScalarType::eCbv: base = 0; limit = 14; break;
      case ir::ScalarType::eSrv: base = 14; limit = 128; break;
      case ir::ScalarType::eSampler: base = 142; limit = 16; break;
      case ir::ScalarType::eUav: base = 158; limit = 8; break;
      default: throw std::runtime_error("unsupported descriptor kind");
    }
    if (reg >= limit) throw std::runtime_error("D3D11 register capacity");
    return {set, base + reg};
  }
  uint32_t mapPushData(ir::ShaderStageMask stages) override {
    if (stages != stages.first()) return 0;
    return stages == ir::ShaderStage::eVertex ? 64 : stages == ir::ShaderStage::ePixel ? 128 : 192;
  }
};
struct LoggerCapture : util::Logger {
  std::string messages;
  LogLevel getMinimumSeverity() override { return LogLevel::eWarn; }
  void message(LogLevel, const char *m) override { if (messages.size() < 4096) { messages += m; messages += '\n'; } }
};
uint32_t u32(const uint8_t *data) { uint32_t v; std::memcpy(&v, data, 4); return v; }
void validate_dxbc(const uint8_t *data, size_t size) {
  if (!data || size < 32 || size > 1024 * 1024 || std::memcmp(data, "DXBC", 4) ||
      u32(data + 20) != 1 || u32(data + 24) != size) throw std::runtime_error("invalid DXBC header/size");
  uint32_t count = u32(data + 28);
  if (!count || count > 128 || size < 32 + count * 4) throw std::runtime_error("invalid DXBC chunk count");
  std::vector<std::pair<uint32_t, uint32_t>> spans;
  uint32_t code_count = 0;
  for (uint32_t i = 0; i < count; i++) {
    uint32_t offset = u32(data + 32 + i * 4);
    if (offset < 32 + count * 4 || offset > size - 8) throw std::runtime_error("DXBC chunk bounds");
    uint32_t length = u32(data + offset + 4);
    if (length > size - offset - 8) throw std::runtime_error("DXBC chunk length");
    uint32_t end = offset + 8 + length;
    for (auto span : spans) if (offset < span.second && end > span.first) throw std::runtime_error("overlapping DXBC chunks");
    spans.emplace_back(offset, end);
    if (!std::memcmp(data + offset, "SHDR", 4) || !std::memcmp(data + offset, "SHEX", 4)) {
      if (length < 8 || length % 4 || u32(data + offset + 12) != length / 4) throw std::runtime_error("DXBC shader length");
      code_count++;
    }
  }
  if (code_count != 1) throw std::runtime_error("DXBC needs one shader chunk");
}

// Reflect the generated module so descriptor layouts match actual emitted
// types, not a guessed D3D resource kind. Restrict arrays to fixed-size ones.
void reflect(Shader& s) {
  auto& w = s.words;
  if (w.size() < 5 || w[0] != 0x07230203 || w[3] > 1000000) throw std::runtime_error("SPIR-V capacity");
  std::map<uint32_t, std::vector<uint32_t>> types;
  std::map<uint32_t, uint32_t> constants, sets, bindings, locations, components;
  std::vector<std::array<uint32_t, 3>> variables;
  for (size_t i = 5; i < w.size();) {
    uint32_t n = w[i] >> 16, op = w[i] & 65535;
    if (!n || n > w.size() - i) throw std::runtime_error("SPIR-V instruction bounds");
    if ((op >= 19 && op <= 33) && n >= 2) types[w[i+1]] = std::vector<uint32_t>(w.begin()+i, w.begin()+i+n);
    if (op == 43 && n == 4) constants[w[i+2]] = w[i+3];
    if (op == 71 && n == 4 && w[i+2] == 34) sets[w[i+1]] = w[i+3];
    if (op == 71 && n == 4 && w[i+2] == 33) bindings[w[i+1]] = w[i+3];
    if (op == 71 && n == 4 && w[i+2] == 30) locations[w[i+1]] = w[i+3];
    if (op == 71 && n == 4 && w[i+2] == 31) components[w[i+1]] = w[i+3];
    if (op == 59 && n >= 4) variables.push_back({w[i+1], w[i+2], w[i+3]});
    i += n;
  }
  for (auto v : variables) {
    if (v[2] == 9) s.push_bytes = 256; // conservative reserved push-data ABI
    if ((v[2] == 1 || v[2] == 3) && locations.count(v[1])) {
      auto pointer = types.at(v[0]);
      if (pointer.size() != 4 || (pointer[0]&65535) != 32) throw std::runtime_error("shader I/O pointer");
      auto type = types.at(pointer[3]);
      // Preserve vector/scalar shape and underlying scalar kind for Metal
      // stage linking. Arbitrary array/matrix I/O is not supported yet.
      std::vector<uint32_t> signature;
      if ((type[0]&65535) == 23 && type.size() == 4) {
        auto scalar = types.at(type[2]); signature.assign(scalar.begin()+2,scalar.end());
        signature.insert(signature.begin(), scalar[0]&65535); signature.push_back(type[3]);
      } else if (((type[0]&65535) == 21 || (type[0]&65535) == 22) && type.size() >= 3) {
        signature.assign(type.begin()+2,type.end()); signature.insert(signature.begin(),type[0]&65535); signature.push_back(1);
      } else throw std::runtime_error("unsupported shader I/O type");
      auto key = std::make_pair(locations.at(v[1]),components.count(v[1]) ? components.at(v[1]) : 0);
      (v[2] == 1 ? s.inputs : s.outputs)[key] = signature;
    }
    if (!sets.count(v[1]) && !bindings.count(v[1])) continue;
    if (!sets.count(v[1]) || !bindings.count(v[1])) throw std::runtime_error("incomplete descriptor decoration");
    auto type = types.at(v[0]);
    if ((type[0] & 65535) != 32 || type.size() != 4) throw std::runtime_error("descriptor pointer type");
    type = types.at(type[3]);
    uint32_t count = 1;
    if ((type[0] & 65535) == 28) {
      if (type.size() != 4 || !constants.count(type[3])) throw std::runtime_error("descriptor array type");
      count = constants.at(type[3]); type = types.at(type[2]);
    }
    if (!count || count > 128) throw std::runtime_error("descriptor array capacity");
    uint32_t kind = UINT32_MAX, op = type[0] & 65535;
    if (op == 26 && v[2] == 0) kind = VK_DESCRIPTOR_TYPE_SAMPLER;
    else if (op == 25 && type.size() >= 9 && v[2] == 0) {
      bool buffer = type[3] == 5;
      if (type[7] == 1) kind = buffer ? VK_DESCRIPTOR_TYPE_UNIFORM_TEXEL_BUFFER : VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE;
      if (type[7] == 2) kind = buffer ? VK_DESCRIPTOR_TYPE_STORAGE_TEXEL_BUFFER : VK_DESCRIPTOR_TYPE_STORAGE_IMAGE;
      if (type[7] == 1) {
        const auto& scalar = types.at(type[2]);
        s.sampled_rgba2d &= type[3] == 1 && type[4] == 0 && type[5] == 0 && type[6] == 0 &&
          scalar.size() == 3 && (scalar[0]&65535) == 22 && scalar[2] == 32;
      }
    } else if (op == 30 && v[2] == 2) kind = VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER;
    else if (op == 30 && v[2] == 12) kind = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER;
    if (kind == UINT32_MAX) throw std::runtime_error("unsupported SPIR-V descriptor type");
    s.bindings.push_back({sets.at(v[1]), bindings.at(v[1]), kind, count});
  }
}
void literal_workgroup_size(std::vector<uint32_t>& w) {
  // DXBC dcl_thread_group is static. Represent it both as LocalSize and as
  // the equivalent WorkgroupSize constant: MoltenVK's zero-initialize-LDS
  // loop needs the latter even when the source shader does not load it.
  // Never fold specialization constants or remove LDS initialization.
  std::map<uint32_t,std::pair<uint32_t,uint32_t>> constants;
  std::array<uint32_t,3> dimensions = {};
  uint32_t scalar = 0, vector = 0;
  bool mode = false;
  for (size_t i = 5; i < w.size();) {
    uint32_t n = w[i]>>16, op = w[i]&65535;
    if (!n || n > w.size()-i) throw std::runtime_error("SPIR-V bounds");
    if (op == 43 && n == 4) constants[w[i+2]] = {w[i+1],w[i+3]};
    if (op == 71 && n == 4 && w[i+2] == 11 && w[i+3] == 25)
      throw std::runtime_error("unexpected existing WorkgroupSize builtin");
    i += n;
  }
  for (size_t i = 5; i < w.size();) {
    uint32_t n = w[i]>>16, op = w[i]&65535;
    if (op == 331 && n == 6 && w[i+2] == 38) {
      if (mode) throw std::runtime_error("multiple workgroup dimensions");
      mode = true;
      for (size_t j = 3; j < 6; j++) {
        auto found = constants.find(w[i+j]);
        if (found == constants.end() || !found->second.second)
          throw std::runtime_error("dynamic workgroup size unsupported");
        if (scalar && scalar != found->second.first) throw std::runtime_error("workgroup dimension type");
        scalar = found->second.first; dimensions[j-3] = w[i+j];
        w[i+j] = found->second.second;
      }
      w[i] = (6u<<16)|16; w[i+2] = 17;
    }
    i += n;
  }
  if (!mode) return;
  for (size_t i = 5; i < w.size(); i += w[i]>>16) {
    if ((w[i]&65535) == 21 && w[i+1] == scalar && (w[i]>>16 != 4 || w[i+2] != 32 || w[i+3] != 0))
      throw std::runtime_error("workgroup dimension must be U32");
    if ((w[i]&65535) == 23 && w[i]>>16 == 4 && w[i+2] == scalar && w[i+3] == 3) vector = w[i+1];
  }
  uint32_t value = w[3]++;
  bool new_vector = !vector;
  if (new_vector) vector = w[3]++;
  std::vector<uint32_t> out(w.begin(),w.begin()+5);
  bool annotated = false, declared = false;
  for (size_t i = 5; i < w.size(); i += w[i]>>16) {
    uint32_t op = w[i]&65535;
    if (!annotated && op >= 19 && op <= 39) {
      out.insert(out.end(),{(4u<<16)|71,value,11,25}); annotated = true;
    }
    if (!declared && op == 54) {
      // A newly allocated vector id has no type declaration in the input.
      if (new_vector)
        out.insert(out.end(),{(4u<<16)|23,vector,scalar,3});
      out.insert(out.end(),{(6u<<16)|44,vector,value,dimensions[0],dimensions[1],dimensions[2]});
      declared = true;
    }
    out.insert(out.end(),w.begin()+i,w.begin()+i+(w[i]>>16));
  }
  if (!annotated || !declared) throw std::runtime_error("workgroup module sections");
  w = std::move(out);
}
std::unique_ptr<Shader> compile(Context& c, const uint8_t *data, size_t size, uint32_t stage) {
  validate_dxbc(data, size);
  if (stage != 1 && stage != 2 && stage != 4 && stage != 8) throw std::runtime_error("only VS / PS / CS / emulated GS supported");
  LoggerCapture logger;
  dxbc::Converter::Options options;
  ir::CompileOptions legalization;
  legalization.min16Options.enableFloat16 = false; legalization.min16Options.enableInt16 = false;
  auto builder = dxbc::compileShaderToLegalizedIr(data, size, options, legalization);
  if (!builder) throw std::runtime_error("DXBC conversion failed: " + logger.messages);
  uint32_t expected = stage == 1 ? uint32_t(ir::ShaderStage::eVertex) : stage == 2 ? uint32_t(ir::ShaderStage::ePixel) : stage == 4 ? uint32_t(ir::ShaderStage::eCompute) : uint32_t(ir::ShaderStage::eGeometry);
  uint32_t entries = 0;
  ir::SsaDef entry;
  auto declarations = builder->getDeclarations();
  for (auto it = declarations.first; it != declarations.second; ++it) {
    const auto& op = *it;
    if (op.getOpCode() != ir::OpCode::eEntryPoint) continue;
    if (uint32_t(op.getOperand(op.getFirstLiteralOperandIndex())) != expected) throw std::runtime_error("shader stage mismatch");
    entries++;
    entry = op.getDef();
  }
  if (entries != 1) throw std::runtime_error("shader entry point count");
  wire_geometry::Geometry geometry;
  if (stage == 8) geometry = wire_geometry::lower(*builder,entry);
  ir::LowerIoPass io(*builder); io.lowerSampleCountToSpecConstant(0);
  Mapping mapping(stage);
  spirv::SpirvBuilder::Options spv;
  spv.includeDebugNames = false;
  spv.maxCbvSize = c.properties.limits.maxUniformBufferRange;
  spv.maxCbvCount = int32_t(std::min(14u,c.properties.limits.maxPerStageDescriptorUniformBuffers));
  if (c.floats.shaderRoundingModeRTEFloat32) spv.supportedRoundModesF32 |= ir::RoundMode::eNearestEven;
  if (c.floats.shaderRoundingModeRTZFloat32) spv.supportedRoundModesF32 |= ir::RoundMode::eZero;
  if (c.floats.shaderDenormPreserveFloat32) spv.supportedDenormModesF32 |= ir::DenormMode::ePreserve;
  if (c.floats.shaderDenormFlushToZeroFloat32) spv.supportedDenormModesF32 |= ir::DenormMode::eFlush;
  spv.supportsZeroInfNanPreserveF32 = c.floats.shaderSignedZeroInfNanPreserveFloat32;
  spirv::SpirvBuilder lower(*builder, mapping, spv); lower.buildSpirvBinary();
  auto result = std::make_unique<Shader>(); result->owner = &c; result->stage = stage; result->words = lower.getSpirvBinary();
  result->geometry = std::move(geometry);
  if (result->words.size() > 4 * 1024 * 1024) throw std::runtime_error("SPIR-V output capacity");
  literal_workgroup_size(result->words); reflect(*result);
  return result;
}

struct PipelineScope {
  Context& c;
  std::vector<VkShaderModule> modules;
  std::array<VkDescriptorSetLayout, 4> sets = {};
  VkPipelineLayout layout = VK_NULL_HANDLE;
  VkRenderPass pass = VK_NULL_HANDLE;
  VkPipeline pipeline = VK_NULL_HANDLE;
  PFN_vkDestroyPipeline destroy_pipeline;
  PFN_vkDestroyRenderPass destroy_pass;
  PFN_vkDestroyPipelineLayout destroy_layout;
  PFN_vkDestroyDescriptorSetLayout destroy_set;
  PFN_vkDestroyShaderModule destroy_module;
  explicit PipelineScope(Context& context) : c(context),
    destroy_pipeline(c.fn<PFN_vkDestroyPipeline>("vkDestroyPipeline")),
    destroy_pass(c.fn<PFN_vkDestroyRenderPass>("vkDestroyRenderPass")),
    destroy_layout(c.fn<PFN_vkDestroyPipelineLayout>("vkDestroyPipelineLayout")),
    destroy_set(c.fn<PFN_vkDestroyDescriptorSetLayout>("vkDestroyDescriptorSetLayout")),
    destroy_module(c.fn<PFN_vkDestroyShaderModule>("vkDestroyShaderModule")) { }
  ~PipelineScope() {
    if (pipeline) destroy_pipeline(c.device, pipeline, nullptr);
    if (pass) destroy_pass(c.device, pass, nullptr);
    if (layout) destroy_layout(c.device, layout, nullptr);
    for (auto set : sets) if (set) destroy_set(c.device, set, nullptr);
    for (auto module : modules) destroy_module(c.device, module, nullptr);
  }
};
VkShaderStageFlagBits vk_stage(uint32_t stage) {
  return stage == 1 ? VK_SHADER_STAGE_VERTEX_BIT : stage == 2 ? VK_SHADER_STAGE_FRAGMENT_BIT : VK_SHADER_STAGE_COMPUTE_BIT;
}
struct GraphicsConfig {
  VkRenderPass pass;
  std::vector<VkVertexInputBindingDescription> bindings;
  VkViewport viewport;
  VkRect2D scissor;
  VkPipelineRasterizationStateCreateInfo raster;
  VkPipelineDepthStencilStateCreateInfo depth;
  VkPipelineColorBlendAttachmentState blend;
  std::array<float,4> blend_factor;
  uint32_t sample_mask;
  bool alpha_to_coverage;
};
void build_pipeline(Context& c, PipelineScope& p, Shader *vertex, Shader *pixel, Shader *compute,
                    const std::vector<VkVertexInputAttributeDescription> *vertex_attributes = nullptr,
                    uint32_t vertex_stride = 48, VkPrimitiveTopology topology = VK_PRIMITIVE_TOPOLOGY_TRIANGLE_LIST,
                    const GraphicsConfig *graphics = nullptr) {
  for (auto s : {vertex, pixel, compute})
    if (s && s->owner != &c) throw std::runtime_error("shader belongs to another GPU");
  if ((compute && (vertex || pixel)) || (!compute && (!vertex || !pixel))) throw std::runtime_error("invalid pipeline stage combination");
  if ((vertex && vertex->stage != 1) || (pixel && pixel->stage != 2) || (compute && compute->stage != 4 && compute->stage != 8)) throw std::runtime_error("pipeline stage mismatch");
  if (pixel) for (const auto& input : pixel->inputs) {
    auto found = vertex->outputs.find(input.first);
    bool compatible = found != vertex->outputs.end() && (found->second == input.second ||
      (c.maintenance4 && found->second.size() == input.second.size() && !input.second.empty() &&
       std::equal(input.second.begin(),input.second.end()-1,found->second.begin()) &&
       found->second.back() >= input.second.back()));
    if (!compatible) {
      auto shape=[](const std::vector<uint32_t>& values) {
        std::string text;for(auto value:values)text+=(text.empty()?"":"/")+std::to_string(value);return text;
      };
      throw std::runtime_error("shader I/O location mismatch at "+std::to_string(input.first.first)+
        ":"+std::to_string(input.first.second)+" vertex="+(found==vertex->outputs.end()?"missing":shape(found->second))+
        " pixel="+shape(input.second));
    }
  }
  std::array<std::map<uint32_t,VkDescriptorSetLayoutBinding>,4> bindings;
  uint32_t push_bytes = 0; VkShaderStageFlags push_stages = 0;
  std::vector<VkPipelineShaderStageCreateInfo> stages;
  for (Shader *s : {vertex, pixel, compute}) {
    if (!s) continue;
    for (auto b : s->bindings) {
      if (b.set >= 4) throw std::runtime_error("descriptor set capacity");
      if (bindings[b.set].count(b.binding)) throw std::runtime_error("duplicate descriptor binding");
      bindings[b.set][b.binding] = {b.binding, VkDescriptorType(b.descriptor_type), b.count, vk_stage(s->stage), nullptr};
    }
    push_bytes = std::max(push_bytes, s->push_bytes);
    if (s->push_bytes) push_stages |= vk_stage(s->stage);
    VkShaderModuleCreateInfo sm = { VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO };
    sm.codeSize = s->words.size()*4; sm.pCode = s->words.data();
    VkShaderModule module;
    check(c.fn<PFN_vkCreateShaderModule>("vkCreateShaderModule")(c.device, &sm, nullptr, &module), "shader module");
    p.modules.push_back(module);
    VkPipelineShaderStageCreateInfo si = { VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO };
    si.stage = vk_stage(s->stage); si.module = module; si.pName = "main"; stages.push_back(si);
  }
  if (push_bytes > c.properties.limits.maxPushConstantsSize) throw std::runtime_error("push constant capacity");
  for (uint32_t set = 0; set < 4; set++) {
    std::vector<VkDescriptorSetLayoutBinding> entries;
    for (auto b : bindings[set]) entries.push_back(b.second);
    VkDescriptorSetLayoutCreateInfo ci = { VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO };
    ci.bindingCount = uint32_t(entries.size()); ci.pBindings = entries.data();
    check(c.fn<PFN_vkCreateDescriptorSetLayout>("vkCreateDescriptorSetLayout")(c.device, &ci, nullptr, &p.sets[set]), "descriptor layout");
  }
  VkPushConstantRange push = { push_stages, 0, push_bytes };
  VkPipelineLayoutCreateInfo layout = { VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO };
  layout.setLayoutCount = 4; layout.pSetLayouts = p.sets.data();
  layout.pushConstantRangeCount = push_bytes ? 1 : 0; layout.pPushConstantRanges = push_bytes ? &push : nullptr;
  check(c.fn<PFN_vkCreatePipelineLayout>("vkCreatePipelineLayout")(c.device, &layout, nullptr, &p.layout), "pipeline layout");
  if (compute) {
    VkComputePipelineCreateInfo ci = { VK_STRUCTURE_TYPE_COMPUTE_PIPELINE_CREATE_INFO };
    ci.stage = stages[0]; ci.layout = p.layout;
    check(c.fn<PFN_vkCreateComputePipelines>("vkCreateComputePipelines")(c.device, VK_NULL_HANDLE, 1, &ci, nullptr, &p.pipeline), "compute pipeline");
    return;
  }
  VkAttachmentDescription attachment = {};
  attachment.format = VK_FORMAT_R8G8B8A8_UNORM; attachment.samples = VK_SAMPLE_COUNT_1_BIT;
  attachment.loadOp = VK_ATTACHMENT_LOAD_OP_CLEAR; attachment.storeOp = VK_ATTACHMENT_STORE_OP_STORE;
  attachment.initialLayout = VK_IMAGE_LAYOUT_UNDEFINED; attachment.finalLayout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL;
  VkAttachmentReference color = { 0, VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL };
  VkSubpassDescription subpass = {}; subpass.pipelineBindPoint = VK_PIPELINE_BIND_POINT_GRAPHICS;
  subpass.colorAttachmentCount = 1; subpass.pColorAttachments = &color;
  VkRenderPassCreateInfo rp = { VK_STRUCTURE_TYPE_RENDER_PASS_CREATE_INFO };
  rp.attachmentCount = 1; rp.pAttachments = &attachment; rp.subpassCount = 1; rp.pSubpasses = &subpass;
  if(graphics)p.pass=graphics->pass;
  else check(c.fn<PFN_vkCreateRenderPass>("vkCreateRenderPass")(c.device, &rp, nullptr, &p.pass), "render pass");
  // Pipeline compilation only. Dummy vertex layout covers the sample's input
  // locations, and no draw/vertex read is executed by this operation.
  std::array<VkVertexInputAttributeDescription, 3> attributes;
  for (uint32_t i = 0; i < 3; i++) attributes[i] = {i, 0, VK_FORMAT_R32G32B32A32_SFLOAT, i*16};
  VkVertexInputBindingDescription binding = {0,vertex_stride,VK_VERTEX_INPUT_RATE_VERTEX};
  VkPipelineVertexInputStateCreateInfo vi = { VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO };
  vi.vertexBindingDescriptionCount = 1; vi.pVertexBindingDescriptions = &binding;
  vi.vertexAttributeDescriptionCount = vertex_attributes ? uint32_t(vertex_attributes->size()) : 3;
  vi.pVertexAttributeDescriptions = vertex_attributes ? vertex_attributes->data() : attributes.data();
  if(graphics) {vi.vertexBindingDescriptionCount=uint32_t(graphics->bindings.size());vi.pVertexBindingDescriptions=graphics->bindings.data();}
  VkPipelineInputAssemblyStateCreateInfo ia = { VK_STRUCTURE_TYPE_PIPELINE_INPUT_ASSEMBLY_STATE_CREATE_INFO };
  ia.topology = topology;
  ia.primitiveRestartEnable=graphics && (topology==VK_PRIMITIVE_TOPOLOGY_LINE_STRIP || topology==VK_PRIMITIVE_TOPOLOGY_TRIANGLE_STRIP);
  VkViewport viewport = {0,0,64,64,0,1}; VkRect2D scissor = {{0,0},{64,64}};
  VkPipelineViewportStateCreateInfo vs = { VK_STRUCTURE_TYPE_PIPELINE_VIEWPORT_STATE_CREATE_INFO };
  vs.viewportCount = 1; vs.pViewports = &viewport; vs.scissorCount = 1; vs.pScissors = &scissor;
  if(graphics) {vs.pViewports=&graphics->viewport;vs.pScissors=&graphics->scissor;}
  VkPipelineRasterizationStateCreateInfo rs = { VK_STRUCTURE_TYPE_PIPELINE_RASTERIZATION_STATE_CREATE_INFO };
  rs.polygonMode = VK_POLYGON_MODE_FILL; rs.cullMode = VK_CULL_MODE_NONE; rs.lineWidth = 1;
  VkPipelineMultisampleStateCreateInfo ms = { VK_STRUCTURE_TYPE_PIPELINE_MULTISAMPLE_STATE_CREATE_INFO };
  ms.rasterizationSamples = VK_SAMPLE_COUNT_1_BIT;
  if(graphics) {ms.pSampleMask=&graphics->sample_mask;ms.alphaToCoverageEnable=graphics->alpha_to_coverage;}
  VkPipelineColorBlendAttachmentState ba = {}; ba.colorWriteMask = 15;
  VkPipelineColorBlendStateCreateInfo bs = { VK_STRUCTURE_TYPE_PIPELINE_COLOR_BLEND_STATE_CREATE_INFO };
  bs.attachmentCount = 1; bs.pAttachments = &ba;
  if(graphics) {bs.pAttachments=&graphics->blend;std::copy(graphics->blend_factor.begin(),graphics->blend_factor.end(),bs.blendConstants);}
  VkGraphicsPipelineCreateInfo gi = { VK_STRUCTURE_TYPE_GRAPHICS_PIPELINE_CREATE_INFO };
  gi.stageCount = uint32_t(stages.size()); gi.pStages = stages.data(); gi.pVertexInputState = &vi;
  gi.pInputAssemblyState = &ia; gi.pViewportState = &vs; gi.pRasterizationState = &rs;
  gi.pMultisampleState = &ms; gi.pColorBlendState = &bs; gi.layout = p.layout; gi.renderPass = p.pass;
  if(graphics) {gi.pRasterizationState=&graphics->raster;gi.pDepthStencilState=&graphics->depth;}
  check(c.fn<PFN_vkCreateGraphicsPipelines>("vkCreateGraphicsPipelines")(c.device, VK_NULL_HANDLE, 1, &gi, nullptr, &p.pipeline), "graphics pipeline");
}
// Bounded offscreen exercise of the same translated shaders, descriptor ABI
// and VkPipeline as the runtime backend. This submits real GPU work, but is
// not a guest D3D11 device or a window swapchain.
struct BlitScope {
  Context& c;
  VkBuffer buffer = VK_NULL_HANDLE;
  VkDeviceMemory buffer_memory = VK_NULL_HANDLE;
  std::array<VkImage,2> images = {};
  std::array<VkDeviceMemory,2> image_memory = {};
  std::array<VkImageView,2> views = {};
  VkSampler sampler = VK_NULL_HANDLE;
  VkFramebuffer framebuffer = VK_NULL_HANDLE;
  VkDescriptorPool descriptors = VK_NULL_HANDLE;
  VkCommandPool commands = VK_NULL_HANDLE;
  VkFence fence = VK_NULL_HANDLE;
  void *mapped = nullptr;
  bool pending = false;
  PFN_vkDestroyBuffer destroy_buffer;
  PFN_vkFreeMemory free_memory;
  PFN_vkDestroyImage destroy_image;
  PFN_vkDestroyImageView destroy_view;
  PFN_vkDestroySampler destroy_sampler;
  PFN_vkDestroyFramebuffer destroy_framebuffer;
  PFN_vkDestroyDescriptorPool destroy_descriptors;
  PFN_vkDestroyCommandPool destroy_commands;
  PFN_vkDestroyFence destroy_fence;
  PFN_vkUnmapMemory unmap;
  explicit BlitScope(Context& context) : c(context),
    destroy_buffer(c.fn<PFN_vkDestroyBuffer>("vkDestroyBuffer")),
    free_memory(c.fn<PFN_vkFreeMemory>("vkFreeMemory")),
    destroy_image(c.fn<PFN_vkDestroyImage>("vkDestroyImage")),
    destroy_view(c.fn<PFN_vkDestroyImageView>("vkDestroyImageView")),
    destroy_sampler(c.fn<PFN_vkDestroySampler>("vkDestroySampler")),
    destroy_framebuffer(c.fn<PFN_vkDestroyFramebuffer>("vkDestroyFramebuffer")),
    destroy_descriptors(c.fn<PFN_vkDestroyDescriptorPool>("vkDestroyDescriptorPool")),
    destroy_commands(c.fn<PFN_vkDestroyCommandPool>("vkDestroyCommandPool")),
    destroy_fence(c.fn<PFN_vkDestroyFence>("vkDestroyFence")),
    unmap(c.fn<PFN_vkUnmapMemory>("vkUnmapMemory")) { }
  ~BlitScope() {
    // Even a fence timeout must not free resources referenced by submitted work.
    if (pending) c.wait_idle(c.device);
    if (mapped) unmap(c.device,buffer_memory);
    if (fence) destroy_fence(c.device,fence,nullptr);
    if (commands) destroy_commands(c.device,commands,nullptr);
    if (descriptors) destroy_descriptors(c.device,descriptors,nullptr);
    if (framebuffer) destroy_framebuffer(c.device,framebuffer,nullptr);
    if (sampler) destroy_sampler(c.device,sampler,nullptr);
    for (auto v : views) if (v) destroy_view(c.device,v,nullptr);
    for (auto image : images) if (image) destroy_image(c.device,image,nullptr);
    for (auto memory : image_memory) if (memory) free_memory(c.device,memory,nullptr);
    if (buffer) destroy_buffer(c.device,buffer,nullptr);
    if (buffer_memory) free_memory(c.device,buffer_memory,nullptr);
  }
};
uint32_t memory_type(Context& c, uint32_t bits, VkMemoryPropertyFlags flags) {
  for (uint32_t i = 0; i < c.memory.memoryTypeCount; i++)
    if ((bits & (1u<<i)) && (c.memory.memoryTypes[i].propertyFlags & flags) == flags) return i;
  throw std::runtime_error("required Vulkan memory type unavailable");
}
VkDeviceMemory allocate(Context& c, const VkMemoryRequirements& req, VkMemoryPropertyFlags flags) {
  VkMemoryAllocateInfo info = { VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO };
  info.allocationSize = req.size; info.memoryTypeIndex = memory_type(c,req.memoryTypeBits,flags);
  VkDeviceMemory memory = VK_NULL_HANDLE;
  check(c.fn<PFN_vkAllocateMemory>("vkAllocateMemory")(c.device,&info,nullptr,&memory),"allocate GPU memory");
  return memory;
}
struct Buffer {
  Context& c;
  VkBuffer handle = VK_NULL_HANDLE;
  VkDeviceMemory memory = VK_NULL_HANDLE;
  void *mapped = nullptr;
  size_t size;
  uint32_t usage;
  PFN_vkDestroyBuffer destroy;
  PFN_vkFreeMemory free_memory;
  PFN_vkUnmapMemory unmap;
  Buffer(Context& context, size_t bytes, uint32_t flags) : c(context), size(bytes), usage(flags),
    destroy(c.fn<PFN_vkDestroyBuffer>("vkDestroyBuffer")),
    free_memory(c.fn<PFN_vkFreeMemory>("vkFreeMemory")),
    unmap(c.fn<PFN_vkUnmapMemory>("vkUnmapMemory")) { }
  ~Buffer() {
    if (mapped) unmap(c.device,memory);
    if (handle) destroy(c.device,handle,nullptr);
    if (memory) free_memory(c.device,memory,nullptr);
  }
};
std::unique_ptr<Buffer> create_buffer(Context& c, const uint8_t *data, size_t size, uint32_t usage) {
  if (!size || size > 64*1024*1024 || !usage || (usage&~15u) ||
      ((usage&1) && (size%16 || size > c.properties.limits.maxUniformBufferRange)) ||
      ((usage&2) && size > c.properties.limits.maxStorageBufferRange))
    throw std::runtime_error("buffer size/usage limits");
  auto b = std::make_unique<Buffer>(c,size,usage);
  VkBufferCreateInfo bi = {VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO}; bi.size = size;
  bi.usage = VK_BUFFER_USAGE_TRANSFER_SRC_BIT|VK_BUFFER_USAGE_TRANSFER_DST_BIT;
  if (usage&1) bi.usage |= VK_BUFFER_USAGE_UNIFORM_BUFFER_BIT;
  if (usage&2) bi.usage |= VK_BUFFER_USAGE_STORAGE_BUFFER_BIT|VK_BUFFER_USAGE_UNIFORM_TEXEL_BUFFER_BIT;
  if (usage&4) bi.usage |= VK_BUFFER_USAGE_VERTEX_BUFFER_BIT;
  if (usage&8) bi.usage |= VK_BUFFER_USAGE_INDEX_BUFFER_BIT;
  check(c.fn<PFN_vkCreateBuffer>("vkCreateBuffer")(c.device,&bi,nullptr,&b->handle),"create owned buffer");
  VkMemoryRequirements req;
  c.fn<PFN_vkGetBufferMemoryRequirements>("vkGetBufferMemoryRequirements")(c.device,b->handle,&req);
  b->memory = allocate(c,req,VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT|VK_MEMORY_PROPERTY_HOST_COHERENT_BIT);
  check(c.fn<PFN_vkBindBufferMemory>("vkBindBufferMemory")(c.device,b->handle,b->memory,0),"bind owned buffer");
  check(c.fn<PFN_vkMapMemory>("vkMapMemory")(c.device,b->memory,0,VK_WHOLE_SIZE,0,&b->mapped),"map owned buffer");
  if (data) std::memcpy(b->mapped,data,size); else std::memset(b->mapped,0,size);
  return b;
}
#include "native_resources.h"
#include "native_present.h"
#include "native_draw.h"
void blit_rgba64(Context& c, Shader *vertex, Shader *pixel, const uint8_t *source, size_t size,
                 uint8_t *output, size_t capacity) {
  constexpr uint32_t side = 64;
  constexpr size_t bytes = side*side*4, vertex_offset = bytes, read_offset = bytes+256;
  if (!source || !output || size != bytes || capacity < bytes) throw std::runtime_error("RGBA64 buffer size");
  if (!vertex || !pixel) throw std::runtime_error("RGBA64 needs VS/PS");
  if (!pixel->sampled_rgba2d || pixel->outputs.size() != 1 ||
      pixel->outputs.at(std::make_pair(0u,0u)) != std::vector<uint32_t>({22,32,4}) ||
      !vertex->bindings.empty() || vertex->push_bytes || pixel->push_bytes || pixel->bindings.size() != 2 ||
      vertex->inputs.size() != 2 || vertex->inputs.at(std::make_pair(0u,0u)) != std::vector<uint32_t>({22,32,4}) ||
      vertex->inputs.at(std::make_pair(1u,0u)) != std::vector<uint32_t>({22,32,2}))
    throw std::runtime_error("RGBA64 shader interface unsupported");
  bool image = false, sampler = false;
  for (auto b : pixel->bindings) {
    if (b.set != 1 || b.count != 1) throw std::runtime_error("RGBA64 descriptor layout");
    image |= b.binding == 14 && b.descriptor_type == VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE;
    sampler |= b.binding == 142 && b.descriptor_type == VK_DESCRIPTOR_TYPE_SAMPLER;
  }
  if (!image || !sampler) throw std::runtime_error("RGBA64 needs t0/s0");
  PipelineScope pipeline(c); build_pipeline(c,pipeline,vertex,pixel,nullptr);
  BlitScope b(c);
  VkBufferCreateInfo bi = { VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO };
  bi.size = read_offset+bytes; bi.usage = VK_BUFFER_USAGE_VERTEX_BUFFER_BIT|VK_BUFFER_USAGE_TRANSFER_SRC_BIT|VK_BUFFER_USAGE_TRANSFER_DST_BIT;
  check(c.fn<PFN_vkCreateBuffer>("vkCreateBuffer")(c.device,&bi,nullptr,&b.buffer),"staging/vertex buffer");
  VkMemoryRequirements req;
  c.fn<PFN_vkGetBufferMemoryRequirements>("vkGetBufferMemoryRequirements")(c.device,b.buffer,&req);
  b.buffer_memory = allocate(c,req,VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT|VK_MEMORY_PROPERTY_HOST_COHERENT_BIT);
  check(c.fn<PFN_vkBindBufferMemory>("vkBindBufferMemory")(c.device,b.buffer,b.buffer_memory,0),"bind buffer");
  check(c.fn<PFN_vkMapMemory>("vkMapMemory")(c.device,b.buffer_memory,0,VK_WHOLE_SIZE,0,&b.mapped),"map staging");
  std::memcpy(b.mapped,source,bytes);
  // Oversized full-screen triangle. The 48-byte layout matches build_pipeline;
  // attribute 0 = clip position, 1 = UV, 2 = unused zeroes.
  const float vertices[36] = {-1,-1,0,1, 0,0,0,0, 0,0,0,0,
                               3,-1,0,1, 2,0,0,0, 0,0,0,0,
                              -1, 3,0,1, 0,2,0,0, 0,0,0,0};
  std::memcpy(static_cast<uint8_t*>(b.mapped)+vertex_offset,vertices,sizeof(vertices));
  for (size_t i = 0; i < 2; i++) {
    VkImageCreateInfo ii = { VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO };
    ii.imageType = VK_IMAGE_TYPE_2D; ii.format = VK_FORMAT_R8G8B8A8_UNORM;
    ii.extent = {side,side,1}; ii.mipLevels = 1; ii.arrayLayers = 1;
    ii.samples = VK_SAMPLE_COUNT_1_BIT; ii.tiling = VK_IMAGE_TILING_OPTIMAL;
    ii.usage = i == 0 ? VK_IMAGE_USAGE_TRANSFER_DST_BIT|VK_IMAGE_USAGE_SAMPLED_BIT : VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT|VK_IMAGE_USAGE_TRANSFER_SRC_BIT;
    check(c.fn<PFN_vkCreateImage>("vkCreateImage")(c.device,&ii,nullptr,&b.images[i]),"RGBA image");
    c.fn<PFN_vkGetImageMemoryRequirements>("vkGetImageMemoryRequirements")(c.device,b.images[i],&req);
    b.image_memory[i] = allocate(c,req,VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT);
    check(c.fn<PFN_vkBindImageMemory>("vkBindImageMemory")(c.device,b.images[i],b.image_memory[i],0),"bind image");
    VkImageViewCreateInfo vi = { VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO };
    vi.image = b.images[i]; vi.viewType = VK_IMAGE_VIEW_TYPE_2D; vi.format = ii.format;
    vi.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT,0,1,0,1};
    check(c.fn<PFN_vkCreateImageView>("vkCreateImageView")(c.device,&vi,nullptr,&b.views[i]),"RGBA view");
  }
  VkSamplerCreateInfo si = { VK_STRUCTURE_TYPE_SAMPLER_CREATE_INFO };
  si.magFilter = VK_FILTER_NEAREST; si.minFilter = VK_FILTER_NEAREST;
  si.mipmapMode = VK_SAMPLER_MIPMAP_MODE_NEAREST;
  si.addressModeU = si.addressModeV = si.addressModeW = VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE;
  check(c.fn<PFN_vkCreateSampler>("vkCreateSampler")(c.device,&si,nullptr,&b.sampler),"nearest sampler");
  VkFramebufferCreateInfo fi = { VK_STRUCTURE_TYPE_FRAMEBUFFER_CREATE_INFO };
  fi.renderPass = pipeline.pass; fi.attachmentCount = 1; fi.pAttachments = &b.views[1];
  fi.width = side; fi.height = side; fi.layers = 1;
  check(c.fn<PFN_vkCreateFramebuffer>("vkCreateFramebuffer")(c.device,&fi,nullptr,&b.framebuffer),"offscreen framebuffer");
  std::array<VkDescriptorPoolSize,2> sizes = {{{VK_DESCRIPTOR_TYPE_SAMPLER,1},{VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE,1}}};
  VkDescriptorPoolCreateInfo di = { VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO };
  di.maxSets = 1; di.poolSizeCount = 2; di.pPoolSizes = sizes.data();
  check(c.fn<PFN_vkCreateDescriptorPool>("vkCreateDescriptorPool")(c.device,&di,nullptr,&b.descriptors),"descriptor pool");
  VkDescriptorSetAllocateInfo da = { VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO };
  da.descriptorPool = b.descriptors; da.descriptorSetCount = 1; da.pSetLayouts = &pipeline.sets[1];
  VkDescriptorSet set;
  check(c.fn<PFN_vkAllocateDescriptorSets>("vkAllocateDescriptorSets")(c.device,&da,&set),"descriptor set");
  std::array<VkDescriptorImageInfo,2> image_info = {{{b.sampler,VK_NULL_HANDLE,VK_IMAGE_LAYOUT_UNDEFINED},
    {VK_NULL_HANDLE,b.views[0],VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL}}};
  std::array<VkWriteDescriptorSet,2> writes = {};
  for (size_t i = 0; i < 2; i++) {
    writes[i].sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET; writes[i].dstSet = set;
    writes[i].dstBinding = i == 0 ? 142 : 14; writes[i].descriptorCount = 1;
    writes[i].descriptorType = sizes[i].type; writes[i].pImageInfo = &image_info[i];
  }
  c.fn<PFN_vkUpdateDescriptorSets>("vkUpdateDescriptorSets")(c.device,2,writes.data(),0,nullptr);
  VkCommandPoolCreateInfo pi = { VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO };
  pi.flags = VK_COMMAND_POOL_CREATE_TRANSIENT_BIT; pi.queueFamilyIndex = c.family;
  check(c.fn<PFN_vkCreateCommandPool>("vkCreateCommandPool")(c.device,&pi,nullptr,&b.commands),"command pool");
  VkCommandBufferAllocateInfo ca = { VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO };
  ca.commandPool = b.commands; ca.level = VK_COMMAND_BUFFER_LEVEL_PRIMARY; ca.commandBufferCount = 1;
  VkCommandBuffer command;
  check(c.fn<PFN_vkAllocateCommandBuffers>("vkAllocateCommandBuffers")(c.device,&ca,&command),"command buffer");
  VkCommandBufferBeginInfo begin = { VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO };
  begin.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
  check(c.fn<PFN_vkBeginCommandBuffer>("vkBeginCommandBuffer")(command,&begin),"begin command");
  auto barrier = c.fn<PFN_vkCmdPipelineBarrier>("vkCmdPipelineBarrier");
  VkImageMemoryBarrier transition = { VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER };
  transition.srcQueueFamilyIndex = transition.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
  transition.image = b.images[0]; transition.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT,0,1,0,1};
  transition.newLayout = VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL; transition.dstAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT;
  barrier(command,VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT,VK_PIPELINE_STAGE_TRANSFER_BIT,0,0,nullptr,0,nullptr,1,&transition);
  VkBufferImageCopy copy = {}; copy.imageSubresource = {VK_IMAGE_ASPECT_COLOR_BIT,0,0,1}; copy.imageExtent = {side,side,1};
  c.fn<PFN_vkCmdCopyBufferToImage>("vkCmdCopyBufferToImage")(command,b.buffer,b.images[0],VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,1,&copy);
  transition.oldLayout = transition.newLayout; transition.newLayout = VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL;
  transition.srcAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT; transition.dstAccessMask = VK_ACCESS_SHADER_READ_BIT;
  barrier(command,VK_PIPELINE_STAGE_TRANSFER_BIT,VK_PIPELINE_STAGE_FRAGMENT_SHADER_BIT,0,0,nullptr,0,nullptr,1,&transition);
  VkClearValue clear = {}; clear.color.float32[3] = 1;
  VkRenderPassBeginInfo render = { VK_STRUCTURE_TYPE_RENDER_PASS_BEGIN_INFO };
  render.renderPass = pipeline.pass; render.framebuffer = b.framebuffer; render.renderArea = {{0,0},{side,side}};
  render.clearValueCount = 1; render.pClearValues = &clear;
  c.fn<PFN_vkCmdBeginRenderPass>("vkCmdBeginRenderPass")(command,&render,VK_SUBPASS_CONTENTS_INLINE);
  c.fn<PFN_vkCmdBindPipeline>("vkCmdBindPipeline")(command,VK_PIPELINE_BIND_POINT_GRAPHICS,pipeline.pipeline);
  VkDeviceSize offset = vertex_offset;
  c.fn<PFN_vkCmdBindVertexBuffers>("vkCmdBindVertexBuffers")(command,0,1,&b.buffer,&offset);
  c.fn<PFN_vkCmdBindDescriptorSets>("vkCmdBindDescriptorSets")(command,VK_PIPELINE_BIND_POINT_GRAPHICS,pipeline.layout,1,1,&set,0,nullptr);
  c.fn<PFN_vkCmdDraw>("vkCmdDraw")(command,3,1,0,0);
  c.fn<PFN_vkCmdEndRenderPass>("vkCmdEndRenderPass")(command);
  transition.image = b.images[1]; transition.oldLayout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL;
  transition.newLayout = VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL;
  transition.srcAccessMask = VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT; transition.dstAccessMask = VK_ACCESS_TRANSFER_READ_BIT;
  barrier(command,VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,VK_PIPELINE_STAGE_TRANSFER_BIT,0,0,nullptr,0,nullptr,1,&transition);
  copy.bufferOffset = read_offset;
  c.fn<PFN_vkCmdCopyImageToBuffer>("vkCmdCopyImageToBuffer")(command,b.images[1],VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,b.buffer,1,&copy);
  VkBufferMemoryBarrier host = { VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER };
  host.srcAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT; host.dstAccessMask = VK_ACCESS_HOST_READ_BIT;
  host.srcQueueFamilyIndex = host.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
  host.buffer = b.buffer; host.offset = read_offset; host.size = bytes;
  barrier(command,VK_PIPELINE_STAGE_TRANSFER_BIT,VK_PIPELINE_STAGE_HOST_BIT,0,0,nullptr,1,&host,0,nullptr);
  check(c.fn<PFN_vkEndCommandBuffer>("vkEndCommandBuffer")(command),"end command");
  VkFenceCreateInfo fence_info = { VK_STRUCTURE_TYPE_FENCE_CREATE_INFO };
  check(c.fn<PFN_vkCreateFence>("vkCreateFence")(c.device,&fence_info,nullptr,&b.fence),"submission fence");
  VkSubmitInfo submit = { VK_STRUCTURE_TYPE_SUBMIT_INFO };
  submit.commandBufferCount = 1; submit.pCommandBuffers = &command;
  check(c.fn<PFN_vkQueueSubmit>("vkQueueSubmit")(c.queue,1,&submit,b.fence),"submit offscreen draw");
  b.pending = true;
  check(c.fn<PFN_vkWaitForFences>("vkWaitForFences")(c.device,1,&b.fence,VK_TRUE,5000000000ull),"wait offscreen draw");
  b.pending = false;
  std::memcpy(output,static_cast<uint8_t*>(b.mapped)+read_offset,bytes);
}
void geometry_dispatch(Context& c, Shader *shader, const uint32_t *input, size_t input_words,
                       uint32_t primitives, const WireBufferBinding *resources, size_t resource_count,
                       uint32_t *output, size_t output_words) {
  if (!shader || shader->owner != &c || shader->stage != 8)
    throw std::runtime_error("GS shader/context mismatch");
  const auto& g = shader->geometry;
  uint64_t required_input = uint64_t(primitives)*g.input_words;
  uint64_t required_output = uint64_t(primitives)*g.instances*g.invocation_words;
  if (!primitives || primitives > c.properties.limits.maxComputeWorkGroupCount[0] ||
      g.instances > c.properties.limits.maxComputeWorkGroupCount[1] ||
      required_input != input_words || required_output > output_words || !output ||
      (input_words && !input) || required_input > 16*1024*1024 || required_output > 16*1024*1024)
    throw std::runtime_error("GS dispatch buffer/count limits");
  if (shader->push_bytes) throw std::runtime_error("GS push data dispatch not implemented");
  if (resource_count > 256 || (resource_count && !resources)) throw std::runtime_error("GS resource capacity");
  std::map<std::pair<uint32_t,uint32_t>,Buffer*> supplied;
  for (size_t i = 0; i < resource_count; i++) {
    const auto& r = resources[i]; auto b = static_cast<Buffer*>(r.buffer);
    if (!b || &b->c != &c || r.set != 3 || r.binding >= 166 || !supplied.emplace(std::make_pair(r.set,r.binding),b).second)
      throw std::runtime_error("GS buffer context/binding mismatch");
  }
  size_t matched = 0;
  uint32_t uniforms = 0, storage = 0;
  for (auto binding : shader->bindings) {
    if (binding.set != 3 || binding.count != 1) throw std::runtime_error("GS descriptor array/set not implemented");
    bool internal = binding.binding == 166 || binding.binding == 167;
    if (internal) {
      if (binding.descriptor_type != VK_DESCRIPTOR_TYPE_STORAGE_BUFFER) throw std::runtime_error("GS internal buffer type");
      storage++; continue;
    }
    auto found = supplied.find({binding.set,binding.binding});
    if (found == supplied.end()) throw std::runtime_error("GS required buffer not bound");
    if (binding.binding < 14 && binding.descriptor_type == VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER && (found->second->usage&1)) uniforms++;
    else if (binding.binding >= 14 && binding.binding < 142 &&
        binding.descriptor_type == VK_DESCRIPTOR_TYPE_STORAGE_BUFFER && (found->second->usage&2)) storage++;
    else throw std::runtime_error("GS descriptor type/usage not implemented");
    matched++;
  }
  if (matched != resource_count) throw std::runtime_error("GS unused buffer binding");
  VkDeviceSize alignment = std::max(VkDeviceSize(4),c.properties.limits.minStorageBufferOffsetAlignment);
  VkDeviceSize input_bytes = std::max(VkDeviceSize(4),VkDeviceSize(input_words)*4);
  VkDeviceSize output_offset = (input_bytes+alignment-1)/alignment*alignment;
  VkDeviceSize output_bytes = required_output*4;
  if (input_bytes > c.properties.limits.maxStorageBufferRange || output_bytes > c.properties.limits.maxStorageBufferRange)
    throw std::runtime_error("GS storage buffer range");
  PipelineScope pipeline(c); build_pipeline(c,pipeline,nullptr,nullptr,shader);
  BlitScope b(c);
  VkBufferCreateInfo bi = {VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO};
  bi.size = output_offset+output_bytes; bi.usage = VK_BUFFER_USAGE_STORAGE_BUFFER_BIT;
  check(c.fn<PFN_vkCreateBuffer>("vkCreateBuffer")(c.device,&bi,nullptr,&b.buffer),"GS storage buffer");
  VkMemoryRequirements req;
  c.fn<PFN_vkGetBufferMemoryRequirements>("vkGetBufferMemoryRequirements")(c.device,b.buffer,&req);
  b.buffer_memory = allocate(c,req,VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT|VK_MEMORY_PROPERTY_HOST_COHERENT_BIT);
  check(c.fn<PFN_vkBindBufferMemory>("vkBindBufferMemory")(c.device,b.buffer,b.buffer_memory,0),"GS bind buffer");
  check(c.fn<PFN_vkMapMemory>("vkMapMemory")(c.device,b.buffer_memory,0,VK_WHOLE_SIZE,0,&b.mapped),"GS map buffer");
  std::memset(b.mapped,0,size_t(bi.size));
  if (input_words) std::memcpy(b.mapped,input,input_words*4);
  std::vector<VkDescriptorPoolSize> pool_sizes;
  if (storage) pool_sizes.push_back({VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,storage});
  if (uniforms) pool_sizes.push_back({VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER,uniforms});
  VkDescriptorPoolCreateInfo pool = {VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO};
  pool.maxSets = 1; pool.poolSizeCount = uint32_t(pool_sizes.size()); pool.pPoolSizes = pool_sizes.data();
  check(c.fn<PFN_vkCreateDescriptorPool>("vkCreateDescriptorPool")(c.device,&pool,nullptr,&b.descriptors),"GS descriptor pool");
  VkDescriptorSetAllocateInfo da = {VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO};
  da.descriptorPool = b.descriptors; da.descriptorSetCount = 1; da.pSetLayouts = &pipeline.sets[3];
  VkDescriptorSet set;
  check(c.fn<PFN_vkAllocateDescriptorSets>("vkAllocateDescriptorSets")(c.device,&da,&set),"GS descriptor set");
  std::vector<VkDescriptorBufferInfo> infos; infos.reserve(shader->bindings.size());
  std::vector<VkWriteDescriptorSet> writes;
  for (auto binding : shader->bindings) {
    VkWriteDescriptorSet write = {VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET};
    write.dstSet = set; write.dstBinding = binding.binding; write.descriptorCount = 1;
    write.descriptorType = VkDescriptorType(binding.descriptor_type);
    if (binding.binding == 166) infos.push_back({b.buffer,0,input_bytes});
    else if (binding.binding == 167) infos.push_back({b.buffer,output_offset,output_bytes});
    else {
      auto resource = supplied.at({binding.set,binding.binding});
      infos.push_back({resource->handle,0,resource->size});
    }
    write.pBufferInfo = &infos.back();
    writes.push_back(write);
  }
  c.fn<PFN_vkUpdateDescriptorSets>("vkUpdateDescriptorSets")(c.device,uint32_t(writes.size()),writes.data(),0,nullptr);
  VkCommandPoolCreateInfo pi = {VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO};
  pi.flags = VK_COMMAND_POOL_CREATE_TRANSIENT_BIT; pi.queueFamilyIndex = c.family;
  check(c.fn<PFN_vkCreateCommandPool>("vkCreateCommandPool")(c.device,&pi,nullptr,&b.commands),"GS command pool");
  VkCommandBufferAllocateInfo ca = {VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO};
  ca.commandPool = b.commands; ca.level = VK_COMMAND_BUFFER_LEVEL_PRIMARY; ca.commandBufferCount = 1;
  VkCommandBuffer command;
  check(c.fn<PFN_vkAllocateCommandBuffers>("vkAllocateCommandBuffers")(c.device,&ca,&command),"GS command buffer");
  VkCommandBufferBeginInfo begin = {VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO};
  begin.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
  check(c.fn<PFN_vkBeginCommandBuffer>("vkBeginCommandBuffer")(command,&begin),"GS begin command");
  c.fn<PFN_vkCmdBindPipeline>("vkCmdBindPipeline")(command,VK_PIPELINE_BIND_POINT_COMPUTE,pipeline.pipeline);
  c.fn<PFN_vkCmdBindDescriptorSets>("vkCmdBindDescriptorSets")(command,VK_PIPELINE_BIND_POINT_COMPUTE,pipeline.layout,3,1,&set,0,nullptr);
  c.fn<PFN_vkCmdDispatch>("vkCmdDispatch")(command,primitives,g.instances,1);
  VkBufferMemoryBarrier host = {VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER};
  host.srcAccessMask = VK_ACCESS_SHADER_WRITE_BIT; host.dstAccessMask = VK_ACCESS_HOST_READ_BIT;
  host.srcQueueFamilyIndex = host.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
  host.buffer = b.buffer; host.offset = output_offset; host.size = output_bytes;
  VkMemoryBarrier resources_host = {VK_STRUCTURE_TYPE_MEMORY_BARRIER};
  resources_host.srcAccessMask = VK_ACCESS_SHADER_WRITE_BIT; resources_host.dstAccessMask = VK_ACCESS_HOST_READ_BIT;
  c.fn<PFN_vkCmdPipelineBarrier>("vkCmdPipelineBarrier")(command,VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT,VK_PIPELINE_STAGE_HOST_BIT,0,1,&resources_host,1,&host,0,nullptr);
  check(c.fn<PFN_vkEndCommandBuffer>("vkEndCommandBuffer")(command),"GS end command");
  VkFenceCreateInfo fi = {VK_STRUCTURE_TYPE_FENCE_CREATE_INFO};
  check(c.fn<PFN_vkCreateFence>("vkCreateFence")(c.device,&fi,nullptr,&b.fence),"GS fence");
  VkSubmitInfo submit = {VK_STRUCTURE_TYPE_SUBMIT_INFO}; submit.commandBufferCount = 1; submit.pCommandBuffers = &command;
  check(c.fn<PFN_vkQueueSubmit>("vkQueueSubmit")(c.queue,1,&submit,b.fence),"GS submit"); b.pending = true;
  check(c.fn<PFN_vkWaitForFences>("vkWaitForFences")(c.device,1,&b.fence,VK_TRUE,5000000000ull),"GS wait"); b.pending = false;
  auto result = reinterpret_cast<const uint32_t*>(static_cast<const uint8_t*>(b.mapped)+output_offset);
  for (uint64_t i = 0; i < uint64_t(primitives)*g.instances; i++)
    if (result[i*g.invocation_words] > g.max_vertices) throw std::runtime_error("GS emitted more than declared max vertices");
  std::memcpy(output,result,size_t(output_bytes));
}
Shader geometry_vertex_shader(Context& c, const wire_geometry::Geometry& g,
                               std::vector<VkVertexInputAttributeDescription>& attributes) {
  using namespace ir;
  Builder b;
  auto function = b.add(Op::Function({}));
  auto entry = b.add(Op::EntryPoint(function,ShaderStage::eVertex));
  b.add(Op::Label());
  bool position = false;
  for (uint32_t i = 0; i < g.outputs.size(); i++) {
    const auto& io = g.outputs[i];
    if (io.array_dimensions || !io.array_size || io.array_size != io.vector_size ||
        io.vector_size > 4 || i >= c.properties.limits.maxVertexInputAttributes)
      throw std::runtime_error("GS forwarding VS aggregate/attribute limit");
    auto scalar = ScalarType(io.scalar_type);
    Type type(scalar,io.array_size);
    SsaDef output;
    if (io.built_in == uint32_t(BuiltIn::ePosition)) {
      if (type != Type(ScalarType::eF32,4) || position) throw std::runtime_error("GS forwarding position type");
      position = true; output = b.add(Op::DclOutputBuiltIn(type,entry,BuiltIn::ePosition));
    } else if (io.built_in == UINT32_MAX) {
      output = b.add(Op::DclOutput(type,entry,io.location,io.component));
    } else throw std::runtime_error("GS forwarding built-in not implemented");
    const std::array<VkFormat,4> floats = {VK_FORMAT_R32_SFLOAT,VK_FORMAT_R32G32_SFLOAT,VK_FORMAT_R32G32B32_SFLOAT,VK_FORMAT_R32G32B32A32_SFLOAT};
    const std::array<VkFormat,4> uints = {VK_FORMAT_R32_UINT,VK_FORMAT_R32G32_UINT,VK_FORMAT_R32G32B32_UINT,VK_FORMAT_R32G32B32A32_UINT};
    const std::array<VkFormat,4> ints = {VK_FORMAT_R32_SINT,VK_FORMAT_R32G32_SINT,VK_FORMAT_R32G32B32_SINT,VK_FORMAT_R32G32B32A32_SINT};
    VkFormat format = scalar == ScalarType::eF32 ? floats[io.array_size-1] : scalar == ScalarType::eU32 ? uints[io.array_size-1] : ints[io.array_size-1];
    attributes.push_back({i,0,format,io.word_offset*4});
    auto input = b.add(Op::DclInput(type,entry,i,0));
    b.add(Op::OutputStore(output,{},b.add(Op::InputLoad(type,input,{}))));
  }
  if (!position) throw std::runtime_error("GS forwarding requires position");
  if (g.output_primitive == 0) {
    auto point = b.add(Op::DclOutputBuiltIn(ScalarType::eF32,entry,BuiltIn::ePointSize));
    b.add(Op::OutputStore(point,{},b.makeConstant(1.0f)));
  }
  b.add(Op::Return()); b.add(Op::FunctionEnd());
  Mapping mapping(1); spirv::SpirvBuilder::Options options; options.includeDebugNames = false;
  spirv::SpirvBuilder compiler(b,mapping,options); compiler.buildSpirvBinary();
  Shader shader; shader.owner = &c; shader.stage = 1; shader.words = compiler.getSpirvBinary();
  reflect(shader); return shader;
}
void geometry_draw_rgba64(Context& c, Shader *geometry, Shader *pixel,
    const uint32_t *vertices, size_t words, const uint32_t *indices, size_t count,
    uint8_t *output, size_t capacity) {
  constexpr size_t bytes = 64*64*4;
  if (!geometry || geometry->owner != &c || geometry->stage != 8 ||
      !pixel || pixel->owner != &c || pixel->stage != 2)
    throw std::runtime_error("GS draw shader/context mismatch");
  const auto& g = geometry->geometry;
  if (!output || capacity < bytes || !g.vertex_words || words%g.vertex_words ||
      words > 16*1024*1024 || count > 16*1024*1024 || (words && !vertices) || (count && !indices))
    throw std::runtime_error("GS draw buffer limits");
  uint32_t arity = g.output_primitive == 0 ? 1 : g.output_primitive == 1 ? 2 : 3;
  if (count%arity) throw std::runtime_error("GS list index arity");
  for (size_t i = 0; i < count; i++) if (indices[i] >= words/g.vertex_words)
    throw std::runtime_error("GS draw index outside vertex buffer");
  if (!pixel->bindings.empty() || pixel->push_bytes || pixel->outputs.size() != 1 ||
      pixel->outputs.at({0,0}) != std::vector<uint32_t>({22,32,4}))
    throw std::runtime_error("GS draw PS resources/output not implemented");
  std::vector<VkVertexInputAttributeDescription> attributes;
  auto vertex = geometry_vertex_shader(c,g,attributes);
  PipelineScope p(c);
  VkPrimitiveTopology topology = arity == 1 ? VK_PRIMITIVE_TOPOLOGY_POINT_LIST : arity == 2 ? VK_PRIMITIVE_TOPOLOGY_LINE_LIST : VK_PRIMITIVE_TOPOLOGY_TRIANGLE_LIST;
  build_pipeline(c,p,&vertex,pixel,nullptr,&attributes,g.vertex_words*4,topology);
  BlitScope b(c);
  VkDeviceSize index_offset = VkDeviceSize(words)*4;
  VkDeviceSize read_offset = std::max(VkDeviceSize(256),(index_offset+count*4+255)/256*256);
  VkBufferCreateInfo bi = {VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO};
  bi.size = read_offset+bytes; bi.usage = VK_BUFFER_USAGE_VERTEX_BUFFER_BIT|VK_BUFFER_USAGE_INDEX_BUFFER_BIT|VK_BUFFER_USAGE_TRANSFER_DST_BIT;
  check(c.fn<PFN_vkCreateBuffer>("vkCreateBuffer")(c.device,&bi,nullptr,&b.buffer),"GS draw buffer");
  VkMemoryRequirements req;
  c.fn<PFN_vkGetBufferMemoryRequirements>("vkGetBufferMemoryRequirements")(c.device,b.buffer,&req);
  b.buffer_memory = allocate(c,req,VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT|VK_MEMORY_PROPERTY_HOST_COHERENT_BIT);
  check(c.fn<PFN_vkBindBufferMemory>("vkBindBufferMemory")(c.device,b.buffer,b.buffer_memory,0),"GS draw bind buffer");
  check(c.fn<PFN_vkMapMemory>("vkMapMemory")(c.device,b.buffer_memory,0,VK_WHOLE_SIZE,0,&b.mapped),"GS draw map");
  if (words) std::memcpy(b.mapped,vertices,words*4);
  if (count) std::memcpy(static_cast<uint8_t*>(b.mapped)+index_offset,indices,count*4);
  VkImageCreateInfo ii = {VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO}; ii.imageType = VK_IMAGE_TYPE_2D;
  ii.format = VK_FORMAT_R8G8B8A8_UNORM; ii.extent = {64,64,1}; ii.mipLevels = 1; ii.arrayLayers = 1;
  ii.samples = VK_SAMPLE_COUNT_1_BIT; ii.tiling = VK_IMAGE_TILING_OPTIMAL;
  ii.usage = VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT|VK_IMAGE_USAGE_TRANSFER_SRC_BIT;
  check(c.fn<PFN_vkCreateImage>("vkCreateImage")(c.device,&ii,nullptr,&b.images[1]),"GS render target");
  c.fn<PFN_vkGetImageMemoryRequirements>("vkGetImageMemoryRequirements")(c.device,b.images[1],&req);
  b.image_memory[1] = allocate(c,req,VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT);
  check(c.fn<PFN_vkBindImageMemory>("vkBindImageMemory")(c.device,b.images[1],b.image_memory[1],0),"GS bind render target");
  VkImageViewCreateInfo vi = {VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO};
  vi.image = b.images[1]; vi.viewType = VK_IMAGE_VIEW_TYPE_2D; vi.format = ii.format;
  vi.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT,0,1,0,1};
  check(c.fn<PFN_vkCreateImageView>("vkCreateImageView")(c.device,&vi,nullptr,&b.views[1]),"GS render target view");
  VkFramebufferCreateInfo fi = {VK_STRUCTURE_TYPE_FRAMEBUFFER_CREATE_INFO};
  fi.renderPass = p.pass; fi.attachmentCount = 1; fi.pAttachments = &b.views[1]; fi.width = 64; fi.height = 64; fi.layers = 1;
  check(c.fn<PFN_vkCreateFramebuffer>("vkCreateFramebuffer")(c.device,&fi,nullptr,&b.framebuffer),"GS framebuffer");
  VkCommandPoolCreateInfo pi = {VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO};
  pi.flags = VK_COMMAND_POOL_CREATE_TRANSIENT_BIT; pi.queueFamilyIndex = c.family;
  check(c.fn<PFN_vkCreateCommandPool>("vkCreateCommandPool")(c.device,&pi,nullptr,&b.commands),"GS draw command pool");
  VkCommandBufferAllocateInfo ca = {VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO};
  ca.commandPool = b.commands; ca.level = VK_COMMAND_BUFFER_LEVEL_PRIMARY; ca.commandBufferCount = 1;
  VkCommandBuffer command;
  check(c.fn<PFN_vkAllocateCommandBuffers>("vkAllocateCommandBuffers")(c.device,&ca,&command),"GS draw command");
  VkCommandBufferBeginInfo begin = {VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO}; begin.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
  check(c.fn<PFN_vkBeginCommandBuffer>("vkBeginCommandBuffer")(command,&begin),"GS draw begin");
  VkClearValue clear = {}; clear.color.float32[3] = 1;
  VkRenderPassBeginInfo render = {VK_STRUCTURE_TYPE_RENDER_PASS_BEGIN_INFO};
  render.renderPass = p.pass; render.framebuffer = b.framebuffer; render.renderArea = {{0,0},{64,64}};
  render.clearValueCount = 1; render.pClearValues = &clear;
  c.fn<PFN_vkCmdBeginRenderPass>("vkCmdBeginRenderPass")(command,&render,VK_SUBPASS_CONTENTS_INLINE);
  c.fn<PFN_vkCmdBindPipeline>("vkCmdBindPipeline")(command,VK_PIPELINE_BIND_POINT_GRAPHICS,p.pipeline);
  if (count) {
    VkDeviceSize offset = 0;
    c.fn<PFN_vkCmdBindVertexBuffers>("vkCmdBindVertexBuffers")(command,0,1,&b.buffer,&offset);
    c.fn<PFN_vkCmdBindIndexBuffer>("vkCmdBindIndexBuffer")(command,b.buffer,index_offset,VK_INDEX_TYPE_UINT32);
    c.fn<PFN_vkCmdDrawIndexed>("vkCmdDrawIndexed")(command,uint32_t(count),1,0,0,0);
  }
  c.fn<PFN_vkCmdEndRenderPass>("vkCmdEndRenderPass")(command);
  VkImageMemoryBarrier transition = {VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER};
  transition.srcAccessMask = VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT; transition.dstAccessMask = VK_ACCESS_TRANSFER_READ_BIT;
  transition.oldLayout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL; transition.newLayout = VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL;
  transition.srcQueueFamilyIndex = transition.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
  transition.image = b.images[1]; transition.subresourceRange = vi.subresourceRange;
  auto barrier = c.fn<PFN_vkCmdPipelineBarrier>("vkCmdPipelineBarrier");
  barrier(command,VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,VK_PIPELINE_STAGE_TRANSFER_BIT,0,0,nullptr,0,nullptr,1,&transition);
  VkBufferImageCopy copy = {}; copy.bufferOffset = read_offset;
  copy.imageSubresource = {VK_IMAGE_ASPECT_COLOR_BIT,0,0,1}; copy.imageExtent = {64,64,1};
  c.fn<PFN_vkCmdCopyImageToBuffer>("vkCmdCopyImageToBuffer")(command,b.images[1],transition.newLayout,b.buffer,1,&copy);
  VkBufferMemoryBarrier host = {VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER};
  host.srcAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT; host.dstAccessMask = VK_ACCESS_HOST_READ_BIT;
  host.srcQueueFamilyIndex = host.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
  host.buffer = b.buffer; host.offset = read_offset; host.size = bytes;
  barrier(command,VK_PIPELINE_STAGE_TRANSFER_BIT,VK_PIPELINE_STAGE_HOST_BIT,0,0,nullptr,1,&host,0,nullptr);
  check(c.fn<PFN_vkEndCommandBuffer>("vkEndCommandBuffer")(command),"GS draw end");
  VkFenceCreateInfo fence = {VK_STRUCTURE_TYPE_FENCE_CREATE_INFO};
  check(c.fn<PFN_vkCreateFence>("vkCreateFence")(c.device,&fence,nullptr,&b.fence),"GS draw fence");
  VkSubmitInfo submit = {VK_STRUCTURE_TYPE_SUBMIT_INFO}; submit.commandBufferCount = 1; submit.pCommandBuffers = &command;
  check(c.fn<PFN_vkQueueSubmit>("vkQueueSubmit")(c.queue,1,&submit,b.fence),"GS draw submit"); b.pending = true;
  check(c.fn<PFN_vkWaitForFences>("vkWaitForFences")(c.device,1,&b.fence,VK_TRUE,5000000000ull),"GS draw wait"); b.pending = false;
  std::memcpy(output,static_cast<uint8_t*>(b.mapped)+read_offset,bytes);
}
} // namespace

extern "C" {
int wire_gpu_timestamp(void *gpu, uint64_t *ticks, uint64_t *frequency, char *error, size_t capacity) {
  try {
    auto cp=static_cast<Context*>(gpu);
    if (!cp || !ticks || !frequency || !cp->timestamp_bits || cp->properties.limits.timestampPeriod<=0)
      throw std::runtime_error("queue timestamps unavailable");
    auto& c=*cp; BlitScope scope(c);
    VkQueryPool pool=VK_NULL_HANDLE;
    struct Pool { Context& c; VkQueryPool& p; ~Pool() { if(p) c.wait_idle(c.device); if(p) c.fn<PFN_vkDestroyQueryPool>("vkDestroyQueryPool")(c.device,p,nullptr); } } cleanup{c,pool};
    VkQueryPoolCreateInfo qi={VK_STRUCTURE_TYPE_QUERY_POOL_CREATE_INFO}; qi.queryType=VK_QUERY_TYPE_TIMESTAMP;qi.queryCount=1;
    check(c.fn<PFN_vkCreateQueryPool>("vkCreateQueryPool")(c.device,&qi,nullptr,&pool),"create timestamp query");
    VkCommandPoolCreateInfo pi={VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO}; pi.queueFamilyIndex=c.family;
    check(c.fn<PFN_vkCreateCommandPool>("vkCreateCommandPool")(c.device,&pi,nullptr,&scope.commands),"timestamp command pool");
    VkCommandBufferAllocateInfo ai={VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO}; ai.commandPool=scope.commands;
    ai.level=VK_COMMAND_BUFFER_LEVEL_PRIMARY;ai.commandBufferCount=1;VkCommandBuffer cmd;
    check(c.fn<PFN_vkAllocateCommandBuffers>("vkAllocateCommandBuffers")(c.device,&ai,&cmd),"timestamp command buffer");
    VkCommandBufferBeginInfo bi={VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO};bi.flags=VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
    check(c.fn<PFN_vkBeginCommandBuffer>("vkBeginCommandBuffer")(cmd,&bi),"timestamp begin");
    c.fn<PFN_vkCmdResetQueryPool>("vkCmdResetQueryPool")(cmd,pool,0,1);
    c.fn<PFN_vkCmdWriteTimestamp>("vkCmdWriteTimestamp")(cmd,VK_PIPELINE_STAGE_BOTTOM_OF_PIPE_BIT,pool,0);
    check(c.fn<PFN_vkEndCommandBuffer>("vkEndCommandBuffer")(cmd),"timestamp end");
    VkFenceCreateInfo fi={VK_STRUCTURE_TYPE_FENCE_CREATE_INFO};
    check(c.fn<PFN_vkCreateFence>("vkCreateFence")(c.device,&fi,nullptr,&scope.fence),"timestamp fence");
    VkSubmitInfo si={VK_STRUCTURE_TYPE_SUBMIT_INFO};si.commandBufferCount=1;si.pCommandBuffers=&cmd;
    check(c.fn<PFN_vkQueueSubmit>("vkQueueSubmit")(c.queue,1,&si,scope.fence),"timestamp submit");scope.pending=true;
    auto waited=c.fn<PFN_vkWaitForFences>("vkWaitForFences")(c.device,1,&scope.fence,VK_TRUE,5000000000ull);
    if(waited!=VK_SUCCESS) {c.wait_idle(c.device);scope.pending=false;check(waited,"timestamp wait");}scope.pending=false;
    uint64_t result=0;
    check(c.fn<PFN_vkGetQueryPoolResults>("vkGetQueryPoolResults")(c.device,pool,0,1,sizeof(result),&result,sizeof(result),VK_QUERY_RESULT_64_BIT|VK_QUERY_RESULT_WAIT_BIT),"timestamp read");
    if(c.timestamp_bits<64) result&=(1ull<<c.timestamp_bits)-1;
    *ticks=result;*frequency=uint64_t(1000000000.0/double(c.properties.limits.timestampPeriod));return 1;
  } catch(const std::exception& e) {error_text(error,capacity,e.what());return 0;}
}
void *wire_gpu_texture_create(void *gpu,const WireTextureDesc *desc,char *error,size_t capacity) {
  try {if(!gpu || !desc)throw std::runtime_error("missing texture descriptor/device");return create_texture(*static_cast<Context*>(gpu),*desc).release();}
  catch(const std::exception& e){error_text(error,capacity,e.what());return nullptr;}
}
void wire_gpu_texture_free(void *texture){delete static_cast<Texture*>(texture);}
void *wire_gpu_surface_create(void *gpu,const void *layer,char *error,size_t capacity) {
  try {if(!gpu || !layer)throw std::runtime_error("missing GPU/layer");return create_surface(*static_cast<Context*>(gpu),layer).release();}
  catch(const std::exception& e){error_text(error,capacity,e.what());return nullptr;}
}
void wire_gpu_surface_free(void *surface){delete static_cast<Surface*>(surface);}
int wire_gpu_surface_present(void *surface,void *texture,uint32_t width,uint32_t height,uint32_t interval,char *error,size_t capacity) {
  try {if(!surface || !texture)throw std::runtime_error("missing surface/texture");present_texture(*static_cast<Surface*>(surface),*static_cast<Texture*>(texture),width,height,interval);return 1;}
  catch(const std::exception& e){error_text(error,capacity,e.what());return 0;}
}
int wire_gpu_texture_write(void *texture,uint32_t subresource,const uint8_t *data,size_t size,char *error,size_t capacity) {
  try {if(!texture)throw std::runtime_error("missing texture");texture_write(*static_cast<Texture*>(texture),subresource,data,size);return 1;}
  catch(const std::exception& e){error_text(error,capacity,e.what());return 0;}
}
int wire_gpu_texture_read(void *texture,uint32_t subresource,uint8_t *data,size_t size,char *error,size_t capacity) {
  try {if(!texture)throw std::runtime_error("missing texture");texture_read(*static_cast<Texture*>(texture),subresource,data,size);return 1;}
  catch(const std::exception& e){error_text(error,capacity,e.what());return 0;}
}
void *wire_gpu_image_view_create(void *texture,const WireImageViewDesc *desc,char *error,size_t capacity) {
  try {if(!texture || !desc)throw std::runtime_error("missing image view descriptor/resource");return create_image_view(*static_cast<Texture*>(texture),*desc).release();}
  catch(const std::exception& e){error_text(error,capacity,e.what());return nullptr;}
}
void wire_gpu_image_view_free(void *view){delete static_cast<ImageView*>(view);}
int wire_gpu_image_view_clear(void *view,const float *rgba,char *error,size_t capacity) {
  try {if(!view)throw std::runtime_error("missing image view");clear_image_view(*static_cast<ImageView*>(view),rgba);return 1;}
  catch(const std::exception& e){error_text(error,capacity,e.what());return 0;}
}
int wire_gpu_image_view_clear_depth(void *view,uint32_t flags,float depth,uint32_t stencil,char *error,size_t capacity) {
  try {if(!view)throw std::runtime_error("missing depth view");clear_depth_view(*static_cast<ImageView*>(view),flags,depth,stencil);return 1;}
  catch(const std::exception& e){error_text(error,capacity,e.what());return 0;}
}
void *wire_gpu_buffer_view_create(void *buffer,uint32_t format,size_t offset,size_t size,char *error,size_t capacity) {
  try {if(!buffer)throw std::runtime_error("missing buffer");return create_buffer_view(*static_cast<Buffer*>(buffer),format,offset,size).release();}
  catch(const std::exception& e){error_text(error,capacity,e.what());return nullptr;}
}
void wire_gpu_buffer_view_free(void *view){delete static_cast<BufferView*>(view);}
int wire_gpu_texture_copy(void *dst,void *src,const WireTextureCopy *region,char *error,size_t capacity) {
  try {if(!dst || !src || !region)throw std::runtime_error("missing texture copy resources/region");texture_copy(*static_cast<Texture*>(dst),*static_cast<Texture*>(src),*region);return 1;}
  catch(const std::exception& e){error_text(error,capacity,e.what());return 0;}
}
uint32_t wire_gpu_format_support(void *gpu, uint32_t format) {
  auto c = static_cast<Context*>(gpu); auto f = texture_storage_format(format);
  if (!c || f == VK_FORMAT_UNDEFINED) return 0;
  VkFormatProperties p = {}; c->format_properties(c->physical,f,&p);
  if(format==27 || format==39 || format==53 || format==90) {
    // Only storage capabilities: the caller must select a supported typed view.
    return (p.optimalTilingFeatures&VK_FORMAT_FEATURE_SAMPLED_IMAGE_BIT)?0x320:0;
  }
  uint32_t result = 0;
  if (p.optimalTilingFeatures & VK_FORMAT_FEATURE_SAMPLED_IMAGE_BIT) result |= 0x20 | 0x100 | 0x200 | 0x1000;
  if (p.optimalTilingFeatures & VK_FORMAT_FEATURE_COLOR_ATTACHMENT_BIT) result |= 0x20 | 0x4000;
  if (p.optimalTilingFeatures & VK_FORMAT_FEATURE_COLOR_ATTACHMENT_BLEND_BIT) result |= 0x8000;
  if (p.optimalTilingFeatures & VK_FORMAT_FEATURE_DEPTH_STENCIL_ATTACHMENT_BIT) {
    result |= 0x20;
    if(format==20 || format==40 || format==45 || format==55)result|=0x10000;
  }
  if (p.bufferFeatures & VK_FORMAT_FEATURE_VERTEX_BUFFER_BIT) result |= 3;
  if (format == 42 || format == 57) result |= 5;
  if (format == 28 || format == 87) result |= 0x80000;
  return result;
}
uint32_t wire_gpu_abi() { return 1; }
int wire_gpu_draw(void *gpu,const WireDraw *description,char *error,size_t capacity) {
  try {if(!gpu || !description)throw std::runtime_error("missing draw device/description");draw(*static_cast<Context*>(gpu),*description);return 1;}
  catch(const std::exception& e){error_text(error,capacity,e.what());return 0;}
}
void *wire_gpu_open(const char *library, WireGpuInfo *info, char *error, size_t capacity) {
  try { if (!info) throw std::runtime_error("missing info output"); WireGpuInfo value;
    auto c = open_gpu(library, value); *info = value; return c.release();
  } catch (const std::exception& e) { error_text(error, capacity, e.what()); return nullptr; }
}
void wire_gpu_close(void *gpu) { delete static_cast<Context*>(gpu); }
void *wire_gpu_compile(void *gpu, const uint8_t *data, size_t size, uint32_t stage, char *error, size_t capacity) {
  try { if (!gpu) throw std::runtime_error("missing GPU");
    return compile(*static_cast<Context*>(gpu), data, size, stage).release();
  } catch (const std::exception& e) { error_text(error, capacity, e.what()); return nullptr; }
}
int wire_gpu_shader_info(void *shader, WireShaderInfo *info) {
  if (!shader || !info) return 0;
  auto& s = *static_cast<Shader*>(shader);
  *info = {s.words.data(),s.words.size(),s.bindings.data(),s.bindings.size(),s.stage,s.push_bytes}; return 1;
}
void wire_gpu_shader_free(void *shader) { delete static_cast<Shader*>(shader); }
int wire_gpu_geometry_info(void *shader, WireGeometryInfo *info) {
  if (!shader || !info || static_cast<Shader*>(shader)->stage != 8) return 0;
  const auto& g = static_cast<Shader*>(shader)->geometry;
  *info = {g.inputs.data(),g.outputs.data(),g.inputs.size(),g.outputs.size(),g.input_vertices,
    g.output_primitive,g.max_vertices,g.instances,g.input_words,g.vertex_words,g.invocation_words,0};
  return 1;
}
void *wire_gpu_geometry_vertex(void *gpu, void *geometry, char *error, size_t capacity) {
  try { auto g = static_cast<Shader*>(geometry); auto c = static_cast<Context*>(gpu);
    if (!c || !g || g->owner != c || g->stage != 8) throw std::runtime_error("GS forwarding shader/context mismatch");
    std::vector<VkVertexInputAttributeDescription> attributes;
    return new Shader(geometry_vertex_shader(*c,g->geometry,attributes));
  } catch (const std::exception& e) { error_text(error,capacity,e.what()); return nullptr; }
}
int wire_gpu_geometry_dispatch(void *gpu, void *shader, const uint32_t *input, size_t input_words,
    uint32_t primitives, const WireBufferBinding *bindings, size_t binding_count,
    uint32_t *output, size_t output_words, char *error, size_t capacity) {
  try { if (!gpu) throw std::runtime_error("missing GPU");
    geometry_dispatch(*static_cast<Context*>(gpu),static_cast<Shader*>(shader),input,input_words,primitives,bindings,binding_count,output,output_words); return 1;
  } catch (const std::exception& e) { error_text(error,capacity,e.what()); return 0; }
}
void *wire_gpu_buffer_create(void *gpu, const uint8_t *data, size_t size, uint32_t usage, char *error, size_t capacity) {
  try { if (!gpu) throw std::runtime_error("missing GPU");
    return create_buffer(*static_cast<Context*>(gpu),data,size,usage).release();
  } catch (const std::exception& e) { error_text(error,capacity,e.what()); return nullptr; }
}
void wire_gpu_buffer_free(void *buffer) { delete static_cast<Buffer*>(buffer); }
int wire_gpu_buffer_write(void *buffer, size_t offset, const uint8_t *data, size_t size, char *error, size_t capacity) {
  try { auto b = static_cast<Buffer*>(buffer);
    if (!b || offset > b->size || size > b->size-offset || (size && !data)) throw std::runtime_error("buffer write range");
    if (size) std::memcpy(static_cast<uint8_t*>(b->mapped)+offset,data,size); return 1;
  } catch (const std::exception& e) { error_text(error,capacity,e.what()); return 0; }
}
int wire_gpu_buffer_read(void *buffer, size_t offset, uint8_t *data, size_t size, char *error, size_t capacity) {
  try { auto b = static_cast<Buffer*>(buffer);
    if (!b || offset > b->size || size > b->size-offset || (size && !data)) throw std::runtime_error("buffer read range");
    if (size) std::memcpy(data,static_cast<uint8_t*>(b->mapped)+offset,size); return 1;
  } catch (const std::exception& e) { error_text(error,capacity,e.what()); return 0; }
}
int wire_gpu_geometry_draw_rgba64(void *gpu, void *geometry, void *pixel,
    const uint32_t *vertices, size_t words, const uint32_t *indices, size_t count,
    uint8_t *output, size_t output_size, char *error, size_t capacity) {
  try { if (!gpu) throw std::runtime_error("missing GPU");
    geometry_draw_rgba64(*static_cast<Context*>(gpu),static_cast<Shader*>(geometry),static_cast<Shader*>(pixel),
      vertices,words,indices,count,output,output_size); return 1;
  } catch (const std::exception& e) { error_text(error,capacity,e.what()); return 0; }
}
int wire_gpu_pipeline(void *gpu, void *vertex, void *pixel, void *compute, char *error, size_t capacity) {
  try { if (!gpu) throw std::runtime_error("missing GPU");
    auto& c = *static_cast<Context*>(gpu); PipelineScope p(c);
    build_pipeline(c,p,static_cast<Shader*>(vertex),static_cast<Shader*>(pixel),static_cast<Shader*>(compute)); return 1;
  } catch (const std::exception& e) { error_text(error, capacity, e.what()); return 0; }
}
int wire_gpu_blit_rgba64(void *gpu, void *vertex, void *pixel, const uint8_t *source, size_t size,
                         uint8_t *output, size_t output_size, char *error, size_t capacity) {
  try { if (!gpu) throw std::runtime_error("missing GPU");
    blit_rgba64(*static_cast<Context*>(gpu),static_cast<Shader*>(vertex),static_cast<Shader*>(pixel),source,size,output,output_size); return 1;
  } catch (const std::exception& e) { error_text(error,capacity,e.what()); return 0; }
}

}
