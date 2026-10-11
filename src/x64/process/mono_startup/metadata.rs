//! Typed metadata handles with owned strings/iterators. Class memory exposes
//! only canonical element-class and embedding userdata cells; native pointers
//! and all neighbouring bytes remain inaccessible.
use super::super::super::memory::{shared::HostRegion, READ, WRITE};
use super::{c_char, CString, Memory, Object};
use std::{collections::BTreeMap, ptr};
const HANDLE_BASE: u64 = 0x6ffe40000000;
const STRING_BASE: u64 = 0x6ffe50000000;
const ITER_BASE: u64 = 0x6ffe60000000;
const CORE_CLASSES: &[&str] = &[
    "mono_get_object_class",
    "mono_get_byte_class",
    "mono_get_void_class",
    "mono_get_boolean_class",
    "mono_get_sbyte_class",
    "mono_get_int16_class",
    "mono_get_uint16_class",
    "mono_get_int32_class",
    "mono_get_uint32_class",
    "mono_get_intptr_class",
    "mono_get_uintptr_class",
    "mono_get_int64_class",
    "mono_get_uint64_class",
    "mono_get_single_class",
    "mono_get_double_class",
    "mono_get_char_class",
    "mono_get_string_class",
    "mono_get_enum_class",
    "mono_get_array_class",
    "mono_get_thread_class",
    "mono_get_exception_class",
];
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Kind {
    Class,
    Method,
    Field,
    Signature,
    Type,
}
struct Entry {
    kind: Kind,
    native: Object,
    userdata: Option<HostRegion>,
    element: Option<HostRegion>,
}
struct Iterator {
    parent: u64,
    function: String,
    native: Object,
}
#[derive(Default)]
pub(super) struct Registry {
    handles: BTreeMap<u64, Entry>,
    reverse_handles: BTreeMap<(Kind, usize), u64>,
    strings: BTreeMap<CString, (u64, HostRegion)>,
    string_bytes: u64,
    iterators: BTreeMap<u64, Iterator>,
    queries: usize,
    retired: bool,
}
macro_rules! sym {
    ($lib:expr,$name:expr,$ty:ty) => {{
        let name = CString::new($name).map_err(|e| e.to_string())?;
        let p = unsafe { libc::dlsym($lib, name.as_ptr()) };
        if p.is_null() {
            return Err(format!("missing typed metadata symbol: {}", $name));
        }
        unsafe { std::mem::transmute::<Object, $ty>(p) }
    }};
}
pub(super) enum Value {
    Handle(Object, Kind),
    Text(CString),
    Scalar(u64),
}
impl Registry {
    pub(super) fn native(&self, token: u64, kind: Kind) -> Result<Object, String> {
        if self.retired {
            return Err("metadata registry retired".into());
        }
        let entry = self.handles.get(&token).ok_or("unknown metadata token")?;
        if entry.kind != kind {
            return Err("metadata token kind mismatch".into());
        }
        Ok(entry.native)
    }
    pub(super) fn wrap(&mut self, memory: &mut Memory, value: Value) -> Result<u64, String> {
        if self.retired {
            return Err("metadata result after retirement".into());
        }
        match value {
            Value::Scalar(value) => Ok(value),
            Value::Text(text) => {
                if let Some((token, _)) = self.strings.get(&text) {
                    return Ok(*token);
                }
                let length = text.as_bytes_with_nul().len();
                // Locked UnityPlayer's strstr scans 16B at a time. Reserve
                // owned zero padding for one unaligned vector past the NUL;
                // never make adjacent host/metadata bytes readable.
                let span = (length as u64 + 30) & !15;
                let next = self
                    .string_bytes
                    .checked_add(span)
                    .ok_or("metadata string arena overflow")?;
                if self.strings.len() >= 65536 || length > 32768 || next > ITER_BASE - STRING_BASE {
                    return Err("metadata string capacity exceeded".into());
                }
                let token = STRING_BASE + self.string_bytes;
                let region = HostRegion::owned(span as usize)?;
                region.with_native(|p| unsafe {
                    ptr::copy_nonoverlapping(
                        text.as_ptr().cast(),
                        p.as_ptr(),
                        text.as_bytes_with_nul().len(),
                    );
                })?;
                memory.map_shared_data(token, region.clone(), READ, READ)?;
                self.strings.insert(text, (token, region));
                self.string_bytes = next;
                Ok(token)
            }
            Value::Handle(native, kind) => {
                if native.is_null() {
                    return Ok(0);
                }
                if let Some(token) = self.reverse_handles.get(&(kind, native as usize)) {
                    return Ok(*token);
                }
                if self.handles.len() >= 65536 {
                    return Err("metadata handle capacity exceeded".into());
                }
                let token = HANDLE_BASE + self.handles.len() as u64 * 4096;
                let userdata = if kind == Kind::Class {
                    // Exact candidate reports this userdata offset at load. It
                    // is an embedding scalar cell, not a managed reference.
                    let initial =
                        unsafe { native.cast::<u8>().add(232).cast::<u64>().read_unaligned() };
                    if initial != 0 {
                        return Err(
                            "non-NULL native class userdata not admitted to guest scalar shadow"
                                .into(),
                        );
                    }
                    let region = HostRegion::owned(8)?;
                    memory.map_shared_data(
                        token + 232,
                        region.clone(),
                        READ | WRITE,
                        READ | WRITE,
                    )?;
                    Some(region)
                } else {
                    None
                };
                self.handles.insert(
                    token,
                    Entry {
                        kind,
                        native,
                        userdata,
                        element: None,
                    },
                );
                self.reverse_handles.insert((kind, native as usize), token);
                Ok(token)
            }
        }
    }
    /// The locked Unity type-cache hash dereferences MonoClass::element_class
    /// at offset zero. Expose exactly this immutable pointer as a canonical
    /// metadata token, never as an alias to the native class header.
    pub(super) fn publish_class_elements(
        &mut self,
        library: Object,
        memory: &mut Memory,
        token: u64,
    ) -> Result<(), String> {
        let get = sym!(
            library,
            "mono_class_get_element_class",
            unsafe extern "C" fn(Object) -> Object
        );
        self.publish_elements(memory, token, |class| {
            let element = unsafe { get(class) };
            // The frozen MSVC-profile class starts with this pointer. Verify
            // that its native API agrees before installing an owned snapshot.
            if unsafe { class.cast::<Object>().read_unaligned() } != element {
                return Err("element-class prefix ABI mismatch".into());
            }
            Ok(element)
        })
    }
    fn publish_elements(
        &mut self,
        memory: &mut Memory,
        mut token: u64,
        mut element: impl FnMut(Object) -> Result<Object, String>,
    ) -> Result<(), String> {
        let mut visited = std::collections::BTreeSet::new();
        while token != 0 && visited.insert(token) {
            if visited.len() > 64 {
                return Err("element-class graph exceeds bounded depth".into());
            }
            let class = self.native(token, Kind::Class)?;
            let native_element = element(class)?;
            let next = self.wrap(memory, Value::Handle(native_element, Kind::Class))?;
            if let Some(region) = &self.handles[&token].element {
                let current =
                    region.with_native(|p| unsafe { p.as_ptr().cast::<u64>().read_unaligned() })?;
                if current != next {
                    return Err("published element-class identity changed".into());
                }
                break;
            }
            let region = HostRegion::owned(8)?;
            region.with_native(|p| unsafe {
                p.as_ptr().cast::<u64>().write_unaligned(next);
            })?;
            memory.map_shared_data(token, region.clone(), READ, READ)?;
            self.handles.get_mut(&token).unwrap().element = Some(region);
            token = next;
        }
        Ok(())
    }
    /// Unity's embedding cache is guest-owned opaque data. Keep exported
    /// get/set coherent with the checked +232 cell; never install a guest
    /// address into the native MonoClass.
    pub(super) fn userdata(
        &mut self,
        memory: &mut Memory,
        token: u64,
        value: Option<u64>,
    ) -> Result<u64, String> {
        self.native(token, Kind::Class)?;
        if self.handles[&token].userdata.is_none() {
            return Err("class has no owned userdata cell".into());
        }
        self.queries += 1;
        if let Some(value) = value {
            memory.write_int(token + 232, 8, value)?;
            Ok(0)
        } else {
            memory.read_int(token + 232, 8)
        }
    }
    pub(super) fn describe_class(&self, lib: Object, token: u64) -> Result<String, String> {
        let class = self.native(token, Kind::Class)?;
        let name = sym!(
            lib,
            "mono_class_get_name",
            unsafe extern "C" fn(Object) -> *const c_char
        );
        let namespace = sym!(
            lib,
            "mono_class_get_namespace",
            unsafe extern "C" fn(Object) -> *const c_char
        );
        let image = sym!(
            lib,
            "mono_class_get_image",
            unsafe extern "C" fn(Object) -> Object
        );
        let image_name = sym!(
            lib,
            "mono_image_get_name",
            unsafe extern "C" fn(Object) -> *const c_char
        );
        let flags = sym!(
            lib,
            "mono_class_get_flags",
            unsafe extern "C" fn(Object) -> u32
        );
        let type_token = sym!(
            lib,
            "mono_class_get_type_token",
            unsafe extern "C" fn(Object) -> u32
        );
        let text = |p: *const c_char| -> Result<String, String> {
            if p.is_null() {
                return Err("pending class NULL name".into());
            }
            let mut bytes = Vec::new();
            for i in 0..32768 {
                let byte = unsafe { p.add(i).read() } as u8;
                if byte == 0 {
                    return String::from_utf8(bytes).map_err(|e| e.to_string());
                }
                bytes.push(byte);
            }
            Err("pending class unbounded name".into())
        };
        let native_image = unsafe { image(class) };
        if native_image.is_null() {
            return Err("pending class has no image".into());
        }
        Ok(format!(
            "{}.{}; image={}; type-token={:#x}; flags={:#x}",
            text(unsafe { namespace(class) })?,
            text(unsafe { name(class) })?,
            text(unsafe { image_name(native_image) })?,
            unsafe { type_token(class) },
            unsafe { flags(class) }
        ))
    }
    pub(super) fn describe_method(&self, lib: Object, token: u64) -> Result<String, String> {
        let method = self.native(token, Kind::Method)?;
        let name = sym!(
            lib,
            "mono_method_get_name",
            unsafe extern "C" fn(Object) -> *const c_char
        );
        let class = sym!(
            lib,
            "mono_method_get_class",
            unsafe extern "C" fn(Object) -> Object
        );
        let class_name = sym!(
            lib,
            "mono_class_get_name",
            unsafe extern "C" fn(Object) -> *const c_char
        );
        let namespace = sym!(
            lib,
            "mono_class_get_namespace",
            unsafe extern "C" fn(Object) -> *const c_char
        );
        let signature = sym!(
            lib,
            "mono_method_signature",
            unsafe extern "C" fn(Object) -> Object
        );
        let count = sym!(
            lib,
            "mono_signature_get_param_count",
            unsafe extern "C" fn(Object) -> u32
        );
        let instance = sym!(
            lib,
            "mono_signature_is_instance",
            unsafe extern "C" fn(Object) -> u32
        );
        let params = sym!(
            lib,
            "mono_signature_get_params",
            unsafe extern "C" fn(Object, *mut Object) -> Object
        );
        let returns = sym!(
            lib,
            "mono_signature_get_return_type",
            unsafe extern "C" fn(Object) -> Object
        );
        let type_kind = sym!(
            lib,
            "mono_type_get_type",
            unsafe extern "C" fn(Object) -> u32
        );
        let byref = sym!(
            lib,
            "mono_type_is_byref",
            unsafe extern "C" fn(Object) -> u32
        );
        let method_token = sym!(
            lib,
            "mono_method_get_token",
            unsafe extern "C" fn(Object) -> u32
        );
        let cls = unsafe { class(method) };
        let sig = unsafe { signature(method) };
        if cls.is_null() || sig.is_null() || unsafe { count(sig) } > 16 {
            return Err("pending method has invalid/unbounded signature".into());
        }
        let text = |p: *const c_char| -> Result<String, String> {
            if p.is_null() {
                return Err("pending method NULL name".into());
            }
            let mut bytes = Vec::new();
            for i in 0..32768 {
                let byte = unsafe { p.add(i).read() } as u8;
                if byte == 0 {
                    return String::from_utf8(bytes).map_err(|e| e.to_string());
                }
                bytes.push(byte);
            }
            Err("pending method unbounded name".into())
        };
        let mut iter = ptr::null_mut();
        let mut types = Vec::new();
        for _ in 0..unsafe { count(sig) } {
            let ty = unsafe { params(sig, &mut iter) };
            if ty.is_null() {
                return Err("pending method missing param type".into());
            }
            types.push(format!(
                "{:#x}/byref={}",
                unsafe { type_kind(ty) },
                unsafe { byref(ty) }
            ));
        }
        let ret = unsafe { returns(sig) };
        if ret.is_null() {
            return Err("pending method missing return type".into());
        }
        Ok(format!(
            "{}.{}::{}; method-token={:#x}; instance={}; params={:?}; return={:#x}/byref={}",
            text(unsafe { namespace(cls) })?,
            text(unsafe { class_name(cls) })?,
            text(unsafe { name(method) })?,
            unsafe { method_token(method) },
            unsafe { instance(sig) },
            types,
            unsafe { type_kind(ret) },
            unsafe { byref(ret) }
        ))
    }
    /// The caller resolves a live provider-owned image before this leaf. Copy
    /// names into the existing owned text arena; never publish a native char*.
    pub(super) fn query_image(
        &mut self,
        lib: Object,
        function: &str,
        image: Object,
        argument: u64,
    ) -> Result<Value, String> {
        if self.retired || image.is_null() {
            return Err("image metadata has no live registry identity".into());
        }
        self.queries += 1;
        match function {
            "mono_image_get_name" => {
                let get = sym!(lib, function, unsafe extern "C" fn(Object) -> *const c_char);
                let p = unsafe { get(image) };
                if p.is_null() {
                    return Err("native image name absent".into());
                }
                let mut bytes = Vec::new();
                for offset in 0..32768 {
                    let b = unsafe { p.add(offset).read() } as u8;
                    if b == 0 {
                        return Ok(Value::Text(CString::new(bytes).map_err(|e| e.to_string())?));
                    }
                    bytes.push(b);
                }
                Err("native image name exceeds bound".into())
            }
            "mono_image_get_table_rows" => {
                let get = sym!(lib, function, unsafe extern "C" fn(Object, i32) -> i32);
                // Preserve Mono's int ABI, including its zero result for an
                // invalid table index, without accessing native table memory.
                Ok(Value::Scalar(
                    unsafe { get(image, argument as u32 as i32) } as u32 as u64,
                ))
            }
            _ => Err(format!("unsupported image metadata leaf: {function}")),
        }
    }
    pub(super) fn query(
        &mut self,
        lib: Object,
        function: &str,
        a: u64,
        b: u64,
        c: u64,
    ) -> Result<Value, String> {
        self.queries += 1;
        if CORE_CLASSES.contains(&function) {
            let get = sym!(lib, function, unsafe extern "C" fn() -> Object);
            return Ok(Value::Handle(unsafe { get() }, Kind::Class));
        }
        if let Some((kind, byte_result)) = predicate_shape(function) {
            let native = self.native(a, kind)?;
            let result = if byte_result {
                let f = sym!(lib, function, unsafe extern "C" fn(Object) -> u8);
                (unsafe { f(native) }) != 0
            } else {
                let f = sym!(lib, function, unsafe extern "C" fn(Object) -> i32);
                (unsafe { f(native) }) != 0
            };
            return Ok(Value::Scalar(u64::from(result)));
        }
        match function {
            "mono_field_get_type" | "mono_field_get_parent" => {
                let field = self.native(a, Kind::Field)?;
                let get = sym!(lib, function, unsafe extern "C" fn(Object) -> Object);
                Ok(Value::Handle(
                    unsafe { get(field) },
                    if function == "mono_field_get_type" {
                        Kind::Type
                    } else {
                        Kind::Class
                    },
                ))
            }
            "mono_field_get_flags" | "mono_field_get_offset" => {
                let field = self.native(a, Kind::Field)?;
                let get = sym!(lib, function, unsafe extern "C" fn(Object) -> u32);
                Ok(Value::Scalar(unsafe { get(field) } as u64))
            }
            "mono_array_class_get" | "mono_bounded_array_class_get" => {
                let element = self.native(a, Kind::Class)?;
                if !(1..=32).contains(&b) || (function == "mono_bounded_array_class_get" && c > 1) {
                    return Err("array metadata rank/bounded flag outside checked contract".into());
                }
                let class = if function == "mono_array_class_get" {
                    let get = sym!(lib, function, unsafe extern "C" fn(Object, u32) -> Object);
                    unsafe { get(element, b as u32) }
                } else {
                    let get = sym!(
                        lib,
                        function,
                        unsafe extern "C" fn(Object, u32, i32) -> Object
                    );
                    unsafe { get(element, b as u32, c as i32) }
                };
                Ok(Value::Handle(class, Kind::Class))
            }
            "mono_class_is_assignable_from" | "mono_class_is_subclass_of" => {
                let left = self.native(a, Kind::Class)?;
                let right = self.native(b, Kind::Class)?;
                let result = if function == "mono_class_is_assignable_from" {
                    let compare = sym!(lib, function, unsafe extern "C" fn(Object, Object) -> i32);
                    unsafe { compare(left, right) }
                } else {
                    if c > 1 {
                        return Err("subclass interface flag outside checked contract".into());
                    }
                    let compare = sym!(
                        lib,
                        function,
                        unsafe extern "C" fn(Object, Object, i32) -> i32
                    );
                    unsafe { compare(left, right, c as i32) }
                };
                Ok(Value::Scalar(result as u32 as u64))
            }
            "mono_class_enum_basetype" => {
                let class = self.native(a, Kind::Class)?;
                let get = sym!(lib, function, unsafe extern "C" fn(Object) -> Object);
                Ok(Value::Handle(unsafe { get(class) }, Kind::Type))
            }
            "mono_metadata_type_equal" => {
                let left = self.native(a, Kind::Type)?;
                let right = self.native(b, Kind::Type)?;
                let f = sym!(lib, function, unsafe extern "C" fn(Object, Object) -> u32);
                Ok(Value::Scalar(unsafe { f(left, right) } as u64))
            }
            "mono_type_get_class"
            | "mono_class_from_mono_type"
            | "mono_method_get_class"
            | "mono_class_get_parent"
            | "mono_class_get_element_class"
            | "mono_class_get_nesting_type"
            | "mono_ptr_class_get" => {
                let kind = if function.starts_with("mono_method") {
                    Kind::Method
                } else if matches!(
                    function,
                    "mono_class_get_parent"
                        | "mono_class_get_element_class"
                        | "mono_class_get_nesting_type"
                ) {
                    Kind::Class
                } else {
                    Kind::Type
                };
                let native = self.native(a, kind)?;
                let f = sym!(lib, function, unsafe extern "C" fn(Object) -> Object);
                Ok(Value::Handle(unsafe { f(native) }, Kind::Class))
            }
            "mono_signature_get_return_type" => {
                let native = self.native(a, Kind::Signature)?;
                let f = sym!(lib, function, unsafe extern "C" fn(Object) -> Object);
                Ok(Value::Handle(unsafe { f(native) }, Kind::Type))
            }
            "mono_type_get_type"
            | "mono_type_is_byref"
            | "mono_class_get_flags"
            | "mono_class_is_valuetype"
            | "mono_class_is_enum"
            | "mono_class_is_generic"
            | "mono_class_is_inflated"
            | "mono_unity_class_is_abstract"
            | "mono_unity_class_is_interface"
            | "mono_unity_class_has_failure"
            | "mono_class_get_rank"
            | "mono_class_num_fields"
            | "mono_class_num_methods"
            | "mono_class_get_type_token"
            | "mono_class_instance_size"
            | "mono_class_array_element_size" => {
                let kind = if function.starts_with("mono_type") {
                    Kind::Type
                } else {
                    Kind::Class
                };
                let native = self.native(a, kind)?;
                let f = sym!(lib, function, unsafe extern "C" fn(Object) -> u32);
                Ok(Value::Scalar(unsafe { f(native) } as u64))
            }
            "mono_array_element_size" => {
                let class = self.native(a, Kind::Class)?;
                let rank = sym!(
                    lib,
                    "mono_class_get_rank",
                    unsafe extern "C" fn(Object) -> u32
                );
                if unsafe { rank(class) } != 1 {
                    return Err("array element size requires an SZ array class".into());
                }
                let size = sym!(
                    lib,
                    "mono_array_element_size",
                    unsafe extern "C" fn(Object) -> i32
                );
                let size = unsafe { size(class) };
                if !(1..=65536).contains(&size) {
                    return Err("array element size exceeds checked bound".into());
                }
                Ok(Value::Scalar(size as u64))
            }
            "mono_class_get_type" => {
                let native = self.native(a, Kind::Class)?;
                let f = sym!(lib, function, unsafe extern "C" fn(Object) -> Object);
                Ok(Value::Handle(unsafe { f(native) }, Kind::Type))
            }
            "mono_method_signature" => {
                let native = self.native(a, Kind::Method)?;
                let f = sym!(lib, function, unsafe extern "C" fn(Object) -> Object);
                Ok(Value::Handle(unsafe { f(native) }, Kind::Signature))
            }
            "mono_class_get_name"
            | "mono_class_get_namespace"
            | "mono_method_get_name"
            | "mono_field_get_name" => {
                let kind = match function {
                    "mono_method_get_name" => Kind::Method,
                    "mono_field_get_name" => Kind::Field,
                    _ => Kind::Class,
                };
                let native = self.native(a, kind)?;
                let f = sym!(lib, function, unsafe extern "C" fn(Object) -> *const c_char);
                let pointer = unsafe { f(native) };
                if pointer.is_null() {
                    return Err("NULL native metadata name".into());
                }
                let mut bytes = Vec::new();
                for offset in 0..32768 {
                    let byte = unsafe { pointer.add(offset).read() } as u8;
                    if byte == 0 {
                        return Ok(Value::Text(CString::new(bytes).map_err(|e| e.to_string())?));
                    }
                    bytes.push(byte);
                }
                Err("native metadata name exceeds bound".into())
            }
            "unity_mono_method_is_inflated"
            | "unity_mono_method_is_generic"
            | "mono_signature_is_instance"
            | "mono_signature_get_param_count" => {
                let kind = if function.starts_with("unity_mono_method") {
                    Kind::Method
                } else {
                    Kind::Signature
                };
                let native = self.native(a, kind)?;
                let f = sym!(lib, function, unsafe extern "C" fn(Object) -> u32);
                Ok(Value::Scalar(unsafe { f(native) } as u64))
            }
            _ => {
                let _ = b;
                Err(format!("metadata query not typed: {function}"))
            }
        }
    }
    pub(super) fn unity_class_get(
        &mut self,
        library: Object,
        image: Object,
        token: u64,
    ) -> Result<Value, String> {
        let rows = sym!(
            library,
            "mono_image_get_table_rows",
            unsafe extern "C" fn(Object, i32) -> i32
        );
        let rows = unsafe { rows(image, 2) };
        let token = type_definition_token(token, rows)?;
        let get = sym!(
            library,
            "mono_unity_class_get",
            unsafe extern "C" fn(Object, u32) -> Object
        );
        self.queries += 1;
        // Call Unity's real checked wrapper: loader failure remains actual NULL.
        Ok(Value::Handle(unsafe { get(image, token) }, Kind::Class))
    }
    pub(super) fn class_from_name(
        &mut self,
        lib: Object,
        image: Object,
        namespace: CString,
        name: CString,
    ) -> Result<Value, String> {
        self.queries += 1;
        let f = sym!(
            lib,
            "mono_class_from_name",
            unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object
        );
        Ok(Value::Handle(
            unsafe { f(image, namespace.as_ptr(), name.as_ptr()) },
            Kind::Class,
        ))
    }
    pub(super) fn iterate(
        &mut self,
        lib: Object,
        function: &str,
        parent: u64,
        token: u64,
    ) -> Result<(Value, u64), String> {
        self.queries += 1;
        let (parent_kind, result_kind) = if function == "mono_signature_get_params" {
            (Kind::Signature, Kind::Type)
        } else if matches!(
            function,
            "mono_class_get_nested_types" | "mono_class_get_interfaces"
        ) {
            (Kind::Class, Kind::Class)
        } else if function == "mono_class_get_fields" {
            (Kind::Class, Kind::Field)
        } else {
            (Kind::Class, Kind::Method)
        };
        let native = self.native(parent, parent_kind)?;
        let mut iterator = if token == 0 {
            ptr::null_mut()
        } else {
            let item = self
                .iterators
                .get(&token)
                .ok_or("unknown metadata iterator token")?;
            if item.parent != parent || item.function != function {
                return Err("metadata iterator parent/kind mismatch".into());
            }
            item.native
        };
        let f = sym!(
            lib,
            function,
            unsafe extern "C" fn(Object, *mut Object) -> Object
        );
        let result = unsafe { f(native, &mut iterator) };
        let iterator_token = if token == 0 {
            if self.iterators.len() >= 65536 {
                return Err("metadata iterator capacity exceeded".into());
            }
            ITER_BASE + self.iterators.len() as u64 * 256
        } else {
            token
        };
        self.iterators.insert(
            iterator_token,
            Iterator {
                parent,
                function: function.into(),
                native: iterator,
            },
        );
        Ok((Value::Handle(result, result_kind), iterator_token))
    }
    pub(super) fn retire(&mut self, memory: &mut Memory) -> Result<(), String> {
        if self.retired {
            return Ok(());
        }
        self.retired = true;
        for (token, entry) in &self.handles {
            if let Some(region) = &entry.element {
                memory.unmap_shared_data(*token)?;
                region.revoke()?;
            }
            if let Some(region) = &entry.userdata {
                memory.unmap_shared_data(token + 232)?;
                region.revoke()?;
            }
        }
        for (token, region) in self.strings.values() {
            memory.unmap_shared_data(*token)?;
            region.revoke()?;
        }
        self.iterators.clear();
        Ok(())
    }
    pub(super) fn note_query(&mut self) {
        self.queries += 1;
    }
    pub(super) fn count(&self) -> usize {
        self.queries
    }
    pub(super) fn report(&self) -> String {
        format!("native-metadata-queries={}\nnative-metadata-handles={}\nnative-metadata-strings={}\nnative-metadata-string-arena-bytes={}\nnative-metadata-retired={}\n", self.queries, self.handles.len(), self.strings.len(), self.string_bytes, self.retired)
    }
}
fn type_definition_token(token: u64, rows: i32) -> Result<u32, String> {
    let token = u32::try_from(token).map_err(|_| "Unity class token exceeds u32")?;
    let row = token & 0x00ff_ffff;
    if token >> 24 != 2 || row == 0 || rows < 0 || row > rows as u32 {
        return Err("Unity class lookup requires an in-range TypeDef token".into());
    }
    Ok(token)
}
// Frozen exports use gboolean (int32) except these two MonoBoolean (byte)
// class predicates. Keep the actual native ABI and typed metadata argument.
fn predicate_shape(function: &str) -> Option<(Kind, bool)> {
    match function {
        "mono_unity_class_is_string" | "mono_unity_class_is_class_type" => {
            Some((Kind::Class, true))
        }
        "mono_unity_class_is_open_constructed_type"
        | "mono_unity_class_is_array"
        | "mono_unity_class_is_delegate"
        | "mono_unity_class_is_inited" => Some((Kind::Class, false)),
        "mono_unity_type_is_generic_instance"
        | "mono_unity_type_is_enum_type"
        | "mono_unity_type_is_boolean"
        | "mono_unity_type_is_pointer_type"
        | "mono_unity_type_is_static" => Some((Kind::Type, false)),
        _ => None,
    }
}
pub(super) fn handles(function: &str) -> bool {
    CORE_CLASSES.contains(&function)
        || predicate_shape(function).is_some()
        || matches!(
            function,
            "mono_get_corlib"
                | "mono_class_from_name"
                | "mono_unity_class_get"
                | "mono_class_get_image"
                | "mono_image_get_name"
                | "mono_image_get_table_rows"
                | "mono_class_get_userdata"
                | "mono_class_set_userdata"
                | "mono_class_get_methods"
                | "mono_method_get_name"
                | "unity_mono_method_is_inflated"
                | "unity_mono_method_is_generic"
                | "mono_method_signature"
                | "mono_class_get_nested_types"
                | "mono_class_get_name"
                | "mono_class_get_namespace"
                | "mono_class_get_type"
                | "mono_signature_get_param_count"
                | "mono_signature_get_params"
                | "mono_signature_is_instance"
                | "mono_metadata_type_equal"
                | "mono_type_get_class"
                | "mono_class_from_mono_type"
                | "mono_method_get_class"
                | "mono_class_get_parent"
                | "mono_signature_get_return_type"
                | "mono_type_get_type"
                | "mono_type_is_byref"
                | "mono_class_get_flags"
                | "mono_class_is_valuetype"
                | "mono_class_is_enum"
                | "mono_class_is_generic"
                | "mono_class_is_inflated"
                | "mono_unity_class_is_abstract"
                | "mono_unity_class_is_interface"
                | "mono_unity_class_has_failure"
                | "mono_class_get_rank"
                | "mono_class_num_fields"
                | "mono_class_num_methods"
                | "mono_class_get_interfaces"
                | "mono_class_get_element_class"
                | "mono_class_get_nesting_type"
                | "mono_ptr_class_get"
                | "mono_array_class_get"
                | "mono_bounded_array_class_get"
                | "mono_class_is_subclass_of"
                | "mono_class_is_assignable_from"
                | "mono_class_enum_basetype"
                | "mono_class_get_type_token"
                | "mono_class_instance_size"
                | "mono_class_array_element_size"
                | "mono_array_element_size"
                | "mono_class_get_fields"
                | "mono_class_get_field_from_name"
                | "mono_class_get_method_from_name"
                | "mono_field_get_name"
                | "mono_field_get_flags"
                | "mono_field_get_offset"
                | "mono_field_get_type"
                | "mono_field_get_parent"
        )
}

