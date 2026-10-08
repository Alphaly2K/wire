//! Experimental D3D11-to-native-Vulkan bridge. This is a compatibility subset,
//! not a certified full feature-level implementation. Unsupported operations fail.
use super::*;
mod draw;
mod layout;
pub(in crate::x64::runtime) fn export(name: &str) -> bool {
    matches!(name, "D3D11CreateDevice" | "D3D11CreateDeviceAndSwapChain")
}
const DEVICE: Guid = guid(
    0xdb6f6ddb,
    0xac77,
    0x4e88,
    [0x82, 0x53, 0x81, 0x9d, 0xf9, 0xbb, 0xf1, 0x40],
);
const CONTEXT: Guid = guid(
    0xc0bfa96c,
    0xe089,
    0x44fb,
    [0x8e, 0xaf, 0x26, 0xf8, 0x79, 0x61, 0x90, 0xda],
);
const CHILD: Guid = guid(
    0x1841e5c8,
    0x16b0,
    0x489b,
    [0xbc, 0xc8, 0x44, 0xcf, 0xb0, 0xd5, 0xde, 0xae],
);
const DXGI_DEVICE: Guid = guid(
    0x54ec77fa,
    0x1377,
    0x44e6,
    [0x8c, 0x32, 0x88, 0xfd, 0x5f, 0x44, 0xc8, 0x4c],
);
const DXGI_DEVICE1: Guid = guid(
    0x77db970f,
    0x6276,
    0x48ba,
    [0xba, 0x28, 0x07, 0x01, 0x43, 0xb4, 0x39, 0x2c],
);
const DXGI: &[(&str, usize, &[usize])] = &[
    ("D3D.Dxgi.GetAdapter", 2, &[]),
    ("D3D.Dxgi.CreateSurface", 6, &[2, 3]),
    ("D3D.Dxgi.QueryResourceResidency", 4, &[3]),
    ("D3D.Dxgi.SetGPUThreadPriority", 2, &[1]),
    ("D3D.Dxgi.GetGPUThreadPriority", 2, &[]),
    ("D3D.Dxgi.SetMaximumFrameLatency", 2, &[1]),
    ("D3D.Dxgi.GetMaximumFrameLatency", 2, &[]),
];
const QUERY_IID: Guid = guid(
    0xd6c00747,
    0x87b7,
    0x425e,
    [0xb8, 0x4d, 0x44, 0xd1, 0x08, 0x56, 0x0a, 0xfd],
);
const ASYNC_IID: Guid = guid(
    0x4b35d0cd,
    0x1e15,
    0x4258,
    [0x9c, 0x98, 0x1b, 0x13, 0x33, 0xf6, 0xdd, 0x3b],
);
const QUERY: &[(&str, usize, &[usize])] = &[
    ("D3D.Query.QueryInterface", 3, &[]),
    ("D3D.Query.AddRef", 1, &[]),
    ("D3D.Query.Release", 1, &[]),
    ("D3D.Query.GetDevice", 2, &[]),
    ("D3D.Query.GetPrivateData", 4, &[]),
    ("D3D.Query.SetPrivateData", 4, &[2]),
    ("D3D.Query.SetPrivateDataInterface", 3, &[]),
    ("D3D.Query.GetDataSize", 1, &[]),
    ("D3D.Query.GetDesc", 2, &[]),
];
const CHILD_ABI: &[(&str, usize, &[usize])] = &[
    ("D3D.Child.QueryInterface", 3, &[]),
    ("D3D.Child.AddRef", 1, &[]),
    ("D3D.Child.Release", 1, &[]),
    ("D3D.Child.GetDevice", 2, &[]),
    ("D3D.Child.GetPrivateData", 4, &[]),
    ("D3D.Child.SetPrivateData", 4, &[2]),
    ("D3D.Child.SetPrivateDataInterface", 3, &[]),
];
const RESOURCE_ABI: &[(&str, usize, &[usize])] = &[
    ("D3D.Resource.GetType", 2, &[]),
    ("D3D.Resource.SetEvictionPriority", 2, &[1]),
    ("D3D.Resource.GetEvictionPriority", 1, &[]),
    ("D3D.Resource.GetDesc", 2, &[]),
];
const STATE_ABI: &[(&str, usize, &[usize])] = &[("D3D.State.GetDesc", 2, &[])];
const VIEW_ABI: &[(&str, usize, &[usize])] = &[
    ("D3D.View.GetResource", 2, &[]),
    ("D3D.View.GetDesc", 2, &[]),
];
pub(super) enum NativeView {
    Image(crate::gpu::ImageView),
    Buffer(crate::gpu::BufferView),
}
impl NativeView {
    fn clear_depth(&self, flags: u32, depth: f32, stencil: u32) -> Result<(), String> {
        match self {
            Self::Image(v) => v.clear_depth(flags, depth, stencil),
            Self::Buffer(_) => Err("depth clear needs image view".into()),
        }
    }
    fn clear(&self, color: [f32; 4]) -> Result<(), String> {
        match self {
            Self::Image(v) => v.clear(color),
            Self::Buffer(v) => Err(format!(
                "cannot clear buffer SRV as RTV: range={:?}",
                v.range()
            )),
        }
    }
}
pub(super) enum Resource {
    View {
        desc: Vec<u8>,
        resource: u64,
        native: NativeView,
    },
    Texture {
        desc: Vec<u8>,
        native: crate::gpu::Texture,
        mapped: BTreeMap<u32, (u64, u64, u32)>,
    },
    Buffer {
        desc: Vec<u8>,
        native: crate::gpu::Buffer,
        mapped: Option<(u64, u64, u32)>,
    },
    Shader(crate::gpu::Shader),
    State(Vec<u8>),
}
#[derive(Default)]
pub(super) struct Context {
    bindings: BTreeMap<String, Vec<u64>>,
    values: BTreeMap<String, Vec<u8>>,
}
impl Context {
    pub(super) fn references(self) -> Vec<u64> {
        self.bindings
            .into_values()
            .flatten()
            .filter(|&p| p != 0)
            .collect()
    }
}
pub(super) struct Query {
    desc: [u8; 8],
    started: bool,
    ready: bool,
    data: Vec<u8>,
}
pub(super) struct Device {
    flags: u32,
    level: u32,
    priority: i32,
    latency: u32,
    exception: u32,
}
pub(super) fn arity(name: &str) -> Option<usize> {
    if export(name) {
        return Some(if name.ends_with("SwapChain") { 12 } else { 10 });
    }
    abi::DEVICE
        .iter()
        .chain(abi::CONTEXT)
        .chain(DXGI)
        .chain(QUERY)
        .chain(CHILD_ABI)
        .chain(RESOURCE_ABI)
        .chain(STATE_ABI)
        .chain(VIEW_ABI)
        .find(|(n, _, _)| *n == name)
        .map(|(_, a, _)| *a)
}
pub(super) fn dwords(name: &str) -> &'static [usize] {
    if export(name) {
        return &[1, 3, 5, 6];
    }
    abi::DEVICE
        .iter()
        .chain(abi::CONTEXT)
        .chain(DXGI)
        .chain(QUERY)
        .chain(CHILD_ABI)
        .chain(RESOURCE_ABI)
        .chain(STATE_ABI)
        .chain(VIEW_ABI)
        .find(|(n, _, _)| *n == name)
        .map_or(&[], |(_, _, d)| *d)
}
pub(super) fn methods(kind: Kind) -> Vec<&'static str> {
    match kind {
        Kind::Device => abi::DEVICE.iter().map(|(n, _, _)| *n).collect(),
        Kind::Context => abi::CONTEXT.iter().map(|(n, _, _)| *n).collect(),
        Kind::Srv | Kind::Rtv | Kind::Dsv => CHILD_ABI
            .iter()
            .chain(VIEW_ABI)
            .map(|(n, _, _)| *n)
            .collect(),
        Kind::Buffer | Kind::Texture => CHILD_ABI
            .iter()
            .chain(RESOURCE_ABI)
            .map(|(n, _, _)| *n)
            .collect(),
        Kind::Shader | Kind::Layout => CHILD_ABI.iter().map(|(n, _, _)| *n).collect(),
        Kind::Blend | Kind::Depth | Kind::Raster | Kind::Sampler => CHILD_ABI
            .iter()
            .chain(STATE_ABI)
            .map(|(n, _, _)| *n)
            .collect(),
        Kind::Query => QUERY.iter().map(|(n, _, _)| *n).collect(),
        Kind::DxgiDevice => COMMON
            .iter()
            .map(|(n, _)| *n)
            .chain(DXGI.iter().map(|(n, _, _)| *n))
            .collect(),
        _ => unreachable!(),
    }
}
fn concrete_iid(kind: Kind) -> Option<Guid> {
    Some(match kind {
        Kind::Layout => guid(
            0xe4819ddc,
            0x4cf0,
            0x4025,
            [0xbd, 0x26, 0x5d, 0xe8, 0x2a, 0x3e, 0x07, 0xb7],
        ),
        Kind::Blend => guid(
            0x75b68faa,
            0x347d,
            0x4159,
            [0x8f, 0x45, 0xa0, 0x64, 0x0f, 0x01, 0xcd, 0x9a],
        ),
        Kind::Depth => guid(
            0x03823efb,
            0x8d8f,
            0x4e1c,
            [0x9a, 0xa2, 0xf6, 0x4b, 0xb2, 0xcb, 0xfd, 0xf1],
        ),
        Kind::Raster => guid(
            0x9bb4ab81,
            0xab1a,
            0x4d8f,
            [0xb5, 0x06, 0xfc, 0x04, 0x20, 0x0b, 0x6e, 0xe7],
        ),
        Kind::Sampler => guid(
            0xda6fea51,
            0x564c,
            0x4487,
            [0x98, 0x10, 0xf0, 0xd0, 0xf9, 0xb4, 0xe3, 0xa5],
        ),
        Kind::Srv => guid(
            0xb0e06fe0,
            0x8192,
            0x4e1a,
            [0xb1, 0xca, 0x36, 0xd7, 0x41, 0x47, 0x10, 0xb2],
        ),
        Kind::Rtv => guid(
            0xdfdba067,
            0x0b8d,
            0x4865,
            [0x87, 0x5b, 0xd7, 0xb4, 0x51, 0x6c, 0xc1, 0x64],
        ),
        Kind::Dsv => guid(
            0x9fdac92a,
            0x1876,
            0x48c3,
            [0xaf, 0xad, 0x25, 0xb9, 0x4f, 0x84, 0xa9, 0xb6],
        ),
        _ => return None,
    })
}
pub(super) fn supports(kind: Kind, iid: &Guid) -> bool {
    if concrete_iid(kind).as_ref() == Some(iid) {
        return true;
    }
    match kind {
        Kind::Device => *iid == DEVICE,
        Kind::Context => *iid == CONTEXT || *iid == CHILD,
        Kind::Buffer => {
            *iid == CHILD
                || *iid
                    == guid(
                        0x48570b85,
                        0xd1ee,
                        0x4fcd,
                        [0xa2, 0x50, 0xeb, 0x35, 0x07, 0x22, 0xb0, 0x37],
                    )
                || *iid
                    == guid(
                        0xdc8e63f3,
                        0xd12b,
                        0x4952,
                        [0xb4, 0x7b, 0x5e, 0x45, 0x02, 0x6a, 0x86, 0x2d],
                    )
        }
        Kind::Shader | Kind::Layout | Kind::Blend | Kind::Depth | Kind::Raster | Kind::Sampler => {
            *iid == CHILD
        }
        Kind::Texture => {
            *iid == CHILD
                || *iid
                    == guid(
                        0xdc8e63f3,
                        0xd12b,
                        0x4952,
                        [0xb4, 0x7b, 0x5e, 0x45, 0x02, 0x6a, 0x86, 0x2d],
                    )
        }
        Kind::Srv | Kind::Rtv | Kind::Dsv => {
            *iid == CHILD
                || *iid
                    == guid(
                        0x839d1216,
                        0xbb2e,
                        0x412b,
                        [0xb7, 0xf4, 0xa9, 0xdb, 0xeb, 0xe0, 0x8e, 0xd1],
                    )
        }
        Kind::Query => *iid == QUERY_IID || *iid == ASYNC_IID || *iid == CHILD,
        Kind::DxgiDevice => *iid == DXGI_DEVICE || *iid == DXGI_DEVICE1,
        _ => false,
    }
}
fn texture_layout(format: u32, width: u32, height: u32) -> (u32, u32) {
    let (block, bytes) = match format {
        2..=4 => (1, 16),
        6..=8 => (1, 12),
        10..=14 | 16..=20 => (1, 8),
        49..=52 | 54..=59 => (1, 2),
        61..=64 => (1, 1),
        71 | 72 | 80 => (4, 8),
        74 | 75 | 77 | 78 | 83 => (4, 16),
        _ => (1, 4),
    };
    (width.div_ceil(block) * bytes, height.div_ceil(block))
}
impl Runtime64 {
    fn d3d_context(&mut self, mem: &mut Memory, device: u64) -> Result<u64, String> {
        if let Some(address) = self
            .graphics
            .objects
            .iter()
            .find_map(|(&p, o)| (o.kind == Kind::Context && o.parent == device).then_some(p))
        {
            self.graphics.retain(address)?;
            return Ok(address);
        }
        let c = self.dxgi_object(mem, Kind::Context, device)?;
        self.graphics.contexts.insert(c, Context::default());
        Ok(c)
    }
    fn d3d_bind_one(
        &mut self,
        mem: &mut Memory,
        context: u64,
        key: &str,
        p: u64,
        want: Kind,
    ) -> Result<(), String> {
        let owner = self.graphics.objects[&context].parent;
        if p != 0 {
            let o = self
                .graphics
                .objects
                .get(&p)
                .ok_or("invalid D3D11 state binding")?;
            if o.kind != want || o.parent != owner {
                return Err("D3D11 state type/device mismatch".into());
            }
            self.graphics.retain(p)?;
        }
        let old = self
            .graphics
            .contexts
            .get_mut(&context)
            .unwrap()
            .bindings
            .insert(key.into(), vec![p]);
        if let Some(old) = old {
            for p in old {
                if p != 0 {
                    self.dxgi_release(mem, p)?;
                }
            }
        }
        Ok(())
    }
    pub(in crate::x64::runtime) fn d3d11_call(
        &mut self,
        mem: &mut Memory,
        name: &str,
        a: &[u64],
    ) -> Result<u64, String> {
        if export(name) {
            if name.ends_with("SwapChain") {
                return Err("D3D11 combined device/swapchain creation not yet implemented".into());
            }
            for (p, n) in [(a[7], 8), (a[8], 4), (a[9], 8)] {
                if p != 0 {
                    mem.check(p, n, WRITE)?;
                }
            }
            if a[6] != 7 || a[2] != 0 || a[3] & !0x23 != 0 || (a[4] == 0 && a[5] != 0) || a[5] > 16
            {
                return Ok(INVALID_ARG);
            }
            if a[3] & 2 != 0 {
                return Ok(0x887a002d);
            }
            let mut levels = Vec::new();
            if a[4] != 0 {
                for i in 0..a[5] {
                    levels.push(mem.read_int(a[4] + 4 * i, 4)? as u32);
                }
            } else {
                levels.extend([0xb000, 0xa100, 0xa000, 0x9300, 0x9200, 0x9100]);
            }
            let level = levels.into_iter().find(|&l| l == 0xa000);
            let adapter = if a[0] != 0 {
                if a[1] != 0
                    || self
                        .graphics
                        .objects
                        .get(&a[0])
                        .is_none_or(|o| o.kind != Kind::Adapter)
                {
                    return Ok(INVALID_ARG);
                }
                a[0]
            } else {
                if a[1] != 1 {
                    return Ok(UNSUPPORTED);
                }
                // The default hardware adapter is the same attached Vulkan device.
                self.graphics.info()?;
                let factory = self.dxgi_object(mem, Kind::Factory, 0)?;
                let adapter = self.dxgi_object(mem, Kind::Adapter, factory)?;
                self.dxgi_release(mem, factory)?;
                adapter
            };
            self.graphics.info()?;
            let Some(level) = level else {
                for p in [a[7], a[9]] {
                    if p != 0 {
                        mem.write_int(p, 8, 0)?;
                    }
                }
                if a[0] == 0 {
                    self.dxgi_release(mem, adapter)?;
                }
                return Ok(UNSUPPORTED);
            };
            let device = self.dxgi_object(mem, Kind::Device, adapter)?;
            self.graphics.devices.insert(
                device,
                Device {
                    flags: a[3] as u32,
                    level,
                    priority: 0,
                    latency: 3,
                    exception: 0,
                },
            );
            if a[0] == 0 {
                self.dxgi_release(mem, adapter)?;
            }
            let context = if a[9] != 0 {
                self.d3d_context(mem, device)?
            } else {
                0
            };
            if a[7] != 0 {
                mem.write_int(a[7], 8, device)?;
            }
            if a[8] != 0 {
                mem.write_int(a[8], 4, level as u64)?;
            }
            if a[9] != 0 {
                mem.write_int(a[9], 8, context)?;
            }
            if a[7] == 0 {
                self.dxgi_release(mem, device)?;
            }
            self.trace_note(format!("experimental D3D11 native Vulkan compatibility subset: level={level:#x}; full FL10_0 conformance not claimed"));
            return Ok(0);
        }
        let o = self
            .graphics
            .objects
            .get(&a[0])
            .ok_or("invalid D3D11 object")?;
        let kind = o.kind;
        let parent = o.parent;
        let owner = if kind == Kind::Device { a[0] } else { parent };
        if (name.starts_with("D3D.Device.") && kind != Kind::Device)
            || (name.starts_with("D3D.Context.") && kind != Kind::Context)
            || (name.starts_with("D3D.Dxgi.") && kind != Kind::DxgiDevice)
        {
            return Err("D3D11 method/object mismatch".into());
        }
        if name.starts_with("D3D.Query.") && kind != Kind::Query {
            return Err("D3D11 query/object mismatch".into());
        }
        if (name.starts_with("D3D.Resource.") && !matches!(kind, Kind::Buffer | Kind::Texture))
            || (name.starts_with("D3D.View.") && !matches!(kind, Kind::Srv | Kind::Rtv | Kind::Dsv))
            || (name.starts_with("D3D.State.")
                && !matches!(
                    kind,
                    Kind::Blend | Kind::Depth | Kind::Raster | Kind::Sampler
                ))
            || (name.starts_with("D3D.Child.")
                && !matches!(
                    kind,
                    Kind::Buffer
                        | Kind::Texture
                        | Kind::Shader
                        | Kind::Layout
                        | Kind::Blend
                        | Kind::Depth
                        | Kind::Raster
                        | Kind::Sampler
                        | Kind::Srv
                        | Kind::Rtv
                        | Kind::Dsv
                ))
        {
            return Err("D3D11 child/object mismatch".into());
        }
        let method = name.rsplit('.').next().unwrap();
        match method {
            "CreateInputLayout" => self.create_input_layout(mem, a),
            "QueryInterface" => {
                if a[1] == 0 || a[2] == 0 {
                    return Ok(INVALID_ARG);
                }
                mem.check(a[2], 8, WRITE)?;
                let iid = read_guid(mem, a[1])?;
                let resource_iid = match self.graphics.resources.get(&a[0]) {
                    Some(Resource::Texture { native, .. }) => {
                        Some(if native.desc().bind & 0x200 != 0 {
                            guid(
                                0x037e866e,
                                0xf56d,
                                0x4357,
                                [0xa8, 0xaf, 0x9d, 0xab, 0xbe, 0x6e, 0x25, 0x0e],
                            )
                        } else {
                            guid(
                                0x6f15aaf2,
                                0xd208,
                                0x4e89,
                                [0x9a, 0xb4, 0x48, 0x95, 0x35, 0xd3, 0x4f, 0x9c],
                            )
                        })
                    }
                    Some(Resource::Shader(s)) => Some(match s.stage {
                        1 => guid(
                            0x3b301d64,
                            0xd678,
                            0x4289,
                            [0x88, 0x97, 0x22, 0xf8, 0x92, 0x8b, 0x72, 0xf3],
                        ),
                        2 => guid(
                            0xea82e40d,
                            0x51dc,
                            0x4f33,
                            [0x93, 0xd4, 0xdb, 0x7c, 0x91, 0x25, 0xae, 0x8c],
                        ),
                        8 => guid(
                            0x38325b96,
                            0xeffb,
                            0x4022,
                            [0xba, 0x02, 0x2e, 0x79, 0x5b, 0x70, 0x27, 0x5c],
                        ),
                        _ => guid(
                            0x4f5b196e,
                            0xc2bd,
                            0x495e,
                            [0xbd, 0x01, 0x1f, 0xde, 0xd3, 0x8e, 0x49, 0x69],
                        ),
                    }),
                    _ => None,
                };
                let target = if kind == Kind::DxgiDevice && (iid == UNKNOWN || iid == DEVICE) {
                    self.graphics.retain(owner)?;
                    owner
                } else if kind == Kind::Device && supports(Kind::DxgiDevice, &iid) {
                    if let Some(p) = self.graphics.objects.iter().find_map(|(&p, o)| {
                        (o.kind == Kind::DxgiDevice && o.parent == a[0]).then_some(p)
                    }) {
                        self.graphics.retain(p)?;
                        p
                    } else {
                        self.dxgi_object(mem, Kind::DxgiDevice, a[0])?
                    }
                } else if iid == UNKNOWN
                    || resource_iid == Some(iid)
                    || supports(kind, &iid)
                    || (kind == Kind::DxgiDevice && iid == OBJECT)
                {
                    self.graphics.retain(a[0])?;
                    a[0]
                } else {
                    mem.write_int(a[2], 8, 0)?;
                    return Ok(NO_INTERFACE);
                };
                mem.write_int(a[2], 8, target)?;
                Ok(0)
            }
            "AddRef" => self.graphics.retain(a[0]),
            "Release" => self.dxgi_release(mem, a[0]),
            "GetDevice" => {
                mem.check(a[1], 8, WRITE)?;
                self.graphics.retain(owner)?;
                mem.write_int(a[1], 8, owner)?;
                Ok(0)
            }
            "GetImmediateContext" => {
                mem.check(a[1], 8, WRITE)?;
                let c = self.d3d_context(mem, a[0])?;
                mem.write_int(a[1], 8, c)?;
                Ok(0)
            }
            "CreateShaderResourceView" | "CreateRenderTargetView" | "CreateDepthStencilView" => {
                if a[3] != 0 {
                    mem.check(a[3], 8, WRITE)?;
                }
                if let Some(Resource::Buffer {
                    desc: buffer_desc,
                    native,
                    ..
                }) = self.graphics.resources.get(&a[1])
                {
                    if method != "CreateShaderResourceView"
                        || a[2] == 0
                        || self.graphics.objects[&a[1]].parent != owner
                    {
                        return Ok(INVALID_ARG);
                    }
                    let mut desc = vec![0; 24];
                    mem.read(a[2], &mut desc)?;
                    let d: Vec<_> = desc
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|b| u32::from_le_bytes(*b))
                        .collect();
                    let misc = u32::from_le_bytes(buffer_desc[16..20].try_into().unwrap());
                    let stride = u32::from_le_bytes(buffer_desc[20..24].try_into().unwrap());
                    let structured = misc & 0x40 != 0;
                    let bytes = if structured {
                        if d[0] != 0 || d[1] != 1 {
                            return Ok(INVALID_ARG);
                        }
                        stride
                    } else {
                        match d[0] {
                            2..=4 => 16,
                            6..=8 => 12,
                            10..=14 | 16..=18 => 8,
                            49..=52 | 54..=59 => 2,
                            61..=64 => 1,
                            _ => 4,
                        }
                    };
                    if d[1] != 1 || bytes == 0 || d[3] == 0 {
                        return Ok(INVALID_ARG);
                    }
                    let offset = d[2] as usize * bytes as usize;
                    let size = d[3] as usize * bytes as usize;
                    let native = native.view(d[0], offset, size)?;
                    if a[3] == 0 {
                        return Ok(1);
                    }
                    self.graphics.retain(a[1])?;
                    let p = match self.dxgi_object(mem, Kind::Srv, a[0]) {
                        Ok(p) => p,
                        Err(e) => {
                            self.dxgi_release(mem, a[1])?;
                            return Err(e);
                        }
                    };
                    self.graphics.resources.insert(
                        p,
                        Resource::View {
                            desc,
                            resource: a[1],
                            native: NativeView::Buffer(native),
                        },
                    );
                    mem.write_int(a[3], 8, p)?;
                    return Ok(0);
                }
                let Some(Resource::Texture {
                    native,
                    desc: resource_desc,
                    ..
                }) = self.graphics.resources.get(&a[1])
                else {
                    return Err(format!("D3D11 view needs texture resource; args={a:x?}"));
                };
                if self.graphics.objects[&a[1]].parent != owner {
                    return Err("D3D11 cross-device image view".into());
                }
                let d = native.desc();
                let kind = match method {
                    "CreateShaderResourceView" => Kind::Srv,
                    "CreateRenderTargetView" => Kind::Rtv,
                    _ => Kind::Dsv,
                };
                let volume = d.bind & 0x200 != 0;
                let array = !volume && d.layers > 1;
                let size = if kind == Kind::Rtv { 20 } else { 24 };
                let mut desc = vec![0; size];
                let mut v = crate::gpu::ImageViewDesc {
                    format: d.format,
                    mip: 0,
                    mips: if kind == Kind::Srv { d.mips } else { 1 },
                    layer: 0,
                    layers: if volume { 1 } else { d.layers },
                    kind: match kind {
                        Kind::Srv => 0,
                        Kind::Rtv => 1,
                        _ => 2,
                    },
                };
                if a[2] != 0 {
                    mem.read(a[2], &mut desc)?;
                    let words: Vec<_> = desc
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|b| u32::from_le_bytes(*b))
                        .collect();
                    v.format = words[0];
                    let dimension = words[1];
                    let base = if kind == Kind::Dsv { 3 } else { 2 };
                    if kind == Kind::Dsv && words[2] != 0 {
                        return Err("readonly D3D11 DSV unsupported".into());
                    }
                    let plain = if kind == Kind::Dsv { 3 } else { 4 };
                    if dimension != plain
                        && dimension != plain + 1
                        && !(kind == Kind::Srv && matches!(dimension, 8..=10))
                    {
                        return Err("D3D11 image view dimension unsupported".into());
                    }
                    v.mip = words[base];
                    if kind == Kind::Srv {
                        v.mips = if words[3] == u32::MAX {
                            d.mips.saturating_sub(v.mip)
                        } else {
                            words[3]
                        };
                    }
                    if kind == Kind::Srv && dimension == 8 {
                        v.kind = 5;
                        v.layer = 0;
                        v.layers = 1;
                    } else if kind == Kind::Srv && matches!(dimension, 9 | 10) {
                        v.kind = if dimension == 9 { 3 } else { 4 };
                        v.layer = if dimension == 9 { 0 } else { words[4] };
                        v.layers = if dimension == 9 {
                            6
                        } else {
                            words[5].checked_mul(6).ok_or("cube view layer overflow")?
                        };
                    } else if dimension == plain + 1 {
                        let at = base + if kind == Kind::Srv { 2 } else { 1 };
                        v.layer = words[at];
                        v.layers = words[at + 1];
                    } else {
                        v.layers = 1;
                    }
                } else {
                    let cube = kind == Kind::Srv && d.bind & 0x100 != 0;
                    let dim = if volume {
                        v.kind = 5;
                        8
                    } else if cube {
                        if d.layers == 6 {
                            9
                        } else {
                            10
                        }
                    } else {
                        (if kind == Kind::Dsv { 3u32 } else { 4u32 }) + array as u32
                    };
                    if cube {
                        v.kind = if d.layers == 6 { 3 } else { 4 };
                    }
                    desc[..4].copy_from_slice(&d.format.to_le_bytes());
                    desc[4..8].copy_from_slice(&dim.to_le_bytes());
                    if kind == Kind::Srv {
                        desc[12..16].copy_from_slice(&d.mips.to_le_bytes());
                        if array && !cube {
                            desc[20..24].copy_from_slice(&d.layers.to_le_bytes());
                        } else if cube && d.layers > 6 {
                            desc[20..24].copy_from_slice(&(d.layers / 6).to_le_bytes());
                        }
                    } else if array {
                        let at = if kind == Kind::Dsv { 20 } else { 16 };
                        desc[at..at + 4].copy_from_slice(&d.layers.to_le_bytes());
                    }
                }
                let bind = d.bind & 0xff;
                let _ = resource_desc;
                let required = match kind {
                    Kind::Srv => 8,
                    Kind::Rtv => 0x20,
                    _ => 0x40,
                };
                if bind & required == 0 {
                    return Ok(INVALID_ARG);
                }
                let native = native.view(v)?;
                if a[3] == 0 {
                    return Ok(1);
                }
                self.graphics.retain(a[1])?;
                let p = match self.dxgi_object(mem, kind, a[0]) {
                    Ok(p) => p,
                    Err(e) => {
                        self.dxgi_release(mem, a[1])?;
                        return Err(e);
                    }
                };
                self.graphics.resources.insert(
                    p,
                    Resource::View {
                        desc,
                        resource: a[1],
                        native: NativeView::Image(native),
                    },
                );
                mem.write_int(a[3], 8, p)?;
                Ok(0)
            }
            "GetResource" => {
                let Some(Resource::View { resource, .. }) = self.graphics.resources.get(&a[0])
                else {
                    return Err("D3D11 view/resource mismatch".into());
                };
                let p = *resource;
                mem.check(a[1], 8, WRITE)?;
                self.graphics.retain(p)?;
                mem.write_int(a[1], 8, p)?;
                Ok(0)
            }
            "ClearRenderTargetView" => {
                let mut bytes = [0; 16];
                mem.read(a[2], &mut bytes)?;
                let Some(Resource::View { native, .. }) = self.graphics.resources.get(&a[1]) else {
                    return Err("D3D11 clear needs RTV".into());
                };
                if self.graphics.objects[&a[1]].kind != Kind::Rtv
                    || self.graphics.objects[&a[1]].parent != owner
                {
                    return Err("D3D11 clear view/device mismatch".into());
                }
                let color = std::array::from_fn(|i| {
                    f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
                });
                native.clear(color)?;
                Ok(0)
            }
            "ClearDepthStencilView" => {
                let Some(Resource::View { native, .. }) = self.graphics.resources.get(&a[1]) else {
                    return Err("depth clear needs DSV".into());
                };
                if self.graphics.objects[&a[1]].kind != Kind::Dsv
                    || self.graphics.objects[&a[1]].parent != owner
                {
                    return Err("depth clear view/device mismatch".into());
                }
                native.clear_depth(a[2] as u32, f32::from_bits(a[3] as u32), a[4] as u8 as u32)?;
                Ok(0)
            }
            "CreateTexture3D" => {
                if a[1] == 0 {
                    return Ok(INVALID_ARG);
                }
                if a[3] != 0 {
                    mem.check(a[3], 8, WRITE)?;
                }
                let mut desc = vec![0; 36];
                mem.read(a[1], &mut desc)?;
                let mut d: Vec<_> = desc
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| u32::from_le_bytes(*b))
                    .collect();
                self.trace_note(format!("D3D11 volume descriptor={d:x?}"));
                if d[..3].iter().any(|&v| v == 0 || v > 2048)
                    || d[5] > 1
                    || d[6] != 8
                    || d[7] != 0
                    || d[8] != 0
                    || (d[5] == 1 && a[2] == 0)
                {
                    if a[3] != 0 {
                        mem.write_int(a[3], 8, 0)?;
                    }
                    return Ok(UNSUPPORTED);
                }
                let maximum = 32 - d[0].max(d[1]).max(d[2]).leading_zeros();
                if d[3] == 0 {
                    d[3] = maximum;
                    desc[12..16].copy_from_slice(&d[3].to_le_bytes());
                }
                if d[3] > maximum {
                    return Ok(INVALID_ARG);
                }
                let mut initial = Vec::new();
                let mut total = 0;
                for mip in 0..d[3] {
                    let (row, rows) =
                        texture_layout(d[4], (d[0] >> mip).max(1), (d[1] >> mip).max(1));
                    let depth = (d[2] >> mip).max(1);
                    let bytes = row as usize * rows as usize * depth as usize;
                    total += bytes;
                    if total > 128 * 1024 * 1024 {
                        return Ok(OUT_OF_MEMORY);
                    }
                    if a[2] != 0 {
                        let at = a[2] + mip as u64 * 16;
                        let data = mem.read_int(at, 8)?;
                        let pitch = mem.read_int(at + 8, 4)?;
                        let slice = mem.read_int(at + 12, 4)?;
                        if data == 0 || pitch < row as u64 || slice < pitch * rows as u64 {
                            return Ok(INVALID_ARG);
                        }
                        let mut tight = vec![0; bytes];
                        for z in 0..depth {
                            for y in 0..rows {
                                let offset =
                                    (z as usize * rows as usize + y as usize) * row as usize;
                                mem.read(
                                    data + z as u64 * slice + y as u64 * pitch,
                                    &mut tight[offset..offset + row as usize],
                                )?;
                            }
                        }
                        initial.push(tight);
                    }
                }
                let native = self
                    .graphics
                    .backend
                    .as_ref()
                    .ok_or("D3D11 backend missing")?
                    .create_texture(crate::gpu::TextureDesc {
                        width: d[0],
                        height: d[1],
                        layers: d[2],
                        mips: d[3],
                        format: d[4],
                        bind: d[6] | 0x200,
                    })?;
                for (i, data) in initial.iter().enumerate() {
                    native.write(i as u32, data)?;
                }
                if a[3] == 0 {
                    return Ok(1);
                }
                let p = self.dxgi_object(mem, Kind::Texture, a[0])?;
                self.graphics.resources.insert(
                    p,
                    Resource::Texture {
                        desc,
                        native,
                        mapped: BTreeMap::new(),
                    },
                );
                mem.write_int(a[3], 8, p)?;
                Ok(0)
            }
            "CreateTexture2D" => {
                if a[1] == 0 {
                    return Ok(INVALID_ARG);
                }
                if a[3] != 0 {
                    mem.check(a[3], 8, WRITE)?;
                }
                let mut desc = vec![0; 44];
                mem.read(a[1], &mut desc)?;
                let mut d: Vec<_> = desc
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| u32::from_le_bytes(*b))
                    .collect();
                self.trace_note(format!("D3D11 texture descriptor={d:x?}"));
                if d[0] == 0
                    || d[1] == 0
                    || d[0] > 8192
                    || d[1] > 8192
                    || d[3] == 0
                    || d[3] > 256
                    || d[5] != 1
                    || d[6] != 0
                    || d[7] > 3
                    || d[8] & !0x68 != 0
                    || d[9] & !0x30000 != 0
                    || d[10] & !4 != 0
                    || (d[7] == 1 && a[2] == 0)
                    || (d[7] == 2 && d[9] != 0x10000)
                {
                    if a[3] != 0 {
                        mem.write_int(a[3], 8, 0)?;
                    }
                    return Ok(UNSUPPORTED);
                }
                if d[2] == 0 {
                    d[2] = 32 - d[0].max(d[1]).leading_zeros();
                    desc[8..12].copy_from_slice(&d[2].to_le_bytes());
                }
                if d[2] > 32 - d[0].max(d[1]).leading_zeros() {
                    return Ok(INVALID_ARG);
                }
                let mut initial = Vec::new();
                let mut total = 0;
                for layer in 0..d[3] {
                    for mip in 0..d[2] {
                        let (row, rows) =
                            texture_layout(d[4], (d[0] >> mip).max(1), (d[1] >> mip).max(1));
                        total += row as usize * rows as usize;
                        if total > 128 * 1024 * 1024 {
                            return Ok(OUT_OF_MEMORY);
                        }
                        if a[2] != 0 {
                            let at = a[2] + (layer * d[2] + mip) as u64 * 16;
                            let data = mem.read_int(at, 8)?;
                            let pitch = mem.read_int(at + 8, 4)?;
                            mem.check(at, 16, READ)?;
                            if data == 0 || pitch < row as u64 {
                                return Ok(INVALID_ARG);
                            }
                            let mut tight = vec![0; row as usize * rows as usize];
                            for y in 0..rows {
                                mem.read(
                                    data + y as u64 * pitch,
                                    &mut tight[y as usize * row as usize
                                        ..(y + 1) as usize * row as usize],
                                )?;
                            }
                            initial.push(tight);
                        }
                    }
                }
                let native = self
                    .graphics
                    .backend
                    .as_ref()
                    .ok_or("D3D11 backend missing")?
                    .create_texture(crate::gpu::TextureDesc {
                        width: d[0],
                        height: d[1],
                        mips: d[2],
                        layers: d[3],
                        format: d[4],
                        bind: d[8] | if d[10] & 4 != 0 { 0x100 } else { 0 },
                    })?;
                for (i, bytes) in initial.iter().enumerate() {
                    native.write(i as u32, bytes)?;
                }
                if a[3] == 0 {
                    return Ok(1);
                }
                let p = self.dxgi_object(mem, Kind::Texture, a[0])?;
                self.graphics.resources.insert(
                    p,
                    Resource::Texture {
                        desc,
                        native,
                        mapped: BTreeMap::new(),
                    },
                );
                mem.write_int(a[3], 8, p)?;
                Ok(0)
            }
            "CreateBuffer" => {
                if a[1] == 0 {
                    return Ok(INVALID_ARG);
                }
                if a[3] != 0 {
                    mem.check(a[3], 8, WRITE)?;
                }
                let mut desc = vec![0; 24];
                mem.read(a[1], &mut desc)?;
                let d: Vec<_> = desc
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| u32::from_le_bytes(*b))
                    .collect();
                if d[0] == 0
                    || d[0] > 64 * 1024 * 1024
                    || d[1] > 3
                    || d[2] & !0x8f != 0
                    || d[4] & !0x70 != 0
                    || d[3] & !0x30000 != 0
                    || (d[2] & 4 != 0 && (d[0] % 16 != 0 || d[0] > 65536 || d[2] != 4))
                    || (d[1] == 1 && (a[2] == 0 || d[3] != 0))
                    || (d[1] == 2 && d[3] != 0x10000)
                    || (d[1] == 3 && (d[2] != 0 || d[3] == 0))
                    || (d[4] & 0x40 != 0 && (d[5] == 0 || d[0] % d[5] != 0))
                {
                    if a[3] != 0 {
                        mem.write_int(a[3], 8, 0)?;
                    }
                    return Ok(INVALID_ARG);
                }
                let mut bytes = vec![0; d[0] as usize];
                if a[2] != 0 {
                    let data = mem.read_int(a[2], 8)?;
                    mem.read(data, &mut bytes)?;
                    mem.check(a[2], 16, READ)?;
                }
                let flags = ((d[2] & 4 != 0) as u32)
                    | (((d[2] & 0x88 != 0 || d[2] == 0) as u32) << 1)
                    | (((d[2] & 1 != 0) as u32) << 2)
                    | (((d[2] & 2 != 0) as u32) << 3);
                let native = self
                    .graphics
                    .backend
                    .as_ref()
                    .ok_or("D3D11 backend missing")?
                    .create_buffer(&bytes, flags)?;
                if a[3] == 0 {
                    return Ok(1);
                }
                let p = self.dxgi_object(mem, Kind::Buffer, a[0])?;
                self.graphics.resources.insert(
                    p,
                    Resource::Buffer {
                        desc,
                        native,
                        mapped: None,
                    },
                );
                mem.write_int(a[3], 8, p)?;
                Ok(0)
            }
            "CreateVertexShader"
            | "CreatePixelShader"
            | "CreateGeometryShader"
            | "CreateComputeShader" => {
                if a[1] == 0 || a[2] == 0 || a[2] > 1024 * 1024 || a[3] != 0 {
                    return Ok(INVALID_ARG);
                }
                if a[4] != 0 {
                    mem.check(a[4], 8, WRITE)?;
                }
                let mut bytes = vec![0; a[2] as usize];
                mem.read(a[1], &mut bytes)?;
                let stage = match method {
                    "CreateVertexShader" => 1,
                    "CreatePixelShader" => 2,
                    "CreateGeometryShader" => 8,
                    _ => 4,
                };
                let native = self
                    .graphics
                    .backend
                    .as_ref()
                    .ok_or("D3D11 backend missing")?
                    .compile(&bytes, stage)?;
                if a[4] == 0 {
                    return Ok(1);
                }
                let p = self.dxgi_object(mem, Kind::Shader, a[0])?;
                self.graphics.resources.insert(p, Resource::Shader(native));
                mem.write_int(a[4], 8, p)?;
                Ok(0)
            }
            "CreateBlendState"
            | "CreateDepthStencilState"
            | "CreateRasterizerState"
            | "CreateSamplerState" => {
                if a[1] == 0 {
                    return Ok(INVALID_ARG);
                }
                if a[2] != 0 {
                    mem.check(a[2], 8, WRITE)?;
                }
                let (k, size) = match method {
                    "CreateBlendState" => (Kind::Blend, 264),
                    "CreateDepthStencilState" => (Kind::Depth, 52),
                    "CreateRasterizerState" => (Kind::Raster, 40),
                    _ => (Kind::Sampler, 52),
                };
                let mut desc = vec![0; size];
                mem.read(a[1], &mut desc)?;
                let d: Vec<_> = desc
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| u32::from_le_bytes(*b))
                    .collect();
                let valid = match k {
                    Kind::Raster => {
                        (2..=3).contains(&d[0])
                            && (1..=3).contains(&d[1])
                            && [2, 6, 7, 8, 9].iter().all(|&i| d[i] <= 1)
                    }
                    Kind::Depth => {
                        d[0] <= 1
                            && d[1] <= 1
                            && (1..=8).contains(&d[2])
                            && d[3] <= 1
                            && [5, 6, 7, 8, 9, 10, 11, 12]
                                .iter()
                                .all(|&i| (1..=8).contains(&d[i]))
                    }
                    Kind::Sampler => {
                        d[0] & !0x1d5 == 0
                            && [1, 2, 3].iter().all(|&i| (1..=5).contains(&d[i]))
                            && (1..=16).contains(&d[5])
                            && (1..=8).contains(&d[6])
                    }
                    Kind::Blend => {
                        d[0] <= 1
                            && d[1] <= 1
                            && (0..8).all(|n| {
                                let i = 2 + n * 8;
                                d[i] <= 1
                                    && [1, 2, 4, 5].iter().all(|&j| {
                                        (1..=19).contains(&d[i + j])
                                            && d[i + j] != 12
                                            && d[i + j] != 13
                                    })
                                    && [3, 6].iter().all(|&j| (1..=5).contains(&d[i + j]))
                                    && d[i + 7] & 255 <= 15
                            })
                    }
                    _ => false,
                };
                if !valid {
                    if a[2] != 0 {
                        mem.write_int(a[2], 8, 0)?;
                    }
                    return Ok(INVALID_ARG);
                }
                if a[2] == 0 {
                    return Ok(1);
                }
                let p = self.dxgi_object(mem, k, a[0])?;
                self.graphics.resources.insert(p, Resource::State(desc));
                mem.write_int(a[2], 8, p)?;
                Ok(0)
            }
            "GetDesc" if kind != Kind::Query => match self.graphics.resources.get(&a[0]) {
                Some(Resource::Buffer { desc, .. })
                | Some(Resource::Texture { desc, .. })
                | Some(Resource::State(desc))
                | Some(Resource::View { desc, .. }) => {
                    mem.write(a[1], desc)?;
                    Ok(0)
                }
                _ => Err("D3D11 descriptor/object mismatch".into()),
            },
            "GetType" if matches!(kind, Kind::Buffer | Kind::Texture) => {
                let resource_type = if kind == Kind::Buffer {
                    1
                } else if matches!(self.graphics.resources.get(&a[0]),Some(Resource::Texture{native,..}) if native.desc().bind&0x200!=0)
                {
                    4
                } else {
                    3
                };
                mem.write_int(a[1], 4, resource_type)?;
                Ok(0)
            }
            "SetEvictionPriority" => {
                self.graphics.objects.get_mut(&a[0]).unwrap().association = a[1];
                Ok(0)
            }
            "GetEvictionPriority" => Ok(self.graphics.objects[&a[0]].association),
            "Map" => {
                mem.check(a[5], 16, WRITE)?;
                if let Some(Resource::Texture {
                    desc,
                    native,
                    mapped,
                }) = self.graphics.resources.get(&a[1])
                {
                    if self.graphics.objects[&a[1]].parent != owner || a[4] & !0x100000 != 0 {
                        return Ok(INVALID_ARG);
                    }
                    let d = native.desc();
                    if a[2] >= d.mips as u64 * d.layers as u64
                        || mapped.contains_key(&(a[2] as u32))
                    {
                        return Ok(INVALID_ARG);
                    }
                    let usage = u32::from_le_bytes(desc[28..32].try_into().unwrap());
                    let access = u32::from_le_bytes(desc[36..40].try_into().unwrap());
                    let read = matches!(a[3], 1 | 3);
                    let write = a[3] != 1;
                    if !(1..=5).contains(&a[3])
                        || (read && access & 0x20000 == 0)
                        || (write && access & 0x10000 == 0)
                        || (a[3] >= 4 && usage != 2)
                        || (a[3] < 4 && usage != 3)
                    {
                        return Ok(INVALID_ARG);
                    }
                    let mip = a[2] as u32 % d.mips;
                    let (row, rows) =
                        texture_layout(d.format, (d.width >> mip).max(1), (d.height >> mip).max(1));
                    let bytes = row as usize * rows as usize;
                    let contents = if a[3] == 4 || a[3] == 2 {
                        vec![0; bytes]
                    } else {
                        native.read(a[2] as u32, bytes)?
                    };
                    let size = (bytes as u64).div_ceil(PAGE) * PAGE;
                    let p = self.next_alloc;
                    mem.map(p, size, READ | WRITE)?;
                    mem.write(p, &contents)?;
                    self.next_alloc += size;
                    let Some(Resource::Texture { mapped, .. }) =
                        self.graphics.resources.get_mut(&a[1])
                    else {
                        unreachable!()
                    };
                    mapped.insert(a[2] as u32, (p, size, a[3] as u32));
                    let mut result = vec![0; 16];
                    result[..8].copy_from_slice(&p.to_le_bytes());
                    result[8..12].copy_from_slice(&row.to_le_bytes());
                    result[12..16].copy_from_slice(&(bytes as u32).to_le_bytes());
                    mem.write(a[5], &result)?;
                    return Ok(0);
                }
                let Some(Resource::Buffer {
                    desc,
                    native,
                    mapped,
                }) = self.graphics.resources.get(&a[1])
                else {
                    return Err("D3D11 Map supports buffers only at present".into());
                };
                if self.graphics.objects[&a[1]].parent != owner
                    || mapped.is_some()
                    || a[2] != 0
                    || !(1..=5).contains(&a[3])
                    || a[4] & !0x100000 != 0
                {
                    return Ok(INVALID_ARG);
                }
                let usage = u32::from_le_bytes(desc[4..8].try_into().unwrap());
                let access = u32::from_le_bytes(desc[12..16].try_into().unwrap());
                let read = matches!(a[3], 1 | 3);
                let write = a[3] != 1;
                if (read && access & 0x20000 == 0)
                    || (write && access & 0x10000 == 0)
                    || (a[3] >= 4 && usage != 2)
                    || (a[3] < 4 && usage != 3)
                {
                    return Ok(INVALID_ARG);
                }
                let bytes = if a[3] == 4 {
                    vec![0; native.size()]
                } else {
                    native.read(0, native.size())?
                };
                let size = (bytes.len() as u64).div_ceil(PAGE) * PAGE;
                let p = self.next_alloc;
                mem.map(p, size, READ | WRITE)?;
                mem.write(p, &bytes)?;
                self.next_alloc += size;
                let Some(Resource::Buffer { mapped, .. }) = self.graphics.resources.get_mut(&a[1])
                else {
                    unreachable!()
                };
                *mapped = Some((p, size, a[3] as u32));
                let mut result = vec![0; 16];
                result[..8].copy_from_slice(&p.to_le_bytes());
                result[8..12].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
                result[12..16].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
                mem.write(a[5], &result)?;
                Ok(0)
            }
            "Unmap" => {
                if let Some(Resource::Texture { native, mapped, .. }) =
                    self.graphics.resources.get_mut(&a[1])
                {
                    let Some((p, size, mode)) = mapped.get(&(a[2] as u32)).copied() else {
                        return Err("D3D11 texture Unmap without Map".into());
                    };
                    if mode != 1 {
                        let d = native.desc();
                        let mip = a[2] as u32 % d.mips;
                        let (row, rows) = texture_layout(
                            d.format,
                            (d.width >> mip).max(1),
                            (d.height >> mip).max(1),
                        );
                        let mut bytes = vec![0; row as usize * rows as usize];
                        mem.read(p, &mut bytes)?;
                        native.write(a[2] as u32, &bytes)?;
                    }
                    mem.unmap(p, size)?;
                    mapped.remove(&(a[2] as u32));
                    return Ok(0);
                }
                let Some(Resource::Buffer { native, mapped, .. }) =
                    self.graphics.resources.get_mut(&a[1])
                else {
                    return Err("D3D11 Unmap resource mismatch".into());
                };
                let Some((p, size, mode)) = *mapped else {
                    return Err("D3D11 Unmap without Map".into());
                };
                if a[2] != 0 {
                    return Err("D3D11 buffer subresource must be zero".into());
                }
                if mode != 1 {
                    let mut bytes = vec![0; native.size()];
                    mem.read(p, &mut bytes)?;
                    native.write(0, &bytes)?;
                }
                mem.unmap(p, size)?;
                *mapped = None;
                Ok(0)
            }
            "CopySubresourceRegion" | "CopyResource" => {
                let (dst, src, ds, ss, x, y, box_address) = if method == "CopyResource" {
                    (a[1], a[2], 0, 0, 0, 0, 0)
                } else {
                    if a[5] != 0 {
                        return Err("2D texture copy Z must be zero".into());
                    }
                    (
                        a[1],
                        a[6],
                        a[2] as u32,
                        a[7] as u32,
                        a[3] as u32,
                        a[4] as u32,
                        a[8],
                    )
                };
                if [dst, src].iter().any(|p| {
                    self.graphics
                        .objects
                        .get(p)
                        .is_none_or(|o| o.parent != owner)
                }) {
                    return Err("D3D11 cross-device copy".into());
                }
                match (
                    self.graphics.resources.get(&dst),
                    self.graphics.resources.get(&src),
                ) {
                    (
                        Some(Resource::Texture {
                            native: destination,
                            mapped: dm,
                            ..
                        }),
                        Some(Resource::Texture {
                            native: source,
                            mapped: sm,
                            ..
                        }),
                    ) => {
                        if !dm.is_empty() || !sm.is_empty() {
                            return Err("cannot copy mapped texture".into());
                        }
                        let d = destination.desc();
                        let s = source.desc();
                        if method == "CopyResource"
                            && (d.width != s.width
                                || d.height != s.height
                                || d.mips != s.mips
                                || d.layers != s.layers)
                        {
                            return Err("D3D11 CopyResource texture shape".into());
                        }
                        let subresources = |d: crate::gpu::TextureDesc| {
                            d.mips * if d.bind & 0x200 != 0 { 1 } else { d.layers }
                        };
                        let count = if method == "CopyResource" {
                            subresources(s)
                        } else {
                            1
                        };
                        for n in 0..count {
                            let ss = if method == "CopyResource" { n } else { ss };
                            let ds = if method == "CopyResource" { n } else { ds };
                            if ss >= subresources(s) || ds >= subresources(d) {
                                return Err("D3D11 texture copy subresource".into());
                            }
                            let mip = ss % s.mips;
                            let mut r = crate::gpu::TextureCopy {
                                dst_sub: ds,
                                src_sub: ss,
                                x,
                                y,
                                left: 0,
                                top: 0,
                                right: (s.width >> mip).max(1),
                                bottom: (s.height >> mip).max(1),
                            };
                            if box_address != 0 {
                                let mut b = [0; 24];
                                mem.read(box_address, &mut b)?;
                                let w: Vec<_> = b
                                    .as_chunks::<4>()
                                    .0
                                    .iter()
                                    .map(|x| u32::from_le_bytes(*x))
                                    .collect();
                                if w[2] != 0 || w[5] != 1 {
                                    return Err("D3D11 2D copy box depth".into());
                                }
                                r.left = w[0];
                                r.top = w[1];
                                r.right = w[3];
                                r.bottom = w[4];
                            }
                            destination.copy_from(source, r)?;
                        }
                        Ok(0)
                    }
                    (
                        Some(Resource::Buffer {
                            native: destination,
                            mapped: dm,
                            ..
                        }),
                        Some(Resource::Buffer {
                            native: source,
                            mapped: sm,
                            ..
                        }),
                    ) => {
                        if dm.is_some()
                            || sm.is_some()
                            || (method == "CopySubresourceRegion" && (ds != 0 || ss != 0 || y != 0))
                        {
                            return Err("D3D11 buffer copy state/subresource".into());
                        }
                        let (left, right) = if box_address == 0 {
                            (0, source.size())
                        } else {
                            let left = mem.read_int(box_address, 4)? as usize;
                            let right = mem.read_int(box_address + 12, 4)? as usize;
                            mem.check(box_address, 24, READ)?;
                            (left, right)
                        };
                        if left > right
                            || right > source.size()
                            || x as usize > destination.size()
                            || right - left > destination.size() - x as usize
                            || (method == "CopyResource" && source.size() != destination.size())
                        {
                            return Err("D3D11 buffer copy bounds".into());
                        }
                        let bytes = source.read(left, right - left)?;
                        let Some(Resource::Buffer { native, .. }) =
                            self.graphics.resources.get_mut(&dst)
                        else {
                            unreachable!()
                        };
                        native.write(x as usize, &bytes)?;
                        Ok(0)
                    }
                    _ => Err("D3D11 resource copy type mismatch".into()),
                }
            }
            "UpdateSubresource" => {
                if let Some(Resource::Texture {
                    desc,
                    native,
                    mapped,
                }) = self.graphics.resources.get(&a[1])
                {
                    let d = native.desc();
                    let volume = d.bind & 0x200 != 0;
                    let usage_at = if volume { 20 } else { 28 };
                    if self.graphics.objects[&a[1]].parent != owner
                        || !mapped.is_empty()
                        || u32::from_le_bytes(desc[usage_at..usage_at + 4].try_into().unwrap()) != 0
                        || a[2] >= d.mips as u64 * if volume { 1 } else { d.layers as u64 }
                    {
                        return Err("D3D11 texture update usage/subresource".into());
                    }
                    let mip = a[2] as u32 % d.mips;
                    let w = (d.width >> mip).max(1);
                    let h = (d.height >> mip).max(1);
                    let z = if volume { (d.layers >> mip).max(1) } else { 1 };
                    let mut box_values = [0, 0, 0, w, h, z];
                    if a[3] != 0 {
                        let mut bytes = [0; 24];
                        mem.read(a[3], &mut bytes)?;
                        for (i, x) in box_values.iter_mut().enumerate() {
                            *x = u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
                        }
                    }
                    let [left, top, front, right, bottom, back] = box_values;
                    if left > right
                        || top > bottom
                        || front > back
                        || right > w
                        || bottom > h
                        || back > z
                    {
                        return Err("D3D11 texture update box bounds".into());
                    }
                    if left == right || top == bottom || front == back {
                        return Ok(0);
                    }
                    let block = if matches!(d.format, 71 | 72 | 74 | 75 | 77 | 78 | 80 | 83) {
                        4
                    } else {
                        1
                    };
                    if left % block != 0
                        || top % block != 0
                        || (right % block != 0 && right != w)
                        || (bottom % block != 0 && bottom != h)
                    {
                        return Err("D3D11 compressed texture update alignment".into());
                    }
                    let (row, rows) = texture_layout(d.format, w, h);
                    let (partial_row, partial_rows) =
                        texture_layout(d.format, right - left, bottom - top);
                    if a[5] < partial_row as u64
                        || (back - front > 1 && a[6] < a[5] * partial_rows as u64)
                    {
                        return Err("D3D11 texture update source pitches".into());
                    }
                    let full = left == 0
                        && top == 0
                        && front == 0
                        && right == w
                        && bottom == h
                        && back == z;
                    let mut data = if full {
                        vec![0; row as usize * rows as usize * z as usize]
                    } else {
                        native.read(a[2] as u32, row as usize * rows as usize * z as usize)?
                    };
                    let (left_bytes, _) = texture_layout(d.format, left, 1);
                    for zi in front..back {
                        for yi in 0..partial_rows {
                            let offset =
                                ((zi * rows + top / block + yi) * row + left_bytes) as usize;
                            mem.read(
                                a[4] + (zi - front) as u64 * a[6] + yi as u64 * a[5],
                                &mut data[offset..offset + partial_row as usize],
                            )?;
                        }
                    }
                    native.write(a[2] as u32, &data)?;
                    return Ok(0);
                }
                let Some(Resource::Buffer {
                    native,
                    desc,
                    mapped,
                }) = self.graphics.resources.get_mut(&a[1])
                else {
                    return Err("D3D11 UpdateSubresource currently requires buffer".into());
                };
                if mapped.is_some()
                    || a[2] != 0
                    || u32::from_le_bytes(desc[4..8].try_into().unwrap()) != 0
                {
                    return Err("D3D11 update usage/subresource mismatch".into());
                }
                let (offset, size) = if a[3] == 0 {
                    (0, native.size())
                } else {
                    let mut bounds = [0; 24];
                    mem.read(a[3], &mut bounds)?;
                    let b: Vec<_> = bounds
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|x| u32::from_le_bytes(*x))
                        .collect();
                    if b[0] > b[3]
                        || b[3] as usize > native.size()
                        || b[1] != 0
                        || b[2] != 0
                        || b[4] != 1
                        || b[5] != 1
                    {
                        return Err("D3D11 buffer update box".into());
                    }
                    (b[0] as usize, (b[3] - b[0]) as usize)
                };
                let mut bytes = vec![0; size];
                mem.read(a[4], &mut bytes)?;
                native.write(offset, &bytes)?;
                Ok(0)
            }
            "IASetVertexBuffers" => {
                if a[1] > 32 || a[2] > 32 - a[1] {
                    return Err("D3D11 vertex binding range".into());
                }
                let mut incoming = Vec::new();
                let mut layout = Vec::new();
                for i in 0..a[2] {
                    let p = if a[3] == 0 {
                        0
                    } else {
                        mem.read_int(a[3] + i * 8, 8)?
                    };
                    if p != 0
                        && self
                            .graphics
                            .objects
                            .get(&p)
                            .is_none_or(|o| o.kind != Kind::Buffer || o.parent != owner)
                    {
                        return Err("D3D11 vertex buffer/device mismatch".into());
                    }
                    incoming.push(p);
                    layout.extend_from_slice(
                        &(if a[4] == 0 {
                            0
                        } else {
                            mem.read_int(a[4] + i * 4, 4)?
                        } as u32)
                            .to_le_bytes(),
                    );
                    layout.extend_from_slice(
                        &(if a[5] == 0 {
                            0
                        } else {
                            mem.read_int(a[5] + i * 4, 4)?
                        } as u32)
                            .to_le_bytes(),
                    );
                }
                self.graphics.retain_many(&incoming)?;
                let context = self.graphics.contexts.get_mut(&a[0]).unwrap();
                let slots = context
                    .bindings
                    .entry(method.into())
                    .or_insert_with(|| vec![0; 32]);
                let old = slots[a[1] as usize..(a[1] + a[2]) as usize].to_vec();
                slots[a[1] as usize..(a[1] + a[2]) as usize].copy_from_slice(&incoming);
                let values = context
                    .values
                    .entry(method.into())
                    .or_insert_with(|| vec![0; 32 * 8]);
                values[a[1] as usize * 8..(a[1] + a[2]) as usize * 8].copy_from_slice(&layout);
                for p in old {
                    if p != 0 {
                        self.dxgi_release(mem, p)?;
                    }
                }
                Ok(0)
            }
            "IASetIndexBuffer" => {
                if a[1] != 0 && !matches!(a[2], 42 | 57) {
                    return Err("D3D11 index format".into());
                }
                self.d3d_bind_one(mem, a[0], method, a[1], Kind::Buffer)?;
                let mut values = (a[2] as u32).to_le_bytes().to_vec();
                values.extend_from_slice(&(a[3] as u32).to_le_bytes());
                self.graphics
                    .contexts
                    .get_mut(&a[0])
                    .unwrap()
                    .values
                    .insert(method.into(), values);
                Ok(0)
            }
            "CreateQuery" => {
                if a[1] == 0 {
                    return Ok(INVALID_ARG);
                }
                if a[2] != 0 {
                    mem.check(a[2], 8, WRITE)?;
                }
                let mut desc = [0u8; 8];
                mem.read(a[1], &mut desc)?;
                let q = u32::from_le_bytes(desc[..4].try_into().unwrap());
                if desc[4..] != [0; 4] || !matches!(q, 0 | 2 | 3) {
                    if a[2] != 0 {
                        mem.write_int(a[2], 8, 0)?;
                    }
                    return Ok(UNSUPPORTED);
                }
                if a[2] == 0 {
                    return Ok(1);
                }
                let p = self.dxgi_object(mem, Kind::Query, a[0])?;
                self.graphics.queries.insert(
                    p,
                    Query {
                        desc,
                        started: false,
                        ready: false,
                        data: vec![
                            0;
                            match q {
                                0 => 4,
                                2 => 8,
                                _ => 16,
                            }
                        ],
                    },
                );
                mem.write_int(a[2], 8, p)?;
                Ok(0)
            }
            "GetDataSize" => Ok(self.graphics.queries[&a[0]].data.len() as u64),
            "GetDesc" if kind == Kind::Query => {
                mem.write(a[1], &self.graphics.queries[&a[0]].desc)?;
                Ok(0)
            }
            "Begin" | "End" => {
                let query = self
                    .graphics
                    .queries
                    .get(&a[1])
                    .ok_or("invalid D3D11 query")?;
                if self.graphics.objects[&a[1]].parent != owner {
                    return Err("D3D11 cross-device query".into());
                }
                let q = u32::from_le_bytes(query.desc[..4].try_into().unwrap());
                if method == "Begin" {
                    if q != 3 {
                        return Err("D3D11 Begin requires a scoped query".into());
                    }
                    let query = self.graphics.queries.get_mut(&a[1]).unwrap();
                    query.started = true;
                    query.ready = false;
                    return Ok(0);
                }
                if q == 3 && !query.started {
                    return Err("D3D11 scoped query ended without Begin".into());
                }
                let (ticks, freq) = self
                    .graphics
                    .backend
                    .as_ref()
                    .ok_or("D3D11 backend missing")?
                    .timestamp()?;
                let data = match q {
                    0 => 1u32.to_le_bytes().to_vec(),
                    2 => ticks.to_le_bytes().to_vec(),
                    _ => {
                        let mut d = vec![0; 16];
                        d[..8].copy_from_slice(&freq.to_le_bytes());
                        d
                    }
                };
                let query = self.graphics.queries.get_mut(&a[1]).unwrap();
                query.data = data;
                query.ready = true;
                query.started = false;
                Ok(0)
            }
            "GetData" => {
                let query = self
                    .graphics
                    .queries
                    .get(&a[1])
                    .ok_or("invalid D3D11 query")?;
                if self.graphics.objects[&a[1]].parent != owner {
                    return Err("D3D11 cross-device query".into());
                }
                if a[4] & !1 != 0
                    || (a[2] == 0 && a[3] != 0)
                    || (a[2] != 0 && a[3] as usize != query.data.len())
                {
                    return Ok(INVALID_ARG);
                }
                if a[2] != 0 {
                    mem.check(a[2], query.data.len(), WRITE)?;
                }
                if !query.ready {
                    return Ok(1);
                }
                if a[2] != 0 {
                    mem.write(a[2], &query.data)?;
                }
                Ok(0)
            }
            "ClearState" => {
                let old = self
                    .graphics
                    .contexts
                    .insert(a[0], Context::default())
                    .unwrap();
                for p in old.references() {
                    self.dxgi_release(mem, p)?;
                }
                Ok(0)
            }
            "Flush" => Ok(0), // all native submissions fence-complete synchronously
            method
                if kind == Kind::Context
                    && (method.ends_with("SetShaderResources")
                        || method.ends_with("SetConstantBuffers")
                        || method.ends_with("SetSamplers")) =>
            {
                let (limit, want) = if method.ends_with("SetShaderResources") {
                    (128, Some(Kind::Srv))
                } else if method.ends_with("SetConstantBuffers") {
                    (14, Some(Kind::Buffer))
                } else {
                    (16, Some(Kind::Sampler))
                };
                if a[1] > limit || a[2] > limit - a[1] {
                    return Err("D3D11 binding range".into());
                }
                let mut incoming = Vec::new();
                for i in 0..a[2] {
                    let p = if a[3] == 0 {
                        0
                    } else {
                        mem.read_int(a[3] + 8 * i, 8)?
                    };
                    if p != 0 {
                        let object = self
                            .graphics
                            .objects
                            .get(&p)
                            .ok_or("D3D11 invalid bound resource")?;
                        if want.is_some_and(|k| k != object.kind) || object.parent != owner {
                            return Err("D3D11 binding type/device mismatch".into());
                        }
                        if want.is_none() {
                            return Err(
                                "D3D11 shader resource view binding not yet implemented".into()
                            );
                        }
                        if object.refs == u32::MAX {
                            return Err("D3D11 binding reference capacity".into());
                        }
                    }
                    incoming.push(p);
                }
                self.graphics.retain_many(&incoming)?;
                let slots = self
                    .graphics
                    .contexts
                    .get_mut(&a[0])
                    .unwrap()
                    .bindings
                    .entry(method.to_owned())
                    .or_insert_with(|| vec![0; limit as usize]);
                let start = a[1] as usize;
                let end = start + incoming.len();
                let old = slots[start..end].to_vec();
                slots[start..end].copy_from_slice(&incoming);
                for p in old {
                    if p != 0 {
                        self.dxgi_release(mem, p)?;
                    }
                }
                Ok(0)
            }
            method if kind == Kind::Context && method.ends_with("SetShader") => {
                if a[3] != 0 || a[2] != 0 {
                    return Err("D3D11 dynamic shader class linkage unsupported".into());
                }
                let stage = if method.starts_with("VS") {
                    1
                } else if method.starts_with("PS") {
                    2
                } else if method.starts_with("GS") {
                    8
                } else if method.starts_with("CS") {
                    4
                } else {
                    0
                };
                if a[1] != 0
                    && !matches!(self.graphics.resources.get(&a[1]),Some(Resource::Shader(s)) if s.stage==stage)
                {
                    return Err("D3D11 shader stage mismatch".into());
                }
                self.d3d_bind_one(mem, a[0], method, a[1], Kind::Shader)?;
                Ok(0)
            }
            "IASetInputLayout" | "RSSetState" => {
                self.d3d_bind_one(
                    mem,
                    a[0],
                    method,
                    a[1],
                    if method == "RSSetState" {
                        Kind::Raster
                    } else {
                        Kind::Layout
                    },
                )?;
                Ok(0)
            }
            "OMSetBlendState" | "OMSetDepthStencilState" => {
                let mut values = if method == "OMSetBlendState" {
                    let mut b = vec![0; 16];
                    if a[2] != 0 {
                        mem.read(a[2], &mut b)?;
                    } else {
                        for n in 0..4 {
                            b[4 * n..4 * n + 4].copy_from_slice(&1f32.to_le_bytes());
                        }
                    }
                    b.extend_from_slice(&(a[3] as u32).to_le_bytes());
                    b
                } else {
                    (a[2] as u32).to_le_bytes().to_vec()
                };
                self.d3d_bind_one(
                    mem,
                    a[0],
                    method,
                    a[1],
                    if method == "OMSetBlendState" {
                        Kind::Blend
                    } else {
                        Kind::Depth
                    },
                )?;
                self.graphics
                    .contexts
                    .get_mut(&a[0])
                    .unwrap()
                    .values
                    .insert(method.into(), std::mem::take(&mut values));
                Ok(0)
            }
            "SetPredication" => {
                if a[1] != 0 {
                    return Err("D3D11 predicates unsupported".into());
                }
                self.graphics
                    .contexts
                    .get_mut(&a[0])
                    .unwrap()
                    .values
                    .insert(method.into(), vec![0]);
                Ok(0)
            }
            "SOSetTargets" => {
                if a[1] != 0 {
                    return Err("D3D11 stream output unsupported".into());
                }
                Ok(0)
            }
            "OMSetRenderTargets" => {
                if a[1] > 8 {
                    return Err("D3D11 render target count".into());
                }
                let mut targets = vec![0; 8];
                for i in 0..a[1] {
                    targets[i as usize] = if a[2] == 0 {
                        0
                    } else {
                        mem.read_int(a[2] + i * 8, 8)?
                    };
                }
                targets.push(a[3]);
                for (i, &p) in targets.iter().enumerate() {
                    if p != 0
                        && self.graphics.objects.get(&p).is_none_or(|o| {
                            o.kind != if i == 8 { Kind::Dsv } else { Kind::Rtv }
                                || o.parent != owner
                        })
                    {
                        return Err("D3D11 output view/device mismatch".into());
                    }
                }
                self.graphics.retain_many(&targets)?;
                let old = self
                    .graphics
                    .contexts
                    .get_mut(&a[0])
                    .unwrap()
                    .bindings
                    .insert(method.into(), targets);
                if let Some(old) = old {
                    for p in old {
                        if p != 0 {
                            self.dxgi_release(mem, p)?;
                        }
                    }
                }
                Ok(0)
            }
            "RSSetViewports" | "RSSetScissorRects" => {
                if a[1] > 16 {
                    return Err("D3D11 viewport/rect capacity".into());
                }
                let stride = if method == "RSSetViewports" { 24 } else { 16 };
                let mut bytes = vec![0; a[1] as usize * stride];
                mem.read(a[2], &mut bytes)?;
                self.graphics
                    .contexts
                    .get_mut(&a[0])
                    .unwrap()
                    .values
                    .insert(method.into(), bytes);
                Ok(0)
            }
            "IASetPrimitiveTopology" => {
                if a[1] > 5 && !matches!(a[1], 10..=13) {
                    return Err("unsupported D3D11 topology".into());
                }
                self.graphics
                    .contexts
                    .get_mut(&a[0])
                    .unwrap()
                    .values
                    .insert(method.to_owned(), (a[1] as u32).to_le_bytes().to_vec());
                Ok(0)
            }
            "GetFeatureLevel" => Ok(self.graphics.devices[&owner].level as u64),
            "GetCreationFlags" => Ok(self.graphics.devices[&owner].flags as u64),
            "GetDeviceRemovedReason" => Ok(0), // actual VkDevice alive; synchronous submissions propagate failures
            "GetType" => Ok(0),                // immediate context
            "GetContextFlags" => Ok(0),
            "GetExceptionMode" => Ok(self.graphics.devices[&owner].exception as u64),
            "SetExceptionMode" => {
                if a[1] != 0 {
                    return Ok(INVALID_ARG);
                }
                self.graphics.devices.get_mut(&owner).unwrap().exception = 0;
                Ok(0)
            }
            "SetGPUThreadPriority" => {
                let p = a[1] as i32;
                if !(-7..=7).contains(&p) {
                    return Ok(INVALID_ARG);
                }
                if p != 0 {
                    return Ok(UNSUPPORTED);
                }
                self.graphics.devices.get_mut(&owner).unwrap().priority = p;
                Ok(0)
            }
            "GetGPUThreadPriority" => {
                mem.write_int(
                    a[1],
                    4,
                    self.graphics.devices[&owner].priority as u32 as u64,
                )?;
                Ok(0)
            }
            "SetMaximumFrameLatency" => {
                if a[1] > 16 {
                    return Ok(INVALID_ARG);
                }
                self.graphics.devices.get_mut(&owner).unwrap().latency =
                    if a[1] == 0 { 3 } else { a[1] as u32 };
                Ok(0)
            }
            "GetMaximumFrameLatency" => {
                mem.write_int(a[1], 4, self.graphics.devices[&owner].latency as u64)?;
                Ok(0)
            }
            "GetAdapter" => {
                let adapter = self.graphics.objects[&owner].parent;
                mem.check(a[1], 8, WRITE)?;
                self.graphics.retain(adapter)?;
                mem.write_int(a[1], 8, adapter)?;
                Ok(0)
            }
            "CheckFormatSupport" => {
                mem.check(a[2], 4, WRITE)?;
                let support = self
                    .graphics
                    .backend
                    .as_ref()
                    .ok_or("D3D11 missing backend")?
                    .format_support(a[1] as u32);
                mem.write_int(a[2], 4, support as u64)?;
                Ok(if support == 0 { INVALID_ARG } else { 0 })
            }
            "CheckMultisampleQualityLevels" => {
                mem.check(a[3], 4, WRITE)?;
                let support = self
                    .graphics
                    .backend
                    .as_ref()
                    .ok_or("D3D11 missing backend")?
                    .format_support(a[1] as u32);
                // Current texture/pipeline bridge only creates single-sample resources.
                mem.write_int(a[3], 4, (support != 0 && a[2] == 1) as u64)?;
                Ok(0)
            }
            "CheckFeatureSupport" => {
                let size = match a[1] {
                    0 => 8,
                    1 => 4,
                    4 => 4,
                    5 => 56,
                    6 => 4,
                    _ => return Ok(INVALID_ARG),
                };
                if a[3] != size {
                    return Ok(INVALID_ARG);
                }
                // No threading, double precision, optional SM4 compute or 11.x extensions exposed.
                mem.write(a[2], &vec![0; size as usize])?;
                Ok(0)
            }
            "GetPrivateData" | "SetPrivateData" | "SetPrivateDataInterface" => {
                self.dxgi_call(mem, &format!("Object.{method}"), a)
            }
            "Draw" | "DrawIndexed" | "DrawInstanced" | "DrawIndexedInstanced" => {
                self.d3d_draw(method, a)
            }
            _ => Err(format!(
                "unimplemented Win64 D3D11 method: {name}; args={a:x?}"
            )),
        }
    }
}
