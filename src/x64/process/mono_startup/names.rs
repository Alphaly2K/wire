//! Parsed assembly names keep native ownership in an aligned host object. Guest
//! structs contain owned string tokens and scalar snapshots, never host pointers.
use super::super::super::memory::READ;
use super::{c_char, CString, Memory, Object};
use std::{collections::BTreeMap, ffi::CStr};
#[repr(C)]
#[derive(Default)]
struct NativeName {
    name: *const c_char,
    culture: *const c_char,
    hash: Object,
    public_key: Object,
    token: [u8; 17],
    hash_alg: u32,
    hash_len: u32,
    flags: u32,
    major: u16,
    minor: u16,
    build: u16,
    revision: u16,
    arch: u16,
    without_version: u8,
    without_culture: u8,
    without_token: u8,
}
#[derive(Clone, Copy)]
pub(super) struct Api {
    parse: unsafe extern "C" fn(*const c_char, *mut NativeName) -> i32,
    loaded: unsafe extern "C" fn(*mut NativeName) -> Object,
    free: unsafe extern "C" fn(*mut NativeName),
}
impl Api {
    pub(super) fn load(library: Object) -> Result<Self, String> {
        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return Err(concat!("missing assembly name symbol: ", $name).into());
                }
                unsafe { std::mem::transmute::<Object, $ty>(p) }
            }};
        }
        Ok(Self {
            parse: sym!(
                "mono_assembly_name_parse",
                unsafe extern "C" fn(*const c_char, *mut NativeName) -> i32
            ),
            loaded: sym!(
                "mono_assembly_loaded",
                unsafe extern "C" fn(*mut NativeName) -> Object
            ),
            free: sym!(
                "mono_assembly_name_free",
                unsafe extern "C" fn(*mut NativeName)
            ),
        })
    }
}
pub(super) struct Snapshot {
    pub(super) name: CString,
    pub(super) culture: Option<CString>,
    pub(super) bytes: [u8; 80],
}
struct Entry {
    native: Box<NativeName>,
    expected: Option<[u8; 80]>,
}
#[derive(Default)]
pub(super) struct Registry {
    entries: BTreeMap<u64, Entry>,
    parses: usize,
    lookups: usize,
    hits: usize,
    frees: usize,
    retired: bool,
}
impl Registry {
    pub(super) fn check_stack(
        memory: &Memory,
        rsp: u64,
        address: u64,
        access: u8,
    ) -> Result<(), String> {
        let first = rsp.checked_add(40).ok_or("assembly name stack overflow")?;
        let end = address
            .checked_add(80)
            .ok_or("assembly name object overflow")?;
        if !address.is_multiple_of(8)
            || address < first
            || address < super::super::STACK
            || end > super::super::STACK + 64 * 1024 * 1024
        {
            return Err("assembly name requires aligned live main-stack storage".into());
        }
        memory.check(address, 80, access)
    }
    pub(super) fn parse(
        &mut self,
        api: Api,
        text: &CString,
        address: u64,
    ) -> Result<Option<Snapshot>, String> {
        if self.retired || self.entries.len() >= 4096 || self.entries.contains_key(&address) {
            return Err("assembly name parse requires a fresh live storage identity".into());
        }
        let mut native = Box::<NativeName>::default();
        if unsafe { (api.parse)(text.as_ptr(), &mut *native) } == 0 {
            // build_assembly_name frees its failure allocations; fields may
            // contain dangling pointers, so never call free on a failed parse.
            return Ok(None);
        }
        let result = (|| {
            if native.name.is_null() || !native.hash.is_null() || !native.public_key.is_null() {
                return Err("assembly name requires name/culture/scalars; hash/public key pointers not admitted".into());
            }
            let name = unsafe { CStr::from_ptr(native.name) }.to_owned();
            let culture = if native.culture.is_null() {
                None
            } else {
                Some(unsafe { CStr::from_ptr(native.culture) }.to_owned())
            };
            let mut bytes = [0u8; 80];
            bytes[32..49].copy_from_slice(&native.token);
            for (offset, value) in [
                (52, native.hash_alg),
                (56, native.hash_len),
                (60, native.flags),
            ] {
                bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            }
            for (offset, value) in [
                (64, native.major),
                (66, native.minor),
                (68, native.build),
                (70, native.revision),
                (72, native.arch),
            ] {
                bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
            }
            bytes[74] = native.without_version;
            bytes[75] = native.without_culture;
            bytes[76] = native.without_token;
            Ok(Snapshot {
                name,
                culture,
                bytes,
            })
        })();
        match result {
            Ok(snapshot) => {
                self.entries.insert(
                    address,
                    Entry {
                        native,
                        expected: None,
                    },
                );
                self.parses += 1;
                Ok(Some(snapshot))
            }
            Err(error) => {
                unsafe { (api.free)(&mut *native) };
                Err(error)
            }
        }
    }
    pub(super) fn publish(
        &mut self,
        memory: &mut Memory,
        address: u64,
        bytes: [u8; 80],
    ) -> Result<(), String> {
        let entry = self
            .entries
            .get_mut(&address)
            .ok_or("assembly name publication without native owner")?;
        if self.retired || entry.expected.is_some() {
            return Err("assembly name publication repeated or retired".into());
        }
        memory.write(address, &bytes)?;
        entry.expected = Some(bytes);
        Ok(())
    }
    pub(super) fn validate(&self, memory: &Memory, rsp: u64, address: u64) -> Result<(), String> {
        if self.retired {
            return Err("assembly name registry retired".into());
        }
        Self::check_stack(memory, rsp, address, READ)?;
        let entry = self
            .entries
            .get(&address)
            .ok_or("unknown or freed parsed assembly name")?;
        let expected = entry
            .expected
            .ok_or("assembly name queried before publication")?;
        let mut actual = [0u8; 80];
        memory.read(address, &mut actual)?;
        if actual != expected {
            return Err("assembly name struct changed outside typed provider".into());
        }
        Ok(())
    }
    pub(super) fn loaded(&mut self, api: Api, address: u64) -> Result<Object, String> {
        if self.retired {
            return Err("assembly lookup after retirement".into());
        }
        let entry = self
            .entries
            .get_mut(&address)
            .ok_or("assembly lookup requires a live parsed name")?;
        let result = unsafe { (api.loaded)(&mut *entry.native) };
        self.lookups += 1;
        self.hits += usize::from(!result.is_null());
        Ok(result)
    }
    pub(super) fn free(&mut self, api: Api, address: u64) -> Result<(), String> {
        if self.retired {
            return Err("assembly name free after retirement".into());
        }
        let mut entry = self
            .entries
            .remove(&address)
            .ok_or("unknown or double-freed assembly name")?;
        unsafe { (api.free)(&mut *entry.native) };
        self.frees += 1;
        Ok(())
    }
    pub(super) fn release(&mut self, api: Api) {
        if self.retired {
            return;
        }
        self.retired = true;
        for (_, mut entry) in std::mem::take(&mut self.entries) {
            unsafe { (api.free)(&mut *entry.native) };
        }
    }
    pub(super) fn ready(&self) -> bool {
        self.hits > 0 && self.frees > 0
    }
    pub(super) fn report(&self) -> String {
        format!("native-assembly-name-parses={}\nnative-assembly-name-lookups={}\nnative-assembly-name-hits={}\nnative-assembly-name-frees={}\nnative-assembly-names-retired={}\n",self.parses,self.lookups,self.hits,self.frees,self.retired)
    }
}
pub(super) fn handles(function: &str) -> bool {
    matches!(
        function,
        "mono_assembly_name_parse" | "mono_assembly_loaded" | "mono_assembly_name_free"
    )
}
#[cfg(test)]
mod tests {
    use super::super::super::super::memory::WRITE;
    use super::*;
    use std::cell::Cell;
    thread_local! { static LIVE: Cell<usize> = const { Cell::new(0) }; }
    unsafe extern "C" fn parse(_: *const c_char, output: *mut NativeName) -> i32 {
        unsafe {
            (*output).name = c"Authored".as_ptr();
            (*output).major = 3;
        }
        LIVE.with(|v| v.set(v.get() + 1));
        1
    }
    unsafe extern "C" fn loaded(_: *mut NativeName) -> Object {
        0x1234usize as Object
    }
    unsafe extern "C" fn free(_: *mut NativeName) {
        LIVE.with(|v| v.set(v.get() - 1));
    }
    #[test]
    fn assembly_name_snapshot_matches_original_layout_and_keeps_host_pointers_private() {
        assert_eq!(std::mem::size_of::<NativeName>(), 80);
        assert_eq!(std::mem::offset_of!(NativeName, token), 32);
        assert_eq!(std::mem::offset_of!(NativeName, hash_alg), 52);
        assert_eq!(std::mem::offset_of!(NativeName, major), 64);
        assert_eq!(std::mem::offset_of!(NativeName, without_token), 76);
        LIVE.with(|v| v.set(0));
        let api = Api {
            parse,
            loaded,
            free,
        };
        let mut registry = Registry::default();
        let mut memory = Memory::default();
        let base = super::super::super::STACK;
        memory.map(base, 4096, READ | WRITE).unwrap();
        let rsp = base + 16;
        let address = base + 256;
        let snapshot = registry
            .parse(api, &CString::new("Authored").unwrap(), address)
            .unwrap()
            .unwrap();
        assert_eq!(&snapshot.bytes[..32], &[0; 32]);
        assert_eq!(&snapshot.bytes[64..66], &3u16.to_le_bytes());
        let mut bytes = snapshot.bytes;
        bytes[..8].copy_from_slice(&0x6ffe50000000u64.to_le_bytes());
        registry.publish(&mut memory, address, bytes).unwrap();
        registry.validate(&memory, rsp, address).unwrap();
        assert!(registry.validate(&memory, rsp, address + 8).is_err());
        assert!(registry
            .parse(api, &CString::new("Authored").unwrap(), address)
            .is_err());
        assert_eq!(registry.loaded(api, address).unwrap() as usize, 0x1234);
        memory.write_int(address + 64, 2, 4).unwrap();
        assert!(registry.validate(&memory, rsp, address).is_err());
        registry.free(api, address).unwrap();
        assert!(registry.free(api, address).is_err());
        assert!(registry.validate(&memory, rsp, address).is_err());
        registry
            .parse(api, &CString::new("Authored").unwrap(), address)
            .unwrap();
        registry.release(api);
        registry.release(api);
        assert_eq!(LIVE.with(Cell::get), 0);
        assert!(registry.loaded(api, address).is_err());
        assert!(Registry::check_stack(&memory, rsp, address + 1, WRITE).is_err());
        assert!(Registry::check_stack(&memory, rsp, rsp, WRITE).is_err());
    }
}
