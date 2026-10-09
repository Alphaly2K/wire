//! External v2 strong GC handles. Guest tokens identify owned native handles;
//! they never expose a Mono handle pointer or accept arbitrary object addresses.
use super::Object;
use std::{collections::BTreeMap, ptr};
#[derive(Clone, Copy)]
pub(super) struct Api {
    new: unsafe extern "C" fn(Object, i32) -> Object,
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
}
#[derive(Default)]
pub(super) struct Registry {
    entries: BTreeMap<u64, Entry>,
    issued: usize,
    released: bool,
}
impl Registry {
    pub(super) fn new_handle(
        &mut self,
        api: Api,
        object: Object,
        token: u64,
        pinned: u64,
    ) -> Result<u64, String> {
        if self.released || self.issued >= 4096 || pinned > 1 || object.is_null() || token == 0 {
            return Err("external strong handle outside live owned object contract".into());
        }
        let native = unsafe { (api.new)(object, pinned as i32) };
        if native.is_null() {
            return Err("external native strong handle allocation failed".into());
        }
        if unsafe { (api.target)(native) } != object {
            unsafe {
                (api.free)(native);
            }
            return Err("external strong handle identity mismatch".into());
        }
        let handle = 0x6ffea0000000 + self.issued as u64 * 16;
        self.issued += 1;
        self.entries.insert(
            handle,
            Entry {
                native,
                object,
                token,
            },
        );
        Ok(handle)
    }
    pub(super) fn target(&self, api: Api, handle: u64) -> Result<u64, String> {
        if self.released {
            return Err("external handle registry released".into());
        }
        let entry = self
            .entries
            .get(&handle)
            .ok_or("unknown or freed external strong handle")?;
        if unsafe { (api.target)(entry.native) } != entry.object {
            return Err("external strong handle lost object identity".into());
        }
        Ok(entry.token)
    }
    pub(super) fn free(&mut self, api: Api, handle: u64) -> Result<(), String> {
        if self.released {
            return Err("external handle freed after registry release".into());
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
        self.entries.clear();
    }
    pub(super) fn verify(&self, api: Api) -> Result<(), String> {
        for &handle in self.entries.keys() {
            self.target(api, handle)?;
        }
        Ok(())
    }
    pub(super) fn report(&self) -> String {
        format!(
            "native-external-strong-handles={}\nnative-external-handles-released={}\n",
            self.issued, self.released
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
    fn external_handles_preserve_owned_identity_reject_forgery_and_release_once() {
        ROOTS.with(|r| r.set(0));
        let api = Api { new, target, free };
        let mut registry = Registry::default();
        let handle = registry
            .new_handle(api, 0x33usize as Object, 0x77, 1)
            .unwrap();
        assert_ne!(handle, 0x33);
        assert_eq!(registry.target(api, handle).unwrap(), 0x77);
        assert!(registry.target(api, 0x33).is_err());
        registry.free(api, handle).unwrap();
        assert!(registry.free(api, handle).is_err());
        let next = registry
            .new_handle(api, 0x33usize as Object, 0x77, 0)
            .unwrap();
        assert_ne!(next, handle);
        registry.verify(api).unwrap();
        registry.release(api);
        registry.release(api);
        assert_eq!(ROOTS.with(Cell::get), 0);
        assert!(registry.target(api, next).is_err());
        assert!(registry
            .new_handle(api, 0x33usize as Object, 0x77, 0)
            .is_err());
    }
}
