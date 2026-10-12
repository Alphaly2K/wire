//! Verify that a constructed declaring type has only concrete type arguments.
//! This admits no methods and publishes no generic context or host pointers.
use super::Object;
#[derive(Clone, Copy)]
pub(super) struct Api {
    ty: unsafe extern "C" fn(Object) -> Object,
    kind: unsafe extern "C" fn(Object) -> u32,
    byref: unsafe extern "C" fn(Object) -> u32,
    definition: unsafe extern "C" fn(Object) -> i32,
    count: unsafe extern "C" fn(Object) -> u32,
    argument: unsafe extern "C" fn(Object, u32) -> Object,
    element: unsafe extern "C" fn(Object) -> Object,
}
impl Api {
    pub(super) fn load(library: Object) -> Result<Self, String> {
        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return Err(concat!("missing closed type metadata API: ", $name).into());
                }
                unsafe { std::mem::transmute::<Object, $ty>(p) }
            }};
        }
        Ok(Self {
            ty: sym!(
                "mono_class_get_type",
                unsafe extern "C" fn(Object) -> Object
            ),
            kind: sym!("mono_type_get_type", unsafe extern "C" fn(Object) -> u32),
            byref: sym!("mono_type_is_byref", unsafe extern "C" fn(Object) -> u32),
            definition: sym!("mono_class_is_generic", unsafe extern "C" fn(Object) -> i32),
            count: sym!(
                "mono_unity_class_get_generic_argument_count",
                unsafe extern "C" fn(Object) -> u32
            ),
            argument: sym!(
                "mono_unity_class_get_generic_argument_at",
                unsafe extern "C" fn(Object, u32) -> Object
            ),
            element: sym!(
                "mono_class_get_element_class",
                unsafe extern "C" fn(Object) -> Object
            ),
        })
    }
    pub(super) fn concrete(self, class: Object) -> Result<(), String> {
        self.walk(class, &mut Vec::new(), &mut 0)
    }
    pub(super) fn constructed(self, class: Object) -> Result<(), String> {
        if class.is_null() {
            return Err("NULL declaring type".into());
        }
        let ty = unsafe { (self.ty)(class) };
        if ty.is_null() || unsafe { (self.kind)(ty) } != 0x15 {
            return Err("inflated entry requires a constructed declaring class".into());
        }
        self.walk(class, &mut Vec::new(), &mut 0)
    }
    fn walk(
        self,
        class: Object,
        path: &mut Vec<Object>,
        visited: &mut usize,
    ) -> Result<(), String> {
        if class.is_null() || path.contains(&class) || path.len() >= 8 || *visited >= 128 {
            return Err("closed type exceeds bounded acyclic metadata contract".into());
        }
        *visited += 1;
        let ty = unsafe { (self.ty)(class) };
        if ty.is_null()
            || unsafe { (self.byref)(ty) } != 0
            || unsafe { (self.definition)(class) } != 0
        {
            return Err("declaring type contains an open definition/byref".into());
        }
        path.push(class);
        let result = match unsafe { (self.kind)(ty) } {
            // Native integers are concrete CLI primitives. This only proves
            // type closure; invoke/value-layout pointer admission stays separate.
            2..=0xe | 0x11 | 0x12 | 0x18 | 0x19 | 0x1c => Ok(()),
            0x14 | 0x1d => self.walk(unsafe { (self.element)(class) }, path, visited),
            0x15 => {
                let count = unsafe { (self.count)(class) };
                if !(1..=16).contains(&count) {
                    return Err("constructed type argument count outside bounds".into());
                }
                for index in 0..count {
                    self.walk(unsafe { (self.argument)(class, index) }, path, visited)?;
                }
                Ok(())
            }
            _ => Err("declaring type contains a generic parameter/pointer/unknown shape".into()),
        };
        path.pop();
        result
    }
}
#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::cell::Cell;
    thread_local! { static MODE: Cell<u32> = const { Cell::new(0) }; }
    unsafe extern "C" fn identity(p: Object) -> Object {
        p
    }
    unsafe extern "C" fn kind(p: Object) -> u32 {
        match p as usize {
            2 => 0x15,
            3 if MODE.with(Cell::get) == 1 => 0x13,
            3 if MODE.with(Cell::get) == 5 => 0x18,
            3 if MODE.with(Cell::get) == 6 => 0xf,
            3 => 8,
            4 => 0x1d,
            _ => 0x12,
        }
    }
    unsafe extern "C" fn zero(_: Object) -> u32 {
        0
    }
    unsafe extern "C" fn definition(_: Object) -> i32 {
        i32::from(MODE.with(Cell::get) == 2)
    }
    unsafe extern "C" fn count(_: Object) -> u32 {
        if MODE.with(Cell::get) == 3 {
            17
        } else {
            1
        }
    }
    unsafe extern "C" fn argument(_: Object, _: u32) -> Object {
        (if MODE.with(Cell::get) == 4 { 2 } else { 4 }) as Object
    }
    unsafe extern "C" fn element(_: Object) -> Object {
        3usize as Object
    }
    pub(in crate::x64::process::mono_startup) fn api() -> Api {
        MODE.with(|m| m.set(0));
        Api {
            ty: identity,
            kind,
            byref: zero,
            definition,
            count,
            argument,
            element,
        }
    }
    #[test]
    fn closed_arguments_are_recursive_and_open_or_cyclic_arguments_reject() {
        let api = api();
        api.constructed(2usize as Object).unwrap();
        for mode in 1..=4 {
            MODE.with(|m| m.set(mode));
            assert!(api.constructed(2usize as Object).is_err());
        }
        MODE.with(|m| m.set(0));
        assert!(api.constructed(3usize as Object).is_err());
        assert!(api.constructed(std::ptr::null_mut()).is_err());
        MODE.with(|m| m.set(5));
        api.concrete(4usize as Object).unwrap();
        api.constructed(2usize as Object).unwrap();
        MODE.with(|m| m.set(6));
        assert!(api.concrete(4usize as Object).is_err());
        assert!(api.constructed(2usize as Object).is_err());
    }
}
