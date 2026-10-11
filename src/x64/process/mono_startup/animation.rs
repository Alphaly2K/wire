//! Exact ColorCurves construction through real managed IL and fixed engine
//! adapters. This finite probe retains the parent root until owner-thread
//! managed finalization; it does not admit production finalizer scheduling.
use super::{c_char, callbacks, Object};
use std::{cell::RefCell, ffi::CStr, ptr};
pub(super) const KEYFRAME_BYTES: usize = 28;
pub(super) const URP: &str = "c6221a413a3d9cb94e15ef83640a835602447f6ce7f74ef55fe301d77d0af845";
pub(super) const RENDER_CORE: &str =
    "9b36919fc9084f3d4a9e796ee2c714ca393e9c8210aaf5d40047400f65d1d338";
pub(super) const BINDINGS: [(&str, u64); 4] = [
    (
        "UnityEngine.AnimationCurve::Internal_Create_Injected",
        0x18006c410,
    ),
    ("UnityEngine.AnimationCurve::Internal_Destroy", 0x18006c2a0),
    (
        "UnityEngine.ScriptableObject::CreateScriptableObject",
        0x1800c1e30,
    ),
    (
        "UnityEngine.Object::GetOffsetOfInstanceIDInCPlusPlusObject",
        0x1800c3690,
    ),
];
const FIELDS: [&CStr; 8] = [
    c"master",
    c"red",
    c"green",
    c"blue",
    c"hueVsHue",
    c"hueVsSat",
    c"satVsSat",
    c"lumVsSat",
];
macro_rules! api {
    ($($field:ident: $name:literal => $ty:ty),* $(,)?) => {
        #[derive(Clone, Copy)]
        struct Api { $($field: $ty,)* }
        impl Api {
            fn load(library: Object) -> Result<Self, String> {
                Ok(Self { $($field: {
                    let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
                    if p.is_null() { return Err(concat!("missing animation ABI symbol: ", $name).into()); }
                    unsafe { std::mem::transmute::<Object, $ty>(p) }
                },)* })
            }
        }
    };
}
api! {
    find: "mono_class_from_name" => unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object,
    method: "mono_class_get_method_from_name" => unsafe extern "C" fn(Object, *const c_char, i32) -> Object,
    method_class: "mono_method_get_class" => unsafe extern "C" fn(Object) -> Object,
    method_name: "mono_method_get_name" => unsafe extern "C" fn(Object) -> *const c_char,
    token: "mono_method_get_token" => unsafe extern "C" fn(Object) -> u32,
    flags: "mono_method_get_flags" => unsafe extern "C" fn(Object, *mut u32) -> u32,
    sig: "mono_method_signature" => unsafe extern "C" fn(Object) -> Object,
    count: "mono_signature_get_param_count" => unsafe extern "C" fn(Object) -> u32,
    instance: "mono_signature_is_instance" => unsafe extern "C" fn(Object) -> u32,
    param: "mono_signature_get_params" => unsafe extern "C" fn(Object, *mut Object) -> Object,
    returns: "mono_signature_get_return_type" => unsafe extern "C" fn(Object) -> Object,
    kind: "mono_type_get_type" => unsafe extern "C" fn(Object) -> u32,
    byref: "mono_type_is_byref" => unsafe extern "C" fn(Object) -> u32,
    type_class: "mono_class_from_mono_type" => unsafe extern "C" fn(Object) -> Object,
    generic: "unity_mono_method_is_generic" => unsafe extern "C" fn(Object) -> u32,
    inflated: "unity_mono_method_is_inflated" => unsafe extern "C" fn(Object) -> u32,
    size: "mono_class_value_size" => unsafe extern "C" fn(Object, *mut u32) -> i32,
    field: "mono_class_get_field_from_name" => unsafe extern "C" fn(Object, *const c_char) -> Object,
    offset: "mono_field_get_offset" => unsafe extern "C" fn(Object) -> i32,
    field_type: "mono_field_get_type" => unsafe extern "C" fn(Object) -> Object,
    field_flags: "mono_field_get_flags" => unsafe extern "C" fn(Object) -> u32,
    get: "mono_field_get_value" => unsafe extern "C" fn(Object, Object, Object),
    object_class: "mono_object_get_class" => unsafe extern "C" fn(Object) -> Object,
    invoke: "mono_runtime_invoke" => unsafe extern "C" fn(Object, Object, *mut Object, *mut Object) -> Object,
    corlib: "mono_get_corlib" => unsafe extern "C" fn() -> Object,
    add: "mono_add_internal_call" => unsafe extern "C" fn(*const c_char, Object),
}
impl Api {
    fn class(&self, image: Object, namespace: &CStr, name: &CStr) -> Result<Object, String> {
        let class = unsafe { (self.find)(image, namespace.as_ptr(), name.as_ptr()) };
        if class.is_null() {
            return Err(format!("animation class absent: {namespace:?}.{name:?}"));
        }
        Ok(class)
    }
    fn ty(&self, ty: Object, kind: u32, byref: bool, class: Option<Object>) -> Result<(), String> {
        if ty.is_null()
            || unsafe { (self.kind)(ty) } != kind
            || (unsafe { (self.byref)(ty) } != 0) != byref
            || class.is_some_and(|class| unsafe { (self.type_class)(ty) } != class)
        {
            return Err("animation ABI type mismatch".into());
        }
        Ok(())
    }
    fn signature(
        &self,
        method: Object,
        instance: bool,
        internal: bool,
        ret: u32,
        params: &[(u32, bool, Option<Object>)],
    ) -> Result<(), String> {
        if method.is_null() {
            return Err("animation method absent".into());
        }
        let mut implementation = 0;
        let flags = unsafe { (self.flags)(method, &mut implementation) };
        let sig = unsafe { (self.sig)(method) };
        if sig.is_null()
            || (flags & 0x10 == 0) != instance
            || (implementation & 0x1000 != 0) != internal
            || (unsafe { (self.instance)(sig) } != 0) != instance
            || unsafe { (self.count)(sig) } != params.len() as u32
            || unsafe { (self.generic)(method) } != 0
            || unsafe { (self.inflated)(method) } != 0
        {
            return Err("animation method ABI signature mismatch".into());
        }
        self.ty(unsafe { (self.returns)(sig) }, ret, false, None)?;
        let mut iterator = ptr::null_mut();
        for &(kind, byref, class) in params {
            self.ty(
                unsafe { (self.param)(sig, &mut iterator) },
                kind,
                byref,
                class,
            )?;
        }
        Ok(())
    }
    fn field(
        &self,
        class: Object,
        name: &CStr,
        kind: u32,
        target: Option<Object>,
    ) -> Result<Object, String> {
        let field = unsafe { (self.field)(class, name.as_ptr()) };
        if field.is_null() || unsafe { (self.field_flags)(field) } & (0x10 | 0x40) != 0 {
            return Err(format!("animation instance field absent: {name:?}"));
        }
        self.ty(unsafe { (self.field_type)(field) }, kind, false, target)?;
        Ok(field)
    }
    fn scalar<T: Copy + Default>(&self, object: Object, field: Object) -> T {
        let mut value = T::default();
        unsafe {
            (self.get)(object, field, (&mut value as *mut T).cast());
        }
        value
    }
    fn reference(
        &self,
        object: Object,
        class: Object,
        name: &CStr,
        target: Object,
    ) -> Result<Object, String> {
        if object.is_null() || unsafe { (self.object_class)(object) } != class {
            return Err("animation graph object/class mismatch".into());
        }
        let field = self.field(class, name, 0x12, Some(target))?;
        let mut value: Object = ptr::null_mut();
        unsafe {
            (self.get)(object, field, (&mut value as *mut Object).cast());
        }
        if value.is_null() || unsafe { (self.object_class)(value) } != target {
            return Err("animation graph reference/class mismatch".into());
        }
        Ok(value)
    }
    fn call(&self, method: Object, object: Object, params: *mut Object) -> Result<(), String> {
        let mut exception = ptr::null_mut();
        let result = unsafe { (self.invoke)(method, object, params, &mut exception) };
        if !exception.is_null() || !result.is_null() {
            return Err("animation managed void invoke returned exception/object".into());
        }
        Ok(())
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Span {
    begin: *const u8,
    length: i32,
}
#[derive(Clone)]
struct Resource {
    pointer: u64,
    count: usize,
    bytes: Vec<u8>,
    destroyed: bool,
}
struct State {
    instance: Option<(Object, u64)>,
    resources: Vec<Resource>,
    scriptable_calls: usize,
    offset_calls: usize,
}
thread_local! { static STATE: RefCell<Option<State>> = const { RefCell::new(None) }; }
fn resources() -> Result<Vec<Resource>, String> {
    STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|s| s.resources.clone())
            .ok_or("animation adapter has no owning thread".into())
    })
}
fn checked<T>(operation: impl FnOnce() -> Result<T, String>) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)) {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            eprintln!("NATIVE_MONO_ANIMATION_REJECT: {error}");
            std::process::abort();
        }
        Err(_) => {
            eprintln!("NATIVE_MONO_ANIMATION_REJECT: adapter panicked");
            std::process::abort();
        }
    }
}
unsafe extern "C" fn create(span: *const Span) -> Object {
    checked(|| {
        callbacks::with_service(|_, _| Ok(()))?;
        if span.is_null() {
            return Err("NULL Keyframe span".into());
        }
        let span = unsafe { span.read_unaligned() };
        if !(0..=4096).contains(&span.length)
            || (span.length > 0
                && (span.begin as usize <= 1 || !(span.begin as usize).is_multiple_of(4)))
            || (span.length == 0 && span.begin as usize > 1)
        {
            return Err("Keyframe span outside locked native layout".into());
        }
        STATE.with(|s| -> Result<(), String> {
            let s = s.borrow();
            let s = s.as_ref().ok_or("curve creation has no owning thread")?;
            if s.instance.is_none() || s.resources.len() >= 8 {
                return Err("curve creation outside live bounded constructor".into());
            }
            Ok(())
        })?;
        let count = span.length as usize;
        let bytes = if count == 0 {
            Vec::new()
        } else {
            // The exact managed wrapper pins its Keyframe array throughout the
            // call. This host pointer is never exposed to the guest executor.
            unsafe { std::slice::from_raw_parts(span.begin, count * KEYFRAME_BYTES) }.to_vec()
        };
        let pointer = callbacks::guest_keyframe_span(BINDINGS[0].1, count, &bytes)?;
        STATE.with(|s| -> Result<(), String> {
            let mut s = s.borrow_mut();
            let s = s.as_mut().ok_or("lost curve owner")?;
            if s.resources.iter().any(|r| r.pointer == pointer) {
                return Err("duplicate live curve resource".into());
            }
            s.resources.push(Resource {
                pointer,
                count,
                bytes,
                destroyed: false,
            });
            Ok(())
        })?;
        Ok(pointer as usize as Object)
    })
}
unsafe extern "C" fn destroy(pointer: Object) {
    checked(|| {
        let pointer = pointer as usize as u64;
        let records = resources()?;
        if !records.iter().any(|r| r.pointer == pointer && !r.destroyed) {
            return Err("destroy requires a live owned guest curve".into());
        }
        callbacks::guest_arguments(BINDINGS[1].1, &[pointer])?;
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.as_mut()
                .unwrap()
                .resources
                .iter_mut()
                .find(|r| r.pointer == pointer)
                .unwrap()
                .destroyed = true;
        });
        Ok(())
    })
}
unsafe extern "C" fn scriptable(object: Object) {
    checked(|| {
        let token = STATE.with(|s| -> Result<u64, String> {
            let s = s.borrow();
            let s = s.as_ref().ok_or("ScriptableObject has no owning thread")?;
            match s.instance {
                Some((native, token)) if native == object && !object.is_null() => Ok(token),
                _ => Err("ScriptableObject callback outside rooted constructor instance".into()),
            }
        })?;
        callbacks::guest_arguments(BINDINGS[2].1, &[token])?;
        STATE.with(|s| s.borrow_mut().as_mut().unwrap().scriptable_calls += 1);
        Ok(())
    })
}
unsafe extern "C" fn instance_id_offset() -> i32 {
    checked(|| {
        STATE.with(|s| -> Result<(), String> {
            let s = s.borrow();
            if s.as_ref()
                .is_none_or(|s| s.instance.is_none() || s.offset_calls != 0)
            {
                return Err("Object cctor query outside live exact constructor".into());
            }
            Ok(())
        })?;
        let value = callbacks::guest_arguments(BINDINGS[3].1, &[])? as u32 as i32;
        if value != 8 {
            return Err("locked engine InstanceID offset differs".into());
        }
        STATE.with(|s| s.borrow_mut().as_mut().unwrap().offset_calls += 1);
        Ok(value)
    })
}
pub(super) struct Constructor {
    api: Api,
    object: Object,
    class: Object,
    parameter: Object,
    texture: Object,
    animation: Object,
    graph: Vec<[Object; 3]>,
    verified: usize,
    serialized: bool,
    cleaned: bool,
}
impl Constructor {
    pub(super) fn execute(
        library: Object,
        images: [Object; 3],
        method: Object,
        object: Object,
        class: Object,
        token: u64,
    ) -> Result<Self, String> {
        callbacks::with_service(|_, _| Ok(()))?;
        let [core, render, urp] = images;
        let api = Api::load(library)?;
        let expected = api.class(urp, c"UnityEngine.Rendering.Universal", c"ColorCurves")?;
        if class != expected
            || object.is_null()
            || unsafe { (api.object_class)(object) } != expected
            || unsafe { (api.method_class)(method) } != expected
            || unsafe { (api.token)(method) } != 0x06000541
        {
            return Err("invoke outside exact rooted ColorCurves constructor".into());
        }
        let name = unsafe { (api.method_name)(method) };
        if name.is_null() || unsafe { CStr::from_ptr(name) } != c".ctor" {
            return Err("ColorCurves constructor name mismatch".into());
        }
        api.signature(method, true, false, 1, &[])?;
        let animation = api.class(core, c"UnityEngine", c"AnimationCurve")?;
        let scriptable_class = api.class(core, c"UnityEngine", c"ScriptableObject")?;
        let span = api.class(core, c"UnityEngine.Bindings", c"ManagedSpanWrapper")?;
        let key = api.class(core, c"UnityEngine", c"Keyframe")?;
        for (class, size, align) in [(key, KEYFRAME_BYTES as i32, 4), (span, 16, 8)] {
            let mut alignment = 0;
            if unsafe { (api.size)(class, &mut alignment) } != size || alignment != align {
                return Err("animation value layout size/alignment mismatch".into());
            }
        }
        for (index, name) in [
            c"m_Time",
            c"m_Value",
            c"m_InTangent",
            c"m_OutTangent",
            c"m_WeightedMode",
            c"m_InWeight",
            c"m_OutWeight",
        ]
        .into_iter()
        .enumerate()
        {
            let field = api.field(key, name, if index == 4 { 8 } else { 0xc }, None)?;
            if unsafe { (api.offset)(field) } != 16 + 4 * index as i32 {
                return Err("Keyframe field layout mismatch".into());
            }
        }
        for (name, kind, offset) in [(c"begin", 0xf, 16), (c"length", 8, 24)] {
            let field = api.field(span, name, kind, None)?;
            if unsafe { (api.offset)(field) } != offset {
                return Err("ManagedSpanWrapper field layout mismatch".into());
            }
        }
        let unity_base = api.class(core, c"UnityEngine", c"Object")?;
        let offset_method = unsafe {
            (api.method)(
                unity_base,
                c"GetOffsetOfInstanceIDInCPlusPlusObject".as_ptr(),
                0,
            )
        };
        api.signature(offset_method, false, true, 8, &[])?;
        if unsafe { (api.token)(offset_method) } != 0x06002547 {
            return Err("Object offset method token mismatch".into());
        }
        let create_method =
            unsafe { (api.method)(animation, c"Internal_Create_Injected".as_ptr(), 1) };
        let destroy_method = unsafe { (api.method)(animation, c"Internal_Destroy".as_ptr(), 1) };
        let scriptable_method =
            unsafe { (api.method)(scriptable_class, c"CreateScriptableObject".as_ptr(), 1) };
        api.signature(
            create_method,
            false,
            true,
            0x18,
            &[(0x11, true, Some(span))],
        )?;
        api.signature(destroy_method, false, true, 1, &[(0x18, false, None)])?;
        api.signature(
            scriptable_method,
            false,
            true,
            1,
            &[(0x12, false, Some(scriptable_class))],
        )?;
        if unsafe { (api.token)(create_method) } != 0x060005aa
            || unsafe { (api.token)(destroy_method) } != 0x06000586
            || unsafe { (api.token)(scriptable_method) } != 0x0600246a
        {
            return Err("animation internalcall method token mismatch".into());
        }
        STATE.with(|s| -> Result<(), String> {
            let mut s = s.borrow_mut();
            if s.is_some() {
                return Err("animation finite constructor already installed".into());
            }
            *s = Some(State {
                instance: Some((object, token)),
                resources: Vec::new(),
                scriptable_calls: 0,
                offset_calls: 0,
            });
            Ok(())
        })?;
        struct Scope;
        impl Drop for Scope {
            fn drop(&mut self) {
                STATE.with(|s| s.borrow_mut().as_mut().unwrap().instance = None);
            }
        }
        let _scope = Scope;
        for (name, adapter) in [
            (
                c"UnityEngine.AnimationCurve::Internal_Create_Injected",
                create as *const (),
            ),
            (
                c"UnityEngine.AnimationCurve::Internal_Destroy",
                destroy as *const (),
            ),
            (
                c"UnityEngine.ScriptableObject::CreateScriptableObject",
                scriptable as *const (),
            ),
            (
                c"UnityEngine.Object::GetOffsetOfInstanceIDInCPlusPlusObject",
                instance_id_offset as *const (),
            ),
        ] {
            unsafe {
                (api.add)(name.as_ptr(), adapter as Object);
            }
        }
        eprintln!(
            "NATIVE_MONO_ANIMATION_ABI: Keyframe size=28 align=4; span size=16; fixed adapters=4"
        );
        api.call(method, object, ptr::null_mut())?;
        let mut result = Self {
            api,
            object,
            class,
            parameter: api.class(render, c"UnityEngine.Rendering", c"TextureCurveParameter")?,
            texture: api.class(render, c"UnityEngine.Rendering", c"TextureCurve")?,
            animation,
            graph: Vec::new(),
            verified: 0,
            serialized: false,
            cleaned: false,
        };
        result.verify()?;
        eprintln!("NATIVE_MONO_COLOR_CURVES_CONSTRUCTED: real managed ctor; engine curves=8 ScriptableObject=1");
        Ok(result)
    }
    pub(super) fn verify(&mut self) -> Result<(), String> {
        if self.cleaned {
            return Err("curve graph verification after cleanup".into());
        }
        let records = resources()?;
        let calls = STATE.with(|s| {
            s.borrow()
                .as_ref()
                .map_or((0, 0), |s| (s.scriptable_calls, s.offset_calls))
        });
        if records.len() != 8 || calls != (1, 1) {
            return Err("ColorCurves engine callback counts mismatch".into());
        }
        let mut graph = Vec::new();
        for (index, name) in FIELDS.into_iter().enumerate() {
            let parameter = self
                .api
                .reference(self.object, self.class, name, self.parameter)?;
            let texture =
                self.api
                    .reference(parameter, self.parameter, c"m_Value", self.texture)?;
            let animation =
                self.api
                    .reference(texture, self.texture, c"m_Curve", self.animation)?;
            let length = self
                .api
                .field(self.texture, c"<length>k__BackingField", 8, None)?;
            let pointer = self.api.field(self.animation, c"m_Ptr", 0x18, None)?;
            let cleanup = self
                .api
                .field(self.animation, c"m_RequiresNativeCleanup", 2, None)?;
            let record = &records[index];
            if record.destroyed
                || record.count != if index < 4 { 2 } else { 0 }
                || record.bytes.len() != record.count * KEYFRAME_BYTES
                || (!self.serialized
                    && self.api.scalar::<i32>(texture, length) != record.count as i32)
                || self.api.scalar::<u64>(animation, pointer) != record.pointer
                || self.api.scalar::<u8>(animation, cleanup) != 1
            {
                return Err("ColorCurves managed graph/resource fields mismatch".into());
            }
            if index < 4 {
                let expected: Vec<u8> = [
                    0u32, 0, 0x3f800000, 0x3f800000, 0, 0, 0, 0x3f800000, 0x3f800000, 0x3f800000,
                    0x3f800000, 0, 0, 0,
                ]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect();
                if record.bytes != expected {
                    return Err("ColorCurves Keyframe input bits mismatch".into());
                }
            }
            for (name, kind, expected) in [
                (
                    c"m_ZeroValue",
                    0xc,
                    if index < 4 { 0u32 } else { 0x3f000000 },
                ),
                (c"m_Range", 0xc, 0x3f800000),
            ] {
                let field = self.api.field(self.texture, name, kind, None)?;
                if !self.serialized && self.api.scalar::<u32>(texture, field) != expected {
                    return Err("ColorCurves TextureCurve scalar mismatch".into());
                }
            }
            let loop_field = self.api.field(self.texture, c"m_Loop", 2, None)?;
            if !self.serialized
                && self.api.scalar::<u8>(texture, loop_field) != u8::from(index == 4 || index == 5)
            {
                return Err("ColorCurves loop field mismatch".into());
            }
            graph.push([parameter, texture, animation]);
        }
        let active = self.api.field(self.class, c"active", 2, None)?;
        if (!self.serialized && self.api.scalar::<u8>(self.object, active) != 1)
            || (!self.graph.is_empty() && self.graph != graph)
        {
            return Err("ColorCurves graph identity/active field changed".into());
        }
        self.graph = graph;
        self.verified += 1;
        Ok(())
    }
    pub(super) fn serialized_fields(&mut self) -> Result<Vec<super::fields::Spec>, String> {
        let mut specs = vec![super::fields::Spec {
            owner: self.object,
            name: c"active",
            kind: 2,
            reference: None,
            guest_pointer: None,
        }];
        for ((name, triple), resource) in FIELDS.into_iter().zip(&self.graph).zip(resources()?) {
            specs.push(super::fields::Spec {
                owner: self.object,
                name,
                kind: 0x12,
                reference: Some(self.parameter),
                guest_pointer: None,
            });
            specs.push(super::fields::Spec {
                owner: triple[0],
                name: c"m_OverrideState",
                kind: 2,
                reference: None,
                guest_pointer: None,
            });
            specs.push(super::fields::Spec {
                owner: triple[0],
                name: c"m_Value",
                kind: 0x12,
                reference: Some(self.texture),
                guest_pointer: None,
            });
            specs.push(super::fields::Spec {
                owner: triple[1],
                name: c"m_Curve",
                kind: 0x12,
                reference: Some(self.animation),
                guest_pointer: None,
            });
            specs.push(super::fields::Spec {
                owner: triple[2],
                name: c"m_Ptr",
                kind: 0x18,
                reference: None,
                guest_pointer: Some(resource.pointer),
            });
            for (name, kind) in [
                (c"<length>k__BackingField", 8),
                (c"m_Loop", 2),
                (c"m_ZeroValue", 0xc),
                (c"m_Range", 0xc),
            ] {
                specs.push(super::fields::Spec {
                    owner: triple[1],
                    name,
                    kind,
                    reference: None,
                    guest_pointer: None,
                });
            }
        }
        self.serialized = true;
        Ok(specs)
    }
    pub(super) fn cleanup(&mut self) -> Result<(), String> {
        if self.cleaned {
            return Ok(());
        }
        self.verify()?;
        // All eight objects remain strongly reachable through the owned parent.
        // Suppress their automatic finalizers using the real managed GC API,
        // then execute the actual managed Finalize bodies on the live owner.
        // Each body must call the original guest Destroy exactly once. No
        // foreign-thread finalizer notification is reported as fake success.
        let gc = self
            .api
            .class(unsafe { (self.api.corlib)() }, c"System", c"GC")?;
        let object = self
            .api
            .class(unsafe { (self.api.corlib)() }, c"System", c"Object")?;
        let suppress = unsafe { (self.api.method)(gc, c"SuppressFinalize".as_ptr(), 1) };
        self.api
            .signature(suppress, false, false, 1, &[(0x1c, false, Some(object))])?;
        let finalize = unsafe { (self.api.method)(self.animation, c"Finalize".as_ptr(), 0) };
        self.api.signature(finalize, true, false, 1, &[])?;
        for triple in &self.graph {
            let mut arguments = [triple[2]];
            self.api
                .call(suppress, ptr::null_mut(), arguments.as_mut_ptr())?;
            self.api.call(finalize, triple[2], ptr::null_mut())?;
        }
        if resources()?.iter().any(|r| !r.destroyed) {
            return Err("curve cleanup left live engine resources".into());
        }
        self.cleaned = true;
        eprintln!("NATIVE_MONO_COLOR_CURVES_CLEANUP: actual owner-thread managed finalizers=8 guest destroys=8 automatic finalizers suppressed");
        Ok(())
    }
    pub(super) fn report(&self) -> String {
        format!("native-color-curves-ready=true\nnative-color-curves-graph-verifications={}\nnative-color-curves-cleaned={}\n", self.verified, self.cleaned)
    }
}
