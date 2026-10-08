// Owned presentation through public Vulkan WSI and CAMetalLayer.
// SPDX-License-Identifier: LGPL-2.1-only
struct Surface {
  Context& c;
  VkSurfaceKHR surface=VK_NULL_HANDLE;
  VkSwapchainKHR swapchain=VK_NULL_HANDLE;
  VkSemaphore acquired=VK_NULL_HANDLE,ready=VK_NULL_HANDLE;
  VkExtent2D extent={};
  VkFormat format=VK_FORMAT_UNDEFINED;
  uint32_t interval=UINT32_MAX;
  std::vector<VkImage> images;
  std::vector<bool> initialized;
  explicit Surface(Context& context):c(context){}
  ~Surface() {
    if(surface) {
      c.wait_idle(c.device);
      if(acquired)c.fn<PFN_vkDestroySemaphore>("vkDestroySemaphore")(c.device,acquired,nullptr);
      if(ready)c.fn<PFN_vkDestroySemaphore>("vkDestroySemaphore")(c.device,ready,nullptr);
      if(swapchain)c.fn<PFN_vkDestroySwapchainKHR>("vkDestroySwapchainKHR")(c.device,swapchain,nullptr);
      c.instance_fn<PFN_vkDestroySurfaceKHR>("vkDestroySurfaceKHR")(c.instance,surface,nullptr);
    }
  }
  void configure(uint32_t width,uint32_t height,uint32_t sync,bool force=false) {
    if(!width || !height || width>8192 || height>8192 || sync>1)
      throw std::runtime_error("surface size/sync interval unsupported");
    VkSurfaceCapabilitiesKHR cap;
    check(c.instance_fn<PFN_vkGetPhysicalDeviceSurfaceCapabilitiesKHR>("vkGetPhysicalDeviceSurfaceCapabilitiesKHR")
      (c.physical,surface,&cap),"surface capabilities");
    VkExtent2D size=cap.currentExtent;
    if(size.width==UINT32_MAX)size={std::clamp(width,cap.minImageExtent.width,cap.maxImageExtent.width),
      std::clamp(height,cap.minImageExtent.height,cap.maxImageExtent.height)};
    if(!force && swapchain && extent.width==size.width && extent.height==size.height && interval==sync)return;
    if(!(cap.supportedUsageFlags&VK_IMAGE_USAGE_TRANSFER_DST_BIT))
      throw std::runtime_error("surface transfer destination unsupported");
    uint32_t count=0;
    auto formats_fn=c.instance_fn<PFN_vkGetPhysicalDeviceSurfaceFormatsKHR>("vkGetPhysicalDeviceSurfaceFormatsKHR");
    check(formats_fn(c.physical,surface,&count,nullptr),"surface format count");
    if(!count || count>256)throw std::runtime_error("surface format count invalid");
    std::vector<VkSurfaceFormatKHR> formats(count);
    check(formats_fn(c.physical,surface,&count,formats.data()),"surface formats");formats.resize(count);
    auto selected=std::find_if(formats.begin(),formats.end(),[](auto f) {
      return (f.format==VK_FORMAT_B8G8R8A8_UNORM || f.format==VK_FORMAT_R8G8B8A8_UNORM) &&
        f.colorSpace==VK_COLOR_SPACE_SRGB_NONLINEAR_KHR;
    });
    if(selected==formats.end())throw std::runtime_error("no supported RGBA/BGRA surface format");
    VkFormatProperties fp;c.format_properties(c.physical,selected->format,&fp);
    if(!(fp.optimalTilingFeatures&VK_FORMAT_FEATURE_BLIT_DST_BIT))
      throw std::runtime_error("surface format blit destination unsupported");
    auto modes_fn=c.instance_fn<PFN_vkGetPhysicalDeviceSurfacePresentModesKHR>("vkGetPhysicalDeviceSurfacePresentModesKHR");
    check(modes_fn(c.physical,surface,&count,nullptr),"surface present-mode count");
    if(!count || count>256)throw std::runtime_error("surface present-mode count invalid");
    std::vector<VkPresentModeKHR> modes(count);
    check(modes_fn(c.physical,surface,&count,modes.data()),"surface present modes");modes.resize(count);
    VkPresentModeKHR mode=VK_PRESENT_MODE_FIFO_KHR;
    if(!sync && std::find(modes.begin(),modes.end(),VK_PRESENT_MODE_IMMEDIATE_KHR)!=modes.end())
      mode=VK_PRESENT_MODE_IMMEDIATE_KHR;
    uint32_t buffers=std::max(2u,cap.minImageCount);
    if(cap.maxImageCount)buffers=std::min(buffers,cap.maxImageCount);
    VkCompositeAlphaFlagBitsKHR alpha=VK_COMPOSITE_ALPHA_OPAQUE_BIT_KHR;
    if(!(cap.supportedCompositeAlpha&alpha)) {
      uint32_t bits=cap.supportedCompositeAlpha;
      if(!bits)throw std::runtime_error("surface composite alpha unavailable");
      alpha=static_cast<VkCompositeAlphaFlagBitsKHR>(bits&(~bits+1));
    }
    VkSwapchainCreateInfoKHR ci={VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR};
    ci.surface=surface;ci.minImageCount=buffers;ci.imageFormat=selected->format;ci.imageColorSpace=selected->colorSpace;
    ci.imageExtent=size;ci.imageArrayLayers=1;ci.imageUsage=VK_IMAGE_USAGE_TRANSFER_DST_BIT;
    ci.imageSharingMode=VK_SHARING_MODE_EXCLUSIVE;ci.preTransform=cap.currentTransform;
    ci.compositeAlpha=alpha;ci.presentMode=mode;ci.clipped=VK_TRUE;
    ci.oldSwapchain=swapchain;
    VkSwapchainKHR next=VK_NULL_HANDLE;
    // WSI retires oldSwapchain on replacement. If creation or image discovery
    // fails, release the retired chain and require a fresh recreation next time.
    VkResult created=c.fn<PFN_vkCreateSwapchainKHR>("vkCreateSwapchainKHR")(c.device,&ci,nullptr,&next);
    if(created!=VK_SUCCESS) {
      if(swapchain)c.fn<PFN_vkDestroySwapchainKHR>("vkDestroySwapchainKHR")(c.device,swapchain,nullptr);
      swapchain=VK_NULL_HANDLE;images.clear();initialized.clear();check(created,"create native swapchain");
    }
    try {
      auto get=c.fn<PFN_vkGetSwapchainImagesKHR>("vkGetSwapchainImagesKHR");
      check(get(c.device,next,&count,nullptr),"swapchain image count");
      if(!count || count>32)throw std::runtime_error("swapchain image capacity");
      std::vector<VkImage> next_images(count);
      check(get(c.device,next,&count,next_images.data()),"swapchain images");next_images.resize(count);
      std::vector<bool> next_initialized(count,false);
      check(c.wait_idle(c.device),"resize surface idle");
      if(swapchain)c.fn<PFN_vkDestroySwapchainKHR>("vkDestroySwapchainKHR")(c.device,swapchain,nullptr);
      swapchain=next;images=std::move(next_images);initialized=std::move(next_initialized);
      extent=size;format=selected->format;interval=sync;
    } catch(...) {
      c.wait_idle(c.device);
      c.fn<PFN_vkDestroySwapchainKHR>("vkDestroySwapchainKHR")(c.device,next,nullptr);
      if(swapchain)c.fn<PFN_vkDestroySwapchainKHR>("vkDestroySwapchainKHR")(c.device,swapchain,nullptr);
      swapchain=VK_NULL_HANDLE;images.clear();initialized.clear();throw;
    }
  }
};
std::unique_ptr<Surface> create_surface(Context& c,const void *layer) {
  if(!c.presentation || !layer)throw std::runtime_error("public Metal surface extensions unavailable");
  auto s=std::make_unique<Surface>(c);
  VkMetalSurfaceCreateInfoEXT ci={VK_STRUCTURE_TYPE_METAL_SURFACE_CREATE_INFO_EXT};
  ci.pLayer=static_cast<const CAMetalLayer*>(layer);
  check(c.instance_fn<PFN_vkCreateMetalSurfaceEXT>("vkCreateMetalSurfaceEXT")(c.instance,&ci,nullptr,&s->surface),"create Metal surface");
  VkBool32 supported=VK_FALSE;
  check(c.instance_fn<PFN_vkGetPhysicalDeviceSurfaceSupportKHR>("vkGetPhysicalDeviceSurfaceSupportKHR")
    (c.physical,c.family,s->surface,&supported),"surface queue support");
  if(!supported)throw std::runtime_error("graphics queue cannot present to this surface");
  VkSemaphoreCreateInfo si={VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO};
  check(c.fn<PFN_vkCreateSemaphore>("vkCreateSemaphore")(c.device,&si,nullptr,&s->acquired),"acquire semaphore");
  check(c.fn<PFN_vkCreateSemaphore>("vkCreateSemaphore")(c.device,&si,nullptr,&s->ready),"present semaphore");
  return s;
}
void present_texture(Surface& s,Texture& t,uint32_t width,uint32_t height,uint32_t interval) {
  auto& c=s.c;
  if(&c!=&t.c || t.desc.layers!=1 || t.volume() || !t.desc.mips ||
      !(t.desc.format==28 || t.desc.format==29 || t.desc.format==87 || t.desc.format==91))
    throw std::runtime_error("present texture device/format/shape mismatch");
  VkFormatProperties fp;c.format_properties(c.physical,t.format,&fp);
  if(!(fp.optimalTilingFeatures&VK_FORMAT_FEATURE_BLIT_SRC_BIT))throw std::runtime_error("present texture cannot blit");
  s.configure(width,height,interval);
  uint32_t index=0;
  auto acquire=c.fn<PFN_vkAcquireNextImageKHR>("vkAcquireNextImageKHR");
  VkResult result=acquire(c.device,s.swapchain,5000000000ull,s.acquired,VK_NULL_HANDLE,&index);
  if(result==VK_ERROR_OUT_OF_DATE_KHR) {
    s.configure(width,height,interval,true);
    result=acquire(c.device,s.swapchain,5000000000ull,s.acquired,VK_NULL_HANDLE,&index);
  }
  if(result!=VK_SUBOPTIMAL_KHR)check(result,"acquire present image");
  if(index>=s.images.size())throw std::runtime_error("invalid acquired image index");
  // An sRGB backbuffer stores encoded display values. Reinterpret via a raw
  // compatible GPU copy before a UNORM surface blit, avoiding sRGB decode here.
  // The following blit still performs RGBA/BGRA component conversion on the GPU.
  std::unique_ptr<Texture> encoded;
  Texture *source=&t;
  if(t.desc.format==29 || t.desc.format==91) {
    WireTextureDesc d=t.desc;d.mips=1;d.format=d.format==29?28:87;d.bind=0;
    encoded=create_texture(c,d);source=encoded.get();
  }
  Commands cmd(c);
  t.barrier(cmd.command,VK_IMAGE_LAYOUT_GENERAL,VK_IMAGE_LAYOUT_GENERAL,0,0,1,1);
  if(encoded) {
    encoded->barrier(cmd.command,VK_IMAGE_LAYOUT_GENERAL,VK_IMAGE_LAYOUT_GENERAL,0,0,1,1);
    VkImageCopy copy={};copy.srcSubresource=copy.dstSubresource={VK_IMAGE_ASPECT_COLOR_BIT,0,0,1};
    copy.extent={t.desc.width,t.desc.height,1};
    c.fn<PFN_vkCmdCopyImage>("vkCmdCopyImage")(cmd.command,t.handle,VK_IMAGE_LAYOUT_GENERAL,
      encoded->handle,VK_IMAGE_LAYOUT_GENERAL,1,&copy);
    encoded->barrier(cmd.command,VK_IMAGE_LAYOUT_GENERAL,VK_IMAGE_LAYOUT_GENERAL,0,0,1,1);
  }
  VkImageMemoryBarrier b={VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER};b.image=s.images[index];
  b.oldLayout=s.initialized[index]?VK_IMAGE_LAYOUT_PRESENT_SRC_KHR:VK_IMAGE_LAYOUT_UNDEFINED;
  b.newLayout=VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL;b.srcQueueFamilyIndex=b.dstQueueFamilyIndex=VK_QUEUE_FAMILY_IGNORED;
  b.subresourceRange={VK_IMAGE_ASPECT_COLOR_BIT,0,1,0,1};b.dstAccessMask=VK_ACCESS_TRANSFER_WRITE_BIT;
  auto barrier=c.fn<PFN_vkCmdPipelineBarrier>("vkCmdPipelineBarrier");
  barrier(cmd.command,VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT,VK_PIPELINE_STAGE_TRANSFER_BIT,0,0,nullptr,0,nullptr,1,&b);
  VkImageBlit blit={};blit.srcSubresource=blit.dstSubresource={VK_IMAGE_ASPECT_COLOR_BIT,0,0,1};
  blit.srcOffsets[1]={int32_t(t.desc.width),int32_t(t.desc.height),1};
  blit.dstOffsets[1]={int32_t(s.extent.width),int32_t(s.extent.height),1};
  c.fn<PFN_vkCmdBlitImage>("vkCmdBlitImage")(cmd.command,source->handle,VK_IMAGE_LAYOUT_GENERAL,s.images[index],
    VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,1,&blit,VK_FILTER_NEAREST);
  b.oldLayout=b.newLayout;b.newLayout=VK_IMAGE_LAYOUT_PRESENT_SRC_KHR;
  b.srcAccessMask=VK_ACCESS_TRANSFER_WRITE_BIT;b.dstAccessMask=0;
  barrier(cmd.command,VK_PIPELINE_STAGE_TRANSFER_BIT,VK_PIPELINE_STAGE_BOTTOM_OF_PIPE_BIT,0,0,nullptr,0,nullptr,1,&b);
  check(c.fn<PFN_vkEndCommandBuffer>("vkEndCommandBuffer")(cmd.command),"end present commands");
  VkFenceCreateInfo fi={VK_STRUCTURE_TYPE_FENCE_CREATE_INFO};
  check(c.fn<PFN_vkCreateFence>("vkCreateFence")(c.device,&fi,nullptr,&cmd.scope.fence),"present fence");
  VkPipelineStageFlags wait_stage=VK_PIPELINE_STAGE_TRANSFER_BIT;
  VkSubmitInfo si={VK_STRUCTURE_TYPE_SUBMIT_INFO};si.waitSemaphoreCount=1;si.pWaitSemaphores=&s.acquired;
  si.pWaitDstStageMask=&wait_stage;si.commandBufferCount=1;si.pCommandBuffers=&cmd.command;
  si.signalSemaphoreCount=1;si.pSignalSemaphores=&s.ready;
  check(c.fn<PFN_vkQueueSubmit>("vkQueueSubmit")(c.queue,1,&si,cmd.scope.fence),"submit present blit");
  cmd.scope.pending=true;
  VkPresentInfoKHR pi={VK_STRUCTURE_TYPE_PRESENT_INFO_KHR};pi.waitSemaphoreCount=1;pi.pWaitSemaphores=&s.ready;
  pi.swapchainCount=1;pi.pSwapchains=&s.swapchain;pi.pImageIndices=&index;
  result=c.fn<PFN_vkQueuePresentKHR>("vkQueuePresentKHR")(c.queue,&pi);
  // Initial conservative ownership model: wait for both the transfer fence and
  // presentation's semaphore consumption before reusing/destroying resources.
  check(c.fn<PFN_vkWaitForFences>("vkWaitForFences")(c.device,1,&cmd.scope.fence,VK_TRUE,5000000000ull),"present transfer wait");
  check(c.fn<PFN_vkQueueWaitIdle>("vkQueueWaitIdle")(c.queue),"present queue idle");
  cmd.scope.pending=false;s.initialized[index]=true;
  if(result!=VK_SUBOPTIMAL_KHR)check(result,"queue present");
}
