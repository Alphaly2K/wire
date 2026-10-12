//! External v2 strong and weak GC handles. Guest tokens identify owned native handles;
//! they never expose a Mono handle pointer or accept arbitrary object addresses.
use super::super::super::memory::{shared::HostRegion, READ};
use super::{Memory, Object};
use std::{collections::BTreeMap, ptr};
#[derive(Clone, Copy)]
pub(super) struct Api {
    new: unsafe extern "C" fn(Object, i32) -> Object,
    weak: unsafe extern "C" fn(Object, i32) -> Object,
    target: unsafe extern "C" fn(Object) -> Object,
    free: unsafe extern "C" fn(Object),
}
impl Api {
    pub(super) fn load(library: Object) -> Result<Self, String> {
        macro_rules! sym {
            ($name:literal,$ty:ty) => {{
                let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return Err(concat!("missing external root symbol: ", $name).into());
                }
                unsafe { std::mem::transmute::<Object, $ty>(p) }
            }};
        }
        Ok(Self {
            new: sym!(
                "mono_gchandle_new_v2",
                unsafe extern "C" fn(Object, i32) -> Object
            ),
            weak: sym!(
                "mono_gchandle_new_weakref_v2",
                unsafe extern "C" fn(Object, i32) -> Object
            ),
            target: sym!(
                "mono_gchandle_get_target_v2",
                unsafe extern "C" fn(Object) -> Object
            ),
            free: sym!("mono_gchandle_free_v2", unsafe extern "C" fn(Object)),
        })
    }
}
struct Entry {
    native: Object,
    object: Object,
    token: u64,
    view: HostRegion,
    published: bool,
    weak: bool,
}
#[derive(Default)]
pub(super) struct Registry {
    entries: BTreeMap<u64, Entry>,
    issued: usize,
    weak_issued: usize,
    released: bool,
    views_retired: bool,
}
impl Registry {
    /// Transfer an actual managed-created strong v2 handle to an engine owner.
    /// The native adapter resolves/roots its typed target before calling this;
    /// the guest never supplies the native handle address.
    pub(super) fn adopt_native(
        &mut self,
        api: Api,
        native: Object,
        object: Object,
        token: u64,
    ) -> Result<u64, String> {
        if self.released
            || self.views_retired
            || self.issued >= 4096
            || native.is_null()
            || object.is_null()
            || token == 0
            || native as usize & 1 != 0
            || self.entries.values().any(|entry| entry.native == native)
            || unsafe { (api.target)(native) } != object
        {
            return Err("native handle transfer requires a unique live strong target".into());
        }
        let view = HostRegion::owned(8)?;
        view.with_native(|dest| unsafe { dest.as_ptr().cast::<u64>().write_unaligned(token) })?;
        let handle = 0x6ffea0000000 + self.issued as u64 * 16;
        self.entries.insert(
            handle,
            Entry {
                native,
                object,
                token,
                view,
                published: false,
                weak: false,
            },
        );
        self.issued += 1;
        Ok(handle)
    }
    pub(super) fn new_handle(
        &mut self,
        api: Api,
        object: Object,
        token: u64,
        pinned: u64,
    ) -> Result<u64, String> {
        self.allocate(api, object, token, pinned, false)
    }
    pub(super) fn new_weak_handle(
        &mut self,
        api: Api,
        object: Object,
        token: u64,
        track_resurrection: u64,
    ) -> Result<u64, String> {
        self.allocate(api, object, token, track_resurrection, true)
    }
    fn allocate(
        &mut self,
        api: Api,
        object: Object,
        token: u64,
        flag: u64,
        weak: bool,
    ) -> Result<u64, String> {
        if self.released || self.issued >= 4096 || flag > 1 || object.is_null() || token == 0 {
            return Err("external handle outside live owned object contract".into());
        }
        let allocate = if weak { api.weak } else { api.new };
        let native = unsafe { allocate(object, flag as i32) };
        if native.is_null() {
            return Err("external native handle allocation failed".into());
        }
        if unsafe { (api.target)(native) } != object {
            unsafe {
                (api.free)(native);
            }
            return Err("external handle identity mismatch".into());
        }
        let view = match HostRegion::owned(8) {
            Ok(view) => view,
            Err(error) => {
                unsafe { (api.free)(native) };
                return Err(error);
            }
        };
        if let Err(error) =
            view.with_native(|dest| unsafe { dest.as_ptr().cast::<u64>().write_unaligned(token) })
        {
            unsafe { (api.free)(native) };
            return Err(error);
        }
        // The frozen BDWGC v2 ABI tags weak handles with bit zero. Unity
        // routes these through get_target instead of direct strong-cell reads.
        let handle = (0x6ffea0000000 + self.issued as u64 * 16) | u64::from(weak);
        self.issued += 1;
        self.weak_issued += usize::from(weak);
        self.entries.insert(
            handle,
            Entry {
                native,
                object,
                token,
                view,
                published: false,
                weak,
            },
        );
        Ok(handle)
    }
    pub(super) fn publish(&mut self, memory: &mut Memory, handle: u64) -> Result<(), String> {
        if self.released || self.views_retired {
            return Err("external handle publication after release".into());
        }
        let entry = self
            .entries
            .get_mut(&handle)
            .ok_or("unknown external handle view")?;
        // Weak slots can clear during native GC. They are opaque: a stale
        // readable snapshot must never bypass the native weak-link query.
        if !entry.weak && !entry.published {
            memory.map_shared_data(handle, entry.view.clone(), READ, READ)?;
            entry.published = true;
        }
        Ok(())
    }
    pub(super) fn retire_view(&mut self, memory: &mut Memory, handle: u64) -> Result<(), String> {
        let entry = self
            .entries
            .get_mut(&handle)
            .ok_or("unknown external handle view retirement")?;
        if entry.published {
            memory.unmap_shared_data(handle)?;
            entry.published = false;
        }
        entry.view.revoke()?;
        Ok(())
    }
    pub(super) fn retire_views(&mut self, memory: &mut Memory) -> Result<(), String> {
        if self.views_retired {
            return Ok(());
        }
        if !self.released && !self.entries.is_empty() {
            return Err("external handle views retired before roots".into());
        }
        for (&handle, entry) in &mut self.entries {
            if entry.published {
                memory.unmap_shared_data(handle)?;
                entry.published = false;
            }
            entry.view.revoke()?;
        }
        self.views_retired = true;
        Ok(())
    }
    pub(super) fn target(&self, api: Api, handle: u64) -> Result<u64, String> {
        if self.released {
            return Err("external handle registry released".into());
        }
        let entry = self
            .entries
            .get(&handle)
            .ok_or("unknown or freed external handle")?;
        let target = unsafe { (api.target)(entry.native) };
        if entry.weak && target.is_null() {
            return Ok(0);
        }
        if target != entry.object {
            return Err("external handle changed object identity".into());
        }
        Ok(entry.token)
    }
    /// Native managed unmarshalling consumes the actual v2 handle. This pointer
    /// is never written to guest memory or returned through a guest gateway.
    pub(super) fn native_handle(&self, api: Api, handle: u64) -> Result<Object, String> {
        if self.views_retired || self.target(api, handle)? == 0 {
            return Err("native handle conversion requires a live owned target".into());
        }
        Ok(self.entries[&handle].native)
    }
    pub(super) fn free(&mut self, api: Api, handle: u64) -> Result<(), String> {
        if self.released {
            return Err("external handle freed after registry release".into());
        }
        if self
            .entries
            .get(&handle)
            .is_some_and(|entry| entry.published)
        {
            return Err("external handle must revoke its object-token cell before free".into());
        }
        let entry = self
            .entries
            .remove(&handle)
            .ok_or("unknown or double-freed external strong handle")?;
        unsafe {
            (api.free)(entry.native);
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
                (api.free)(entry.native);
            }
            entry.native = ptr::null_mut();
        }
    }
    pub(super) fn verify(&self, api: Api) -> Result<(), String> {
        for (&handle, entry) in &self.entries {
            self.target(api, handle)?;
            let cell = entry.view.with_native(|pointer| unsafe {
                pointer.as_ptr().cast::<u64>().read_unaligned()
            })?;
            if cell != entry.token {
                return Err("external handle object-token cell changed".into());
            }
        }
        Ok(())
    }
    pub(super) fn report(&self) -> String {
        format!(
            "native-external-strong-handles={}\nnative-external-weak-handles={}\nnative-external-handles-released={}\nnative-external-handle-views-retired={}\n",
            self.issued - self.weak_issued, self.weak_issued, self.released, self.views_retired
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    thread_local! { static ROOTS:Cell<usize> = const { Cell::new(0) }; }
    unsafe extern "C" fn new(p: Object, _: i32) -> Object {
        ROOTS.with(|r| r.set(r.get() + 1));
        p
    }
    unsafe extern "C" fn target(p: Object) -> Object {
        p
    }
    unsafe extern "C" fn free(_: Object) {
        ROOTS.with(|r| r.set(r.get() - 1));
    }
    #[test]
    fn transferred_native_handle_is_freed_once_by_its_guest_owner() {
        ROOTS.with(|r| r.set(0));
        let api = Api {
            new,
            weak: new,
            target,
            free,
        };
        let object = 0x44usize as Object;
        let native = unsafe { new(object, 0) };
        let mut registry = Registry::default();
        let guest = registry.adopt_native(api, native, object, 0x77).unwrap();
        assert_ne!(guest, native as u64);
        assert!(registry.adopt_native(api, native, object, 0x77).is_err());
        assert!(registry
            .adopt_native(api, 0x45usize as Object, object, 0x77)
            .is_err());
        assert_eq!(ROOTS.with(Cell::get), 1);
        let mut memory = Memory::default();
        registry.publish(&mut memory, guest).unwrap();
        assert_eq!(memory.read_int(guest, 8).unwrap(), 0x77);
        assert!(registry.free(api, guest).is_err());
        registry.retire_view(&mut memory, guest).unwrap();
        registry.free(api, guest).unwrap();
        registry.release(api);
        assert_eq!(ROOTS.with(Cell::get), 0);
        assert!(registry.native_handle(api, guest).is_err());
    }
    #[test]
    fn external_handles_preserve_owned_identity_reject_forgery_and_release_once() {
        ROOTS.with(|r| r.set(0));
        let api = Api {
            new,
            weak: new,
            target,
            free,
        };
        let mut registry = Registry::default();
        let handle = registry
            .new_handle(api, 0x33usize as Object, 0x77, 1)
            .unwrap();
        assert_ne!(handle, 0x33);
        let mut memory = Memory::default();
        registry.publish(&mut memory, handle).unwrap();
        assert_eq!(memory.read_int(handle, 8).unwrap(), 0x77);
        assert!(memory.read_int(handle + 8, 1).is_err());
        assert!(memory.write_int(handle, 8, 0).is_err());
        assert!(registry.free(api, handle).is_err());
        assert_eq!(registry.target(api, handle).unwrap(), 0x77);
        assert_eq!(
            registry.native_handle(api, handle).unwrap(),
            0x33usize as Object
        );
        assert!(registry.native_handle(api, 0x33).is_err());
        assert!(registry.target(api, 0x33).is_err());
        registry.retire_view(&mut memory, handle).unwrap();
        registry.free(api, handle).unwrap();
        assert!(memory.read_int(handle, 8).is_err());
        assert!(registry.free(api, handle).is_err());
        assert!(registry.native_handle(api, handle).is_err());
        let next = registry
            .new_handle(api, 0x33usize as Object, 0x77, 0)
            .unwrap();
        assert_ne!(next, handle);
        registry.publish(&mut memory, next).unwrap();
        registry.verify(api).unwrap();
        registry.release(api);
        registry.retire_views(&mut memory).unwrap();
        registry.release(api);
        registry.retire_views(&mut memory).unwrap();
        assert!(memory.read_int(next, 8).is_err());
        assert_eq!(ROOTS.with(Cell::get), 0);
        assert!(registry.target(api, next).is_err());
        assert!(registry.native_handle(api, next).is_err());
        assert!(registry
            .new_handle(api, 0x33usize as Object, 0x77, 0)
            .is_err());
    }
    #[test]
    fn weak_handles_preserve_tag_query_real_target_and_never_expose_stale_cells() {
        thread_local! { static WEAK_TARGET: Cell<usize> = const { Cell::new(0x33) }; }
        unsafe extern "C" fn weak_target(_: Object) -> Object {
            WEAK_TARGET.with(Cell::get) as Object
        }
        ROOTS.with(|r| r.set(0));
        WEAK_TARGET.with(|r| r.set(0x33));
        let api = Api {
            new,
            weak: new,
            target: weak_target,
            free,
        };
        let mut registry = Registry::default();
        let mut memory = Memory::default();
        let handle = registry
            .new_weak_handle(api, 0x33usize as Object, 0x77, 1)
            .unwrap();
        assert_eq!(handle & 1, 1);
        registry.publish(&mut memory, handle).unwrap();
        assert!(memory.read_int(handle & !1, 8).is_err());
        assert_eq!(registry.target(api, handle).unwrap(), 0x77);
        assert!(registry.target(api, handle & !1).is_err());
        WEAK_TARGET.with(|r| r.set(0));
        assert_eq!(registry.target(api, handle).unwrap(), 0);
        assert!(registry.native_handle(api, handle).is_err());
        registry.verify(api).unwrap();
        WEAK_TARGET.with(|r| r.set(0x44));
        assert!(registry.target(api, handle).is_err());
        WEAK_TARGET.with(|r| r.set(0));
        registry.retire_view(&mut memory, handle).unwrap();
        registry.free(api, handle).unwrap();
        registry.release(api);
        registry.retire_views(&mut memory).unwrap();
        assert_eq!(ROOTS.with(Cell::get), 0);
        assert!(registry.target(api, handle).is_err());
        assert!(registry
            .new_weak_handle(api, 0x33usize as Object, 0x77, 2)
            .is_err());
    }
}
