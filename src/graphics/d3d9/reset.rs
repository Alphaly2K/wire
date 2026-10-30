//! Replace implicit surfaces only after parameters and resource leases validate.
use super::*;
impl<G: RasterBackend> D3D9<G> {
    pub(super) fn reset<B: Backend>(&mut self, a: &Args, mem: &mut B) -> Result<u32, B::Error> {
        let h = a[0];
        mem.check(a[1], 56, true)?;
        let mut params = words::<14, _>(mem, a[1])?;
        let d = self.device(h)?;
        if d.scene || self.recording.contains_key(&h) {
            return Err(INVALID.into());
        }
        if params[4] != 0 || params[9] > 1 || params[3] > 1 || (params[9] == 1 && params[10] != 75)
        {
            return Err(UNAVAILABLE.into());
        }
        let hwnd = if params[7] == 0 { d.hwnd } else { params[7] };
        let size = mem.window(hwnd).ok_or(INVALID)?;
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
                return Err(INVALID.into());
            }
        }
        let data = self.pixels(params[0], params[1], params[2], 0, 1, mem)?;
        let back = match self.surface(h, data.clone(), 1, 0, mem) {
            Ok(back) => back,
            Err(e) => {
                self.free_buffer(data, mem)?;
                return Err(e);
            }
        };
        drop(data);
        let depth = if params[9] == 1 {
            match self.create_depth(h, params[0], params[1], mem) {
                Ok(depth) => depth,
                Err(e) => {
                    self.release(back, mem)?;
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
                self.unpin(child, mem)?;
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
        d.texture1 = 0;
        d.stream = [0; 3];
        d.indices = 0;
        d.fvf = 0;
        d.declaration = 0;
        d.vertex_shader = 0;
        d.vertex_constants = [[0.; 4]; 256];
        d.pixel_shader = 0;
        d.pixel_constants = [[0.; 4]; 32];
        d.render.clear();
        d.sampler.clear();
        d.stage.clear();
        d.transforms.clear();
        d.viewport = [0, 0, params[0], params[1], 0, 1f32.to_bits()];
        d.scissor = [0, 0, params[0] as i32, params[1] as i32];
        self.release(back, mem)?;
        if depth != 0 {
            self.release(depth, mem)?;
        }
        self.unpin(h, mem)?;
        // Match pinned Wine d3d9's successful Reset: report the resolved
        // swapchain dimensions/count/format, including windowed fallback.
        put(mem, a[1], &params)?;
        Ok(0)
    }
}
