use super::*;
impl D3D9 {
    pub(super) fn create_depth(
        &mut self,
        h: u32,
        w: u32,
        height: u32,
        mem: &mut GuestMem,
        api: &mut ApiTable,
        allocations: &mut HeapResources<'_>,
    ) -> Result<u32, u32> {
        let data = self.pixels(w, height, 75, 0, 2, mem, allocations)?;
        match self.surface(h, data.clone(), 1, 0, mem, api, allocations) {
            Ok(s) => Ok(s),
            Err(e) => {
                self.free_buffer(data, allocations);
                Err(e)
            }
        }
    }
    pub(super) fn bind_depth(
        &mut self,
        h: u32,
        s: u32,
        allocations: &mut HeapResources<'_>,
    ) -> Result<(), u32> {
        if s != 0 {
            match &self.objects.get(&s).ok_or(INVALID)?.payload {
                Payload::Surface { device, data, .. }
                    if *device == h && data.borrow().format == 75 => {}
                _ => return Err(INVALID),
            }
            self.pin(s)?;
        }
        let old = std::mem::replace(&mut self.device_mut(h)?.depth, s);
        let o = self.objects.get_mut(&h).unwrap();
        if s != 0 {
            o.children.push(s);
        }
        if old != 0 {
            let i = o.children.iter().position(|&v| v == old).unwrap();
            o.children.remove(i);
            self.unpin(old, allocations);
        }
        Ok(())
    }
    pub(super) fn bind(
        &mut self,
        h: u32,
        resource: u32,
        texture: bool,
        allocations: &mut HeapResources<'_>,
    ) -> Result<(), u32> {
        self.bind_slot(h, resource, texture, 0, allocations)
    }
    pub(super) fn bind_slot(
        &mut self,
        h: u32,
        resource: u32,
        texture: bool,
        slot: u32,
        allocations: &mut HeapResources<'_>,
    ) -> Result<(), u32> {
        if slot > 1 {
            return Err(UNAVAILABLE);
        }
        if resource != 0 {
            let o = self.objects.get(&resource).ok_or(INVALID)?;
            let owner = match o.payload {
                Payload::Texture { device, .. } if texture => device,
                Payload::Surface { device, .. } if !texture => device,
                _ => return Err(INVALID),
            };
            if owner != h {
                return Err(INVALID);
            }
            if !texture {
                let b = self.buffer(resource)?;
                if b.borrow().usage != 1 {
                    return Err(INVALID);
                }
            }
            self.pin(resource)?;
        }
        let d = self.device_mut(h)?;
        let old = if texture {
            std::mem::replace(
                if slot == 0 {
                    &mut d.texture
                } else {
                    &mut d.texture1
                },
                resource,
            )
        } else {
            std::mem::replace(&mut d.target, resource)
        };
        if !texture {
            let b = self.buffer(resource)?;
            let b = b.borrow();
            let (width, height) = (b.width, b.height);
            let d = self.device_mut(h)?;
            // D3D9 resets viewport and scissor to the new target's full
            // extent on every SetRenderTarget(0), including the back buffer.
            d.viewport = [0, 0, width, height, 0, 1f32.to_bits()];
            d.scissor = [0, 0, width as i32, height as i32];
        }
        let o = self.objects.get_mut(&h).unwrap();
        if resource != 0 {
            o.children.push(resource);
        }
        if old != 0 {
            let i = o.children.iter().position(|&r| r == old).unwrap();
            o.children.remove(i);
            self.unpin(old, allocations);
        }
        Ok(())
    }
    pub(super) fn device_call(
        &mut self,
        m: &str,
        a: &Args,
        mem: &mut GuestMem,
        api: &mut ApiTable,
        resources: &mut DeviceResources<'_>,
    ) -> Result<u32, u32> {
        let h = a[0];
        match m {
            "BeginStateBlock" => self.begin_stateblock(h),
            "EndStateBlock" => self.end_stateblock(a, mem, api, &mut resources.allocations),
            "CreateStateBlock" => self.create_stateblock(a, mem, api, &mut resources.allocations),
            "TestCooperativeLevel" => Ok(0),
            "Reset" => self.reset(a, mem, api, resources),
            "GetAvailableTextureMem" => Ok((256 * 1024 * 1024 - self.allocated) as u32),
            "EvictManagedResources" => Ok(0), // CPU storage is the only resource copy
            "GetDirect3D" => self.output_ref(self.device(h)?.factory, mem, a[1]),
            "GetDeviceCaps" => {
                caps(mem, a[1])?;
                Ok(0)
            }
            "GetDisplayMode" => {
                if a[1] != 0 {
                    return Err(INVALID);
                }
                put(mem, a[2], &[1024, 768, 60, 22])?;
                Ok(0)
            }
            "GetCreationParameters" => {
                let d = self.device(h)?;
                put(mem, a[1], &[0, 1, d.hwnd, d.flags])?;
                Ok(0)
            }
            "GetNumberOfSwapChains" => Ok(1),
            "GetSwapChain" => {
                if a[1] != 0 {
                    return Err(INVALID);
                }
                mem.ptr(a[2], 4, true).map_err(|_| INVALID)?;
                let mut swap = self.device(h)?.swap;
                if swap == 0 {
                    swap = self.object(
                        "IDirect3DSwapChain9",
                        Payload::Swap { device: h },
                        0,
                        vec![],
                        vec![h],
                        mem,
                        api,
                        &mut resources.allocations,
                    )?;
                    self.pin(swap)?;
                    self.objects.get_mut(&h).unwrap().children.push(swap);
                    self.device_mut(h)?.swap = swap;
                }
                self.output_ref(swap, mem, a[2])
            }
            "GetBackBuffer" => {
                if a[1] != 0 || a[2] != 0 || a[3] != 0 {
                    return Err(INVALID);
                }
                self.output_ref(self.device(h)?.back, mem, a[4])
            }
            "Present" => self.present(h, mem, resources, a[1], a[2], a[3], a[4]),
            "CreateTexture" => {
                mem.ptr(a[7], 4, true).map_err(|_| INVALID)?;
                put(mem, a[7], &[0])?;
                if a[3] != 1 || a[4] & !0x201 != 0 || a[6] > 3 || a[8] != 0 {
                    return Err(UNAVAILABLE);
                }
                let data = self.pixels(
                    a[1],
                    a[2],
                    a[5],
                    a[6],
                    a[4],
                    mem,
                    &mut resources.allocations,
                )?;
                let texture = match self.object(
                    "IDirect3DTexture9",
                    Payload::Texture {
                        device: h,
                        surface: 0,
                        data: data.clone(),
                    },
                    1,
                    vec![],
                    vec![h],
                    mem,
                    api,
                    &mut resources.allocations,
                ) {
                    Ok(t) => t,
                    Err(e) => {
                        self.free_buffer(data, &mut resources.allocations);
                        return Err(e);
                    }
                };
                let surface =
                    match self.surface(h, data, 0, texture, mem, api, &mut resources.allocations) {
                        Ok(s) => s,
                        Err(e) => {
                            self.release(texture, &mut resources.allocations)?;
                            return Err(e);
                        }
                    };
                self.pin(surface)?;
                self.objects
                    .get_mut(&texture)
                    .unwrap()
                    .children
                    .push(surface);
                if let Payload::Texture { surface: s, .. } =
                    &mut self.objects.get_mut(&texture).unwrap().payload
                {
                    *s = surface;
                }
                put(mem, a[7], &[texture])?;
                Ok(0)
            }
            "CreateOffscreenPlainSurface" | "CreateRenderTarget" => {
                let (pool, out, shared, usage) = if m == "CreateRenderTarget" {
                    if a[4] != 0 || a[5] != 0 {
                        return Err(UNAVAILABLE);
                    }
                    (0, a[7], a[8], 1)
                } else {
                    (a[4], a[5], a[6], 0)
                };
                mem.ptr(out, 4, true).map_err(|_| INVALID)?;
                put(mem, out, &[0])?;
                if shared != 0 || pool > 3 {
                    return Err(UNAVAILABLE);
                }
                let data = self.pixels(
                    a[1],
                    a[2],
                    a[3],
                    pool,
                    usage,
                    mem,
                    &mut resources.allocations,
                )?;
                let s =
                    match self.surface(h, data.clone(), 1, 0, mem, api, &mut resources.allocations)
                    {
                        Ok(s) => s,
                        Err(e) => {
                            self.free_buffer(data, &mut resources.allocations);
                            return Err(e);
                        }
                    };
                put(mem, out, &[s])?;
                Ok(0)
            }
            "ColorFill" => {
                self.fill(mem, a[1], a[2], a[3])?;
                Ok(0)
            }
            "UpdateSurface" => {
                self.copy(mem, a[1], a[2], a[3], a[4], false)?;
                Ok(0)
            }
            "StretchRect" => {
                if !matches!(a[5], 0 | 1) {
                    return Err(UNAVAILABLE);
                }
                self.copy(mem, a[1], a[2], a[3], a[4], true)?;
                Ok(0)
            }
            "UpdateTexture" | "GetRenderTargetData" => {
                self.copy(mem, a[1], 0, a[2], 0, false)?;
                Ok(0)
            }
            "SetRenderTarget" => {
                if a[1] != 0 || a[2] == 0 {
                    return Err(INVALID);
                }
                self.bind(h, a[2], false, &mut resources.allocations)?;
                Ok(0)
            }
            "GetRenderTarget" => {
                if a[1] != 0 {
                    return Err(INVALID);
                }
                self.output_ref(self.device(h)?.target, mem, a[2])
            }
            "SetDepthStencilSurface" => {
                self.bind_depth(h, a[1], &mut resources.allocations)?;
                Ok(0)
            }
            "GetDepthStencilSurface" => {
                put(mem, a[1], &[0])?;
                let s = self.device(h)?.depth;
                if s == 0 {
                    Err(0x88760866)
                } else {
                    self.output_ref(s, mem, a[1])
                }
            }
            "CreateDepthStencilSurface" => {
                put(mem, a[7], &[0])?;
                if a[3] != 75 || a[4] != 0 || a[5] != 0 || a[8] != 0 {
                    return Err(UNAVAILABLE);
                }
                let s = self.create_depth(h, a[1], a[2], mem, api, &mut resources.allocations)?;
                put(mem, a[7], &[s])?;
                Ok(0)
            }
            "BeginScene" => {
                let d = self.device_mut(h)?;
                if d.scene {
                    return Err(INVALID);
                }
                d.scene = true;
                Ok(0)
            }
            "EndScene" => {
                let d = self.device_mut(h)?;
                if !d.scene {
                    return Err(INVALID);
                }
                d.scene = false;
                Ok(0)
            }
            "Clear" => {
                if a[3] == 0 || a[3] & !7 != 0 || a[1] > 4096 || (a[1] != 0 && a[2] == 0) {
                    return Err(UNAVAILABLE);
                }
                let target = self.device(h)?.target;
                let b = self.buffer(target)?;
                let b = b.borrow();
                let mut rects = Vec::new();
                if a[1] == 0 {
                    rects.push([0, 0, b.width, b.height]);
                } else {
                    mem.ptr(a[2], a[1] * 16, false).map_err(|_| INVALID)?;
                    for i in 0..a[1] {
                        rects.push(resource::rect(mem, a[2] + i * 16, &b)?);
                    }
                }
                if a[3] & 1 != 0 {
                    if b.locked {
                        return Err(INVALID);
                    }
                    mem.ptr(b.bits, b.len(), true).map_err(|_| INVALID)?;
                }
                let depth = if a[3] & 6 != 0 {
                    let depth = self.buffer(self.device(h)?.depth)?;
                    let z = f32::from_bits(a[5]);
                    {
                        let ds = depth.borrow();
                        if ds.locked
                            || ds.width < b.width
                            || ds.height < b.height
                            || !z.is_finite()
                            || !(0.0..=1.0).contains(&z)
                        {
                            return Err(INVALID);
                        }
                        mem.ptr(ds.bits, ds.len(), true).map_err(|_| INVALID)?;
                    }
                    Some(depth)
                } else {
                    None
                };
                if a[3] & 1 != 0 {
                    let color = if b.format == 22 {
                        a[4] | 0xff000000
                    } else {
                        a[4]
                    };
                    for r in &rects {
                        let row = color.to_le_bytes().repeat((r[2] - r[0]) as usize);
                        for y in r[1]..r[3] {
                            mem.write_bytes(b.bits + y * b.pitch() + r[0] * 4, &row)
                                .map_err(|_| INVALID)?;
                        }
                    }
                }
                if let Some(depth) = depth {
                    let ds = depth.borrow();
                    let z = (f32::from_bits(a[5]) as f64 * 0xffffff as f64).round() as u32;
                    let mask = (if a[3] & 2 != 0 { 0xffffff00 } else { 0 })
                        | if a[3] & 4 != 0 { 0xff } else { 0 };
                    let value = (z << 8) | (a[6] & 0xff);
                    for r in &rects {
                        let mut row = value.to_le_bytes().repeat((r[2] - r[0]) as usize);
                        for y in r[1]..r[3] {
                            let p = ds.bits + y * ds.pitch() + r[0] * 4;
                            if mask != u32::MAX {
                                mem.read_bytes(p, &mut row).map_err(|_| INVALID)?;
                                for pixel in row.as_chunks_mut::<4>().0 {
                                    let old = u32::from_le_bytes(*pixel);
                                    *pixel = ((old & !mask) | (value & mask)).to_le_bytes();
                                }
                            }
                            mem.write_bytes(p, &row).map_err(|_| INVALID)?;
                        }
                    }
                }
                Ok(0)
            }
            "SetViewport" => {
                let v = words::<6>(mem, a[1])?;
                // Wine accepts empty viewports too; raster coverage clips to
                // the stored rectangle and target, so zero size writes no pixels.
                if v[0].checked_add(v[2]).is_none_or(|n| n > 16384)
                    || v[1].checked_add(v[3]).is_none_or(|n| n > 16384)
                    || !f32::from_bits(v[4]).is_finite()
                    || !f32::from_bits(v[5]).is_finite()
                {
                    return Err(INVALID);
                }
                self.device_mut(h)?.viewport = v;
                Ok(0)
            }
            "GetViewport" => {
                put(mem, a[1], &self.device(h)?.viewport)?;
                Ok(0)
            }
            "SetScissorRect" => {
                let r = words::<4>(mem, a[1])?.map(|v| v as i32);
                if r[0] > r[2] || r[1] > r[3] {
                    return Err(INVALID);
                }
                self.device_mut(h)?.scissor = r;
                Ok(0)
            }
            "GetScissorRect" => {
                put(mem, a[1], &self.device(h)?.scissor.map(|v| v as u32))?;
                Ok(0)
            }
            "SetTexture" => {
                if a[1] > 1 {
                    return Err(UNAVAILABLE);
                }
                self.bind_slot(h, a[2], true, a[1], &mut resources.allocations)?;
                Ok(0)
            }
            "GetTexture" => {
                if a[1] > 1 {
                    return Err(UNAVAILABLE);
                }
                let d = self.device(h)?;
                let t = if a[1] == 0 { d.texture } else { d.texture1 };
                if t == 0 {
                    put(mem, a[2], &[0])?;
                    Ok(0)
                } else {
                    self.output_ref(t, mem, a[2])
                }
            }
            "SetFVF" => {
                if !matches!(a[1], 0x102 | 0x142 | 0x104 | 0x144) {
                    return Err(UNAVAILABLE);
                }
                self.set_declaration(h, 0, &mut resources.allocations)?;
                self.device_mut(h)?.fvf = a[1];
                Ok(0)
            }
            "CreateVertexDeclaration" => {
                self.create_declaration(a, mem, api, &mut resources.allocations)
            }
            "SetVertexDeclaration" => {
                self.set_declaration(h, a[1], &mut resources.allocations)?;
                self.device_mut(h)?.fvf = 0;
                Ok(0)
            }
            "GetVertexDeclaration" => {
                let decl = self.device(h)?.declaration;
                if decl == 0 {
                    put(mem, a[1], &[0])?;
                    Ok(0)
                } else {
                    self.output_ref(decl, mem, a[1])
                }
            }
            "SetTransform" | "GetTransform" | "MultiplyTransform" => {
                if !matches!(a[1], 2 | 3 | 256) {
                    return Err(UNAVAILABLE);
                }
                let identity = std::array::from_fn(|i| if i % 5 == 0 { 1. } else { 0. });
                let current = self
                    .device(h)?
                    .transforms
                    .get(&a[1])
                    .copied()
                    .unwrap_or(identity);
                if m == "GetTransform" {
                    put(mem, a[2], &current.map(f32::to_bits))?;
                } else {
                    let matrix = words::<16>(mem, a[2])?.map(f32::from_bits);
                    if matrix.iter().any(|v| !v.is_finite()) {
                        return Err(INVALID);
                    }
                    let matrix = if m == "MultiplyTransform" {
                        std::array::from_fn(|i| {
                            (0..4)
                                .map(|k| matrix[i / 4 * 4 + k] * current[k * 4 + i % 4])
                                .sum()
                        })
                    } else {
                        matrix
                    };
                    self.device_mut(h)?.transforms.insert(a[1], matrix);
                }
                Ok(0)
            }
            "GetFVF" => {
                put(mem, a[1], &[self.device(h)?.fvf])?;
                Ok(0)
            }
            "CreatePixelShader" => {
                self.create_pixel_shader(a, mem, api, &mut resources.allocations)
            }
            "CreateVertexShader" => {
                self.create_vertex_shader(a, mem, api, &mut resources.allocations)
            }
            "SetPixelShader" => self.set_pixel_shader(h, a[1], &mut resources.allocations),
            "GetPixelShader" => {
                let s = self.device(h)?.pixel_shader;
                if s == 0 {
                    put(mem, a[1], &[0])?;
                    Ok(0)
                } else {
                    self.output_ref(s, mem, a[1])
                }
            }
            "SetPixelShaderConstantF" | "GetPixelShaderConstantF" => {
                let start = a[1] as usize;
                let count = a[3] as usize;
                if start >= 32 || count > 32 - start {
                    return Err(INVALID);
                }
                if m.starts_with("Set") {
                    let mut values = Vec::with_capacity(count);
                    for i in 0..count {
                        values.push(
                            words::<4>(mem, a[2].checked_add((i * 16) as u32).ok_or(INVALID)?)?
                                .map(f32::from_bits),
                        );
                    }
                    self.device_mut(h)?.pixel_constants[start..start + count]
                        .copy_from_slice(&values);
                } else {
                    put(
                        mem,
                        a[2],
                        &self.device(h)?.pixel_constants[start..start + count]
                            .iter()
                            .flatten()
                            .map(|v| v.to_bits())
                            .collect::<Vec<_>>(),
                    )?;
                }
                Ok(0)
            }
            "SetVertexShader" => self.set_vertex_shader(h, a[1], &mut resources.allocations),
            "GetVertexShader" => {
                let s = self.device(h)?.vertex_shader;
                if s == 0 {
                    put(mem, a[1], &[0])?;
                    Ok(0)
                } else {
                    self.output_ref(s, mem, a[1])
                }
            }
            "SetVertexShaderConstantF" | "GetVertexShaderConstantF" => {
                let start = a[1] as usize;
                let count = a[3] as usize;
                if start >= 256 || count > 256 - start {
                    return Err(INVALID);
                }
                if m.starts_with("Set") {
                    let mut values = Vec::with_capacity(count);
                    for i in 0..count {
                        values.push(
                            words::<4>(mem, a[2].checked_add((i * 16) as u32).ok_or(INVALID)?)?
                                .map(f32::from_bits),
                        );
                    }
                    self.device_mut(h)?.vertex_constants[start..start + count]
                        .copy_from_slice(&values);
                } else {
                    put(
                        mem,
                        a[2],
                        &self.device(h)?.vertex_constants[start..start + count]
                            .iter()
                            .flatten()
                            .map(|v| v.to_bits())
                            .collect::<Vec<_>>(),
                    )?;
                }
                Ok(0)
            }
            "SetRenderState" => {
                if !raster::render_state(a[1], a[2]) {
                    return Err(UNAVAILABLE);
                }
                self.device_mut(h)?.render.insert(a[1], a[2]);
                Ok(0)
            }
            "GetRenderState" => {
                put(mem, a[2], &[raster::render(self.device(h)?, a[1])?])?;
                Ok(0)
            }
            "SetSamplerState" => {
                if a[1] > 1 || !raster::sampler_state(a[2], a[3]) {
                    return Err(UNAVAILABLE);
                }
                self.device_mut(h)?.sampler.insert(a[1] * 16 + a[2], a[3]);
                Ok(0)
            }
            "GetSamplerState" => {
                if a[1] > 1 {
                    return Err(UNAVAILABLE);
                }
                put(
                    mem,
                    a[3],
                    &[raster::sampler_slot(self.device(h)?, a[1], a[2])?],
                )?;
                Ok(0)
            }
            "SetTextureStageState" => {
                if a[1] > 1 || !raster::stage_state(a[1], a[2], a[3]) {
                    return Err(UNAVAILABLE);
                }
                self.device_mut(h)?.stage.insert(a[1] * 32 + a[2], a[3]);
                Ok(0)
            }
            "GetTextureStageState" => {
                put(mem, a[3], &[raster::stage(self.device(h)?, a[1], a[2])?])?;
                Ok(0)
            }
            "ValidateDevice" => {
                raster::validate(self.device(h)?)?;
                put(mem, a[1], &[1])?;
                Ok(0)
            }
            "DrawPrimitiveUP" => self.draw(h, a, mem),
            "CreateVertexBuffer" | "CreateIndexBuffer" => self.create_vertices(
                m == "CreateIndexBuffer",
                a,
                mem,
                api,
                &mut resources.allocations,
            ),
            "SetIndices" => self.set_indices(a, &mut resources.allocations),
            "GetIndices" => {
                put(mem, a[1], &[0])?;
                let buffer = self.device(h)?.indices;
                if buffer != 0 {
                    self.output_ref(buffer, mem, a[1])?;
                }
                Ok(0)
            }
            "DrawIndexedPrimitive" => self.draw_indexed(a, mem),
            "SetStreamSource" => self.set_stream(a, &mut resources.allocations),
            "GetStreamSource" => {
                if a[1] != 0 {
                    return Err(UNAVAILABLE);
                }
                for p in [a[2], a[3], a[4]] {
                    mem.ptr(p, 4, true).map_err(|_| INVALID)?;
                }
                let [buffer, offset, stride] = self.device(h)?.stream;
                put(mem, a[2], &[0])?;
                put(mem, a[3], &[offset])?;
                put(mem, a[4], &[stride])?;
                if buffer != 0 {
                    self.output_ref(buffer, mem, a[2])?;
                }
                Ok(0)
            }
            "DrawPrimitive" => self.draw_stream(a, mem),
            _ => Err(UNAVAILABLE),
        }
    }
    pub(super) fn swap(
        &mut self,
        m: &str,
        a: &Args,
        mem: &GuestMem,
        resources: &mut DeviceResources<'_>,
    ) -> Result<u32, u32> {
        let device = match self.objects[&a[0]].payload {
            Payload::Swap { device } => device,
            _ => return Err(INVALID),
        };
        match m {
            "GetDevice" => self.output_ref(device, mem, a[1]),
            "GetPresentParameters" => {
                put(mem, a[1], &self.device(device)?.params)?;
                Ok(0)
            }
            "GetDisplayMode" => {
                put(mem, a[1], &[1024, 768, 60, 22])?;
                Ok(0)
            }
            "GetBackBuffer" => {
                if a[1] != 0 || a[2] != 0 {
                    return Err(INVALID);
                }
                self.output_ref(self.device(device)?.back, mem, a[3])
            }
            "Present" => {
                if a[5] != 0 {
                    return Err(UNAVAILABLE);
                }
                self.present(device, mem, resources, a[1], a[2], a[3], a[4])
            }
            _ => Err(UNAVAILABLE),
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn present(
        &mut self,
        h: u32,
        mem: &GuestMem,
        resources: &mut DeviceResources<'_>,
        src: u32,
        dst: u32,
        hwnd: u32,
        dirty: u32,
    ) -> Result<u32, u32> {
        if dirty != 0 {
            return Err(UNAVAILABLE);
        }
        let d = self.device(h)?;
        if d.scene {
            return Err(INVALID);
        }
        let hwnd = if hwnd == 0 { d.hwnd } else { hwnd };
        let size = resources.windows.get(&hwnd).ok_or(INVALID)?.client_size();
        let w = size[0].max(1) as u32;
        let height = size[1].max(1) as u32;
        let data = self.buffer(d.back)?;
        let b = data.borrow();
        if b.locked {
            return Err(INVALID);
        }
        let sr = resource::rect(mem, src, &b)?;
        let dr = if dst == 0 {
            [0, 0, w, height]
        } else {
            words::<4>(mem, dst)?
        };
        if dr != [0, 0, w, height] {
            return Err(UNAVAILABLE);
        }
        let source = resource::read(mem, &b)?;
        let count = w
            .checked_mul(height)
            .filter(|&n| n <= 16 * 1024 * 1024)
            .ok_or(NOMEM)?;
        let pixels = if sr == [0, 0, w, height] && b.width == w && b.height == height {
            source
        } else {
            let mut pixels = vec![0; count as usize];
            for y in 0..height {
                for x in 0..w {
                    let sy = sr[1] as u64 + y as u64 * (sr[3] - sr[1]) as u64 / height as u64;
                    let sx = sr[0] as u64 + x as u64 * (sr[2] - sr[0]) as u64 / w as u64;
                    pixels[(y * w + x) as usize] = source[(sy * b.width as u64 + sx) as usize];
                }
            }
            pixels
        };
        resources
            .graphics
            .upload(hwnd, w, height, &pixels)
            .map_err(|_| NOMEM)?;
        self.frames += 1;
        if self.first_present_seconds.is_none() {
            self.first_present_seconds = self.run_start.map(|t| t.elapsed().as_secs_f64());
        }
        self.last_presented = hwnd;
        Ok(0)
    }
}
