//! Metadata-qualified, zero-argument managed constructors. Class names and
//! method tokens are diagnostics, never admission rules. All native crossings
//! still use the provider's fixed adapters and rejecting P/Invoke resolver.
use super::{c_char, Object};
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
}
pub(super) struct Descriptor {
    pub(super) class: Object,
    pub(super) image: Object,
    pub(super) name: String,
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
        if unsafe { CStr::from_ptr(name) } != c".ctor" {
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
            || unsafe { (self.inflated)(method) } != 0
            || unsafe { (self.value)(class) } != 0
        {
            return Err(
                "constructor requires nongeneric managed instance void() on a reference class"
                    .into(),
            );
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
            || class != descriptor.class
            || unsafe { (self.object_class)(object) } != class
        {
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
    fn api() -> Api {
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
        }
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
