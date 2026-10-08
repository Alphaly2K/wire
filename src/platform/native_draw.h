// Independently implemented D3D11 VS/PS draws into owned Vulkan attachments.
// SPDX-License-Identifier: LGPL-2.1-only
float draw_float(uint32_t bits) {float f;std::memcpy(&f,&bits,4);return f;}
VkCompareOp draw_compare(uint32_t value) {
  if(value<1 || value>8)throw std::runtime_error("draw comparison value");
  return VkCompareOp(value-1);
}
VkStencilOp draw_stencil(uint32_t value) {
  static const VkStencilOp ops[]={VK_STENCIL_OP_KEEP,VK_STENCIL_OP_ZERO,VK_STENCIL_OP_REPLACE,
    VK_STENCIL_OP_INCREMENT_AND_CLAMP,VK_STENCIL_OP_DECREMENT_AND_CLAMP,VK_STENCIL_OP_INVERT,
    VK_STENCIL_OP_INCREMENT_AND_WRAP,VK_STENCIL_OP_DECREMENT_AND_WRAP};
  if(value<1 || value>8)throw std::runtime_error("draw stencil operation");return ops[value-1];
}
VkBlendFactor draw_blend(uint32_t value,bool alpha) {
  switch(value) {
    case 1:return VK_BLEND_FACTOR_ZERO;case 2:return VK_BLEND_FACTOR_ONE;
    case 3:return alpha?VK_BLEND_FACTOR_SRC_ALPHA:VK_BLEND_FACTOR_SRC_COLOR;
    case 4:return alpha?VK_BLEND_FACTOR_ONE_MINUS_SRC_ALPHA:VK_BLEND_FACTOR_ONE_MINUS_SRC_COLOR;
    case 5:return VK_BLEND_FACTOR_SRC_ALPHA;case 6:return VK_BLEND_FACTOR_ONE_MINUS_SRC_ALPHA;
    case 7:return VK_BLEND_FACTOR_DST_ALPHA;case 8:return VK_BLEND_FACTOR_ONE_MINUS_DST_ALPHA;
    case 9:return alpha?VK_BLEND_FACTOR_DST_ALPHA:VK_BLEND_FACTOR_DST_COLOR;
    case 10:return alpha?VK_BLEND_FACTOR_ONE_MINUS_DST_ALPHA:VK_BLEND_FACTOR_ONE_MINUS_DST_COLOR;
    case 11:return VK_BLEND_FACTOR_SRC_ALPHA_SATURATE;
    case 14:return alpha?VK_BLEND_FACTOR_CONSTANT_ALPHA:VK_BLEND_FACTOR_CONSTANT_COLOR;
    case 15:return alpha?VK_BLEND_FACTOR_ONE_MINUS_CONSTANT_ALPHA:VK_BLEND_FACTOR_ONE_MINUS_CONSTANT_COLOR;
    default:throw std::runtime_error("dual-source/unknown blend factor unsupported");
  }
}
VkBlendOp draw_blend_op(uint32_t value) {
  if(value<1 || value>5)throw std::runtime_error("draw blend operation");return VkBlendOp(value-1);
}
VkSamplerAddressMode draw_address(uint32_t value) {
  switch(value) {case 1:return VK_SAMPLER_ADDRESS_MODE_REPEAT;case 2:return VK_SAMPLER_ADDRESS_MODE_MIRRORED_REPEAT;
    case 3:return VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE;case 4:return VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_BORDER;
    default:throw std::runtime_error("sampler mirror-once/unknown address unsupported");}
}
struct DrawSamplers {
  Context& c;std::vector<VkSampler> handles;
  ~DrawSamplers(){for(auto s:handles)c.fn<PFN_vkDestroySampler>("vkDestroySampler")(c.device,s,nullptr);}
  VkSampler make(const uint32_t *d) {
    if(d[0]&~0xd5u || ((d[0]&0x40) && (d[0]&0x15)!=0x15))throw std::runtime_error("sampler reduction/filter unsupported");
    VkSamplerCreateInfo s={VK_STRUCTURE_TYPE_SAMPLER_CREATE_INFO};
    s.minFilter=(d[0]&0x10)?VK_FILTER_LINEAR:VK_FILTER_NEAREST;s.magFilter=(d[0]&4)?VK_FILTER_LINEAR:VK_FILTER_NEAREST;
    s.mipmapMode=(d[0]&1)?VK_SAMPLER_MIPMAP_MODE_LINEAR:VK_SAMPLER_MIPMAP_MODE_NEAREST;
    s.addressModeU=draw_address(d[1]);s.addressModeV=draw_address(d[2]);s.addressModeW=draw_address(d[3]);
    s.mipLodBias=draw_float(d[4]);s.anisotropyEnable=d[0]&0x40;s.maxAnisotropy=float(d[5]);
    s.compareEnable=d[0]&0x80;s.compareOp=draw_compare(d[6]);s.minLod=std::max(0.f,draw_float(d[11]));s.maxLod=draw_float(d[12]);
    if(!std::isfinite(s.mipLodBias) || std::abs(s.mipLodBias)>c.properties.limits.maxSamplerLodBias ||
       !std::isfinite(s.minLod) || !std::isfinite(s.maxLod) || s.maxLod<s.minLod ||
       (s.anisotropyEnable && (!c.features.samplerAnisotropy || !d[5] || s.maxAnisotropy>c.properties.limits.maxSamplerAnisotropy)))
      throw std::runtime_error("sampler LOD/anisotropy unsupported");
    if(d[1]==4 || d[2]==4 || d[3]==4) {
      std::array<float,4> color={draw_float(d[7]),draw_float(d[8]),draw_float(d[9]),draw_float(d[10])};
      if(color==std::array<float,4>{0,0,0,0})s.borderColor=VK_BORDER_COLOR_FLOAT_TRANSPARENT_BLACK;
      else if(color==std::array<float,4>{0,0,0,1})s.borderColor=VK_BORDER_COLOR_FLOAT_OPAQUE_BLACK;
      else if(color==std::array<float,4>{1,1,1,1})s.borderColor=VK_BORDER_COLOR_FLOAT_OPAQUE_WHITE;
      else throw std::runtime_error("custom sampler border color unsupported");
    }
    VkSampler result;check(c.fn<PFN_vkCreateSampler>("vkCreateSampler")(c.device,&s,nullptr,&result),"draw sampler");
    handles.push_back(result);return result;
  }
};
void draw(Context& c,const WireDraw& d) {
  auto vs=static_cast<Shader*>(d.vertex),ps=static_cast<Shader*>(d.pixel);
  auto target=static_cast<ImageView*>(d.target),depth=static_cast<ImageView*>(d.depth);
  if(!vs || !ps || !target || target->desc.kind!=1 || &target->t.c!=&c || target->desc.layers!=1 ||
     !d.count || !d.instances || d.count>16*1024*1024 || d.instances>65536 || d.attribute_count>32 ||
     (d.attribute_count && !d.attributes) || d.binding_count>316 || (d.binding_count && !d.bindings))
    throw std::runtime_error("draw stages/target/count/capacity unsupported");
  if(vs->stage!=1 || ps->stage!=2 || vs->owner!=&c || ps->owner!=&c || vs->push_bytes || ps->push_bytes)
    throw std::runtime_error("draw shader stage/device/push-data unsupported");
  if(!vs->sampled_rgba2d || !ps->sampled_rgba2d)throw std::runtime_error("draw sampled-image shape unsupported");
  uint32_t width=std::max(1u,target->t.desc.width>>target->desc.mip),height=std::max(1u,target->t.desc.height>>target->desc.mip);
  if(depth && (depth->desc.kind!=2 || &depth->t.c!=&c || depth->desc.layers!=1 ||
    std::max(1u,depth->t.desc.width>>depth->desc.mip)!=width || std::max(1u,depth->t.desc.height>>depth->desc.mip)!=height))
    throw std::runtime_error("draw depth attachment/device/extent mismatch");
  VkPrimitiveTopology topology;
  switch(d.topology) {case 1:topology=VK_PRIMITIVE_TOPOLOGY_POINT_LIST;break;case 2:topology=VK_PRIMITIVE_TOPOLOGY_LINE_LIST;break;
    case 3:topology=VK_PRIMITIVE_TOPOLOGY_LINE_STRIP;break;case 4:topology=VK_PRIMITIVE_TOPOLOGY_TRIANGLE_LIST;break;
    case 5:topology=VK_PRIMITIVE_TOPOLOGY_TRIANGLE_STRIP;break;default:throw std::runtime_error("draw topology unsupported");}
  uint64_t max_vertex=uint64_t(d.first)+d.count-1;
  auto index=static_cast<Buffer*>(d.index);
  if(d.indexed) {
    uint32_t bytes=d.index_format==57?2:d.index_format==42?4:0;
    uint64_t at=uint64_t(d.index_offset)+uint64_t(d.first)*bytes,end=at+uint64_t(d.count)*bytes;
    if(!index || !bytes || &index->c!=&c || !(index->usage&8) || d.index_offset%bytes || end>index->size)
      throw std::runtime_error("draw index buffer/device/range");
    max_vertex=0;
    bool strip=d.topology==3 || d.topology==5;
    for(uint32_t i=0;i<d.count;i++) {
      uint32_t value=0;std::memcpy(&value,static_cast<uint8_t*>(index->mapped)+at+uint64_t(i)*bytes,bytes);
      if(strip && value==(bytes==2?0xffffu:0xffffffffu))continue;
      int64_t vertex=int64_t(value)+d.base_vertex;
      if(vertex<0)throw std::runtime_error("negative indexed vertex unsupported");
      max_vertex=std::max(max_vertex,uint64_t(vertex));
    }
  }
  GraphicsConfig graphics={};graphics.raster.sType=VK_STRUCTURE_TYPE_PIPELINE_RASTERIZATION_STATE_CREATE_INFO;
  graphics.depth.sType=VK_STRUCTURE_TYPE_PIPELINE_DEPTH_STENCIL_STATE_CREATE_INFO;
  std::vector<VkVertexInputAttributeDescription> attributes;
  std::map<uint32_t,std::pair<uint32_t,uint32_t>> slots;
  std::set<uint32_t> locations;
  for(size_t i=0;i<d.attribute_count;i++) {
    const auto& a=d.attributes[i];if(a.slot>=32 || a.input_class>1 || a.step>1 || !locations.insert(a.location).second)
      throw std::runtime_error("draw input slot/class/divisor/location unsupported");
    auto f=dxgi_format(a.format);VkFormatProperties fp={};c.format_properties(c.physical,f,&fp);
    if(f==VK_FORMAT_UNDEFINED || !(fp.bufferFeatures&VK_FORMAT_FEATURE_VERTEX_BUFFER_BIT))throw std::runtime_error("draw vertex format unsupported");
    auto b=static_cast<Buffer*>(d.vertices[a.slot].buffer);
    uint32_t stride=(a.input_class && !a.step)?0:d.vertices[a.slot].stride;
    auto declaration=std::make_pair(stride,a.input_class);
    if(slots.count(a.slot) && slots[a.slot]!=declaration)throw std::runtime_error("mixed vertex rates in one slot");
    slots[a.slot]=declaration;
    uint64_t last=a.input_class?uint64_t(d.first_instance)+d.instances-1:max_vertex;
    Texture format_size(c,{1,1,1,1,a.format,0});
    uint64_t end=uint64_t(d.vertices[a.slot].offset)+uint64_t(stride)*last+a.offset+format_size.bytes;
    if(!b || &b->c!=&c || !(b->usage&4) || stride>c.properties.limits.maxVertexInputBindingStride ||
       a.offset>c.properties.limits.maxVertexInputAttributeOffset || end>b->size)
      throw std::runtime_error("draw vertex buffer/device/range");
    attributes.push_back({a.location,a.slot,f,a.offset});
  }
  for(const auto& input:vs->inputs)if(!locations.count(input.first.first) || input.first.second)
    throw std::runtime_error("draw input layout does not cover shader locations/components");
  for(auto [slot,rate]:slots)graphics.bindings.push_back({slot,rate.first,rate.second?VK_VERTEX_INPUT_RATE_INSTANCE:VK_VERTEX_INPUT_RATE_VERTEX});
  const auto r=d.raster;
  if(r[0]<2 || r[0]>3 || r[1]<1 || r[1]>3 || r[2]>1 || r[6]>1 || r[7]>1 || r[8]>1 || r[9]>1 ||
     ((d.topology==2 || d.topology==3) && (r[8] || r[9])) ||
     (r[0]==2 && !c.features.fillModeNonSolid) || (!r[6] && !c.features.depthClamp) || r[3] || draw_float(r[4])!=0 || draw_float(r[5])!=0)
    throw std::runtime_error("draw raster fill/cull/depth-bias/multisample state unsupported");
  graphics.raster.polygonMode=r[0]==3?VK_POLYGON_MODE_FILL:VK_POLYGON_MODE_LINE;
  graphics.raster.cullMode=r[1]==1?VK_CULL_MODE_NONE:r[1]==2?VK_CULL_MODE_FRONT_BIT:VK_CULL_MODE_BACK_BIT;
  graphics.raster.frontFace=r[2]?VK_FRONT_FACE_COUNTER_CLOCKWISE:VK_FRONT_FACE_CLOCKWISE;
  graphics.raster.depthClampEnable=!r[6];graphics.raster.lineWidth=1;
  for(float v:d.viewport)if(!std::isfinite(v))throw std::runtime_error("nonfinite draw viewport");
  if(d.viewport[2]<=0 || d.viewport[3]<=0 || d.viewport[2]>c.properties.limits.maxViewportDimensions[0] ||
    d.viewport[3]>c.properties.limits.maxViewportDimensions[1] || d.viewport[4]<0 || d.viewport[5]>1 || d.viewport[4]>d.viewport[5] ||
    d.viewport[0]<c.properties.limits.viewportBoundsRange[0] || d.viewport[1]<c.properties.limits.viewportBoundsRange[0] ||
    d.viewport[0]+d.viewport[2]>c.properties.limits.viewportBoundsRange[1] || d.viewport[1]+d.viewport[3]>c.properties.limits.viewportBoundsRange[1])
    throw std::runtime_error("draw viewport range unsupported");
  graphics.viewport={d.viewport[0],d.viewport[1]+d.viewport[3],d.viewport[2],-d.viewport[3],d.viewport[4],d.viewport[5]};
  int32_t left=0,top=0,right=int32_t(width),bottom=int32_t(height);
  if(r[7]) {left=std::clamp(d.scissor[0],0,right);top=std::clamp(d.scissor[1],0,bottom);
    right=std::clamp(d.scissor[2],left,right);bottom=std::clamp(d.scissor[3],top,bottom);}
  graphics.scissor={{left,top},{uint32_t(right-left),uint32_t(bottom-top)}};
  auto z=d.depth_state;if(z[0]>1 || z[1]>1 || z[3]>1)throw std::runtime_error("draw depth BOOL/mask");
  graphics.depth.depthTestEnable=depth && z[0];graphics.depth.depthWriteEnable=depth && z[1];graphics.depth.depthCompareOp=draw_compare(z[2]);
  graphics.depth.stencilTestEnable=depth && z[3];
  if(graphics.depth.stencilTestEnable && !(depth->t.aspect&VK_IMAGE_ASPECT_STENCIL_BIT))throw std::runtime_error("draw stencil aspect absent");
  for(uint32_t face=0;face<2;face++) {
    auto& s=face?graphics.depth.back:graphics.depth.front;auto at=5+face*4;
    s.failOp=draw_stencil(z[at]);s.depthFailOp=draw_stencil(z[at+1]);s.passOp=draw_stencil(z[at+2]);s.compareOp=draw_compare(z[at+3]);
    s.compareMask=z[4]&255;s.writeMask=(z[4]>>8)&255;s.reference=d.stencil_ref&255;
  }
  auto b=d.blend;if(b[0]>1 || b[1]>1 || b[2]>1 || (b[9]&255)>15)throw std::runtime_error("draw blend BOOL/mask");
  graphics.blend.blendEnable=b[2];graphics.blend.colorWriteMask=b[9]&255;
  // Disabled blending does not consume uninitialized blend-factor fields.
  if(b[2]) {graphics.blend.srcColorBlendFactor=draw_blend(b[3],false);graphics.blend.dstColorBlendFactor=draw_blend(b[4],false);
    graphics.blend.colorBlendOp=draw_blend_op(b[5]);graphics.blend.srcAlphaBlendFactor=draw_blend(b[6],true);
    graphics.blend.dstAlphaBlendFactor=draw_blend(b[7],true);graphics.blend.alphaBlendOp=draw_blend_op(b[8]);}
  std::copy(d.blend_factor,d.blend_factor+4,graphics.blend_factor.begin());
  for(float f:graphics.blend_factor)if(!std::isfinite(f))throw std::runtime_error("draw blend factor nonfinite");
  graphics.sample_mask=d.sample_mask;graphics.alpha_to_coverage=b[0];
  PipelineScope pipeline(c);
  std::array<VkAttachmentDescription,2> attachments={};std::array<VkImageView,2> views={target->handle,depth?depth->handle:VK_NULL_HANDLE};
  for(uint32_t i=0;i<(depth?2u:1u);i++) {
    auto& a=attachments[i];a.format=dxgi_format(i?depth->desc.format:target->desc.format);a.samples=VK_SAMPLE_COUNT_1_BIT;
    a.loadOp=a.stencilLoadOp=VK_ATTACHMENT_LOAD_OP_LOAD;a.storeOp=a.stencilStoreOp=VK_ATTACHMENT_STORE_OP_STORE;
    a.initialLayout=a.finalLayout=VK_IMAGE_LAYOUT_GENERAL;
  }
  VkAttachmentReference color={0,VK_IMAGE_LAYOUT_GENERAL},ds={1,VK_IMAGE_LAYOUT_GENERAL};
  VkSubpassDescription sub={};sub.pipelineBindPoint=VK_PIPELINE_BIND_POINT_GRAPHICS;sub.colorAttachmentCount=1;
  sub.pColorAttachments=&color;sub.pDepthStencilAttachment=depth?&ds:nullptr;
  VkRenderPassCreateInfo pass={VK_STRUCTURE_TYPE_RENDER_PASS_CREATE_INFO};pass.attachmentCount=depth?2:1;pass.pAttachments=attachments.data();
  pass.subpassCount=1;pass.pSubpasses=&sub;
  check(c.fn<PFN_vkCreateRenderPass>("vkCreateRenderPass")(c.device,&pass,nullptr,&pipeline.pass),"draw render pass");graphics.pass=pipeline.pass;
  build_pipeline(c,pipeline,vs,ps,nullptr,&attributes,0,topology,&graphics);
  Commands commands(c);DrawSamplers samplers{c,{}};
  VkFramebufferCreateInfo fb={VK_STRUCTURE_TYPE_FRAMEBUFFER_CREATE_INFO};fb.renderPass=pipeline.pass;fb.attachmentCount=pass.attachmentCount;
  fb.pAttachments=views.data();fb.width=width;fb.height=height;fb.layers=1;
  check(c.fn<PFN_vkCreateFramebuffer>("vkCreateFramebuffer")(c.device,&fb,nullptr,&commands.scope.framebuffer),"draw framebuffer");
  std::map<std::pair<uint32_t,uint32_t>,const WireDrawBinding*> resources;
  for(size_t i=0;i<d.binding_count;i++)if(!resources.emplace(std::make_pair(d.bindings[i].set,d.bindings[i].binding),&d.bindings[i]).second)
    throw std::runtime_error("duplicate draw descriptor");
  std::vector<WireBinding> reflected=vs->bindings;reflected.insert(reflected.end(),ps->bindings.begin(),ps->bindings.end());
  if(resources.size()!=reflected.size())throw std::runtime_error("draw descriptors differ from shader reflection");
  std::map<VkDescriptorType,uint32_t> totals;
  for(auto binding:reflected) {if(binding.count!=1)throw std::runtime_error("draw descriptor arrays unsupported");totals[VkDescriptorType(binding.descriptor_type)]++;}
  std::array<VkDescriptorSet,4> sets={};
  if(!reflected.empty()) {
    std::vector<VkDescriptorPoolSize> sizes;for(auto [type,n]:totals)sizes.push_back({type,n});
    VkDescriptorPoolCreateInfo pool={VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO};pool.maxSets=4;pool.poolSizeCount=uint32_t(sizes.size());pool.pPoolSizes=sizes.data();
    check(c.fn<PFN_vkCreateDescriptorPool>("vkCreateDescriptorPool")(c.device,&pool,nullptr,&commands.scope.descriptors),"draw descriptor pool");
    VkDescriptorSetAllocateInfo allocate={VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO};allocate.descriptorPool=commands.scope.descriptors;
    allocate.descriptorSetCount=4;allocate.pSetLayouts=pipeline.sets.data();
    check(c.fn<PFN_vkAllocateDescriptorSets>("vkAllocateDescriptorSets")(c.device,&allocate,sets.data()),"draw descriptor sets");
  }
  for(auto binding:reflected) {
    auto found=resources.find({binding.set,binding.binding});if(found==resources.end())throw std::runtime_error("draw descriptor absent");
    const auto& resource=*found->second;VkDescriptorBufferInfo buffer={};VkDescriptorImageInfo image={};VkBufferView texel=VK_NULL_HANDLE;
    if(resource.kind==0 && binding.descriptor_type==VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER) {
      auto b=static_cast<Buffer*>(resource.resource);if(!b || &b->c!=&c || !(b->usage&1) || b->size>c.properties.limits.maxUniformBufferRange)
        throw std::runtime_error("draw CBV resource/device/range");
      buffer={b->handle,0,b->size};
    }else if(resource.kind==1 && binding.descriptor_type==VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE) {
      auto v=static_cast<ImageView*>(resource.resource);if(!v || &v->t.c!=&c || v->desc.kind!=0 || v->desc.layers!=1 || &v->t==&target->t || (depth && &v->t==&depth->t))
        throw std::runtime_error("draw image SRV/device/shape/attachment conflict");
      image.imageView=v->handle;image.imageLayout=VK_IMAGE_LAYOUT_GENERAL;
      v->t.barrier(commands.command,VK_IMAGE_LAYOUT_GENERAL,VK_IMAGE_LAYOUT_GENERAL,v->desc.mip,v->desc.layer,v->desc.mips,v->desc.layers);
    }else if(resource.kind==2 && (binding.descriptor_type==VK_DESCRIPTOR_TYPE_UNIFORM_TEXEL_BUFFER || binding.descriptor_type==VK_DESCRIPTOR_TYPE_STORAGE_BUFFER)) {
      auto v=static_cast<BufferView*>(resource.resource);if(!v || &v->buffer.c!=&c ||
        ((binding.descriptor_type==VK_DESCRIPTOR_TYPE_UNIFORM_TEXEL_BUFFER)!=bool(v->format)))throw std::runtime_error("draw buffer SRV/device/type");
      texel=v->handle;buffer={v->buffer.handle,v->offset,v->size};
    }else if(resource.kind==3 && binding.descriptor_type==VK_DESCRIPTOR_TYPE_SAMPLER)image.sampler=samplers.make(resource.sampler);
    else throw std::runtime_error("draw descriptor type mismatch");
    VkWriteDescriptorSet write={VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET};write.dstSet=sets.at(binding.set);write.dstBinding=binding.binding;
    write.descriptorCount=1;write.descriptorType=VkDescriptorType(binding.descriptor_type);write.pBufferInfo=&buffer;write.pImageInfo=&image;write.pTexelBufferView=&texel;
    c.fn<PFN_vkUpdateDescriptorSets>("vkUpdateDescriptorSets")(c.device,1,&write,0,nullptr);
  }
  target->t.barrier(commands.command,VK_IMAGE_LAYOUT_GENERAL,VK_IMAGE_LAYOUT_GENERAL,target->desc.mip,target->desc.layer,1,1);
  if(depth)depth->t.barrier(commands.command,VK_IMAGE_LAYOUT_GENERAL,VK_IMAGE_LAYOUT_GENERAL,depth->desc.mip,depth->desc.layer,1,1);
  VkRenderPassBeginInfo begin={VK_STRUCTURE_TYPE_RENDER_PASS_BEGIN_INFO};begin.renderPass=pipeline.pass;begin.framebuffer=commands.scope.framebuffer;
  begin.renderArea={{0,0},{width,height}};
  c.fn<PFN_vkCmdBeginRenderPass>("vkCmdBeginRenderPass")(commands.command,&begin,VK_SUBPASS_CONTENTS_INLINE);
  c.fn<PFN_vkCmdBindPipeline>("vkCmdBindPipeline")(commands.command,VK_PIPELINE_BIND_POINT_GRAPHICS,pipeline.pipeline);
  if(!reflected.empty())c.fn<PFN_vkCmdBindDescriptorSets>("vkCmdBindDescriptorSets")(commands.command,VK_PIPELINE_BIND_POINT_GRAPHICS,pipeline.layout,0,4,sets.data(),0,nullptr);
  for(auto [slot,rate]:slots) {
    auto b=static_cast<Buffer*>(d.vertices[slot].buffer);VkDeviceSize offset=d.vertices[slot].offset;
    c.fn<PFN_vkCmdBindVertexBuffers>("vkCmdBindVertexBuffers")(commands.command,slot,1,&b->handle,&offset);
  }
  if(d.indexed) {
    c.fn<PFN_vkCmdBindIndexBuffer>("vkCmdBindIndexBuffer")(commands.command,index->handle,d.index_offset,d.index_format==57?VK_INDEX_TYPE_UINT16:VK_INDEX_TYPE_UINT32);
    c.fn<PFN_vkCmdDrawIndexed>("vkCmdDrawIndexed")(commands.command,d.count,d.instances,d.first,d.base_vertex,d.first_instance);
  }else c.fn<PFN_vkCmdDraw>("vkCmdDraw")(commands.command,d.count,d.instances,d.first,d.first_instance);
  c.fn<PFN_vkCmdEndRenderPass>("vkCmdEndRenderPass")(commands.command);
  target->t.barrier(commands.command,VK_IMAGE_LAYOUT_GENERAL,VK_IMAGE_LAYOUT_GENERAL,target->desc.mip,target->desc.layer,1,1);
  if(depth)depth->t.barrier(commands.command,VK_IMAGE_LAYOUT_GENERAL,VK_IMAGE_LAYOUT_GENERAL,depth->desc.mip,depth->desc.layer,1,1);
  commands.submit();
}
