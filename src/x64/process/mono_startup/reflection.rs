//! Strong-rooted RuntimeType identities with one exact read-only metadata-token
//! cell. Managed object headers and native MonoType pointers remain private.
use super::super::super::memory::{shared::HostRegion, READ};
use super::{c_char, Memory, Object};
use std::{collections::BTreeMap, ptr};
const BASE: u64 = 0x6ffe80000000;
#[derive(Clone, Copy)]
pub(super) struct Api {
    strong: unsafe extern "C" fn(Object, i32) -> Object,
    target: unsafe extern "C" fn(Object) -> Object,
    free: unsafe extern "C" fn(Object),
    class: unsafe extern "C" fn(Object) -> Object,
    corlib: unsafe extern "C" fn() -> Object,
    class_from_name: unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object,
    pub(super) get_type: unsafe extern "C" fn(Object) -> Object,
}
impl Api {
    pub(super) fn load(library: Object) -> Result<Self, String> {
        macro_rules! sym {
            ($name:literal,$ty:ty) => {{
                let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return Err(concat!("missing Mono reflection symbol: ", $name).into());
                }
                unsafe { std::mem::transmute::<Object, $ty>(p) }
            }};
        }
        Ok(Self {
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
            corlib: sym!("mono_get_corlib", unsafe extern "C" fn() -> Object),
            class_from_name: sym!(
                "mono_class_from_name",
                unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object
            ),
            get_type: sym!(
                "mono_reflection_type_get_type",
                unsafe extern "C" fn(Object) -> Object
            ),
        })
    }
}
struct Entry {
    native: Object,
    root: Object,
    ty: Object,
    view: Option<HostRegion>,
    token: u64,
}
#[derive(Default)]
pub(super) struct Registry {
    entries: BTreeMap<u64, Entry>,
    verified: usize,
    roots_released: bool,
    retired: bool,
}
impl Registry {
    pub(super) fn contains(&self, token: u64) -> bool {
        !self.roots_released && !self.retired && self.entries.contains_key(&token)
    }
    pub(super) fn root_type(&mut self, api: Api, native: Object) -> Result<(u64, Object), String> {
        if self.roots_released || self.retired || self.entries.len() >= 1024 {
            return Err("reflection root registry retired/full".into());
        }
        if native.is_null() {
            return Ok((0, ptr::null_mut()));
        }
        if let Some((token, entry)) = self.entries.iter().find(|(_, e)| e.native == native) {
            return Ok((*token, entry.ty));
        }
        let expected = unsafe {
            (api.class_from_name)((api.corlib)(), c"System".as_ptr(), c"RuntimeType".as_ptr())
        };
        if expected.is_null() || unsafe { (api.class)(native) } != expected {
            return Err("invoke result is not exact corelib RuntimeType".into());
        }
        let root = unsafe { (api.strong)(native, 0) };
        if root.is_null() {
            return Err("RuntimeType strong root allocation failed".into());
        }
        let result = (|| {
            if unsafe { (api.target)(root) } != native {
                return Err("RuntimeType root identity mismatch".into());
            }
            let ty = unsafe { (api.get_type)(native) };
            if ty.is_null()
                || unsafe {
                    native
                        .cast::<u8>()
                        .add(16)
                        .cast::<Object>()
                        .read_unaligned()
                } != ty
            {
                return Err("RuntimeType metadata payload ABI mismatch".into());
            }
            let token = BASE + self.entries.len() as u64 * 256;
            self.entries.insert(
                token,
                Entry {
                    native,
                    root,
                    ty,
                    view: None,
                    token: 0,
                },
            );
            Ok((token, ty))
        })();
        if result.is_err() {
            unsafe {
                (api.free)(root);
            }
        }
        result
    }
    pub(super) fn publish(
        &mut self,
        memory: &mut Memory,
        object: u64,
        ty: u64,
    ) -> Result<(), String> {
        if object == 0 && ty == 0 {
            return Ok(());
        }
        if !self.contains(object) || ty == 0 {
            return Err("unknown reflection object/type publication".into());
        }
        let entry = self.entries.get_mut(&object).unwrap();
        if entry.view.is_some() {
            if entry.token != ty {
                return Err("reflection metadata identity changed".into());
            }
            return Ok(());
        }
        let view = HostRegion::owned(8)?;
        view.with_native(|p| unsafe {
            ptr::copy_nonoverlapping(ty.to_le_bytes().as_ptr(), p.as_ptr(), 8);
        })?;
        memory.map_shared_data(object + 16, view.clone(), READ, READ)?;
        entry.view = Some(view);
        entry.token = ty;
        Ok(())
    }
    pub(super) fn native(&self, api: Api, token: u64) -> Result<Object, String> {
        if !self.contains(token) {
            return Err("unknown or retired RuntimeType token".into());
        }
        let entry = &self.entries[&token];
        if unsafe { (api.target)(entry.root) } != entry.native {
            return Err("RuntimeType strong root lost identity".into());
        }
        Ok(entry.native)
    }
    pub(super) fn verify(&mut self, api: Api) -> Result<(), String> {
        if self.roots_released || self.retired {
            return Err("RuntimeType verification after root retirement".into());
        }
        for entry in self.entries.values() {
            if unsafe { (api.target)(entry.root) } != entry.native
                || unsafe { (api.get_type)(entry.native) } != entry.ty
            {
                return Err("RuntimeType changed identity across GC".into());
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
            return Err("reflection views retired before native roots".into());
        }
        for (token, entry) in &self.entries {
            if let Some(view) = &entry.view {
                memory.unmap_shared_data(token + 16)?;
                view.revoke()?;
            }
        }
        self.retired = true;
        Ok(())
    }
    pub(super) fn report(&self) -> String {
        format!("native-reflection-types={}\nnative-reflection-gc-verifications={}\nnative-reflection-roots-released={}\nnative-reflection-views-retired={}\n", self.entries.len(), self.verified, self.roots_released, self.retired)
    }
}
#[cfg(test)]
mod tests {
    use super::super::super::super::memory::{EXECUTE, WRITE};
    use super::*;
    use std::cell::Cell;
    thread_local! {static ROOTS: Cell<usize> = const {Cell::new(0)};}
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
    unsafe extern "C" fn class(_: Object) -> Object {
        0x55usize as Object
    }
    unsafe extern "C" fn corlib() -> Object {
        0x66usize as Object
    }
    unsafe extern "C" fn class_from_name(_: Object, _: *const c_char, _: *const c_char) -> Object {
        0x55usize as Object
    }
    unsafe extern "C" fn get_type(p: Object) -> Object {
        unsafe { p.cast::<u8>().add(16).cast::<Object>().read_unaligned() }
    }
    #[test]
    fn reflection_roots_publish_only_typed_metadata_cell_and_retire_without_leaking_native_addresses(
    ) {
        let api = Api {
            strong,
            target,
            free,
            class,
            corlib,
            class_from_name,
            get_type,
        };
        let mut raw = vec![0u8; 24];
        raw[16..24].copy_from_slice(&0x11223344u64.to_ne_bytes());
        let native = raw.as_mut_ptr().cast();
        let mut registry = Registry::default();
        let mut memory = Memory::default();
        assert_eq!(registry.root_type(api, ptr::null_mut()).unwrap().0, 0);
        let (token, ty) = registry.root_type(api, native).unwrap();
        assert_eq!(ty as u64, 0x11223344);
        registry
            .publish(&mut memory, token, 0x6ffe40000100)
            .unwrap();
        assert_eq!(memory.read_int(token + 16, 8).unwrap(), 0x6ffe40000100);
        assert!(memory.read_int(token, 8).is_err());
        assert!(memory.read_int(token + 24, 1).is_err());
        assert!(memory.check(token + 16, 8, WRITE).is_err());
        assert!(memory.check(token + 16, 8, EXECUTE).is_err());
        assert!(registry.native(api, token + 1).is_err());
        assert!(registry.native(api, native as u64).is_err());
        assert!(registry.publish(&mut memory, token, 0x11223344).is_err());
        registry.verify(api).unwrap();
        assert!(registry.retire_views(&mut memory).is_err());
        assert_eq!(ROOTS.with(Cell::get), 1);
        registry.release_roots(api);
        registry.release_roots(api);
        assert_eq!(ROOTS.with(Cell::get), 0);
        assert!(registry.native(api, token).is_err());
        registry.retire_views(&mut memory).unwrap();
        registry.retire_views(&mut memory).unwrap();
        assert!(memory.read_int(token + 16, 8).is_err());
        assert!(registry.root_type(api, native).is_err());
    }
}
