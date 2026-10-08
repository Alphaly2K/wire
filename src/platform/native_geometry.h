// Independently implemented GS -> compute lowering using dxbc-spirv's IR API.
// SPDX-License-Identifier: LGPL-2.1-only
#pragma once
#include "native_graphics.h"
#include "ir/ir_builder.h"
#include <map>
#include <stdexcept>
#include <vector>

namespace wire_geometry {
using namespace dxbc_spv::ir;
struct Geometry {
  std::vector<WireGeometryIo> inputs, outputs;
  uint32_t input_vertices = 0, output_primitive = UINT32_MAX;
  uint32_t max_vertices = 0, instances = 0;
  uint32_t input_words = 0, vertex_words = 0, invocation_words = 0;
};
// Invoke after upstream legalization: all loads/stores have concrete types.
// Each workgroup executes one original GS invocation, with no shared state.
inline Geometry lower(Builder& b, SsaDef entry) {
  Geometry g;
  std::vector<SsaDef> original, modes, declarations;
  std::map<SsaDef,WireGeometryIo> inputs, outputs;
  for (const auto& op : b) original.push_back(op.getDef());
  for (auto def : original) {
    const auto& op = b.getOp(def);
    switch (op.getOpCode()) {
      case OpCode::eSetGsInstances: g.instances = uint32_t(op.getOperand(1)); modes.push_back(def); break;
      case OpCode::eSetGsInputPrimitive: {
        auto p = uint32_t(op.getOperand(1));
        if (p > 4) throw std::runtime_error("GS patch input not implemented");
        g.input_vertices = primitiveVertexCount(PrimitiveType(p)); modes.push_back(def); break;
      }
      case OpCode::eSetGsOutputVertices: g.max_vertices = uint32_t(op.getOperand(1)); modes.push_back(def); break;
      case OpCode::eSetGsOutputPrimitive:
        g.output_primitive = uint32_t(op.getOperand(1));
        // SM4 uses implicit stream 0, represented by a zero explicit mask.
        if (uint32_t(op.getOperand(2)) > 1) throw std::runtime_error("GS multiple streams not implemented");
        modes.push_back(def); break;
      default: break;
    }
  }
  if (!g.input_vertices || !g.max_vertices || g.max_vertices > 1024 ||
      !g.instances || g.instances > 32 ||
      (g.output_primitive != 0 && g.output_primitive != 1 && g.output_primitive != 3))
    throw std::runtime_error("GS declaration limits");
  for (auto def : original) {
    const auto& op = b.getOp(def);
    bool in = op.getOpCode() == OpCode::eDclInput || op.getOpCode() == OpCode::eDclInputBuiltIn;
    bool out = op.getOpCode() == OpCode::eDclOutput || op.getOpCode() == OpCode::eDclOutputBuiltIn;
    if (!in && !out) continue;
    if (SsaDef(op.getOperand(0)) != entry) throw std::runtime_error("GS I/O entry point");
    auto type = op.getType();
    auto scalar = type.getBaseType(0).getBaseType();
    if (type.getStructMemberCount() != 1 ||
        (scalar != ScalarType::eF32 && scalar != ScalarType::eI32 && scalar != ScalarType::eU32) ||
        type.getArrayDimensions() > 2 || (type.isArrayType() && !type.isSizedArray()))
      throw std::runtime_error("GS I/O type not implemented");
    bool builtin = op.getOpCode() == OpCode::eDclInputBuiltIn || op.getOpCode() == OpCode::eDclOutputBuiltIn;
    uint32_t id = builtin ? uint32_t(op.getOperand(1)) : UINT32_MAX;
    bool generated = in && (id == uint32_t(BuiltIn::ePrimitiveId) ||
      id == uint32_t(BuiltIn::eGsInstanceId) || id == uint32_t(BuiltIn::eGsVertexCountIn));
    if (generated && (!type.isScalarType() || scalar != ScalarType::eU32))
      throw std::runtime_error("GS generated input type");
    if (in && !generated && (!type.isArrayType() ||
        type.getArraySize(type.getArrayDimensions()-1) != g.input_vertices))
      throw std::runtime_error("GS per-vertex input dimension");
    if (out) {
      auto stream = uint32_t(op.getOperand(builtin ? 2 : 3));
      if (stream) throw std::runtime_error("GS output stream not implemented");
    }
    uint32_t words = type.computeFlattenedScalarCount();
    if (!words || words > 768) throw std::runtime_error("GS I/O capacity");
    WireGeometryIo io = {builtin ? UINT32_MAX : uint32_t(op.getOperand(1)),
      builtin ? 0u : uint32_t(op.getOperand(2)), id, uint32_t(scalar), words,
      generated ? UINT32_MAX : in ? g.input_words : g.vertex_words,
      type.getBaseType(0).getVectorSize(),type.getArrayDimensions()};
    if (in) {
      inputs[def] = io;
      if (!generated) { g.inputs.push_back(io); g.input_words += words; }
    } else {
      outputs[def] = io; g.outputs.push_back(io); g.vertex_words += words;
    }
    declarations.push_back(def);
  }
  if (g.input_words > 768 || g.vertex_words > 128 || g.max_vertices*g.vertex_words > 1024)
    throw std::runtime_error("GS scalar output limits");
  size_t emits = 0;
  for (auto def : original) emits += b.getOp(def).getOpCode() == OpCode::eEmitVertex;
  if (original.size() > 262144 || emits > (262144-original.size())/(40+g.vertex_words*8))
    throw std::runtime_error("GS lowering instruction capacity");
  g.invocation_words = 1+(g.max_vertices+1)*(1+g.vertex_words);
  auto old_entry = b.getOp(entry);
  b.rewriteOp(entry, Op::EntryPoint(SsaDef(old_entry.getOperand(0)),ShaderStage::eCompute));
  for (auto def : modes) b.remove(def);
  b.add(Op::SetCsWorkgroupSize(entry,1,1,1));
  auto group = b.add(Op::DclInputBuiltIn(Type(ScalarType::eU32,3),entry,BuiltIn::eWorkgroupId));
  auto raw = Type(ScalarType::eU32).addArrayDimension(0);
  // Internal descriptors live outside the 8 guest UAV registers.
  auto input = b.add(Op::DclUav(raw,entry,1,0,1,ResourceKind::eBufferRaw,UavFlag::eReadOnly));
  auto output = b.add(Op::DclUav(raw,entry,1,1,1,ResourceKind::eBufferRaw,UavFlag::eWriteOnly));
  auto count = b.add(Op::DclScratch(ScalarType::eU32,entry));
  auto cut = b.add(Op::DclScratch(ScalarType::eU32,entry));
  for (auto [def, io] : outputs) {
    (void)io;
    b.rewriteOp(def,Op::DclScratch(b.getOp(def).getType(),entry));
  }
  // Per-use descriptor and ID loads also work inside original helper functions.
  auto group_component = [&](uint32_t n) {
    return b.add(Op::InputLoad(ScalarType::eU32,group,b.makeConstant(n)));
  };
  auto add = [&](SsaDef x,SsaDef y) { return b.add(Op::IAdd(ScalarType::eU32,x,y)); };
  auto mul = [&](SsaDef x,uint32_t n) { return b.add(Op::IMul(ScalarType::eU32,x,b.makeConstant(n))); };
  auto base_out = [&]() {
    return mul(add(mul(group_component(0),g.instances),group_component(1)),g.invocation_words);
  };
  auto descriptor = [&](SsaDef d) { return b.add(Op::DescriptorLoad(ScalarType::eUav,d,b.makeConstant(0u))); };
  auto store = [&](SsaDef address,SsaDef value) { b.add(Op::BufferStore(descriptor(output),address,value,4)); };
  // Initialize only in the entry function, immediately after its first label.
  auto function = SsaDef(old_entry.getOperand(0));
  auto label = b.getNext(function);
  while (label && b.getOp(label).getOpCode() != OpCode::eLabel) label = b.getNext(label);
  if (!label) throw std::runtime_error("GS entry block missing");
  b.setCursor(label);
  b.add(Op::ScratchStore(count,{},b.makeConstant(0u)));
  b.add(Op::ScratchStore(cut,{},b.makeConstant(1u)));
  store(base_out(),b.makeConstant(0u));
  for (auto def : original) {
    auto op = b.getOp(def);
    auto code = op.getOpCode();
    b.setCursor(b.getPrev(def));
    if (code == OpCode::eInputLoad) {
      auto decl = SsaDef(op.getOperand(0));
      auto found = inputs.find(decl);
      if (found == inputs.end()) continue;
      auto io = found->second;
      if (!op.getType().isBasicType()) throw std::runtime_error("GS aggregate load not implemented");
      SsaDef value;
      if (io.word_offset == UINT32_MAX) {
        value = io.built_in == uint32_t(BuiltIn::ePrimitiveId) ? group_component(0) :
          io.built_in == uint32_t(BuiltIn::eGsInstanceId) ? group_component(1) : b.makeConstant(g.input_vertices);
      } else {
        auto offset = b.makeConstant(io.word_offset);
        auto type = b.getOp(decl).getType();
        auto address = SsaDef(op.getOperand(1));
        auto bounds = b.makeConstant(true);
        uint32_t indices = address ? b.getOp(address).getType().computeFlattenedScalarCount() : 0;
        for (uint32_t i = 0; i < indices; i++) {
          auto address_scalar = b.getOp(address).getType().getBaseType(0).getBaseType();
          auto index = indices == 1 ? address : b.add(Op::CompositeExtract(address_scalar,address,b.makeConstant(i)));
          if (b.getOp(index).getType() != Type(ScalarType::eU32)) index = b.add(Op::Cast(ScalarType::eU32,index));
          auto valid = b.add(Op::ULt(ScalarType::eBool,index,b.makeConstant(type.computeTopLevelMemberCount())));
          bounds = b.add(Op::Select(ScalarType::eBool,valid,bounds,b.makeConstant(false)));
          index = b.add(Op::Select(ScalarType::eU32,valid,index,b.makeConstant(0u)));
          type = type.getSubType(0);
          offset = add(offset,mul(index,type.byteSize()/4));
        }
        if (!type.isBasicType() || type.byteSize() != op.getType().byteSize())
          throw std::runtime_error("GS input scalar/vector address");
        offset = add(mul(group_component(0),g.input_words),offset);
        auto bits = Type(ScalarType::eU32,op.getType().getBaseType(0).getVectorSize());
        value = b.add(Op::BufferLoad(bits,descriptor(input),offset,4));
        if (op.getType() != bits) value = b.add(Op::Cast(op.getType(),value));
        value = b.add(Op::Select(op.getType(),bounds,value,b.makeConstantZero(op.getType())));
      }
      b.rewriteDef(def,value);
    } else if (code == OpCode::eOutputStore || code == OpCode::eOutputLoad) {
      auto decl = SsaDef(op.getOperand(0));
      if (!outputs.count(decl)) throw std::runtime_error("GS output declaration");
      if (code == OpCode::eOutputStore)
        b.rewriteOp(def,Op::ScratchStore(decl,SsaDef(op.getOperand(1)),SsaDef(op.getOperand(2))));
      else b.rewriteOp(def,Op::ScratchLoad(op.getType(),decl,SsaDef(op.getOperand(1))));
    } else if (code == OpCode::eEmitVertex) {
      if (uint32_t(op.getOperand(0))) throw std::runtime_error("GS emit stream not implemented");
      auto n = b.add(Op::ScratchLoad(ScalarType::eU32,count,{}));
      auto limit = b.makeConstant(g.max_vertices);
      auto valid = b.add(Op::ULt(ScalarType::eBool,n,limit));
      auto slot = b.add(Op::Select(ScalarType::eU32,valid,n,limit));
      auto base = add(base_out(),add(b.makeConstant(1u),mul(slot,g.vertex_words+1)));
      store(base,b.add(Op::ScratchLoad(ScalarType::eU32,cut,{})));
      for (auto [decl, io] : outputs) {
        auto t = b.getOp(decl).getType();
        for (uint32_t i = 0; i < io.array_size; i++) {
          auto type = t;
          uint32_t remainder = i;
          std::vector<SsaDef> path;
          while (!type.isScalarType()) {
            auto sub = type.getSubType(0);
            auto stride = sub.computeFlattenedScalarCount();
            path.push_back(b.makeConstant(remainder/stride)); remainder %= stride; type = sub;
          }
          SsaDef address;
          if (path.size() == 1) address = path[0];
          else if (!path.empty()) {
            auto composite = Op::CompositeConstruct(Type(ScalarType::eU32,uint32_t(path.size())));
            for (auto part : path) composite.addOperand(part);
            address = b.add(std::move(composite));
          }
          auto value = b.add(Op::ScratchLoad(type,decl,address));
          if (type != Type(ScalarType::eU32)) value = b.add(Op::Cast(ScalarType::eU32,value));
          store(add(base,b.makeConstant(1+io.word_offset+i)),value);
        }
      }
      // Clamp overflow to one extra sink slot; reject it after GPU completion.
      auto next = b.add(Op::Select(ScalarType::eU32,valid,add(n,b.makeConstant(1u)),b.makeConstant(g.max_vertices+1)));
      b.add(Op::ScratchStore(count,{},next)); store(base_out(),next);
      b.add(Op::ScratchStore(cut,{},b.makeConstant(0u)));
      b.remove(def);
    } else if (code == OpCode::eEmitPrimitive) {
      if (uint32_t(op.getOperand(0))) throw std::runtime_error("GS cut stream not implemented");
      b.add(Op::ScratchStore(cut,{},b.makeConstant(1u))); b.remove(def);
    } else if ((code == OpCode::eSemantic || code == OpCode::eDebugName) &&
        (inputs.count(SsaDef(op.getOperand(0))) || outputs.count(SsaDef(op.getOperand(0))))) {
      b.remove(def);
    } else if (code == OpCode::eDclXfb) {
      throw std::runtime_error("GS transform feedback declaration not implemented");
    }
  }
  for (auto def : declarations) if (inputs.count(def)) b.remove(def);
  b.resetCursor();
  return g;
}
} // namespace wire_geometry
