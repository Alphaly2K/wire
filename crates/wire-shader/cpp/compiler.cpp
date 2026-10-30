// Wire's bounded Wasm facade for the unmodified upstream DXBC parser and IR.
#include <algorithm>
#include <array>
#include <cstring>
#include <exception>
#include <stdexcept>
#include <string>
#include <vector>
#ifdef __EMSCRIPTEN__
#include <malloc.h>
#include <emscripten/emmalloc.h>
#endif
#include "dxbc/dxbc_api.h"
#include "ir/passes/ir_pass_lower_io.h"
#include "spirv/spirv_builder.h"
#include "util/util_log.h"
using namespace dxbc_spv;
namespace {
constexpr size_t INPUT_LIMIT=1024*1024, OUTPUT_LIMIT=16*1024*1024;
std::vector<uint8_t> input;
std::vector<uint32_t> output;
std::array<char,8192> error;
size_t error_size=0;
void failure(const char* text) {
  error_size=std::min(std::strlen(text),error.size()-1);
  std::memcpy(error.data(),text,error_size);error[error_size]=0;
}
void reset() {
  std::vector<uint8_t>().swap(input);std::vector<uint32_t>().swap(output);
  error_size=0;error[0]=0;
}
uint32_t word(size_t offset) {uint32_t v;std::memcpy(&v,input.data()+offset,4);return v;}
// Same finite container checks as the existing native shader entry. The
// upstream parser remains responsible for tokens and shader semantics.
void validate() {
  auto size=input.size();
  if(size<32||size>INPUT_LIMIT||std::memcmp(input.data(),"DXBC",4)||word(20)!=1||word(24)!=size)
    throw std::runtime_error("invalid DXBC header/size");
  auto count=word(28);if(!count||count>128||size<32+count*4)
    throw std::runtime_error("invalid DXBC chunk count");
  std::array<std::pair<uint32_t,uint32_t>,128> spans={};uint32_t code=0;
  for(uint32_t i=0;i<count;i++) {
    auto offset=word(32+i*4);
    if(offset<32+count*4||offset>size-8)throw std::runtime_error("DXBC chunk bounds");
    auto length=word(offset+4);if(length>size-offset-8)throw std::runtime_error("DXBC chunk length");
    auto end=offset+8+length;
    for(uint32_t j=0;j<i;j++)if(offset<spans[j].second&&end>spans[j].first)
      throw std::runtime_error("overlapping DXBC chunks");
    spans[i]={offset,end};
    if(!std::memcmp(input.data()+offset,"SHDR",4)||!std::memcmp(input.data()+offset,"SHEX",4)) {
      if(length<8||length%4||word(offset+12)!=length/4)throw std::runtime_error("DXBC shader length");
      code++;
    }
  }
  if(code!=1)throw std::runtime_error("DXBC needs one shader chunk");
}
struct Mapping:spirv::ResourceMapping {
  uint32_t set;
  explicit Mapping(uint32_t stage):set(stage==1?0:stage==2?1:2){}
  spirv::DescriptorBinding mapDescriptor(ir::ScalarType type,uint32_t space,uint32_t reg) override {
    if(space)throw std::runtime_error("SM5.1 register spaces unsupported");
    uint32_t base=0,limit=0;
    switch(type) {
      case ir::ScalarType::eCbv:base=0;limit=14;break;
      case ir::ScalarType::eSrv:base=14;limit=128;break;
      case ir::ScalarType::eSampler:base=142;limit=16;break;
      case ir::ScalarType::eUav:base=158;limit=8;break;
      default:throw std::runtime_error("unsupported descriptor kind");
    }
    if(reg>=limit)throw std::runtime_error("D3D11 register capacity");
    return {set,base+reg};
  }
  uint32_t mapPushData(ir::ShaderStageMask stages) override {
    if(stages!=stages.first())return 0;
    return stages==ir::ShaderStage::eVertex?64:stages==ir::ShaderStage::ePixel?128:192;
  }
};
struct CaptureLogger:util::Logger {
  std::string messages;
  LogLevel getMinimumSeverity() override {return LogLevel::eWarn;}
  void message(LogLevel,const char* text) override {
    auto room=4096-messages.size();if(room)messages.append(text,std::min(room,std::strlen(text)));
  }
};
}
extern "C" {
uint32_t wire_dxbc_abi(){return 1;}
void wire_dxbc_reset(){reset();}
uint8_t* wire_dxbc_input(uint32_t size) {
  reset();if(size<32||size>INPUT_LIMIT){failure("DXBC input capacity");return nullptr;}
  try {input.resize(size);return input.data();}
  catch(const std::exception&){reset();failure("DXBC input allocation failed");return nullptr;}
}
uint32_t wire_dxbc_compile(uint32_t stage,uint32_t max_cbv_bytes,uint32_t max_cbv_count) {
  try {
    if(stage!=1&&stage!=2&&stage!=4)throw std::runtime_error("portable compiler supports VS/PS/CS; other stages need lowering");
    if(max_cbv_bytes<16||max_cbv_bytes>65536||!max_cbv_count||max_cbv_count>64)
      throw std::runtime_error("invalid shader device limits");
    validate();CaptureLogger logger;dxbc::Converter::Options options;ir::CompileOptions legal;
    legal.min16Options.enableFloat16=false;legal.min16Options.enableInt16=false;
    auto builder=dxbc::compileShaderToLegalizedIr(input.data(),input.size(),options,legal);
    if(!builder)throw std::runtime_error("DXBC conversion failed: "+logger.messages);
    auto expected=stage==1?ir::ShaderStage::eVertex:stage==2?ir::ShaderStage::ePixel:ir::ShaderStage::eCompute;
    uint32_t entries=0,draw_parameters=0;
    auto declarations=builder->getDeclarations();
    for(auto it=declarations.first;it!=declarations.second;++it) {
      const auto& op=*it;
      if(op.getOpCode()==ir::OpCode::eDclInputBuiltIn) {
        auto built_in=ir::BuiltIn(op.getOperand(1u));
        if(built_in==ir::BuiltIn::eVertexId)draw_parameters|=1;
        if(built_in==ir::BuiltIn::eInstanceId)draw_parameters|=2;
      }
      if(op.getOpCode()!=ir::OpCode::eEntryPoint)continue;
      if(uint32_t(op.getOperand(op.getFirstLiteralOperandIndex()))!=uint32_t(expected))
        throw std::runtime_error("shader stage mismatch");
      entries++;
    }
    if(entries!=1)throw std::runtime_error("shader entry point count");
    ir::LowerIoPass io(*builder);io.lowerSampleCountToSpecConstant(0);
    Mapping mapping(stage);spirv::SpirvBuilder::Options spv;
    auto internal_count=uint32_t(bool(draw_parameters&1))+uint32_t(bool(draw_parameters&2));
    spv.includeDebugNames=false;spv.maxCbvSize=max_cbv_bytes;
    spv.maxCbvCount=int32_t(std::min(14u,max_cbv_count-std::min(max_cbv_count,internal_count)));
    // Unsupported float controls use the upstream legalization, as in the
    // logical target proof. Never advertise Vulkan-only device capabilities.
    spirv::SpirvBuilder lower(*builder,mapping,spv);lower.buildSpirvBinary();
    output=lower.getSpirvBinary();
    if(output.size()>OUTPUT_LIMIT/4)throw std::runtime_error("SPIR-V output capacity");
    std::vector<uint8_t>().swap(input);return 0;
  } catch(const std::exception& e) {
    // Preserve the diagnostic before destroying temporary ownership. This
    // fixed error buffer does not allocate when the arena is exhausted.
    std::array<char,8192> saved={};std::strncpy(saved.data(),e.what(),saved.size()-1);
    reset();failure(saved.data());return 1;
  }
}
const uint32_t* wire_dxbc_output(){return output.data();}
uint32_t wire_dxbc_output_size(){return uint32_t(output.size()*4);}
const char* wire_dxbc_error(){return error.data();}
uint32_t wire_dxbc_error_size(){return uint32_t(error_size);}
uint32_t wire_dxbc_owned_bytes(){return uint32_t(input.capacity()+output.capacity()*4);}
uint32_t wire_dxbc_heap_used(){
#ifdef __EMSCRIPTEN__
  return uint32_t(emmalloc_mallinfo().uordblks);
#else
  return wire_dxbc_owned_bytes();
#endif
}
}
