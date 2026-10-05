use super::*;
impl D3D9 {
    pub(super) fn bind(
        &mut self,
        h: u32,
        resource: u32,
        texture: bool,
        state: &mut ApiState,
    ) -> Result<(), u32> {
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
            self.pin(resource)?;
        }
        let d = self.device_mut(h)?;
        let old = if texture {
            std::mem::replace(&mut d.texture, resource)
        } else {
            std::mem::replace(&mut d.target, resource)
        };
        let o = self.objects.get_mut(&h).unwrap();
        if resource != 0 {
            o.children.push(resource);
        }
        if old != 0 {
            let i = o.children.iter().position(|&r| r == old).unwrap();
            o.children.remove(i);
            self.unpin(old, state);
        }
        Ok(())
    }
    pub(super) fn device_call(
        &mut self,
        m: &str,
        a: &Args,
        mem: &mut GuestMem,
        api: &mut ApiTable,
        state: &mut ApiState,
    ) -> Result<u32, u32> {
        let h = a[0];
        match m {
            "TestCooperativeLevel" => Ok(0),
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
                        state,
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
            "Present" => self.present(h, mem, state, a[1], a[2], a[3], a[4]),
            "CreateTexture" => {
                mem.ptr(a[7], 4, true).map_err(|_| INVALID)?;
                put(mem, a[7], &[0])?;
                if a[3] != 1 || a[4] & !0x201 != 0 || a[6] > 3 || a[8] != 0 {
                    return Err(UNAVAILABLE);
                }
                let data = self.pixels(a[1], a[2], a[5], a[6], a[4], mem, state)?;
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
                    state,
                ) {
                    Ok(t) => t,
                    Err(e) => {
                        self.free_buffer(data, state);
                        return Err(e);
                    }
                };
                let surface = match self.surface(h, data, 0, texture, mem, api, state) {
                    Ok(s) => s,
                    Err(e) => {
                        self.release(texture, state)?;
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
                let data = self.pixels(a[1], a[2], a[3], pool, usage, mem, state)?;
                let s = match self.surface(h, data.clone(), 1, 0, mem, api, state) {
                    Ok(s) => s,
                    Err(e) => {
                        self.free_buffer(data, state);
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
                self.bind(h, a[2], false, state)?;
                Ok(0)
            }
            "GetRenderTarget" => {
                if a[1] != 0 {
                    return Err(INVALID);
                }
                self.output_ref(self.device(h)?.target, mem, a[2])
            }
            "SetDepthStencilSurface" => {
                if a[1] == 0 {
                    Ok(0)
                } else {
                    Err(UNAVAILABLE)
                }
            }
            "GetDepthStencilSurface" => {
                put(mem, a[1], &[0])?;
                Err(0x88760866)
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
                if a[3] != 1 || a[1] > 4096 || (a[1] != 0 && a[2] == 0) {
                    return Err(UNAVAILABLE);
                }
                let target = self.device(h)?.target;
                if a[1] == 0 {
                    self.fill(mem, target, 0, a[4])?;
                } else {
                    mem.ptr(a[2], a[1] * 16, false).map_err(|_| INVALID)?;
                    for i in 0..a[1] {
                        resource::rect(mem, a[2] + i * 16, &self.buffer(target)?.borrow())?;
                    }
                    for i in 0..a[1] {
                        self.fill(mem, target, a[2] + i * 16, a[4])?;
                    }
                }
                Ok(0)
            }
            "SetViewport" => {
                let v = words::<6>(mem, a[1])?;
                let b = self.buffer(self.device(h)?.target)?;
                let b = b.borrow();
                if v[2] == 0
                    || v[3] == 0
                    || v[0].checked_add(v[2]).is_none_or(|n| n > b.width)
                    || v[1].checked_add(v[3]).is_none_or(|n| n > b.height)
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
                if a[1] != 0 {
                    return Err(UNAVAILABLE);
                }
                self.bind(h, a[2], true, state)?;
                Ok(0)
            }
            "GetTexture" => {
                if a[1] != 0 {
                    return Err(UNAVAILABLE);
                }
                let t = self.device(h)?.texture;
                if t == 0 {
                    put(mem, a[2], &[0])?;
                    Ok(0)
                } else {
                    self.output_ref(t, mem, a[2])
                }
            }
            "SetFVF" => {
                if !matches!(a[1], 0x104 | 0x144) {
                    return Err(UNAVAILABLE);
                }
                self.device_mut(h)?.fvf = a[1];
                Ok(0)
            }
            "GetFVF" => {
                put(mem, a[1], &[self.device(h)?.fvf])?;
                Ok(0)
            }
            "SetVertexShader" | "SetPixelShader" => {
                if a[1] == 0 {
                    Ok(0)
                } else {
                    Err(UNAVAILABLE)
                }
            }
            "GetVertexShader" | "GetPixelShader" => {
                put(mem, a[1], &[0])?;
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
                if a[1] != 0 || !raster::sampler_state(a[2], a[3]) {
                    return Err(UNAVAILABLE);
                }
                self.device_mut(h)?.sampler.insert(a[2], a[3]);
                Ok(0)
            }
            "GetSamplerState" => {
                if a[1] != 0 {
                    return Err(UNAVAILABLE);
                }
                put(mem, a[3], &[raster::sampler(self.device(h)?, a[2])?])?;
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
            _ => Err(UNAVAILABLE),
        }
    }
    pub(super) fn swap(
        &mut self,
        m: &str,
        a: &Args,
        mem: &GuestMem,
        state: &mut ApiState,
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
                self.present(device, mem, state, a[1], a[2], a[3], a[4])
            }
            _ => Err(UNAVAILABLE),
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn present(
        &mut self,
        h: u32,
        mem: &GuestMem,
        state: &mut ApiState,
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
        let size = state
            .user32
            .windows
            .get(&hwnd)
            .ok_or(INVALID)?
            .client_size();
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
        let mut pixels = vec![0; count as usize];
        for y in 0..height {
            for x in 0..w {
                let sy = sr[1] as u64 + y as u64 * (sr[3] - sr[1]) as u64 / height as u64;
                let sx = sr[0] as u64 + x as u64 * (sr[2] - sr[0]) as u64 / w as u64;
                pixels[(y * w + x) as usize] = source[(sy * b.width as u64 + sx) as usize];
            }
        }
        state
            .graphics
            .upload(hwnd, w, height, &pixels)
            .map_err(|_| NOMEM)?;
        self.frames += 1;
        self.last_presented = hwnd;
        Ok(0)
    }
}
