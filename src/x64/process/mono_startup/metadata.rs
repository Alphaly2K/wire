//! Typed metadata handles with owned strings/iterators. Class memory exposes
//! only the exact 8B embedding userdata cell; all neighbouring bytes fault.
use super::super::super::memory::{shared::HostRegion, READ, WRITE};
use super::{c_char, CString, Memory, Object};
use std::{collections::BTreeMap, ptr};
const HANDLE_BASE: u64 = 0x6ffe40000000;
const STRING_BASE: u64 = 0x6ffe50000000;
const ITER_BASE: u64 = 0x6ffe60000000;
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Class,
    Method,
    Signature,
    Type,
}
struct Entry {
    kind: Kind,
    native: Object,
    userdata: Option<HostRegion>,
}
struct Iterator {
    parent: u64,
    function: String,
    native: Object,
}
#[derive(Default)]
pub(super) struct Registry {
    handles: BTreeMap<u64, Entry>,
    strings: BTreeMap<CString, (u64, HostRegion)>,
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
                if self.strings.len() >= 4096 {
                    return Err("metadata string capacity exceeded".into());
                }
                let token = STRING_BASE + self.strings.len() as u64 * 65536;
                let region = HostRegion::owned(text.as_bytes_with_nul().len())?;
                region.with_native(|p| unsafe {
                    ptr::copy_nonoverlapping(
                        text.as_ptr().cast(),
                        p.as_ptr(),
                        text.as_bytes_with_nul().len(),
                    );
                })?;
                memory.map_shared_data(token, region.clone(), READ, READ)?;
                self.strings.insert(text, (token, region));
                Ok(token)
            }
            Value::Handle(native, kind) => {
                if native.is_null() {
                    return Ok(0);
                }
                if let Some((token, _)) = self
                    .handles
                    .iter()
                    .find(|(_, entry)| entry.kind == kind && entry.native == native)
                {
                    return Ok(*token);
                }
                if self.handles.len() >= 32768 {
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
                    },
                );
                Ok(token)
            }
        }
    }
    pub(super) fn query(
        &mut self,
        lib: Object,
        function: &str,
        a: u64,
        b: u64,
    ) -> Result<Value, String> {
        self.queries += 1;
        match function {
            "mono_metadata_type_equal" => {
                let left = self.native(a, Kind::Type)?;
                let right = self.native(b, Kind::Type)?;
                let f = sym!(lib, function, unsafe extern "C" fn(Object, Object) -> u32);
                Ok(Value::Scalar(unsafe { f(left, right) } as u64))
            }
            "mono_type_get_class"
            | "mono_class_from_mono_type"
            | "mono_method_get_class"
            | "mono_class_get_parent" => {
                let kind = if function.starts_with("mono_method") {
                    Kind::Method
                } else if function == "mono_class_get_parent" {
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
            | "mono_class_get_rank"
            | "mono_class_num_fields"
            | "mono_class_num_methods" => {
                let kind = if function.starts_with("mono_type") {
                    Kind::Type
                } else {
                    Kind::Class
                };
                let native = self.native(a, kind)?;
                let f = sym!(lib, function, unsafe extern "C" fn(Object) -> u32);
                Ok(Value::Scalar(unsafe { f(native) } as u64))
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
            "mono_class_get_name" | "mono_class_get_namespace" | "mono_method_get_name" => {
                let kind = if function != "mono_method_get_name" {
                    Kind::Class
                } else {
                    Kind::Method
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
        } else if function == "mono_class_get_nested_types" {
            (Kind::Class, Kind::Class)
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
        format!("native-metadata-queries={}\nnative-metadata-handles={}\nnative-metadata-strings={}\nnative-metadata-retired={}\n",self.queries,self.handles.len(),self.strings.len(),self.retired)
    }
}
pub(super) fn handles(function: &str) -> bool {
    matches!(
        function,
        "mono_get_corlib"
            | "mono_class_from_name"
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
            | "mono_class_get_rank"
            | "mono_class_num_fields"
            | "mono_class_num_methods"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
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
        memory.write_int(token + 232, 8, 0x12345).unwrap();
        assert_eq!(memory.read_int(token + 232, 8).unwrap(), 0x12345);
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
}
