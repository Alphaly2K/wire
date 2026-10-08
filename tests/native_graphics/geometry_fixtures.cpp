// Original test shaders, assembled through the external MIT dxbc-spirv API.
// SPDX-License-Identifier: LGPL-2.1-only
#include "dxbc/dxbc_container.h"
#include "dxbc/dxbc_parser.h"
#include "dxbc/dxbc_signature.h"
#include <cstring>
#include <filesystem>
#include <fstream>
#include <stdexcept>
#include <vector>
using namespace dxbc_spv;
// Upstream defines this helper but does not declare it in container.h.
namespace dxbc_spv::dxbc { util::md5::Digest hashDxbcBinary(const void*,size_t); }
namespace {
using Words = std::vector<uint32_t>;
void op(Words& w, uint32_t opcode, Words args = {}, uint32_t control = 0) {
  w.push_back(opcode | control | (uint32_t(args.size()+1)<<24));
  w.insert(w.end(),args.begin(),args.end());
}
Words reg(uint32_t kind, Words indices, bool source = false, uint32_t mask = 15) {
  uint32_t select = source ? (1u<<2)|(0xe4u<<4) : mask<<4;
  Words w = {2 | select | (kind<<12) | (uint32_t(indices.size())<<20)};
  w.insert(w.end(),indices.begin(),indices.end()); return w;
}
Words scalar(uint32_t kind) { return {1|(kind<<12)}; }
void move(Words& w, Words dst, Words src) {
  dst.insert(dst.end(),src.begin(),src.end()); op(w,54,std::move(dst));
}
void write_fixture(const std::filesystem::path& path, const Words& code,
                   const dxbc::Signature& in, const dxbc::Signature& out) {
  dxbc::Parser parser(util::ByteReader(code.data(),code.size()*4));
  while (parser) { auto instruction = parser.parseInstruction();
    if (!instruction) throw std::runtime_error("fixture instruction"); }
  util::ByteWriter input_bytes, output_bytes;
  if (!in.write(input_bytes) || !out.write(output_bytes)) throw std::runtime_error("fixture signature");
  auto isgn = std::move(input_bytes).extract(), osgn = std::move(output_bytes).extract();
  // Use original code tokens: upstream serialization currently omits source
  // immediate literals. Its generic container MD5 also differs from DXBC.
  Words header = {0x43425844u,0,0,0,0,1,0,3,44,
    uint32_t(44+isgn.size()),uint32_t(44+isgn.size()+osgn.size())};
  std::vector<uint8_t> data(header.size()*4);
  std::memcpy(data.data(),header.data(),data.size());
  data.insert(data.end(),isgn.begin(),isgn.end()); data.insert(data.end(),osgn.begin(),osgn.end());
  auto code_bytes = reinterpret_cast<const uint8_t*>(code.data());
  data.insert(data.end(),code_bytes,code_bytes+code.size()*4);
  uint32_t size = uint32_t(data.size()); std::memcpy(data.data()+24,&size,4);
  auto digest = dxbc::hashDxbcBinary(data.data(),data.size());
  std::memcpy(data.data()+4,digest.data.data(),16);
  if (!dxbc::Container(data.data(),data.size()).validateHash()) throw std::runtime_error("fixture checksum");
  std::ofstream file(path,std::ios::binary); file.write(reinterpret_cast<const char*>(data.data()),data.size());
  if (!file) throw std::runtime_error("fixture write");
}
void fixture(const std::filesystem::path& path, uint32_t primitive, uint32_t vertices,
             uint32_t topology, uint32_t max_vertices, bool cuts, bool conditional,
             uint32_t instances = 1, bool overflow = false, bool cbv = false,
             bool loop = false, bool oob = false) {
  Words w;
  op(w,93,{},primitive<<11); op(w,92,{},topology<<11); op(w,94,{max_vertices});
  if (instances > 1) op(w,206,{instances});
  // v[vertex][register0] position, input primitive ID, output position, UINT IDs.
  auto input = reg(1,{vertices,0}); input.push_back(1); op(w,97,input);
  op(w,95,scalar(11));
  if (instances > 1) op(w,95,scalar(37)); // vGSInstanceID
  auto output = reg(2,{0}); output.push_back(1); op(w,103,output);
  op(w,101,reg(2,{1},false,3));
  if (cbv) op(w,89,reg(8,{0,1}));
  if (conditional) {
    // Odd primitive IDs emit; even IDs branch around the emission block.
    op(w,104,{1}); // dcl_temps
    auto args = reg(0,{0},false,1); auto id = scalar(11);
    args.insert(args.end(),id.begin(),id.end()); args.insert(args.end(),{1|(4u<<12),1}); op(w,1,args);
    op(w,31,{0x10000a,0},1u<<18); // if r0.x != 0
  }
  if (loop) {
    op(w,104,{oob ? 2u : 1u}); move(w,reg(0,{0},false,1),{0x4001,0}); op(w,48);
    op(w,80,{0x100022,0, 0x10000a,0, 0x4001,vertices}); // uge r0.y, r0.x, vertex count
    op(w,3,{0x10001a,0},1u<<18); // breakc r0.y != 0
    if (oob) op(w,30,{0x100012,1, 0x10000a,0, 0x4001,vertices}); // r1.x = index + vertex count
  }
  for (uint32_t i = 0; i < (loop ? 1u : vertices+(overflow ? 1 : 0)); i++) {
    if (loop) move(w,reg(2,{0}),{0xa01e46,0x10000a,oob ? 1u : 0u,0}); // v[r0.x/r1.x][0]
    else move(w,reg(2,{0}),reg(1,{i%vertices,0},true));
    auto src = scalar(11); move(w,reg(2,{1},false,1),src);
    if (cbv) {
      auto source = reg(8,{0,0},true); source[0] = 2|(1u<<2)|(8u<<12)|(2u<<20);
      move(w,reg(2,{1},false,2),source);
    } else if (instances > 1) move(w,reg(2,{1},false,2),scalar(37));
    else move(w,reg(2,{1},false,2),{1|(4u<<12),0});
    op(w,19);
    if (cuts && i == 1) { op(w,9); op(w,9); }
  }
  if (loop) { op(w,30,{0x100012,0, 0x10000a,0, 0x4001,1}); op(w,22); }
  if (conditional) op(w,21); // endif
  op(w,62);
  Words code = {(2u<<16)|((instances > 1 ? 5u : 4u)<<4),uint32_t(w.size()+2)};
  code.insert(code.end(),w.begin(),w.end());
  code.insert(code.begin(),{instances > 1 ? 0x58454853u : 0x52444853u,uint32_t(code.size()*4)});
  dxbc::Signature in(util::FourCC("ISGN")), out(util::FourCC("OSGN"));
  in.add(dxbc::SignatureEntry("SV_POSITION",0,0,0,0xf0f,dxbc::SignatureSysval::ePosition,ir::ScalarType::eF32));
  out.add(dxbc::SignatureEntry("SV_POSITION",0,0,0,0xf,dxbc::SignatureSysval::ePosition,ir::ScalarType::eF32));
  out.add(dxbc::SignatureEntry("TEXCOORD",0,1,0,3,dxbc::SignatureSysval::eNone,ir::ScalarType::eU32));
  write_fixture(path,code,in,out);
}
void pixel_fixture(const std::filesystem::path& path,bool cbv=false) {
  Words w; op(w,101,reg(2,{0}));
  if(cbv)op(w,89,reg(8,{0,1}));
  move(w,reg(2,{0}),cbv?reg(8,{0,0},true):Words{0x4e46,0x3e800000,0x3f000000,0x3f400000,0x3f800000}); op(w,62);
  Words code = {0x40,uint32_t(w.size()+2)}; code.insert(code.end(),w.begin(),w.end());
  code.insert(code.begin(),{0x52444853u,uint32_t(code.size()*4)});
  dxbc::Signature in(util::FourCC("ISGN")), out(util::FourCC("OSGN"));
  out.add(dxbc::SignatureEntry("SV_TARGET",0,0,0,15,dxbc::SignatureSysval::eTarget,ir::ScalarType::eF32));
  write_fixture(path,code,in,out);
}
void vertex_fixture(const std::filesystem::path& path) {
  Words w;op(w,95,reg(1,{0}));op(w,89,reg(8,{0,1}));
  auto output=reg(2,{0});output.push_back(1);op(w,103,output);
  auto args=reg(2,{0});auto input=reg(1,{0},true),cbv=reg(8,{0,0},true);
  args.insert(args.end(),input.begin(),input.end());args.insert(args.end(),cbv.begin(),cbv.end());op(w,0,args);op(w,62);
  Words code={(1u<<16)|0x40,uint32_t(w.size()+2)};code.insert(code.end(),w.begin(),w.end());
  code.insert(code.begin(),{0x52444853u,uint32_t(code.size()*4)});
  dxbc::Signature in(util::FourCC("ISGN")),out(util::FourCC("OSGN"));
  in.add(dxbc::SignatureEntry("POSITION",0,0,0,0xf0f,dxbc::SignatureSysval::eNone,ir::ScalarType::eF32));
  out.add(dxbc::SignatureEntry("SV_POSITION",0,0,0,0xf,dxbc::SignatureSysval::ePosition,ir::ScalarType::eF32));
  write_fixture(path,code,in,out);
}
void procedural_vertex_fixture(const std::filesystem::path& path) {
  // A fullscreen triangle from SV_VertexID, with no IA attributes or CBVs.
  Words w;auto input=reg(1,{0},false,1);input.push_back(6);op(w,96,input);
  auto output=reg(2,{0});output.push_back(1);op(w,103,output);op(w,104,{1});
  move(w,reg(2,{0}),{0x4e46,0xbf800000,0x3f800000,0x3f000000,0x3f800000});
  for(uint32_t id=1;id<=2;id++) {
    auto equal=reg(0,{0},false,1),source=reg(1,{0},true);
    equal.insert(equal.end(),source.begin(),source.end());equal.insert(equal.end(),{0x4001,id});op(w,32,equal);
    auto select=reg(2,{0}),condition=reg(0,{0},true);
    condition[0]&=~(0xffu<<4); // xxxx selector, one condition for all channels.
    select.insert(select.end(),condition.begin(),condition.end());
    Words position={0x4e46,id==1?0x40400000u:0xbf800000u,id==1?0x3f800000u:0xc0400000u,0x3f000000,0x3f800000};
    select.insert(select.end(),position.begin(),position.end());auto old=reg(2,{0},true);
    select.insert(select.end(),old.begin(),old.end());op(w,55,select);
  }
  op(w,62);Words code={(1u<<16)|0x40,uint32_t(w.size()+2)};code.insert(code.end(),w.begin(),w.end());
  code.insert(code.begin(),{0x52444853u,uint32_t(code.size()*4)});
  dxbc::Signature in(util::FourCC("ISGN")),out(util::FourCC("OSGN"));
  in.add(dxbc::SignatureEntry("SV_VERTEXID",0,0,0,0x101,dxbc::SignatureSysval::eVertexId,ir::ScalarType::eU32));
  out.add(dxbc::SignatureEntry("SV_POSITION",0,0,0,0xf,dxbc::SignatureSysval::ePosition,ir::ScalarType::eF32));
  write_fixture(path,code,in,out);
}
}
int main(int argc, char** argv) {
  if (argc != 2) return 2;
  std::filesystem::path out(argv[1]); std::filesystem::create_directories(out);
  fixture(out/"triangle.dxbc",3,3,5,3,false,false);
  fixture(out/"line.dxbc",2,2,3,2,false,false);
  fixture(out/"point.dxbc",1,1,1,1,false,false);
  fixture(out/"line-adjacency-cut.dxbc",6,4,3,4,true,false);
  fixture(out/"triangle-adjacency-cut.dxbc",7,6,5,6,true,false);
  fixture(out/"conditional.dxbc",3,3,5,3,false,true);
  fixture(out/"instanced.dxbc",3,3,5,3,false,false,3);
  fixture(out/"overflow.dxbc",3,3,5,3,false,false,1,true);
  fixture(out/"constant-buffer.dxbc",3,3,5,3,false,false,1,false,true);
  fixture(out/"loop.dxbc",3,3,5,3,false,false,1,false,false,true);
  fixture(out/"out-of-range.dxbc",3,3,5,3,false,false,1,false,false,true,true);
  pixel_fixture(out/"color.ps.dxbc");
  pixel_fixture(out/"constant-color.ps.dxbc",true);
  vertex_fixture(out/"offset.vs.dxbc");
  procedural_vertex_fixture(out/"procedural.vs.dxbc");
}
