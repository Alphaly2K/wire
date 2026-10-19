//! Windowed DXGI discard/sequential swapchains with owned native back buffers.
//! Public Win64 ABI reviewed against DXVK Native c8dc91fa dxgi*.h.
use super::*;
pub(super) const IIDS: [Guid; 3] = [
    guid(
        0x310d36a0,
        0xd2e7,
        0x4c0a,
        [0xaa, 4, 0x6a, 0x9d, 0x23, 0xb8, 0x88, 0x6a],
    ),
    guid(
        0x790a45f7,
        0x0d42,
        0x4876,
        [0x98, 0x3a, 0x0a, 0x55, 0xcf, 0xe6, 0xf4, 0xaa],
    ),
    guid(
        0x3d3e0379,
        0xf9de,
        0x4d58,
        [0xbb, 0x6c, 0x18, 0xd6, 0x29, 0x92, 0xf1, 0xa6],
    ), // DeviceSubObject.
];
pub(super) const METHODS: &[MethodAbi] = &[
    ("SwapChain.GetDevice", 3, &[]),
    ("SwapChain.Present", 3, &[1, 2]),
    ("SwapChain.GetBuffer", 4, &[1]),
    ("SwapChain.SetFullscreenState", 3, &[1]),
    ("SwapChain.GetFullscreenState", 3, &[]),
    ("SwapChain.GetDesc", 2, &[]),
    ("SwapChain.ResizeBuffers", 6, &[1, 2, 3, 4, 5]),
    ("SwapChain.ResizeTarget", 2, &[]),
    ("SwapChain.GetContainingOutput", 2, &[]),
    ("SwapChain.GetFrameStatistics", 2, &[]),
    ("SwapChain.GetLastPresentCount", 2, &[]),
    ("SwapChain.GetDesc1", 2, &[]),
    ("SwapChain.GetFullscreenDesc", 2, &[]),
    ("SwapChain.GetHwnd", 2, &[]),
    ("SwapChain.GetCoreWindow", 3, &[]),
    ("SwapChain.Present1", 4, &[1, 2]),
    ("SwapChain.IsTemporaryMonoSupported", 1, &[]),
    ("SwapChain.GetRestrictToOutput", 2, &[]),
    ("SwapChain.SetBackgroundColor", 2, &[]),
    ("SwapChain.GetBackgroundColor", 2, &[]),
    ("SwapChain.SetRotation", 2, &[1]),
    ("SwapChain.GetRotation", 2, &[]),
];
#[derive(Clone)]
struct Desc {
    mode: [u32; 7],
    sample: [u32; 2],
    usage: u32,
    count: u32,
    window: u64,
    effect: u32,
    flags: u32,
    scaling: u32,
    alpha: u32,
}
fn words(mem: &Memory, ptr: u64, count: usize) -> Result<Vec<u32>, String> {
    let mut bytes = vec![0; count * 4];
    mem.read(ptr, &mut bytes)?;
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect())
}
fn bytes(fields: &[u32]) -> Vec<u8> {
    fields.iter().flat_map(|v| v.to_le_bytes()).collect()
}
impl Desc {
    fn legacy(&self) -> Vec<u8> {
        let mut b = vec![0; 72];
        b[..28].copy_from_slice(&bytes(&self.mode));
        b[28..36].copy_from_slice(&bytes(&self.sample));
        b[36..44].copy_from_slice(&bytes(&[self.usage, self.count]));
        b[48..56].copy_from_slice(&self.window.to_le_bytes());
        b[56..68].copy_from_slice(&bytes(&[1, self.effect, self.flags]));
        b
    }
    fn extended(&self) -> Vec<u8> {
        bytes(&[
            self.mode[0],
            self.mode[1],
            self.mode[4],
            0,
            self.sample[0],
            self.sample[1],
            self.usage,
            self.count,
            self.scaling,
            self.effect,
            self.alpha,
            self.flags,
        ])
    }
    fn valid(&self) -> bool {
        self.mode[0] > 0
            && self.mode[1] > 0
            && self.mode[0] <= 8192
            && self.mode[1] <= 8192
            && u64::from(self.mode[0]) * u64::from(self.mode[1]) * 4 <= 128 * 1024 * 1024
            && matches!(self.mode[4], 28 | 29 | 87 | 91)
            && self.sample == [1, 0]
            && self.usage & 0x20 != 0
            && self.usage & !0x30 == 0
            && self.flags & !2 == 0
            && ((self.effect == 0 && matches!(self.count, 1 | 2))
                || (self.effect == 1 && self.count == 1))
            && self.scaling == 0
            && matches!(self.alpha, 0 | 3)
            && self.mode[5] <= 3
            && self.mode[6] <= 2
            && (self.mode[2] == 0 || self.mode[3] != 0)
    }
}
pub(super) struct SwapChain {
    desc: Desc,
    pub(super) device: u64,
    pub(super) buffer: u64,
    presents: u32,
}
impl Runtime64 {
    fn swapchain_buffer(&mut self, mem: &mut Memory, device: u64, d: &Desc) -> Result<u64, String> {
        let bind = 0x20 | if d.usage & 0x10 != 0 { 8 } else { 0 };
        let native = self
            .graphics
            .backend
            .as_ref()
            .ok_or("swapchain backend missing")?
            .create_texture(crate::gpu::TextureDesc {
                width: d.mode[0],
                height: d.mode[1],
                mips: 1,
                layers: 1,
                format: d.mode[4],
                bind: bind | 0x400, // Backbuffers permit UNORM/sRGB view reinterpretation.
            })?;
        let p = self.dxgi_object(mem, Kind::Texture, device)?;
        self.graphics.resources.insert(
            p,
            d3d11::Resource::Texture {
                desc: bytes(&[d.mode[0], d.mode[1], 1, 1, d.mode[4], 1, 0, 0, bind, 0, 0]),
                native,
                mapped: BTreeMap::new(),
            },
        );
        Ok(p)
    }
    pub(super) fn create_swapchain(
        &mut self,
        mem: &mut Memory,
        name: &str,
        a: &[u64],
    ) -> Result<u64, String> {
        let extended = name.ends_with("Hwnd");
        let out = a[if extended { 6 } else { 3 }];
        if out == 0 {
            return Ok(INVALID_ARG);
        }
        mem.check(out, 8, WRITE)?;
        let ptr = a[if extended { 3 } else { 2 }];
        if ptr == 0 || a[1] == 0 {
            mem.write_int(out, 8, 0)?;
            return Ok(INVALID_ARG);
        }
        let d = words(mem, ptr, if extended { 12 } else { 18 })?;
        self.trace_note(format!("DXGI swapchain descriptor={d:x?}"));
        let mut desc = if extended {
            Desc {
                mode: [d[0], d[1], 0, 0, d[2], 0, 0],
                sample: [d[4], d[5]],
                usage: d[6],
                count: d[7],
                window: a[2],
                effect: d[9],
                flags: d[11],
                scaling: d[8],
                alpha: d[10],
            }
        } else {
            Desc {
                mode: d[..7].try_into().unwrap(),
                sample: d[7..9].try_into().unwrap(),
                usage: d[9],
                count: d[10],
                window: u64::from(d[12]) | (u64::from(d[13]) << 32),
                effect: d[15],
                flags: d[16],
                scaling: 0,
                alpha: 0,
            }
        };
        let fullscreen = if extended && a[4] != 0 {
            let f = words(mem, a[4], 5)?;
            desc.mode[2..4].copy_from_slice(&f[..2]);
            desc.mode[5..7].copy_from_slice(&f[2..4]);
            f[4] == 0
        } else {
            !extended && d[14] == 0
        };
        let device = match self.graphics.objects.get(&a[1]) {
            Some(o) if o.kind == Kind::Device => a[1],
            Some(o) if o.kind == Kind::DxgiDevice => o.parent,
            _ => {
                mem.write_int(out, 8, 0)?;
                return Ok(INVALID_ARG);
            }
        };
        let window = self
            .window_info()
            .into_iter()
            .find(|w| w.handle == desc.window);
        let Some(window) = window else {
            mem.write_int(out, 8, 0)?;
            return Ok(INVALID);
        };
        if desc.mode[0] == 0 {
            desc.mode[0] = window.width;
        }
        if desc.mode[1] == 0 {
            desc.mode[1] = window.height;
        }
        if fullscreen || (extended && (d[3] != 0 || a[5] != 0)) || !desc.valid() {
            mem.write_int(out, 8, 0)?;
            return Ok(UNSUPPORTED);
        }
        if self.graphics.objects.len() + 2 > OBJECT_LIMIT {
            mem.write_int(out, 8, 0)?;
            return Ok(OUT_OF_MEMORY);
        }
        // Preflight all non-buffer references before making an owned GPU image.
        for (p, count) in [(device, 2), (a[0], 1)] {
            self.graphics.objects[&p]
                .refs
                .checked_add(count)
                .ok_or("swapchain reference capacity")?;
        }
        self.graphics.retain(device)?;
        let buffer = match self.swapchain_buffer(mem, device, &desc) {
            Ok(p) => p,
            Err(e) => {
                self.dxgi_release(mem, device)?;
                return Err(e);
            }
        };
        let chain = match self.dxgi_object(mem, Kind::SwapChain, a[0]) {
            Ok(p) => p,
            Err(e) => {
                self.dxgi_release(mem, buffer)?;
                self.dxgi_release(mem, device)?;
                return Err(e);
            }
        };
        self.graphics.swapchains.insert(
            chain,
            SwapChain {
                desc,
                device,
                buffer,
                presents: 0,
            },
        );
        mem.write_int(out, 8, chain)?;
        Ok(0)
    }
    fn present_status(&self, mem: &Memory, name: &str, a: &[u64]) -> Result<u64, String> {
        let chain = self
            .graphics
            .swapchains
            .get(&a[0])
            .ok_or("invalid DXGI swapchain")?;
        if a[1] > 4 {
            return Ok(INVALID);
        }
        if a[1] > 1 || a[2] & !1 != 0 {
            return Ok(UNSUPPORTED);
        }
        if name.ends_with('1') {
            if a[3] == 0 {
                return Ok(INVALID_ARG);
            }
            let mut parameters = [0; 32];
            mem.read(a[3], &mut parameters)?;
            if u32::from_le_bytes(parameters[..4].try_into().unwrap()) != 0
                || parameters[16..32].iter().any(|&b| b != 0)
            {
                return Ok(UNSUPPORTED);
            }
        }
        let Some(w) = self
            .window_info()
            .into_iter()
            .find(|w| w.handle == chain.desc.window)
        else {
            return Ok(INVALID);
        };
        Ok(if !w.visible || w.width == 0 || w.height == 0 {
            0x087a0001
        } else {
            0
        })
    }
    pub(in crate::x64::runtime) fn dxgi_present_needed(
        &self,
        mem: &Memory,
        name: &str,
        a: &[u64],
    ) -> Result<bool, String> {
        Ok(self.present_status(mem, name, a)? == 0 && a[2] & 1 == 0)
    }
    pub(crate) fn graphics_frame(
        &self,
        chain: u64,
    ) -> Result<(u64, crate::gpu::Texture, NativeGraphics), String> {
        let c = self
            .graphics
            .swapchains
            .get(&chain)
            .ok_or("invalid DXGI swapchain")?;
        let Some(d3d11::Resource::Texture { native, .. }) = self.graphics.resources.get(&c.buffer)
        else {
            return Err("swapchain buffer missing".into());
        };
        Ok((
            c.desc.window,
            native.clone(),
            self.graphics
                .backend
                .as_ref()
                .ok_or("swapchain backend missing")?
                .clone(),
        ))
    }
    pub(crate) fn complete_present(&mut self, dll: &str, name: &str, a: &[u64]) {
        let chain = self.graphics.swapchains.get_mut(&a[0]).unwrap();
        chain.presents = chain.presents.wrapping_add(1);
        self.graphics.presented = self.graphics.presented.saturating_add(1);
        self.api_calls += 1;
        self.unblock();
        self.record(dll, name, a, 0);
    }
    pub(super) fn swapchain_call(
        &mut self,
        mem: &mut Memory,
        name: &str,
        a: &[u64],
    ) -> Result<u64, String> {
        let chain = &self.graphics.swapchains[&a[0]];
        match name {
            "SwapChain.Present" | "SwapChain.Present1" => self.present_status(mem, name, a),
            "SwapChain.GetDevice" => self.d3d11_call(
                mem,
                "D3D.Device.QueryInterface",
                &[chain.device, a[1], a[2]],
            ),
            "SwapChain.GetBuffer" => {
                if a[3] == 0 || a[2] == 0 {
                    return Ok(INVALID_ARG);
                }
                mem.check(a[3], 8, WRITE)?;
                let iid = read_guid(mem, a[2])?;
                if a[1] != 0 {
                    mem.write_int(a[3], 8, 0)?;
                    return Ok(INVALID);
                }
                let texture2d = guid(
                    0x6f15aaf2,
                    0xd208,
                    0x4e89,
                    [0x9a, 0xb4, 0x48, 0x95, 0x35, 0xd3, 0x4f, 0x9c],
                );
                if !supports(Kind::Texture, &iid) && iid != texture2d {
                    mem.write_int(a[3], 8, 0)?;
                    return Ok(NO_INTERFACE);
                }
                let buffer = chain.buffer;
                self.graphics.retain(buffer)?;
                mem.write_int(a[3], 8, buffer)?;
                Ok(0)
            }
            "SwapChain.GetDesc" | "SwapChain.GetDesc1" | "SwapChain.GetFullscreenDesc" => {
                if a[1] == 0 {
                    return Ok(INVALID_ARG);
                }
                let d = &chain.desc;
                let b = if name.ends_with("Desc1") {
                    d.extended()
                } else if name.ends_with("FullscreenDesc") {
                    bytes(&[d.mode[2], d.mode[3], d.mode[5], d.mode[6], 1])
                } else {
                    d.legacy()
                };
                mem.write(a[1], &b)?;
                Ok(0)
            }
            "SwapChain.GetHwnd" => {
                if a[1] == 0 {
                    return Ok(INVALID_ARG);
                }
                mem.write_int(a[1], 8, chain.desc.window)?;
                Ok(0)
            }
            "SwapChain.SetFullscreenState" => Ok(if a[1] != 0 {
                UNSUPPORTED
            } else if a[2] != 0 {
                INVALID
            } else {
                0
            }),
            "SwapChain.GetFullscreenState" => {
                if a[1] != 0 {
                    mem.check(a[1], 4, WRITE)?;
                }
                if a[2] != 0 {
                    mem.check(a[2], 8, WRITE)?;
                }
                if a[1] != 0 {
                    mem.write_int(a[1], 4, 0)?;
                }
                if a[2] != 0 {
                    mem.write_int(a[2], 8, 0)?;
                }
                Ok(0)
            }
            "SwapChain.GetContainingOutput" => {
                if a[1] == 0 {
                    return Ok(INVALID_ARG);
                }
                mem.check(a[1], 8, WRITE)?;
                let adapter = self.graphics.objects[&chain.device].parent;
                self.dxgi_output(mem, adapter, a[1])
            }
            "SwapChain.GetLastPresentCount" => {
                if a[1] == 0 {
                    return Ok(INVALID_ARG);
                }
                mem.write_int(a[1], 4, chain.presents as u64)?;
                Ok(0)
            }
            "SwapChain.GetRestrictToOutput" => {
                if a[1] == 0 {
                    return Ok(INVALID_ARG);
                }
                mem.write_int(a[1], 8, 0)?;
                Ok(0)
            }
            "SwapChain.GetCoreWindow" => {
                if a[2] == 0 || a[1] == 0 {
                    return Ok(INVALID_ARG);
                }
                read_guid(mem, a[1])?;
                mem.write_int(a[2], 8, 0)?;
                Ok(NO_INTERFACE)
            }
            "SwapChain.GetRotation" => {
                if a[1] == 0 {
                    return Ok(INVALID_ARG);
                }
                mem.write_int(a[1], 4, 1)?;
                Ok(0)
            }
            "SwapChain.SetRotation" => Ok(if a[1] == 1 { 0 } else { UNSUPPORTED }),
            "SwapChain.IsTemporaryMonoSupported" => Ok(0),
            "SwapChain.ResizeBuffers" => {
                let old = chain.buffer;
                let device = chain.device;
                let mut d = chain.desc.clone();
                if self.graphics.objects[&old].refs != 1 {
                    return Ok(INVALID);
                }
                if matches!(self.graphics.resources.get(&old),Some(d3d11::Resource::Texture{mapped,..}) if !mapped.is_empty())
                {
                    return Ok(INVALID);
                }
                if a[1] != 0 {
                    d.count = a[1] as u32;
                }
                let Some(w) = self
                    .window_info()
                    .into_iter()
                    .find(|w| w.handle == d.window)
                else {
                    return Ok(INVALID);
                };
                d.mode[0] = if a[2] == 0 { w.width } else { a[2] as u32 };
                d.mode[1] = if a[3] == 0 { w.height } else { a[3] as u32 };
                if a[4] != 0 {
                    d.mode[4] = a[4] as u32;
                }
                d.flags = a[5] as u32;
                if !d.valid() {
                    return Ok(UNSUPPORTED);
                }
                let next = self.swapchain_buffer(mem, device, &d)?;
                self.dxgi_release(mem, old)?;
                let c = self.graphics.swapchains.get_mut(&a[0]).unwrap();
                c.buffer = next;
                c.desc = d;
                Ok(0)
            }
            _ => Ok(UNSUPPORTED),
        }
    }
    pub fn graphics_presents(&self) -> u64 {
        self.graphics.presented
    }
}
