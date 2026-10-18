//! Host-backed DXGI discovery. Win64 COM layout is serialized explicitly;
//! host pointers never enter guest memory. D3D11 is an experimental subset.
//! ABI checked against DXVK Native c8dc91fa directx/dxgi*.h; no implementation copied.
use super::{Memory, Runtime64, PAGE, READ, WRITE};
use crate::gpu::{AdapterInfo, NativeGraphics};
use std::collections::BTreeMap;
mod abi;
mod d3d11;
mod output;
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
const COMMON: &[(&str, usize)] = &[
    ("Object.QueryInterface", 3),
    ("Object.AddRef", 1),
    ("Object.Release", 1),
    ("Object.SetPrivateData", 4),
    ("Object.SetPrivateDataInterface", 3),
    ("Object.GetPrivateData", 4),
    ("Object.GetParent", 3),
];
const FACTORY: &[(&str, usize)] = &[
    ("Factory.EnumAdapters", 3),
    ("Factory.MakeWindowAssociation", 3),
    ("Factory.GetWindowAssociation", 2),
    ("Factory.CreateSwapChain", 4),
    ("Factory.CreateSoftwareAdapter", 3),
    ("Factory.EnumAdapters1", 3),
    ("Factory.IsCurrent", 1),
    ("Factory.IsWindowedStereoEnabled", 1),
    ("Factory.CreateSwapChainForHwnd", 7),
    ("Factory.CreateSwapChainForCoreWindow", 6),
    ("Factory.GetSharedResourceAdapterLuid", 3),
    ("Factory.RegisterOcclusionStatusWindow", 4),
    ("Factory.RegisterStereoStatusEvent", 3),
    ("Factory.UnregisterStereoStatus", 2),
    ("Factory.RegisterStereoStatusWindow", 4),
    ("Factory.RegisterOcclusionStatusEvent", 3),
    ("Factory.UnregisterOcclusionStatus", 2),
    ("Factory.CreateSwapChainForComposition", 5),
    ("Factory.GetCreationFlags", 1),
    ("Factory.EnumAdapterByLuid", 4),
    ("Factory.EnumWarpAdapter", 3),
];
const ADAPTER: &[(&str, usize)] = &[
    ("Adapter.EnumOutputs", 3),
    ("Adapter.GetDesc", 2),
    ("Adapter.CheckInterfaceSupport", 3),
    ("Adapter.GetDesc1", 2),
];
pub(super) fn export(name: &str) -> bool {
    matches!(
        name,
        "CreateDXGIFactory" | "CreateDXGIFactory1" | "CreateDXGIFactory2"
    )
}
pub(super) use d3d11::export as d3d11_export;
pub(super) fn d3d12_export(name: &str) -> bool {
    matches!(name, "D3D12CreateDevice" | "D3D12GetDebugInterface")
}
pub(super) fn symbol<'a>(dll: &str, name: &'a str) -> &'a str {
    if dll.eq_ignore_ascii_case("d3d12.dll") {
        // Wine 10.0 d3d12.spec, ordinal ABI data only.
        match name {
            "#101" => "D3D12CreateDevice",
            "#102" => "D3D12GetDebugInterface",
            _ => name,
        }
    } else {
        name
    }
}
pub(super) fn arity(name: &str) -> Option<usize> {
    if let Some(arity) = d3d11::arity(name) {
        return Some(arity);
    }
    match name {
        "CreateDXGIFactory" | "CreateDXGIFactory1" => Some(2),
        "CreateDXGIFactory2" => Some(3),
        "D3D12CreateDevice" => Some(4),
        "D3D12GetDebugInterface" => Some(2),
        _ => COMMON
            .iter()
            .chain(FACTORY)
            .chain(ADAPTER)
            .chain(output::METHODS)
            .chain(swapchain::METHODS)
            .find(|(n, _)| *n == name)
            .map(|(_, a)| *a),
    }
}
pub(super) fn dwords(name: &str) -> &'static [usize] {
    if name.starts_with("SwapChain.") {
        return swapchain::dwords(name);
    }
    if name.starts_with("Output.") {
        return output::dwords(name);
    }
    if d3d11::arity(name).is_some() {
        return d3d11::dwords(name);
    }
    match name {
        "CreateDXGIFactory2" => &[0],
        "D3D12CreateDevice" => &[1],
        "Object.SetPrivateData" => &[2],
        "Factory.EnumAdapters" | "Factory.EnumAdapters1" | "Adapter.EnumOutputs" => &[1],
        "Factory.MakeWindowAssociation" => &[2],
        _ => &[],
    }
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
    backend: Option<NativeGraphics>,
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
    fn info(&self) -> Result<&AdapterInfo, String> {
        #[cfg(test)]
        if let Some(info) = &self.fixture {
            return Ok(info);
        }
        self.backend
            .as_ref()
            .map(NativeGraphics::adapter)
            .ok_or_else(|| {
                "Win64 DXGI requires an explicitly configured native graphics backend".into()
            })
    }
    fn luid(&self) -> Result<u64, String> {
        // Process-local adapter identifier derived from Vulkan device identity;
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
impl Runtime64 {
    pub fn attach_graphics(&mut self, graphics: NativeGraphics) -> Result<(), String> {
        if self.graphics.backend.is_some() || !self.graphics.objects.is_empty() {
            return Err("Win64 graphics backend already attached".into());
        }
        self.graphics.backend = Some(graphics);
        Ok(())
    }
    fn dxgi_table(&mut self, mem: &mut Memory, kind: Kind) -> Result<u64, String> {
        if let Some(&address) = self.graphics.tables.get(&kind) {
            return Ok(address);
        }
        let mut bytes = Vec::new();
        let methods: Vec<_> = match kind {
            Kind::Factory => COMMON.iter().chain(FACTORY).map(|(n, _)| *n).collect(),
            Kind::Adapter => COMMON.iter().chain(ADAPTER).map(|(n, _)| *n).collect(),
            Kind::Output => COMMON
                .iter()
                .chain(output::METHODS)
                .map(|(n, _)| *n)
                .collect(),
            Kind::SwapChain => COMMON
                .iter()
                .chain(swapchain::METHODS)
                .map(|(n, _)| *n)
                .collect(),
            _ => d3d11::methods(kind),
        };
        for name in methods {
            bytes.extend_from_slice(&self.register(DLL, name).to_le_bytes());
        }
        let address = self.next_alloc;
        mem.map(address, PAGE, READ | WRITE)?;
        mem.write(address, &bytes)?;
        mem.seal_runtime_readonly(address)?;
        self.next_alloc += PAGE;
        self.graphics.tables.insert(kind, address);
        Ok(address)
    }
    fn dxgi_object(&mut self, mem: &mut Memory, kind: Kind, parent: u64) -> Result<u64, String> {
        if self.graphics.objects.len() >= OBJECT_LIMIT {
            return Err("graphics object capacity exceeded".into());
        }
        let table = self.dxgi_table(mem, kind)?;
        if parent != 0
            && self
                .graphics
                .objects
                .get(&parent)
                .is_none_or(|o| o.refs == u32::MAX)
        {
            return Err("DXGI parent reference capacity".into());
        }
        let address = self.next_alloc;
        mem.map(address, PAGE, READ | WRITE)?;
        mem.write_int(address, 8, table)?;
        mem.seal_runtime_readonly(address)?;
        if parent != 0 {
            self.graphics.retain(parent)?;
        }
        self.next_alloc += PAGE;
        self.graphics.objects.insert(
            address,
            Object {
                kind,
                refs: 1,
                parent,
                association: if matches!(kind, Kind::Buffer | Kind::Texture) {
                    0x78000000
                } else {
                    0
                },
                private: BTreeMap::new(),
            },
        );
        Ok(address)
    }
    fn dxgi_release(&mut self, mem: &mut Memory, address: u64) -> Result<u64, String> {
        let o = self
            .graphics
            .objects
            .get_mut(&address)
            .ok_or("invalid DXGI object")?;
        if o.refs > 1 {
            o.refs -= 1;
            return Ok(o.refs as u64);
        }
        let parent = o.parent;
        mem.unmap(address, PAGE)?;
        self.graphics.objects.remove(&address);
        self.graphics.devices.remove(&address);
        self.graphics.queries.remove(&address);
        if let Some(chain) = self.graphics.swapchains.remove(&address) {
            self.dxgi_release(mem, chain.buffer)?;
            self.dxgi_release(mem, chain.device)?;
        }
        if let Some(d3d11::Resource::Buffer {
            mapped: Some((p, size, _)),
            ..
        }) = self.graphics.resources.get(&address)
        {
            mem.unmap(*p, *size)?;
        }
        if let Some(d3d11::Resource::Texture { mapped, .. }) = self.graphics.resources.get(&address)
        {
            for &(p, size, _) in mapped.values() {
                mem.unmap(p, size)?;
            }
        }
        if let Some(d3d11::Resource::View { resource, .. }) =
            self.graphics.resources.remove(&address)
        {
            self.dxgi_release(mem, resource)?;
        }
        if let Some(context) = self.graphics.contexts.remove(&address) {
            for target in context.references() {
                self.dxgi_release(mem, target)?;
            }
        }
        if parent != 0 {
            self.dxgi_release(mem, parent)?;
        }
        Ok(0)
    }
    fn dxgi_adapter(
        &mut self,
        mem: &mut Memory,
        parent: u64,
        out: u64,
        iid: &Guid,
    ) -> Result<u64, String> {
        if !supports(Kind::Adapter, iid) {
            mem.write_int(out, 8, 0)?;
            return Ok(NO_INTERFACE);
        }
        let cached = self.graphics.objects.iter().find_map(|(&address, o)| {
            (o.kind == Kind::Adapter && o.parent == parent).then_some(address)
        });
        let address = if let Some(address) = cached {
            self.graphics.retain(address)?;
            address
        } else {
            if self.graphics.objects.len() >= OBJECT_LIMIT {
                mem.write_int(out, 8, 0)?;
                return Ok(OUT_OF_MEMORY);
            }
            self.dxgi_object(mem, Kind::Adapter, parent)?
        };
        mem.write_int(out, 8, address)?;
        Ok(0)
    }
    pub(super) fn dxgi_call(
        &mut self,
        mem: &mut Memory,
        name: &str,
        a: &[u64],
    ) -> Result<u64, String> {
        if d3d11::arity(name).is_some() {
            return self.d3d11_call(mem, name, a);
        }
        if d3d12_export(name) {
            // Unity probes D3D12 before falling back to D3D11. There is no
            // D3D12 backend; report actual lack of support, including when
            // ppDevice is NULL (a capability query), rather than issue a device.
            let (iid, out) = if name == "D3D12CreateDevice" {
                (a[2], a[3])
            } else {
                (a[0], a[1])
            };
            if iid == 0 {
                return Ok(INVALID_ARG);
            }
            if out != 0 {
                mem.check(out, 8, WRITE)?;
            }
            read_guid(mem, iid)?;
            if out != 0 {
                mem.write_int(out, 8, 0)?;
            }
            return Ok(if name == "D3D12GetDebugInterface" {
                NO_INTERFACE
            } else {
                UNSUPPORTED
            });
        }
        if export(name) {
            let (flags, iid, out) = if name == "CreateDXGIFactory2" {
                (a[0], a[1], a[2])
            } else {
                (0, a[0], a[1])
            };
            if out == 0 || iid == 0 {
                return Ok(INVALID_ARG);
            }
            mem.check(out, 8, WRITE)?;
            let iid = read_guid(mem, iid)?;
            let result = if flags & !1 != 0 {
                INVALID
            } else if flags != 0 {
                0x887a002d
            }
            // debug layer not installed
            else if !supports(Kind::Factory, &iid) {
                NO_INTERFACE
            } else if self.graphics.objects.len() >= OBJECT_LIMIT {
                OUT_OF_MEMORY
            } else {
                0
            };
            if result != 0 {
                mem.write_int(out, 8, 0)?;
                return Ok(result);
            }
            self.graphics.info()?;
            let object = self.dxgi_object(mem, Kind::Factory, 0)?;
            mem.write_int(out, 8, object)?;
            return Ok(0);
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
            return self.output_call(mem, name, a);
        }
        if name.starts_with("SwapChain.") {
            return self.swapchain_call(mem, name, a);
        }
        match name {
            "Factory.CreateSwapChain" | "Factory.CreateSwapChainForHwnd" => {
                self.create_swapchain(mem, name, a)
            }
            "Adapter.EnumOutputs" => {
                if a[2] == 0 {
                    return Ok(INVALID_ARG);
                }
                mem.check(a[2], 8, WRITE)?;
                if a[1] != 0 {
                    mem.write_int(a[2], 8, 0)?;
                    return Ok(NOT_FOUND);
                }
                self.dxgi_output(mem, a[0], a[2])
            }
            "Object.QueryInterface" | "Object.GetParent" => {
                if a[1] == 0 || a[2] == 0 {
                    return Ok(INVALID_ARG);
                }
                mem.check(a[2], 8, WRITE)?;
                let iid = read_guid(mem, a[1])?;
                let target = if name == "Object.GetParent" {
                    parent
                } else {
                    a[0]
                };
                if target == 0 {
                    mem.write_int(a[2], 8, 0)?;
                    return Ok(NO_INTERFACE);
                }
                if !supports(self.graphics.objects[&target].kind, &iid) {
                    mem.write_int(a[2], 8, 0)?;
                    return Ok(NO_INTERFACE);
                }
                self.graphics.retain(target)?;
                mem.write_int(a[2], 8, target)?;
                Ok(0)
            }
            "Object.AddRef" => self.graphics.retain(a[0]),
            "Object.Release" => self.dxgi_release(mem, a[0]),
            "Object.SetPrivateData" | "Object.SetPrivateDataInterface" => {
                let key = read_guid(mem, a[1])?;
                if name.ends_with("Interface") && a[2] != 0 {
                    return Err(
                        "DXGI private IUnknown callback ownership is not implemented".into(),
                    );
                }
                let size = if name.ends_with("Interface") {
                    0
                } else {
                    a[2] as usize
                };
                if size == 0 {
                    self.graphics
                        .objects
                        .get_mut(&a[0])
                        .unwrap()
                        .private
                        .remove(&key);
                    return Ok(0);
                }
                let data = &self.graphics.objects[&a[0]].private;
                let total: usize = data.values().map(Vec::len).sum();
                if size > DATA_LIMIT
                    || total - data.get(&key).map_or(0, Vec::len) + size > DATA_LIMIT
                    || (!data.contains_key(&key) && data.len() >= 64)
                {
                    return Ok(OUT_OF_MEMORY);
                }
                if a[3] == 0 {
                    return Ok(INVALID_ARG);
                }
                let mut bytes = vec![0; size];
                mem.read(a[3], &mut bytes)?;
                self.graphics
                    .objects
                    .get_mut(&a[0])
                    .unwrap()
                    .private
                    .insert(key, bytes);
                Ok(0)
            }
            "Object.GetPrivateData" => {
                if a[2] == 0 {
                    return Ok(INVALID_ARG);
                }
                mem.check(a[2], 4, READ | WRITE)?;
                let capacity = mem.read_int(a[2], 4)? as usize;
                let key = read_guid(mem, a[1])?;
                let Some(data) = self.graphics.objects[&a[0]].private.get(&key) else {
                    mem.write_int(a[2], 4, 0)?;
                    return Ok(NOT_FOUND);
                };
                if a[3] != 0 && capacity >= data.len() {
                    mem.check(a[3], data.len(), WRITE)?;
                }
                mem.write_int(a[2], 4, data.len() as u64)?;
                if a[3] == 0 {
                    return Ok(0);
                }
                if capacity < data.len() {
                    return Ok(MORE_DATA);
                }
                mem.write(a[3], data)?;
                Ok(0)
            }
            "Factory.EnumAdapters" | "Factory.EnumAdapters1" | "Factory.EnumAdapterByLuid" => {
                let out = a[if name.ends_with("Luid") { 3 } else { 2 }];
                if out == 0 {
                    return Ok(INVALID_ARG);
                }
                mem.check(out, 8, WRITE)?;
                let iid = if name.ends_with("Luid") {
                    read_guid(mem, a[2])?
                } else {
                    ADAPTERS[1]
                };
                let found = if name.ends_with("Luid") {
                    a[1] == self.graphics.luid()?
                } else {
                    a[1] == 0
                };
                if !found {
                    mem.write_int(out, 8, 0)?;
                    return Ok(NOT_FOUND);
                }
                self.dxgi_adapter(mem, a[0], out, &iid)
            }
            "Factory.EnumWarpAdapter" => {
                if a[2] == 0 {
                    return Ok(INVALID_ARG);
                }
                mem.check(a[2], 8, WRITE)?;
                read_guid(mem, a[1])?;
                mem.write_int(a[2], 8, 0)?;
                Ok(NOT_FOUND)
            }
            "Factory.MakeWindowAssociation" => {
                if a[2] & !7 != 0 || (a[1] != 0 && !self.user.windows.contains_key(&a[1])) {
                    return Ok(INVALID);
                }
                // No native default Alt-Enter/PrintScreen handling is installed.
                self.graphics.objects.get_mut(&a[0]).unwrap().association = a[1];
                Ok(0)
            }
            "Factory.GetWindowAssociation" => {
                if a[1] == 0 {
                    return Ok(INVALID_ARG);
                }
                mem.write_int(a[1], 8, self.graphics.objects[&a[0]].association)?;
                Ok(0)
            }
            "Factory.IsCurrent" => Ok(1), // fixed attached device inventory; no hotplug enumeration
            "Factory.IsWindowedStereoEnabled" | "Factory.GetCreationFlags" => Ok(0),
            "Adapter.GetDesc" | "Adapter.GetDesc1" => {
                if a[1] == 0 {
                    return Ok(INVALID_ARG);
                }
                let mut bytes = vec![0; if name.ends_with('1') { 312 } else { 304 }];
                let info = self.graphics.info()?;
                for (i, c) in info.name.encode_utf16().take(127).enumerate() {
                    bytes[2 * i..2 * i + 2].copy_from_slice(&c.to_le_bytes());
                }
                bytes[256..260].copy_from_slice(&info.vendor.to_le_bytes());
                bytes[260..264].copy_from_slice(&info.device.to_le_bytes());
                for (at, value) in [
                    (272, info.dedicated_video),
                    (280, info.dedicated_system),
                    (288, info.shared_system),
                    (296, self.graphics.luid()?),
                ] {
                    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
                }
                mem.write(a[1], &bytes)?;
                Ok(0)
            }
            // No UMD/D3D interface is implemented. Never report a fabricated version.
            "Adapter.CheckInterfaceSupport" => {
                read_guid(mem, a[1])?;
                Ok(UNSUPPORTED)
            }
            _ => Err(format!("unimplemented Win64 DXGI method: {name}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::x64::{cpu::Cpu64, runtime::Control};
    const DATA: u64 = 0x600000000;
    const STACK: u64 = 0x7ff000000000;
    fn setup() -> (Runtime64, Memory) {
        let mut m = Memory::default();
        m.map(DATA, 2 * PAGE, READ | WRITE).unwrap();
        m.map(STACK, PAGE, READ | WRITE).unwrap();
        let mut r = Runtime64::new(&mut m, 0x140000000, STACK, PAGE, "dxgi-test").unwrap();
        r.graphics.fixture = Some(AdapterInfo {
            name: "Test GPU — 测试".into(),
            vendor: 0x1234,
            device: 0x5678,
            api_version: 0,
            dedicated_video: 0x123456789,
            dedicated_system: 0,
            shared_system: 0x23456789a,
            uuid: [0x83; 16],
        });
        (r, m)
    }
    fn call(r: &mut Runtime64, m: &mut Memory, n: &str, a: &[u64]) -> Result<u64, String> {
        m.write_int(STACK, 8, 0x140001234).unwrap();
        let mut cpu = Cpu64::new(0x7ffe00000000, STACK);
        for (i, &arg) in a.iter().enumerate() {
            if i < 4 {
                cpu.regs[[1, 2, 8, 9][i]] = arg;
            } else {
                m.write_int(STACK + 40 + (i as u64 - 4) * 8, 8, arg)
                    .unwrap();
            }
        }
        let before = (cpu.rip, cpu.regs, r.api_calls);
        let result = r.dispatch(
            if export(n) {
                "dxgi.dll"
            } else if d3d12_export(n) || n.starts_with('#') {
                "d3d12.dll"
            } else if n == "GetProcAddress" {
                "kernel32.dll"
            } else {
                DLL
            },
            n,
            &mut cpu,
            m,
        );
        match result {
            Ok(Control::Returned) => {
                assert_eq!(cpu.rip, 0x140001234);
                Ok(cpu.regs[0])
            }
            Ok(_) => panic!("unexpected DXGI control"),
            Err(e) => {
                assert_eq!((cpu.rip, cpu.regs, r.api_calls), before);
                Err(e)
            }
        }
    }
    fn factory(r: &mut Runtime64, m: &mut Memory) -> u64 {
        m.write(DATA, &FACTORIES[4]).unwrap();
        assert_eq!(
            call(r, m, "CreateDXGIFactory2", &[0, DATA, DATA + 32]).unwrap(),
            0
        );
        m.read_int(DATA + 32, 8).unwrap()
    }
    #[test]
    fn blend_state_validates_only_active_target_and_enabled_factors() {
        let (mut r, mut m) = setup();
        let device = r.dxgi_object(&mut m, Kind::Device, 0).unwrap();
        let mut desc = [0u32; 66];
        // Valid alpha blending in RT0; unused RT1..7 have zero enum fields.
        desc[2..10].copy_from_slice(&[1, 5, 6, 1, 2, 1, 1, 15]);
        for (independent, enabled, expected) in [(0, 1, 0), (0, 0, 0), (1, 1, INVALID_ARG)] {
            desc[1] = independent;
            desc[2] = enabled;
            if enabled != 0 {
                desc[3..9].copy_from_slice(&[5, 6, 1, 2, 1, 1]);
            }
            if independent != 0 {
                desc[10] = 1;
            } // active RT1 has invalid factors
            if enabled == 0 {
                desc[3..9].fill(0);
            }
            m.write(
                DATA + 256,
                &desc
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            m.write_int(DATA + 32, 8, 0xaaaaaaaa).unwrap();
            assert_eq!(
                call(
                    &mut r,
                    &mut m,
                    "D3D.Device.CreateBlendState",
                    &[device, DATA + 256, DATA + 32]
                )
                .unwrap(),
                expected
            );
            let state = m.read_int(DATA + 32, 8).unwrap();
            if expected == 0 {
                call(&mut r, &mut m, "D3D.State.GetDesc", &[state, DATA + 1024]).unwrap();
                let mut got = [0; 264];
                m.read(DATA + 1024, &mut got).unwrap();
                assert_eq!(
                    got.as_slice(),
                    &desc
                        .iter()
                        .flat_map(|v| v.to_le_bytes())
                        .collect::<Vec<_>>()
                );
                call(&mut r, &mut m, "D3D.Child.Release", &[state]).unwrap();
            } else {
                assert_eq!(state, 0);
            }
        }
        // Ignoring unused fields must not hide an invalid enabled RT0.
        desc[1] = 0;
        desc[2] = 1;
        desc[3] = 0;
        m.write(
            DATA + 256,
            &desc
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "D3D.Device.CreateBlendState",
                &[device, DATA + 256, DATA + 32]
            )
            .unwrap(),
            INVALID_ARG
        );
        assert_eq!(m.read_int(DATA + 32, 8).unwrap(), 0);
        call(&mut r, &mut m, "D3D.Device.Release", &[device]).unwrap();
        assert!(r.graphics.objects.is_empty());
    }
    #[test]
    #[ignore = "requires native Vulkan backend and original WIRE_GS_FIXTURES shaders"]
    fn native_procedural_draw_ignores_stale_layout_but_checks_active_inputs() {
        let (mut r, mut m) = setup();
        r.graphics.fixture = None;
        r.attach_graphics(
            NativeGraphics::load(
                std::path::Path::new(&std::env::var("WIRE_NATIVE_GRAPHICS").unwrap()),
                std::path::Path::new(&std::env::var("WIRE_MOLTENVK").unwrap()),
            )
            .unwrap(),
        )
        .unwrap();
        let fixtures = std::path::PathBuf::from(std::env::var_os("WIRE_GS_FIXTURES").unwrap());
        let f = factory(&mut r, &mut m);
        call(&mut r, &mut m, "Factory.EnumAdapters", &[f, 0, DATA + 32]).unwrap();
        let adapter = m.read_int(DATA + 32, 8).unwrap();
        call(
            &mut r,
            &mut m,
            "D3D11CreateDevice",
            &[adapter, 0, 0, 0, 0, 0, 7, DATA + 40, 0, DATA + 48],
        )
        .unwrap();
        let device = m.read_int(DATA + 40, 8).unwrap();
        let context = m.read_int(DATA + 48, 8).unwrap();
        let mut shaders = Vec::new();
        for (name, method) in [
            ("procedural.vs.dxbc", "D3D.Device.CreateVertexShader"),
            ("offset.vs.dxbc", "D3D.Device.CreateVertexShader"),
            ("color.ps.dxbc", "D3D.Device.CreatePixelShader"),
        ] {
            let bytes = std::fs::read(fixtures.join(name)).unwrap();
            m.write(DATA + 1024, &bytes).unwrap();
            call(
                &mut r,
                &mut m,
                method,
                &[device, DATA + 1024, bytes.len() as u64, 0, DATA + 56],
            )
            .unwrap();
            shaders.push(m.read_int(DATA + 56, 8).unwrap());
        }
        let Some(d3d11::Resource::Shader(procedural)) = r.graphics.resources.get(&shaders[0])
        else {
            panic!("VS")
        };
        assert!(procedural.input_locations.is_empty());
        let Some(d3d11::Resource::Shader(regular)) = r.graphics.resources.get(&shaders[1]) else {
            panic!("VS")
        };
        assert_eq!(regular.input_locations, [0].into_iter().collect());
        let texture_desc = [16u32, 16, 1, 1, 28, 1, 0, 0, 0x20, 0, 0];
        m.write(
            DATA + 1024,
            &texture_desc
                .iter()
                .flat_map(|x| x.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        call(
            &mut r,
            &mut m,
            "D3D.Device.CreateTexture2D",
            &[device, DATA + 1024, 0, DATA + 64],
        )
        .unwrap();
        let texture = m.read_int(DATA + 64, 8).unwrap();
        call(
            &mut r,
            &mut m,
            "D3D.Device.CreateRenderTargetView",
            &[device, texture, 0, DATA + 72],
        )
        .unwrap();
        let target = m.read_int(DATA + 72, 8).unwrap();
        m.write_int(DATA + 80, 8, target).unwrap();
        call(
            &mut r,
            &mut m,
            "D3D.Context.OMSetRenderTargets",
            &[context, 1, DATA + 80, 0],
        )
        .unwrap();
        // A retained POSITION layout and a mapped VB are harmless when the VS
        // only uses VertexId; switching to a VS using POSITION must fail.
        let layout = r.dxgi_object(&mut m, Kind::Layout, device).unwrap();
        r.graphics.resources.insert(
            layout,
            d3d11::Resource::State(
                [0u32, 2, 0, 0, 0, 0]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect(),
            ),
        );
        call(
            &mut r,
            &mut m,
            "D3D.Context.IASetInputLayout",
            &[context, layout],
        )
        .unwrap();
        let desc = [48u32, 2, 1, 0x10000, 0, 0];
        m.write(
            DATA + 1024,
            &desc
                .iter()
                .flat_map(|x| x.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        call(
            &mut r,
            &mut m,
            "D3D.Device.CreateBuffer",
            &[device, DATA + 1024, 0, DATA + 88],
        )
        .unwrap();
        let vb = m.read_int(DATA + 88, 8).unwrap();
        call(
            &mut r,
            &mut m,
            "D3D.Context.Map",
            &[context, vb, 0, 4, 0, DATA + 128],
        )
        .unwrap();
        m.write_int(DATA + 96, 8, vb).unwrap();
        m.write_int(DATA + 104, 4, 16).unwrap();
        m.write_int(DATA + 108, 4, 0).unwrap();
        call(
            &mut r,
            &mut m,
            "D3D.Context.IASetVertexBuffers",
            &[context, 0, 1, DATA + 96, DATA + 104, DATA + 108],
        )
        .unwrap();
        let vc = r
            .graphics
            .backend
            .as_ref()
            .unwrap()
            .create_buffer(&[0; 16], 1)
            .unwrap();
        let cb = r.dxgi_object(&mut m, Kind::Buffer, device).unwrap();
        r.graphics.resources.insert(
            cb,
            d3d11::Resource::Buffer {
                desc: vec![],
                native: vc,
                mapped: None,
            },
        );
        m.write_int(DATA + 112, 8, cb).unwrap();
        call(
            &mut r,
            &mut m,
            "D3D.Context.VSSetConstantBuffers",
            &[context, 0, 1, DATA + 112],
        )
        .unwrap();
        for (name, shader) in [("VS", shaders[0]), ("PS", shaders[2])] {
            call(
                &mut r,
                &mut m,
                &format!("D3D.Context.{name}SetShader"),
                &[context, shader, 0, 0],
            )
            .unwrap();
        }
        call(
            &mut r,
            &mut m,
            "D3D.Context.IASetPrimitiveTopology",
            &[context, 4],
        )
        .unwrap();
        m.write(
            DATA + 192,
            &[0f32, 0., 16., 16., 0., 1.]
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        call(
            &mut r,
            &mut m,
            "D3D.Context.RSSetViewports",
            &[context, 1, DATA + 192],
        )
        .unwrap();
        call(&mut r, &mut m, "D3D.Context.Draw", &[context, 3, 0]).unwrap();
        let Some(d3d11::Resource::Texture { native, .. }) = r.graphics.resources.get(&texture)
        else {
            panic!("texture")
        };
        let pixels = native.read(0, 16 * 16 * 4).unwrap();
        assert_eq!(pixels, [64u8, 128, 191, 255].repeat(256));
        assert_eq!(r.graphics.drawn, 1);
        call(
            &mut r,
            &mut m,
            "D3D.Context.VSSetShader",
            &[context, shaders[1], 0, 0],
        )
        .unwrap();
        assert!(call(&mut r, &mut m, "D3D.Context.Draw", &[context, 3, 0])
            .unwrap_err()
            .contains("vertex buffer slot 0"));
        assert_eq!(r.graphics.drawn, 1);
        let Some(d3d11::Resource::Texture { native, .. }) = r.graphics.resources.get(&texture)
        else {
            panic!("texture")
        };
        assert_eq!(native.read(0, 16 * 16 * 4).unwrap(), pixels);
        call(
            &mut r,
            &mut m,
            "D3D.Context.VSSetShader",
            &[context, shaders[0], 0, 0],
        )
        .unwrap();
        call(
            &mut r,
            &mut m,
            "D3D.Context.IASetVertexBuffers",
            &[context, 0, 1, 0, DATA + 104, DATA + 108],
        )
        .unwrap();
        call(&mut r, &mut m, "D3D.Context.Draw", &[context, 3, 0]).unwrap();
        assert_eq!(r.graphics.drawn, 2);
        call(
            &mut r,
            &mut m,
            "D3D.Context.VSSetShader",
            &[context, shaders[1], 0, 0],
        )
        .unwrap();
        assert!(call(&mut r, &mut m, "D3D.Context.Draw", &[context, 3, 0])
            .unwrap_err()
            .contains("vertex buffer slot 0"));
        assert_eq!(r.graphics.drawn, 2);
        let Some(d3d11::Resource::Texture { native, .. }) = r.graphics.resources.get(&texture)
        else {
            panic!("texture")
        };
        assert_eq!(native.read(0, 16 * 16 * 4).unwrap(), pixels);
        call(&mut r, &mut m, "D3D.Context.Unmap", &[context, vb, 0]).unwrap();
        call(&mut r, &mut m, "D3D.Context.ClearState", &[context]).unwrap();
        for p in shaders.into_iter().chain([layout, cb, vb, target, texture]) {
            call(&mut r, &mut m, "D3D.Child.Release", &[p]).unwrap();
        }
        call(&mut r, &mut m, "D3D.Context.Release", &[context]).unwrap();
        call(&mut r, &mut m, "D3D.Device.Release", &[device]).unwrap();
        call(&mut r, &mut m, "Object.Release", &[adapter]).unwrap();
        call(&mut r, &mut m, "Object.Release", &[f]).unwrap();
        assert!(r.graphics.objects.is_empty());
        assert!(r.graphics.resources.is_empty());
    }
    #[test]
    #[ignore = "requires explicitly provided native Vulkan backend"]
    fn native_swapchain_backbuffer_fault_resize_lifetime_and_deferred_present() {
        let (mut r, mut m) = setup();
        let backend = std::env::var("WIRE_NATIVE_GRAPHICS").unwrap();
        let moltenvk = std::env::var("WIRE_MOLTENVK").unwrap();
        r.graphics.fixture = None;
        r.attach_graphics(
            NativeGraphics::load(
                std::path::Path::new(&backend),
                std::path::Path::new(&moltenvk),
            )
            .unwrap(),
        )
        .unwrap();
        r.enable_scheduler();
        let procedure = r.register("user32.dll", "DefWindowProcW");
        m.write(
            DATA + 512,
            &"SwapProbe\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        m.write(DATA + 640, &[0; 80]).unwrap();
        m.write_int(DATA + 640, 4, 80).unwrap();
        m.write_int(DATA + 648, 8, procedure).unwrap();
        m.write_int(DATA + 664, 8, 0x140000000).unwrap();
        m.write_int(DATA + 704, 8, DATA + 512).unwrap();
        assert_ne!(
            r.class_call(
                &mut m,
                crate::win32::catalog::ClassApi::RegisterExW,
                &[DATA + 640],
            )
            .unwrap(),
            0
        );
        let returned = Cpu64::new(0x140001234, STACK + 128);
        let super::super::user::WindowResult::Callback {
            parameters,
            call: mut window_call,
            release,
            ..
        } = r
            .window_start(
                &mut m,
                crate::win32::catalog::WindowControlApi::CreateW,
                "CreateWindowExW",
                &[
                    0,
                    DATA + 512,
                    0,
                    0x10000000,
                    0,
                    0,
                    128,
                    96,
                    0,
                    0,
                    0x140000000,
                    0,
                ],
                &returned,
            )
            .unwrap()
        else {
            panic!("creation callback expected")
        };
        let mut args = parameters;
        let window = loop {
            let value = r
                .window_value_call(
                    &mut m,
                    crate::win32::catalog::WindowControlApi::DefaultW,
                    &args,
                )
                .unwrap();
            match r.window_resume(&mut m, &mut window_call, value).unwrap() {
                super::super::WindowStep::Next(_, next) => args = next,
                super::super::WindowStep::Returned(window) => break window,
            }
        };
        r.release_callback_buffers(&mut m, &release).unwrap();
        let f = factory(&mut r, &mut m);
        call(&mut r, &mut m, "Factory.EnumAdapters", &[f, 0, DATA + 32]).unwrap();
        let adapter = m.read_int(DATA + 32, 8).unwrap();
        m.write_int(DATA + 64, 4, 0xa000).unwrap();
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "D3D11CreateDevice",
                &[adapter, 0, 0, 0, DATA + 64, 1, 7, DATA + 40, 0, 0]
            )
            .unwrap(),
            0
        );
        let device = m.read_int(DATA + 40, 8).unwrap();
        let mut desc = [0u8; 72];
        for (offset, value) in [
            (0, 64u32),
            (4, 32),
            (12, 1),
            (16, 28),
            (28, 1),
            (36, 0x30),
            (40, 1),
            (56, 1),
            (64, 2),
        ] {
            desc[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        desc[48..56].copy_from_slice(&window.to_le_bytes());
        m.write(DATA + 128, &desc).unwrap();
        let objects = r.graphics.objects.len();
        let device_refs = r.graphics.objects[&device].refs;
        assert!(call(
            &mut r,
            &mut m,
            "Factory.CreateSwapChain",
            &[f, device, DATA + 128, DATA + 2 * PAGE - 4]
        )
        .is_err());
        assert_eq!(r.graphics.objects.len(), objects);
        assert_eq!(r.graphics.objects[&device].refs, device_refs);
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "Factory.CreateSwapChain",
                &[f, device, DATA + 128, DATA + 48]
            )
            .unwrap(),
            0
        );
        let chain = m.read_int(DATA + 48, 8).unwrap();
        call(&mut r, &mut m, "SwapChain.GetDesc", &[chain, DATA + 256]).unwrap();
        let mut actual = [0; 72];
        m.read(DATA + 256, &mut actual).unwrap();
        assert_eq!(actual, desc);
        m.write(
            DATA + 96,
            &guid(
                0x6f15aaf2,
                0xd208,
                0x4e89,
                [0x9a, 0xb4, 0x48, 0x95, 0x35, 0xd3, 0x4f, 0x9c],
            ),
        )
        .unwrap();
        let buffer = r.graphics.swapchains[&chain].buffer;
        assert!(call(
            &mut r,
            &mut m,
            "SwapChain.GetBuffer",
            &[chain, 0, DATA + 96, DATA + 2 * PAGE - 4]
        )
        .is_err());
        assert_eq!(r.graphics.objects[&buffer].refs, 1);
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "SwapChain.GetBuffer",
                &[chain, 0, DATA + 96, DATA + 56]
            )
            .unwrap(),
            0
        );
        assert_eq!(m.read_int(DATA + 56, 8).unwrap(), buffer);
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "SwapChain.ResizeBuffers",
                &[chain, 0, 96, 48, 0, 2]
            )
            .unwrap(),
            INVALID
        );
        call(&mut r, &mut m, "D3D.Child.Release", &[buffer]).unwrap();
        call(
            &mut r,
            &mut m,
            "D3D.Device.CreateRenderTargetView",
            &[device, buffer, 0, DATA + 56],
        )
        .unwrap();
        let view = m.read_int(DATA + 56, 8).unwrap();
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "SwapChain.ResizeBuffers",
                &[chain, 0, 96, 48, 0, 2]
            )
            .unwrap(),
            INVALID
        );
        call(&mut r, &mut m, "D3D.Child.Release", &[view]).unwrap();
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "SwapChain.ResizeBuffers",
                &[chain, 0, 96, 48, 0, 2]
            )
            .unwrap(),
            0
        );
        assert!(m.read_int(buffer, 8).is_err());
        let (_, texture, _) = r.graphics_frame(chain).unwrap();
        assert_eq!((texture.desc().width, texture.desc().height), (96, 48));
        // No submission has occurred: TEST does not increment counters, and a
        // real Present defers the CPU return/API completion to the native bridge.
        assert_eq!(
            call(&mut r, &mut m, "SwapChain.Present", &[chain, 0, 1]).unwrap(),
            0
        );
        let mut cpu = Cpu64::new(0x7ffe00000000, STACK);
        cpu.regs[1] = chain;
        m.write_int(STACK, 8, 0x140001234).unwrap();
        let before = (cpu.rip, cpu.regs, r.api_calls);
        assert!(matches!(
            r.dispatch(DLL, "SwapChain.Present", &mut cpu, &mut m)
                .unwrap(),
            Control::Present { .. }
        ));
        assert_eq!((cpu.rip, cpu.regs, r.api_calls), before);
        call(
            &mut r,
            &mut m,
            "SwapChain.GetLastPresentCount",
            &[chain, DATA + 64],
        )
        .unwrap();
        assert_eq!(m.read_int(DATA + 64, 4).unwrap(), 0);
        // Typeless storage retains the guest descriptor, requires typed views,
        // and checks output faults before any COM resource/reference mutation.
        for (format, bind, view_format) in [
            (27, 0x28, 28u32),
            (39, 0x48, 41),
            (53, 0x28, 56),
            (90, 0x28, 87),
        ] {
            let color_desc = [8u32, 4, 1, 1, format, 1, 0, 0, bind, 0, 0];
            m.write(
                DATA + 1024,
                &color_desc
                    .iter()
                    .flat_map(|x| x.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            call(
                &mut r,
                &mut m,
                "D3D.Device.CreateTexture2D",
                &[device, DATA + 1024, 0, DATA + 72],
            )
            .unwrap();
            let color = m.read_int(DATA + 72, 8).unwrap();
            call(
                &mut r,
                &mut m,
                "D3D.Resource.GetDesc",
                &[color, DATA + 1088],
            )
            .unwrap();
            assert_eq!(m.read_int(DATA + 1088 + 16, 4).unwrap(), format as u64);
            assert_eq!(
                call(
                    &mut r,
                    &mut m,
                    "D3D.Device.CreateShaderResourceView",
                    &[device, color, 0, DATA + 80]
                )
                .unwrap(),
                INVALID_ARG
            );
            assert_eq!(m.read_int(DATA + 80, 8).unwrap(), 0);
            m.write(
                DATA + 1088,
                &[view_format, 4, 0, 1, 0, 0]
                    .iter()
                    .flat_map(|x| x.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            let refs = r.graphics.objects[&color].refs;
            let objects = r.graphics.objects.len();
            assert!(call(
                &mut r,
                &mut m,
                "D3D.Device.CreateShaderResourceView",
                &[device, color, DATA + 1088, DATA + 2 * PAGE - 4]
            )
            .is_err());
            assert_eq!(r.graphics.objects[&color].refs, refs);
            assert_eq!(r.graphics.objects.len(), objects);
            call(
                &mut r,
                &mut m,
                "D3D.Device.CreateShaderResourceView",
                &[device, color, DATA + 1088, DATA + 80],
            )
            .unwrap();
            let color_view = m.read_int(DATA + 80, 8).unwrap();
            assert_eq!(r.graphics.objects[&color].refs, refs + 1);
            // Even a one-layer resource may have an array SRV. Materialize
            // all-remaining ranges in GetDesc without modifying guest input.
            let array_desc = [view_format, 5, 0, u32::MAX, 0, u32::MAX];
            m.write(
                DATA + 1152,
                &array_desc
                    .iter()
                    .flat_map(|x| x.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            assert_eq!(
                call(
                    &mut r,
                    &mut m,
                    "D3D.Device.CreateShaderResourceView",
                    &[device, color, DATA + 1152, DATA + 1200]
                )
                .unwrap(),
                0
            );
            let array_view = m.read_int(DATA + 1200, 8).unwrap();
            call(
                &mut r,
                &mut m,
                "D3D.View.GetDesc",
                &[array_view, DATA + 1216],
            )
            .unwrap();
            for (offset, value) in [(4, 5), (12, 1), (20, 1)] {
                assert_eq!(m.read_int(DATA + 1216 + offset, 4).unwrap(), value);
            }
            assert_eq!(m.read_int(DATA + 1152 + 12, 4).unwrap(), u32::MAX as u64);
            assert_eq!(m.read_int(DATA + 1152 + 20, 4).unwrap(), u32::MAX as u64);
            let Some(d3d11::Resource::View {
                native: d3d11::NativeView::Image(native),
                ..
            }) = r.graphics.resources.get(&array_view)
            else {
                panic!("array SRV expected")
            };
            assert_eq!((native.desc().kind, native.desc().layers), (6, 1));
            call(&mut r, &mut m, "D3D.Child.Release", &[array_view]).unwrap();
            let objects = r.graphics.objects.len();
            assert_eq!(
                call(
                    &mut r,
                    &mut m,
                    "D3D.Device.CreateShaderResourceView",
                    &[device, color, DATA + 1152, 0]
                )
                .unwrap(),
                1
            );
            assert_eq!(r.graphics.objects.len(), objects);
            assert_eq!(r.graphics.objects[&color].refs, refs + 1);
            // Invalid ranges fail with HRESULT, a cleared output and no refs;
            // a checked output fault preserves its prior contents and objects.
            assert!(call(
                &mut r,
                &mut m,
                "D3D.Device.CreateShaderResourceView",
                &[device, color, DATA + 1152, DATA + 2 * PAGE - 4]
            )
            .is_err());
            for desc in [
                [view_format, 5, 1, u32::MAX, 0, u32::MAX],
                [view_format, 5, 0, u32::MAX, 1, u32::MAX],
                [view_format, 5, 0, u32::MAX, 0, 0],
                [view_format, 5, 0, u32::MAX, 0, 2],
                [view_format, 5, 0, u32::MAX, u32::MAX, u32::MAX],
            ] {
                m.write(
                    DATA + 1152,
                    &desc
                        .iter()
                        .flat_map(|x| x.to_le_bytes())
                        .collect::<Vec<_>>(),
                )
                .unwrap();
                m.write_int(DATA + 1200, 8, u64::MAX).unwrap();
                assert_eq!(
                    call(
                        &mut r,
                        &mut m,
                        "D3D.Device.CreateShaderResourceView",
                        &[device, color, DATA + 1152, DATA + 1200]
                    )
                    .unwrap(),
                    INVALID_ARG
                );
                assert_eq!(m.read_int(DATA + 1200, 8).unwrap(), 0);
                assert_eq!(r.graphics.objects.len(), objects);
                assert_eq!(r.graphics.objects[&color].refs, refs + 1);
            }
            if format == 39 {
                m.write(
                    DATA + 1088,
                    &[40u32, 3, 0, 0, 0, 0]
                        .iter()
                        .flat_map(|x| x.to_le_bytes())
                        .collect::<Vec<_>>(),
                )
                .unwrap();
                call(
                    &mut r,
                    &mut m,
                    "D3D.Device.CreateDepthStencilView",
                    &[device, color, DATA + 1088, DATA + 88],
                )
                .unwrap();
                let dsv = m.read_int(DATA + 88, 8).unwrap();
                call(
                    &mut r,
                    &mut m,
                    "D3D.Device.GetImmediateContext1",
                    &[device, DATA + 96],
                )
                .unwrap();
                let context = m.read_int(DATA + 96, 8).unwrap();
                let mut clear_cpu = Cpu64::new(0x7ffe00000000, STACK);
                clear_cpu.regs[1] = context;
                clear_cpu.regs[2] = dsv;
                clear_cpu.regs[8] = 3; // Unity clears depth + absent stencil.
                clear_cpu.regs[9] = 0xdeadbeef;
                clear_cpu.xmm[3][..4].copy_from_slice(&0.375f32.to_le_bytes());
                m.write_int(STACK + 40, 8, 7).unwrap();
                assert_eq!(
                    r.dispatch(
                        DLL,
                        "D3D.Context.ClearDepthStencilView",
                        &mut clear_cpu,
                        &mut m
                    )
                    .unwrap(),
                    Control::Returned
                );
                let Some(d3d11::Resource::Texture { native, .. }) =
                    r.graphics.resources.get(&color)
                else {
                    panic!("R32 texture expected")
                };
                assert_eq!(
                    native.read(0, 8 * 4 * 4).unwrap(),
                    0.375f32.to_le_bytes().repeat(32)
                );
                call(&mut r, &mut m, "D3D.Child.Release", &[dsv]).unwrap();
                call(&mut r, &mut m, "D3D.Context.Release", &[context]).unwrap();
                assert_eq!(r.graphics.objects[&color].refs, refs + 1);
            }
            m.write_int(DATA + 1088, 4, format as u64).unwrap();
            m.write_int(DATA + 1092, 4, 4).unwrap();
            assert_eq!(
                call(
                    &mut r,
                    &mut m,
                    "D3D.Device.CreateRenderTargetView",
                    &[device, color, DATA + 1088, DATA + 88]
                )
                .unwrap(),
                INVALID_ARG
            );
            assert_eq!(m.read_int(DATA + 88, 8).unwrap(), 0);
            assert_eq!(r.graphics.objects[&color].refs, refs + 1);
            call(&mut r, &mut m, "D3D.Child.Release", &[color_view]).unwrap();
            call(&mut r, &mut m, "D3D.Child.Release", &[color]).unwrap();
        }
        // Nonzero first mip/slice, including cube units and RTV/DSV layouts,
        // must become finite ranges at both the COM and native boundaries.
        for (method, td, input, expected, native_range) in [
            (
                "D3D.Device.CreateShaderResourceView",
                [8u32, 8, 4, 4, 28, 1, 0, 0, 8, 0, 0],
                [28u32, 5, 1, u32::MAX, 2, u32::MAX],
                [28u32, 5, 1, 3, 2, 2],
                (6, 1, 3, 2, 2),
            ),
            (
                "D3D.Device.CreateRenderTargetView",
                [8, 8, 4, 4, 28, 1, 0, 0, 0x20, 0, 0],
                [28, 5, 1, 2, u32::MAX, 0],
                [28, 5, 1, 2, 2, 0],
                (1, 1, 1, 2, 2),
            ),
            (
                "D3D.Device.CreateDepthStencilView",
                [8, 8, 4, 4, 39, 1, 0, 0, 0x40, 0, 0],
                [40, 4, 0, 1, 2, u32::MAX],
                [40, 4, 0, 1, 2, 2],
                (2, 1, 1, 2, 2),
            ),
            (
                "D3D.Device.CreateShaderResourceView",
                [8, 8, 4, 18, 28, 1, 0, 0, 8, 0, 4],
                [28, 10, 1, u32::MAX, 6, u32::MAX],
                [28, 10, 1, 3, 6, 2],
                (4, 1, 3, 6, 12),
            ),
        ] {
            m.write(
                DATA + 1024,
                &td.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<_>>(),
            )
            .unwrap();
            assert_eq!(
                call(
                    &mut r,
                    &mut m,
                    "D3D.Device.CreateTexture2D",
                    &[device, DATA + 1024, 0, DATA + 72]
                )
                .unwrap(),
                0
            );
            let texture = m.read_int(DATA + 72, 8).unwrap();
            let input: Vec<_> = input.iter().flat_map(|x| x.to_le_bytes()).collect();
            m.write(DATA + 1088, &input).unwrap();
            assert_eq!(
                call(
                    &mut r,
                    &mut m,
                    method,
                    &[device, texture, DATA + 1088, DATA + 80]
                )
                .unwrap(),
                0
            );
            let view = m.read_int(DATA + 80, 8).unwrap();
            call(&mut r, &mut m, "D3D.View.GetDesc", &[view, DATA + 1152]).unwrap();
            let size = if method.ends_with("RenderTargetView") {
                20
            } else {
                24
            };
            let mut actual = vec![0; size];
            m.read(DATA + 1152, &mut actual).unwrap();
            let expected: Vec<_> = expected.iter().flat_map(|x| x.to_le_bytes()).collect();
            assert_eq!(actual, expected[..size]);
            let mut untouched = vec![0; 24];
            m.read(DATA + 1088, &mut untouched).unwrap();
            assert_eq!(untouched, input);
            let Some(d3d11::Resource::View {
                native: d3d11::NativeView::Image(native),
                ..
            }) = r.graphics.resources.get(&view)
            else {
                panic!("native image view expected")
            };
            let d = native.desc();
            assert_eq!((d.kind, d.mip, d.mips, d.layer, d.layers), native_range);
            assert_eq!(r.graphics.objects[&texture].refs, 2);
            call(&mut r, &mut m, "D3D.Child.Release", &[view]).unwrap();
            assert_eq!(r.graphics.objects[&texture].refs, 1);
            call(&mut r, &mut m, "D3D.Child.Release", &[texture]).unwrap();
        }
        // Exercise the mixed integer/FLOAT Win64 clear ABI against real GPU data.
        // XMM3 holds depth while R9 deliberately contains unrelated integer bits.
        let td = [8u32, 4, 1, 1, 19, 1, 0, 0, 0x40, 0, 0];
        m.write(
            DATA + 1024,
            &td.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<_>>(),
        )
        .unwrap();
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "D3D.Device.CreateTexture2D",
                &[device, DATA + 1024, 0, DATA + 72]
            )
            .unwrap(),
            0
        );
        let depth = m.read_int(DATA + 72, 8).unwrap();
        m.write(
            DATA + 1088,
            &[20u32, 3, 0, 0, 0, 0]
                .iter()
                .flat_map(|x| x.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "D3D.Device.CreateDepthStencilView",
                &[device, depth, DATA + 1088, DATA + 80]
            )
            .unwrap(),
            0
        );
        let dsv = m.read_int(DATA + 80, 8).unwrap();
        call(
            &mut r,
            &mut m,
            "D3D.Device.GetImmediateContext",
            &[device, DATA + 88],
        )
        .unwrap();
        let context = m.read_int(DATA + 88, 8).unwrap();
        // Signature-only DXBC is valid for layouts; no executable shader chunk
        // is needed. HRESULT errors clear ppLayout, checked faults do not.
        let mut signature = vec![0u8; 85];
        signature[..4].copy_from_slice(b"DXBC");
        for (at, value) in [
            (20, 1u32),
            (24, 85),
            (28, 1),
            (32, 36),
            (40, 41),
            (44, 1),
            (48, 8),
            (52, 32),
            (64, 3),
            (68, 0),
        ] {
            signature[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        signature[36..40].copy_from_slice(b"ISGN");
        signature[72] = 15;
        signature[76..].copy_from_slice(b"POSITION\0");
        m.write(DATA + 1152, &signature).unwrap();
        m.write(DATA + 1280, b"position\0").unwrap();
        m.write(DATA + 1312, &[0; 32]).unwrap();
        m.write_int(DATA + 1312, 8, DATA + 1280).unwrap();
        m.write_int(DATA + 1324, 4, 2).unwrap();
        m.write_int(DATA + 1332, 4, u32::MAX as u64).unwrap();
        let args = [device, DATA + 1312, 1, DATA + 1152, 85, DATA + 96];
        assert_eq!(
            call(&mut r, &mut m, "D3D.Device.CreateInputLayout", &args).unwrap(),
            0
        );
        let layout = m.read_int(DATA + 96, 8).unwrap();
        let Some(d3d11::Resource::State(attributes)) = r.graphics.resources.get(&layout) else {
            panic!("input layout expected")
        };
        assert_eq!(
            *attributes,
            [0u32, 2, 0, 0, 0, 0]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>()
        );
        call(&mut r, &mut m, "D3D.Child.Release", &[layout]).unwrap();
        m.write_int(DATA + 96, 8, 0xabababababababab).unwrap();
        let mut fault = args;
        fault[3] = DATA + 2 * PAGE - 4;
        let objects = r.graphics.objects.len();
        let references = r.graphics.objects[&device].refs;
        assert!(call(&mut r, &mut m, "D3D.Device.CreateInputLayout", &fault).is_err());
        assert_eq!(m.read_int(DATA + 96, 8).unwrap(), 0xabababababababab);
        assert_eq!(r.graphics.objects.len(), objects);
        assert_eq!(r.graphics.objects[&device].refs, references);
        m.write_int(DATA + 1312, 8, 0).unwrap();
        assert_eq!(
            call(&mut r, &mut m, "D3D.Device.CreateInputLayout", &args).unwrap(),
            INVALID_ARG
        );
        assert_eq!(m.read_int(DATA + 96, 8).unwrap(), 0);
        cpu = Cpu64::new(0x7ffe00000000, STACK);
        cpu.regs[1] = context;
        cpu.regs[2] = dsv;
        cpu.regs[8] = 3;
        cpu.regs[9] = 0xdeadbeef;
        cpu.xmm[3][..4].copy_from_slice(&0.625f32.to_le_bytes());
        m.write_int(STACK + 40, 8, 0x123456789abcde5b).unwrap();
        assert_eq!(
            r.dispatch(DLL, "D3D.Context.ClearDepthStencilView", &mut cpu, &mut m)
                .unwrap(),
            Control::Returned
        );
        let Some(d3d11::Resource::Texture { native, .. }) = r.graphics.resources.get(&depth) else {
            panic!("depth texture expected")
        };
        let pixel = [0x00, 0x00, 0x20, 0x3f, 0x5b, 0, 0, 0];
        assert_eq!(native.read(0, 8 * 4 * 8).unwrap(), pixel.repeat(32));
        call(&mut r, &mut m, "D3D.Child.Release", &[dsv]).unwrap();
        call(&mut r, &mut m, "D3D.Child.Release", &[depth]).unwrap();
        call(&mut r, &mut m, "D3D.Context.Release", &[context]).unwrap();
        call(&mut r, &mut m, "Object.Release", &[chain]).unwrap();
        call(&mut r, &mut m, "D3D.Device.Release", &[device]).unwrap();
        call(&mut r, &mut m, "Object.Release", &[adapter]).unwrap();
        call(&mut r, &mut m, "Object.Release", &[f]).unwrap();
        assert!(r.graphics.objects.is_empty());
        assert!(r.graphics.resources.is_empty());
        // A native texture clone still owns its GPU after guest COM destruction.
        assert_eq!(texture.read(0, 96 * 48 * 4).unwrap().len(), 96 * 48 * 4);
    }
    #[test]
    fn virtual_output_identity_win64_desc_and_parent_lifetime_match_display_profile() {
        let (mut r, mut m) = setup();
        let f = factory(&mut r, &mut m);
        call(&mut r, &mut m, "Factory.EnumAdapters1", &[f, 0, DATA + 32]).unwrap();
        let adapter = m.read_int(DATA + 32, 8).unwrap();
        let refs = r.graphics.objects[&adapter].refs;
        m.write_int(DATA + 2 * PAGE - 4, 4, 0xaaaaaaaa).unwrap();
        assert!(call(
            &mut r,
            &mut m,
            "Adapter.EnumOutputs",
            &[adapter, 0, DATA + 2 * PAGE - 4]
        )
        .is_err());
        assert_eq!(r.graphics.objects[&adapter].refs, refs);
        assert!(!r.graphics.objects.values().any(|o| o.kind == Kind::Output));
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "Adapter.EnumOutputs",
                &[adapter, 0, DATA + 32]
            )
            .unwrap(),
            0
        );
        let output = m.read_int(DATA + 32, 8).unwrap();
        assert!(output > u32::MAX as u64);
        assert_eq!(r.graphics.objects[&adapter].refs, refs + 1);
        let table = m.read_int(output, 8).unwrap();
        let method = m.read_int(table + 19 * 8, 8).unwrap();
        assert_eq!(r.gateways[&method], "wire.dxgi!Output.GetDisplayModeList1");
        assert!(m.write_int(table, 8, 0).is_err());
        call(
            &mut r,
            &mut m,
            "Adapter.EnumOutputs",
            &[adapter, 0, DATA + 32],
        )
        .unwrap();
        assert_eq!(m.read_int(DATA + 32, 8).unwrap(), output);
        assert_eq!(r.graphics.objects[&output].refs, 2);
        call(&mut r, &mut m, "Output.GetDesc", &[output, DATA + 128]).unwrap();
        assert_eq!(m.read_int(DATA + 128 + 72, 4).unwrap(), 1024);
        assert_eq!(m.read_int(DATA + 128 + 76, 4).unwrap(), 768);
        assert_eq!(m.read_int(DATA + 128 + 80, 4).unwrap(), 1);
        assert_eq!(m.read_int(DATA + 128 + 84, 4).unwrap(), 1);
        assert_eq!(
            m.read_int(DATA + 128 + 88, 8).unwrap(),
            crate::x64::runtime::display::MONITOR
        );
        let mut name = [0u8; 64];
        m.read(DATA + 128, &mut name).unwrap();
        let units: Vec<_> = name
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes(*b))
            .take_while(|&u| u != 0)
            .collect();
        assert_eq!(String::from_utf16(&units).unwrap(), "\\\\.\\DISPLAY1");
        m.write(DATA + 64, &output::IIDS[1]).unwrap();
        call(
            &mut r,
            &mut m,
            "Object.QueryInterface",
            &[output, DATA + 64, DATA + 32],
        )
        .unwrap();
        assert_eq!(m.read_int(DATA + 32, 8).unwrap(), output);
        call(&mut r, &mut m, "Object.Release", &[output]).unwrap();
        call(&mut r, &mut m, "Object.Release", &[output]).unwrap();
        call(&mut r, &mut m, "Object.Release", &[output]).unwrap();
        assert!(!r.graphics.objects.contains_key(&output));
        assert!(m.read_int(output, 8).is_err());
        assert_eq!(r.graphics.objects[&adapter].refs, refs);
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "Adapter.EnumOutputs",
                &[adapter, 1, DATA + 32]
            )
            .unwrap(),
            NOT_FOUND
        );
        assert_eq!(m.read_int(DATA + 32, 8).unwrap(), 0);
    }
    #[test]
    fn virtual_output_mode_queries_preflight_counts_arrays_and_matching_before_writes() {
        let (mut r, mut m) = setup();
        let f = factory(&mut r, &mut m);
        call(&mut r, &mut m, "Factory.EnumAdapters1", &[f, 0, DATA + 32]).unwrap();
        let adapter = m.read_int(DATA + 32, 8).unwrap();
        call(
            &mut r,
            &mut m,
            "Adapter.EnumOutputs",
            &[adapter, 0, DATA + 32],
        )
        .unwrap();
        let output = m.read_int(DATA + 32, 8).unwrap();
        for (name, size) in [
            ("Output.GetDisplayModeList", 28),
            ("Output.GetDisplayModeList1", 32),
        ] {
            m.write_int(DATA + 64, 4, 0).unwrap();
            assert_eq!(
                call(&mut r, &mut m, name, &[output, 28, 0, DATA + 64, 0]).unwrap(),
                0
            );
            assert_eq!(m.read_int(DATA + 64, 4).unwrap(), 1);
            m.write(DATA + 128, &[0xaa; 32]).unwrap();
            m.write_int(DATA + 64, 4, 0).unwrap();
            assert_eq!(
                call(
                    &mut r,
                    &mut m,
                    name,
                    &[output, 28, 0, DATA + 64, DATA + 128]
                )
                .unwrap(),
                MORE_DATA
            );
            assert_eq!(m.read_int(DATA + 128, 8).unwrap(), 0xaaaaaaaaaaaaaaaa);
            m.write_int(DATA + 64, 4, 1).unwrap();
            assert!(call(
                &mut r,
                &mut m,
                name,
                &[output, 28, 0, DATA + 64, DATA + 2 * PAGE - 4]
            )
            .is_err());
            assert_eq!(m.read_int(DATA + 64, 4).unwrap(), 1);
            call(
                &mut r,
                &mut m,
                name,
                &[output, 87, 0, DATA + 64, DATA + 128],
            )
            .unwrap();
            let mut bytes = vec![0; size];
            m.read(DATA + 128, &mut bytes).unwrap();
            let actual: Vec<_> = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| u32::from_le_bytes(*b))
                .collect();
            let mut expected = vec![1024, 768, 60, 1, 87, 1, 0];
            if size == 32 {
                expected.push(0);
            }
            assert_eq!(actual, expected);
        }
        m.write(DATA + 128, &[0; 32]).unwrap();
        m.write_int(DATA + 128 + 16, 4, 28).unwrap();
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "Output.FindClosestMatchingMode1",
                &[output, DATA + 128, DATA + 128, 0]
            )
            .unwrap(),
            0
        );
        assert_eq!(m.read_int(DATA + 128, 4).unwrap(), 1024);
        m.write_int(DATA + 128 + 28, 4, 1).unwrap();
        m.write(DATA + 256, &[0x55; 32]).unwrap();
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "Output.FindClosestMatchingMode1",
                &[output, DATA + 128, DATA + 256, 0]
            )
            .unwrap(),
            NOT_FOUND
        );
        assert_eq!(m.read_int(DATA + 256, 8).unwrap(), 0x5555555555555555);
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "Output.DuplicateOutput",
                &[output, 0, DATA + 32]
            )
            .unwrap(),
            UNSUPPORTED
        );
        assert_eq!(m.read_int(DATA + 32, 8).unwrap(), 0);
        assert_eq!(
            call(&mut r, &mut m, "Output.WaitForVBlank", &[output]).unwrap(),
            UNSUPPORTED
        );
    }
    #[test]
    fn d3d11_1_interfaces_keep_base_slots_identity_references_and_real_optional_failures() {
        let (mut r, mut m) = setup();
        let f = factory(&mut r, &mut m);
        call(&mut r, &mut m, "Factory.EnumAdapters1", &[f, 0, DATA + 32]).unwrap();
        let adapter = m.read_int(DATA + 32, 8).unwrap();
        m.write_int(DATA + 64, 4, 0xa000).unwrap();
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "D3D11CreateDevice",
                &[
                    adapter,
                    0,
                    0,
                    0x20,
                    DATA + 64,
                    1,
                    7,
                    DATA + 80,
                    DATA + 88,
                    DATA + 96
                ]
            )
            .unwrap(),
            0
        );
        let device = m.read_int(DATA + 80, 8).unwrap();
        let context = m.read_int(DATA + 96, 8).unwrap();
        let device1 = guid(
            0xa04bfb29,
            0x08ef,
            0x43d6,
            [0xa4, 0x9c, 0xa9, 0xbd, 0xbd, 0xcb, 0xe6, 0x86],
        );
        let context1 = guid(
            0xbb2c6faa,
            0xb5fb,
            0x4082,
            [0x8e, 0x6b, 0x38, 0x8b, 0x8c, 0xfa, 0x90, 0xe1],
        );
        for (object, iid, prefix) in [(device, device1, "Device"), (context, context1, "Context")] {
            let refs = r.graphics.objects[&object].refs;
            m.write(DATA + 128, &iid).unwrap();
            call(
                &mut r,
                &mut m,
                &format!("D3D.{prefix}.QueryInterface"),
                &[object, DATA + 128, DATA + 144],
            )
            .unwrap();
            assert_eq!(m.read_int(DATA + 144, 8).unwrap(), object);
            assert_eq!(r.graphics.objects[&object].refs, refs + 1);
            call(&mut r, &mut m, &format!("D3D.{prefix}.Release"), &[object]).unwrap();
            assert_eq!(r.graphics.objects[&object].refs, refs);
        }
        let vtable = m.read_int(device, 8).unwrap();
        for (slot, name) in [
            (33, "D3D.Device.CheckFeatureSupport"),
            (43, "D3D.Device.GetImmediateContext1"),
            (49, "D3D.Device.OpenSharedResourceByName"),
        ] {
            assert_eq!(
                r.gateways[&m.read_int(vtable + slot * 8, 8).unwrap()],
                format!("{DLL}!{name}")
            );
        }
        assert!(m.write_int(vtable + 43 * 8, 8, 0).is_err());
        let vtable = m.read_int(context, 8).unwrap();
        for (offset, name) in [
            (0, "D3D.Context.CopySubresourceRegion1"),
            (18, "D3D.Context.DiscardView1"),
        ] {
            assert_eq!(
                r.gateways[&m
                    .read_int(vtable + (abi::CONTEXT.len() as u64 + offset) * 8, 8)
                    .unwrap()],
                format!("{DLL}!{name}")
            );
        }
        call(
            &mut r,
            &mut m,
            "D3D.Device.GetImmediateContext1",
            &[device, DATA + 144],
        )
        .unwrap();
        assert_eq!(m.read_int(DATA + 144, 8).unwrap(), context);
        call(&mut r, &mut m, "D3D.Context.Release", &[context]).unwrap();
        let refs = r.graphics.objects[&device].refs;
        m.write(
            DATA + 128,
            &guid(
                0x54ec77fa,
                0x1377,
                0x44e6,
                [0x8c, 0x32, 0x88, 0xfd, 0x5f, 0x44, 0xc8, 0x4c],
            ),
        )
        .unwrap();
        call(
            &mut r,
            &mut m,
            "D3D.Device.QueryInterface",
            &[device, DATA + 128, DATA + 144],
        )
        .unwrap();
        let dxgi = m.read_int(DATA + 144, 8).unwrap();
        m.write(DATA + 128, &device1).unwrap();
        call(
            &mut r,
            &mut m,
            "Object.QueryInterface",
            &[dxgi, DATA + 128, DATA + 144],
        )
        .unwrap();
        assert_eq!(m.read_int(DATA + 144, 8).unwrap(), device);
        call(&mut r, &mut m, "D3D.Device.Release", &[device]).unwrap();
        call(&mut r, &mut m, "Object.Release", &[dxgi]).unwrap();
        assert_eq!(r.graphics.objects[&device].refs, refs);
        m.write(DATA + 128, &device1).unwrap();
        assert!(call(
            &mut r,
            &mut m,
            "D3D.Device.QueryInterface",
            &[device, DATA + 128, DATA + 2 * PAGE - 4]
        )
        .is_err());
        assert_eq!(r.graphics.objects[&device].refs, refs);
        let objects = r.graphics.objects.len();
        for (name, args) in [
            (
                "D3D.Device.CreateDeferredContext1",
                vec![device, 0, DATA + 144],
            ),
            (
                "D3D.Device.CreateBlendState1",
                vec![device, DATA + 256, DATA + 144],
            ),
            (
                "D3D.Device.CreateRasterizerState1",
                vec![device, DATA + 256, DATA + 144],
            ),
            (
                "D3D.Device.OpenSharedResource1",
                vec![device, 1, DATA + 128, DATA + 144],
            ),
            (
                "D3D.Device.OpenSharedResourceByName",
                vec![device, DATA + 256, 1, DATA + 128, DATA + 144],
            ),
        ] {
            m.write_int(DATA + 144, 8, u64::MAX).unwrap();
            assert_eq!(call(&mut r, &mut m, name, &args).unwrap(), UNSUPPORTED);
            assert_eq!(m.read_int(DATA + 144, 8).unwrap(), 0);
            assert_eq!(r.graphics.objects.len(), objects);
        }
        m.write(DATA + 256, &[0xaa; 56]).unwrap();
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "D3D.Device.CheckFeatureSupport",
                &[device, 5, DATA + 256, 56]
            )
            .unwrap(),
            0
        );
        let mut data = [0; 56];
        m.read(DATA + 256, &mut data).unwrap();
        assert_eq!(data, [0; 56]);
        m.write_int(DATA + 144, 8, 0x1234).unwrap();
        assert!(call(
            &mut r,
            &mut m,
            "D3D.Device.CreateDeviceContextState",
            &[
                device,
                0,
                0,
                0,
                7,
                DATA + 128,
                DATA + 2 * PAGE - 2,
                DATA + 144
            ]
        )
        .is_err());
        assert_eq!(m.read_int(DATA + 144, 8).unwrap(), 0x1234);
        // A whole-buffer Context1 setter reaches the real base binding state.
        call(
            &mut r,
            &mut m,
            "D3D.Context.VSSetConstantBuffers1",
            &[context, 0, 1, 0, 0, 0],
        )
        .unwrap();
        let refs = r.graphics.objects[&context].refs;
        m.write_int(DATA + 256, 4, 1).unwrap();
        assert!(call(
            &mut r,
            &mut m,
            "D3D.Context.VSSetConstantBuffers1",
            &[context, 0, 1, 0, DATA + 256, 0]
        )
        .is_err());
        assert_eq!(r.graphics.objects[&context].refs, refs);
    }
    #[test]
    fn experimental_d3d11_device_context_query_identity_and_faults_keep_real_references() {
        let (mut r, mut m) = setup();
        let f = factory(&mut r, &mut m);
        call(&mut r, &mut m, "Factory.EnumAdapters1", &[f, 0, DATA + 32]).unwrap();
        let adapter = m.read_int(DATA + 32, 8).unwrap();
        m.write_int(DATA + 64, 4, 0xb100).unwrap();
        m.write_int(DATA + 68, 4, 0xa000).unwrap();
        let objects = r.graphics.objects.len();
        assert!(call(
            &mut r,
            &mut m,
            "D3D11CreateDevice",
            &[
                adapter,
                0,
                0,
                0x20,
                DATA + 64,
                2,
                7,
                DATA + 2 * PAGE - 4,
                DATA + 88,
                DATA + 96
            ]
        )
        .is_err());
        assert_eq!(r.graphics.objects.len(), objects);
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "D3D11CreateDevice",
                &[
                    adapter,
                    0,
                    0,
                    0x20,
                    DATA + 64,
                    2,
                    7,
                    DATA + 80,
                    DATA + 88,
                    DATA + 96
                ]
            )
            .unwrap(),
            0
        );
        let device = m.read_int(DATA + 80, 8).unwrap();
        let context = m.read_int(DATA + 96, 8).unwrap();
        assert_eq!(m.read_int(DATA + 88, 4).unwrap(), 0xa000);
        assert!(device > u32::MAX as u64 && context > u32::MAX as u64);
        assert!(m.write_int(device, 8, 0).is_err());
        assert!(m.write_int(m.read_int(context, 8).unwrap(), 8, 0).is_err());
        call(
            &mut r,
            &mut m,
            "D3D.Device.GetImmediateContext",
            &[device, DATA + 104],
        )
        .unwrap();
        assert_eq!(m.read_int(DATA + 104, 8).unwrap(), context);
        assert_eq!(
            call(&mut r, &mut m, "D3D.Context.Release", &[context]).unwrap(),
            1
        );
        m.write(DATA + 128, &OBJECT).unwrap();
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "D3D.Device.QueryInterface",
                &[device, DATA + 128, DATA + 144]
            )
            .unwrap(),
            NO_INTERFACE
        );
        m.write(
            DATA + 128,
            &guid(
                0x54ec77fa,
                0x1377,
                0x44e6,
                [0x8c, 0x32, 0x88, 0xfd, 0x5f, 0x44, 0xc8, 0x4c],
            ),
        )
        .unwrap();
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "D3D.Device.QueryInterface",
                &[device, DATA + 128, DATA + 144]
            )
            .unwrap(),
            0
        );
        let dxgi_device = m.read_int(DATA + 144, 8).unwrap();
        m.write(DATA + 128, &UNKNOWN).unwrap();
        call(
            &mut r,
            &mut m,
            "Object.QueryInterface",
            &[dxgi_device, DATA + 128, DATA + 152],
        )
        .unwrap();
        assert_eq!(m.read_int(DATA + 152, 8).unwrap(), device);
        call(&mut r, &mut m, "D3D.Device.Release", &[device]).unwrap();
        m.write(DATA + 160, &[0; 8]).unwrap();
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "D3D.Device.CreateQuery",
                &[device, DATA + 160, DATA + 168]
            )
            .unwrap(),
            0
        );
        let query = m.read_int(DATA + 168, 8).unwrap();
        m.write_int(DATA + 176, 4, 0x12345678).unwrap();
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "D3D.Context.GetData",
                &[context, query, DATA + 176, 4, 0]
            )
            .unwrap(),
            1
        );
        assert_eq!(m.read_int(DATA + 176, 4).unwrap(), 0x12345678);
        let refs = r.graphics.objects[&device].refs;
        assert!(call(
            &mut r,
            &mut m,
            "D3D.Query.GetDevice",
            &[query, DATA + 2 * PAGE - 4]
        )
        .is_err());
        assert_eq!(r.graphics.objects[&device].refs, refs);
        assert!(call(
            &mut r,
            &mut m,
            "D3D.Resource.GetDesc",
            &[device, DATA + 160]
        )
        .is_err());
        assert_eq!(
            call(&mut r, &mut m, "D3D.Query.Release", &[query]).unwrap(),
            0
        );
        assert_eq!(
            call(&mut r, &mut m, "Object.Release", &[dxgi_device]).unwrap(),
            0
        );
        assert_eq!(
            call(&mut r, &mut m, "D3D.Context.Release", &[context]).unwrap(),
            0
        );
        assert_eq!(
            call(&mut r, &mut m, "D3D.Device.Release", &[device]).unwrap(),
            0
        );
        assert_eq!(
            call(&mut r, &mut m, "Object.Release", &[adapter]).unwrap(),
            0
        );
        assert_eq!(call(&mut r, &mut m, "Object.Release", &[f]).unwrap(), 0);
        assert!(r.graphics.objects.is_empty());
    }
    #[test]
    fn win64_com_identity_adapter_layout_and_parent_lifetime() {
        let (mut r, mut m) = setup();
        let f = factory(&mut r, &mut m);
        assert!(f > u32::MAX as u64);
        let table = m.read_int(f, 8).unwrap();
        assert_eq!(
            r.gateways[&m.read_int(table + 26 * 8, 8).unwrap()],
            "wire.dxgi!Factory.EnumAdapterByLuid"
        );
        assert_eq!(
            r.gateways[&m.read_int(table + 27 * 8, 8).unwrap()],
            "wire.dxgi!Factory.EnumWarpAdapter"
        );
        assert!(m.write_int(f, 8, 0).is_err());
        assert_eq!(m.virtual_protect(table, 8, 4), Err(5));
        assert_eq!(m.virtual_protect(f, 8, 0x40), Err(5));
        assert_eq!(m.virtual_free(f, 0, 0x8000), Err(487));
        for iid in [UNKNOWN, OBJECT].into_iter().chain(FACTORIES) {
            m.write(DATA, &iid).unwrap();
            assert_eq!(
                call(
                    &mut r,
                    &mut m,
                    "Object.QueryInterface",
                    &[f, DATA, DATA + 40]
                )
                .unwrap(),
                0
            );
            assert_eq!(m.read_int(DATA + 40, 8).unwrap(), f);
            assert_eq!(call(&mut r, &mut m, "Object.Release", &[f]).unwrap(), 1);
        }
        // The DWORD adapter index ignores unspecified upper register bits.
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "Factory.EnumAdapters1",
                &[f, 0xf000000000000000, DATA + 40]
            )
            .unwrap(),
            0
        );
        let adapter = m.read_int(DATA + 40, 8).unwrap();
        assert_eq!(
            call(&mut r, &mut m, "Adapter.GetDesc1", &[adapter, DATA + 128]).unwrap(),
            0
        );
        assert_eq!(m.read_int(DATA + 128 + 256, 4).unwrap(), 0x1234);
        assert_eq!(m.read_int(DATA + 128 + 272, 8).unwrap(), 0x123456789);
        assert_eq!(m.read_int(DATA + 128 + 288, 8).unwrap(), 0x23456789a);
        assert_eq!(m.read_int(DATA + 128 + 304, 4).unwrap(), 0); // hardware, non-stereo
        let luid = m.read_int(DATA + 128 + 296, 8).unwrap();
        m.write(DATA, &ADAPTERS[0]).unwrap();
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "Factory.EnumAdapterByLuid",
                &[f, luid, DATA, DATA + 40]
            )
            .unwrap(),
            0
        );
        assert_eq!(m.read_int(DATA + 40, 8).unwrap(), adapter);
        assert_eq!(
            call(&mut r, &mut m, "Object.Release", &[adapter]).unwrap(),
            1
        );
        assert_eq!(
            call(&mut r, &mut m, "Factory.EnumAdapters1", &[f, 1, DATA + 40]).unwrap(),
            NOT_FOUND
        );
        assert_eq!(m.read_int(DATA + 40, 8).unwrap(), 0);
        assert_eq!(call(&mut r, &mut m, "Object.Release", &[f]).unwrap(), 1); // child owns parent
        m.write(DATA, &FACTORIES[4]).unwrap();
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "Object.GetParent",
                &[adapter, DATA, DATA + 40]
            )
            .unwrap(),
            0
        );
        assert_eq!(m.read_int(DATA + 40, 8).unwrap(), f);
        assert_eq!(call(&mut r, &mut m, "Object.Release", &[f]).unwrap(), 1);
        assert_eq!(
            call(&mut r, &mut m, "Object.Release", &[adapter]).unwrap(),
            0
        );
        assert!(r.graphics.objects.is_empty());
        assert!(m.read_int(f, 8).is_err());
        assert!(m.read_int(adapter, 8).is_err());
        assert!(call(&mut r, &mut m, "Object.AddRef", &[f]).is_err());
    }
    #[test]
    fn dxgi_output_faults_do_not_allocate_or_retain_and_no_capability_is_faked() {
        let (mut r, mut m) = setup();
        m.write(DATA, &FACTORIES[4]).unwrap();
        let next = r.next_alloc;
        assert!(call(
            &mut r,
            &mut m,
            "CreateDXGIFactory2",
            &[0, DATA, DATA + 2 * PAGE - 4]
        )
        .is_err());
        assert_eq!(r.next_alloc, next);
        assert!(r.graphics.objects.is_empty());
        let f = factory(&mut r, &mut m);
        let count = r.graphics.objects[&f].refs;
        assert!(call(
            &mut r,
            &mut m,
            "Object.QueryInterface",
            &[f, DATA, DATA + 2 * PAGE - 4]
        )
        .is_err());
        assert_eq!(r.graphics.objects[&f].refs, count);
        assert!(call(
            &mut r,
            &mut m,
            "Factory.EnumAdapters1",
            &[f, 0, DATA + 2 * PAGE - 4]
        )
        .is_err());
        assert_eq!(r.graphics.objects.len(), 1);
        m.write(DATA, &[0x55; 16]).unwrap();
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "Object.QueryInterface",
                &[f, DATA, DATA + 40]
            )
            .unwrap(),
            NO_INTERFACE
        );
        assert_eq!(m.read_int(DATA + 40, 8).unwrap(), 0);
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "Factory.EnumWarpAdapter",
                &[f, DATA, DATA + 40]
            )
            .unwrap(),
            NOT_FOUND
        );
        assert_eq!(
            call(&mut r, &mut m, "Factory.CreateSwapChain", &[f, 0, 0, 0]).unwrap(),
            INVALID_ARG
        );
        m.write(DATA, &FACTORIES[4]).unwrap();
        assert_eq!(
            call(&mut r, &mut m, "CreateDXGIFactory2", &[1, DATA, DATA + 40]).unwrap(),
            0x887a002d
        );
        assert_eq!(r.graphics.objects.len(), 1);
        r.graphics.fixture = None;
        m.write_int(DATA + 40, 8, 0xaaaa).unwrap();
        assert!(
            call(&mut r, &mut m, "CreateDXGIFactory", &[DATA, DATA + 40])
                .unwrap_err()
                .contains("backend")
        );
        assert_eq!(m.read_int(DATA + 40, 8).unwrap(), 0xaaaa);
    }
    #[test]
    fn dxgi_private_data_size_queries_and_faults_preserve_stored_bytes() {
        let (mut r, mut m) = setup();
        let f = factory(&mut r, &mut m);
        m.write(DATA, &[0x11; 16]).unwrap();
        m.write(DATA + 64, b"ABCD").unwrap();
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "Object.SetPrivateData",
                &[f, DATA, 4, DATA + 64]
            )
            .unwrap(),
            0
        );
        m.write_int(DATA + 32, 4, 2).unwrap();
        m.write(DATA + 96, b"xxxx").unwrap();
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "Object.GetPrivateData",
                &[f, DATA, DATA + 32, DATA + 96]
            )
            .unwrap(),
            MORE_DATA
        );
        assert_eq!(m.read_int(DATA + 32, 4).unwrap(), 4);
        assert_eq!(m.read_int(DATA + 96, 4).unwrap(), 0x78787878);
        assert!(call(
            &mut r,
            &mut m,
            "Object.GetPrivateData",
            &[f, DATA, DATA + 32, DATA + 2 * PAGE - 2]
        )
        .is_err());
        assert_eq!(m.read_int(DATA + 32, 4).unwrap(), 4);
        assert!(call(
            &mut r,
            &mut m,
            "Object.SetPrivateData",
            &[f, DATA, 4, DATA + 2 * PAGE - 2]
        )
        .is_err());
        assert!(call(
            &mut r,
            &mut m,
            "Object.SetPrivateDataInterface",
            &[f, DATA, 0x1234]
        )
        .is_err());
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "Object.GetPrivateData",
                &[f, DATA, DATA + 32, DATA + 96]
            )
            .unwrap(),
            0
        );
        assert_eq!(m.read_int(DATA + 96, 4).unwrap(), 0x44434241);
        assert_eq!(
            call(&mut r, &mut m, "Object.SetPrivateData", &[f, DATA, 0, 0]).unwrap(),
            0
        );
        assert_eq!(
            call(
                &mut r,
                &mut m,
                "Object.GetPrivateData",
                &[f, DATA, DATA + 32, DATA + 96]
            )
            .unwrap(),
            NOT_FOUND
        );
    }
    #[test]
    fn d3d12_capability_queries_fail_truthfully_with_named_and_ordinal_abi() {
        let (mut r, mut m) = setup();
        m.write(DATA, &[0x44; 16]).unwrap();
        for name in ["D3D12CreateDevice", "#101"] {
            m.write_int(DATA + 32, 8, 0xabcdef1234).unwrap();
            assert_eq!(
                call(&mut r, &mut m, name, &[0, 0xb000, DATA, DATA + 32]).unwrap(),
                UNSUPPORTED
            );
            assert_eq!(m.read_int(DATA + 32, 8).unwrap(), 0);
            assert_eq!(
                call(&mut r, &mut m, name, &[0, 0xb000, DATA, 0]).unwrap(),
                UNSUPPORTED
            );
            assert!(call(
                &mut r,
                &mut m,
                name,
                &[0, 0xb000, DATA, DATA + 2 * PAGE - 4]
            )
            .is_err());
        }
        assert_eq!(
            call(&mut r, &mut m, "#102", &[DATA, DATA + 32]).unwrap(),
            NO_INTERFACE
        );
        assert!(r.graphics.objects.is_empty());
        let module = r.native_module("d3d12.dll", true).unwrap();
        m.write(DATA + 128, b"D3D12CreateDevice\0").unwrap();
        let named = call(&mut r, &mut m, "GetProcAddress", &[module, DATA + 128]).unwrap();
        let ordinal = call(&mut r, &mut m, "GetProcAddress", &[module, 101]).unwrap();
        assert_eq!(named, ordinal);
        assert_eq!(r.gateways[&named], "d3d12.dll!D3D12CreateDevice");
        assert_eq!(r.register("d3d12.dll", "#101"), named);
        assert_eq!(
            call(&mut r, &mut m, "GetProcAddress", &[module, 103]).unwrap(),
            0
        );
    }
}
