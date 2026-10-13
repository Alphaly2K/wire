//! Checked custom-attribute queries and opaque rooted instances. Info ownership
//! and managed roots are separate; no native object header or pointer is mapped.
use super::super::super::memory::{shared::HostRegion, READ};
use super::{c_char, Memory, Object};
use std::{collections::BTreeMap, ptr, sync::Arc};
#[derive(Clone, Copy)]
pub(super) struct Api {
    from_class: unsafe extern "C" fn(Object, *mut u16) -> Object,
    from_field: unsafe extern "C" fn(Object, Object, *mut u16) -> Object,
    has: unsafe extern "C" fn(Object, Object) -> i32,
    get: unsafe extern "C" fn(Object, Object, *mut u16) -> Object,
    construct: unsafe extern "C" fn(Object, *mut u16) -> Object,
    length: unsafe extern "C" fn(Object) -> usize,
    array_class: unsafe extern "C" fn(Object, u32) -> Object,
    array_addr: unsafe extern "C" fn(Object, i32, usize) -> Object,
    free_info: unsafe extern "C" fn(Object),
    strong: unsafe extern "C" fn(Object, i32) -> Object,
    target: unsafe extern "C" fn(Object) -> Object,
    free_root: unsafe extern "C" fn(Object),
    object_class: unsafe extern "C" fn(Object) -> Object,
    assignable: unsafe extern "C" fn(Object, Object) -> i32,
    field_parent: unsafe extern "C" fn(Object) -> Object,
    field_flags: unsafe extern "C" fn(Object) -> u32,
    field_type: unsafe extern "C" fn(Object) -> Object,
    kind: unsafe extern "C" fn(Object) -> u32,
    byref: unsafe extern "C" fn(Object) -> u32,
    type_class: unsafe extern "C" fn(Object) -> Object,
    is_enum: unsafe extern "C" fn(Object) -> i32,
    is_value: unsafe extern "C" fn(Object) -> i32,
    enum_base: unsafe extern "C" fn(Object) -> Object,
    field_get: unsafe extern "C" fn(Object, Object, Object),
    attribute_class: Object,
}
impl Api {
    pub(super) fn load(library: Object) -> Result<Self, String> {
        macro_rules! sym {
            ($name:literal,$ty:ty) => {{
                let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return Err(concat!("missing checked attrs symbol: ", $name).into());
                }
                unsafe { std::mem::transmute::<Object, $ty>(p) }
            }};
        }
        let find = sym!(
            "mono_class_from_name",
            unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object
        );
        let corlib = sym!("mono_get_corlib", unsafe extern "C" fn() -> Object);
        let attribute_class = unsafe { find(corlib(), c"System".as_ptr(), c"Attribute".as_ptr()) };
        if attribute_class.is_null() {
            return Err("System.Attribute absent".into());
        }
        Ok(Self {
            from_class: sym!(
                "wire_mono_custom_attrs_from_class_checked",
                unsafe extern "C" fn(Object, *mut u16) -> Object
            ),
            from_field: sym!(
                "wire_mono_custom_attrs_from_field_checked",
                unsafe extern "C" fn(Object, Object, *mut u16) -> Object
            ),
            has: sym!(
                "mono_custom_attrs_has_attr",
                unsafe extern "C" fn(Object, Object) -> i32
            ),
            get: sym!(
                "wire_mono_custom_attrs_get_attr_checked",
                unsafe extern "C" fn(Object, Object, *mut u16) -> Object
            ),
            construct: sym!(
                "wire_mono_custom_attrs_construct_checked",
                unsafe extern "C" fn(Object, *mut u16) -> Object
            ),
            length: sym!("mono_array_length", unsafe extern "C" fn(Object) -> usize),
            array_class: sym!(
                "mono_array_class_get",
                unsafe extern "C" fn(Object, u32) -> Object
            ),
            array_addr: sym!(
                "mono_array_addr_with_size",
                unsafe extern "C" fn(Object, i32, usize) -> Object
            ),
            free_info: sym!("mono_custom_attrs_free", unsafe extern "C" fn(Object)),
            strong: sym!(
                "mono_gchandle_new_v2",
                unsafe extern "C" fn(Object, i32) -> Object
            ),
            target: sym!(
                "mono_gchandle_get_target_v2",
                unsafe extern "C" fn(Object) -> Object
            ),
            free_root: sym!("mono_gchandle_free_v2", unsafe extern "C" fn(Object)),
            object_class: sym!(
                "mono_object_get_class",
                unsafe extern "C" fn(Object) -> Object
            ),
            assignable: sym!(
                "mono_class_is_assignable_from",
                unsafe extern "C" fn(Object, Object) -> i32
            ),
            field_parent: sym!(
                "mono_field_get_parent",
                unsafe extern "C" fn(Object) -> Object
            ),
            field_flags: sym!("mono_field_get_flags", unsafe extern "C" fn(Object) -> u32),
            field_type: sym!(
                "mono_field_get_type",
                unsafe extern "C" fn(Object) -> Object
            ),
            kind: sym!("mono_type_get_type", unsafe extern "C" fn(Object) -> u32),
            byref: sym!("mono_type_is_byref", unsafe extern "C" fn(Object) -> u32),
            type_class: sym!(
                "mono_class_from_mono_type",
                unsafe extern "C" fn(Object) -> Object
            ),
            is_enum: sym!("mono_class_is_enum", unsafe extern "C" fn(Object) -> i32),
            is_value: sym!(
                "mono_class_is_valuetype",
                unsafe extern "C" fn(Object) -> i32
            ),
            enum_base: sym!(
                "mono_class_enum_basetype",
                unsafe extern "C" fn(Object) -> Object
            ),
            field_get: sym!(
                "mono_field_get_value",
                unsafe extern "C" fn(Object, Object, Object)
            ),
            attribute_class,
        })
    }
    pub(super) fn field_width(self, field: Object) -> Result<usize, String> {
        Ok(match self.field_shape(field)? {
            FieldShape::Scalar(width) => width,
            FieldShape::Reference(_) => 8,
        })
    }
    fn field_shape(self, field: Object) -> Result<FieldShape, String> {
        if field.is_null() || unsafe { (self.field_flags)(field) } & (0x10 | 0x40) != 0 {
            return Err("attribute field must be a real instance field".into());
        }
        let mut ty = unsafe { (self.field_type)(field) };
        if ty.is_null() || unsafe { (self.byref)(ty) } != 0 {
            return Err("attribute field type/byref not admitted".into());
        }
        if unsafe { (self.kind)(ty) } == 0x11 {
            let class = unsafe { (self.type_class)(ty) };
            if class.is_null() || unsafe { (self.is_enum)(class) } == 0 {
                return Err("attribute struct field not admitted".into());
            }
            ty = unsafe { (self.enum_base)(class) };
            if ty.is_null() {
                return Err("attribute enum base absent".into());
            }
        }
        match unsafe { (self.kind)(ty) } {
            2 | 4 | 5 => Ok(FieldShape::Scalar(1)),
            3 | 6 | 7 => Ok(FieldShape::Scalar(2)),
            8 | 9 | 12 => Ok(FieldShape::Scalar(4)),
            10 | 11 | 13 => Ok(FieldShape::Scalar(8)),
            0x0e | 0x12 | 0x15 | 0x1c | 0x1d => {
                let class = unsafe { (self.type_class)(ty) };
                if class.is_null() || unsafe { (self.is_value)(class) } != 0 {
                    return Err("attribute reference has no resolved reference class".into());
                }
                Ok(FieldShape::Reference(class))
            }
            _ => Err("attribute field shape requires scalar or owned managed reference; pointer/byref/value storage not admitted".into()),
        }
    }
}
enum FieldShape {
    Scalar(usize),
    Reference(Object),
}
pub(super) enum FieldValue {
    Scalar(Vec<u8>),
    /// Private, pinned reference. The caller must canonicalize and publish its
    /// existing object/string/RuntimeType protocol before writing a guest token.
    Reference(Object),
}
enum FieldCheck {
    Scalar(Vec<u8>),
    Reference(Object), // private pinned handle, NULL for a NULL field
}
struct Root {
    native: Object,
    class: Object,
    handle: Object,
}
struct Array {
    root: Root,
    elements: Vec<u64>,
    view: HostRegion,
    published: bool,
}
struct Info {
    native: Object,
    free: unsafe extern "C" fn(Object),
}
// Owned, nonmoving immutable image metadata. An in-flight constructor retains
// it independently of the guest token; the last lease performs exactly one free.
unsafe impl Send for Info {}
unsafe impl Sync for Info {}
impl Drop for Info {
    fn drop(&mut self) {
        unsafe { (self.free)(self.native) };
    }
}
pub(super) struct ConstructRequest {
    api: Api,
    info: Arc<Info>,
    single: Option<Object>,
}
pub(super) struct Constructed {
    api: Api,
    array: Object,
    handle: Object,
    status: u16,
}
impl Drop for Constructed {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { (self.api.free_root)(self.handle) };
        }
    }
}
impl ConstructRequest {
    /// May allocate and execute attribute constructors/GC callbacks. No guest
    /// memory, Runtime or registry borrow is held across this invocation.
    pub(super) fn execute(&self) -> Result<Constructed, String> {
        let mut status = 65535;
        let array = unsafe {
            match self.single {
                Some(class) => (self.api.get)(self.info.native, class, &mut status),
                None => (self.api.construct)(self.info.native, &mut status),
            }
        };
        if status == 65535 {
            return Err("attribute array construction omitted status".into());
        }
        let mut result = Constructed {
            api: self.api,
            array,
            handle: ptr::null_mut(),
            status,
        };
        if status == 0 {
            if array.is_null() && self.single.is_none() {
                return Err("successful attribute construction returned no array".into());
            }
            if array.is_null() {
                return Ok(result);
            }
            result.handle = unsafe { (self.api.strong)(array, 1) };
            if result.handle.is_null() {
                return Err("attribute array root allocation failed".into());
            }
        }
        Ok(result)
    }
}
enum WorkerOperation {
    Query {
        class: Object,
        field: Option<Object>,
    },
    Has {
        info: Object,
        class: Object,
    },
    Free {
        info: Arc<Info>,
    },
}
/// Private non-moving image metadata/info capabilities only. No constructor or
/// unrooted managed object is admitted by these worker commands.
pub(super) struct WorkerRequest {
    api: Api,
    operation: WorkerOperation,
}
unsafe impl Send for WorkerRequest {}
pub(super) enum WorkerResult {
    Info(Object),
    Scalar(u64),
}
// CustomAttrInfo is owned native image metadata, not a movable managed object.
unsafe impl Send for WorkerResult {}
impl WorkerRequest {
    pub(super) fn execute(self) -> Result<WorkerResult, String> {
        match self.operation {
            WorkerOperation::Query { class, field } => {
                query_info(self.api, class, field).map(WorkerResult::Info)
            }
            WorkerOperation::Has { info, class } => {
                Ok(WorkerResult::Scalar(
                    unsafe { (self.api.has)(info, class) } as u32 as u64,
                ))
            }
            WorkerOperation::Free { info } => {
                drop(info);
                Ok(WorkerResult::Scalar(0))
            }
        }
    }
}
pub(super) struct WorkerBorrow<'a> {
    registry: &'a mut Registry,
    request: WorkerRequest,
}
impl WorkerBorrow<'_> {
    pub(super) fn run(
        self,
        execute: impl FnOnce(WorkerRequest) -> Result<WorkerResult, String>,
    ) -> Result<u64, String> {
        let api = self.request.api;
        let result = execute(self.request)?;
        match result {
            WorkerResult::Info(info) => {
                self.registry.queries += 1;
                self.registry.adopt_info(api, info)
            }
            WorkerResult::Scalar(value) => Ok(value),
        }
    }
}
fn query_info(api: Api, class: Object, field: Option<Object>) -> Result<Object, String> {
    if let Some(field) = field {
        if unsafe { (api.field_parent)(field) } != class {
            return Err("attribute field query requires exact declaring class".into());
        }
    }
    let mut status = 65535;
    let info = unsafe {
        match field {
            Some(field) => (api.from_field)(class, field, &mut status),
            None => (api.from_class)(class, &mut status),
        }
    };
    if status != 0 {
        return Err(format!("native checked attribute metadata error={status}"));
    }
    Ok(info)
}
#[derive(Default)]
pub(super) struct Registry {
    infos: BTreeMap<u64, Arc<Info>>,
    objects: BTreeMap<u64, Root>,
    arrays: BTreeMap<u64, Array>,
    array_verified: usize,
    views_retired: bool,
    field_checks: BTreeMap<(u64, usize), FieldCheck>,
    issued: usize,
    queries: usize,
    reads: usize,
    verified: usize,
    released: bool,
}
impl Registry {
    /// Caller resolves class/field through the live typed metadata registry.
    /// This borrow prevents info release while the actual OS worker uses it;
    /// only profiler scalars can reenter the owner during this synchronous wait.
    pub(super) fn prepare_worker(
        &mut self,
        api: Api,
        function: &str,
        token: u64,
        class: Object,
        field: Option<Object>,
    ) -> Result<WorkerBorrow<'_>, String> {
        if self.released {
            return Err("attribute worker command after retirement".into());
        }
        let operation = match function {
            "mono_custom_attrs_from_class" | "mono_custom_attrs_from_field" => {
                if self.issued >= 65536 || class.is_null() {
                    return Err("attribute worker query capacity/class invalid".into());
                }
                WorkerOperation::Query { class, field }
            }
            "mono_custom_attrs_has_attr" if !class.is_null() => WorkerOperation::Has {
                info: self.info(token)?,
                class,
            },
            "mono_custom_attrs_free" => WorkerOperation::Free {
                info: self.take_info(token)?,
            },
            _ => return Err("untyped worker attribute metadata operation".into()),
        };
        Ok(WorkerBorrow {
            registry: self,
            request: WorkerRequest { api, operation },
        })
    }
    pub(super) fn contains(&self, token: u64) -> bool {
        !self.released
            && !self.views_retired
            && (self.objects.contains_key(&token) || self.arrays.contains_key(&token))
    }
    pub(super) fn native(&self, api: Api, token: u64) -> Result<Object, String> {
        if !self.contains(token) {
            return Err("unknown or retired attribute object token".into());
        }
        let root = self
            .objects
            .get(&token)
            .or_else(|| self.arrays.get(&token).map(|a| &a.root))
            .ok_or("unknown attribute object token")?;
        if unsafe { (api.target)(root.handle) } != root.native
            || unsafe { (api.object_class)(root.native) } != root.class
        {
            return Err("attribute object root/class identity changed".into());
        }
        Ok(root.native)
    }
    pub(super) fn class(&self, api: Api, token: u64) -> Result<Object, String> {
        self.native(api, token)?;
        Ok(self
            .objects
            .get(&token)
            .map(|r| r.class)
            .unwrap_or_else(|| self.arrays[&token].root.class))
    }
    fn info(&self, token: u64) -> Result<Object, String> {
        if self.released {
            return Err("attribute info registry released".into());
        }
        self.infos
            .get(&token)
            .map(|info| info.native)
            .ok_or("unknown or freed custom-attribute info token".into())
    }
    pub(super) fn query(
        &mut self,
        api: Api,
        class: Object,
        field: Option<Object>,
    ) -> Result<u64, String> {
        if self.released || self.issued >= 65536 {
            return Err("attribute info registry outside live capacity".into());
        }
        let info = query_info(api, class, field)?;
        self.queries += 1;
        self.adopt_info(api, info)
    }
    fn adopt_info(&mut self, api: Api, info: Object) -> Result<u64, String> {
        if info.is_null() {
            return Ok(0);
        }
        // The locked non-dynamic image path returns a fresh owned info. Keep
        // an existing owner intact if a future runtime violates that contract.
        if self.infos.values().any(|existing| existing.native == info) {
            return Err("duplicate live attribute info ownership".into());
        }
        let token = 0x6ffee0000000 + self.issued as u64 * 16;
        self.issued += 1;
        self.infos.insert(
            token,
            Arc::new(Info {
                native: info,
                free: api.free_info,
            }),
        );
        Ok(token)
    }
    pub(super) fn has(&self, api: Api, token: u64, class: Object) -> Result<u64, String> {
        Ok(unsafe { (api.has)(self.info(token)?, class) } as u32 as u64)
    }
    pub(super) fn get(&mut self, api: Api, token: u64, class: Object) -> Result<u64, String> {
        let request = self.prepare_get(api, token, class)?;
        let result = request.execute();
        let (token, status) = self.finish_construct(request, result)?;
        if status != 0 {
            return Err(format!(
                "native checked attribute constructor error={status}"
            ));
        }
        Ok(token)
    }
    fn root_object(&mut self, api: Api, object: Object, class: Object) -> Result<u64, String> {
        let actual = unsafe { (api.object_class)(object) };
        if actual.is_null()
            || unsafe { (api.assignable)(api.attribute_class, actual) } == 0
            || unsafe { (api.assignable)(class, actual) } == 0
        {
            return Err("constructed object is not the requested Attribute subtype".into());
        }
        if let Some((token, _)) = self.objects.iter().find(|(_, r)| r.native == object) {
            return Ok(*token);
        }
        if self.objects.len() >= 8192 {
            return Err("attribute object root capacity exceeded".into());
        }
        let handle = unsafe { (api.strong)(object, 1) };
        if handle.is_null() {
            return Err("attribute object root allocation failed".into());
        }
        if unsafe { (api.target)(handle) } != object {
            unsafe { (api.free_root)(handle) };
            return Err("attribute root target mismatch".into());
        }
        let token = 0x6ffee8000000 + self.objects.len() as u64 * 256;
        self.objects.insert(
            token,
            Root {
                native: object,
                class: actual,
                handle,
            },
        );
        Ok(token)
    }
    pub(super) fn construct(&mut self, api: Api, token: u64) -> Result<(u64, u16), String> {
        let request = self.prepare_construct(api, token)?;
        let result = request.execute();
        self.finish_construct(request, result)
    }
    pub(super) fn prepare_construct(
        &self,
        api: Api,
        token: u64,
    ) -> Result<ConstructRequest, String> {
        if self.released || self.arrays.len() >= 1024 {
            return Err("attribute arrays outside live capacity".into());
        }
        let info = self
            .infos
            .get(&token)
            .ok_or("unknown or freed custom-attribute construction info")?
            .clone();
        Ok(ConstructRequest {
            api,
            info,
            single: None,
        })
    }
    pub(super) fn prepare_get(
        &self,
        api: Api,
        token: u64,
        class: Object,
    ) -> Result<ConstructRequest, String> {
        if self.released
            || class.is_null()
            || unsafe { (api.assignable)(api.attribute_class, class) } == 0
        {
            return Err("attribute getter class is not an Attribute subtype".into());
        }
        let info = self
            .infos
            .get(&token)
            .ok_or("unknown or freed custom-attribute getter info")?
            .clone();
        Ok(ConstructRequest {
            api,
            info,
            single: Some(class),
        })
    }
    pub(super) fn finish_construct(
        &mut self,
        request: ConstructRequest,
        result: Result<Constructed, String>,
    ) -> Result<(u64, u16), String> {
        if self.released || (request.single.is_none() && self.arrays.len() >= 1024) {
            return Err("attribute construction completion outside live capacity".into());
        }
        let mut allocated = result?;
        if allocated.status != 0 {
            return Ok((0, allocated.status));
        }
        let api = request.api;
        let array = allocated.array;
        let handle = allocated.handle;
        if let Some(class) = request.single {
            if array.is_null() {
                return Ok((0, 0));
            }
            return self.root_object(api, array, class).map(|token| (token, 0));
        }
        let result = (|| {
            let class = unsafe { (api.array_class)(api.attribute_class, 1) };
            if class.is_null()
                || unsafe { (api.object_class)(array) } != class
                || unsafe { (api.target)(handle) } != array
            {
                return Err("attribute array type/root mismatch".into());
            }
            let count = unsafe { (api.length)(array) };
            if count > 1024 {
                return Err("attribute array length exceeds bounded startup contract".into());
            }
            // SZ arrays have NULL bounds. Inspect only the locked native header;
            // the guest receives an owned scalar/token view, never these bytes.
            if !unsafe { array.cast::<u8>().add(16).cast::<Object>().read_unaligned() }.is_null() {
                return Err("attribute construction did not return a SZ array".into());
            }
            let mut elements = Vec::with_capacity(count);
            for index in 0..count {
                let slot = unsafe { (api.array_addr)(array, 8, index) };
                if slot.is_null() {
                    return Err("attribute array element address absent".into());
                }
                let object = unsafe { slot.cast::<Object>().read_unaligned() };
                if object.is_null() {
                    return Err("attribute constructor produced NULL element".into());
                }
                elements.push(self.root_object(api, object, api.attribute_class)?);
            }
            let view = HostRegion::owned(16 + count * 8)?;
            let mut bytes = vec![0u8; 16 + count * 8];
            bytes[8..16].copy_from_slice(&(count as u64).to_le_bytes());
            for (index, element) in elements.iter().enumerate() {
                bytes[16 + index * 8..24 + index * 8].copy_from_slice(&element.to_le_bytes());
            }
            view.with_native(|dest| unsafe {
                ptr::copy_nonoverlapping(bytes.as_ptr(), dest.as_ptr(), bytes.len())
            })?;
            let token = 0x6ffef0000000 + self.arrays.len() as u64 * 65536;
            self.arrays.insert(
                token,
                Array {
                    root: Root {
                        native: array,
                        class,
                        handle,
                    },
                    elements,
                    view,
                    published: false,
                },
            );
            Ok((token, 0))
        })();
        if result.is_ok() {
            // The published Registry now owns the pin. Failure drops the
            // private reply pin, including late header/element rejection.
            allocated.handle = ptr::null_mut();
        }
        result
    }
    pub(super) fn publish_array(&mut self, memory: &mut Memory, token: u64) -> Result<(), String> {
        if !self.contains(token) {
            return Err("attribute array publication after retirement".into());
        }
        let array = self
            .arrays
            .get_mut(&token)
            .ok_or("unknown attribute array token")?;
        if !array.published {
            memory.map_shared_data(token + 16, array.view.clone(), READ, READ)?;
            array.published = true;
        }
        Ok(())
    }
    pub(super) fn arrays_ready(&self) -> bool {
        !self.released && !self.arrays.is_empty()
    }
    pub(super) fn contains_array(&self, token: u64) -> bool {
        self.contains(token) && self.arrays.contains_key(&token)
    }
    pub(super) fn array_length(&self, api: Api, token: u64) -> Result<u64, String> {
        let native = self.native(api, token)?;
        let array = self
            .arrays
            .get(&token)
            .ok_or("attribute length requires an array identity")?;
        let length = unsafe { (api.length)(native) };
        if !array.published || length != array.elements.len() {
            return Err("attribute array length differs from its issued identity".into());
        }
        Ok(length as u64)
    }
    pub(super) fn array_address(&self, token: u64, size: u64, index: u64) -> Result<u64, String> {
        let array = self
            .arrays
            .get(&token)
            .ok_or("unknown attribute array identity")?;
        if !self.contains_array(token)
            || !array.published
            || size != 8
            || index >= array.elements.len() as u64
        {
            return Err("attribute array address size/index/lifetime mismatch".into());
        }
        Ok(token + 32 + index * 8)
    }
    pub(super) fn retire_views(&mut self, memory: &mut Memory) -> Result<(), String> {
        if self.views_retired {
            return Ok(());
        }
        if !self.released && !self.arrays.is_empty() {
            return Err("attribute array views retired before roots".into());
        }
        for (&token, array) in &self.arrays {
            if array.published {
                memory.unmap_shared_data(token + 16)?;
            }
            array.view.revoke()?;
        }
        self.views_retired = true;
        Ok(())
    }
    pub(super) fn read_field(
        &mut self,
        api: Api,
        token: u64,
        field: Object,
    ) -> Result<FieldValue, String> {
        if !self.objects.contains_key(&token) {
            return Err("attribute field requires a single object identity".into());
        }
        let object = self.native(api, token)?;
        let parent = unsafe { (api.field_parent)(field) };
        if parent.is_null() || unsafe { (api.assignable)(parent, self.objects[&token].class) } == 0
        {
            return Err("attribute field belongs to a different object type".into());
        }
        let (value, check) = match api.field_shape(field)? {
            FieldShape::Scalar(width) => {
                let mut bytes = vec![0u8; width];
                unsafe { (api.field_get)(object, field, bytes.as_mut_ptr().cast()) };
                (FieldValue::Scalar(bytes.clone()), FieldCheck::Scalar(bytes))
            }
            FieldShape::Reference(expected) => {
                let mut native: Object = ptr::null_mut();
                unsafe { (api.field_get)(object, field, (&mut native as *mut Object).cast()) };
                let handle = if native.is_null() {
                    ptr::null_mut()
                } else {
                    let class = unsafe { (api.object_class)(native) };
                    if class.is_null() || unsafe { (api.assignable)(expected, class) } == 0 {
                        return Err("attribute reference field violates declared class".into());
                    }
                    let handle = unsafe { (api.strong)(native, 1) };
                    if handle.is_null() {
                        return Err("attribute reference field pin allocation failed".into());
                    }
                    if unsafe { (api.target)(handle) } != native {
                        unsafe { (api.free_root)(handle) };
                        return Err("attribute reference field root identity changed".into());
                    }
                    handle
                };
                (FieldValue::Reference(native), FieldCheck::Reference(handle))
            }
        };
        self.reads += 1;
        if let Some(FieldCheck::Reference(handle)) =
            self.field_checks.insert((token, field as usize), check)
        {
            if !handle.is_null() {
                unsafe { (api.free_root)(handle) };
            }
        }
        Ok(value)
    }
    fn take_info(&mut self, token: u64) -> Result<Arc<Info>, String> {
        if self.released {
            return Err("attribute info free after release".into());
        }
        self.infos
            .remove(&token)
            .ok_or("unknown or double-freed attribute info".into())
    }
    pub(super) fn free(&mut self, _api: Api, token: u64) -> Result<(), String> {
        let info = self.take_info(token)?;
        drop(info);
        Ok(())
    }
    pub(super) fn ready(&self) -> bool {
        !self.released && !self.objects.is_empty() && self.reads > 0
    }
    pub(super) fn verify(&mut self, api: Api) -> Result<(), String> {
        for &token in self.objects.keys() {
            self.native(api, token)?;
        }
        for (&token, array) in &self.arrays {
            let native = self.native(api, token)?;
            if unsafe { (api.length)(native) } != array.elements.len() {
                return Err("attribute array length changed across GC".into());
            }
            for (index, &element) in array.elements.iter().enumerate() {
                let slot = unsafe { (api.array_addr)(native, 8, index) };
                if slot.is_null()
                    || unsafe { slot.cast::<Object>().read_unaligned() }
                        != self.native(api, element)?
                {
                    return Err("attribute array element identity changed across GC".into());
                }
            }
        }
        self.array_verified += self.arrays.len();
        for (&(token, field), expected) in &self.field_checks {
            let object = self.native(api, token)?;
            match expected {
                FieldCheck::Scalar(expected) => {
                    let mut actual = vec![0u8; expected.len()];
                    unsafe { (api.field_get)(object, field as Object, actual.as_mut_ptr().cast()) };
                    if actual != *expected {
                        return Err("attribute scalar field changed across forced GC".into());
                    }
                }
                FieldCheck::Reference(handle) => {
                    let mut actual = ptr::null_mut();
                    unsafe {
                        (api.field_get)(
                            object,
                            field as Object,
                            (&mut actual as *mut Object).cast(),
                        )
                    };
                    let expected = if handle.is_null() {
                        ptr::null_mut()
                    } else {
                        unsafe { (api.target)(*handle) }
                    };
                    if actual != expected || (!handle.is_null() && expected.is_null()) {
                        return Err(
                            "attribute reference field identity changed across forced GC".into(),
                        );
                    }
                }
            }
        }
        self.verified += self.objects.len();
        Ok(())
    }
    fn retire_resources(&mut self) -> (Vec<Arc<Info>>, Vec<Object>) {
        if self.released {
            return (Vec::new(), Vec::new());
        }
        self.released = true;
        let infos = std::mem::take(&mut self.infos).into_values().collect();
        let mut roots: Vec<Object> = self
            .objects
            .values_mut()
            .map(|root| std::mem::replace(&mut root.handle, ptr::null_mut()))
            .collect();
        roots.extend(
            self.arrays
                .values_mut()
                .map(|array| std::mem::replace(&mut array.root.handle, ptr::null_mut())),
        );
        roots.extend(
            std::mem::take(&mut self.field_checks)
                .into_values()
                .filter_map(|check| match check {
                    FieldCheck::Reference(handle) if !handle.is_null() => Some(handle),
                    _ => None,
                }),
        );
        (infos, roots)
    }
    pub(super) fn release(&mut self, api: Api) {
        let (infos, roots) = self.retire_resources();
        drop(infos);
        for root in roots {
            unsafe { (api.free_root)(root) };
        }
    }
    pub(super) fn report(&self) -> String {
        format!("native-attribute-queries={}\nnative-attribute-info-handles={}\nnative-attribute-objects={}\nnative-attribute-field-reads={}\nnative-attribute-gc-verifications={}\nnative-attribute-resources-released={}\nnative-attribute-arrays={}\nnative-attribute-array-gc-verifications={}\nnative-attribute-array-views-retired={}\n",self.queries,self.issued,self.objects.len(),self.reads,self.verified,self.released,self.arrays.len(),self.array_verified,self.views_retired)
    }
}
pub(super) fn handles(function: &str) -> bool {
    matches!(
        function,
        "mono_custom_attrs_from_class"
            | "mono_custom_attrs_from_field"
            | "mono_custom_attrs_has_attr"
            | "mono_custom_attrs_get_attr"
            | "mono_custom_attrs_free"
            | "mono_unity_custom_attrs_construct"
            | "mono_object_get_class"
            | "mono_field_get_value"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    thread_local! {
        static INFO_FREES: Cell<usize> = const { Cell::new(0) };
        static ROOT_FREES: Cell<usize> = const { Cell::new(0) };
        static RESULT: Cell<Object> = const { Cell::new(ptr::null_mut()) };
        static STATUS: Cell<u16> = const { Cell::new(0) };
        static RESULT_CLASS: Cell<usize> = const { Cell::new(2) };
        static FIELD_KIND: Cell<u32> = const { Cell::new(0x12) };
        static FIELD_REFERENCE: Cell<Object> = const { Cell::new(ptr::null_mut()) };
        static FIELD_COMPATIBLE: Cell<bool> = const { Cell::new(true) };
    }
    fn checked_api() -> Api {
        unsafe extern "C" fn construct(info: Object, status: *mut u16) -> Object {
            assert_eq!(info as usize, 0x1234);
            assert_eq!(INFO_FREES.with(Cell::get), 0);
            unsafe { status.write(STATUS.with(Cell::get)) };
            RESULT.with(Cell::get)
        }
        unsafe extern "C" fn field(_: Object, info: Object, status: *mut u16) -> Object {
            unsafe { construct(info, status) }
        }
        unsafe extern "C" fn get_attribute(info: Object, _: Object, status: *mut u16) -> Object {
            unsafe { construct(info, status) }
        }
        unsafe extern "C" fn free_info(_: Object) {
            INFO_FREES.with(|n| n.set(n.get() + 1));
        }
        unsafe extern "C" fn free_root(_: Object) {
            ROOT_FREES.with(|n| n.set(n.get() + 1));
        }
        unsafe extern "C" fn root(object: Object, _: i32) -> Object {
            object
        }
        unsafe extern "C" fn target(object: Object) -> Object {
            object
        }
        unsafe extern "C" fn class(_: Object) -> Object {
            RESULT_CLASS.with(Cell::get) as Object
        }
        unsafe extern "C" fn array_class(_: Object, _: u32) -> Object {
            2usize as Object
        }
        unsafe extern "C" fn length(_: Object) -> usize {
            0
        }
        unsafe extern "C" fn address(_: Object, _: i32, _: usize) -> Object {
            ptr::null_mut()
        }
        unsafe extern "C" fn assignable(_: Object, _: Object) -> i32 {
            1
        }
        unsafe extern "C" fn zero(_: Object) -> u32 {
            0
        }
        unsafe extern "C" fn value(_: Object) -> i32 {
            0
        }
        unsafe extern "C" fn get(_: Object, _: Object, _: Object) {}
        Api {
            from_class: construct,
            from_field: field,
            has: assignable,
            get: get_attribute,
            construct,
            length,
            array_class,
            array_addr: address,
            free_info,
            strong: root,
            target,
            free_root,
            object_class: class,
            assignable,
            field_parent: target,
            field_flags: zero,
            field_type: target,
            kind: zero,
            byref: zero,
            type_class: target,
            is_enum: value,
            is_value: value,
            enum_base: target,
            field_get: get,
            attribute_class: 1usize as Object,
        }
    }
    fn reset_checked() {
        INFO_FREES.with(|n| n.set(0));
        ROOT_FREES.with(|n| n.set(0));
        STATUS.with(|n| n.set(0));
        RESULT_CLASS.with(|n| n.set(2));
        FIELD_KIND.with(|n| n.set(0x12));
        FIELD_REFERENCE.with(|n| n.set(ptr::null_mut()));
        FIELD_COMPATIBLE.with(|n| n.set(true));
    }
    fn field_api() -> Api {
        unsafe extern "C" fn kind(_: Object) -> u32 {
            FIELD_KIND.with(Cell::get)
        }
        unsafe extern "C" fn parent(_: Object) -> Object {
            1usize as Object
        }
        unsafe extern "C" fn assignable(expected: Object, _: Object) -> i32 {
            i32::from(expected as usize != 9 || FIELD_COMPATIBLE.with(Cell::get))
        }
        unsafe extern "C" fn read(_: Object, _: Object, output: Object) {
            if FIELD_KIND.with(Cell::get) == 8 {
                unsafe { output.cast::<i32>().write_unaligned(-123) };
            } else {
                unsafe {
                    output
                        .cast::<Object>()
                        .write_unaligned(FIELD_REFERENCE.with(Cell::get))
                };
            }
        }
        Api {
            kind,
            field_parent: parent,
            assignable,
            field_get: read,
            ..checked_api()
        }
    }
    fn field_registry() -> Registry {
        let mut registry = Registry::default();
        registry.objects.insert(
            0x6ffee8000000,
            Root {
                native: 0x2345usize as Object,
                class: 2usize as Object,
                handle: 0x2345usize as Object,
            },
        );
        registry
    }
    #[test]
    fn attribute_reference_fields_keep_private_pins_and_verify_real_identity_after_gc() {
        reset_checked();
        let api = field_api();
        let mut registry = field_registry();
        let token = 0x6ffee8000000;
        let field = 9usize as Object;
        assert_eq!(api.field_width(field).unwrap(), 8);
        FIELD_REFERENCE.with(|p| p.set(0x3456usize as Object));
        let FieldValue::Reference(native) = registry.read_field(api, token, field).unwrap() else {
            panic!("reference field must remain private until canonicalization");
        };
        assert_eq!(native as usize, 0x3456);
        registry.verify(api).unwrap();
        FIELD_REFERENCE.with(|p| p.set(0x4567usize as Object));
        assert!(registry.verify(api).is_err());
        registry.read_field(api, token, field).unwrap();
        assert_eq!(ROOT_FREES.with(Cell::get), 1);
        registry.verify(api).unwrap();
        FIELD_REFERENCE.with(|p| p.set(ptr::null_mut()));
        let FieldValue::Reference(native) = registry.read_field(api, token, field).unwrap() else {
            panic!()
        };
        assert!(native.is_null());
        assert_eq!(ROOT_FREES.with(Cell::get), 2);
        registry.verify(api).unwrap();
        registry.release(api);
        assert_eq!(ROOT_FREES.with(Cell::get), 3);
        assert!(registry.read_field(api, token, field).is_err());
    }
    #[test]
    fn attribute_fields_reject_incompatible_and_pointer_storage_and_preserve_signed_scalar_bytes() {
        reset_checked();
        let api = field_api();
        let mut registry = field_registry();
        let token = 0x6ffee8000000;
        let field = 9usize as Object;
        FIELD_REFERENCE.with(|p| p.set(0x3456usize as Object));
        FIELD_COMPATIBLE.with(|p| p.set(false));
        assert!(registry.read_field(api, token, field).is_err());
        assert!(registry.field_checks.is_empty());
        assert_eq!(registry.reads, 0);
        FIELD_KIND.with(|p| p.set(0x0f));
        assert!(api.field_width(field).is_err());
        assert!(registry.read_field(api, token, field).is_err());
        assert!(registry.read_field(api, token + 1, field).is_err());
        FIELD_KIND.with(|p| p.set(8));
        let FieldValue::Scalar(bytes) = registry.read_field(api, token, field).unwrap() else {
            panic!()
        };
        assert_eq!(bytes, (-123i32).to_le_bytes());
        registry.verify(api).unwrap();
        registry.release(api);
        assert_eq!(ROOT_FREES.with(Cell::get), 1);
    }
    #[test]
    fn attribute_construction_retains_info_after_guest_free_and_transfers_the_array_pin() {
        reset_checked();
        let api = checked_api();
        let mut data = [0usize; 4]; // actual private empty SZ header
        RESULT.with(|p| p.set(data.as_mut_ptr().cast()));
        let mut registry = Registry::default();
        let token = registry.adopt_info(api, 0x1234usize as Object).unwrap();
        let request = registry.prepare_construct(api, token).unwrap();
        registry.free(api, token).unwrap();
        assert!(registry.prepare_construct(api, token).is_err());
        assert_eq!(INFO_FREES.with(Cell::get), 0);
        let result = request.execute();
        let (array, status) = registry.finish_construct(request, result).unwrap();
        assert_eq!(status, 0);
        assert_eq!(INFO_FREES.with(Cell::get), 1);
        assert_eq!(ROOT_FREES.with(Cell::get), 0);
        let mut memory = Memory::default();
        registry.publish_array(&mut memory, array).unwrap();
        assert_eq!(registry.array_length(api, array).unwrap(), 0);
        registry.release(api);
        assert_eq!(ROOT_FREES.with(Cell::get), 1);
    }
    #[test]
    fn single_attribute_requests_keep_canonical_roots_and_allow_an_absent_match() {
        reset_checked();
        let api = checked_api();
        let mut data = [0usize; 4];
        RESULT.with(|p| p.set(data.as_mut_ptr().cast()));
        let mut registry = Registry::default();
        let info = registry.adopt_info(api, 0x1234usize as Object).unwrap();
        let first = registry.get(api, info, 2usize as Object).unwrap();
        assert!(registry.contains(first));
        assert_eq!(registry.get(api, info, 2usize as Object).unwrap(), first);
        assert_eq!(registry.objects.len(), 1);
        assert!(registry.arrays.is_empty());
        assert_eq!(ROOT_FREES.with(Cell::get), 2);
        RESULT.with(|p| p.set(ptr::null_mut()));
        assert_eq!(registry.get(api, info, 2usize as Object).unwrap(), 0);
        registry.release(api);
        assert_eq!(ROOT_FREES.with(Cell::get), 3);
        assert_eq!(INFO_FREES.with(Cell::get), 1);
    }
    #[test]
    fn attribute_reply_pin_retires_on_discard_and_late_type_failure_and_errors_keep_status() {
        for failure in [0, 1, 2] {
            reset_checked();
            let api = checked_api();
            let mut data = [0usize; 4];
            RESULT.with(|p| p.set(data.as_mut_ptr().cast()));
            let mut registry = Registry::default();
            let token = registry.adopt_info(api, 0x1234usize as Object).unwrap();
            let request = registry.prepare_construct(api, token).unwrap();
            match failure {
                0 => {
                    drop(request.execute().unwrap());
                    drop(request);
                }
                1 => {
                    let result = request.execute();
                    RESULT_CLASS.with(|c| c.set(3));
                    assert!(registry.finish_construct(request, result).is_err());
                }
                _ => {
                    STATUS.with(|c| c.set(7));
                    RESULT.with(|p| p.set(ptr::null_mut()));
                    let result = request.execute();
                    assert_eq!(registry.finish_construct(request, result).unwrap(), (0, 7));
                }
            }
            assert!(registry.arrays.is_empty());
            assert_eq!(ROOT_FREES.with(Cell::get), usize::from(failure != 2));
            registry.release(api);
            assert_eq!(INFO_FREES.with(Cell::get), 1);
        }
    }
    unsafe extern "C" fn no_free(_: Object) {}
    fn info(pointer: usize) -> Arc<Info> {
        Arc::new(Info {
            native: pointer as Object,
            free: no_free,
        })
    }
    #[test]
    fn array_token_view_is_readonly_owns_length_and_elements_and_retires_after_roots() {
        let mut registry = Registry::default();
        let token = 0x6ffef0000000;
        let element = 0x6ffee8000000u64;
        let view = HostRegion::owned(24).unwrap();
        view.with_native(|dest| unsafe {
            dest.as_ptr().add(8).cast::<u64>().write_unaligned(1);
            dest.as_ptr().add(16).cast::<u64>().write_unaligned(element);
        })
        .unwrap();
        registry.arrays.insert(
            token,
            Array {
                root: Root {
                    native: 0x1234usize as Object,
                    class: 0x2345usize as Object,
                    handle: 0x3456usize as Object,
                },
                elements: vec![element],
                view: view.clone(),
                published: false,
            },
        );
        let mut memory = Memory::default();
        registry.publish_array(&mut memory, token).unwrap();
        assert_eq!(registry.array_address(token, 8, 0).unwrap(), token + 32);
        assert!(registry.array_address(token, 4, 0).is_err());
        assert!(registry.array_address(token, 8, 1).is_err());
        assert!(registry.array_address(token + 8, 8, 0).is_err());
        assert_eq!(memory.read_int(token + 16, 8).unwrap(), 0);
        assert_eq!(memory.read_int(token + 24, 8).unwrap(), 1);
        assert_eq!(memory.read_int(token + 32, 8).unwrap(), element);
        assert!(memory.read_int(token, 8).is_err());
        assert!(memory.read_int(token + 40, 1).is_err());
        assert!(memory.write_int(token + 32, 8, 0).is_err());
        assert!(registry.retire_views(&mut memory).is_err());
        let (_, roots) = registry.retire_resources();
        assert_eq!(roots, vec![0x3456usize as Object]);
        registry.retire_views(&mut memory).unwrap();
        assert!(memory.read_int(token + 32, 8).is_err());
        assert!(view.with_native(|_| ()).is_err());
        assert!(registry.publish_array(&mut memory, token).is_err());
        assert!(registry.array_address(token, 8, 0).is_err());
        registry.retire_views(&mut memory).unwrap();
    }
    #[test]
    fn info_and_object_ownership_reject_forgery_double_free_and_retirement() {
        let mut registry = Registry::default();
        registry.infos.insert(0x6ffee0000000, info(0x1234));
        registry.objects.insert(
            0x6ffee8000000,
            Root {
                native: 0x2345usize as Object,
                class: 0x3456usize as Object,
                handle: 0x4567usize as Object,
            },
        );
        assert!(registry.info(0x6ffee0000001).is_err());
        assert!(registry.take_info(0x6ffee0000001).is_err());
        assert_eq!(
            registry.take_info(0x6ffee0000000).unwrap().native as usize,
            0x1234
        );
        assert!(registry.take_info(0x6ffee0000000).is_err());
        registry.infos.insert(0x6ffee0000010, info(0x5678));
        assert!(registry.contains(0x6ffee8000000));
        assert!(!registry.contains(0x6ffee8000001));
        let (infos, roots) = registry.retire_resources();
        assert_eq!(
            infos
                .iter()
                .map(|info| info.native as usize)
                .collect::<Vec<_>>(),
            vec![0x5678]
        );
        assert_eq!(roots, vec![0x4567usize as Object]);
        assert!(registry.objects[&0x6ffee8000000].handle.is_null());
        assert!(!registry.contains(0x6ffee8000000));
        assert!(registry.info(0x6ffee0000010).is_err());
        assert!(registry.take_info(0x6ffee0000010).is_err());
        let (infos, roots) = registry.retire_resources();
        assert!(infos.is_empty() && roots.is_empty());
    }
}
