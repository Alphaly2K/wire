// Shared DXGI/D3D11 owners; the embedding runtime supplies memory and call boundaries.
use super::{Memory, PAGE, READ, WRITE};
use crate::gpu::{AdapterInfo, Backend};
use crate::win32::catalog::GraphicsExportApi;
use std::collections::BTreeMap;
#[path = "dxgi/abi.rs"]
mod abi;
#[path = "dxgi/calls.rs"]
mod calls;
#[path = "dxgi/d3d11.rs"]
mod d3d11;
#[path = "dxgi/objects.rs"]
mod objects;
#[path = "dxgi/output.rs"]
mod output;
#[path = "dxgi/swapchain.rs"]
mod swapchain;

pub(super) const DLL: &str = "wire.dxgi";
const INVALID: u64 = 0x887a0001;
const NOT_FOUND: u64 = 0x887a0002;
const MORE_DATA: u64 = 0x887a0003;
const UNSUPPORTED: u64 = 0x887a0004;
const NO_INTERFACE: u64 = 0x80004002;
const INVALID_ARG: u64 = 0x80070057;
const OUT_OF_MEMORY: u64 = 0x8007000e;
const OBJECT_LIMIT: usize = 1024;
const DATA_LIMIT: usize = 65536;
/// COM method gateways are owned by the embedding executor.
pub(crate) trait Bindings {
    fn check_capacity(&self, _names: &[&'static str]) -> Result<(), String> {
        Ok(())
    }
    fn register(&mut self, dll: &str, name: &'static str) -> Result<u64, String>;
}
pub(crate) trait Notes {
    fn note(&mut self, message: String);
}
impl Notes for () {
    fn note(&mut self, _message: String) {}
}
pub(crate) struct Window {
    pub width: u32,
    pub height: u32,
    pub visible: bool,
}
pub(crate) trait Windows {
    fn has_window(&self, handle: u64) -> bool;
    fn window(&self, handle: u64) -> Option<Window>;
    fn dimensions(&self) -> [u32; 2];
    fn monitor(&self) -> u64;
    fn device_name(&self) -> &str;
    fn refresh_hz(&self) -> u32;
}
type Guid = [u8; 16];
const fn guid(a: u32, b: u16, c: u16, d: [u8; 8]) -> Guid {
    let a = a.to_le_bytes();
    let b = b.to_le_bytes();
    let c = c.to_le_bytes();
    [
        a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], d[0], d[1], d[2], d[3], d[4], d[5], d[6],
        d[7],
    ]
}
const UNKNOWN: Guid = guid(0, 0, 0, [0xc0, 0, 0, 0, 0, 0, 0, 0x46]);
const OBJECT: Guid = guid(
    0xaec22fb8,
    0x76f3,
    0x4639,
    [0x9b, 0xe0, 0x28, 0xeb, 0x43, 0xa6, 0x7a, 0x2e],
);
const FACTORIES: [Guid; 5] = [
    guid(
        0x7b7166ec,
        0x21c7,
        0x44ae,
        [0xb2, 0x1a, 0xc9, 0xae, 0x32, 0x1a, 0xe3, 0x69],
    ),
    guid(
        0x770aae78,
        0xf26f,
        0x4dba,
        [0xa8, 0x29, 0x25, 0x3c, 0x83, 0xd1, 0xb3, 0x87],
    ),
    guid(
        0x50c83a1c,
        0xe072,
        0x4c48,
        [0x87, 0xb0, 0x36, 0x30, 0xfa, 0x36, 0xa6, 0xd0],
    ),
    guid(
        0x25483823,
        0xcd46,
        0x4c7d,
        [0x86, 0xca, 0x47, 0xaa, 0x95, 0xb8, 0x37, 0xbd],
    ),
    guid(
        0x1bc6ea02,
        0xef36,
        0x464f,
        [0xbf, 0x0c, 0x21, 0xca, 0x39, 0xe5, 0x16, 0x8a],
    ),
];
const ADAPTERS: [Guid; 2] = [
    guid(
        0x2411e7e1,
        0x12ac,
        0x4ccf,
        [0xbd, 0x14, 0x97, 0x98, 0xe8, 0x53, 0x4d, 0xc0],
    ),
    guid(
        0x29038f61,
        0x3839,
        0x4626,
        [0x91, 0xfd, 0x08, 0x68, 0x79, 0x01, 0x1a, 0x05],
    ),
];
// Slots 0..6 are common; factory4 appends through slot 27, adapter1 through 10.
type MethodAbi = (&'static str, usize, &'static [usize]);
const COMMON: &[MethodAbi] = &[
    ("Object.QueryInterface", 3, &[]),
    ("Object.AddRef", 1, &[]),
    ("Object.Release", 1, &[]),
    ("Object.SetPrivateData", 4, &[2]),
    ("Object.SetPrivateDataInterface", 3, &[]),
    ("Object.GetPrivateData", 4, &[]),
    ("Object.GetParent", 3, &[]),
];
const FACTORY: &[MethodAbi] = &[
    ("Factory.EnumAdapters", 3, &[1]),
    ("Factory.MakeWindowAssociation", 3, &[2]),
    ("Factory.GetWindowAssociation", 2, &[]),
    ("Factory.CreateSwapChain", 4, &[]),
    ("Factory.CreateSoftwareAdapter", 3, &[]),
    ("Factory.EnumAdapters1", 3, &[1]),
    ("Factory.IsCurrent", 1, &[]),
    ("Factory.IsWindowedStereoEnabled", 1, &[]),
    ("Factory.CreateSwapChainForHwnd", 7, &[]),
    ("Factory.CreateSwapChainForCoreWindow", 6, &[]),
    ("Factory.GetSharedResourceAdapterLuid", 3, &[]),
    ("Factory.RegisterOcclusionStatusWindow", 4, &[]),
    ("Factory.RegisterStereoStatusEvent", 3, &[]),
    ("Factory.UnregisterStereoStatus", 2, &[]),
    ("Factory.RegisterStereoStatusWindow", 4, &[]),
    ("Factory.RegisterOcclusionStatusEvent", 3, &[]),
    ("Factory.UnregisterOcclusionStatus", 2, &[]),
    ("Factory.CreateSwapChainForComposition", 5, &[]),
    ("Factory.GetCreationFlags", 1, &[]),
    ("Factory.EnumAdapterByLuid", 4, &[]),
    ("Factory.EnumWarpAdapter", 3, &[]),
];
const ADAPTER: &[MethodAbi] = &[
    ("Adapter.EnumOutputs", 3, &[1]),
    ("Adapter.GetDesc", 2, &[]),
    ("Adapter.CheckInterfaceSupport", 3, &[]),
    ("Adapter.GetDesc1", 2, &[]),
];

#[derive(Clone, Copy)]
enum MethodRoute {
    Dxgi,
    D3d11,
}
#[derive(Clone, Copy)]
pub(crate) struct BoundMethod {
    abi: &'static MethodAbi,
    route: MethodRoute,
}
impl BoundMethod {
    #[allow(dead_code)]
    pub(crate) fn floats(self) -> &'static [usize] {
        match self.abi.0 {
            "D3D.Context.ClearDepthStencilView" => &[3],
            "D3D.Context.SetResourceMinLOD" => &[2],
            _ => &[],
        }
    }
    #[allow(dead_code)]
    pub(crate) fn name(self) -> &'static str {
        self.abi.0
    }
    pub(crate) fn arity(self) -> usize {
        self.abi.1
    }
    pub(crate) fn dwords(self) -> &'static [usize] {
        self.abi.2
    }
}
pub(super) fn bind_method(name: &str) -> Option<BoundMethod> {
    if let Some(abi) = d3d11::method_abi(name) {
        return Some(BoundMethod {
            abi,
            route: MethodRoute::D3d11,
        });
    }
    COMMON
        .iter()
        .chain(FACTORY)
        .chain(ADAPTER)
        .chain(output::METHODS)
        .chain(swapchain::METHODS)
        .find(|(n, _, _)| *n == name)
        .map(|abi| BoundMethod {
            abi,
            route: MethodRoute::Dxgi,
        })
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Factory,
    Adapter,
    Output,
    SwapChain,
    Device,
    Context,
    DxgiDevice,
    Query,
    Buffer,
    Texture,
    Shader,
    Blend,
    Depth,
    Raster,
    Sampler,
    Layout,
    Srv,
    Rtv,
    Dsv,
}
struct Object {
    kind: Kind,
    refs: u32,
    parent: u64,
    association: u64,
    private: BTreeMap<Guid, Vec<u8>>,
}
#[derive(Default)]
pub(super) struct Graphics {
    backend: Option<Backend>,
    objects: BTreeMap<u64, Object>,
    tables: BTreeMap<Kind, u64>,
    devices: BTreeMap<u64, d3d11::Device>,
    queries: BTreeMap<u64, d3d11::Query>,
    contexts: BTreeMap<u64, d3d11::Context>,
    resources: BTreeMap<u64, d3d11::Resource>,
    swapchains: BTreeMap<u64, swapchain::SwapChain>,
    presented: u64,
    drawn: u64,
    #[cfg(test)]
    fixture: Option<AdapterInfo>,
}
impl Graphics {
    #[allow(dead_code)]
    pub(super) fn pending(&self) -> bool {
        self.backend.as_ref().is_some_and(Backend::pending)
    }
    #[allow(dead_code)]
    pub(super) fn clear_pending(&self) {
        if let Some(backend) = &self.backend {
            backend.clear_pending();
        }
    }

