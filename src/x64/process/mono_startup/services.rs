//! Exact locked Windows corelib entries. All process/handle/VFS operations use
//! the live guest owner. Host classlib pointers are copied only on its Mono
//! calling thread; they never enter Memory or become guest addresses.
use super::super::super::memory::{Memory, READ, WRITE};
use super::super::super::runtime::Runtime64;
use super::super::Cpu64;
use super::{c_char, callbacks, Object};
use std::{
    ffi::{CStr, CString},
    ptr,
    sync::OnceLock,
};
const SCRATCH: u64 = 0x6ffb00000000;
const STACK: u64 = SCRATCH + 0x20000;
const OUTPUT: u64 = SCRATCH + 65536;
type Handle = Object;
type Entry = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    Vec<&'static str>,
    usize,
);
macro_rules! sym {
    ($library:expr, $name:literal, $ty:ty) => {{
        let p = unsafe { libc::dlsym($library, concat!($name, "\0").as_ptr().cast()) };
        if p.is_null() {
            return Err(concat!("missing Windows service symbol: ", $name).into());
        }
        unsafe { std::mem::transmute::<Object, $ty>(p) }
    }};
}
struct Api {
    corlib: usize,
    class: unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object,
    methods: unsafe extern "C" fn(Object, *mut Object) -> Object,
    method_name: unsafe extern "C" fn(Object) -> *const c_char,
    method_class: unsafe extern "C" fn(Object) -> Object,
    class_image: unsafe extern "C" fn(Object) -> Object,
    signature: unsafe extern "C" fn(Object) -> Object,
    parameters: unsafe extern "C" fn(Object, *mut Object) -> Object,
    return_type: unsafe extern "C" fn(Object) -> Object,
    instance: unsafe extern "C" fn(Object) -> u32,
    type_name: unsafe extern "C" fn(Object) -> *mut c_char,
    flags: unsafe extern "C" fn(Object, *mut u32) -> u32,
    free: unsafe extern "C" fn(Object),
    string_new: unsafe extern "C" fn(Object, *const c_char) -> Object,
    domain: unsafe extern "C" fn() -> Object,
    object_class: unsafe extern "C" fn(Object) -> Object,
    element_class: unsafe extern "C" fn(Object) -> Object,
    array_length: unsafe extern "C" fn(Object) -> usize,
    array_addr: unsafe extern "C" fn(Object, i32, usize) -> Object,
    new_handle: unsafe extern "C" fn(Object, i32) -> Handle,
    free_handle: unsafe extern "C" fn(Handle),
    byte_class: usize,
    pinvokes: Vec<(usize, &'static str, &'static str, u32, usize)>,
}
static API: OnceLock<Api> = OnceLock::new();
fn text(p: *const c_char) -> Result<String, String> {
    if p.is_null() {
        return Err("NULL native service metadata text".into());
    }
    Ok(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
}
impl Api {
    fn type_text(&self, ty: Object) -> Result<String, String> {
        if ty.is_null() {
            return Err("NULL native service type".into());
        }
        let p = unsafe { (self.type_name)(ty) };
        let result = text(p);
        unsafe {
            (self.free)(p.cast());
        }
        result
    }
    fn matches(
        &self,
        method: Object,
        name: &str,
        ret: &str,
        params: &[&str],
        internal: bool,
    ) -> Result<bool, String> {
        if method.is_null() || text(unsafe { (self.method_name)(method) })? != name {
            return Ok(false);
        }
        let mut implementation = 0;
        let flags = unsafe { (self.flags)(method, &mut implementation) };
        if flags & 0x10 == 0 || (implementation & 0x1000 != 0) != internal {
            return Ok(false);
        }
        let signature = unsafe { (self.signature)(method) };
        if signature.is_null()
            || unsafe { (self.instance)(signature) } != 0
            || self.type_text(unsafe { (self.return_type)(signature) })? != ret
        {
            return Ok(false);
        }
        let mut iterator = ptr::null_mut();
        let mut actual = Vec::new();
        loop {
            let ty = unsafe { (self.parameters)(signature, &mut iterator) };
            if ty.is_null() {
                break;
            }
            if actual.len() >= 16 {
                return Ok(false);
            }
            actual.push(self.type_text(ty)?);
        }
        Ok(actual == params)
    }
}
pub(super) fn install(library: Object) -> Result<(), String> {
    if API.get().is_some() {
        return Ok(());
    }
    let get_corlib = sym!(library, "mono_get_corlib", unsafe extern "C" fn() -> Object);
    let byte = sym!(
        library,
        "mono_get_byte_class",
        unsafe extern "C" fn() -> Object
    );
    let get_method = sym!(
        library,
        "mono_get_method",
        unsafe extern "C" fn(Object, u32, Object) -> Object
    );
    let mut api = Api {
        corlib: unsafe { get_corlib() } as usize,
        class: sym!(
            library,
            "mono_class_from_name",
            unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object
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
        method_class: sym!(
            library,
            "mono_method_get_class",
            unsafe extern "C" fn(Object) -> Object
        ),
        class_image: sym!(
            library,
            "mono_class_get_image",
            unsafe extern "C" fn(Object) -> Object
        ),
        signature: sym!(
            library,
            "mono_method_signature",
            unsafe extern "C" fn(Object) -> Object
        ),
        parameters: sym!(
            library,
            "mono_signature_get_params",
            unsafe extern "C" fn(Object, *mut Object) -> Object
        ),
        return_type: sym!(
            library,
            "mono_signature_get_return_type",
            unsafe extern "C" fn(Object) -> Object
        ),
        instance: sym!(
            library,
            "mono_signature_is_instance",
            unsafe extern "C" fn(Object) -> u32
        ),
        type_name: sym!(
            library,
            "mono_type_get_name",
            unsafe extern "C" fn(Object) -> *mut c_char
        ),
        flags: sym!(
            library,
            "mono_method_get_flags",
            unsafe extern "C" fn(Object, *mut u32) -> u32
        ),
        free: sym!(library, "mono_free", unsafe extern "C" fn(Object)),
        string_new: sym!(
            library,
            "mono_string_new",
            unsafe extern "C" fn(Object, *const c_char) -> Object
        ),
        domain: sym!(library, "mono_domain_get", unsafe extern "C" fn() -> Object),
        object_class: sym!(
            library,
            "mono_object_get_class",
            unsafe extern "C" fn(Object) -> Object
        ),
        element_class: sym!(
            library,
            "mono_class_get_element_class",
            unsafe extern "C" fn(Object) -> Object
        ),
        array_length: sym!(
            library,
            "mono_array_length",
            unsafe extern "C" fn(Object) -> usize
        ),
        array_addr: sym!(
            library,
            "mono_array_addr_with_size",
            unsafe extern "C" fn(Object, i32, usize) -> Object
        ),
        new_handle: sym!(
            library,
            "mono_gchandle_new_v2",
            unsafe extern "C" fn(Object, i32) -> Handle
        ),
        free_handle: sym!(
            library,
            "mono_gchandle_free_v2",
            unsafe extern "C" fn(Handle)
        ),
        byte_class: unsafe { byte() } as usize,
        pinvokes: Vec::new(),
    };
    let add = sym!(
        library,
        "mono_add_internal_call",
        unsafe extern "C" fn(*const c_char, Object)
    );
    let entries: Vec<Entry> = vec![
        (
            "System",
            "Environment",
            "get_Platform",
            "System.PlatformID",
            vec![],
            platform as *const () as usize,
        ),
        (
            "System",
            "Environment",
            "GetNewLine",
            "System.String",
            vec![],
            newline as *const () as usize,
        ),
        (
            "System",
            "Environment",
            "GetOSVersionString",
            "System.String",
            vec![],
            os_version as *const () as usize,
        ),
        (
            "System.IO",
            "Path",
            "get_temp_path",
            "System.String",
            vec![],
            temp_path as *const () as usize,
        ),
        (
            "System.IO",
            "MonoIO",
            "get_DirectorySeparatorChar",
            "System.Char",
            vec![],
            directory_separator as *const () as usize,
        ),
        (
            "System.IO",
            "MonoIO",
            "get_AltDirectorySeparatorChar",
            "System.Char",
            vec![],
            alternate_separator as *const () as usize,
        ),
        (
            "System.IO",
            "MonoIO",
            "get_VolumeSeparatorChar",
            "System.Char",
            vec![],
            volume_separator as *const () as usize,
        ),
        (
            "System.IO",
            "MonoIO",
            "get_PathSeparator",
            "System.Char",
            vec![],
            path_separator as *const () as usize,
        ),
        (
            "System.IO",
            "MonoIO",
            "GetCurrentDirectory",
            "System.String",
            vec!["System.IO.MonoIOError&"],
            current_directory as *const () as usize,
        ),
        (
            "System.IO",
            "MonoIO",
            "get_ConsoleInput",
            "System.IntPtr",
            vec![],
            console_input as *const () as usize,
        ),
        (
            "System.IO",
            "MonoIO",
            "get_ConsoleOutput",
            "System.IntPtr",
            vec![],
            console_output as *const () as usize,
        ),
        (
            "System.IO",
            "MonoIO",
            "get_ConsoleError",
            "System.IntPtr",
            vec![],
            console_error as *const () as usize,
        ),
    ];
    let mut verified = Vec::new();
    for (namespace, class, name, ret, params, target) in entries.into_iter().chain(io::entries()) {
        let ns = CString::new(namespace).unwrap();
        let cls = CString::new(class).unwrap();
        let class = unsafe { (api.class)(api.corlib as Object, ns.as_ptr(), cls.as_ptr()) };
        if class.is_null() {
            return Err(format!("missing native Windows identity class {cls:?}"));
        }
        let mut iterator = ptr::null_mut();
        let mut matching = 0;
        loop {
            let method = unsafe { (api.methods)(class, &mut iterator) };
            if method.is_null() {
                break;
            }
            if api.matches(method, name, ret, &params, true)? {
                matching += 1;
            }
        }
        if matching != 1 {
            return Err(format!("native Windows service signature mismatch: {namespace}.{cls:?}::{name}, matches={matching}"));
        }
        verified.push((
            CString::new(format!("{namespace}.{}::{name}", cls.to_str().unwrap())).unwrap(),
            target,
        ));
    }
    for (token, scope, entry, flags, name, ret, params, target) in [
        (
            0x06000020,
            "kernel32.dll",
            "GetFileAttributesExW",
            0x164,
            "GetFileAttributesExPrivate",
            "System.Boolean",
            vec![
                "System.String",
                "Interop.Kernel32.GET_FILEEX_INFO_LEVELS",
                "Interop.Kernel32.WIN32_FILE_ATTRIBUTE_DATA&",
            ],
            attributes as *const () as usize,
        ),
        (
            0x06006a21,
            "Kernel32.dll",
            "GetFullPathName",
            0x144,
            "GetFullPathName",
            "System.Int32",
            vec![
                "System.String",
                "System.Int32",
                "System.Text.StringBuilder",
                "System.IntPtr&",
            ],
            full_path as *const () as usize,
        ),
        (
            0x06001947,
            "kernel32.dll",
            "GetConsoleCP",
            0x107,
            "GetConsoleCP",
            "System.Int32",
            vec![],
            input_codepage as *const () as usize,
        ),
        (
            0x06001948,
            "kernel32.dll",
            "GetConsoleOutputCP",
            0x107,
            "GetConsoleOutputCP",
            "System.Int32",
            vec![],
            output_codepage as *const () as usize,
        ),
        (
            0x0600002d,
            "kernel32.dll",
            "SetThreadErrorMode",
            0x141,
            "SetThreadErrorMode",
            "System.Boolean",
            vec!["System.UInt32", "System.UInt32&"],
            thread_error_mode as *const () as usize,
        ),
        (
            0x0600003e,
            "BCrypt.dll",
            "BCryptGenRandom",
            0x104,
            "BCryptGenRandom",
            "Interop.BCrypt.NTSTATUS",
            vec![
                "System.IntPtr",
                "System.Byte*",
                "System.Int32",
                "System.Int32",
            ],
            random as *const () as usize,
        ),
    ] {
        let method = unsafe { get_method(api.corlib as Object, token, ptr::null_mut()) };
        if !api.matches(method, name, ret, &params, false)?
            || unsafe { (api.class_image)((api.method_class)(method)) } as usize != api.corlib
        {
            return Err(format!(
                "locked Windows P/Invoke signature mismatch token={token:#x}"
            ));
        }
        api.pinvokes
            .push((method as usize, scope, entry, flags, target));
    }
    let install = sym!(
        library,
        "wire_mono_install_pinvoke_adapter",
        unsafe extern "C" fn(
            u32,
            unsafe extern "C" fn(Object, *const c_char, *const c_char, u32, *mut Object) -> i32,
        ) -> i32
    );
    if API.set(api).is_err() {
        return Err("native Windows service API already installed".into());
    }
    if unsafe { install(1, resolve) } != 0 {
        return Err("native Windows P/Invoke adapter installation rejected".into());
    }
    for (name, target) in verified {
        unsafe {
            add(name.as_ptr(), target as Object);
        }
    }
    eprintln!("NATIVE_MONO_WINDOWS_SERVICES: locked corelib identity, MonoIO and 6 exact P/Invoke adapters installed; same guest process owner");
    Ok(())
}
unsafe extern "C" fn resolve(
    method: Object,
    scope: *const c_char,
    entry: *const c_char,
    flags: u32,
    target: *mut Object,
) -> i32 {
    let result = std::panic::catch_unwind(|| {
        let api = API.get().ok_or("Windows service API not installed")?;
        let scope = text(scope)?;
        let entry = text(entry)?;
        let binding = api.pinvokes.iter().find(|(m, s, e, f, _)| {
            *m == method as usize && *s == scope && *e == entry && *f == flags
        });
        eprintln!(
            "NATIVE_MONO_PINVOKE: {scope}!{entry} flags={flags:#x} admitted={}",
            binding.is_some()
        );
        let Some((_, _, _, _, address)) = binding else {
            return Err("unadmitted native Windows P/Invoke".into());
        };
        if target.is_null() {
            return Err("NULL P/Invoke target slot".into());
        }
        unsafe {
            target.write(*address as Object);
        }
        Ok::<_, String>(1)
    });
    result.ok().and_then(Result::ok).unwrap_or(-1)
}
struct Services<'a> {
    memory: &'a mut Memory,
    runtime: &'a mut Runtime64,
}
impl Drop for Services<'_> {
    fn drop(&mut self) {
        if let Err(error) = self.memory.unmap(SCRATCH, 0x21000) {
            eprintln!("native service scratch cleanup: {error}");
            std::process::abort();
        }
    }
}
impl Services<'_> {
    fn call(&mut self, dll: &str, name: &str, args: &[u64]) -> Result<u64, u32> {
        let mut cpu = Cpu64::new(0, STACK + 0x100);
        self.memory
            .write_int(cpu.regs[4], 8, 0)
            .map_err(|_| 998u32)?;
        for (&index, &value) in [1, 2, 8, 9].iter().zip(args) {
            cpu.regs[index] = value;
        }
        for (index, value) in args.iter().enumerate().skip(4) {
            self.memory
                .write_int(cpu.regs[4] + 0x28 + (index as u64 - 4) * 8, 8, *value)
                .map_err(|_| 998u32)?;
        }
        self.runtime
            .dispatch(dll, name, &mut cpu, self.memory)
            .map_err(|_| 50u32)?;
        Ok(cpu.regs[0])
    }
    fn last_error(&self) -> u32 {
        self.memory
            .read_int(self.runtime.teb() + 0x68, 4)
            .unwrap_or(998) as u32
    }
    fn boolean(&mut self, name: &str, args: &[u64]) -> Result<(), u32> {
        if self.call("kernel32.dll", name, args)? == 0 {
            Err(self.last_error())
        } else {
            Ok(())
        }
    }
    fn path(&mut self, name: &str) -> Result<u64, u32> {
        let bytes: Vec<u8> = name
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        if bytes.len() > 65536 {
            return Err(206);
        }
        self.memory.write(SCRATCH, &bytes).map_err(|_| 998u32)?;
        Ok(SCRATCH)
    }
    fn wide(&self, at: u64, units: usize) -> Result<String, u32> {
        if units > 32768 {
            return Err(206);
        }
        let mut bytes = vec![0; units * 2];
        self.memory.read(at, &mut bytes).map_err(|_| 998u32)?;
        String::from_utf16(
            &bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>(),
        )
        .map_err(|_| 123)
    }
}
fn service<T>(operation: impl FnOnce(&mut Services<'_>) -> Result<T, u32>) -> Result<T, u32> {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        callbacks::with_service(|memory, runtime| {
            memory.map(SCRATCH, 0x21000, READ | WRITE)?;
            let mut services = Services { memory, runtime };
            Ok(operation(&mut services))
        })
    }));
    match result {
        Ok(Ok(value)) => value,
        _ => Err(50),
    }
}
fn errno<T: Default>(status: u32) -> T {
    unsafe {
        *libc::__error() = status as i32;
    }
    T::default()
}
unsafe fn wide(p: *const u16) -> Result<String, u32> {
    if p.is_null() {
        return Err(87);
    }
    for length in 0..32768 {
        if unsafe { p.add(length).read_unaligned() } == 0 {
            let units: Vec<_> = (0..length)
                .map(|i| unsafe { p.add(i).read_unaligned() })
                .collect();
            return String::from_utf16(&units).map_err(|_| 123);
        }
    }
    Err(206)
}
fn new_string(value: String) -> Object {
    let Ok(value) = CString::new(value) else {
        return ptr::null_mut();
    };
    let api = API.get().unwrap();
    unsafe { (api.string_new)((api.domain)(), value.as_ptr()) }
}
unsafe extern "C" fn platform() -> i32 {
    if callbacks::with_service(|_, _| Ok(())).is_ok() {
        2
    } else {
        std::process::abort();
    }
}
unsafe extern "C" fn directory_separator() -> u16 {
    b'\\' as u16
}
unsafe extern "C" fn alternate_separator() -> u16 {
    b'/' as u16
}
unsafe extern "C" fn volume_separator() -> u16 {
    b':' as u16
}
unsafe extern "C" fn path_separator() -> u16 {
    b';' as u16
}
unsafe extern "C" fn newline() -> Object {
    new_string("\r\n".into())
}
unsafe extern "C" fn os_version() -> Object {
    let value = service(|s| {
        s.memory.write_int(OUTPUT, 4, 276).map_err(|_| 998u32)?;
        s.boolean("GetVersionExW", &[OUTPUT])?;
        let a = s.memory.read_int(OUTPUT + 4, 4).map_err(|_| 998u32)?;
        let b = s.memory.read_int(OUTPUT + 8, 4).map_err(|_| 998u32)?;
        let c = s.memory.read_int(OUTPUT + 12, 4).map_err(|_| 998u32)?;
        Ok(format!("{a}.{b}.{c}.0"))
    });
    value.map(new_string).unwrap_or_else(errno)
}
fn query_directory(temp: bool) -> Result<String, u32> {
    service(|s| {
        let size = s.call(
            "kernel32.dll",
            if temp {
                "GetTempPathW"
            } else {
                "GetCurrentDirectoryW"
            },
            &[32768, OUTPUT],
        )? as usize;
        if size == 0 {
            return Err(s.last_error());
        }
        s.wide(OUTPUT, size)
    })
}
unsafe extern "C" fn temp_path() -> Object {
    query_directory(true).map(new_string).unwrap_or_else(errno)
}
unsafe extern "C" fn current_directory(error: *mut i32) -> Object {
    let value = query_directory(false);
    if !error.is_null() {
        unsafe {
            error.write_unaligned(value.as_ref().err().copied().unwrap_or(0) as i32);
        }
    }
    value.map(new_string).unwrap_or(ptr::null_mut())
}
fn std_handle(which: i32) -> Object {
    service(|s| s.call("kernel32.dll", "GetStdHandle", &[which as u32 as u64]))
        .map(|v| v as Object)
        .unwrap_or_else(errno)
}
unsafe extern "C" fn console_input() -> Object {
    std_handle(-10)
}
unsafe extern "C" fn console_output() -> Object {
    std_handle(-11)
}
unsafe extern "C" fn console_error() -> Object {
    std_handle(-12)
}
unsafe extern "C" fn input_codepage() -> i32 {
    service(|s| s.call("kernel32.dll", "GetConsoleCP", &[]))
        .map(|v| v as i32)
        .unwrap_or_else(errno)
}
unsafe extern "C" fn output_codepage() -> i32 {
    service(|s| s.call("kernel32.dll", "GetConsoleOutputCP", &[]))
        .map(|v| v as i32)
        .unwrap_or_else(errno)
}
unsafe extern "C" fn attributes(path: *const u16, level: i32, output: *mut u8) -> i32 {
    if output.is_null() || level != 0 {
        return errno(87);
    }
    let path = match unsafe { wide(path) } {
        Ok(p) => p,
        Err(e) => return errno(e),
    };
    let value = service(|s| {
        let path = s.path(&path)?;
        s.boolean("GetFileAttributesExW", &[path, 0, OUTPUT])?;
        let mut bytes = [0; 36];
        s.memory.read(OUTPUT, &mut bytes).map_err(|_| 998u32)?;
        Ok(bytes)
    });
    match value {
        Ok(bytes) => {
            unsafe {
                ptr::copy_nonoverlapping(bytes.as_ptr(), output, 36);
            }
            1
        }
        Err(e) => errno(e),
    }
}
unsafe extern "C" fn full_path(
    path: *const u16,
    capacity: i32,
    output: *mut u16,
    file_part: *mut Object,
) -> i32 {
    if !(0..=32768).contains(&capacity) {
        return errno(87);
    }
    let path = match unsafe { wide(path) } {
        Ok(p) => p,
        Err(e) => return errno(e),
    };
    let value = service(|s| {
        let path = s.path(&path)?;
        let length = s.call(
            "kernel32.dll",
            "GetFullPathNameW",
            &[path, 32768, OUTPUT, STACK + 0x800],
        )? as usize;
        if length == 0 {
            return Err(s.last_error());
        }
        s.wide(OUTPUT, length)
    });
    let value = match value {
        Ok(s) => s,
        Err(e) => return errno(e),
    };
    let units: Vec<_> = value.encode_utf16().collect();
    if capacity as usize <= units.len() {
        return (units.len() + 1) as i32;
    }
    if output.is_null() {
        return errno(87);
    }
    unsafe {
        ptr::copy_nonoverlapping(units.as_ptr(), output, units.len());
        output.add(units.len()).write(0);
        if !file_part.is_null() {
            let offset = value
                .rfind(['\\', '/'])
                .map(|i| value[..=i].encode_utf16().count())
                .unwrap_or(0);
            file_part.write(output.add(offset).cast());
        }
    }
    units.len() as i32
}
unsafe extern "C" fn thread_error_mode(mode: u32, previous: *mut u32) -> i32 {
    let value = service(|s| {
        s.boolean(
            "SetThreadErrorMode",
            &[mode as u64, if previous.is_null() { 0 } else { OUTPUT }],
        )?;
        if previous.is_null() {
            Ok(0)
        } else {
            s.memory
                .read_int(OUTPUT, 4)
                .map(|v| v as u32)
                .map_err(|_| 998u32)
        }
    });
    match value {
        Ok(old) => {
            if !previous.is_null() {
                unsafe {
                    previous.write_unaligned(old);
                }
            }
            1
        }
        Err(e) => errno(e),
    }
}
unsafe extern "C" fn random(algorithm: Object, output: *mut u8, count: i32, flags: i32) -> i32 {
    if output.is_null() || !(0..=65536).contains(&count) {
        return 0xc000000du32 as i32;
    }
    let value = service(|s| {
        let status = s.call(
            "bcrypt.dll",
            "BCryptGenRandom",
            &[algorithm as u64, SCRATCH, count as u64, flags as u32 as u64],
        )? as u32;
        let mut bytes = vec![0; count as usize];
        if status == 0 {
            s.memory.read(SCRATCH, &mut bytes).map_err(|_| 998u32)?;
        }
        Ok((status, bytes))
    });
    match value {
        Ok((0, bytes)) => {
            unsafe {
                ptr::copy_nonoverlapping(bytes.as_ptr(), output, bytes.len());
            }
            0
        }
        Ok((status, _)) => status as i32,
        Err(_) => 0xc0000001u32 as i32,
    }
}
mod io;

