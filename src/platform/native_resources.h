// Independently implemented owned Vulkan images and synchronous transfer scope.
// SPDX-License-Identifier: LGPL-2.1-only
// Typeless is a resource storage choice, never a shader/vertex/view format.
VkFormat texture_storage_format(uint32_t format,uint32_t bind=0) {
  if(format==39)return (bind&0x40)?VK_FORMAT_D32_SFLOAT:VK_FORMAT_R32_SFLOAT;
  if(format==53)return (bind&0x40)?VK_FORMAT_D16_UNORM:VK_FORMAT_R16_SFLOAT;
  return dxgi_format(format==27?28:format==90?87:format);
}
bool rgba8_family(uint32_t format) {return format>=27 && format<=32;}
bool bgra8_family(uint32_t format) {return format==87 || format==90 || format==91;}
bool r32_color_family(uint32_t format) {return format==39 || (format>=41 && format<=43);}
bool r16_color_family(uint32_t format) {return format==53 || format==54 || (format>=56 && format<=59);}
struct Commands {
  Context& c;
  BlitScope scope;
  VkCommandBuffer command=VK_NULL_HANDLE;
  explicit Commands(Context& context):c(context),scope(c) {
    VkCommandPoolCreateInfo pi={VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO};pi.queueFamilyIndex=c.family;
    pi.flags=VK_COMMAND_POOL_CREATE_TRANSIENT_BIT;
    check(c.fn<PFN_vkCreateCommandPool>("vkCreateCommandPool")(c.device,&pi,nullptr,&scope.commands),"resource command pool");
    VkCommandBufferAllocateInfo ai={VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO};ai.commandPool=scope.commands;
    ai.level=VK_COMMAND_BUFFER_LEVEL_PRIMARY;ai.commandBufferCount=1;
    check(c.fn<PFN_vkAllocateCommandBuffers>("vkAllocateCommandBuffers")(c.device,&ai,&command),"resource command buffer");
    VkCommandBufferBeginInfo bi={VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO};bi.flags=VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
    check(c.fn<PFN_vkBeginCommandBuffer>("vkBeginCommandBuffer")(command,&bi),"resource begin");
  }
  void submit() {
    auto queue_submit=c.fn<PFN_vkQueueSubmit>("vkQueueSubmit");auto wait=c.fn<PFN_vkWaitForFences>("vkWaitForFences");
    check(c.fn<PFN_vkEndCommandBuffer>("vkEndCommandBuffer")(command),"resource end");
    VkFenceCreateInfo fi={VK_STRUCTURE_TYPE_FENCE_CREATE_INFO};
    check(c.fn<PFN_vkCreateFence>("vkCreateFence")(c.device,&fi,nullptr,&scope.fence),"resource fence");
    VkSubmitInfo si={VK_STRUCTURE_TYPE_SUBMIT_INFO};si.commandBufferCount=1;si.pCommandBuffers=&command;
    check(queue_submit(c.queue,1,&si,scope.fence),"resource submit");scope.pending=true;
    auto result=wait(c.device,1,&scope.fence,VK_TRUE,5000000000ull);
    if(result!=VK_SUCCESS) {c.wait_idle(c.device);scope.pending=false;check(result,"resource wait");}
    scope.pending=false;
  }
};
struct Texture {
  Context& c;
  WireTextureDesc desc;
  VkFormat format;
  VkImage handle=VK_NULL_HANDLE;
  VkDeviceMemory memory=VK_NULL_HANDLE;
  VkImageAspectFlags aspect;
  uint32_t block=1,bytes=0;
  Texture(Context& context,WireTextureDesc d):c(context),desc(d),format(texture_storage_format(d.format,d.bind)) {
    aspect=(d.format==40 || d.format==55 || ((d.format==39 || d.format==53) && (d.bind&0x40)))?VK_IMAGE_ASPECT_DEPTH_BIT:
      (d.format==19 || d.format==20 || d.format==45)?VK_IMAGE_ASPECT_DEPTH_BIT|VK_IMAGE_ASPECT_STENCIL_BIT:VK_IMAGE_ASPECT_COLOR_BIT;
    switch(d.format) {
      case 2:case 3:case 4: bytes=16;break;
      case 6:case 7:case 8:bytes=12;break;
      case 10:case 11:case 12:case 13:case 14:case 16:case 17:case 18:case 19:case 20:bytes=8;break;
      case 49:case 50:case 51:case 52:case 53:case 54:case 55:case 56:case 57:case 58:case 59:bytes=2;break;
      case 61:case 62:case 63:case 64:bytes=1;break;
      case 71:case 72:case 80:block=4;bytes=8;break;
      case 74:case 75:case 77:case 78:case 83:block=4;bytes=16;break;
      default:bytes=4;
    }
  }
  ~Texture() {
    if(handle)c.fn<PFN_vkDestroyImage>("vkDestroyImage")(c.device,handle,nullptr);
    if(memory)c.fn<PFN_vkFreeMemory>("vkFreeMemory")(c.device,memory,nullptr);
  }
  bool volume() const {return desc.bind&0x200;}
  uint32_t subresources() const {return desc.mips*(volume()?1:desc.layers);}
  uint32_t depth(uint32_t mip) const {return volume()?std::max(1u,desc.layers>>mip):1;}
  size_t size(uint32_t mip) const {
    uint64_t w=std::max(1u,desc.width>>mip),h=std::max(1u,desc.height>>mip);
    return size_t(((w+block-1)/block)*((h+block-1)/block)*bytes*depth(mip));
  }
  void barrier(VkCommandBuffer cmd,VkImageLayout old_layout,VkImageLayout layout,uint32_t mip,uint32_t layer,uint32_t levels,uint32_t layers) {
    VkImageMemoryBarrier b={VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER};b.image=handle;b.oldLayout=old_layout;b.newLayout=layout;
    b.srcQueueFamilyIndex=b.dstQueueFamilyIndex=VK_QUEUE_FAMILY_IGNORED;
    b.subresourceRange={aspect,mip,levels,layer,layers};
    b.srcAccessMask=old_layout==VK_IMAGE_LAYOUT_UNDEFINED?0:VK_ACCESS_MEMORY_READ_BIT|VK_ACCESS_MEMORY_WRITE_BIT;
    b.dstAccessMask=VK_ACCESS_MEMORY_READ_BIT|VK_ACCESS_MEMORY_WRITE_BIT;
    c.fn<PFN_vkCmdPipelineBarrier>("vkCmdPipelineBarrier")(cmd,VK_PIPELINE_STAGE_ALL_COMMANDS_BIT,VK_PIPELINE_STAGE_ALL_COMMANDS_BIT,0,0,nullptr,0,nullptr,1,&b);
  }
};
std::unique_ptr<Texture> create_texture(Context& c,const WireTextureDesc& d) {
  if(d.format==21 || d.format==22 || (d.format==19 && !(d.bind&0x40)))
    throw std::runtime_error("depth/stencil view-only or typeless texture binding unsupported");
  if(!d.width || !d.height || !d.mips || !d.layers || d.width>c.properties.limits.maxImageDimension2D ||
     d.height>c.properties.limits.maxImageDimension2D || d.layers>c.properties.limits.maxImageArrayLayers || d.mips>14 ||
     d.mips>uint32_t(32-__builtin_clz(std::max(std::max(d.width,d.height),(d.bind&0x200)?d.layers:1u))) || d.bind&~0x768u || texture_storage_format(d.format,d.bind)==VK_FORMAT_UNDEFINED)
    throw std::runtime_error("texture dimensions/format/usage unsupported");
  auto t=std::make_unique<Texture>(c,d);uint64_t total=0;
  for(uint32_t mip=0;mip<d.mips;mip++)total+=uint64_t(t->size(mip))*(t->volume()?1:d.layers);
  if(total>128*1024*1024)throw std::runtime_error("texture allocation limit");
  VkFormatProperties fp={};c.format_properties(c.physical,t->format,&fp);
  if((d.bind&8) && !(fp.optimalTilingFeatures&VK_FORMAT_FEATURE_SAMPLED_IMAGE_BIT))throw std::runtime_error("texture sampling format unsupported");
  if((d.bind&0x20) && !(fp.optimalTilingFeatures&VK_FORMAT_FEATURE_COLOR_ATTACHMENT_BIT))throw std::runtime_error("render target format unsupported");
  if((d.bind&0x40) && !(fp.optimalTilingFeatures&VK_FORMAT_FEATURE_DEPTH_STENCIL_ATTACHMENT_BIT))throw std::runtime_error("depth target format unsupported");
  VkImageCreateInfo ii={VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO};ii.imageType=VK_IMAGE_TYPE_2D;ii.format=t->format;
  if(d.format==27 || d.format==90 || ((d.format==39 || d.format==53) && !(d.bind&0x40)) || (d.bind&0x400)) {
    if(!(rgba8_family(d.format) || bgra8_family(d.format) || r32_color_family(d.format) || r16_color_family(d.format)))throw std::runtime_error("mutable image format family unsupported");
    ii.flags|=VK_IMAGE_CREATE_MUTABLE_FORMAT_BIT;
  }
  if(d.bind&0x100) {if(d.width!=d.height || d.layers%6)throw std::runtime_error("cube texture dimensions");ii.flags|=VK_IMAGE_CREATE_CUBE_COMPATIBLE_BIT;}
  if(t->volume()) {if(d.bind&0x100 || d.width>c.properties.limits.maxImageDimension3D || d.height>c.properties.limits.maxImageDimension3D || d.layers>c.properties.limits.maxImageDimension3D || d.layers>2048)throw std::runtime_error("volume texture dimensions/flags");ii.imageType=VK_IMAGE_TYPE_3D;}
  ii.extent={d.width,d.height,t->depth(0)};ii.mipLevels=d.mips;ii.arrayLayers=t->volume()?1:d.layers;ii.samples=VK_SAMPLE_COUNT_1_BIT;ii.tiling=VK_IMAGE_TILING_OPTIMAL;
  ii.usage=VK_IMAGE_USAGE_TRANSFER_SRC_BIT|VK_IMAGE_USAGE_TRANSFER_DST_BIT;
  if(d.bind&8)ii.usage|=VK_IMAGE_USAGE_SAMPLED_BIT;if(d.bind&0x20)ii.usage|=VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT;
  if(d.bind&0x40)ii.usage|=VK_IMAGE_USAGE_DEPTH_STENCIL_ATTACHMENT_BIT;
  check(c.fn<PFN_vkCreateImage>("vkCreateImage")(c.device,&ii,nullptr,&t->handle),"create owned texture");
  VkMemoryRequirements req;c.fn<PFN_vkGetImageMemoryRequirements>("vkGetImageMemoryRequirements")(c.device,t->handle,&req);
  t->memory=allocate(c,req,VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT);
  check(c.fn<PFN_vkBindImageMemory>("vkBindImageMemory")(c.device,t->handle,t->memory,0),"bind owned texture");
  Commands cmd(c);t->barrier(cmd.command,VK_IMAGE_LAYOUT_UNDEFINED,VK_IMAGE_LAYOUT_GENERAL,0,0,d.mips,t->volume()?1:d.layers);cmd.submit();return t;
}
void texture_write(Texture& t,uint32_t subresource,const uint8_t *data,size_t size) {
  if(subresource>=t.subresources() || !data || t.aspect!=VK_IMAGE_ASPECT_COLOR_BIT)throw std::runtime_error("texture write subresource/aspect");
  uint32_t mip=subresource%t.desc.mips,layer=t.volume()?0:subresource/t.desc.mips;
  if(size!=t.size(mip))throw std::runtime_error("texture tight data size");
  auto staging=create_buffer(t.c,data,size,2);Commands cmd(t.c);
  t.barrier(cmd.command,VK_IMAGE_LAYOUT_GENERAL,VK_IMAGE_LAYOUT_GENERAL,mip,layer,1,1);
  VkBufferImageCopy copy={};copy.imageSubresource={t.aspect,mip,layer,1};copy.imageExtent={std::max(1u,t.desc.width>>mip),std::max(1u,t.desc.height>>mip),t.depth(mip)};
  t.c.fn<PFN_vkCmdCopyBufferToImage>("vkCmdCopyBufferToImage")(cmd.command,staging->handle,t.handle,VK_IMAGE_LAYOUT_GENERAL,1,&copy);
  t.barrier(cmd.command,VK_IMAGE_LAYOUT_GENERAL,VK_IMAGE_LAYOUT_GENERAL,mip,layer,1,1);cmd.submit();
}
void texture_read(Texture& t,uint32_t subresource,uint8_t *data,size_t size) {
  if(subresource>=t.subresources() || !data)throw std::runtime_error("texture read subresource/output");
  bool depth=t.desc.format==19 || t.desc.format==20 || t.desc.format==40 || t.desc.format==55 ||
    ((t.desc.format==39 || t.desc.format==53) && (t.desc.bind&0x40));
  if(t.aspect!=VK_IMAGE_ASPECT_COLOR_BIT && !depth)throw std::runtime_error("texture read depth format unsupported");
  uint32_t mip=subresource%t.desc.mips,layer=t.volume()?0:subresource/t.desc.mips;
  if(size!=t.size(mip))throw std::runtime_error("texture tight read size");
  auto staging=create_buffer(t.c,nullptr,size,2);Commands cmd(t.c);
  t.barrier(cmd.command,VK_IMAGE_LAYOUT_GENERAL,VK_IMAGE_LAYOUT_GENERAL,mip,layer,1,1);
  VkBufferImageCopy copies[2]={};auto& copy=copies[0];
  copy.imageSubresource={depth?VkImageAspectFlags(VK_IMAGE_ASPECT_DEPTH_BIT):t.aspect,mip,layer,1};
  copy.imageExtent={std::max(1u,t.desc.width>>mip),std::max(1u,t.desc.height>>mip),t.depth(mip)};
  size_t pixels=size/t.bytes;
  bool stencil=depth && (t.aspect&VK_IMAGE_ASPECT_STENCIL_BIT);
  if(stencil) {copies[1]=copy;copies[1].bufferOffset=pixels*4;copies[1].imageSubresource.aspectMask=VK_IMAGE_ASPECT_STENCIL_BIT;}
  t.c.fn<PFN_vkCmdCopyImageToBuffer>("vkCmdCopyImageToBuffer")(cmd.command,t.handle,VK_IMAGE_LAYOUT_GENERAL,staging->handle,stencil?2:1,copies);
  VkBufferMemoryBarrier b={VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER};b.buffer=staging->handle;b.offset=0;b.size=size;
  b.srcQueueFamilyIndex=b.dstQueueFamilyIndex=VK_QUEUE_FAMILY_IGNORED;b.srcAccessMask=VK_ACCESS_TRANSFER_WRITE_BIT;b.dstAccessMask=VK_ACCESS_HOST_READ_BIT;
  t.c.fn<PFN_vkCmdPipelineBarrier>("vkCmdPipelineBarrier")(cmd.command,VK_PIPELINE_STAGE_TRANSFER_BIT,VK_PIPELINE_STAGE_HOST_BIT,0,0,nullptr,1,&b,0,nullptr);
  cmd.submit();
  if(stencil) {
    auto source=static_cast<const uint8_t*>(staging->mapped);std::memset(data,0,size);
    for(size_t p=0;p<pixels;p++) {std::memcpy(data+p*8,source+p*4,4);data[p*8+4]=source[pixels*4+p];}
  }else std::memcpy(data,staging->mapped,size);
}
struct ImageView {
  Texture& t;WireImageViewDesc desc;VkImageView handle=VK_NULL_HANDLE;
  ImageView(Texture& texture,WireImageViewDesc d):t(texture),desc(d){}
  ~ImageView(){if(handle)t.c.fn<PFN_vkDestroyImageView>("vkDestroyImageView")(t.c.device,handle,nullptr);}
};
std::unique_ptr<ImageView> create_image_view(Texture& t,WireImageViewDesc d) {
  bool compatible=d.format==t.desc.format || ((t.desc.bind&0x400) &&
    (((d.format==28 || d.format==29) && (t.desc.format==28 || t.desc.format==29)) ||
     ((d.format==87 || d.format==91) && (t.desc.format==87 || t.desc.format==91))));
  if(t.desc.format==27)compatible=d.format>=28 && d.format<=32;
  if(t.desc.format==90)compatible=d.format==87 || d.format==91;
  if(t.desc.format==39) {
    compatible=(t.aspect==VK_IMAGE_ASPECT_DEPTH_BIT)?
      ((d.kind==2 && d.format==40) || (d.kind==0 && d.format==41)):(d.format>=41 && d.format<=43);
  }
  if(t.desc.format==53) {
    compatible=(t.aspect==VK_IMAGE_ASPECT_DEPTH_BIT)?
      ((d.kind==2 && d.format==55) || (d.kind==0 && d.format==56)):(d.format==54 || (d.format>=56 && d.format<=59));
  }
  VkImageAspectFlags aspect=t.aspect;
  if(t.desc.format==19) {
    compatible=(d.kind==2 && d.format==20) || (d.kind==0 && (d.format==21 || d.format==22));
    if(d.kind==0)aspect=d.format==21?VK_IMAGE_ASPECT_DEPTH_BIT:VK_IMAGE_ASPECT_STENCIL_BIT;
  }else if(t.desc.format==20 && d.kind!=2)compatible=false;
  if(!compatible || !d.mips || !d.layers || d.mip>=t.desc.mips || d.mips>t.desc.mips-d.mip ||
     d.layer>=t.desc.layers || d.layers>t.desc.layers-d.layer || d.kind>5 ||
     ((d.kind==0 || d.kind>=3) && !(t.desc.bind&8)) || (d.kind==1 && (!(t.desc.bind&0x20) || d.mips!=1)) ||
     (d.kind==2 && (!(t.desc.bind&0x40) || d.mips!=1)))throw std::runtime_error("image view format/range/usage unsupported");
  auto format=((t.desc.format==39 || t.desc.format==53) && t.aspect==VK_IMAGE_ASPECT_DEPTH_BIT)?t.format:dxgi_format(d.format);VkFormatProperties fp={};
  if(format==VK_FORMAT_UNDEFINED)throw std::runtime_error("typeless/unknown image view format");
  t.c.format_properties(t.c.physical,format,&fp);
  auto required=d.kind==1?VK_FORMAT_FEATURE_COLOR_ATTACHMENT_BIT:d.kind==2?VK_FORMAT_FEATURE_DEPTH_STENCIL_ATTACHMENT_BIT:VK_FORMAT_FEATURE_SAMPLED_IMAGE_BIT;
  if(!(fp.optimalTilingFeatures&required))throw std::runtime_error("typed image view format capability unsupported");
  auto v=std::make_unique<ImageView>(t,d);VkImageViewCreateInfo vi={VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO};
  vi.image=t.handle;vi.format=format;vi.viewType=d.layers==1?VK_IMAGE_VIEW_TYPE_2D:VK_IMAGE_VIEW_TYPE_2D_ARRAY;
  if(d.kind==5) {if(!t.volume() || d.layer!=0 || d.layers!=1)throw std::runtime_error("volume view range");vi.viewType=VK_IMAGE_VIEW_TYPE_3D;}
  else if((d.kind>=3 && d.kind<=4)) {if(!(t.desc.bind&0x100) || d.layer%6 || d.layers%6 || (d.kind==3 && d.layers!=6))throw std::runtime_error("cube view range");vi.viewType=d.kind==3?VK_IMAGE_VIEW_TYPE_CUBE:VK_IMAGE_VIEW_TYPE_CUBE_ARRAY;}
  vi.subresourceRange={aspect,d.mip,d.mips,d.layer,d.layers};
  check(t.c.fn<PFN_vkCreateImageView>("vkCreateImageView")(t.c.device,&vi,nullptr,&v->handle),"create owned image view");return v;
}
void clear_image_view(ImageView& v,const float *rgba) {
  if(!rgba || v.desc.kind!=1 || v.t.aspect!=VK_IMAGE_ASPECT_COLOR_BIT)throw std::runtime_error("clear needs color render target");
  std::unique_ptr<Texture> typed;
  if(v.desc.format!=v.t.desc.format) {
    WireTextureDesc d={std::max(1u,v.t.desc.width>>v.desc.mip),std::max(1u,v.t.desc.height>>v.desc.mip),
      1,v.desc.layers,v.desc.format,0};
    typed=create_texture(v.t.c,d);
  }
  Commands cmd(v.t.c);v.t.barrier(cmd.command,VK_IMAGE_LAYOUT_GENERAL,VK_IMAGE_LAYOUT_GENERAL,v.desc.mip,v.desc.layer,1,v.desc.layers);
  VkClearColorValue value={};std::memcpy(value.float32,rgba,16);
  VkImageSubresourceRange range={v.t.aspect,typed?0:v.desc.mip,1,typed?0:v.desc.layer,v.desc.layers};
  v.t.c.fn<PFN_vkCmdClearColorImage>("vkCmdClearColorImage")(cmd.command,typed?typed->handle:v.t.handle,VK_IMAGE_LAYOUT_GENERAL,&value,1,&range);
  if(typed) {
    typed->barrier(cmd.command,VK_IMAGE_LAYOUT_GENERAL,VK_IMAGE_LAYOUT_GENERAL,0,0,1,v.desc.layers);
    VkImageCopy copy={};copy.srcSubresource={v.t.aspect,0,0,v.desc.layers};
    copy.dstSubresource={v.t.aspect,v.desc.mip,v.desc.layer,v.desc.layers};copy.extent={typed->desc.width,typed->desc.height,1};
    v.t.c.fn<PFN_vkCmdCopyImage>("vkCmdCopyImage")(cmd.command,typed->handle,VK_IMAGE_LAYOUT_GENERAL,v.t.handle,VK_IMAGE_LAYOUT_GENERAL,1,&copy);
  }
  v.t.barrier(cmd.command,VK_IMAGE_LAYOUT_GENERAL,VK_IMAGE_LAYOUT_GENERAL,v.desc.mip,v.desc.layer,1,v.desc.layers);cmd.submit();
}
void clear_depth_view(ImageView& v,uint32_t flags,float depth,uint32_t stencil) {
  if(v.desc.kind!=2 || flags&~3u || stencil>255)
    throw std::runtime_error("invalid depth/stencil view clear");
  // ClearFlags select only aspects actually present in the DSV. A stencil bit
  // on a depth-only view is ignored; it does not imply stencil support.
  VkImageAspectFlags aspect=(((flags&1)?VK_IMAGE_ASPECT_DEPTH_BIT:0)|((flags&2)?VK_IMAGE_ASPECT_STENCIL_BIT:0))&v.t.aspect;
  if(!aspect)return;
  if(aspect&VK_IMAGE_ASPECT_DEPTH_BIT) {
    if(std::isnan(depth))throw std::runtime_error("invalid depth view clear value");
    depth=std::clamp(depth,0.f,1.f);
  }else depth=0; // Unused depth must not introduce a Vulkan validation error.
  Commands cmd(v.t.c);v.t.barrier(cmd.command,VK_IMAGE_LAYOUT_GENERAL,VK_IMAGE_LAYOUT_GENERAL,v.desc.mip,v.desc.layer,1,v.desc.layers);
  VkClearDepthStencilValue value={depth,stencil};VkImageSubresourceRange range={aspect,v.desc.mip,1,v.desc.layer,v.desc.layers};
  v.t.c.fn<PFN_vkCmdClearDepthStencilImage>("vkCmdClearDepthStencilImage")(cmd.command,v.t.handle,VK_IMAGE_LAYOUT_GENERAL,&value,1,&range);
  v.t.barrier(cmd.command,VK_IMAGE_LAYOUT_GENERAL,VK_IMAGE_LAYOUT_GENERAL,v.desc.mip,v.desc.layer,1,v.desc.layers);cmd.submit();
}
struct BufferView {
  Buffer& buffer;VkBufferView handle=VK_NULL_HANDLE;size_t offset,size;uint32_t format;
  BufferView(Buffer& b,uint32_t f,size_t at,size_t bytes):buffer(b),offset(at),size(bytes),format(f){}
  ~BufferView(){if(handle)buffer.c.fn<PFN_vkDestroyBufferView>("vkDestroyBufferView")(buffer.c.device,handle,nullptr);}
};
std::unique_ptr<BufferView> create_buffer_view(Buffer& b,uint32_t format,size_t offset,size_t size) {
  if(!(b.usage&2) || !size || offset>b.size || size>b.size-offset)throw std::runtime_error("buffer view usage/range");
  auto v=std::make_unique<BufferView>(b,format,offset,size);
  if(format) {
    auto f=dxgi_format(format);VkFormatProperties fp={};b.c.format_properties(b.c.physical,f,&fp);
    if(f==VK_FORMAT_UNDEFINED || !(fp.bufferFeatures&VK_FORMAT_FEATURE_UNIFORM_TEXEL_BUFFER_BIT) || offset%b.c.properties.limits.minTexelBufferOffsetAlignment)
      throw std::runtime_error("typed buffer view format/alignment unsupported");
    VkBufferViewCreateInfo vi={VK_STRUCTURE_TYPE_BUFFER_VIEW_CREATE_INFO};vi.buffer=b.handle;vi.format=f;vi.offset=offset;vi.range=size;
    check(b.c.fn<PFN_vkCreateBufferView>("vkCreateBufferView")(b.c.device,&vi,nullptr,&v->handle),"create owned buffer view");
  }
  return v;
}
void texture_copy(Texture& dst,Texture& src,const WireTextureCopy& r) {
  if(&dst.c!=&src.c || dst.aspect!=src.aspect || dst.block!=src.block || dst.bytes!=src.bytes || dst.volume()!=src.volume() ||
     (dst.format!=src.format && !(rgba8_family(dst.desc.format) && rgba8_family(src.desc.format)) &&
      !(bgra8_family(dst.desc.format) && bgra8_family(src.desc.format)) &&
      !(r32_color_family(dst.desc.format) && r32_color_family(src.desc.format)) &&
      !(r16_color_family(dst.desc.format) && r16_color_family(src.desc.format))) ||
     r.dst_sub>=dst.subresources() || r.src_sub>=src.subresources() || (&dst==&src && r.dst_sub==r.src_sub))
    throw std::runtime_error("texture copy context/format/subresource mismatch");
  uint32_t dm=r.dst_sub%dst.desc.mips,sm=r.src_sub%src.desc.mips;
  uint32_t dw=std::max(1u,dst.desc.width>>dm),dh=std::max(1u,dst.desc.height>>dm);
  uint32_t sw=std::max(1u,src.desc.width>>sm),sh=std::max(1u,src.desc.height>>sm);
  if(r.left>=r.right || r.top>=r.bottom || r.right>sw || r.bottom>sh || r.x>dw || r.y>dh ||
     r.right-r.left>dw-r.x || r.bottom-r.top>dh-r.y || r.left%src.block || r.top%src.block || r.x%dst.block || r.y%dst.block ||
     (r.right%src.block && r.right!=sw) || (r.bottom%src.block && r.bottom!=sh))throw std::runtime_error("texture copy region bounds/alignment");
  if(src.depth(sm)>dst.depth(dm))throw std::runtime_error("texture copy depth bounds");
  Commands cmd(dst.c);src.barrier(cmd.command,VK_IMAGE_LAYOUT_GENERAL,VK_IMAGE_LAYOUT_GENERAL,sm,src.volume()?0:r.src_sub/src.desc.mips,1,1);
  dst.barrier(cmd.command,VK_IMAGE_LAYOUT_GENERAL,VK_IMAGE_LAYOUT_GENERAL,dm,dst.volume()?0:r.dst_sub/dst.desc.mips,1,1);
  for(uint32_t aspect: {uint32_t(VK_IMAGE_ASPECT_COLOR_BIT),uint32_t(VK_IMAGE_ASPECT_DEPTH_BIT),uint32_t(VK_IMAGE_ASPECT_STENCIL_BIT)}) {
    if(!(dst.aspect&aspect))continue;
    VkImageCopy copy={};copy.srcSubresource={aspect,sm,src.volume()?0:r.src_sub/src.desc.mips,1};copy.dstSubresource={aspect,dm,dst.volume()?0:r.dst_sub/dst.desc.mips,1};
    copy.srcOffset={int32_t(r.left),int32_t(r.top),0};copy.dstOffset={int32_t(r.x),int32_t(r.y),0};copy.extent={r.right-r.left,r.bottom-r.top,src.depth(sm)};
    dst.c.fn<PFN_vkCmdCopyImage>("vkCmdCopyImage")(cmd.command,src.handle,VK_IMAGE_LAYOUT_GENERAL,dst.handle,VK_IMAGE_LAYOUT_GENERAL,1,&copy);
  }
  dst.barrier(cmd.command,VK_IMAGE_LAYOUT_GENERAL,VK_IMAGE_LAYOUT_GENERAL,dm,dst.volume()?0:r.dst_sub/dst.desc.mips,1,1);cmd.submit();
}