    fn info(&self) -> Result<&AdapterInfo, String> {
        #[cfg(test)]
        if let Some(info) = &self.fixture {
            return Ok(info);
        }
        self.backend
            .as_ref()
            .map(Backend::adapter)
            .ok_or_else(|| "Win64 DXGI requires an explicitly configured graphics backend".into())
    }
    fn luid(&self) -> Result<u64, String> {
        // Process-local adapter identifier derived from the backend profile;
        // not a Windows kernel LUID or an address of the native device.
        Ok(u64::from_le_bytes(self.info()?.uuid[..8].try_into().unwrap()).max(1))
    }
    fn retain(&mut self, address: u64) -> Result<u64, String> {
        let o = self
            .objects
            .get_mut(&address)
            .ok_or("invalid DXGI object")?;
        o.refs = o.refs.checked_add(1).ok_or("DXGI reference capacity")?;
        Ok(o.refs as u64)
    }
    fn retain_many(&mut self, addresses: &[u64]) -> Result<(), String> {
        let mut counts = BTreeMap::<u64, u32>::new();
        for &p in addresses.iter().filter(|&&p| p != 0) {
            *counts.entry(p).or_default() += 1;
        }
        for (&p, &count) in &counts {
            let o = self.objects.get(&p).ok_or("invalid graphics binding")?;
            o.refs
                .checked_add(count)
                .ok_or("graphics binding reference capacity")?;
        }
        for (p, count) in counts {
            self.objects.get_mut(&p).unwrap().refs += count;
        }
        Ok(())
    }
}
fn read_guid(mem: &Memory, address: u64) -> Result<Guid, String> {
    let mut iid = [0; 16];
    mem.read(address, &mut iid)?;
    Ok(iid)
}
fn supports(kind: Kind, iid: &Guid) -> bool {
    *iid == UNKNOWN
        || (*iid == OBJECT
            && matches!(
                kind,
                Kind::Factory | Kind::Adapter | Kind::Output | Kind::DxgiDevice | Kind::SwapChain
            ))
        || match kind {
            Kind::Factory => FACTORIES.contains(iid),
            Kind::Adapter => ADAPTERS.contains(iid),
            Kind::Output => output::IIDS.contains(iid),
            Kind::SwapChain => swapchain::IIDS.contains(iid),
            _ => d3d11::supports(kind, iid),
        }
}

