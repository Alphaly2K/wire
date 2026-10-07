// Independently implemented host Vulkan/COM boundary for wire.
// dxbc-spirv is linked separately under its MIT license; no upstream source copied here.
// SPDX-License-Identifier: LGPL-2.1-only
#define VK_NO_PROTOTYPES
#include <vulkan/vulkan.h>
#include "native_graphics.h"
#include "dxbc/dxbc_api.h"
#include "spirv/spirv_builder.h"
#include "ir/passes/ir_pass_lower_io.h"
#include "util/util_log.h"
#include <dlfcn.h>
#include <algorithm>
#include <array>
#include <cstring>
#include <limits>
#include <map>
#include <memory>
#include <stdexcept>
#include <string>
#include <vector>

static_assert(sizeof(WireGpuInfo) == 312, "host adapter ABI");
static_assert(sizeof(WireBinding) == 16, "host descriptor ABI");
static_assert(sizeof(WireShaderInfo) == 40, "host shader ABI (64-bit host only)");

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
  VkPhysicalDeviceMemoryProperties memory = {};
  VkPhysicalDeviceProperties properties = {};
  VkPhysicalDeviceFloatControlsProperties floats = {
    VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FLOAT_CONTROLS_PROPERTIES, nullptr };
  PFN_vkDestroyInstance destroy_instance = nullptr;
  PFN_vkDestroyDevice destroy_device = nullptr;
  PFN_vkDeviceWaitIdle wait_idle = nullptr;
  PFN_vkGetDeviceProcAddr get_device = nullptr;
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
  ci.enabledExtensionCount = portable ? 1 : 0; ci.ppEnabledExtensionNames = portable ? &portability : nullptr;
  auto create = symbol<PFN_vkCreateInstance>(get, nullptr, "vkCreateInstance");
  check(create(&ci, nullptr, &c->instance), "create instance");
  c->destroy_instance = symbol<PFN_vkDestroyInstance>(get, c->instance, "vkDestroyInstance");
  auto devices = symbol<PFN_vkEnumeratePhysicalDevices>(get, c->instance, "vkEnumeratePhysicalDevices");
  check(devices(c->instance, &count, nullptr), "physical devices");
  if (!count || count > 64) throw std::runtime_error("no usable physical device");
  std::vector<VkPhysicalDevice> physical(count);
  check(devices(c->instance, &count, physical.data()), "physical devices");
  c->physical = physical[0];
  VkPhysicalDeviceIDProperties id = { VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ID_PROPERTIES };
  c->floats.pNext = &id;
  VkPhysicalDeviceProperties2 properties = { VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2, &c->floats };
  symbol<PFN_vkGetPhysicalDeviceProperties2>(get, c->instance, "vkGetPhysicalDeviceProperties2")(c->physical, &properties);
  c->properties = properties.properties; c->floats.pNext = nullptr;
  if (c->properties.apiVersion < VK_API_VERSION_1_3) throw std::runtime_error("device Vulkan < 1.3");
  VkPhysicalDeviceVulkan12Features v12 = { VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES };
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
  VkPhysicalDeviceVulkan12Features enabled = { VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES };
  enabled.bufferDeviceAddress = VK_TRUE; enabled.vulkanMemoryModel = VK_TRUE;
  enabled.vulkanMemoryModelDeviceScope = VK_TRUE;
  enabled.shaderOutputViewportIndex = v12.shaderOutputViewportIndex;
  enabled.shaderOutputLayer = v12.shaderOutputLayer;
  VkPhysicalDeviceFeatures core = {};
  core.shaderStorageImageWriteWithoutFormat = features.features.shaderStorageImageWriteWithoutFormat;
  core.shaderStorageImageReadWithoutFormat = features.features.shaderStorageImageReadWithoutFormat;
  core.shaderClipDistance = features.features.shaderClipDistance;
  core.shaderInt64 = features.features.shaderInt64;
  core.multiViewport = features.features.multiViewport;
  float priority = 1;
  VkDeviceQueueCreateInfo q = { VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO };
  q.queueFamilyIndex = family; q.queueCount = 1; q.pQueuePriorities = &priority;
  VkDeviceCreateInfo d = { VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO, &enabled };
  d.pEnabledFeatures = &core;
  d.queueCreateInfoCount = 1; d.pQueueCreateInfos = &q;
  d.enabledExtensionCount = has_subset ? 1 : 0; d.ppEnabledExtensionNames = has_subset ? &subset : nullptr;
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
};
struct Mapping : spirv::ResourceMapping {
  uint32_t set;
  explicit Mapping(uint32_t stage) : set(stage == 1 ? 0 : stage == 2 ? 1 : 2) { }
  spirv::DescriptorBinding mapDescriptor(ir::ScalarType type, uint32_t space, uint32_t reg) override {
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
  if (stage != 1 && stage != 2 && stage != 4) throw std::runtime_error("only VS / PS / CS supported");
  LoggerCapture logger;
  dxbc::Converter::Options options;
  ir::CompileOptions legalization;
  legalization.min16Options.enableFloat16 = false; legalization.min16Options.enableInt16 = false;
  auto builder = dxbc::compileShaderToLegalizedIr(data, size, options, legalization);
  if (!builder) throw std::runtime_error("DXBC conversion failed: " + logger.messages);
  uint32_t expected = stage == 1 ? uint32_t(ir::ShaderStage::eVertex) : stage == 2 ? uint32_t(ir::ShaderStage::ePixel) : uint32_t(ir::ShaderStage::eCompute);
  uint32_t entries = 0;
  auto declarations = builder->getDeclarations();
  for (auto it = declarations.first; it != declarations.second; ++it) {
    const auto& op = *it;
    if (op.getOpCode() != ir::OpCode::eEntryPoint) continue;
    if (uint32_t(op.getOperand(op.getFirstLiteralOperandIndex())) != expected) throw std::runtime_error("shader stage mismatch");
    entries++;
  }
  if (entries != 1) throw std::runtime_error("shader entry point count");
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
  if (result->words.size() > 4 * 1024 * 1024) throw std::runtime_error("SPIR-V output capacity");
  literal_workgroup_size(result->words); reflect(*result);
  return result;
}

struct PipelineScope {
  Context& c;
  std::vector<VkShaderModule> modules;
  std::array<VkDescriptorSetLayout, 3> sets = {};
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
void build_pipeline(Context& c, PipelineScope& p, Shader *vertex, Shader *pixel, Shader *compute) {
  for (auto s : {vertex, pixel, compute})
    if (s && s->owner != &c) throw std::runtime_error("shader belongs to another GPU");
  if ((compute && (vertex || pixel)) || (!compute && (!vertex || !pixel))) throw std::runtime_error("invalid pipeline stage combination");
  if ((vertex && vertex->stage != 1) || (pixel && pixel->stage != 2) || (compute && compute->stage != 4)) throw std::runtime_error("pipeline stage mismatch");
  if (pixel) for (const auto& input : pixel->inputs) {
    auto found = vertex->outputs.find(input.first);
    if (found == vertex->outputs.end() || found->second != input.second)
      throw std::runtime_error("shader I/O location mismatch");
  }
  std::array<std::map<uint32_t,VkDescriptorSetLayoutBinding>,3> bindings;
  uint32_t push_bytes = 0; VkShaderStageFlags push_stages = 0;
  std::vector<VkPipelineShaderStageCreateInfo> stages;
  for (Shader *s : {vertex, pixel, compute}) {
    if (!s) continue;
    for (auto b : s->bindings) {
      if (b.set >= 3) throw std::runtime_error("descriptor set capacity");
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
  for (uint32_t set = 0; set < 3; set++) {
    std::vector<VkDescriptorSetLayoutBinding> entries;
    for (auto b : bindings[set]) entries.push_back(b.second);
    VkDescriptorSetLayoutCreateInfo ci = { VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO };
    ci.bindingCount = uint32_t(entries.size()); ci.pBindings = entries.data();
    check(c.fn<PFN_vkCreateDescriptorSetLayout>("vkCreateDescriptorSetLayout")(c.device, &ci, nullptr, &p.sets[set]), "descriptor layout");
  }
  VkPushConstantRange push = { push_stages, 0, push_bytes };
  VkPipelineLayoutCreateInfo layout = { VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO };
  layout.setLayoutCount = 3; layout.pSetLayouts = p.sets.data();
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
  check(c.fn<PFN_vkCreateRenderPass>("vkCreateRenderPass")(c.device, &rp, nullptr, &p.pass), "render pass");
  // Pipeline compilation only. Dummy vertex layout covers the sample's input
  // locations, and no draw/vertex read is executed by this operation.
  std::array<VkVertexInputAttributeDescription, 3> attributes;
  for (uint32_t i = 0; i < 3; i++) attributes[i] = {i, 0, VK_FORMAT_R32G32B32A32_SFLOAT, i*16};
  VkVertexInputBindingDescription binding = {0,48,VK_VERTEX_INPUT_RATE_VERTEX};
  VkPipelineVertexInputStateCreateInfo vi = { VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO };
  vi.vertexBindingDescriptionCount = 1; vi.pVertexBindingDescriptions = &binding;
  vi.vertexAttributeDescriptionCount = 3; vi.pVertexAttributeDescriptions = attributes.data();
  VkPipelineInputAssemblyStateCreateInfo ia = { VK_STRUCTURE_TYPE_PIPELINE_INPUT_ASSEMBLY_STATE_CREATE_INFO };
  ia.topology = VK_PRIMITIVE_TOPOLOGY_TRIANGLE_LIST;
  VkViewport viewport = {0,0,64,64,0,1}; VkRect2D scissor = {{0,0},{64,64}};
  VkPipelineViewportStateCreateInfo vs = { VK_STRUCTURE_TYPE_PIPELINE_VIEWPORT_STATE_CREATE_INFO };
  vs.viewportCount = 1; vs.pViewports = &viewport; vs.scissorCount = 1; vs.pScissors = &scissor;
  VkPipelineRasterizationStateCreateInfo rs = { VK_STRUCTURE_TYPE_PIPELINE_RASTERIZATION_STATE_CREATE_INFO };
  rs.polygonMode = VK_POLYGON_MODE_FILL; rs.cullMode = VK_CULL_MODE_NONE; rs.lineWidth = 1;
  VkPipelineMultisampleStateCreateInfo ms = { VK_STRUCTURE_TYPE_PIPELINE_MULTISAMPLE_STATE_CREATE_INFO };
  ms.rasterizationSamples = VK_SAMPLE_COUNT_1_BIT;
  VkPipelineColorBlendAttachmentState ba = {}; ba.colorWriteMask = 15;
  VkPipelineColorBlendStateCreateInfo bs = { VK_STRUCTURE_TYPE_PIPELINE_COLOR_BLEND_STATE_CREATE_INFO };
  bs.attachmentCount = 1; bs.pAttachments = &ba;
  VkGraphicsPipelineCreateInfo gi = { VK_STRUCTURE_TYPE_GRAPHICS_PIPELINE_CREATE_INFO };
  gi.stageCount = uint32_t(stages.size()); gi.pStages = stages.data(); gi.pVertexInputState = &vi;
  gi.pInputAssemblyState = &ia; gi.pViewportState = &vs; gi.pRasterizationState = &rs;
  gi.pMultisampleState = &ms; gi.pColorBlendState = &bs; gi.layout = p.layout; gi.renderPass = p.pass;
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
} // namespace

extern "C" {
uint32_t wire_gpu_abi() { return 1; }
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