#[cfg(test)]
mod tests {
    #[test]
    fn unity_class_tokens_reject_wrong_table_nil_row_out_of_range_and_truncation() {
        assert_eq!(
            super::type_definition_token(0x02000001, 29).unwrap(),
            0x02000001
        );
        for token in [0, 0x02000000, 0x0200001e, 0x01000001, 0x102000001] {
            assert!(super::type_definition_token(token, 29).is_err());
        }
        assert!(super::type_definition_token(0x02000001, -1).is_err());
    }

    use super::*;
    #[test]
    fn compact_string_arena_survives_large_metadata_scans_and_rejects_overlap_transactionally() {
        let mut registry = Registry::default();
        let mut memory = Memory::default();
        let mut first = 0;
        for index in 0..4097 {
            let text = CString::new(format!("Type{index}")).unwrap();
            let token = registry
                .wrap(&mut memory, Value::Text(text.clone()))
                .unwrap();
            assert_eq!(
                registry
                    .wrap(&mut memory, Value::Text(text.clone()))
                    .unwrap(),
                token
            );
            if index == 0 {
                first = token;
            }
            let mut bytes = vec![0u8; text.as_bytes_with_nul().len()];
            memory.read(token, &mut bytes).unwrap();
            assert_eq!(bytes, text.as_bytes_with_nul());
            assert!(memory.write_int(token, 1, 0).is_err());
        }
        assert!(registry.string_bytes < 65536 * 3);
        registry.string_bytes = ITER_BASE - STRING_BASE - 8;
        assert!(registry
            .wrap(&mut memory, Value::Text(CString::new("outside").unwrap()))
            .is_err());
        assert_eq!(memory.read_int(first, 1).unwrap(), b'T' as u64);
        registry.retire(&mut memory).unwrap();
        assert!(memory.read_int(first, 1).is_err());
        assert!(registry
            .wrap(&mut memory, Value::Text(CString::new("Type0").unwrap()))
            .is_err());
    }
    #[test]
    fn metadata_names_have_owned_zero_padding_for_vector_scans_without_relaxing_other_views() {
        let mut memory = Memory::default();
        let mut registry = Registry::default();
        let token = registry
            .wrap(&mut memory, Value::Text(CString::new("Bounds").unwrap()))
            .unwrap();
        let mut vector = [0xff; 16];
        memory.read(token, &mut vector).unwrap();
        assert_eq!(&vector[..6], b"Bounds");
        assert_eq!(&vector[6..], &[0; 10]);
        memory.read(token + 6, &mut vector).unwrap();
        assert_eq!(vector, [0; 16]);
        assert!(memory.read_int(token + 32, 1).is_err());
        assert!(memory.write_int(token + 7, 1, 1).is_err());
        let next = registry
            .wrap(&mut memory, Value::Text(CString::new("secret").unwrap()))
            .unwrap();
        assert_eq!(next, token + 32);
        memory.read(token + 6, &mut vector).unwrap();
        assert_eq!(vector, [0; 16]);
        registry.retire(&mut memory).unwrap();
        assert!(memory.read_int(token, 1).is_err());
        assert!(memory.read_int(next, 1).is_err());
    }
    #[test]
    fn metadata_class_exposes_only_its_owned_userdata_cell_and_retires_strings_and_handles() {
        let mut memory = Memory::default();
        let mut registry = Registry::default();
        let mut native = [0u64; 32];
        let token = registry
            .wrap(
                &mut memory,
                Value::Handle(native.as_mut_ptr().cast(), Kind::Class),
            )
            .unwrap();
        assert_eq!(
            registry
                .wrap(
                    &mut memory,
                    Value::Handle(native.as_mut_ptr().cast(), Kind::Class)
                )
                .unwrap(),
            token
        );
        let method = registry
            .wrap(
                &mut memory,
                Value::Handle(native.as_mut_ptr().cast(), Kind::Method),
            )
            .unwrap();
        assert_ne!(method, token);
        assert_eq!(
            registry
                .wrap(
                    &mut memory,
                    Value::Handle(native.as_mut_ptr().cast(), Kind::Method)
                )
                .unwrap(),
            method
        );
        assert!(registry.native(method, Kind::Class).is_err());
        memory.write_int(token + 232, 8, 0x12345).unwrap();
        assert_eq!(memory.read_int(token + 232, 8).unwrap(), 0x12345);
        assert_eq!(
            registry.userdata(&mut memory, token, None).unwrap(),
            0x12345
        );
        registry
            .userdata(&mut memory, token, Some(0xabcdef))
            .unwrap();
        assert_eq!(memory.read_int(token + 232, 8).unwrap(), 0xabcdef);
        assert_eq!(
            registry.userdata(&mut memory, token, None).unwrap(),
            0xabcdef
        );
        assert!(registry.userdata(&mut memory, token + 1, Some(0)).is_err());
        assert_eq!(native[29], 0);
        assert!(memory.read_int(token + 231, 1).is_err());
        assert!(memory.read_int(token + 240, 1).is_err());
        assert!(memory.fetch(token + 232, &mut [0]).is_err());
        assert!(registry.native(token, Kind::Method).is_err());
        let text = registry
            .wrap(&mut memory, Value::Text(CString::new("owned").unwrap()))
            .unwrap();
        assert_eq!(memory.read_int(text, 1).unwrap(), b'o' as u64);
        assert!(memory.write_int(text, 1, 0).is_err());
        registry.retire(&mut memory).unwrap();
        assert!(memory.read_int(token + 232, 8).is_err());
        assert!(memory.read_int(text, 1).is_err());
        assert!(registry.native(token, Kind::Class).is_err());
    }
    #[test]
    fn class_element_graph_uses_readonly_canonical_tokens_and_retires_exact_cells() {
        let mut memory = Memory::default();
        let mut registry = Registry::default();
        let mut array = [0u64; 32];
        let mut scalar = [0u64; 32];
        let scalar_native = scalar.as_mut_ptr().cast();
        let array_native = array.as_mut_ptr().cast();
        let token = registry
            .wrap(&mut memory, Value::Handle(array_native, Kind::Class))
            .unwrap();
        registry
            .publish_elements(&mut memory, token, |class| {
                assert!(class == array_native || class == scalar_native);
                Ok(scalar_native)
            })
            .unwrap();
        let scalar_token = memory.read_int(token, 8).unwrap();
        assert_ne!(scalar_token, scalar_native as u64);
        assert_eq!(
            registry.native(scalar_token, Kind::Class).unwrap(),
            scalar_native
        );
        assert_eq!(memory.read_int(scalar_token, 8).unwrap(), scalar_token);
        assert!(memory.write_int(token, 8, 0).is_err());
        assert!(memory.read_int(token + 8, 1).is_err());
        assert!(memory.fetch(token, &mut [0]).is_err());
        assert!(registry
            .publish_elements(&mut memory, token, |_| Ok(array_native))
            .is_err());
        registry.retire(&mut memory).unwrap();
        assert!(memory.read_int(token, 8).is_err());
        assert!(memory.read_int(scalar_token, 8).is_err());
    }
}
