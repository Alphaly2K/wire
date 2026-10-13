//! Empty startup array identities. Native strong roots own real Mono arrays;
//! guest sees only the verified immutable bounds/length cells, never headers.
use super::super::super::memory::{shared::HostRegion, READ};
use super::{Memory, Object};
use std::{collections::BTreeMap, ptr};
const BASE: u64 = 0x6ffe90000000;
#[derive(Clone, Copy)]
pub(super) struct Api {
    pub(super) new: unsafe extern "C" fn(Object, Object, usize) -> Object,
    strong: unsafe extern "C" fn(Object, i32) -> Object,
    target: unsafe extern "C" fn(Object) -> Object,
    free: unsafe extern "C" fn(Object),
    class: unsafe extern "C" fn(Object) -> Object,
    array_class: unsafe extern "C" fn(Object, u32) -> Object,
    length: unsafe extern "C" fn(Object) -> usize,
}
impl Api {
    pub(super) fn load(library: Object) -> Result<Self, String> {
        macro_rules! sym {
            ($name:literal,$ty:ty) => {{
                let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return Err(concat!("missing startup array symbol: ", $name).into());
                }
                unsafe { std::mem::transmute::<Object, $ty>(p) }
            }};
        }
        Ok(Self {
            new: sym!(
                "mono_array_new",
                unsafe extern "C" fn(Object, Object, usize) -> Object
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
            class: sym!(
                "mono_object_get_class",
                unsafe extern "C" fn(Object) -> Object
            ),
            array_class: sym!(
                "mono_array_class_get",
                unsafe extern "C" fn(Object, u32) -> Object
            ),
            length: sym!("mono_array_length", unsafe extern "C" fn(Object) -> usize),
        })
    }
}
struct Entry {
    native: Object,
    root: Object,
    class: Object,
    view: HostRegion,
    published: bool,
}
#[derive(Default)]
pub(super) struct Registry {
    entries: BTreeMap<u64, Entry>,
    verified: usize,
    released: bool,
    retired: bool,
}
impl Registry {
    pub(super) fn contains(&self, token: u64) -> bool {
        !self.released && !self.retired && self.entries.contains_key(&token)
    }
    fn validate(api: Api, native: Object, class: Object) -> Result<(), String> {
        if native.is_null()
            || class.is_null()
            || unsafe { (api.class)(native) } != class
            || unsafe { (api.length)(native) } != 0
            || !unsafe {
                native
                    .cast::<u8>()
                    .add(16)
                    .cast::<Object>()
                    .read_unaligned()
            }
            .is_null()
            || unsafe { native.cast::<u8>().add(24).cast::<usize>().read_unaligned() } != 0
        {
            return Err("native empty SZ array identity/layout mismatch".into());
        }
        Ok(())
    }
    pub(super) fn root_empty(
        &mut self,
        api: Api,
        native: Object,
        element: Object,
    ) -> Result<u64, String> {
        if self.released
            || self.retired
            || self.entries.len() >= 1024
            || element.is_null()
            || native.is_null()
        {
            return Err("empty array outside live bounded startup registry".into());
        }
        let root = unsafe { (api.strong)(native, 0) };
        if root.is_null() {
            return Err("empty array strong root allocation failed".into());
        }
        let result = (|| {
            let class = unsafe { (api.array_class)(element, 1) };
            Self::validate(api, native, class)?;
            if unsafe { (api.target)(root) } != native {
                return Err("empty array root identity mismatch".into());
            }
            let token = BASE + self.entries.len() as u64 * 256;
            let view = HostRegion::owned(16)?;
            // Bounds NULL and max_length zero are checked against the real
            // array. This does not admit array data stores or native vtables.
            self.entries.insert(
                token,
                Entry {
                    native,
                    root,
                    class,
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
    pub(super) fn native(&self, api: Api, token: u64) -> Result<Object, String> {
        if !self.contains(token) {
            return Err("unknown or retired empty array token".into());
        }
        let entry = &self.entries[&token];
        if unsafe { (api.target)(entry.root) } != entry.native {
            return Err("empty array root lost identity".into());
        }
        Ok(entry.native)
    }
    pub(super) fn adopt_early_roots(
        &self,
        api: Api,
        root_api: super::instances::Api,
        roots: &mut super::instances::Registry,
    ) -> Result<(), String> {
        if self.released || self.retired {
            return Err("early empty array adoption after retirement".into());
        }
        let mut pending = Vec::with_capacity(self.entries.len());
        for (&token, entry) in &self.entries {
            let native = self.native(api, token)?;
            Self::validate(api, native, entry.class)?;
            pending.push((token, native));
        }
        for (token, native) in pending {
            roots.adopt_empty_array(root_api, token, native)?;
        }
        Ok(())
    }
    pub(super) fn publish(&mut self, memory: &mut Memory, token: u64) -> Result<(), String> {
        if !self.contains(token) {
            return Err("empty array publication after retirement".into());
        }
        let entry = self.entries.get_mut(&token).unwrap();
        if !entry.published {
            memory.map_shared_data(token + 16, entry.view.clone(), READ, READ)?;
            entry.published = true;
        }
        Ok(())
    }
    pub(super) fn verify(&mut self, api: Api) -> Result<(), String> {
        if self.released || self.retired {
            return Err("empty arrays verified after retirement".into());
        }
        for entry in self.entries.values() {
            if unsafe { (api.target)(entry.root) } != entry.native {
                return Err("empty array root changed across GC".into());
            }
            Self::validate(api, entry.native, entry.class)?;
            self.verified += 1;
        }
        Ok(())
    }
    pub(super) fn release(&mut self, api: Api) {
        if self.released {
            return;
        }
        self.released = true;
        for entry in self.entries.values_mut() {
            unsafe {
                (api.free)(entry.root);
            }
            entry.root = ptr::null_mut();
        }
    }
    pub(super) fn retire(&mut self, memory: &mut Memory) -> Result<(), String> {
        if self.retired {
            return Ok(());
        }
        if !self.released && !self.entries.is_empty() {
            return Err("empty array views retired before roots".into());
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
        format!("native-empty-arrays={}\nnative-array-gc-verifications={}\nnative-array-roots-released={}\nnative-array-views-retired={}\n",self.entries.len(),self.verified,self.released,self.retired)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    thread_local! { static ROOTS:Cell<usize> = const { Cell::new(0) }; }
    unsafe extern "C" fn new(_: Object, _: Object, _: usize) -> Object {
        ptr::null_mut()
    }
    unsafe extern "C" fn strong(p: Object, _: i32) -> Object {
        ROOTS.with(|r| r.set(r.get() + 1));
        p
    }
    unsafe extern "C" fn target(p: Object) -> Object {
        p
    }
    unsafe extern "C" fn free(_: Object) {
        ROOTS.with(|r| r.set(r.get() - 1));
    }
    unsafe extern "C" fn class(_: Object) -> Object {
        0x55usize as Object
    }
    unsafe extern "C" fn array_class(_: Object, _: u32) -> Object {
        0x55usize as Object
    }
    unsafe extern "C" fn length(p: Object) -> usize {
        unsafe { p.cast::<u8>().add(24).cast::<usize>().read_unaligned() }
    }
    #[test]
    fn bootstrap_empty_arrays_reuse_graph_identity_and_reject_changed_headers() {
        let api = Api {
            new,
            strong,
            target,
            free,
            class,
            array_class,
            length,
        };
        let root_api = super::super::instances::tests::identity_api();
        let mut raw = [[0usize; 4]; 2];
        let mut registry = Registry::default();
        let mut roots = super::super::instances::Registry::default();
        let tokens: Vec<_> = raw
            .iter_mut()
            .map(|bytes| {
                registry
                    .root_empty(api, bytes.as_mut_ptr().cast(), 0x11usize as Object)
                    .unwrap()
            })
            .collect();
        raw[1][3] = 1;
        assert!(registry
            .adopt_early_roots(api, root_api, &mut roots)
            .is_err());
        assert_eq!(roots.count(), 0);
        raw[1][3] = 0;
        registry
            .adopt_early_roots(api, root_api, &mut roots)
            .unwrap();
        registry
            .adopt_early_roots(api, root_api, &mut roots)
            .unwrap();
        for (&token, bytes) in tokens.iter().zip(&mut raw) {
            let native = bytes.as_mut_ptr().cast();
            assert_eq!(roots.borrow(root_api, native).unwrap(), token);
            assert_eq!(roots.native(root_api, token).unwrap(), native);
        }
        roots.release(root_api);
        registry.release(api);
        assert!(registry
            .adopt_early_roots(api, root_api, &mut roots)
            .is_err());
        assert_eq!(ROOTS.with(Cell::get), 0);
    }
    #[test]
    fn empty_array_roots_check_real_header_and_revoke_only_the_readonly_view() {
        ROOTS.with(|r| r.set(0));
        let api = Api {
            new,
            strong,
            target,
            free,
            class,
            array_class,
            length,
        };
        let mut raw = [0usize; 4];
        let mut registry = Registry::default();
        let mut memory = Memory::default();
        let native: Object = raw.as_mut_ptr().cast();
        unsafe {
            native.cast::<usize>().add(3).write(1);
        }
        assert!(registry
            .root_empty(api, native, 0x11usize as Object)
            .is_err());
        assert_eq!(ROOTS.with(Cell::get), 0);
        unsafe {
            native.cast::<usize>().add(3).write(0);
        }
        let token = registry
            .root_empty(api, native, 0x11usize as Object)
            .unwrap();
        registry.publish(&mut memory, token).unwrap();
        assert_ne!(token, native as u64);
        assert_eq!(memory.read_int(token + 16, 8).unwrap(), 0);
        assert_eq!(memory.read_int(token + 24, 8).unwrap(), 0);
        assert!(memory.read_int(token, 8).is_err());
        assert!(memory.read_int(token + 32, 1).is_err());
        assert!(memory.write_int(token + 24, 8, 1).is_err());
        assert!(registry.retire(&mut memory).is_err());
        registry.verify(api).unwrap();
        unsafe {
            native.cast::<usize>().add(3).write(1);
        }
        assert!(registry.verify(api).is_err());
        registry.release(api);
        registry.release(api);
        assert_eq!(ROOTS.with(Cell::get), 0);
        assert!(registry.publish(&mut memory, token).is_err());
        assert!(registry.verify(api).is_err());
        registry.retire(&mut memory).unwrap();
        registry.retire(&mut memory).unwrap();
        assert!(memory.read_int(token + 24, 8).is_err());
        assert!(!registry.contains(token));
    }
}
