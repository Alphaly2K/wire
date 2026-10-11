//! Resolve metadata against an already rooted reference object. No method is
//! invoked and no vtable/code address is published to the guest.
use super::Object;
#[derive(Clone, Copy)]
pub(super) struct Api {
    object_class: unsafe extern "C" fn(Object) -> Object,
    method_class: unsafe extern "C" fn(Object) -> Object,
    flags: unsafe extern "C" fn(Object, *mut u32) -> u32,
    assignable: unsafe extern "C" fn(Object, Object) -> i32,
    value_type: unsafe extern "C" fn(Object) -> i32,
    resolve: unsafe extern "C" fn(Object, Object) -> Object,
}
impl Api {
    pub(super) fn load(library: Object) -> Result<Self, String> {
        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return Err(concat!("missing virtual metadata symbol: ", $name).into());
                }
                unsafe { std::mem::transmute::<Object, $ty>(p) }
            }};
        }
        Ok(Self {
            object_class: sym!(
                "mono_object_get_class",
                unsafe extern "C" fn(Object) -> Object
            ),
            method_class: sym!(
                "mono_method_get_class",
                unsafe extern "C" fn(Object) -> Object
            ),
            flags: sym!(
                "mono_method_get_flags",
                unsafe extern "C" fn(Object, *mut u32) -> u32
            ),
            assignable: sym!(
                "mono_class_is_assignable_from",
                unsafe extern "C" fn(Object, Object) -> i32
            ),
            value_type: sym!(
                "mono_class_is_valuetype",
                unsafe extern "C" fn(Object) -> i32
            ),
            resolve: sym!(
                "mono_object_get_virtual_method",
                unsafe extern "C" fn(Object, Object) -> Object
            ),
        })
    }
    fn compatible_method(self, class: Object, method: Object) -> Result<(), String> {
        if method.is_null() {
            return Err("virtual metadata method absent".into());
        }
        let parent = unsafe { (self.method_class)(method) };
        if parent.is_null()
            || unsafe { (self.flags)(method, std::ptr::null_mut()) } & 0x10 != 0
            || unsafe { (self.assignable)(parent, class) } == 0
        {
            return Err("virtual metadata receiver/declaration/static contract mismatch".into());
        }
        Ok(())
    }
    pub(super) fn resolve(
        self,
        object: Object,
        class: Object,
        method: Object,
    ) -> Result<Object, String> {
        if object.is_null()
            || class.is_null()
            || unsafe { (self.object_class)(object) } != class
            || unsafe { (self.value_type)(class) } != 0
        {
            return Err("virtual metadata requires a rooted reference receiver".into());
        }
        self.compatible_method(class, method)?;
        let result = unsafe { (self.resolve)(object, method) };
        self.compatible_method(class, result)?;
        Ok(result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    thread_local! { static CALLS: Cell<usize> = const { Cell::new(0) }; }
    unsafe extern "C" fn object_class(_: Object) -> Object {
        4usize as Object
    }
    unsafe extern "C" fn method_class(method: Object) -> Object {
        if method as usize == 20 {
            6usize as Object
        } else {
            2usize as Object
        }
    }
    unsafe extern "C" fn flags(method: Object, _: *mut u32) -> u32 {
        if method as usize == 24 {
            0x10
        } else {
            0x40
        }
    }
    unsafe extern "C" fn assignable(parent: Object, class: Object) -> i32 {
        i32::from(parent as usize == 2 && class as usize == 4)
    }
    unsafe extern "C" fn value_type(_: Object) -> i32 {
        0
    }
    unsafe extern "C" fn resolve(_: Object, _: Object) -> Object {
        CALLS.with(|c| c.set(c.get() + 1));
        16usize as Object
    }
    #[test]
    fn unrelated_static_and_unrooted_receivers_never_reach_native_resolver() {
        CALLS.with(|c| c.set(0));
        let api = Api {
            object_class,
            method_class,
            flags,
            assignable,
            value_type,
            resolve,
        };
        for (obj, class, method) in [
            (0, 4, 12),
            (8, 0, 12),
            (8, 6, 12),
            (8, 4, 0),
            (8, 4, 20),
            (8, 4, 24),
        ] {
            assert!(api
                .resolve(obj as Object, class as Object, method as Object)
                .is_err());
        }
        assert_eq!(CALLS.with(Cell::get), 0);
        assert_eq!(
            api.resolve(8usize as Object, 4usize as Object, 12usize as Object)
                .unwrap(),
            16usize as Object
        );
        assert_eq!(CALLS.with(Cell::get), 1);
        unsafe extern "C" fn unrelated(_: Object, _: Object) -> Object {
            20usize as Object
        }
        assert!(Api {
            resolve: unrelated,
            ..api
        }
        .resolve(8usize as Object, 4usize as Object, 12usize as Object)
        .is_err());
    }
}
