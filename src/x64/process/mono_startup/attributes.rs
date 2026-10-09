//! Checked custom-attribute queries and opaque rooted instances. Info ownership
//! and managed roots are separate; no native object header or pointer is mapped.
use super::{c_char, Object};
use std::{collections::BTreeMap, ptr};
#[derive(Clone, Copy)]
pub(super) struct Api {
    from_class: unsafe extern "C" fn(Object, *mut u16) -> Object,
    from_field: unsafe extern "C" fn(Object, Object, *mut u16) -> Object,
    has: unsafe extern "C" fn(Object, Object) -> i32,
    get: unsafe extern "C" fn(Object, Object, *mut u16) -> Object,
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
        match unsafe{(self.kind)(ty)} {
            2|4|5=>Ok(1),3|6|7=>Ok(2),8|9|12=>Ok(4),10|11|13=>Ok(8),
            _=>Err("attribute field requires primitive/enum bytes; references and pointers not admitted".into()),
        }
    }
}
struct Root {
    native: Object,
    class: Object,
    handle: Object,
}
#[derive(Default)]
pub(super) struct Registry {
    infos: BTreeMap<u64, Object>,
    objects: BTreeMap<u64, Root>,
    field_checks: BTreeMap<(u64, usize), Vec<u8>>,
    issued: usize,
    queries: usize,
    reads: usize,
    verified: usize,
    released: bool,
}
impl Registry {
    pub(super) fn contains(&self, token: u64) -> bool {
        !self.released && self.objects.contains_key(&token)
    }
    pub(super) fn native(&self, api: Api, token: u64) -> Result<Object, String> {
        if self.released {
            return Err("attribute objects released".into());
        }
        let root = self
            .objects
            .get(&token)
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
        Ok(self.objects[&token].class)
    }
    fn info(&self, token: u64) -> Result<Object, String> {
        if self.released {
            return Err("attribute info registry released".into());
        }
        self.infos
            .get(&token)
            .copied()
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
        self.queries += 1;
        if info.is_null() {
            return Ok(0);
        }
        // The locked non-dynamic image path returns a fresh owned info. Keep
        // an existing owner intact if a future runtime violates that contract.
        if self.infos.values().any(|&existing| existing == info) {
            return Err("duplicate live attribute info ownership".into());
        }
        let token = 0x6ffee0000000 + self.issued as u64 * 16;
        self.issued += 1;
        self.infos.insert(token, info);
        Ok(token)
    }
    pub(super) fn has(&self, api: Api, token: u64, class: Object) -> Result<u64, String> {
        Ok(unsafe { (api.has)(self.info(token)?, class) } as u32 as u64)
    }
    pub(super) fn get(&mut self, api: Api, token: u64, class: Object) -> Result<u64, String> {
        let mut status = 65535;
        let object = unsafe { (api.get)(self.info(token)?, class, &mut status) };
        if status != 0 {
            return Err(format!(
                "native checked attribute constructor error={status}"
            ));
        }
        if object.is_null() {
            return Ok(0);
        }
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
    pub(super) fn read_field(
        &mut self,
        api: Api,
        token: u64,
        field: Object,
    ) -> Result<Vec<u8>, String> {
        let object = self.native(api, token)?;
        let parent = unsafe { (api.field_parent)(field) };
        if parent.is_null() || unsafe { (api.assignable)(parent, self.objects[&token].class) } == 0
        {
            return Err("attribute field belongs to a different object type".into());
        }
        let width = api.field_width(field)?;
        let mut bytes = vec![0u8; width];
        unsafe { (api.field_get)(object, field, bytes.as_mut_ptr().cast()) };
        self.reads += 1;
        self.field_checks
            .insert((token, field as usize), bytes.clone());
        Ok(bytes)
    }
    fn take_info(&mut self, token: u64) -> Result<Object, String> {
        if self.released {
            return Err("attribute info free after release".into());
        }
        self.infos
            .remove(&token)
            .ok_or("unknown or double-freed attribute info".into())
    }
    pub(super) fn free(&mut self, api: Api, token: u64) -> Result<(), String> {
        let info = self.take_info(token)?;
        unsafe { (api.free_info)(info) };
        Ok(())
    }
    pub(super) fn ready(&self) -> bool {
        !self.released && !self.objects.is_empty() && self.reads > 0
    }
    pub(super) fn verify(&mut self, api: Api) -> Result<(), String> {
        for &token in self.objects.keys() {
            self.native(api, token)?;
        }
        for (&(token, field), expected) in &self.field_checks {
            let mut actual = vec![0u8; expected.len()];
            unsafe {
                (api.field_get)(
                    self.native(api, token)?,
                    field as Object,
                    actual.as_mut_ptr().cast(),
                )
            };
            if actual != *expected {
                return Err("attribute primitive field changed across forced GC".into());
            }
        }
        self.verified += self.objects.len();
        Ok(())
    }
    fn retire_resources(&mut self) -> (Vec<Object>, Vec<Object>) {
        if self.released {
            return (Vec::new(), Vec::new());
        }
        self.released = true;
        let infos = std::mem::take(&mut self.infos).into_values().collect();
        let roots = self
            .objects
            .values_mut()
            .map(|root| std::mem::replace(&mut root.handle, ptr::null_mut()))
            .collect();
        self.field_checks.clear();
        (infos, roots)
    }
    pub(super) fn release(&mut self, api: Api) {
        let (infos, roots) = self.retire_resources();
        for info in infos {
            unsafe { (api.free_info)(info) };
        }
        for root in roots {
            unsafe { (api.free_root)(root) };
        }
    }
    pub(super) fn report(&self) -> String {
        format!("native-attribute-queries={}\nnative-attribute-info-handles={}\nnative-attribute-objects={}\nnative-attribute-field-reads={}\nnative-attribute-gc-verifications={}\nnative-attribute-resources-released={}\n",self.queries,self.issued,self.objects.len(),self.reads,self.verified,self.released)
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
            | "mono_object_get_class"
            | "mono_field_get_value"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn info_and_object_ownership_reject_forgery_double_free_and_retirement() {
        let mut registry = Registry::default();
        registry.infos.insert(0x6ffee0000000, 0x1234usize as Object);
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
        assert_eq!(registry.take_info(0x6ffee0000000).unwrap() as usize, 0x1234);
        assert!(registry.take_info(0x6ffee0000000).is_err());
        registry.infos.insert(0x6ffee0000010, 0x5678usize as Object);
        assert!(registry.contains(0x6ffee8000000));
        assert!(!registry.contains(0x6ffee8000001));
        let (infos, roots) = registry.retire_resources();
        assert_eq!(infos, vec![0x5678usize as Object]);
        assert_eq!(roots, vec![0x4567usize as Object]);
        assert!(registry.objects[&0x6ffee8000000].handle.is_null());
        assert!(!registry.contains(0x6ffee8000000));
        assert!(registry.info(0x6ffee0000010).is_err());
        assert!(registry.take_info(0x6ffee0000010).is_err());
        let (infos, roots) = registry.retire_resources();
        assert!(infos.is_empty() && roots.is_empty());
    }
}
