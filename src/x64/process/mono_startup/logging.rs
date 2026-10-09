//! Locked Unity logger ABI: fixed ARM64 adapter, pinned native UTF16 input and
//! a bounded owned copy for the original Win64 engine callback.
use super::{c_char, callbacks, Object};
use std::{cell::RefCell, ffi::CStr, ptr, sync::OnceLock};
pub(super) const NAME: &str = "UnityEngine.UnityLogWriter::WriteStringToUnityLogImpl_Injected";
pub(super) const TARGET: u64 = 0x1800b0b80;
static TARGET_PC: OnceLock<u64> = OnceLock::new();
thread_local! { static RECORDS: RefCell<Vec<Option<Vec<u16>>>> = const { RefCell::new(Vec::new()) }; }
#[repr(C)]
#[derive(Clone, Copy)]
struct Span {
    begin: *const u16,
    length: i32,
}
macro_rules! sym {
    ($library:expr, $name:literal, $ty:ty) => {{
        let p = unsafe { libc::dlsym($library, concat!($name, "\0").as_ptr().cast()) };
        if p.is_null() {
            return Err(concat!("missing logging ABI symbol: ", $name).into());
        }
        unsafe { std::mem::transmute::<Object, $ty>(p) }
    }};
}
struct Api {
    class: unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object,
    method: unsafe extern "C" fn(Object, *const c_char, i32) -> Object,
    methods: unsafe extern "C" fn(Object, *mut Object) -> Object,
    method_name: unsafe extern "C" fn(Object) -> *const c_char,
    token: unsafe extern "C" fn(Object) -> u32,
    flags: unsafe extern "C" fn(Object, *mut u32) -> u32,
    signature: unsafe extern "C" fn(Object) -> Object,
    count: unsafe extern "C" fn(Object) -> u32,
    instance: unsafe extern "C" fn(Object) -> u32,
    parameter: unsafe extern "C" fn(Object, *mut Object) -> Object,
    returns: unsafe extern "C" fn(Object) -> Object,
    kind: unsafe extern "C" fn(Object) -> u32,
    byref: unsafe extern "C" fn(Object) -> u32,
    type_class: unsafe extern "C" fn(Object) -> Object,
}
impl Api {
    fn load(library: Object) -> Result<Self, String> {
        Ok(Self {
            class: sym!(
                library,
                "mono_class_from_name",
                unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object
            ),
            method: sym!(
                library,
                "mono_class_get_method_from_name",
                unsafe extern "C" fn(Object, *const c_char, i32) -> Object
            ),
            methods: sym!(
                library,
                "mono_class_get_methods",
                unsafe extern "C" fn(Object, *mut Object) -> Object
            ),
            method_name: sym!(
                library,
                "mono_method_get_name",
                unsafe extern "C" fn(Object) -> *const c_char
            ),
            token: sym!(
                library,
                "mono_method_get_token",
                unsafe extern "C" fn(Object) -> u32
            ),
            flags: sym!(
                library,
                "mono_method_get_flags",
                unsafe extern "C" fn(Object, *mut u32) -> u32
            ),
            signature: sym!(
                library,
                "mono_method_signature",
                unsafe extern "C" fn(Object) -> Object
            ),
            count: sym!(
                library,
                "mono_signature_get_param_count",
                unsafe extern "C" fn(Object) -> u32
            ),
            instance: sym!(
                library,
                "mono_signature_is_instance",
                unsafe extern "C" fn(Object) -> u32
            ),
            parameter: sym!(
                library,
                "mono_signature_get_params",
                unsafe extern "C" fn(Object, *mut Object) -> Object
            ),
            returns: sym!(
                library,
                "mono_signature_get_return_type",
                unsafe extern "C" fn(Object) -> Object
            ),
            kind: sym!(
                library,
                "mono_type_get_type",
                unsafe extern "C" fn(Object) -> u32
            ),
            byref: sym!(
                library,
                "mono_type_is_byref",
                unsafe extern "C" fn(Object) -> u32
            ),
            type_class: sym!(
                library,
                "mono_class_from_mono_type",
                unsafe extern "C" fn(Object) -> Object
            ),
        })
    }
    fn parameter(&self, method: Object, internal: bool) -> Result<Object, String> {
        if method.is_null() {
            return Err("NULL logging method".into());
        }
        let mut implementation = 0;
        let flags = unsafe { (self.flags)(method, &mut implementation) };
        let sig = unsafe { (self.signature)(method) };
        if flags & 0x10 == 0
            || (implementation & 0x1000 != 0) != internal
            || sig.is_null()
            || unsafe { (self.instance)(sig) } != 0
            || unsafe { (self.count)(sig) } != 1
        {
            return Err("logging static method signature mismatch".into());
        }
        let ret = unsafe { (self.returns)(sig) };
        if ret.is_null() || unsafe { (self.kind)(ret) } != 1 || unsafe { (self.byref)(ret) } != 0 {
            return Err("logging return type mismatch".into());
        }
        let mut iter = ptr::null_mut();
        let parameter = unsafe { (self.parameter)(sig, &mut iter) };
        if parameter.is_null() {
            return Err("logging parameter absent".into());
        }
        Ok(parameter)
    }
    fn string_parameter(&self, method: Object) -> Result<(), String> {
        let ty = self.parameter(method, false)?;
        if unsafe { (self.kind)(ty) } != 0xe || unsafe { (self.byref)(ty) } != 0 {
            return Err("logging String signature mismatch".into());
        }
        Ok(())
    }
}
pub(super) fn install(library: Object, image: Object, target: u64) -> Result<(), String> {
    callbacks::with_service(|_, _| Ok(()))?;
    if target != TARGET {
        return Err("Unity logging target mismatch".into());
    }
    let api = Api::load(library)?;
    let class = unsafe { (api.class)(image, c"UnityEngine".as_ptr(), c"UnityLogWriter".as_ptr()) };
    let span = unsafe {
        (api.class)(
            image,
            c"UnityEngine.Bindings".as_ptr(),
            c"ManagedSpanWrapper".as_ptr(),
        )
    };
    if class.is_null() || span.is_null() {
        return Err("locked Unity logging classes absent".into());
    }
    let method = unsafe { (api.method)(class, c"WriteStringToUnityLogImpl_Injected".as_ptr(), 1) };
    let parameter = api.parameter(method, true)?;
    if unsafe { (api.token)(method) } != 0x06001d07
        || unsafe { (api.kind)(parameter) } != 0x11
        || unsafe { (api.byref)(parameter) } != 1
        || unsafe { (api.type_class)(parameter) } != span
    {
        return Err("locked Unity logging span signature mismatch".into());
    }
    let field = sym!(
        library,
        "mono_class_get_field_from_name",
        unsafe extern "C" fn(Object, *const c_char) -> Object
    );
    let offset = sym!(
        library,
        "mono_field_get_offset",
        unsafe extern "C" fn(Object) -> u32
    );
    let field_type = sym!(
        library,
        "mono_field_get_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let value_size = sym!(
        library,
        "mono_class_value_size",
        unsafe extern "C" fn(Object, *mut u32) -> i32
    );
    let begin = unsafe { field(span, c"begin".as_ptr()) };
    let length = unsafe { field(span, c"length".as_ptr()) };
    let mut alignment = 0;
    if begin.is_null()
        || length.is_null()
        || unsafe { value_size(span, &mut alignment) } != 16
        || alignment != 8
        || unsafe { offset(begin) } != 16
        || unsafe { offset(length) } != 24
        || unsafe { (api.kind)(field_type(begin)) } != 0xf
        || unsafe { (api.kind)(field_type(length)) } != 8
    {
        return Err("locked ManagedSpanWrapper layout mismatch".into());
    }
    TARGET_PC
        .set(target)
        .map_err(|_| "Unity logging adapter already installed")?;
    let add = sym!(
        library,
        "mono_add_internal_call",
        unsafe extern "C" fn(*const c_char, Object)
    );
    unsafe {
        add(
            c"UnityEngine.UnityLogWriter::WriteStringToUnityLogImpl_Injected".as_ptr(),
            write as *const () as Object,
        );
    }
    eprintln!("NATIVE_MONO_LOGGING_ABI: ref span size=16 align=8 begin=0 length=8; fixed owned UTF16 adapter");
    Ok(())
}
unsafe extern "C" fn write(span: *const Span) {
    let result = std::panic::catch_unwind(|| -> Result<(), String> {
        callbacks::with_service(|_, _| Ok(()))?;
        if span.is_null() {
            return Err("NULL native logging span".into());
        }
        let span = unsafe { span.read_unaligned() };
        if span.length < 0
            || span.length > 65536
            || (span.length > 0
                && (span.begin as usize <= 1 || !(span.begin as usize).is_multiple_of(2)))
        {
            return Err("native logging span outside bounded UTF16 contract".into());
        }
        let units = if span.length == 0 {
            match span.begin as usize {
                0 => None,
                1 => Some(Vec::new()),
                _ => return Err("invalid empty Unity log sentinel".into()),
            }
        } else {
            Some(unsafe { std::slice::from_raw_parts(span.begin, span.length as usize) }.to_vec())
        };
        callbacks::log_span(
            *TARGET_PC.get().ok_or("logging adapter not installed")?,
            units.as_deref(),
        )?;
        RECORDS.with(|records| {
            let mut records = records.borrow_mut();
            if records.len() >= 1024 {
                return Err("diagnostic logging record capacity exceeded".into());
            }
            records.push(units);
            Ok(())
        })
    });
    if !matches!(result, Ok(Ok(()))) {
        eprintln!("NATIVE_MONO_LOGGING_REJECT: {result:?}");
        std::process::abort();
    }
}
pub(super) fn verify(library: Object, image: Object) -> Result<(), String> {
    let api = Api::load(library)?;
    let class = unsafe { (api.class)(image, c"UnityEngine".as_ptr(), c"UnityLogWriter".as_ptr()) };
    let method = unsafe { (api.method)(class, c"WriteStringToUnityLogImpl".as_ptr(), 1) };
    api.string_parameter(method)?;
    if unsafe { (api.token)(method) } != 0x06001d00 {
        return Err("logging marshal method token mismatch".into());
    }
    let domain = sym!(library, "mono_domain_get", unsafe extern "C" fn() -> Object);
    let string = sym!(
        library,
        "mono_string_new",
        unsafe extern "C" fn(Object, *const c_char) -> Object
    );
    let root = sym!(
        library,
        "mono_gchandle_new_v2",
        unsafe extern "C" fn(Object, i32) -> Object
    );
    let free = sym!(
        library,
        "mono_gchandle_free_v2",
        unsafe extern "C" fn(Object)
    );
    let invoke = sym!(
        library,
        "mono_runtime_invoke",
        unsafe extern "C" fn(Object, Object, *mut Object, *mut Object) -> Object
    );
    let corlib = sym!(library, "mono_get_corlib", unsafe extern "C" fn() -> Object);
    let console = unsafe { (api.class)(corlib(), c"System".as_ptr(), c"Console".as_ptr()) };
    if console.is_null() {
        return Err("Console absent after logger Init".into());
    }
    let mut iterator = ptr::null_mut();
    let mut console_write: Object = ptr::null_mut();
    loop {
        let m = unsafe { (api.methods)(console, &mut iterator) };
        if m.is_null() {
            break;
        }
        let name = unsafe { (api.method_name)(m) };
        if !name.is_null()
            && unsafe { CStr::from_ptr(name) }.to_bytes() == b"Write"
            && api.string_parameter(m).is_ok()
        {
            if !console_write.is_null() {
                return Err("ambiguous Console.Write(String)".into());
            }
            console_write = m;
        }
    }
    if console_write.is_null() {
        return Err("Console.Write(String) absent".into());
    }
    let before = RECORDS.with(|r| r.borrow().len());
    let mut expected = Vec::new();
    for (method, text) in [
        (method, None),
        (method, Some(c"")),
        (method, Some(c"wire native Mono 日志 ✓ 😀")),
        (console_write, Some(c"wire Console route verified\r\n")),
    ] {
        let object = match text {
            None => ptr::null_mut(),
            Some(text) => unsafe { string(domain(), text.as_ptr()) },
        };
        if text.is_some() && object.is_null() {
            return Err("logging String allocation failed".into());
        }
        let handle = if object.is_null() {
            ptr::null_mut()
        } else {
            unsafe { root(object, 1) }
        };
        if !object.is_null() && handle.is_null() {
            return Err("logging String pin failed".into());
        }
        let mut arguments = [object];
        let mut exception = ptr::null_mut();
        let result = unsafe {
            invoke(
                method,
                ptr::null_mut(),
                arguments.as_mut_ptr(),
                &mut exception,
            )
        };
        if !handle.is_null() {
            unsafe {
                free(handle);
            }
        }
        if !exception.is_null() || !result.is_null() {
            return Err("actual Unity log/Console managed invoke failed".into());
        }
        expected.push(text.map(|t| t.to_str().unwrap().encode_utf16().collect::<Vec<_>>()));
    }
    RECORDS.with(|r| -> Result<(), String> {
        if r.borrow()[before..] != expected {
            return Err("actual log null/empty/Unicode/Console payload verification failed".into());
        }
        Ok(())
    })?;
    eprintln!("NATIVE_MONO_LOGGING_VERIFIED: actual managed null/empty/Unicode/Console routes=4");
    Ok(())
}
