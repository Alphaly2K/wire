//! Finite diagnostic object identities. Native strong roots own strings; only
//! immutable length/UTF16 payload snapshots are visible in guest memory.
use super::super::super::memory::{shared::HostRegion, READ};
use super::{c_char, Memory, Object};
use std::{collections::BTreeMap, ptr};
const BASE: u64 = 0x6ffe70000000;
const STRIDE: u64 = 131072;
const LIMIT: usize = 1024;
#[derive(Clone, Copy)]
pub(super) struct Api {
    pub(super) new_string: unsafe extern "C" fn(Object, *const c_char, u32) -> Object,
    strong: unsafe extern "C" fn(Object, i32) -> Object,
    target: unsafe extern "C" fn(Object) -> Object,
    free: unsafe extern "C" fn(Object),
    length: unsafe extern "C" fn(Object) -> i32,
    chars: unsafe extern "C" fn(Object) -> *const u16,
    class: unsafe extern "C" fn(Object) -> Object,
    string_class: unsafe extern "C" fn() -> Object,
}
impl Api {
    pub(super) fn load(library: Object) -> Result<Self, String> {
        macro_rules! sym {
            ($name:literal,$ty:ty) => {{
                let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return Err(concat!("missing Mono object symbol: ", $name).into());
                }
                unsafe { std::mem::transmute::<Object, $ty>(p) }
            }};
        }
        Ok(Self {
            new_string: sym!(
                "mono_string_new_len",
                unsafe extern "C" fn(Object, *const c_char, u32) -> Object
            ),
            strong: sym!(
                "mono_gchandle_new_v2",
                unsafe extern "C" fn(Object, i32) -> Object
            ),
            target: sym!(
                "mono_gchandle_get_target_v2",
                unsafe extern "C" fn(Object) -> Object
            ),
            free: sym!("mono_gchandle_free_v2", unsafe extern "C" fn(Object)),
            length: sym!("mono_string_length", unsafe extern "C" fn(Object) -> i32),
            chars: sym!(
                "mono_string_chars",
                unsafe extern "C" fn(Object) -> *const u16
            ),
            class: sym!(
                "mono_object_get_class",
                unsafe extern "C" fn(Object) -> Object
            ),
            string_class: sym!("mono_get_string_class", unsafe extern "C" fn() -> Object),
        })
    }
}
struct Entry {
    native: Object,
    root: Object,
    payload: Vec<u8>,
    view: HostRegion,
    published: bool,
}
#[derive(Default)]
pub(super) struct Registry {
    entries: BTreeMap<u64, Entry>,
    verified: usize,
    roots_released: bool,
    retired: bool,
}
impl Registry {
    pub(super) fn count(&self) -> usize {
        self.entries.len()
    }
    pub(super) fn contains(&self, token: u64) -> bool {
        !self.roots_released && !self.retired && self.entries.contains_key(&token)
    }
    fn payload(api: Api, native: Object) -> Result<Vec<u8>, String> {
        if native.is_null() || unsafe { (api.class)(native) != (api.string_class)() } {
            return Err("object is not an exact native System.String".into());
        }
        let length = unsafe { (api.length)(native) };
        if !(0..=32768).contains(&length) {
            return Err("native string length outside diagnostic bound".into());
        }
        let chars = unsafe { (api.chars)(native) };
        // Candidate and original locked 64-bit MonoString payload ABI. Never
        // expose the preceding vtable/synchronization fields or host pointers.
        if chars != unsafe { native.cast::<u8>().add(20).cast::<u16>() }
            || unsafe { native.cast::<u8>().add(16).cast::<i32>().read_unaligned() } != length
        {
            return Err("native string payload ABI mismatch".into());
        }
        let mut payload = length.to_le_bytes().to_vec();
        for index in 0..=length as usize {
            let ch = unsafe { chars.add(index).read_unaligned() };
            if index == length as usize && ch != 0 {
                return Err("native string has no UTF16 terminator".into());
            }
            payload.extend_from_slice(&ch.to_le_bytes());
        }
        Ok(payload)
    }
    pub(super) fn root_string(&mut self, api: Api, native: Object) -> Result<u64, String> {
        if self.roots_released || self.retired || self.entries.len() >= LIMIT {
            return Err("native string outside live bounded root registry".into());
        }
        if native.is_null() {
            return Err("native string allocation returned NULL".into());
        }
        if let Some((token, _)) = self.entries.iter().find(|(_, e)| e.native == native) {
            return Ok(*token);
        }
        let root = unsafe { (api.strong)(native, 0) };
        if root.is_null() {
            return Err("native string strong root allocation failed".into());
        }
        let result = (|| {
            if unsafe { (api.target)(root) } != native {
                return Err("native string root target mismatch".into());
            }
            let payload = Self::payload(api, native)?;
            let view = HostRegion::owned(payload.len())?;
            view.with_native(|p| unsafe {
                ptr::copy_nonoverlapping(payload.as_ptr(), p.as_ptr(), payload.len());
            })?;
            let token = BASE + self.entries.len() as u64 * STRIDE;
            self.entries.insert(
                token,
                Entry {
                    native,
                    root,
                    payload,
                    view,
                    published: false,
                },
            );
            Ok(token)
        })();
        if result.is_err() {
            unsafe {
                (api.free)(root);
            }
        }
        result
    }
    pub(super) fn publish_string(&mut self, memory: &mut Memory, token: u64) -> Result<(), String> {
        if !self.contains(token) {
            return Err("string publish outside live root registry".into());
        }
        let entry = self.entries.get_mut(&token).unwrap();
        if !entry.published {
            memory.map_shared_data(token + 16, entry.view.clone(), READ, READ)?;
            entry.published = true;
        }
        Ok(())
    }
    pub(super) fn native_string(&self, api: Api, token: u64) -> Result<Object, String> {
        if !self.contains(token) {
            return Err("unknown or retired managed string token".into());
        }
        let entry = &self.entries[&token];
        if unsafe { (api.target)(entry.root) } != entry.native {
            return Err("native string root lost identity".into());
        }
        Ok(entry.native)
    }
    pub(super) fn verify(&mut self, api: Api) -> Result<(), String> {
        if self.roots_released || self.retired {
            return Err("string verification after root retirement".into());
        }
        for entry in self.entries.values() {
            if unsafe { (api.target)(entry.root) } != entry.native
                || Self::payload(api, entry.native)? != entry.payload
            {
                return Err(
                    "strong-rooted string changed identity or immutable payload across GC".into(),
                );
            }
            self.verified += 1;
        }
        Ok(())
    }
    pub(super) fn release_roots(&mut self, api: Api) {
        if self.roots_released {
            return;
        }
        self.roots_released = true;
        for entry in self.entries.values_mut() {
            unsafe {
                (api.free)(entry.root);
            }
            entry.root = ptr::null_mut();
        }
    }
    pub(super) fn retire_views(&mut self, memory: &mut Memory) -> Result<(), String> {
        if self.retired {
            return Ok(());
        }
        if !self.roots_released && !self.entries.is_empty() {
            return Err("string views retired before native roots".into());
        }
        for (token, entry) in &self.entries {
            if entry.published {
                memory.unmap_shared_data(token + 16)?;
            }
            entry.view.revoke()?;
        }
        self.retired = true;
        Ok(())
    }
    pub(super) fn report(&self) -> String {
        format!("native-strings={}\nnative-string-gc-verifications={}\nnative-string-roots-released={}\nnative-string-views-retired={}\n", self.count(), self.verified, self.roots_released, self.retired)
    }
}
#[cfg(test)]
mod tests {
    use super::super::super::super::memory::{EXECUTE, WRITE};
    use super::*;
    use std::cell::Cell;
    thread_local! { static ROOTS:Cell<usize> = const {Cell::new(0)}; }
    unsafe extern "C" fn new_string(_: Object, _: *const c_char, _: u32) -> Object {
        ptr::null_mut()
    }
    unsafe extern "C" fn strong(p: Object, _: i32) -> Object {
        ROOTS.with(|n| n.set(n.get() + 1));
        p
    }
    unsafe extern "C" fn target(p: Object) -> Object {
        p
    }
    unsafe extern "C" fn free(_: Object) {
        ROOTS.with(|n| n.set(n.get() - 1));
    }
    unsafe extern "C" fn length(p: Object) -> i32 {
        unsafe { p.cast::<u8>().add(16).cast::<i32>().read_unaligned() }
    }
    unsafe extern "C" fn chars(p: Object) -> *const u16 {
        unsafe { p.cast::<u8>().add(20).cast() }
    }
    unsafe extern "C" fn class(_: Object) -> Object {
        0x55usize as Object
    }
    unsafe extern "C" fn string_class() -> Object {
        0x55usize as Object
    }
    #[test]
    fn string_roots_own_exact_readonly_payload_and_release_once_before_view_retirement() {
        let api = Api {
            new_string,
            strong,
            target,
            free,
            length,
            chars,
            class,
            string_class,
        };
        let mut raw = vec![0u8; 28];
        raw[16..20].copy_from_slice(&3i32.to_le_bytes());
        raw[20..26].copy_from_slice(&[0x41, 0, 0x3d, 0xd8, 0, 0xde]);
        let native = raw.as_mut_ptr().cast();
        let mut registry = Registry::default();
        let mut memory = Memory::default();
        let token = registry.root_string(api, native).unwrap();
        registry.publish_string(&mut memory, token).unwrap();
        assert_ne!(token, native as u64);
        assert_eq!(ROOTS.with(Cell::get), 1);
        assert_eq!(memory.read_int(token + 16, 4).unwrap(), 3);
        assert_eq!(memory.read_int(token + 20, 2).unwrap(), 0x41);
        assert!(memory.read_int(token + 8, 8).is_err());
        assert!(memory.read_int(token + 28, 1).is_err());
        assert!(memory.check(token + 20, 2, WRITE).is_err());
        assert!(memory.check(token + 20, 2, EXECUTE).is_err());
        assert!(registry.native_string(api, token + 1).is_err());
        assert!(registry.native_string(api, native as u64).is_err());
        registry.verify(api).unwrap();
        assert!(registry.retire_views(&mut memory).is_err());
        assert_eq!(registry.root_string(api, native).unwrap(), token);
        assert_eq!(ROOTS.with(Cell::get), 1);
        registry.release_roots(api);
        registry.release_roots(api);
        assert_eq!(ROOTS.with(Cell::get), 0);
        assert!(registry.native_string(api, token).is_err());
        registry.retire_views(&mut memory).unwrap();
        registry.retire_views(&mut memory).unwrap();
        assert!(memory.read_int(token + 16, 4).is_err());
        assert!(registry.root_string(api, native).is_err());
    }
}