#[cfg(test)]
mod tests {
    use super::super::callbacks;
    use super::*;
    #[test]
    fn classlib_services_use_current_vfs_preserve_failed_outputs_and_revoke_owner() {
        struct Directory(std::path::PathBuf);
        impl Directory {
            fn path(&self) -> &std::path::Path {
                &self.0
            }
        }
        impl Drop for Directory {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let root = Directory(std::env::temp_dir().join(format!(
                "wire-classlib-services-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )));
        std::fs::create_dir(root.path()).unwrap();
        std::fs::write(root.path().join("present.bin"), [11, 22, 33, 44]).unwrap();
        std::fs::create_dir(root.path().join("Temp")).unwrap();
        let mut memory = Memory::default();
        let guest_stack = crate::x64::process::STACK;
        memory.map(guest_stack, 65536, READ | WRITE).unwrap();
        let mut runtime = Runtime64::new(
            &mut memory,
            0x140000000,
            guest_stack,
            65536,
            "classlib-service-test",
        )
        .unwrap();
        runtime.set_filesystem(crate::fs::GuestFs::new(root.path()).unwrap());
        let path: Vec<u16> = "C:\\present.bin".encode_utf16().chain(Some(0)).collect();
        let missing: Vec<u16> = "C:\\missing.bin".encode_utf16().chain(Some(0)).collect();
        let temporary: Vec<u16> = "C:\\Temp\\owned.bin"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let ((), _) = callbacks::with_owner(&mut memory, &mut runtime, [0; 7], || {
            let mut data = [0xa5; 36];
            assert_eq!(
                unsafe { attributes(path.as_ptr(), 0, data.as_mut_ptr()) },
                1
            );
            assert_eq!(u32::from_le_bytes(data[32..36].try_into().unwrap()), 4);
            data.fill(0xa5);
            assert_eq!(
                unsafe { attributes(missing.as_ptr(), 0, data.as_mut_ptr()) },
                0
            );
            assert_eq!(data, [0xa5; 36]);
            assert_eq!(unsafe { *libc::__error() }, 2);
            assert_eq!(unsafe { input_codepage() }, 932);
            assert_eq!(unsafe { output_codepage() }, 932);
            let mut rng = [0x5a; 16];
            assert_eq!(
                unsafe { random(ptr::null_mut(), rng.as_mut_ptr(), 16, 0x100) },
                0xc000000du32 as i32
            );
            assert_eq!(rng, [0x5a; 16]);
            assert_eq!(
                unsafe { random(ptr::null_mut(), rng.as_mut_ptr(), 16, 2) },
                0
            );
            let mut error = -1;
            let handle = unsafe { io::open(temporary.as_ptr(), 1, 3, 0, 0x04000000, &mut error) };
            assert_eq!(handle as usize, usize::MAX);
            assert_eq!(error, 50); // Current Runtime64 rejects delete-on-close.
            assert!(!root.path().join("Temp/owned.bin").exists());
            let handle = unsafe { io::open(temporary.as_ptr(), 1, 3, 0, 0, &mut error) };
            assert_ne!(handle as usize, usize::MAX, "open error={error}");
            assert_eq!(error, 0);
            assert!(root.path().join("Temp/owned.bin").exists());
            assert_eq!(unsafe { io::close(handle, &mut error) }, 1);
            assert_eq!(error, 0);
            assert!(root.path().join("Temp/owned.bin").exists());
            assert_eq!(service::<()>(|_| Err(87)), Err(87));
            assert_eq!(service(|_| Ok(service::<()>(|_| Ok(())))), Ok(Err(50)));
        })
        .unwrap();
        assert!(memory.read_int(SCRATCH, 1).is_err());
        assert!(memory.read_int(STACK, 1).is_err());
        let mut data = [0x81; 36];
        assert_eq!(
            unsafe { attributes(path.as_ptr(), 0, data.as_mut_ptr()) },
            0
        );
        assert_eq!(unsafe { *libc::__error() }, 50);
        assert_eq!(data, [0x81; 36]);
    }
}
