//! Replace implicit surfaces only after parameters and resource leases validate.
use super::*;
impl D3D9 {
    pub(super) fn reset(
        &mut self,
        a: &Args,
        mem: &mut GuestMem,
        api: &mut ApiTable,
        state: &mut ApiState,
    ) -> Result<u32, u32> {
        let h = a[0];
        mem.ptr(a[1], 56, true).map_err(|_| INVALID)?;
        let mut params = words::<14>(mem, a[1])?;
        let d = self.device(h)?;
        if d.scene {
            return Err(INVALID);
        }
        if params[4] != 0 || params[9] > 1 || params[3] > 1 || (params[9] == 1 && params[10] != 75)
        {
            return Err(UNAVAILABLE);
        }
        let hwnd = if params[7] == 0 { d.hwnd } else { params[7] };
        let size = state
            .user32
            .windows
            .get(&hwnd)
            .ok_or(INVALID)?
            .client_size();
        if params[0] == 0 {
            params[0] = size[0].max(1) as u32;
        }
        if params[1] == 0 {
            params[1] = size[1].max(1) as u32;
        }
        if params[2] == 0 {
            params[2] = 22;
        }
        params[3] = 1;
        params[7] = hwnd;
        for o in self.objects.values() {
            let blocks = match &o.payload {
                Payload::Texture { device, data, .. } | Payload::Surface { device, data, .. } => {
                    *device == h && data.borrow().pool == 0 && (o.refs != 0 || data.borrow().locked)
                }
                Payload::Vertices {
                    device,
                    pool,
                    locked,
                    ..
                } => *device == h && *pool == 0 && (o.refs != 0 || *locked),
                Payload::Swap { device } => *device == h && o.refs != 0,
                _ => false,
            };
            if blocks {
                return Err(INVALID);
            }
        }
        let data = self.pixels(params[0], params[1], params[2], 0, 1, mem, state)?;
        let back = match self.surface(h, data.clone(), 1, 0, mem, api, state) {
            Ok(back) => back,
            Err(e) => {
                self.free_buffer(data, state);
                return Err(e);
            }
        };
        drop(data);
        let depth = if params[9] == 1 {
            match self.create_depth(h, params[0], params[1], mem, api, state) {
                Ok(depth) => depth,
                Err(e) => {
                    self.release(back, state)?;
                    return Err(e);
                }
            }
        } else {
            0
        };
        self.pin(h)?;
        let swap = self.device(h)?.swap;
        let old = std::mem::take(&mut self.objects.get_mut(&h).unwrap().children);
        // Swap-chain COM identity survives Reset; all bindings return to defaults.
        for child in old {
            if child == swap {
                self.objects.get_mut(&h).unwrap().children.push(child);
            } else {
                self.unpin(child, state);
            }
        }
        self.pin(back)?;
        self.pin(back)?;
        self.objects
            .get_mut(&h)
            .unwrap()
            .children
            .extend([back, back]);
        if depth != 0 {
            self.pin(depth)?;
            self.objects.get_mut(&h).unwrap().children.push(depth);
        }
        let d = self.device_mut(h)?;
        d.params = params;
        d.hwnd = hwnd;
        d.back = back;
        d.target = back;
        d.depth = depth;
        d.texture = 0;
        d.stream = [0; 3];
        d.indices = 0;
        d.fvf = 0;
        d.declaration = 0;
        d.render.clear();
        d.sampler.clear();
        d.stage.clear();
        d.transforms.clear();
        d.viewport = [0, 0, params[0], params[1], 0, 1f32.to_bits()];
        d.scissor = [0, 0, params[0] as i32, params[1] as i32];
        self.release(back, state)?;
        if depth != 0 {
            self.release(depth, state)?;
        }
        self.unpin(h, state);
        // Windows Reset leaves these three caller fields zeroed.
        params[0] = 0;
        params[1] = 0;
        params[3] = 0;
        if params[8] != 0 {
            params[2] = 0;
        }
        put(mem, a[1], &params)?;
        Ok(0)
    }
}
