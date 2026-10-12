//! Same-owner metadata and object reentry. Native handles remain in the existing
//! registries; ordinary IL calls and typed fields reuse their metadata contracts.
//! Guest data pointers never enter native Mono object storage.
use super::{
    animation, arrays, attributes, c_char, constructors, field_layouts, fields, handles, images,
    instances, managed_calls, metadata, objects, reference_arrays, reflection, string,
    value_layout, Cpu64, Memory, Object,
};
use std::ffi::CString;
#[cfg(test)]
use std::ptr;
macro_rules! sym {
    ($lib:expr, $name:expr, $ty:ty) => {{
        let name = CString::new($name).map_err(|e| e.to_string())?;
        let p = unsafe { libc::dlsym($lib, name.as_ptr()) };
        if p.is_null() {
            return Err(format!("missing nested metadata symbol: {name:?}"));
        }
        unsafe { std::mem::transmute::<Object, $ty>(p) }
    }};
}
pub(super) struct ObjectRoots<'a> {
    pub(super) api: instances::Api,
    pub(super) roots: &'a mut instances::Registry,
    pub(super) strings: objects::Api,
    pub(super) objects: &'a mut objects::Registry,
    pub(super) fields: &'a mut fields::Registry,
    pub(super) arrays: Option<(reference_arrays::Api, &'a mut reference_arrays::Registry)>,
    pub(super) attributes: (attributes::Api, &'a mut attributes::Registry),
    pub(super) reflection: (reflection::Api, &'a mut reflection::Registry),
    pub(super) empty_arrays: (arrays::Api, &'a mut arrays::Registry),
    pub(super) handles: (handles::Api, &'a mut handles::Registry),
}
pub(super) struct Metadata<'a> {
    pub(super) library: Object,
    pub(super) domain: Object,
    pub(super) thread_handle: Object,
    pub(super) metadata: &'a mut metadata::Registry,
    pub(super) images: &'a mut images::Registry,
    pub(super) rooted_instance: Option<(u64, Object)>,
    pub(super) objects: Option<ObjectRoots<'a>>,
}
pub(super) fn handles_allocation(function: &str) -> bool {
    matches!(
        function,
        "mono_array_new"
            | "mono_string_new"
            | "mono_string_new_len"
            | "mono_string_new_wrapper"
            | "mono_unity_string_empty_wrapper"
    )
}
pub(super) enum PreparedAllocation {
    Array(Box<reference_arrays::WorkerRequest>),
    EmptyArray {
        api: arrays::Api,
        roots: instances::Api,
        domain: Object,
        element: Object,
    },
    String {
        new: unsafe extern "C" fn(Object, *const c_char, u32) -> Object,
        roots: instances::Api,
        domain: Object,
        bytes: Vec<u8>,
        empty: Option<unsafe extern "C" fn() -> Object>,
    },
}
pub(super) enum Allocated {
    Array(reference_arrays::WorkerAllocation),
    EmptyArray(Object, instances::WorkerRoot),
    String(instances::WorkerRoot),
}
impl PreparedAllocation {
    /// Native allocation may expand the GC heap and call the original Unity
    /// profiler. No Metadata, Memory or Runtime RefCell is borrowed here.
    pub(super) fn execute(self) -> Result<Allocated, String> {
        match self {
            Self::Array(request) => Ok(Allocated::Array(request.execute()?)),
            Self::EmptyArray {
                api,
                roots,
                domain,
                element,
            } => {
                let native = unsafe { (api.new)(domain, element, 0) };
                Ok(Allocated::EmptyArray(element, roots.worker_root(native)?))
            }
            Self::String {
                new,
                roots,
                domain,
                bytes,
                empty,
            } => {
                let native = match empty {
                    Some(empty) => unsafe { empty() },
                    None => unsafe { new(domain, bytes.as_ptr().cast(), bytes.len() as u32) },
                };
                Ok(Allocated::String(roots.worker_root(native)?))
            }
        }
    }
}
impl Metadata<'_> {
    pub(super) fn prepare_allocation(
        &mut self,
        function: &str,
        cpu: &Cpu64,
        memory: &Memory,
    ) -> Result<PreparedAllocation, String> {
        let a = [cpu.regs[1], cpu.regs[2], cpu.regs[8]];
        let roots = self
            .objects
            .as_mut()
            .ok_or("nested allocation has no owned root service")?;
        if function == "mono_array_new" {
            if a[0] != super::ROOT_TOKEN {
                return Err("nested array allocation has unknown domain".into());
            }
            let element = self.metadata.native(a[1], metadata::Kind::Class)?;
            if a[2] == 0 {
                let image = sym!(
                    self.library,
                    "mono_class_get_image",
                    unsafe extern "C" fn(Object) -> Object
                );
                if !self
                    .images
                    .live_native_images()?
                    .contains(&unsafe { image(element) })
                {
                    return Err("nested empty array class has no live image owner".into());
                }
                super::closed_types::Api::load(self.library)?.concrete(element)?;
                return Ok(PreparedAllocation::EmptyArray {
                    api: roots.empty_arrays.0,
                    roots: roots.api,
                    domain: self.domain,
                    element,
                });
            }
            let (api, arrays) = roots
                .arrays
                .as_mut()
                .ok_or("nested typed array service absent")?;
            if !api.supports(element) {
                return Err("nested array element outside owned typed family".into());
            }
            return Ok(PreparedAllocation::Array(Box::new(arrays.prepare_worker(
                *api,
                roots.api,
                self.domain,
                element,
                a[2],
            )?)));
        }
        if !handles_allocation(function) {
            return Err("untyped nested allocation".into());
        }
        let empty = function == "mono_unity_string_empty_wrapper";
        let wrapper = function == "mono_string_new_wrapper";
        if !wrapper && !empty && a[0] != super::ROOT_TOKEN {
            return Err("nested String allocation has unknown domain".into());
        }
        let bytes = if empty {
            Vec::new()
        } else if function == "mono_string_new_len" {
            if a[2] > 32768 {
                return Err("nested String length exceeds bound".into());
            }
            let mut bytes = vec![0; a[2] as usize];
            memory.read(a[1], &mut bytes)?;
            bytes
        } else {
            string(memory, if wrapper { a[0] } else { a[1] })?.into_bytes()
        };
        if bytes.len() > 32768 {
            return Err("nested String input exceeds bound".into());
        }
        let empty = if empty {
            let current = sym!(
                self.library,
                "mono_domain_get",
                unsafe extern "C" fn() -> Object
            );
            if unsafe { current() } != self.domain {
                return Err("nested empty string current domain mismatch".into());
            }
            Some(sym!(
                self.library,
                "mono_unity_string_empty_wrapper",
                unsafe extern "C" fn() -> Object
            ))
        } else {
            None
        };
        Ok(PreparedAllocation::String {
            new: roots.strings.new_string,
            roots: roots.api,
            domain: self.domain,
            bytes,
            empty,
        })
    }
    pub(super) fn finish_allocation(
        &mut self,
        allocation: Allocated,
        memory: &mut Memory,
    ) -> Result<u64, String> {
        let roots = self
            .objects
            .as_mut()
            .ok_or("nested allocation completion has no owned root service")?;
        match allocation {
            Allocated::Array(allocation) => {
                let (api, arrays) = roots
                    .arrays
                    .as_mut()
                    .ok_or("nested typed array completion service absent")?;
                let token = arrays.adopt_worker(*api, roots.api, roots.roots, allocation)?;
                arrays.publish(memory, token)?;
                Ok(token)
            }
            Allocated::EmptyArray(element, pin) => {
                let native = pin.target()?;
                let token =
                    roots
                        .empty_arrays
                        .1
                        .root_empty(roots.empty_arrays.0, native, element)?;
                roots.roots.adopt_empty_array(roots.api, token, native)?;
                roots.empty_arrays.1.publish(memory, token)?;
                Ok(token)
            }
            Allocated::String(pin) => {
                let token = roots.objects.borrow_reference(
                    roots.strings,
                    roots.api,
                    roots.roots,
                    pin.target()?,
                )?;
                roots.objects.publish_pending(memory)?;
                Ok(token)
            }
        }
    }
    pub(super) fn root_reflection(
        &mut self,
        native: Object,
        memory: &mut Memory,
    ) -> Result<u64, String> {
        let roots = self
            .objects
            .as_mut()
            .ok_or("reflection callback has no owned object service")?;
        let (token, ty) = roots.reflection.1.root_type(roots.reflection.0, native)?;
        let ty = self
            .metadata
            .wrap(memory, metadata::Value::Handle(ty, metadata::Kind::Type))?;
        roots.reflection.1.publish(memory, token, ty)?;
        Ok(token)
    }
    pub(super) fn root_reference(
        &mut self,
        native: Object,
        memory: &mut Memory,
    ) -> Result<u64, String> {
        let roots = self
            .objects
            .as_mut()
            .ok_or("reference conversion has no owned service")?;
        let token =
            roots
                .objects
                .borrow_reference(roots.strings, roots.api, roots.roots, native)?;
        roots.objects.publish_pending(memory)?;
        Ok(token)
    }
    pub(super) fn rooted_native(&self, token: u64) -> Result<Object, String> {
        if token == 0 {
            return Ok(std::ptr::null_mut());
        }
        let roots = self
            .objects
            .as_ref()
            .ok_or("nested object query has no root service")?;
        if roots.reflection.1.contains(token) {
            roots.reflection.1.native(roots.reflection.0, token)
        } else if roots.empty_arrays.1.contains(token) {
            roots.empty_arrays.1.native(roots.empty_arrays.0, token)
        } else if roots.attributes.1.contains(token) {
            roots.attributes.1.native(roots.attributes.0, token)
        } else {
            roots.roots.native(roots.api, token)
        }
    }
    pub(super) fn root_blittable_array(
        &mut self,
        native: Object,
        memory: &mut Memory,
    ) -> Result<(u64, usize, usize), String> {
        self.refresh_native(memory)?;
        let roots = self
            .objects
            .as_mut()
            .ok_or("GPU data array has no owned root service")?;
        let (api, arrays) = roots
            .arrays
            .as_mut()
            .ok_or("GPU data array has no typed array service")?;
        let (length, width) = api.blittable_array(native)?;
        let token =
            arrays.borrow_with(*api, roots.api, roots.roots, native, |registry, native| {
                roots
                    .objects
                    .borrow_reference(roots.strings, roots.api, registry, native)
            })?;
        arrays.publish(memory, token)?;
        Ok((token, length, width))
    }
    pub(super) fn native_handle(&self, token: u64) -> Result<Object, String> {
        let roots = self
            .objects
            .as_ref()
            .ok_or("native handle conversion has no object service")?;
        roots.handles.1.native_handle(roots.handles.0, token)
    }
    pub(super) fn sync_guest(&mut self, memory: &Memory) -> Result<(), String> {
        if let Some(roots) = self.objects.as_mut() {
            roots.roots.sync_from_guest(roots.api, memory)?;
            roots.fields.sync(roots.api, roots.roots)?;
            if let Some((api, arrays)) = roots.arrays.as_mut() {
                arrays.sync(*api, roots.api, roots.roots, memory)?;
            }
        }
        Ok(())
    }
    fn refresh_native(&mut self, memory: &mut Memory) -> Result<(), String> {
        let Some(roots) = self.objects.as_mut() else {
            return Ok(());
        };
        let fields =
            roots
                .fields
                .prepare_refresh(roots.api, roots.roots, None, |registry, native| {
                    roots
                        .objects
                        .borrow_reference(roots.strings, roots.api, registry, native)
                })?;
        let arrays = if let Some((api, arrays)) = roots.arrays.as_ref() {
            Some(
                arrays.prepare_refresh(*api, roots.api, roots.roots, |registry, native| {
                    roots
                        .objects
                        .borrow_reference(roots.strings, roots.api, registry, native)
                })?,
            )
        } else {
            None
        };
        let mut references = fields.references.clone();
        if let Some(arrays) = &arrays {
            references.extend_from_slice(&arrays.references);
        }
        roots.objects.publish_pending(memory)?;
        roots.fields.commit_refresh(fields)?;
        if let (Some(plan), Some((_, arrays))) = (arrays, roots.arrays.as_mut()) {
            arrays.commit_refresh(plan)?;
        }
        if !references.is_empty() {
            self.publish_container_graph(memory, references)?;
        }
        Ok(())
    }
    /// Publish backing arrays/serialized children before returning to engine
    /// code which may read them directly. Root identity and actual metadata
    /// govern this bounded graph; native headers/pointers never become views.
    fn publish_container_graph(
        &mut self,
        memory: &mut Memory,
        mut queue: Vec<Object>,
    ) -> Result<(), String> {
        let object_class = sym!(
            self.library,
            "mono_object_get_class",
            unsafe extern "C" fn(Object) -> Object
        );
        let rank = sym!(
            self.library,
            "mono_class_get_rank",
            unsafe extern "C" fn(Object) -> u32
        );
        let image = sym!(
            self.library,
            "mono_class_get_image",
            unsafe extern "C" fn(Object) -> Object
        );
        let core = self.images.unique_native_image(super::CORE_MODULE)?;
        let corlib = self.images.unique_native_image(super::CORELIB)?;
        let images = self.images.live_native_images()?;
        let roots = self
            .objects
            .as_mut()
            .ok_or("nested container graph has no owned root service")?;
        let mut seen = std::collections::BTreeSet::new();
        while let Some(native) = queue.pop() {
            if native.is_null() || !seen.insert(native as usize) {
                continue;
            }
            if seen.len() > 4096 {
                return Err("nested container graph exceeds bounded capacity".into());
            }
            let token =
                roots
                    .objects
                    .borrow_reference(roots.strings, roots.api, roots.roots, native)?;
            if roots.objects.contains(token) {
                continue;
            }
            let class = unsafe { object_class(native) };
            if unsafe { rank(class) } != 0 {
                if roots.empty_arrays.1.contains(token) {
                    continue;
                }
                let (api, arrays) = roots
                    .arrays
                    .as_mut()
                    .ok_or("nested container array service absent")?;
                let token = arrays.borrow_with(
                    *api,
                    roots.api,
                    roots.roots,
                    native,
                    |registry, native| {
                        roots
                            .objects
                            .borrow_reference(roots.strings, roots.api, registry, native)
                    },
                )?;
                arrays.publish(memory, token)?;
                queue.extend(arrays.native_references(*api, roots.api, roots.roots, token)?);
                continue;
            }
            if roots.api.engine_class(class) && !roots.fields.has_owner(token) {
                continue;
            }
            if !images.contains(&unsafe { image(class) }) {
                return Err("nested container class has no live image owner".into());
            }
            if !roots.fields.has_owner(token) {
                let specs = if let Some(specs) =
                    field_layouts::storage_specs(self.library, core, corlib, native, class)?
                {
                    specs
                } else {
                    field_layouts::specs(self.library, core, &images, native, class)?
                };
                roots.fields.install_with(
                    self.library,
                    roots.api,
                    roots.roots,
                    memory,
                    specs,
                    |registry, native| {
                        roots
                            .objects
                            .borrow_reference(roots.strings, roots.api, registry, native)
                    },
                )?;
            }
            queue.extend(
                roots
                    .fields
                    .native_references(roots.api, roots.roots, token)?,
            );
        }
        roots.objects.publish_pending(memory)
    }
    pub(super) fn dispatch(
        &mut self,
        function: &str,
        cpu: &mut Cpu64,
        memory: &mut Memory,
        stack: (u64, u64),
    ) -> Result<(), String> {
        if function == "mono_domain_get" {
            let get = sym!(
                self.library,
                "mono_domain_get",
                unsafe extern "C" fn() -> Object
            );
            if self.domain.is_null() || unsafe { get() } != self.domain {
                return Err("nested domain identity mismatch".into());
            }
            return cpu.return_integer(memory, super::ROOT_TOKEN);
        }
        if function == "mono_thread_current" {
            if self.thread_handle.is_null()
                || memory.read_int(
                    cpu.gs_base.checked_add(0x48).ok_or("nested TEB overflow")?,
                    8,
                )? != 1
            {
                return Err("nested thread query requires the retained main-thread owner".into());
            }
            let current = sym!(self.library, function, unsafe extern "C" fn() -> Object);
            let target = sym!(
                self.library,
                "mono_gchandle_get_target_v2",
                unsafe extern "C" fn(Object) -> Object
            );
            let thread = unsafe { current() };
            if thread.is_null() || unsafe { target(self.thread_handle) } != thread {
                return Err("nested main-thread root identity changed".into());
            }
            return cpu.return_integer(memory, super::THREAD_TOKEN);
        }
        if function == "mono_object_new" && self.objects.is_some() {
            if cpu.regs[1] != super::ROOT_TOKEN {
                return Err("nested allocation has unknown domain".into());
            }
            let class = self.metadata.native(cpu.regs[2], metadata::Kind::Class)?;
            let image = sym!(
                self.library,
                "mono_class_get_image",
                unsafe extern "C" fn(Object) -> Object
            );
            let images = self.images.live_native_images()?;
            if !images.contains(&unsafe { image(class) }) {
                return Err("nested allocation class outside owned images".into());
            }
            super::closed_types::Api::load(self.library)?.concrete(class)?;
            let value = sym!(
                self.library,
                "mono_class_is_valuetype",
                unsafe extern "C" fn(Object) -> i32
            );
            let layout = if unsafe { value(class) } != 0 {
                Some(
                    value_layout::Api::load(self.library, unsafe { image(class) })?
                        .layout(class)?,
                )
            } else {
                None
            };
            let roots = self
                .objects
                .as_mut()
                .ok_or("nested allocation has no owned root service")?;
            let token = if let Some(layout) = &layout {
                roots
                    .roots
                    .allocate_boxed(roots.api, self.domain, class, layout.width)?
            } else {
                roots.roots.allocate(roots.api, self.domain, class)?
            };
            roots.roots.publish(memory, token)?;
            let native = roots.roots.native(roots.api, token)?;
            let specs = if layout.is_some() {
                Vec::new()
            } else if roots.api.engine_class(class) {
                field_layouts::specs(
                    self.library,
                    self.images.unique_native_image(super::CORE_MODULE)?,
                    &images,
                    native,
                    class,
                )?
            } else {
                field_layouts::reference_storage_specs(
                    self.library,
                    self.images.unique_native_image(super::CORE_MODULE)?,
                    &images,
                    native,
                    class,
                )?
            };
            roots.fields.install_with(
                self.library,
                roots.api,
                roots.roots,
                memory,
                specs,
                |registry, native| {
                    roots
                        .objects
                        .borrow_reference(roots.strings, roots.api, registry, native)
                },
            )?;
            roots.objects.publish_pending(memory)?;
            if let Some(layout) = layout {
                roots.fields.publish_boxed_payload(
                    roots.api,
                    roots.roots,
                    memory,
                    token,
                    layout,
                )?;
            }
            return cpu.return_integer(memory, token);
        }
        if function == "mono_object_unbox" && self.objects.is_some() {
            let token = cpu.regs[1];
            let roots = self
                .objects
                .as_mut()
                .ok_or("nested unbox has no owned service")?;
            if !roots.roots.is_boxed(token) {
                return Err("nested unbox requires a live allocated box".into());
            }
            let class = roots.roots.class(roots.api, token)?;
            let image = sym!(
                self.library,
                "mono_class_get_image",
                unsafe extern "C" fn(Object) -> Object
            );
            let image = unsafe { image(class) };
            if !self.images.live_native_images()?.contains(&image) {
                return Err("nested box class outside owned image family".into());
            }
            let layout = value_layout::Api::load(self.library, image)?.layout(class)?;
            roots.fields.sync(roots.api, roots.roots)?;
            let address =
                roots
                    .fields
                    .unbox(self.library, roots.api, roots.roots, memory, token, layout)?;
            return cpu.return_integer(memory, address);
        }
        if self.objects.is_some()
            && matches!(
                function,
                "mono_gchandle_new_v2"
                    | "mono_gchandle_new_weakref_v2"
                    | "mono_gchandle_get_target_v2"
                    | "mono_gchandle_free_v2"
            )
        {
            let a = [cpu.regs[1], cpu.regs[2]];
            let native = if matches!(
                function,
                "mono_gchandle_new_v2" | "mono_gchandle_new_weakref_v2"
            ) {
                self.rooted_native(a[0])?
            } else {
                std::ptr::null_mut()
            };
            let roots = self
                .objects
                .as_mut()
                .ok_or("nested handles have no object service")?;
            let (api, handles) = &mut roots.handles;
            let result = match function {
                "mono_gchandle_new_v2" => handles.new_handle(*api, native, a[0], a[1])?,
                "mono_gchandle_new_weakref_v2" => {
                    handles.new_weak_handle(*api, native, a[0], a[1])?
                }
                "mono_gchandle_get_target_v2" => handles.target(*api, a[0])?,
                "mono_gchandle_free_v2" => {
                    handles.retire_view(memory, a[0])?;
                    handles.free(*api, a[0])?;
                    0
                }
                _ => unreachable!(),
            };
            if matches!(
                function,
                "mono_gchandle_new_v2" | "mono_gchandle_new_weakref_v2"
            ) {
                handles.publish(memory, result)?;
            }
            return cpu.return_integer(memory, result);
        }
        if function == "mono_object_get_class" {
            let class = if self.objects.is_some() {
                let native = self.rooted_native(cpu.regs[1])?;
                if native.is_null() {
                    return Err("nested class query has NULL object".into());
                }
                let get = sym!(
                    self.library,
                    "mono_object_get_class",
                    unsafe extern "C" fn(Object) -> Object
                );
                unsafe { get(native) }
            } else {
                let (token, class) = self
                    .rooted_instance
                    .ok_or("nested object query has no owned instance")?;
                if cpu.regs[1] != token {
                    return Err(
                        "nested object query requires the owned constructor instance".into(),
                    );
                }
                class
            };
            let result = self.metadata.wrap(
                memory,
                metadata::Value::Handle(class, metadata::Kind::Class),
            )?;
            if self.objects.is_some() {
                self.metadata
                    .publish_class_elements(self.library, memory, result)?;
            }
            return cpu.return_integer(memory, result);
        }
        if function == "mono_reflection_type_get_type" && self.objects.is_some() {
            let native = self.rooted_native(cpu.regs[1])?;
            let roots = self
                .objects
                .as_ref()
                .ok_or("nested reflection service absent")?;
            if !roots.reflection.1.contains(cpu.regs[1]) {
                return Err("nested reflection input is not a RuntimeType".into());
            }
            let ty = unsafe { (roots.reflection.0.get_type)(native) };
            let token = self
                .metadata
                .wrap(memory, metadata::Value::Handle(ty, metadata::Kind::Type))?;
            return cpu.return_integer(memory, token);
        }
        if matches!(
            function,
            "mono_custom_attrs_from_class"
                | "mono_custom_attrs_from_field"
                | "mono_custom_attrs_has_attr"
                | "mono_custom_attrs_get_attr"
                | "mono_custom_attrs_free"
        ) {
            let a = [cpu.regs[1], cpu.regs[2]];
            let roots = self
                .objects
                .as_mut()
                .ok_or("nested attributes have no object service")?;
            let (api, attrs) = &mut roots.attributes;
            let result = match function {
                "mono_custom_attrs_from_class" | "mono_custom_attrs_from_field" => {
                    let class = self.metadata.native(a[0], metadata::Kind::Class)?;
                    let field = if function == "mono_custom_attrs_from_field" {
                        Some(self.metadata.native(a[1], metadata::Kind::Field)?)
                    } else {
                        None
                    };
                    attrs.query(*api, class, field)?
                }
                "mono_custom_attrs_has_attr" => attrs.has(
                    *api,
                    a[0],
                    self.metadata.native(a[1], metadata::Kind::Class)?,
                )?,
                "mono_custom_attrs_get_attr" => attrs.get(
                    *api,
                    a[0],
                    self.metadata.native(a[1], metadata::Kind::Class)?,
                )?,
                "mono_custom_attrs_free" => {
                    attrs.free(*api, a[0])?;
                    0
                }
                _ => unreachable!(),
            };
            return cpu.return_integer(memory, result);
        }
        if self.objects.is_some()
            && matches!(
                function,
                "mono_array_new"
                    | "mono_array_length"
                    | "mono_array_addr_with_size"
                    | "mono_gc_wbarrier_set_arrayref"
            )
        {
            let a = [cpu.regs[1], cpu.regs[2], cpu.regs[8]];
            let roots = self.objects.as_mut().ok_or("nested array service absent")?;
            let result = match function {
                "mono_array_new" => {
                    if a[0] != super::ROOT_TOKEN {
                        return Err("nested array allocation has unknown domain".into());
                    }
                    let element = self.metadata.native(a[1], metadata::Kind::Class)?;
                    if a[2] == 0 {
                        // Empty arrays have no element payload. Match the outer
                        // route with concrete metadata and a live image owner.
                        let image = sym!(
                            self.library,
                            "mono_class_get_image",
                            unsafe extern "C" fn(Object) -> Object
                        );
                        if !self
                            .images
                            .live_native_images()?
                            .contains(&unsafe { image(element) })
                        {
                            return Err("nested empty array class has no live image owner".into());
                        }
                        super::closed_types::Api::load(self.library)?.concrete(element)?;
                        let native = unsafe { (roots.empty_arrays.0.new)(self.domain, element, 0) };
                        let token = roots.empty_arrays.1.root_empty(
                            roots.empty_arrays.0,
                            native,
                            element,
                        )?;
                        roots.roots.adopt_empty_array(roots.api, token, native)?;
                        roots.empty_arrays.1.publish(memory, token)?;
                        token
                    } else {
                        let (api, arrays) = roots
                            .arrays
                            .as_mut()
                            .ok_or("nested typed array service absent")?;
                        if !api.supports(element) {
                            return Err("nested array element outside owned typed family".into());
                        }
                        let token = arrays.allocate(
                            *api,
                            roots.api,
                            roots.roots,
                            self.domain,
                            element,
                            a[2],
                        )?;
                        arrays.publish(memory, token)?;
                        token
                    }
                }
                "mono_array_length" => {
                    if roots.empty_arrays.1.contains(a[0]) {
                        roots.empty_arrays.1.native(roots.empty_arrays.0, a[0])?;
                        0
                    } else {
                        let (api, arrays) = roots
                            .arrays
                            .as_ref()
                            .ok_or("nested typed array service absent")?;
                        arrays.length(*api, roots.api, roots.roots, a[0])?
                    }
                }
                "mono_array_addr_with_size" => roots
                    .arrays
                    .as_ref()
                    .ok_or("nested typed array service absent")?
                    .1
                    .address(a[0], a[1], a[2])?,
                "mono_gc_wbarrier_set_arrayref" => {
                    let (api, arrays) = roots
                        .arrays
                        .as_mut()
                        .ok_or("nested typed array service absent")?;
                    arrays.store(*api, roots.api, roots.roots, memory, a)?;
                    0
                }
                _ => unreachable!(),
            };
            return cpu.return_integer(memory, result);
        }
        if self.objects.is_some()
            && matches!(
                function,
                "mono_string_new_wrapper"
                    | "mono_unity_string_empty_wrapper"
                    | "mono_string_new"
                    | "mono_string_new_len"
                    | "mono_string_length"
                    | "mono_string_chars"
                    | "mono_field_get_value"
                    | "mono_field_set_value"
                    | "mono_object_isinst"
            )
        {
            return self.dispatch_object(function, cpu, memory, stack);
        }
        if function == "mono_gc_wbarrier_set_field" {
            memory.read_int(cpu.regs[4], 8)?;
            let a = [cpu.regs[1], cpu.regs[2], cpu.regs[8]];
            if let Some(roots) = self.objects.as_mut() {
                if let Some((api, arrays)) = roots.arrays.as_mut() {
                    if arrays.has_reference(a[0], a[1]) {
                        arrays.store(*api, roots.api, roots.roots, memory, a)?;
                        return cpu.return_integer(memory, 0);
                    }
                }
                // A constructor can create a referenced ordinary object in
                // native IL before the engine first accesses its fields. A
                // live root permits metadata discovery, never an arbitrary
                // heap write or an early Unity cached-pointer publication.
                if !roots.fields.has_reference(a[0], a[1]) {
                    if let Some(owner) = roots.roots.interior_owner(a[1]) {
                        if a[0] == 0 || a[0] == owner {
                            let class = roots.roots.class(roots.api, owner)?;
                            if !roots.api.engine_class(class) && !roots.roots.is_boxed(owner) {
                                let native = roots.roots.native(roots.api, owner)?;
                                let specs = field_layouts::reference_storage_specs(
                                    self.library,
                                    self.images.unique_native_image(super::CORE_MODULE)?,
                                    &self.images.live_native_images()?,
                                    native,
                                    class,
                                )?;
                                roots.fields.install_with(
                                    self.library,
                                    roots.api,
                                    roots.roots,
                                    memory,
                                    specs,
                                    |registry, native| {
                                        roots.objects.borrow_reference(
                                            roots.strings,
                                            roots.api,
                                            registry,
                                            native,
                                        )
                                    },
                                )?;
                                roots.objects.publish_pending(memory)?;
                            }
                        }
                    }
                }
                if roots.fields.has_reference(a[0], a[1]) {
                    roots.fields.store_reference(
                        roots.api,
                        roots.roots,
                        memory,
                        a[0],
                        a[1],
                        a[2],
                    )?;
                    let owner = roots
                        .roots
                        .interior_owner(a[1])
                        .ok_or("reference barrier owner vanished")?;
                    let native = roots.roots.native(roots.api, owner)?;
                    self.publish_container_graph(memory, vec![native])?;
                    return cpu.return_integer(memory, 0);
                }
            }
            if a[0] != 0 {
                return Err("nested stack barrier requires NULL owner".into());
            }
            if a[2] != 0 && !(a[2] == super::THREAD_TOKEN && !self.thread_handle.is_null()) {
                if self.objects.is_some() {
                    self.rooted_native(a[2])?;
                } else if self.rooted_instance.is_none_or(|(token, _)| a[2] != token) {
                    return Err("nested barrier value has no owned root identity".into());
                }
            }
            checked_output(memory, cpu, stack, a[1], 8)?;
            memory.write_int(a[1], 8, a[2])?;
            return cpu.return_integer(memory, 0);
        }
        if !metadata::handles(function) {
            return Err(format!(
                "nested Mono operation outside metadata route: {function}"
            ));
        }
        let a = [cpu.regs[1], cpu.regs[2], cpu.regs[8]];
        memory.read_int(cpu.regs[4], 8)?;
        cpu.regs[4]
            .checked_add(8)
            .ok_or("nested metadata return overflow")?;
        let mut iterator_output = None;
        let value = match function {
            "mono_class_get_userdata" | "mono_class_set_userdata" => {
                metadata::Value::Scalar(self.metadata.userdata(
                    memory,
                    a[0],
                    (function == "mono_class_set_userdata").then_some(a[1]),
                )?)
            }
            "mono_get_corlib" => {
                let get = sym!(self.library, function, unsafe extern "C" fn() -> Object);
                self.metadata.note_query();
                metadata::Value::Scalar(self.images.borrow_registered(unsafe { get() })?)
            }
            "mono_class_get_image" => {
                let class = self.metadata.native(a[0], metadata::Kind::Class)?;
                let get = sym!(
                    self.library,
                    function,
                    unsafe extern "C" fn(Object) -> Object
                );
                self.metadata.note_query();
                metadata::Value::Scalar(self.images.borrow_registered(unsafe { get(class) })?)
            }
            "mono_class_from_name" => {
                let image = self.images.native_image(a[0])?;
                self.metadata.class_from_name(
                    self.library,
                    image,
                    string(memory, a[1])?,
                    string(memory, a[2])?,
                )?
            }
            "mono_class_get_field_from_name" | "mono_class_get_method_from_name" => {
                let class = self.metadata.native(a[0], metadata::Kind::Class)?;
                let name = string(memory, a[1])?;
                self.metadata.note_query();
                if function == "mono_class_get_field_from_name" {
                    let get = sym!(
                        self.library,
                        function,
                        unsafe extern "C" fn(Object, *const c_char) -> Object
                    );
                    metadata::Value::Handle(
                        unsafe { get(class, name.as_ptr()) },
                        metadata::Kind::Field,
                    )
                } else {
                    let argc = a[2] as u32 as i32;
                    if !(-1..=16).contains(&argc) {
                        return Err("nested method lookup arity outside contract".into());
                    }
                    let get = sym!(
                        self.library,
                        function,
                        unsafe extern "C" fn(Object, *const c_char, i32) -> Object
                    );
                    metadata::Value::Handle(
                        unsafe { get(class, name.as_ptr(), argc) },
                        metadata::Kind::Method,
                    )
                }
            }
            "mono_class_get_methods"
            | "mono_class_get_fields"
            | "mono_class_get_nested_types"
            | "mono_class_get_interfaces"
            | "mono_signature_get_params" => {
                memory.check(
                    a[1],
                    8,
                    crate::x64::memory::READ | crate::x64::memory::WRITE,
                )?;
                let iterator = memory.read_int(a[1], 8)?;
                let (value, next) =
                    self.metadata
                        .iterate(self.library, function, a[0], iterator)?;
                iterator_output = Some((a[1], next));
                value
            }
            "mono_image_get_name" | "mono_image_get_table_rows" => {
                let image = self.images.native_image(a[0])?;
                self.metadata
                    .query_image(self.library, function, image, a[1])?
            }
            _ => self
                .metadata
                .query(self.library, function, a[0], a[1], a[2])?,
        };
        let result = self.metadata.wrap(memory, value)?;
        if let Some((address, iterator)) = iterator_output {
            memory.write_int(address, 8, iterator)?;
        }
        cpu.return_integer(memory, result)
    }
}
/// Preparation borrows the registries and checked guest memory. Execution owns
/// only stable native metadata and owned ABI data, so GC and guest reentry
/// can run without an outstanding Memory/registry borrow.
pub(super) struct PreparedInvoke {
    library: Object,
    method: Object,
    image: Object,
    step: Option<usize>,
    ordinary: Option<managed_calls::Descriptor>,
    constructor: Option<(Object, Object, u64)>,
    receiver: Object,
    frame: Option<managed_calls::Frame>,
    input: Option<super::input_events::Prepared>,
    stack: (u64, u64),
    rsp: u64,
    pub(super) exception_slot: u64,
}
impl Metadata<'_> {
    pub(super) fn prepare_invoke(
        &mut self,
        cpu: &Cpu64,
        memory: &Memory,
        runtime: &crate::x64::runtime::Runtime64,
        stack: (u64, u64),
    ) -> Result<PreparedInvoke, String> {
        memory.read_int(cpu.regs[4], 8)?;
        cpu.regs[4]
            .checked_add(8)
            .ok_or("nested invoke return overflow")?;
        let exception_slot = cpu.regs[9];
        if exception_slot != 0 {
            checked_output(memory, cpu, stack, exception_slot, 8)?;
        }
        let method = self.metadata.native(cpu.regs[1], metadata::Kind::Method)?;
        eprintln!(
            "NATIVE_MONO_NESTED_INVOKE: {}",
            self.metadata.describe_method(self.library, cpu.regs[1])?
        );
        let image = self.images.unique_native_image(super::SUBSYSTEMS_MODULE)?;
        self.sync_guest(memory)?;
        if let Some(input) =
            super::input_events::prepare(method, memory, runtime, cpu.regs[8], |address, width| {
                checked_output(memory, cpu, stack, address, 1).is_ok()
                    && address
                        .checked_add(width as u64)
                        .is_some_and(|end| end <= stack.1)
            })?
        {
            if cpu.regs[2] != 0
                || (exception_slot != 0
                    && input.output_address() < exception_slot + 8
                    && exception_slot < input.output_address() + 20)
            {
                return Err("input callback receiver/exception output mismatch".into());
            }
            return Ok(PreparedInvoke {
                library: self.library,
                method,
                image,
                step: None,
                ordinary: None,
                constructor: None,
                receiver: std::ptr::null_mut(),
                frame: None,
                input: Some(input),
                stack,
                rsp: cpu.regs[4],
                exception_slot,
            });
        }
        if cpu.regs[2] != 0 {
            let mut images = self.images.live_native_images()?;
            let core = self.images.unique_native_image(super::CORE_MODULE)?;
            images.retain(|&image| image != core);
            images.insert(0, core);
            let api = constructors::Api::load(self.library)?;
            let object = self.rooted_native(cpu.regs[2])?;
            let get_class = sym!(
                self.library,
                "mono_object_get_class",
                unsafe extern "C" fn(Object) -> Object
            );
            let constructor = api.describe(method, &images)?;
            let ordinary = if let Some(descriptor) = constructor.as_ref() {
                api.receiver(descriptor, object, unsafe { get_class(object) })?;
                None
            } else {
                let calls = managed_calls::Api::load(self.library)?;
                let descriptor = calls
                    .describe(method, &images, &images)?
                    .ok_or("nested instance entry is not metadata-qualified ordinary IL")?;
                let declared = descriptor
                    .receiver
                    .ok_or("nested instance method is static")?;
                calls.receiver(object, declared, &images)?;
                if matches!(
                    descriptor.returns,
                    Some(managed_calls::Shape::Scalar { .. })
                ) {
                    return Err(
                        "nested ordinary instance entry requires void/reference return ABI".into(),
                    );
                }
                Some(descriptor)
            };
            self.sync_guest(memory)?;
            let frame = ordinary
                .as_ref()
                .map(|descriptor| self.prepare_frame(descriptor, cpu, memory, stack))
                .transpose()?;
            return Ok(PreparedInvoke {
                library: self.library,
                method,
                image,
                step: None,
                ordinary,
                constructor: constructor.map(|_| (core, object, cpu.regs[2])),
                receiver: object,
                frame,
                input: None,
                stack,
                rsp: cpu.regs[4],
                exception_slot,
            });
        }
        let (step, ordinary) =
            if super::invoke::validate_classlib_init(self.library, method, image, 5).is_ok() {
                (Some(5), None)
            } else if super::invoke::validate_classlib_init(self.library, method, image, 6).is_ok()
            {
                (Some(6), None)
            } else {
                if self.objects.is_none() {
                    return Err("nested ordinary invoke has no owned object service".into());
                }
                let images = self.images.live_native_images()?;
                let descriptor = managed_calls::Api::load(self.library)?
                    .describe(method, &images, &images)?
                    .ok_or("nested ordinary entry is not admitted IL")?;
                if descriptor.receiver.is_some()
                    || matches!(
                        descriptor.returns,
                        Some(managed_calls::Shape::Scalar { .. })
                    )
                {
                    return Err(
                        "nested ordinary static invoke requires void/reference return ABI".into(),
                    );
                }
                (None, Some(descriptor))
            };
        self.sync_guest(memory)?;
        let frame = ordinary
            .as_ref()
            .map(|descriptor| self.prepare_frame(descriptor, cpu, memory, stack))
            .transpose()?;
        self.metadata.note_query();
        Ok(PreparedInvoke {
            library: self.library,
            method,
            image,
            step,
            ordinary,
            constructor: None,
            receiver: std::ptr::null_mut(),
            frame,
            input: None,
            stack,
            rsp: cpu.regs[4],
            exception_slot,
        })
    }
    fn prepare_frame(
        &self,
        descriptor: &managed_calls::Descriptor,
        cpu: &Cpu64,
        memory: &Memory,
        stack: (u64, u64),
    ) -> Result<managed_calls::Frame, String> {
        let calls = managed_calls::Api::load(self.library)?;
        let frame = managed_calls::Frame::prepare(
            memory,
            descriptor,
            cpu.regs[8],
            |token, shape| calls.compatible(self.rooted_native(token)?, shape),
            |address, width| checked_output(memory, cpu, stack, address, width).is_ok(),
        )?;
        if cpu.regs[9] != 0
            && frame.outputs()?.iter().any(|out| {
                out.address < cpu.regs[9] + 8
                    && cpu.regs[9] < out.address + out.shape.width() as u64
            })
        {
            return Err("nested exception slot overlaps byref output".into());
        }
        Ok(frame)
    }
}
impl PreparedInvoke {
    fn commit_outputs(
        &self,
        memory: &mut Memory,
        outputs: &[(u64, usize, u64)],
    ) -> Result<(), String> {
        for &(address, width, _) in outputs {
            checked_output_at(memory, self.rsp, self.stack, address, width)?;
        }
        if self.exception_slot != 0 {
            checked_output_at(memory, self.rsp, self.stack, self.exception_slot, 8)?;
        }
        for &(address, width, word) in outputs {
            memory.write_int(address, width, word)?;
        }
        if self.exception_slot != 0 {
            memory.write_int(self.exception_slot, 8, 0)?;
        }
        Ok(())
    }
    pub(super) fn execute(&mut self) -> Result<Object, String> {
        if let Some(input) = self.input.as_mut() {
            return input.execute();
        }
        if let Some((core, object, token)) = self.constructor {
            animation::execute_nested_managed(self.library, core, self.method, object, token)?;
            eprintln!("NATIVE_MONO_NESTED_CONSTRUCTOR_VERIFIED: receiver={token:#x}; metadata-qualified=true");
            return Ok(std::ptr::null_mut());
        }
        let invoke = sym!(
            self.library,
            "mono_runtime_invoke",
            unsafe extern "C" fn(Object, Object, *mut Object, *mut Object) -> Object
        );
        let mut parameters = self
            .frame
            .as_mut()
            .map(|frame| frame.pointers())
            .unwrap_or_default();
        let mut exception = std::ptr::null_mut();
        let result = unsafe {
            invoke(
                self.method,
                self.receiver,
                if parameters.is_empty() {
                    std::ptr::null_mut()
                } else {
                    parameters.as_mut_ptr()
                },
                &mut exception,
            )
        };
        if !exception.is_null() {
            return Err("nested IL invoke raised an exception".into());
        }
        if let Some(step) = self.step {
            if !result.is_null() {
                return Err("nested subsystem clear returned an object".into());
            }
            super::invoke::verify_subsystems(self.library, self.image, step == 6)?;
            eprintln!(
                "NATIVE_MONO_NESTED_INVOKE_VERIFIED: {} static-void=true",
                if step == 5 {
                    "SubsystemDescriptorStore.ClearManagedDescriptors"
                } else {
                    "SubsystemManager.ClearSubsystems"
                }
            );
        } else {
            let descriptor = self
                .ordinary
                .as_ref()
                .ok_or("nested invoke descriptor absent")?;
            if let Some(shape) = descriptor.returns {
                managed_calls::Api::load(self.library)?.compatible(result, shape)?;
            } else if !result.is_null() {
                return Err("nested void IL returned an object".into());
            }
        }
        Ok(result)
    }
}

impl Metadata<'_> {
    pub(super) fn finish_invoke(
        &mut self,
        prepared: &PreparedInvoke,
        result: Object,
        memory: &mut Memory,
        runtime: &crate::x64::runtime::Runtime64,
    ) -> Result<u64, String> {
        if let Some(input) = prepared.input.as_ref() {
            self.refresh_native(memory)?;
            if prepared.exception_slot != 0 {
                checked_output_at(
                    memory,
                    prepared.rsp,
                    prepared.stack,
                    prepared.exception_slot,
                    8,
                )?;
            }
            input.finish(memory, runtime)?;
            prepared.commit_outputs(memory, &[])?;
            return Ok(0);
        }
        let mut outputs = Vec::new();
        if let Some(frame) = prepared.frame.as_ref() {
            let calls = managed_calls::Api::load(self.library)?;
            for out in frame.outputs()? {
                let word = match out.shape {
                    shape @ managed_calls::Shape::Reference { .. } => {
                        self.root_reference(calls.compatible(out.word as Object, shape)?, memory)?
                    }
                    managed_calls::Shape::Scalar { .. } => out.word,
                };
                checked_output_at(
                    memory,
                    prepared.rsp,
                    prepared.stack,
                    out.address,
                    out.shape.width(),
                )?;
                outputs.push((out.address, out.shape.width(), word));
            }
        }
        if prepared.exception_slot != 0 {
            checked_output_at(
                memory,
                prepared.rsp,
                prepared.stack,
                prepared.exception_slot,
                8,
            )?;
        }
        if result.is_null() {
            self.refresh_native(memory)?;
            prepared.commit_outputs(memory, &outputs)?;
            return Ok(0);
        }
        let roots = self
            .objects
            .as_mut()
            .ok_or("nested reference result has no owned root service")?;
        let token =
            roots
                .objects
                .borrow_reference(roots.strings, roots.api, roots.roots, result)?;
        roots.objects.publish_pending(memory)?;
        self.refresh_native(memory)?;
        let roots = self
            .objects
            .as_mut()
            .ok_or("nested result root service retired")?;
        if !roots.objects.contains(token) && !roots.fields.has_owner(token) {
            let class = roots.roots.class(roots.api, token)?;
            let images = self.images.live_native_images()?;
            let specs = field_layouts::reference_storage_specs(
                self.library,
                self.images.unique_native_image(super::CORE_MODULE)?,
                &images,
                result,
                class,
            )?;
            roots.fields.install_with(
                self.library,
                roots.api,
                roots.roots,
                memory,
                specs,
                |registry, native| {
                    roots
                        .objects
                        .borrow_reference(roots.strings, roots.api, registry, native)
                },
            )?;
            roots.objects.publish_pending(memory)?;
        }
        eprintln!(
            "NATIVE_MONO_NESTED_ORDINARY_INVOKE_VERIFIED: {}; rooted-reference={token:#x}",
            prepared
                .ordinary
                .as_ref()
                .ok_or("nested ordinary descriptor absent")?
                .name
        );
        prepared.commit_outputs(memory, &outputs)?;
        Ok(token)
    }
}

