//! Checked ordinary object allocation. Mono returns an already strong-rooted
//! value; the guest gets an opaque token with no native header or field alias.
use super::super::super::memory::{shared::HostRegion, READ, WRITE};
use super::{c_char, Memory, Object};
use std::{collections::BTreeMap, ptr};
const BASE: u64 = 0x6fff00000000;
const LIMIT: usize = 32768;
#[derive(Clone, Copy)]
pub(super) struct Api {
    strong: unsafe extern "C" fn(Object, i32) -> Object,
    new: unsafe extern "C" fn(Object, Object, *mut u16) -> Object,
    value_box: unsafe extern "C" fn(Object, Object, Object) -> Object,
    target: unsafe extern "C" fn(Object) -> Object,
    free: unsafe extern "C" fn(Object),
    class: unsafe extern "C" fn(Object) -> Object,
    assignable: unsafe extern "C" fn(Object, Object) -> i32,
    field_get: unsafe extern "C" fn(Object, Object, Object),
    field_set: unsafe extern "C" fn(Object, Object, Object),
    unity_base: Object,
    cached_field: Object,
}
impl Api {
    pub(super) fn load(library: Object, unity_image: Object) -> Result<Self, String> {
        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                let pointer = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
                if pointer.is_null() {
                    return Err(concat!("missing rooted allocation symbol: ", $name).into());
                }
                unsafe { std::mem::transmute::<Object, $ty>(pointer) }
            }};
        }
        let find = sym!(
            "mono_class_from_name",
            unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object
        );
        let field = sym!(
            "mono_class_get_field_from_name",
            unsafe extern "C" fn(Object, *const c_char) -> Object
        );
        let offset = sym!("mono_field_get_offset", unsafe extern "C" fn(Object) -> i32);
        let flags = sym!("mono_field_get_flags", unsafe extern "C" fn(Object) -> u32);
        let ty = sym!(
            "mono_field_get_type",
            unsafe extern "C" fn(Object) -> Object
        );
        let kind = sym!("mono_type_get_type", unsafe extern "C" fn(Object) -> u32);
        let byref = sym!("mono_type_is_byref", unsafe extern "C" fn(Object) -> u32);
        let unity_base = unsafe { find(unity_image, c"UnityEngine".as_ptr(), c"Object".as_ptr()) };
        if unity_base.is_null() {
            return Err("locked UnityEngine.Object class absent".into());
        }
        let cached_field = unsafe { field(unity_base, c"m_CachedPtr".as_ptr()) };
        if cached_field.is_null()
            || unsafe { offset(cached_field) } != 16
            || unsafe { flags(cached_field) } & (0x10 | 0x40) != 0
        {
            return Err("Unity cached pointer field offset/instance contract mismatch".into());
        }
        let cached_type = unsafe { ty(cached_field) };
        if cached_type.is_null()
            || unsafe { kind(cached_type) } != 0x18
            || unsafe { byref(cached_type) } != 0
        {
            return Err("Unity cached pointer is not a scalar native int".into());
        }
        Ok(Self {
            strong: sym!(
                "mono_gchandle_new_v2",
                unsafe extern "C" fn(Object, i32) -> Object
            ),
            new: sym!(
                "wire_mono_object_new_rooted_checked",
                unsafe extern "C" fn(Object, Object, *mut u16) -> Object
            ),
            value_box: sym!(
                "mono_value_box",
                unsafe extern "C" fn(Object, Object, Object) -> Object
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
            assignable: sym!(
                "mono_class_is_assignable_from",
                unsafe extern "C" fn(Object, Object) -> i32
            ),
            field_get: sym!(
                "mono_field_get_value",
                unsafe extern "C" fn(Object, Object, Object)
            ),
            field_set: sym!(
                "mono_field_set_value",
                unsafe extern "C" fn(Object, Object, Object)
            ),
            unity_base,
            cached_field,
        })
    }
}
struct Root {
    handle: Object,
    native: Object,
    class: Object,
    borrowed: bool,
    cached: Option<HostRegion>,
    published: bool,
}
/// Private pinned receiver ownership for synchronous actual-worker requests.
/// The caller must first resolve an existing canonical registry root.
pub(super) struct WorkerRoot {
    api: Api,
    handle: Object,
}
unsafe impl Send for WorkerRoot {}
impl Drop for WorkerRoot {
    fn drop(&mut self) {
        unsafe { (self.api.free)(self.handle) };
    }
}
impl WorkerRoot {
    pub(super) fn target(&self) -> Result<Object, String> {
        let object = unsafe { (self.api.target)(self.handle) };
        if object.is_null() {
            return Err("worker receiver root lost its target".into());
        }
        Ok(object)
    }
}
impl Api {
    pub(super) fn worker_root(self, object: Object) -> Result<WorkerRoot, String> {
        if object.is_null() {
            return Err("worker receiver cannot be NULL".into());
        }
        let handle = unsafe { (self.strong)(object, 1) };
        if handle.is_null() {
            return Err("worker receiver pin allocation failed".into());
        }
        let root = WorkerRoot { api: self, handle };
        if root.target()? != object {
            return Err("worker receiver pin identity mismatch".into());
        }
        Ok(root)
    }
}
/// Private nonmoving metadata capabilities and typed allocation APIs only.
/// The exclusively borrowed owner registry retains the image/domain lifetime.
pub(super) struct WorkerRequest {
    api: Api,
    domain: Object,
    class: Object,
    boxed: Option<usize>,
}
unsafe impl Send for WorkerRequest {}
/// The actual v2 strong root survives the worker wait and ownership transfer.
/// BDWGC does not move the object; no naked managed pointer crosses the queue.
pub(super) struct WorkerAllocation {
    api: Api,
    handle: Object,
    class: Object,
    boxed: bool,
}
unsafe impl Send for WorkerAllocation {}
impl Drop for WorkerAllocation {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { (self.api.free)(self.handle) };
        }
    }
}
impl WorkerRequest {
    pub(super) fn execute(self) -> Result<WorkerAllocation, String> {
        let mut status = 65535;
        let handle = if let Some(size) = self.boxed {
            let mut zero = [0u64; 32];
            if !(1..=256).contains(&size) {
                return Err("worker boxed layout exceeds bounded stack value".into());
            }
            let object =
                unsafe { (self.api.value_box)(self.domain, self.class, zero.as_mut_ptr().cast()) };
            if object.is_null() {
                return Err("worker zero boxing returned NULL".into());
            }
            status = 0;
            unsafe { (self.api.strong)(object, 1) }
        } else {
            unsafe { (self.api.new)(self.domain, self.class, &mut status) }
        };
        let root = WorkerAllocation {
            api: self.api,
            handle,
            class: self.class,
            boxed: self.boxed.is_some(),
        };
        if status != 0 {
            return Err(format!("checked native object allocation error={status}"));
        }
        if handle.is_null() {
            return Err("checked allocation returned no strong root".into());
        }
        Ok(root)
    }
}
pub(super) struct WorkerBorrow<'a> {
    registry: &'a mut Registry,
    request: WorkerRequest,
}
impl WorkerBorrow<'_> {
    pub(super) fn run(
        self,
        execute: impl FnOnce(WorkerRequest) -> Result<WorkerAllocation, String>,
    ) -> Result<u64, String> {
        let Self { registry, request } = self;
        registry.adopt_allocation(execute(request)?)
    }
}
#[derive(Default)]
pub(super) struct Registry {
    roots: BTreeMap<u64, Root>,
    reverse: BTreeMap<usize, u64>,
    verified: usize,
    boxed: std::collections::BTreeSet<u64>,
    borrowed_verified: usize,
    released: bool,
    views_retired: bool,
    synchronized: usize,
}
impl Registry {
    pub(super) fn count(&self) -> usize {
        self.roots.len()
    }
    pub(super) fn contains(&self, token: u64) -> bool {
        !self.released && !self.views_retired && self.roots.contains_key(&token)
    }
    pub(super) fn allocate(
        &mut self,
        api: Api,
        domain: Object,
        class: Object,
    ) -> Result<u64, String> {
        if self.released || self.roots.len() >= LIMIT || domain.is_null() || class.is_null() {
            return Err("ordinary allocation outside live bounded domain/class contract".into());
        }
        self.adopt_allocation(
            WorkerRequest {
                api,
                domain,
                class,
                boxed: None,
            }
            .execute()?,
        )
    }
    pub(super) fn prepare_worker(
        &mut self,
        api: Api,
        domain: Object,
        class: Object,
        boxed: Option<usize>,
    ) -> Result<WorkerBorrow<'_>, String> {
        if self.released
            || self.views_retired
            || self.roots.len() >= LIMIT
            || domain.is_null()
            || class.is_null()
            || boxed.is_some_and(|size| !(1..=256).contains(&size))
        {
            return Err("worker allocation outside live bounded domain/class/layout".into());
        }
        Ok(WorkerBorrow {
            registry: self,
            request: WorkerRequest {
                api,
                domain,
                class,
                boxed,
            },
        })
    }
    fn adopt_allocation(&mut self, mut root: WorkerAllocation) -> Result<u64, String> {
        let WorkerAllocation {
            api,
            handle,
            class,
            boxed,
        } = root;
        let result = (|| {
            if self.released || self.views_retired || self.roots.len() >= LIMIT || handle.is_null()
            {
                return Err("checked allocation returned no strong root".into());
            }
            let native = unsafe { (api.target)(handle) };
            if native.is_null() || unsafe { (api.class)(native) } != class {
                return Err("allocated object root/class mismatch".into());
            }
            if self.reverse.contains_key(&(native as usize)) {
                return Err("ordinary allocation reused a live object identity".into());
            }
            let cached = if !boxed && unsafe { (api.assignable)(api.unity_base, class) } != 0 {
                let mut initial = 0u64;
                unsafe {
                    (api.field_get)(native, api.cached_field, (&mut initial as *mut u64).cast());
                }
                if initial != 0 {
                    return Err(
                        "new Unity object already has an unknown cached native pointer".into(),
                    );
                }
                Some(HostRegion::owned(8)?)
            } else {
                None
            };
            let token = BASE + self.roots.len() as u64 * 65536;
            self.roots.insert(
                token,
                Root {
                    handle,
                    native,
                    class,
                    borrowed: false,
                    cached,
                    published: false,
                },
            );
            self.reverse.insert(native as usize, token);
            if boxed {
                self.boxed.insert(token);
            }
            Ok(token)
        })();
        if result.is_ok() {
            root.handle = ptr::null_mut();
        }
        result
    }
    pub(super) fn is_boxed(&self, token: u64) -> bool {
        self.contains(token) && self.boxed.contains(&token)
    }
    /// The caller must validate the exact locked value class and its layout.
    /// Copy a zero value with NULL references, pin immediately, then publish
    /// field tokens only. No unboxed native address leaves this registry.
    pub(super) fn allocate_boxed(
        &mut self,
        api: Api,
        domain: Object,
        class: Object,
        size: usize,
    ) -> Result<u64, String> {
        if self.released
            || self.views_retired
            || self.roots.len() >= LIMIT
            || domain.is_null()
            || class.is_null()
            || size == 0
            || size > 256
        {
            return Err("boxed zero allocation outside live bounded layout".into());
        }
        let mut zero = [0u64; 32];
        let native = unsafe { (api.value_box)(domain, class, zero.as_mut_ptr().cast()) };
        if native.is_null() {
            return Err("native zero value boxing returned NULL".into());
        }
        let handle = unsafe { (api.strong)(native, 1) };
        let result = (|| {
            if handle.is_null()
                || unsafe { (api.target)(handle) } != native
                || unsafe { (api.class)(native) } != class
                || self.reverse.contains_key(&(native as usize))
            {
                return Err("boxed zero root/class/identity mismatch".into());
            }
            let token = BASE + self.roots.len() as u64 * 65536;
            self.roots.insert(
                token,
                Root {
                    handle,
                    native,
                    class,
                    borrowed: false,
                    cached: None,
                    published: false,
                },
            );
            self.reverse.insert(native as usize, token);
            self.boxed.insert(token);
            Ok(token)
        })();
        if result.is_err() && !handle.is_null() {
            unsafe {
                (api.free)(handle);
            }
        }
        result
    }
    /// Called only for a native reference just read from a still-rooted graph.
    /// Pin before publishing an identity, never accept arbitrary guest pointers.
    pub(super) fn borrow(&mut self, api: Api, native: Object) -> Result<u64, String> {
        if self.released || self.views_retired || native.is_null() {
            return Err("borrowed object outside live rooted graph".into());
        }
        if let Some(&token) = self.reverse.get(&(native as usize)) {
            if self.native(api, token)? != native {
                return Err("borrowed reverse identity changed".into());
            }
            return Ok(token);
        }
        if self.roots.len() >= LIMIT {
            return Err("borrowed object root capacity exceeded".into());
        }
        let class = unsafe { (api.class)(native) };
        if class.is_null() {
            return Err("borrowed object has no class".into());
        }
        let handle = unsafe { (api.strong)(native, 1) };
        if handle.is_null() || unsafe { (api.target)(handle) } != native {
            if !handle.is_null() {
                unsafe {
                    (api.free)(handle);
                }
            }
            return Err("borrowed pinned root failed".into());
        }
        let token = BASE + self.roots.len() as u64 * 65536;
        self.roots.insert(
            token,
            Root {
                handle,
                native,
                class,
                borrowed: true,
                cached: None,
                published: false,
            },
        );
        self.reverse.insert(native as usize, token);
        Ok(token)
    }
    /// Real mono_runtime_invoke scalar box; exact value class/payload are
    /// validated by the typed call adapter before this pinned adoption.
    pub(super) fn borrow_boxed_result(
        &mut self,
        api: Api,
        native: Object,
        class: Object,
    ) -> Result<u64, String> {
        let token = self.borrow(api, native)?;
        if class.is_null() || self.class(api, token)? != class {
            return Err("returned boxed value class changed".into());
        }
        self.boxed.insert(token);
        Ok(token)
    }
    /// Attach an already validated empty-array identity owned by its canonical
    /// registry. This does not publish headers or accept arbitrary guest tokens.
    pub(super) fn adopt_empty_array(
        &mut self,
        api: Api,
        token: u64,
        native: Object,
    ) -> Result<(), String> {
        if !(0x6ffe90000000..0x6ffe90040000).contains(&token)
            || !(token - 0x6ffe90000000).is_multiple_of(256)
        {
            return Err("array alias outside canonical empty-array arena".into());
        }
        self.adopt_canonical(api, token, native)
    }
    /// Called only after the string registry verifies this canonical identity.
    pub(super) fn adopt_string(
        &mut self,
        api: Api,
        token: u64,
        native: Object,
    ) -> Result<(), String> {
        if !(0x6ffe70000000..0x6ffe78000000).contains(&token)
            || !(token - 0x6ffe70000000).is_multiple_of(131072)
        {
            return Err("string alias outside canonical string arena".into());
        }
        self.adopt_canonical(api, token, native)
    }
    /// Only a fully validated owned SZ reference-array allocation may call this.
    pub(super) fn adopt_reference_array(
        &mut self,
        api: Api,
        token: u64,
        native: Object,
    ) -> Result<(), String> {
        if !(0x6ffeb0000000..0x6ffeb1000000).contains(&token)
            || !(token - 0x6ffeb0000000).is_multiple_of(65536)
        {
            return Err("reference array alias outside canonical arena".into());
        }
        self.adopt_canonical(api, token, native)
    }
    fn adopt_canonical(&mut self, api: Api, token: u64, native: Object) -> Result<(), String> {
        if self.released || self.views_retired || native.is_null() {
            return Err("canonical alias after retirement/NULL".into());
        }
        if self.roots.contains_key(&token) {
            if self.native(api, token)? == native {
                return Ok(());
            }
            return Err("canonical array alias changed identity".into());
        }
        if self.roots.len() >= LIMIT || self.reverse.contains_key(&(native as usize)) {
            return Err("canonical alias exceeds capacity or duplicates native identity".into());
        }
        let handle = unsafe { (api.strong)(native, 1) };
        if handle.is_null() || unsafe { (api.target)(handle) } != native {
            if !handle.is_null() {
                unsafe {
                    (api.free)(handle);
                }
            }
            return Err("canonical array alias pin failed".into());
        }
        let class = unsafe { (api.class)(native) };
        if class.is_null() {
            unsafe {
                (api.free)(handle);
            }
            return Err("canonical array alias has no class".into());
        }
        self.roots.insert(
            token,
            Root {
                handle,
                native,
                class,
                borrowed: true,
                cached: None,
                published: false,
            },
        );
        self.reverse.insert(native as usize, token);
        Ok(())
    }
    pub(super) fn compatible(
        &self,
        api: Api,
        token: u64,
        expected: Object,
    ) -> Result<Object, String> {
        let native = self.native(api, token)?;
        if expected.is_null()
            || unsafe { (api.assignable)(expected, self.roots[&token].class) } == 0
        {
            return Err("object reference token has incompatible class".into());
        }
        Ok(native)
    }
    pub(super) fn publish(&mut self, memory: &mut Memory, token: u64) -> Result<(), String> {
        if !self.contains(token) {
            return Err("ordinary object publication after retirement".into());
        }
        let root = self
            .roots
            .get_mut(&token)
            .ok_or("unknown ordinary object token")?;
        if let Some(view) = &root.cached {
            if !root.published {
                // Exact original UnityPlayer at RVA 0x78392c writes the bound
                // C++ object address to m_CachedPtr at +16. Only that scalar
                // cell is mirrored; object headers/references remain private.
                memory.map_shared_data(token + 16, view.clone(), READ | WRITE, READ | WRITE)?;
                root.published = true;
            }
        }
        Ok(())
    }
    pub(super) fn sync_from_guest(&mut self, api: Api, memory: &Memory) -> Result<(), String> {
        if self.released || self.views_retired {
            return Err("ordinary scalar synchronization after retirement".into());
        }
        let mut updates = Vec::new();
        for (&token, root) in &self.roots {
            let Some(view) = &root.cached else {
                continue;
            };
            if !root.published {
                return Err("Unity cached pointer view not published".into());
            }
            let value =
                view.with_native(|p| unsafe { p.as_ptr().cast::<u64>().read_unaligned() })?;
            if value != 0 {
                if !value.is_multiple_of(8) {
                    return Err("Unity cached pointer is not aligned guest data".into());
                }
                memory.check(value, 1, READ)?;
            }
            updates.push((self.native(api, token)?, value));
        }
        // Check every guest pointer and root before mutating any host field.
        for (native, mut value) in updates {
            unsafe {
                (api.field_set)(native, api.cached_field, (&mut value as *mut u64).cast());
            }
            self.synchronized += 1;
        }
        Ok(())
    }
    pub(super) fn retire_views(&mut self, memory: &mut Memory) -> Result<(), String> {
        if self.views_retired {
            return Ok(());
        }
        if !self.released && !self.roots.is_empty() {
            return Err("ordinary object views retired before strong roots".into());
        }
        for (&token, root) in &mut self.roots {
            if root.published {
                memory.unmap_shared_data(token + 16)?;
                root.published = false;
            }
            if let Some(view) = &root.cached {
                view.revoke()?;
            }
        }
        self.views_retired = true;
        Ok(())
    }
    pub(super) fn native(&self, api: Api, token: u64) -> Result<Object, String> {
        if !self.contains(token) {
            return Err("unknown or retired ordinary object token".into());
        }
        let root = &self.roots[&token];
        if unsafe { (api.target)(root.handle) } != root.native
            || unsafe { (api.class)(root.native) } != root.class
        {
            return Err("ordinary object root/class identity changed".into());
        }
        Ok(root.native)
    }
    pub(super) fn class(&self, api: Api, token: u64) -> Result<Object, String> {
        self.native(api, token)?;
        Ok(self.roots[&token].class)
    }
    pub(super) fn verify(&mut self, api: Api) -> Result<(), String> {
        if self.released {
            return Err("ordinary object GC verification after release".into());
        }
        for (&token, root) in &self.roots {
            let native = self.native(api, token)?;
            if let Some(view) = &root.cached {
                let expected =
                    view.with_native(|p| unsafe { p.as_ptr().cast::<u64>().read_unaligned() })?;
                let mut actual = 0u64;
                unsafe {
                    (api.field_get)(native, api.cached_field, (&mut actual as *mut u64).cast());
                }
                if actual != expected {
                    return Err("Unity cached scalar differs across native GC".into());
                }
            }
        }
        self.verified += self.roots.values().filter(|r| !r.borrowed).count();
        self.borrowed_verified += self.roots.values().filter(|r| r.borrowed).count();
        Ok(())
    }
    pub(super) fn roots_released(&self) -> bool {
        self.released || self.roots.is_empty()
    }
    pub(super) fn release(&mut self, api: Api) {
        if self.released {
            return;
        }
        self.released = true;
        for root in self.roots.values_mut() {
            unsafe {
                (api.free)(std::mem::replace(&mut root.handle, ptr::null_mut()));
            }
        }
    }
    pub(super) fn report(&self) -> String {
        format!("native-allocated-objects={}\nnative-allocated-object-gc-verifications={}\nnative-allocated-object-roots-released={}\nnative-allocated-object-views-retired={}\nnative-unity-cached-pointer-syncs={}\n", self.roots.values().filter(|r| !r.borrowed).count(), self.verified, self.released, self.views_retired, self.synchronized) + &format!("native-borrowed-object-roots={}\nnative-borrowed-object-gc-verifications={}\n", self.roots.values().filter(|r| r.borrowed).count(), self.borrowed_verified) + &format!("native-boxed-value-objects={}\n",self.boxed.len())
    }
}
#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::cell::Cell;
    thread_local! {
        static FREES: Cell<usize> = const { Cell::new(0) };
        static CHANGED: Cell<bool> = const { Cell::new(false) };
    }
    unsafe extern "C" fn strong(_: Object, _: i32) -> Object {
        3usize as Object
    }
    unsafe extern "C" fn new(_: Object, _: Object, status: *mut u16) -> Object {
        unsafe {
            *status = 0;
        }
        3usize as Object
    }
    unsafe extern "C" fn target(_: Object) -> Object {
        if CHANGED.with(Cell::get) {
            ptr::null_mut()
        } else {
            4usize as Object
        }
    }
    unsafe extern "C" fn class(_: Object) -> Object {
        2usize as Object
    }
    unsafe extern "C" fn free(_: Object) {
        FREES.with(|v| v.set(v.get() + 1));
    }
    unsafe extern "C" fn assignable(base: Object, class: Object) -> i32 {
        i32::from(base == class || base == 1usize as Object)
    }
    thread_local! { static CACHED: Cell<u64> = const { Cell::new(0) }; }
    unsafe extern "C" fn field_get(_: Object, _: Object, dest: Object) {
        unsafe {
            dest.cast::<u64>().write_unaligned(CACHED.with(Cell::get));
        }
    }
    unsafe extern "C" fn field_set(_: Object, _: Object, value: Object) {
        CACHED.with(|v| v.set(unsafe { value.cast::<u64>().read_unaligned() }));
    }
    pub(in crate::x64::process::mono_startup) fn api() -> Api {
        FREES.with(|v| v.set(0));
        CHANGED.with(|v| v.set(false));
        Api {
            strong,
            new,
            value_box: {
                unsafe extern "C" fn value_box(_: Object, _: Object, _: Object) -> Object {
                    4usize as Object
                }
                value_box
            },
            target,
            free,
            class,
            assignable,
            field_get,
            field_set,
            unity_base: ptr::null_mut(),
            cached_field: 5usize as Object,
        }
    }
    pub(in crate::x64::process::mono_startup) fn identity_api() -> Api {
        unsafe extern "C" fn identity(p: Object) -> Object {
            p
        }
        unsafe extern "C" fn pin(p: Object, _: i32) -> Object {
            p
        }
        Api {
            strong: pin,
            target: identity,
            ..api()
        }
    }
    #[test]
    fn worker_allocation_transfers_a_strong_root_and_drops_failed_replies() {
        unsafe extern "C" fn worker_new(domain: Object, class: Object, status: *mut u16) -> Object {
            assert_eq!(std::thread::current().name(), Some("wire-test-allocation"));
            unsafe { new(domain, class, status) }
        }
        let api = Api {
            new: worker_new,
            ..api()
        };
        let execute = |request: WorkerRequest| {
            std::thread::Builder::new()
                .name("wire-test-allocation".into())
                .spawn(move || request.execute())
                .unwrap()
                .join()
                .unwrap()
        };
        let mut registry = Registry::default();
        assert!(registry
            .prepare_worker(api, ptr::null_mut(), 2usize as Object, None)
            .is_err());
        assert!(registry
            .prepare_worker(api, 1usize as Object, 2usize as Object, Some(257))
            .is_err());
        let request = registry
            .prepare_worker(api, 1usize as Object, 2usize as Object, None)
            .unwrap();
        assert!(request
            .run(|request| {
                drop(execute(request)?);
                Err("owner profiler callback failed".into())
            })
            .is_err());
        assert_eq!(registry.count(), 0);
        assert_eq!(FREES.with(Cell::get), 1);
        let request = registry
            .prepare_worker(api, 1usize as Object, 2usize as Object, None)
            .unwrap();
        let token = request.run(execute).unwrap();
        assert_eq!(registry.native(api, token).unwrap(), 4usize as Object);
        registry.verify(api).unwrap();
        registry.release(api);
        assert_eq!(FREES.with(Cell::get), 2);
        assert!(registry
            .prepare_worker(api, 1usize as Object, 2usize as Object, None)
            .is_err());
    }
    #[test]
    fn canonical_empty_array_alias_keeps_identity_without_publishing_native_header() {
        let api = api();
        let mut registry = Registry::default();
        let token = 0x6ffe90000000;
        registry
            .adopt_empty_array(api, token, 4usize as Object)
            .unwrap();
        registry
            .adopt_empty_array(api, token, 4usize as Object)
            .unwrap();
        assert_eq!(registry.borrow(api, 4usize as Object).unwrap(), token);
        assert!(registry
            .adopt_empty_array(api, token + 1, 4usize as Object)
            .is_err());
        assert!(registry
            .adopt_empty_array(api, token, 8usize as Object)
            .is_err());
        assert!(registry
            .adopt_empty_array(api, token + 256, 8usize as Object)
            .is_err());
        assert_eq!(registry.count(), 1);
        let mut memory = Memory::default();
        registry.publish(&mut memory, token).unwrap();
        assert!(memory.read_int(token, 8).is_err());
        registry.verify(api).unwrap();
        registry.release(api);
        registry.retire_views(&mut memory).unwrap();
        assert_eq!(FREES.with(Cell::get), 2);
        assert!(registry
            .adopt_empty_array(api, token, 4usize as Object)
            .is_err());
    }
    #[test]
    fn boxed_zero_root_has_no_unboxed_host_alias_and_retires_with_other_objects() {
        let api = api();
        let mut roots = Registry::default();
        assert!(roots
            .allocate_boxed(api, 1usize as Object, 2usize as Object, 257)
            .is_err());
        let token = roots
            .allocate_boxed(api, 1usize as Object, 2usize as Object, 88)
            .unwrap();
        assert_eq!(roots.native(api, token).unwrap(), 4usize as Object);
        let mut memory = Memory::default();
        roots.publish(&mut memory, token).unwrap();
        assert!(memory.read_int(token + 16, 8).is_err());
        roots.verify(api).unwrap();
        roots.release(api);
        roots.retire_views(&mut memory).unwrap();
        assert_eq!(FREES.with(Cell::get), 1);
    }
    #[test]
    fn canonical_string_alias_checks_arena_and_retains_exact_field_reference_identity() {
        let api = api();
        let mut registry = Registry::default();
        let token = 0x6ffe70000000;
        for invalid in [token + 1, token - 1, 0x6ffe78000000] {
            assert!(registry
                .adopt_string(api, invalid, 4usize as Object)
                .is_err());
        }
        registry.adopt_string(api, token, 4usize as Object).unwrap();
        assert_eq!(registry.borrow(api, 4usize as Object).unwrap(), token);
        assert_eq!(
            registry.compatible(api, token, 2usize as Object).unwrap(),
            4usize as Object
        );
        assert!(registry.compatible(api, token, 6usize as Object).is_err());
        assert!(registry
            .adopt_string(api, token + 131072, 4usize as Object)
            .is_err());
        registry.release(api);
        assert_eq!(FREES.with(Cell::get), 1);
    }
    #[test]
    fn borrowed_reachable_object_is_pinned_deduplicated_and_separately_counted() {
        let api = api();
        let mut registry = Registry::default();
        let token = registry.borrow(api, 4usize as Object).unwrap();
        assert_eq!(registry.borrow(api, 4usize as Object).unwrap(), token);
        assert!(registry.compatible(api, token, 5usize as Object).is_err());
        assert!(registry.borrow(api, 8usize as Object).is_err());
        assert!(registry.borrow(api, ptr::null_mut()).is_err());
        assert_eq!(registry.count(), 1);
        registry.verify(api).unwrap();
        assert!(registry.report().contains("native-allocated-objects=0\n"));
        assert!(registry
            .report()
            .contains("native-borrowed-object-roots=1\n"));
        registry.release(api);
        assert_eq!(FREES.with(Cell::get), 2);
        assert!(registry.borrow(api, 4usize as Object).is_err());
    }
    #[test]
    fn allocation_root_identity_gc_and_retirement_have_no_guest_native_alias() {
        let api = api();
        let mut registry = Registry::default();
        assert!(registry
            .allocate(api, ptr::null_mut(), 2usize as Object)
            .is_err());
        let token = registry
            .allocate(api, 1usize as Object, 2usize as Object)
            .unwrap();
        assert!(crate::x64::memory::Memory::default()
            .read_int(token, 8)
            .is_err());
        assert!(registry.native(api, token + 8).is_err());
        assert_eq!(registry.class(api, token).unwrap(), 2usize as Object);
        registry.verify(api).unwrap();
        CHANGED.with(|v| v.set(true));
        assert!(registry.verify(api).is_err());
        CHANGED.with(|v| v.set(false));
        registry.release(api);
        registry.release(api);
        assert_eq!(FREES.with(Cell::get), 1);
        assert!(!registry.contains(token));
        assert!(registry.native(api, token).is_err());
        assert!(registry
            .allocate(api, 1usize as Object, 2usize as Object)
            .is_err());
    }
    #[test]
    fn failed_root_class_validation_releases_only_new_root() {
        let api = api();
        let mut registry = Registry::default();
        assert!(registry
            .allocate(api, 1usize as Object, 5usize as Object)
            .is_err());
        assert_eq!(registry.count(), 0);
        assert_eq!(FREES.with(Cell::get), 1);
        registry
            .allocate(api, 1usize as Object, 2usize as Object)
            .unwrap();
        assert!(registry
            .allocate(api, 1usize as Object, 2usize as Object)
            .is_err());
        assert_eq!(registry.count(), 1);
        assert_eq!(FREES.with(Cell::get), 2);
        registry.release(api);
        assert_eq!(FREES.with(Cell::get), 3);
    }
    #[test]
    fn unity_cached_scalar_is_exact_guest_data_and_syncs_without_native_header_alias() {
        let mut api = api();
        api.unity_base = 1usize as Object;
        CACHED.with(|v| v.set(0));
        let mut registry = Registry::default();
        let token = registry
            .allocate(api, 1usize as Object, 2usize as Object)
            .unwrap();
        let mut memory = Memory::default();
        memory.map(0x2000, 4096, READ | WRITE).unwrap();
        registry.publish(&mut memory, token).unwrap();
        assert!(memory.read_int(token, 8).is_err());
        assert!(memory.read_int(token + 8, 8).is_err());
        assert!(memory.read_int(token + 24, 8).is_err());
        memory.write_int(token + 16, 8, 0x2000).unwrap();
        assert!(registry.verify(api).is_err());
        registry.sync_from_guest(api, &memory).unwrap();
        registry.verify(api).unwrap();
        assert_eq!(CACHED.with(Cell::get), 0x2000);
        memory.write_int(token + 16, 8, 0x3000).unwrap();
        assert!(registry.sync_from_guest(api, &memory).is_err());
        assert_eq!(CACHED.with(Cell::get), 0x2000);
        registry.release(api);
        registry.retire_views(&mut memory).unwrap();
        assert!(memory.read_int(token + 16, 8).is_err());
        assert!(registry.sync_from_guest(api, &memory).is_err());
    }
}
