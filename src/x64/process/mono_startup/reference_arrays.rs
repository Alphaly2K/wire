//! Bounded SZ arrays of the two locked InputSystem reference classes. Guest
//! elements contain rooted tokens; native elements only contain Mono references.
use super::{c_char, instances, Memory, Object};
use crate::x64::memory::{shared::HostRegion, READ, WRITE};
use std::{collections::BTreeMap, ptr};
const BASE: u64 = 0x6ffeb0000000;
const STRIDE: u64 = 65536;
#[derive(Clone, Copy)]
pub(super) struct Api {
    new: unsafe extern "C" fn(Object, Object, usize) -> Object,
    class: unsafe extern "C" fn(Object) -> Object,
    array_class: unsafe extern "C" fn(Object, u32) -> Object,
    length: unsafe extern "C" fn(Object) -> usize,
    element_size: unsafe extern "C" fn(Object) -> i32,
    address: unsafe extern "C" fn(Object, i32, usize) -> Object,
    barrier: unsafe extern "C" fn(Object, Object, Object),
    map: Object,
    action: Object,
}
impl Api {
    pub(super) fn load(library: Object, image: Object) -> Result<Self, String> {
        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return Err(concat!("missing reference array API: ", $name).into());
                }
                unsafe { std::mem::transmute::<Object, $ty>(p) }
            }};
        }
        let find = sym!(
            "mono_class_from_name",
            unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object
        );
        let map = unsafe {
            find(
                image,
                c"UnityEngine.InputSystem".as_ptr(),
                c"InputActionMap".as_ptr(),
            )
        };
        let action = unsafe {
            find(
                image,
                c"UnityEngine.InputSystem".as_ptr(),
                c"InputAction".as_ptr(),
            )
        };
        let value_type = sym!(
            "mono_class_is_valuetype",
            unsafe extern "C" fn(Object) -> i32
        );
        if map.is_null()
            || action.is_null()
            || unsafe { value_type(map) } != 0
            || unsafe { value_type(action) } != 0
        {
            return Err("locked reference element classes absent/value types".into());
        }
        Ok(Self {
            new: sym!(
                "mono_array_new",
                unsafe extern "C" fn(Object, Object, usize) -> Object
            ),
            class: sym!(
                "mono_object_get_class",
                unsafe extern "C" fn(Object) -> Object
            ),
            array_class: sym!(
                "mono_array_class_get",
                unsafe extern "C" fn(Object, u32) -> Object
            ),
            length: sym!("mono_array_length", unsafe extern "C" fn(Object) -> usize),
            element_size: sym!(
                "mono_array_element_size",
                unsafe extern "C" fn(Object) -> i32
            ),
            address: sym!(
                "mono_array_addr_with_size",
                unsafe extern "C" fn(Object, i32, usize) -> Object
            ),
            barrier: sym!(
                "mono_gc_wbarrier_set_arrayref",
                unsafe extern "C" fn(Object, Object, Object)
            ),
            map,
            action,
        })
    }
    pub(super) fn supports(self, element: Object) -> bool {
        !element.is_null() && [self.map, self.action].contains(&element)
    }
}
struct Entry {
    native: Object,
    class: Object,
    element: Object,
    length: usize,
    header: HostRegion,
    payload: HostRegion,
    committed: Vec<u64>,
    published: bool,
}
#[derive(Default)]
pub(super) struct Registry {
    entries: BTreeMap<u64, Entry>,
    retired: bool,
    writes: usize,
    verified: usize,
}
impl Registry {
    fn layout(api: Api, entry: &Entry) -> Result<(), String> {
        let native = entry.native;
        if native.is_null()
            || unsafe { (api.class)(native) } != entry.class
            || unsafe { (api.array_class)(entry.element, 1) } != entry.class
            || unsafe { (api.element_size)(entry.class) } != 8
            || unsafe { (api.length)(native) } != entry.length
            || !unsafe {
                native
                    .cast::<u8>()
                    .add(16)
                    .cast::<Object>()
                    .read_unaligned()
            }
            .is_null()
            || unsafe { native.cast::<u8>().add(24).cast::<usize>().read_unaligned() }
                != entry.length
            || unsafe { (api.address)(native, 8, 0) }
                != unsafe { native.cast::<u8>().add(32).cast() }
        {
            return Err("reference SZ array identity/layout mismatch".into());
        }
        Ok(())
    }
    pub(super) fn allocate(
        &mut self,
        api: Api,
        root_api: instances::Api,
        roots: &mut instances::Registry,
        domain: Object,
        element: Object,
        length: u64,
    ) -> Result<u64, String> {
        if self.retired
            || self.entries.len() >= 256
            || domain.is_null()
            || !api.supports(element)
            || !(1..=4096).contains(&length)
        {
            return Err("reference array outside locked bounded allocation contract".into());
        }
        let length = length as usize;
        let native = unsafe { (api.new)(domain, element, length) };
        let token = BASE + self.entries.len() as u64 * STRIDE;
        roots.adopt_reference_array(root_api, token, native)?;
        let class = unsafe { (api.array_class)(element, 1) };
        let header = HostRegion::owned(16)?;
        header.with_native(|p| unsafe {
            p.as_ptr()
                .add(8)
                .cast::<u64>()
                .write_unaligned(length as u64);
        })?;
        let entry = Entry {
            native,
            class,
            element,
            length,
            header,
            payload: HostRegion::owned(length * 8)?,
            committed: vec![0; length],
            published: false,
        };
        Self::layout(api, &entry)?;
        for index in 0..length {
            if !unsafe {
                (api.address)(native, 8, index)
                    .cast::<Object>()
                    .read_unaligned()
            }
            .is_null()
            {
                return Err("new reference array has non-NULL element".into());
            }
        }
        self.entries.insert(token, entry);
        Ok(token)
    }
    pub(super) fn publish(&mut self, memory: &mut Memory, token: u64) -> Result<(), String> {
        if self.retired {
            return Err("reference array publication after retirement".into());
        }
        let entry = self
            .entries
            .get_mut(&token)
            .ok_or("unknown reference array")?;
        if !entry.published {
            memory.map_shared_data(token + 16, entry.header.clone(), READ, READ)?;
            if let Err(e) = memory.map_shared_data(
                token + 32,
                entry.payload.clone(),
                READ | WRITE,
                READ | WRITE,
            ) {
                memory.unmap_shared_data(token + 16)?;
                return Err(e);
            }
            entry.published = true;
        }
        Ok(())
    }
    fn values(entry: &Entry) -> Result<Vec<u64>, String> {
        entry.payload.with_native(|p| {
            (0..entry.length)
                .map(|i| unsafe { p.as_ptr().add(i * 8).cast::<u64>().read_unaligned() })
                .collect()
        })
    }
    fn slot(&self, owner: u64, address: u64) -> Option<(u64, usize)> {
        if self.retired {
            return None;
        }
        let (&token, entry) = self.entries.range(..=address).next_back()?;
        let offset = address.checked_sub(token + 32)?;
        ((owner == 0 || owner == token)
            && entry.published
            && offset.is_multiple_of(8)
            && offset / 8 < entry.length as u64)
            .then_some((token, (offset / 8) as usize))
    }
    pub(super) fn has_reference(&self, owner: u64, address: u64) -> bool {
        self.slot(owner, address).is_some()
    }
    pub(super) fn address(&self, token: u64, size: u64, index: u64) -> Result<u64, String> {
        let entry = self
            .entries
            .get(&token)
            .ok_or("unknown reference array address")?;
        if self.retired || !entry.published || size != 8 || index >= entry.length as u64 {
            return Err("reference array address size/index/retirement mismatch".into());
        }
        Ok(token + 32 + index * 8)
    }
    pub(super) fn sync(
        &mut self,
        api: Api,
        root_api: instances::Api,
        roots: &instances::Registry,
    ) -> Result<(), String> {
        if self.retired {
            return Err("reference array sync after retirement".into());
        }
        let mut updates = Vec::new();
        for (&token, entry) in &self.entries {
            if roots.native(root_api, token)? != entry.native {
                return Err("reference array root changed".into());
            }
            Self::layout(api, entry)?;
            for (index, value) in Self::values(entry)?.into_iter().enumerate() {
                let native = if value == 0 {
                    ptr::null_mut()
                } else {
                    roots.compatible(root_api, value, entry.element)?
                };
                if value != entry.committed[index] {
                    updates.push((token, index, value, native));
                }
            }
        }
        // Validate all arrays and every token before changing any native slot.
        for (token, index, value, native) in updates {
            let entry = self.entries.get_mut(&token).unwrap();
            unsafe {
                (api.barrier)(entry.native, (api.address)(entry.native, 8, index), native);
            }
            entry.committed[index] = value;
            self.writes += 1;
        }
        Ok(())
    }
    pub(super) fn store(
        &mut self,
        api: Api,
        root_api: instances::Api,
        roots: &instances::Registry,
        memory: &mut Memory,
        args: [u64; 3],
    ) -> Result<(), String> {
        let [owner, address, value] = args;
        let (token, index) = self
            .slot(owner, address)
            .ok_or("barrier destination is not an owned reference array slot")?;
        let entry = &self.entries[&token];
        let native = if value == 0 {
            ptr::null_mut()
        } else {
            roots.compatible(root_api, value, entry.element)?
        };
        let changed = value != entry.committed[index];
        memory.write_int(address, 8, value)?;
        self.sync(api, root_api, roots)?;
        if !changed {
            let entry = &self.entries[&token];
            unsafe {
                (api.barrier)(entry.native, (api.address)(entry.native, 8, index), native);
            }
            self.writes += 1;
        }
        Ok(())
    }
    pub(super) fn verify(
        &mut self,
        api: Api,
        root_api: instances::Api,
        roots: &instances::Registry,
    ) -> Result<(), String> {
        if self.retired {
            return Err("reference array verification after retirement".into());
        }
        for (&token, entry) in &self.entries {
            if roots.native(root_api, token)? != entry.native {
                return Err("reference array root lost across GC".into());
            }
            Self::layout(api, entry)?;
            if Self::values(entry)? != entry.committed {
                return Err("reference array guest values not synchronized before GC".into());
            }
            for (index, &value) in entry.committed.iter().enumerate() {
                let expected = if value == 0 {
                    ptr::null_mut()
                } else {
                    roots.compatible(root_api, value, entry.element)?
                };
                if unsafe {
                    (api.address)(entry.native, 8, index)
                        .cast::<Object>()
                        .read_unaligned()
                } != expected
                {
                    return Err("native array reference differs across GC".into());
                }
            }
            self.verified += 1;
        }
        Ok(())
    }
    pub(super) fn retire(
        &mut self,
        memory: &mut Memory,
        roots: &instances::Registry,
    ) -> Result<(), String> {
        if self.retired {
            return Ok(());
        }
        if !roots.roots_released() && !self.entries.is_empty() {
            return Err("reference array views retired before roots".into());
        }
        for (&token, entry) in &self.entries {
            if entry.published {
                memory.unmap_shared_data(token + 16)?;
                memory.unmap_shared_data(token + 32)?;
            }
            entry.header.revoke()?;
            entry.payload.revoke()?;
        }
        self.retired = true;
        Ok(())
    }
    pub(super) fn report(&self) -> String {
        format!("native-reference-arrays={}\nnative-reference-array-writes={}\nnative-reference-array-gc-verifications={}\nnative-reference-array-views-retired={}\n",self.entries.len(),self.writes,self.verified,self.retired)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    thread_local! {
        static RAW: RefCell<Vec<Box<[usize]>>> = const { RefCell::new(Vec::new()) };
        static WRITES: Cell<usize> = const { Cell::new(0) };
    }
    unsafe extern "C" fn new(_: Object, _: Object, length: usize) -> Object {
        RAW.with(|raw| {
            let mut data = vec![0; 4 + length].into_boxed_slice();
            data[3] = length;
            let p = data.as_mut_ptr().cast();
            raw.borrow_mut().push(data);
            p
        })
    }
    unsafe extern "C" fn class(_: Object) -> Object {
        2usize as Object
    }
    unsafe extern "C" fn array_class(_: Object, _: u32) -> Object {
        2usize as Object
    }
    unsafe extern "C" fn length(p: Object) -> usize {
        unsafe { p.cast::<usize>().add(3).read() }
    }
    unsafe extern "C" fn size(_: Object) -> i32 {
        8
    }
    unsafe extern "C" fn address(p: Object, _: i32, index: usize) -> Object {
        unsafe { p.cast::<usize>().add(4 + index).cast() }
    }
    unsafe extern "C" fn barrier(_: Object, p: Object, value: Object) {
        unsafe {
            p.cast::<Object>().write(value);
        }
        WRITES.with(|c| c.set(c.get() + 1));
    }
    fn api() -> Api {
        Api {
            new,
            class,
            array_class,
            length,
            element_size: size,
            address,
            barrier,
            map: 2usize as Object,
            action: 3usize as Object,
        }
    }
    #[test]
    fn reference_arrays_reject_late_forgery_before_any_native_store_and_keep_headers_private() {
        WRITES.with(|c| c.set(0));
        let api = api();
        let root_api = instances::tests::identity_api();
        let mut roots = instances::Registry::default();
        let mut registry = Registry::default();
        let mut memory = Memory::default();
        let value = roots.borrow(root_api, 4usize as Object).unwrap();
        let a = registry
            .allocate(
                api,
                root_api,
                &mut roots,
                1usize as Object,
                2usize as Object,
                2,
            )
            .unwrap();
        let b = registry
            .allocate(
                api,
                root_api,
                &mut roots,
                1usize as Object,
                2usize as Object,
                1,
            )
            .unwrap();
        registry.publish(&mut memory, a).unwrap();
        registry.publish(&mut memory, b).unwrap();
        assert!(memory.read_int(a, 8).is_err());
        assert!(memory.write_int(a + 24, 8, 7).is_err());
        assert_eq!(memory.read_int(a + 24, 8).unwrap(), 2);
        assert!(memory.read_int(a + 48, 8).is_err());
        assert!(registry.address(a, 4, 0).is_err());
        assert!(registry.address(a, 8, 2).is_err());
        assert!(registry
            .store(api, root_api, &roots, &mut memory, [0, a + 33, value])
            .is_err());
        memory.write_int(a + 32, 8, value).unwrap();
        memory.write_int(b + 32, 8, 0x1234).unwrap();
        assert!(registry.sync(api, root_api, &roots).is_err());
        assert_eq!(WRITES.with(Cell::get), 0);
        memory.write_int(b + 32, 8, 0).unwrap();
        registry.sync(api, root_api, &roots).unwrap();
        assert_eq!(WRITES.with(Cell::get), 1);
        registry
            .store(api, root_api, &roots, &mut memory, [a, a + 32, value])
            .unwrap();
        assert_eq!(WRITES.with(Cell::get), 2);
        registry.verify(api, root_api, &roots).unwrap();
        assert!(registry.retire(&mut memory, &roots).is_err());
        roots.release(root_api);
        registry.retire(&mut memory, &roots).unwrap();
        assert!(memory.read_int(a + 32, 8).is_err());
        assert!(registry.address(a, 8, 0).is_err());
    }
}