fn checked_output(
    memory: &Memory,
    cpu: &Cpu64,
    stack: (u64, u64),
    address: u64,
    width: usize,
) -> Result<(), String> {
    checked_output_at(memory, cpu.regs[4], stack, address, width)
}
fn checked_output_at(
    memory: &Memory,
    rsp: u64,
    stack: (u64, u64),
    address: u64,
    width: usize,
) -> Result<(), String> {
    let end = address
        .checked_add(width as u64)
        .ok_or("nested output overflow")?;
    if rsp < stack.0
        || rsp >= stack.1
        || !address.is_multiple_of(width as u64)
        || address < rsp.saturating_add(8)
        || address < stack.0
        || end > stack.1
        || !memory.owned_noncode_access(address, width)
    {
        return Err("nested output is not a live owned callback-stack slot".into());
    }
    memory.check(address, width, crate::x64::memory::WRITE)
}
impl Metadata<'_> {
    fn dispatch_object(
        &mut self,
        function: &str,
        cpu: &mut Cpu64,
        memory: &mut Memory,
        stack: (u64, u64),
    ) -> Result<(), String> {
        let a = [cpu.regs[1], cpu.regs[2], cpu.regs[8]];
        let library = self.library;
        if matches!(function, "mono_field_get_value" | "mono_field_set_value") {
            let field = self.metadata.native(a[1], metadata::Kind::Field)?;
            let roots = self
                .objects
                .as_mut()
                .ok_or("nested fields have no object service")?;
            roots.roots.sync_from_guest(roots.api, memory)?;
            roots.fields.sync(roots.api, roots.roots)?;
            if function == "mono_field_set_value" {
                roots
                    .fields
                    .store_value(roots.api, roots.roots, memory, a[0], field, a[2])?;
            } else {
                let bytes = if roots.attributes.1.contains(a[0]) {
                    roots
                        .attributes
                        .1
                        .read_field(roots.attributes.0, a[0], field)?
                } else {
                    roots.fields.read(a[0], field)?
                };
                checked_output(memory, cpu, stack, a[2], bytes.len())?;
                memory.write(a[2], &bytes)?;
            }
            return cpu.return_integer(memory, 0);
        }
        let roots = self
            .objects
            .as_mut()
            .ok_or("nested object service absent")?;
        let result = if matches!(function, "mono_string_length" | "mono_string_chars") {
            roots.objects.native_string(roots.strings, a[0])?;
            if function == "mono_string_length" {
                memory.read_int(a[0] + 16, 4)?
            } else {
                a[0] + 20
            }
        } else if function == "mono_object_isinst" {
            let class = self.metadata.native(a[1], metadata::Kind::Class)?;
            let object = if a[0] == 0 {
                std::ptr::null_mut()
            } else {
                roots.roots.native(roots.api, a[0])?
            };
            let isinst = sym!(
                library,
                "mono_object_isinst",
                unsafe extern "C" fn(Object, Object) -> Object
            );
            let found = unsafe { isinst(object, class) };
            if found.is_null() {
                0
            } else if found == object {
                a[0]
            } else {
                return Err("nested isinst produced another identity".into());
            }
        } else {
            let empty = function == "mono_unity_string_empty_wrapper";
            let wrapper = function == "mono_string_new_wrapper";
            if !wrapper && !empty && a[0] != super::ROOT_TOKEN {
                return Err("nested String allocation has unknown domain".into());
            }
            let bytes = if empty {
                Vec::new()
            } else if function == "mono_string_new_len" {
                if a[2] > 32768 {
                    return Err("nested String length exceeds bound".into());
                }
                let mut bytes = vec![0; a[2] as usize];
                memory.read(a[1], &mut bytes)?;
                bytes
            } else {
                string(memory, if wrapper { a[0] } else { a[1] })?.into_bytes()
            };
            if bytes.len() > 32768 {
                return Err("nested String input exceeds bound".into());
            }
            let native = if empty {
                let current = sym!(library, "mono_domain_get", unsafe extern "C" fn() -> Object);
                let new = sym!(
                    library,
                    "mono_unity_string_empty_wrapper",
                    unsafe extern "C" fn() -> Object
                );
                if unsafe { current() } != self.domain {
                    return Err("nested empty string current domain mismatch".into());
                }
                unsafe { new() }
            } else {
                unsafe {
                    (roots.strings.new_string)(
                        self.domain,
                        bytes.as_ptr().cast(),
                        bytes.len() as u32,
                    )
                }
            };
            let token =
                roots
                    .objects
                    .borrow_reference(roots.strings, roots.api, roots.roots, native)?;
            roots.objects.publish_pending(memory)?;
            token
        };
        cpu.return_integer(memory, result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_byref_completion_preflights_all_outputs_and_exception_before_writes() {
        use crate::x64::memory::{READ, WRITE};
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE).unwrap();
        let mut prepared = PreparedInvoke {
            library: ptr::null_mut(),
            method: ptr::null_mut(),
            image: ptr::null_mut(),
            step: None,
            ordinary: None,
            constructor: None,
            receiver: ptr::null_mut(),
            frame: None,
            input: None,
            stack: (0x1000, 0x2000),
            rsp: 0x1800,
            exception_slot: 0x1880,
        };
        memory.write_int(0x1840, 8, 0xaaaaaaaa).unwrap();
        memory.write_int(0x1880, 8, 0xbbbbbbbb).unwrap();
        assert!(prepared
            .commit_outputs(&mut memory, &[(0x1840, 1, 1), (0x2000, 8, 2)])
            .is_err());
        assert_eq!(memory.read_int(0x1840, 8).unwrap(), 0xaaaaaaaa);
        assert_eq!(memory.read_int(0x1880, 8).unwrap(), 0xbbbbbbbb);
        prepared.exception_slot = 0x1800;
        assert!(prepared
            .commit_outputs(&mut memory, &[(0x1840, 1, 1)])
            .is_err());
        assert_eq!(memory.read_int(0x1840, 8).unwrap(), 0xaaaaaaaa);
        prepared.exception_slot = 0x1880;
        prepared
            .commit_outputs(&mut memory, &[(0x1840, 1, 1)])
            .unwrap();
        assert_eq!(memory.read_int(0x1840, 8).unwrap(), 0xaaaaaa01);
        assert_eq!(memory.read_int(0x1880, 8).unwrap(), 0);
        memory.unmap(0x1000, 4096).unwrap();
        assert!(prepared
            .commit_outputs(&mut memory, &[(0x1840, 1, 1)])
            .is_err());
    }
    #[test]
    fn nested_output_rejects_return_slot_misalignment_code_and_retired_storage() {
        use crate::x64::memory::{EXECUTE, READ, WRITE};
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE).unwrap();
        memory.map(0x2000, 4096, READ | WRITE | EXECUTE).unwrap();
        let cpu = Cpu64::new(0, 0x1800);
        checked_output(&memory, &cpu, (0x1000, 0x2000), 0x1840, 8).unwrap();
        for address in [0, 0x1400, 0x1800, 0x1841, 0x2000, u64::MAX] {
            assert!(checked_output(&memory, &cpu, (0x1000, 0x2000), address, 8).is_err());
        }
        assert!(checked_output(&memory, &cpu, (0x1000, 0x3000), 0x2040, 8).is_err());
        memory.unmap(0x1000, 4096).unwrap();
        assert!(checked_output(&memory, &cpu, (0x1000, 0x2000), 0x1840, 8).is_err());
    }
    #[test]
    fn null_root_clear_checks_active_stack_and_keeps_outputs_on_failure() {
        let mut memory = Memory::default();
        memory
            .map(
                0x1000,
                4096,
                crate::x64::memory::READ | crate::x64::memory::WRITE,
            )
            .unwrap();
        memory.write_int(0x1800, 8, 0x1234).unwrap();
        let mut registry = metadata::Registry::default();
        let mut images = images::Registry::default();
        let mut route = Metadata {
            library: ptr::null_mut(),
            domain: ptr::null_mut(),
            thread_handle: ptr::null_mut(),
            metadata: &mut registry,
            images: &mut images,
            rooted_instance: None,
            objects: None,
        };
        let mut cpu = Cpu64::new(0, 0x1800);
        assert!(route
            .dispatch(
                "mono_thread_current",
                &mut cpu,
                &mut memory,
                (0x1000, 0x2000)
            )
            .is_err());
        assert_eq!(cpu.rip, 0);
        assert_eq!(cpu.regs[4], 0x1800);
        cpu.regs[2] = 0x1840;
        memory.write_int(0x1840, 8, 0xaaaaaaaa).unwrap();
        route
            .dispatch(
                "mono_gc_wbarrier_set_field",
                &mut cpu,
                &mut memory,
                (0x1000, 0x2000),
            )
            .unwrap();
        assert_eq!(memory.read_int(0x1840, 8).unwrap(), 0);
        assert_eq!(cpu.rip, 0x1234);
        for (object, value, destination, stack) in [
            (1, 0, 0x1840, (0x1000, 0x2000)),
            (0, 1, 0x1840, (0x1000, 0x2000)),
            (0, 0, 0x1400, (0x1000, 0x2000)),
            (0, 0, 0x1840, (0x1900, 0x2000)),
            (0, 0, 0x1841, (0x1000, 0x2000)),
        ] {
            cpu.regs[4] = 0x1800;
            cpu.regs[1] = object;
            cpu.regs[8] = value;
            cpu.regs[2] = destination;
            memory.write_int(0x1840, 8, 0xaaaaaaaa).unwrap();
            memory.write_int(0x1400, 8, 0xbbbbbbbb).unwrap();
            assert!(route
                .dispatch("mono_gc_wbarrier_set_field", &mut cpu, &mut memory, stack)
                .is_err());
            assert_eq!(memory.read_int(0x1840, 8).unwrap(), 0xaaaaaaaa);
            assert_eq!(memory.read_int(0x1400, 8).unwrap(), 0xbbbbbbbb);
            assert_eq!(cpu.regs[4], 0x1800);
        }
    }
    #[test]
    fn rooted_constructor_reentry_accepts_only_exact_identity_and_live_stack_slots() {
        let mut memory = Memory::default();
        memory
            .map(
                0x1000,
                4096,
                crate::x64::memory::READ | crate::x64::memory::WRITE,
            )
            .unwrap();
        memory.write_int(0x1800, 8, 0x1234).unwrap();
        let mut registry = metadata::Registry::default();
        let mut images = images::Registry::default();
        let mut native_class = [0u64; 32];
        let owned = 0x6fff00000000;
        let mut route = Metadata {
            library: ptr::null_mut(),
            domain: ptr::null_mut(),
            thread_handle: ptr::null_mut(),
            metadata: &mut registry,
            images: &mut images,
            rooted_instance: Some((owned, native_class.as_mut_ptr().cast())),
            objects: None,
        };
        let mut cpu = Cpu64::new(0, 0x1800);
        assert!(route
            .dispatch(
                "mono_thread_current",
                &mut cpu,
                &mut memory,
                (0x1000, 0x2000)
            )
            .is_err());
        assert_eq!(cpu.rip, 0);
        assert_eq!(cpu.regs[4], 0x1800);
        cpu.regs[2] = 0x1840;
        cpu.regs[8] = owned;
        route
            .dispatch(
                "mono_gc_wbarrier_set_field",
                &mut cpu,
                &mut memory,
                (0x1000, 0x2000),
            )
            .unwrap();
        assert_eq!(memory.read_int(0x1840, 8).unwrap(), owned);
        assert!(memory.read_int(owned, 8).is_err());
        cpu.regs[4] = 0x1800;
        cpu.regs[1] = owned;
        route
            .dispatch(
                "mono_object_get_class",
                &mut cpu,
                &mut memory,
                (0x1000, 0x2000),
            )
            .unwrap();
        assert_eq!(
            route
                .metadata
                .native(cpu.regs[0], metadata::Kind::Class)
                .unwrap(),
            native_class.as_mut_ptr().cast()
        );
        for (object, destination, value) in [
            (0, 0x1840, owned + 8),
            (owned, 0x1840, owned),
            (0, 0x1400, owned),
        ] {
            cpu.regs[4] = 0x1800;
            cpu.regs[1] = object;
            cpu.regs[2] = destination;
            cpu.regs[8] = value;
            assert!(route
                .dispatch(
                    "mono_gc_wbarrier_set_field",
                    &mut cpu,
                    &mut memory,
                    (0x1000, 0x2000)
                )
                .is_err());
            assert_eq!(memory.read_int(0x1840, 8).unwrap(), owned);
            assert_eq!(cpu.regs[4], 0x1800);
        }
        cpu.regs[1] = owned + 8;
        assert!(route
            .dispatch(
                "mono_object_get_class",
                &mut cpu,
                &mut memory,
                (0x1000, 0x2000)
            )
            .is_err());
        route.rooted_instance = None;
        cpu.regs[1] = owned;
        assert!(route
            .dispatch(
                "mono_object_get_class",
                &mut cpu,
                &mut memory,
                (0x1000, 0x2000)
            )
            .is_err());
    }
    #[test]
    fn nested_userdata_uses_the_existing_identity_and_rejects_managed_invokes() {
        let mut memory = Memory::default();
        memory
            .map(
                0x1000,
                4096,
                crate::x64::memory::READ | crate::x64::memory::WRITE,
            )
            .unwrap();
        memory.write_int(0x1800, 8, 0x1234).unwrap();
        let mut registry = metadata::Registry::default();
        let mut native = [0u64; 32];
        let token = registry
            .wrap(
                &mut memory,
                metadata::Value::Handle(native.as_mut_ptr().cast(), metadata::Kind::Class),
            )
            .unwrap();
        let mut images = images::Registry::default();
        let mut route = Metadata {
            library: ptr::null_mut(),
            domain: ptr::null_mut(),
            thread_handle: ptr::null_mut(),
            metadata: &mut registry,
            images: &mut images,
            rooted_instance: None,
            objects: None,
        };
        let mut cpu = Cpu64::new(0, 0x1800);
        cpu.regs[1] = token;
        cpu.regs[2] = 0x334455;
        route
            .dispatch(
                "mono_class_set_userdata",
                &mut cpu,
                &mut memory,
                (0x1000, 0x2000),
            )
            .unwrap();
        assert_eq!(memory.read_int(token + 232, 8).unwrap(), 0x334455);
        assert_eq!(native[29], 0);
        cpu.regs[4] = 0x1800;
        route
            .dispatch(
                "mono_class_get_userdata",
                &mut cpu,
                &mut memory,
                (0x1000, 0x2000),
            )
            .unwrap();
        assert_eq!(cpu.regs[0], 0x334455);
        cpu.regs[4] = 0x1800;
        assert!(route
            .dispatch(
                "mono_runtime_invoke",
                &mut cpu,
                &mut memory,
                (0x1000, 0x2000)
            )
            .is_err());
        assert_eq!(cpu.regs[4], 0x1800);
        cpu.regs[1] = token + 1;
        assert!(route
            .dispatch(
                "mono_class_set_userdata",
                &mut cpu,
                &mut memory,
                (0x1000, 0x2000)
            )
            .is_err());
        assert_eq!(memory.read_int(token + 232, 8).unwrap(), 0x334455);
    }
}
