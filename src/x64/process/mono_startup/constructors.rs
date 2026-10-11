//! Metadata-qualified constructors and serialization-interface callbacks. Class names and
//! method tokens are diagnostics, never admission rules. All native crossings
//! still use the provider's fixed adapters and rejecting P/Invoke resolver.
use super::{c_char, closed_types, Object};
use std::ffi::CStr;
#[derive(Clone, Copy)]
pub(super) struct Api {
    class: unsafe extern "C" fn(Object) -> Object,
    image: unsafe extern "C" fn(Object) -> Object,
    name: unsafe extern "C" fn(Object) -> *const c_char,
    class_name: unsafe extern "C" fn(Object) -> *const c_char,
    flags: unsafe extern "C" fn(Object, *mut u32) -> u32,
    signature: unsafe extern "C" fn(Object) -> Object,
    count: unsafe extern "C" fn(Object) -> u32,
    instance: unsafe extern "C" fn(Object) -> u32,
    returns: unsafe extern "C" fn(Object) -> Object,
    kind: unsafe extern "C" fn(Object) -> u32,
    byref: unsafe extern "C" fn(Object) -> u32,
    generic: unsafe extern "C" fn(Object) -> u32,
    inflated: unsafe extern "C" fn(Object) -> u32,
    value: unsafe extern "C" fn(Object) -> i32,
    object_class: unsafe extern "C" fn(Object) -> Object,
    header: unsafe extern "C" fn(Object) -> Object,
    code: unsafe extern "C" fn(Object, *mut u32, *mut u32) -> *const u8,
    free_header: unsafe extern "C" fn(Object),
    find: unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object,
    method: unsafe extern "C" fn(Object, *const c_char, i32) -> Object,
    assignable: unsafe extern "C" fn(Object, Object) -> i32,
    resolve: unsafe extern "C" fn(Object, Object) -> Object,
    context: unsafe extern "C" fn(Object) -> Object,
    closed: Option<closed_types::Api>,
}
pub(super) struct Descriptor {
    pub(super) class: Object,
    pub(super) image: Object,
    pub(super) name: String,
    pub(super) callback: Option<Object>,
    method: Object,
}
impl Api {
    pub(super) fn load(library: Object) -> Result<Self, String> {
        macro_rules! sym {
            ($name:literal,$ty:ty) => {{
                let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return Err(concat!("missing constructor metadata API: ", $name).into());
                }
                unsafe { std::mem::transmute::<Object, $ty>(p) }
            }};
        }
        Ok(Self {
            class: sym!(
                "mono_method_get_class",
                unsafe extern "C" fn(Object) -> Object
            ),
            image: sym!(
                "mono_class_get_image",
                unsafe extern "C" fn(Object) -> Object
            ),
            name: sym!(
                "mono_method_get_name",
                unsafe extern "C" fn(Object) -> *const c_char
            ),
            class_name: sym!(
                "mono_class_get_name",
                unsafe extern "C" fn(Object) -> *const c_char
            ),
            flags: sym!(
                "mono_method_get_flags",
                unsafe extern "C" fn(Object, *mut u32) -> u32
            ),
            signature: sym!(
                "mono_method_signature",
                unsafe extern "C" fn(Object) -> Object
            ),
            count: sym!(
                "mono_signature_get_param_count",
                unsafe extern "C" fn(Object) -> u32
            ),
            instance: sym!(
                "mono_signature_is_instance",
                unsafe extern "C" fn(Object) -> u32
            ),
            returns: sym!(
                "mono_signature_get_return_type",
                unsafe extern "C" fn(Object) -> Object
            ),
            kind: sym!("mono_type_get_type", unsafe extern "C" fn(Object) -> u32),
            byref: sym!("mono_type_is_byref", unsafe extern "C" fn(Object) -> u32),
            generic: sym!(
                "unity_mono_method_is_generic",
                unsafe extern "C" fn(Object) -> u32
            ),
            inflated: sym!(
                "unity_mono_method_is_inflated",
                unsafe extern "C" fn(Object) -> u32
            ),
            value: sym!(
                "mono_class_is_valuetype",
                unsafe extern "C" fn(Object) -> i32
            ),
            object_class: sym!(
                "mono_object_get_class",
                unsafe extern "C" fn(Object) -> Object
            ),
            header: sym!(
                "mono_method_get_header",
                unsafe extern "C" fn(Object) -> Object
            ),
            code: sym!(
                "mono_method_header_get_code",
                unsafe extern "C" fn(Object, *mut u32, *mut u32) -> *const u8
            ),
            free_header: sym!("mono_metadata_free_mh", unsafe extern "C" fn(Object)),
            find: sym!(
                "mono_class_from_name",
                unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object
            ),
            method: sym!(
                "mono_class_get_method_from_name",
                unsafe extern "C" fn(Object, *const c_char, i32) -> Object
            ),
            assignable: sym!(
                "mono_class_is_assignable_from",
                unsafe extern "C" fn(Object, Object) -> i32
            ),
            context: sym!(
                "mono_method_get_context",
                unsafe extern "C" fn(Object) -> Object
            ),
            closed: Some(closed_types::Api::load(library)?),
            resolve: sym!(
                "mono_object_get_virtual_method",
                unsafe extern "C" fn(Object, Object) -> Object
            ),
        })
    }
    pub(super) fn describe(
        self,
        method: Object,
        images: &[Object],
    ) -> Result<Option<Descriptor>, String> {
        if method.is_null() {
            return Err("NULL constructor method identity".into());
        }
        let name = unsafe { (self.name)(method) };
        if name.is_null() {
            return Err("constructor method has no name".into());
        }
        let bytes = unsafe { CStr::from_ptr(name) }.to_bytes();
        let callback_name = bytes
            .strip_prefix(b"UnityEngine.ISerializationCallbackReceiver.")
            .unwrap_or(bytes);
        let serialization = match callback_name {
            b"OnAfterDeserialize" => Some(c"OnAfterDeserialize"),
            b"OnBeforeSerialize" => Some(c"OnBeforeSerialize"),
            _ => None,
        };
        if bytes != b".ctor" && serialization.is_none() {
            return Ok(None);
        }
        let class = unsafe { (self.class)(method) };
        if class.is_null() {
            return Err("constructor declaring class absent".into());
        }
        let image = unsafe { (self.image)(class) };
        if image.is_null() || !images.contains(&image) {
            return Err("constructor outside fingerprinted owned images".into());
        }
        let callback = if let Some(name) = serialization {
            let core = *images
                .first()
                .ok_or("serialization callback requires owned core image")?;
            let interface = unsafe {
                (self.find)(
                    core,
                    c"UnityEngine".as_ptr(),
                    c"ISerializationCallbackReceiver".as_ptr(),
                )
            };
            if interface.is_null() || unsafe { (self.assignable)(interface, class) } == 0 {
                return Err(
                    "managed callback does not implement the serialization interface".into(),
                );
            }
            let contract = unsafe { (self.method)(interface, name.as_ptr(), 0) };
            if contract.is_null() {
                return Err("serialization interface method absent".into());
            }
            Some(contract)
        } else {
            None
        };
        let mut implementation = 0;
        let flags = unsafe { (self.flags)(method, &mut implementation) };
        let sig = unsafe { (self.signature)(method) };
        // No static/abstract/PInvoke, synchronized execution, native/runtime
        // body or unmanaged entry. Value-type this needs a separate typed ABI.
        if flags & (0x10 | 0x400 | 0x2000) != 0
            || implementation & (0x3 | 0x4 | 0x20 | 0x1000) != 0
            || sig.is_null()
            || unsafe { (self.instance)(sig) } == 0
            || unsafe { (self.count)(sig) } != 0
            || unsafe { (self.generic)(method) } != 0
            || unsafe { (self.value)(class) } != 0
        {
            return Err(
                "constructor requires nongeneric managed instance void() on a reference class"
                    .into(),
            );
        }
        if unsafe { (self.inflated)(method) } != 0 {
            // Frozen MonoGenericContext is exactly class_inst/method_inst.
            // Read only the native context returned for this owned method;
            // method-specific generics remain unsupported.
            let context = unsafe { (self.context)(method) };
            if context.is_null() || !(context as usize).is_multiple_of(8) {
                return Err("inflated method has no native generic context".into());
            }
            let instances = unsafe { std::slice::from_raw_parts(context.cast::<Object>(), 2) };
            if instances[0].is_null() || !instances[1].is_null() {
                return Err("managed entry requires class-only generic inflation".into());
            }
            self.closed
                .ok_or("closed declaring type metadata API absent")?
                .constructed(class)?;
        }
        let ret = unsafe { (self.returns)(sig) };
        if ret.is_null() || unsafe { (self.kind)(ret) } != 1 || unsafe { (self.byref)(ret) } != 0 {
            return Err("constructor return is not plain void".into());
        }
        let header = unsafe { (self.header)(method) };
        if header.is_null() {
            return Err("constructor has no managed IL body".into());
        }
        let mut size = 0;
        let mut stack = 0;
        let code = unsafe { (self.code)(header, &mut size, &mut stack) };
        let valid = !code.is_null() && (1..=65536).contains(&size) && stack <= 1024;
        unsafe {
            (self.free_header)(header);
        }
        if !valid {
            return Err("constructor IL body exceeds bounded metadata contract".into());
        }
        let name = unsafe { (self.class_name)(class) };
        if name.is_null() {
            return Err("constructor class has no diagnostic name".into());
        }
        Ok(Some(Descriptor {
            class,
            image,
            callback,
            method,
            name: unsafe { CStr::from_ptr(name) }
                .to_string_lossy()
                .into_owned(),
        }))
    }
    pub(super) fn receiver(
        self,
        descriptor: &Descriptor,
        object: Object,
        class: Object,
    ) -> Result<(), String> {
        if object.is_null()
            || class.is_null()
            || unsafe { (self.object_class)(object) } != class
            || unsafe { (self.value)(class) } != 0
        {
            return Err("managed receiver differs from live rooted reference class".into());
        }
        if let Some(closed) = self.closed {
            closed.concrete(class)?;
        }
        if let Some(contract) = descriptor.callback {
            // An inherited interface implementation can declare a base class;
            // Mono's actual virtual resolution is the execution contract.
            let interface = unsafe { (self.class)(contract) };
            if interface.is_null()
                || unsafe { (self.assignable)(interface, class) } == 0
                || unsafe { (self.resolve)(object, contract) } != descriptor.method
            {
                return Err(
                    "managed callback is not the receiver's resolved interface implementation"
                        .into(),
                );
            }
        } else if class != descriptor.class {
            return Err("constructor receiver differs from rooted declaring class".into());
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, ptr};
    thread_local! {
        static METHOD_INSTANTIATION: Cell<bool> = const { Cell::new(false) };
        static FLAGS: Cell<u32> = const { Cell::new(0) };
        static IMPL: Cell<u32> = const { Cell::new(0) };
        static COUNT: Cell<u32> = const { Cell::new(0) };
        static VALUE: Cell<i32> = const { Cell::new(0) };
        static NAME: Cell<bool> = const { Cell::new(true) };
        static HEADERS: Cell<usize> = const { Cell::new(0) };
    }
    unsafe extern "C" fn class(_: Object) -> Object {
        2usize as Object
    }
    unsafe extern "C" fn image(_: Object) -> Object {
        3usize as Object
    }
    unsafe extern "C" fn name(_: Object) -> *const c_char {
        if NAME.with(Cell::get) {
            c".ctor".as_ptr()
        } else {
            c"Other".as_ptr()
        }
    }
    unsafe extern "C" fn class_name(_: Object) -> *const c_char {
        c"PreviouslyUnknownClass".as_ptr()
    }
    unsafe extern "C" fn flags(_: Object, out: *mut u32) -> u32 {
        unsafe {
            out.write(IMPL.with(Cell::get));
        }
        FLAGS.with(Cell::get)
    }
    unsafe extern "C" fn count(_: Object) -> u32 {
        COUNT.with(Cell::get)
    }
    unsafe extern "C" fn one(_: Object) -> u32 {
        1
    }
    unsafe extern "C" fn zero(_: Object) -> u32 {
        0
    }
    unsafe extern "C" fn value(_: Object) -> i32 {
        VALUE.with(Cell::get)
    }
    unsafe extern "C" fn header(_: Object) -> Object {
        HEADERS.with(|c| c.set(c.get() + 1));
        4usize as Object
    }
    unsafe extern "C" fn code(_: Object, size: *mut u32, stack: *mut u32) -> *const u8 {
        unsafe {
            size.write(1);
            stack.write(1);
        }
        c"x".as_ptr().cast()
    }
    unsafe extern "C" fn free(_: Object) {
        HEADERS.with(|c| c.set(c.get() - 1));
    }
    thread_local! {static INTERFACE:Cell<bool>=const{Cell::new(true)};static RESOLVED:Cell<usize>=const{Cell::new(5)};}
    unsafe extern "C" fn find(_: Object, _: *const c_char, _: *const c_char) -> Object {
        6usize as Object
    }
    unsafe extern "C" fn method(_: Object, _: *const c_char, _: i32) -> Object {
        7usize as Object
    }
    unsafe extern "C" fn assignable(_: Object, _: Object) -> i32 {
        i32::from(INTERFACE.with(Cell::get))
    }
    unsafe extern "C" fn resolve(_: Object, _: Object) -> Object {
        RESOLVED.with(Cell::get) as Object
    }
    unsafe extern "C" fn context(_: Object) -> Object {
        static CONTEXT: [usize; 2] = [1, 0];
        static METHOD_CONTEXT: [usize; 2] = [1, 1];
        if METHOD_INSTANTIATION.with(Cell::get) {
            METHOD_CONTEXT.as_ptr().cast_mut().cast()
        } else {
            CONTEXT.as_ptr().cast_mut().cast()
        }
    }
    unsafe extern "C" fn serialization_name(_: Object) -> *const c_char {
        c"OnAfterDeserialize".as_ptr()
    }
    fn api() -> Api {
        INTERFACE.with(|c| c.set(true));
        RESOLVED.with(|c| c.set(5));
        METHOD_INSTANTIATION.with(|c| c.set(false));
        FLAGS.with(|c| c.set(0));
        IMPL.with(|c| c.set(0));
        COUNT.with(|c| c.set(0));
        VALUE.with(|c| c.set(0));
        NAME.with(|c| c.set(true));
        HEADERS.with(|c| c.set(0));
        Api {
            class,
            image,
            name,
            class_name,
            flags,
            signature: class,
            count,
            instance: one,
            returns: class,
            kind: one,
            byref: zero,
            generic: zero,
            inflated: zero,
            value,
            object_class: class,
            header,
            code,
            free_header: free,
            find,
            method,
            assignable,
            resolve,
            context,
            closed: Some(closed_types::tests::api()),
        }
    }
    #[test]
    fn inherited_callback_requires_actual_interface_resolution() {
        let api = Api {
            name: serialization_name,
            ..api()
        };
        let descriptor = api
            .describe(5usize as Object, &[3usize as Object])
            .unwrap()
            .unwrap();
        unsafe extern "C" fn derived(_: Object) -> Object {
            8usize as Object
        }
        let api = Api {
            object_class: derived,
            ..api
        };
        api.receiver(&descriptor, 9usize as Object, 8usize as Object)
            .unwrap();
        RESOLVED.with(|c| c.set(10));
        assert!(api
            .receiver(&descriptor, 9usize as Object, 8usize as Object)
            .is_err());
        RESOLVED.with(|c| c.set(5));
        INTERFACE.with(|c| c.set(false));
        assert!(api
            .receiver(&descriptor, 9usize as Object, 8usize as Object)
            .is_err());
    }
    #[test]
    fn closed_class_inflation_is_admitted_but_method_instantiation_is_not() {
        let api = Api {
            inflated: one,
            ..api()
        };
        let descriptor = api
            .describe(5usize as Object, &[3usize as Object])
            .unwrap()
            .unwrap();
        api.receiver(&descriptor, 8usize as Object, 2usize as Object)
            .unwrap();
        METHOD_INSTANTIATION.with(|c| c.set(true));
        assert!(api.describe(5usize as Object, &[3usize as Object]).is_err());
        assert_eq!(HEADERS.with(Cell::get), 0);
    }
    #[test]
    fn callbacks_require_actual_interface_and_resolved_receiver_implementation() {
        INTERFACE.with(|c| c.set(true));
        RESOLVED.with(|c| c.set(5));
        let api = Api {
            name: serialization_name,
            ..api()
        };
        let descriptor = api
            .describe(5usize as Object, &[3usize as Object])
            .unwrap()
            .unwrap();
        assert!(descriptor.callback.is_some());
        api.receiver(&descriptor, 9usize as Object, 2usize as Object)
            .unwrap();
        RESOLVED.with(|c| c.set(99));
        assert!(api
            .receiver(&descriptor, 9usize as Object, 2usize as Object)
            .is_err());
        INTERFACE.with(|c| c.set(false));
        assert!(api.describe(5usize as Object, &[3usize as Object]).is_err());
        assert_eq!(HEADERS.with(Cell::get), 0);
    }
    #[test]
    fn unknown_class_uses_metadata_not_name_or_method_token() {
        let api = api();
        let desc = api
            .describe(12345usize as Object, &[3usize as Object])
            .unwrap()
            .unwrap();
        assert_eq!(desc.name, "PreviouslyUnknownClass");
        api.receiver(&desc, 9usize as Object, 2usize as Object)
            .unwrap();
        assert_eq!(HEADERS.with(Cell::get), 0);
        assert!(api.receiver(&desc, ptr::null_mut(), desc.class).is_err());
        assert!(api
            .receiver(&desc, 9usize as Object, 8usize as Object)
            .is_err());
    }
    #[test]
    fn invalid_constructor_is_rejected_before_body_or_call() {
        let api = api();
        let method = 5usize as Object;
        let images = [3usize as Object];
        assert!(api.describe(ptr::null_mut(), &images).is_err());
        assert!(api.describe(method, &[7usize as Object]).is_err());
        for flags in [0x10, 0x400, 0x2000] {
            FLAGS.with(|c| c.set(flags));
            assert!(api.describe(method, &images).is_err());
        }
        FLAGS.with(|c| c.set(0));
        for flags in [1, 3, 4, 0x20, 0x1000] {
            IMPL.with(|c| c.set(flags));
            assert!(api.describe(method, &images).is_err());
        }
        IMPL.with(|c| c.set(0));
        COUNT.with(|c| c.set(1));
        assert!(api.describe(method, &images).is_err());
        COUNT.with(|c| c.set(0));
        VALUE.with(|c| c.set(1));
        assert!(api.describe(method, &images).is_err());
        VALUE.with(|c| c.set(0));
        NAME.with(|c| c.set(false));
        assert!(api.describe(method, &images).unwrap().is_none());
        assert_eq!(HEADERS.with(Cell::get), 0);
    }
}
