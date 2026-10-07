//! Host-backed DXGI discovery. Win64 COM layout is serialized explicitly;
//! host pointers never enter guest memory. No D3D feature level is advertised.
//! ABI checked against DXVK Native c8dc91fa directx/dxgi*.h; no implementation copied.
use super::{Memory, Runtime64, PAGE, READ, WRITE};
use crate::gpu::{AdapterInfo, NativeGraphics};
use std::collections::BTreeMap;

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
    match name {
        "CreateDXGIFactory" | "CreateDXGIFactory1" => Some(2),
        "CreateDXGIFactory2" => Some(3),
        "D3D12CreateDevice" => Some(4),
        "D3D12GetDebugInterface" => Some(2),
        _ => COMMON
            .iter()
            .chain(FACTORY)
            .chain(ADAPTER)
            .find(|(n, _)| *n == name)
            .map(|(_, a)| *a),
    }
}
pub(super) fn dwords(name: &str) -> &'static [usize] {
    match name {
        "CreateDXGIFactory2" => &[0],
        "D3D12CreateDevice" => &[1],
        "Object.SetPrivateData" => &[2],
        "Factory.EnumAdapters" | "Factory.EnumAdapters1" | "Adapter.EnumOutputs" => &[1],
        "Factory.MakeWindowAssociation" => &[2],
        _ => &[],
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Factory,
    Adapter,
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
    tables: [u64; 2],
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
}
fn read_guid(mem: &Memory, address: u64) -> Result<Guid, String> {
    let mut iid = [0; 16];
    mem.read(address, &mut iid)?;
    Ok(iid)
}
fn supports(kind: Kind, iid: &Guid) -> bool {
    *iid == UNKNOWN
        || *iid == OBJECT
        || match kind {
            Kind::Factory => FACTORIES.contains(iid),
            Kind::Adapter => ADAPTERS.contains(iid),
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
        let index = (kind == Kind::Adapter) as usize;
        if self.graphics.tables[index] != 0 {
            return Ok(self.graphics.tables[index]);
        }
        let mut bytes = Vec::new();
        for (name, _) in COMMON
            .iter()
            .chain(if index == 0 { FACTORY } else { ADAPTER })
        {
            bytes.extend_from_slice(&self.register(DLL, name).to_le_bytes());
        }
        let address = self.next_alloc;
        mem.map(address, PAGE, READ | WRITE)?;
        mem.write(address, &bytes)?;
        mem.seal_runtime_readonly(address)?;
        self.next_alloc += PAGE;
        self.graphics.tables[index] = address;
        Ok(address)
    }
    fn dxgi_object(&mut self, mem: &mut Memory, kind: Kind, parent: u64) -> Result<u64, String> {
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
                association: 0,
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
        if (name.starts_with("Factory.") && kind != Kind::Factory)
            || (name.starts_with("Adapter.") && kind != Kind::Adapter)
        {
            return Err("DXGI method/object mismatch".into());
        }
        match name {
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
        assert!(
            call(&mut r, &mut m, "Factory.CreateSwapChain", &[f, 0, 0, 0])
                .unwrap_err()
                .contains("unimplemented")
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