/// Shared object services borrow only their owners and executor adapters.
pub(super) struct Calls<'a> {
    pub graphics: &'a mut Graphics,
    pub gateways: &'a mut dyn Bindings,
    pub next_alloc: &'a mut u64,
    pub trace: &'a mut dyn Notes,
    pub user: &'a dyn Windows,
}
impl Calls<'_> {
    pub(super) fn export(
        &mut self,
        mem: &mut Memory,
        id: GraphicsExportApi,
        a: &[u64],
    ) -> Result<u64, String> {
        if matches!(
            id,
            GraphicsExportApi::D3d11Device | GraphicsExportApi::D3d11DeviceAndSwapChain
        ) {
            return self.d3d11_create(mem, id == GraphicsExportApi::D3d11DeviceAndSwapChain, a);
        }
        self.graphics
            .export_call(mem, self.gateways, self.next_alloc, id, a)
    }
    pub(super) fn d3d11_create(
        &mut self,
        mem: &mut Memory,
        combined: bool,
        a: &[u64],
    ) -> Result<u64, String> {
        d3d11::Calls {
            graphics: self.graphics,
            gateways: self.gateways,
            next_alloc: self.next_alloc,
            trace: self.trace,
        }
        .create(mem, combined, a)
    }
    pub(super) fn d3d11_call(
        &mut self,
        mem: &mut Memory,
        name: &str,
        a: &[u64],
    ) -> Result<u64, String> {
        d3d11::Calls {
            graphics: self.graphics,
            gateways: self.gateways,
            next_alloc: self.next_alloc,
            trace: self.trace,
        }
        .call(mem, name, a)
    }
    pub(super) fn create_swapchain(
        &mut self,
        mem: &mut Memory,
        name: &str,
        a: &[u64],
    ) -> Result<u64, String> {
        swapchain::Calls {
            graphics: self.graphics,
            gateways: self.gateways,
            next_alloc: self.next_alloc,
            trace: self.trace,
            user: self.user,
        }
        .create(mem, name, a)
    }
    pub(super) fn swapchain_call(
        &mut self,
        mem: &mut Memory,
        name: &str,
        a: &[u64],
    ) -> Result<u64, String> {
        swapchain::Calls {
            graphics: self.graphics,
            gateways: self.gateways,
            next_alloc: self.next_alloc,
            trace: self.trace,
            user: self.user,
        }
        .call(mem, name, a)
    }
    pub(super) fn method(
        &mut self,
        mem: &mut Memory,
        binding: BoundMethod,
        a: &[u64],
    ) -> Result<u64, String> {
        let name = binding.abi.0;
        if matches!(binding.route, MethodRoute::D3d11) {
            return self.d3d11_call(mem, name, a);
        }
        let o = self
            .graphics
            .objects
            .get(&a[0])
            .ok_or("invalid DXGI object")?;
        let kind = o.kind;
        let parent = o.parent;
        if kind == Kind::DxgiDevice && name == "Object.QueryInterface" {
            return self.d3d11_call(mem, "D3D.Dxgi.QueryInterface", a);
        }
        if (name.starts_with("Factory.") && kind != Kind::Factory)
            || (name.starts_with("Adapter.") && kind != Kind::Adapter)
            || (name.starts_with("Output.") && kind != Kind::Output)
            || (name.starts_with("SwapChain.") && kind != Kind::SwapChain)
        {
            return Err("DXGI method/object mismatch".into());
        }
        if name.starts_with("Output.") {
            return self.graphics.output_call(mem, name, a, self.user);
        }
        if name.starts_with("SwapChain.") {
            return self.swapchain_call(mem, name, a);
        }
        match name {
            "Factory.CreateSwapChain" | "Factory.CreateSwapChainForHwnd" => {
                self.create_swapchain(mem, name, a)
            }
            _ => self.graphics.scalar_call(
                mem,
                self.gateways,
                self.next_alloc,
                self.user,
                parent,
                name,
                a,
            ),
        }
    }
}

fn ascii(mem: &Memory, ptr: u64) -> Result<String, String> {
    let mut bytes = Vec::new();
    for n in 0..32768u64 {
        let byte = mem.read_int(ptr.checked_add(n).ok_or("string overflow")?, 1)? as u8;
        if byte == 0 {
            return String::from_utf8(bytes)
                .map_err(|_| "non-ASCII Win64 export/module name".into());
        }
        bytes.push(byte);
    }
    Err("unterminated Win64 export/module name".into())
}
