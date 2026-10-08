//! Trusted-classlib prototype. Typed static ARM64 entries reuse Runtime64's
//! confined Win32 services without executing any guest instruction. This is
//! not a general native-pointer sandbox or the game's provider.
use super::*;
use std::{ffi::CStr, sync::OnceLock};
use wire::{
    fs::GuestFs,
    x64::runtime::{Runtime64, TEB},
};

mod io;

const SCRATCH: u64 = 0x51000000;
const SERVICE_STACK: u64 = 0x52000000;
struct Services {
    memory: Memory,
    runtime: Runtime64,
    filesystem: GuestFs,
    calls: u64,
}
thread_local! { static SERVICES: Cell<*mut Services> = const { Cell::new(ptr::null_mut()) }; }
struct Api {
    method_class: unsafe extern "C" fn(Object) -> Object,
    class_image: unsafe extern "C" fn(Object) -> Object,
    class_name: unsafe extern "C" fn(Object) -> *const c_char,
    class_namespace: unsafe extern "C" fn(Object) -> *const c_char,
    method_name: unsafe extern "C" fn(Object) -> *const c_char,
    signature: unsafe extern "C" fn(Object) -> Object,
    return_type: unsafe extern "C" fn(Object) -> Object,
    parameters: unsafe extern "C" fn(Object, *mut Object) -> Object,
    type_name: unsafe extern "C" fn(Object) -> *mut c_char,
    free: unsafe extern "C" fn(Object),
    domain_get: unsafe extern "C" fn() -> Object,
    string_new: unsafe extern "C" fn(Object, *const c_char) -> Object,
    array_addr: unsafe extern "C" fn(Object, i32, usize) -> Object,
    array_length: unsafe extern "C" fn(Object) -> usize,
    object_class: unsafe extern "C" fn(Object) -> Object,
    element_class: unsafe extern "C" fn(Object) -> Object,
    byte_class: usize,
    new_handle: unsafe extern "C" fn(Object, i32) -> Handle,
    free_handle: unsafe extern "C" fn(Handle),
    corlib: usize,
    fixture: usize,
}
static API: OnceLock<Api> = OnceLock::new();
unsafe extern "C" fn platform() -> i32 {
    2
}
unsafe extern "C" fn directory_separator() -> i32 {
    b'\\' as i32
}
unsafe extern "C" fn alternate_separator() -> i32 {
    b'/' as i32
}
unsafe extern "C" fn volume_separator() -> i32 {
    b':' as i32
}
unsafe extern "C" fn path_separator() -> i32 {
    b';' as i32
}
unsafe extern "C" fn current_directory(error: *mut i32) -> Object {
    let result = std::panic::catch_unwind(|| {
        SERVICES.with(|slot| {
            let state = slot.get();
            if state.is_null() {
                None
            } else {
                Some(unsafe { &(*state).filesystem }.cwd())
            }
        })
    });
    let Ok(Some(path)) = result else {
        if !error.is_null() {
            unsafe {
                error.write_unaligned(50);
            }
        }
        return ptr::null_mut();
    };
    // End the service borrow before calling Mono: string allocation may GC.
    let path = CString::new(path).unwrap();
    let api = API.get().unwrap();
    let result = unsafe { (api.string_new)((api.domain_get)(), path.as_ptr()) };
    if !error.is_null() {
        unsafe {
            error.write_unaligned(if result.is_null() { 8 } else { 0 });
        }
    }
    result
}
unsafe extern "C" fn os_version() -> Object {
    let value: String = with_services(|services| {
        let at = SCRATCH + 65536;
        services.memory.write_int(at, 4, 276).map_err(|_| 998u32)?;
        if services.call("GetVersionExW", &[at])? == 0 {
            return Err(50);
        }
        let major = services.memory.read_int(at + 4, 4).map_err(|_| 998u32)?;
        let minor = services.memory.read_int(at + 8, 4).map_err(|_| 998u32)?;
        let build = services.memory.read_int(at + 12, 4).map_err(|_| 998u32)?;
        Ok(format!("{major}.{minor}.{build}.0"))
    });
    if value.is_empty() {
        return ptr::null_mut();
    }
    let value = CString::new(value).unwrap();
    let api = API.get().unwrap();
    unsafe { (api.string_new)((api.domain_get)(), value.as_ptr()) }
}
unsafe extern "C" fn new_line() -> Object {
    let api = API.get().unwrap();
    unsafe { (api.string_new)((api.domain_get)(), c"\r\n".as_ptr()) }
}

fn install_identity(mono: &Mono) {
    let api = API.get().unwrap();
    let class = symbol!(
        mono.library,
        "mono_class_from_name",
        unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object
    );
    let add = symbol!(
        mono.library,
        "mono_add_internal_call",
        unsafe extern "C" fn(*const c_char, Object)
    );
    let method_flags = symbol!(
        mono.library,
        "mono_method_get_flags",
        unsafe extern "C" fn(Object, *mut u32) -> u32
    );
    for (namespace, class_name, name, return_name, params, pointer) in [
        (
            "System",
            "Environment",
            "GetOSVersionString",
            "System.String",
            Vec::<&str>::new(),
            os_version as *const () as Object,
        ),
        (
            "System",
            "Environment",
            "GetNewLine",
            "System.String",
            vec![],
            new_line as *const () as Object,
        ),
        (
            "System",
            "Environment",
            "get_Platform",
            "System.PlatformID",
            Vec::<&str>::new(),
            platform as *const () as Object,
        ),
        (
            "System.IO",
            "MonoIO",
            "get_DirectorySeparatorChar",
            "System.Char",
            vec![],
            directory_separator as *const () as Object,
        ),
        (
            "System.IO",
            "MonoIO",
            "get_AltDirectorySeparatorChar",
            "System.Char",
            vec![],
            alternate_separator as *const () as Object,
        ),
        (
            "System.IO",
            "MonoIO",
            "get_VolumeSeparatorChar",
            "System.Char",
            vec![],
            volume_separator as *const () as Object,
        ),
        (
            "System.IO",
            "MonoIO",
            "get_PathSeparator",
            "System.Char",
            vec![],
            path_separator as *const () as Object,
        ),
        (
            "System.IO",
            "MonoIO",
            "GetCurrentDirectory",
            "System.String",
            vec!["System.IO.MonoIOError&"],
            current_directory as *const () as Object,
        ),
        (
            "System.IO",
            "MonoIO",
            "Open",
            "System.IntPtr",
            vec![
                "System.Char*",
                "System.IO.FileMode",
                "System.IO.FileAccess",
                "System.IO.FileShare",
                "System.IO.FileOptions",
                "System.IO.MonoIOError&",
            ],
            io::open as *const () as Object,
        ),
        (
            "System.IO",
            "MonoIO",
            "Close",
            "System.Boolean",
            vec!["System.IntPtr", "System.IO.MonoIOError&"],
            io::close as *const () as Object,
        ),
        (
            "System.IO",
            "MonoIO",
            "Flush",
            "System.Boolean",
            vec!["System.IntPtr", "System.IO.MonoIOError&"],
            io::flush as *const () as Object,
        ),
        (
            "System.IO",
            "MonoIO",
            "GetFileType",
            "System.IO.MonoFileType",
            vec!["System.IntPtr", "System.IO.MonoIOError&"],
            io::file_type as *const () as Object,
        ),
        (
            "System.IO",
            "MonoIO",
            "GetLength",
            "System.Int64",
            vec!["System.IntPtr", "System.IO.MonoIOError&"],
            io::length as *const () as Object,
        ),
        (
            "System.IO",
            "MonoIO",
            "Seek",
            "System.Int64",
            vec![
                "System.IntPtr",
                "System.Int64",
                "System.IO.SeekOrigin",
                "System.IO.MonoIOError&",
            ],
            io::seek as *const () as Object,
        ),
        (
            "System.IO",
            "MonoIO",
            "Read",
            "System.Int32",
            vec![
                "System.IntPtr",
                "System.Byte[]",
                "System.Int32",
                "System.Int32",
                "System.IO.MonoIOError&",
            ],
            io::read as *const () as Object,
        ),
        (
            "System.IO",
            "MonoIO",
            "Write",
            "System.Int32",
            vec![
                "System.IntPtr",
                "System.Byte[]",
                "System.Int32",
                "System.Int32",
                "System.IO.MonoIOError&",
            ],
            io::write as *const () as Object,
        ),
    ] {
        let ns = CString::new(namespace).unwrap();
        let cls = CString::new(class_name).unwrap();
        let method_name = CString::new(name).unwrap();
        let class = unsafe { class(api.corlib as Object, ns.as_ptr(), cls.as_ptr()) };
        assert!(!class.is_null());
        let method = unsafe { (mono.method)(class, method_name.as_ptr(), params.len() as i32) };
        assert!(!method.is_null(), "missing identity method {name}");
        let mut implementation_flags = 0;
        unsafe {
            method_flags(method, &mut implementation_flags);
        }
        assert_ne!(
            implementation_flags & 0x1000,
            0,
            "{name} is not an InternalCall method"
        );
        let signature = unsafe { (api.signature)(method) };
        let returns = unsafe { type_name(api, (api.return_type)(signature)) };
        assert_eq!(returns, return_name);
        let mut iterator = ptr::null_mut();
        let mut actual = Vec::new();
        loop {
            let ty = unsafe { (api.parameters)(signature, &mut iterator) };
            if ty.is_null() {
                break;
            }
            actual.push(unsafe { type_name(api, ty) });
        }
        assert_eq!(actual, params);
        let key = CString::new(format!("{namespace}.{class_name}::{name}")).unwrap();
        unsafe {
            add(key.as_ptr(), pointer);
        }
        println!("NATIVE_IDENTITY_ICALL {namespace}.{class_name}::{name} signature_verified=1");
    }
}

static THREAD_READY: (std::sync::Mutex<u32>, std::sync::Condvar) =
    (std::sync::Mutex::new(0), std::sync::Condvar::new());
unsafe extern "C" fn thread_barrier() -> i32 {
    std::panic::catch_unwind(|| {
        let (lock, changed) = &THREAD_READY;
        let mut count = lock.lock().unwrap();
        *count += 1;
        changed.notify_all();
        while *count < 2 {
            let result = changed
                .wait_timeout(count, std::time::Duration::from_secs(5))
                .unwrap();
            count = result.0;
            if result.1.timed_out() {
                return 0;
            }
        }
        1
    })
    .unwrap_or(0)
}
struct Scope {
    state: Box<Services>,
}
impl Scope {
    fn new(filesystem: GuestFs) -> Self {
        let mut memory = Memory::default();
        memory.map(SCRATCH, 0x20000, READ | WRITE).unwrap();
        memory.map(SERVICE_STACK, 4096, READ | WRITE).unwrap();
        let mut runtime = Runtime64::new(
            &mut memory,
            BASE,
            SERVICE_STACK,
            4096,
            "native-services-probe",
        )
        .unwrap();
        runtime.set_filesystem(filesystem.clone());
        let mut state = Box::new(Services {
            memory,
            runtime,
            filesystem,
            calls: 0,
        });
        SERVICES.with(|slot| {
            assert!(slot.get().is_null());
            slot.set(&mut *state);
        });
        Self { state }
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        SERVICES.with(|slot| {
            assert_eq!(slot.get(), &mut *self.state as *mut Services);
            slot.set(ptr::null_mut());
        });
    }
}

unsafe fn text(pointer: *const c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned()
}
unsafe fn type_name(api: &Api, ty: Object) -> String {
    let value = unsafe { (api.type_name)(ty) };
    let name = unsafe { text(value) };
    unsafe { (api.free)(value.cast()) };
    name
}
unsafe extern "C" fn resolve(
    method: Object,
    scope: *const c_char,
    entry: *const c_char,
    flags: u32,
    target: *mut Object,
) -> i32 {
    let result = std::panic::catch_unwind(|| {
        let scope = unsafe { text(scope) };
        if !scope.eq_ignore_ascii_case("kernel32.dll") && !scope.eq_ignore_ascii_case("kernel32") {
            return 0;
        }
        let Some(api) = API.get() else {
            return -1;
        };
        let class = unsafe { (api.method_class)(method) };
        let image = unsafe { (api.class_image)(class) } as usize;
        let class_name = unsafe { text((api.class_name)(class)) };
        let namespace = unsafe { text((api.class_namespace)(class)) };
        let name = unsafe { text((api.method_name)(method)) };
        let entry = unsafe { text(entry) };
        let signature = unsafe { (api.signature)(method) };
        let return_type = unsafe { type_name(api, (api.return_type)(signature)) };
        let mut iterator = ptr::null_mut();
        let mut args = Vec::new();
        loop {
            let ty = unsafe { (api.parameters)(signature, &mut iterator) };
            if ty.is_null() {
                break;
            }
            args.push(unsafe { type_name(api, ty) });
        }
        let trusted_class = (image == api.corlib
            && ((namespace == "Microsoft.Win32" && class_name == "Win32Native")
                || (namespace.is_empty() && class_name == "Kernel32")
                || (namespace == "System.IO" && class_name == "Path")))
            || (image == api.fixture
                && namespace.is_empty()
                && class_name == "NativeServicesProbe");
        let charset = flags & 6;
        let trusted_class = trusted_class && flags & 0x700 == 0x100;
        let pointer: Object = if !trusted_class {
            ptr::null_mut()
        } else if name == "GetFileAttributesEx"
            && entry == "GetFileAttributesEx"
            && return_type == "System.Boolean"
            && args
                == [
                    "System.String",
                    "System.Int32",
                    "Microsoft.Win32.Win32Native.WIN32_FILE_ATTRIBUTE_DATA&",
                ]
            && charset == 6
            && flags & 64 != 0
        {
            attributes_utf8 as *const () as Object
        } else if entry == "GetFileAttributesExW"
            && return_type == "System.Boolean"
            && charset == 4
            && flags & 64 != 0
            && ((name == "GetFileAttributesExPrivate"
                && args
                    == [
                        "System.String",
                        "Interop.Kernel32.GET_FILEEX_INFO_LEVELS",
                        "Interop.Kernel32.WIN32_FILE_ATTRIBUTE_DATA&",
                    ])
                || (name == "GetFileAttributesExW"
                    && args
                        == [
                            "System.String",
                            "System.Int32",
                            "NativeServicesProbe.AttributeData&",
                        ]))
        {
            attributes_wide as *const () as Object
        } else if name == "GetFullPathName"
            && entry == "GetFullPathName"
            && return_type == "System.Int32"
            && args
                == [
                    "System.Char*",
                    "System.Int32",
                    "System.Char*",
                    "System.IntPtr",
                ]
        {
            full_path_wide as *const () as Object
        } else if name == "GetFullPathName"
            && entry == "GetFullPathName"
            && return_type == "System.Int32"
            && args
                == [
                    "System.String",
                    "System.Int32",
                    "System.Text.StringBuilder",
                    "System.IntPtr&",
                ]
            && charset == 4
            && flags & 64 != 0
        {
            full_path_builder_wide as *const () as Object
        } else if name == "SetThreadErrorMode"
            && entry == "SetThreadErrorMode"
            && return_type == "System.Boolean"
            && args == ["System.UInt32", "System.UInt32&"]
            && flags & 64 != 0
        {
            set_thread_error_mode as *const () as Object
        } else if name == "SetErrorMode"
            && entry == "SetErrorMode"
            && return_type == "System.Int32"
            && args == ["System.Int32"]
        {
            set_error_mode as *const () as Object
        } else {
            ptr::null_mut()
        };
        eprintln!(
            "PINVOKE_SERVICE {}.{} {}({}) flags={flags:#x} entry={entry} bound={}",
            class_name,
            name,
            return_type,
            args.join(","),
            !pointer.is_null()
        );
        if pointer.is_null() {
            -1
        } else {
            unsafe { *target = pointer };
            1
        }
    });
    result.unwrap_or(-1)
}
impl Services {
    fn call(&mut self, name: &str, args: &[u64]) -> Result<u64, u32> {
        let mut cpu = Cpu64::new(0, SERVICE_STACK + 0x100);
        self.memory
            .write_int(cpu.regs[4], 8, RETURN)
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
            .dispatch("kernel32.dll", name, &mut cpu, &mut self.memory)
            .map_err(|_| 50u32)?;
        self.calls += 1;
        Ok(cpu.regs[0])
    }
    fn attributes(&mut self, path: &str, level: i32, output: *mut u8) -> Result<i32, u32> {
        if output.is_null() {
            return Err(87);
        }
        if level != 0 {
            return Err(87);
        }
        eprintln!("NATIVE_FILE_QUERY path={path:?}");
        let bytes: Vec<u8> = path
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        if bytes.len() > 65536 {
            return Err(206);
        }
        self.memory.write(SCRATCH, &bytes).map_err(|_| 998u32)?;
        let value = self.call("GetFileAttributesExW", &[SCRATCH, 0, SCRATCH + 65536])? as i32;
        let error = self.memory.read_int(TEB + 0x68, 4).map_err(|_| 998u32)? as u32;
        if value == 0 {
            return Err(error);
        }
        let mut data = [0u8; 36];
        self.memory
            .read(SCRATCH + 65536, &mut data)
            .map_err(|_| 998u32)?;
        // Only admitted trusted marshaling wrappers reach this entry; their
        // ref struct retains valid native storage for these exact 36 bytes.
        unsafe { ptr::copy_nonoverlapping(data.as_ptr(), output, data.len()) };
        Ok(value)
    }
}
fn with_services<T: Default>(f: impl FnOnce(&mut Services) -> Result<T, u32>) -> T {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        SERVICES.with(|slot| {
            let state = slot.get();
            if state.is_null() {
                Err(50)
            } else {
                f(unsafe { &mut *state })
            }
        })
    }));
    match result {
        Ok(Ok(value)) => value,
        other => {
            let error = match other {
                Ok(Err(error)) => error,
                _ => 50,
            };
            // On this Unix Mono build SetLastError wrappers capture errno into
            // Mono's per-thread last-error TLS. Set it after all host work.
            unsafe { *libc::__error() = error as i32 };
            T::default()
        }
    }
}
unsafe fn wide(pointer: *const u16) -> Result<String, u32> {
    if pointer.is_null() {
        return Err(87);
    }
    for length in 0..32768 {
        if unsafe { pointer.add(length).read_unaligned() } == 0 {
            return String::from_utf16(unsafe { std::slice::from_raw_parts(pointer, length) })
                .map_err(|_| 123);
        }
    }
    Err(206)
}
unsafe extern "C" fn attributes_wide(path: *const u16, level: i32, output: *mut u8) -> i32 {
    with_services(|services| services.attributes(&unsafe { wide(path) }?, level, output))
}
unsafe extern "C" fn attributes_utf8(path: *const c_char, level: i32, output: *mut u8) -> i32 {
    with_services(|services| {
        if path.is_null() {
            return Err(87);
        }
        let name = unsafe { CStr::from_ptr(path) }
            .to_str()
            .map_err(|_| 123u32)?;
        services.attributes(name, level, output)
    })
}
unsafe extern "C" fn set_thread_error_mode(mode: u32, previous: *mut u32) -> i32 {
    with_services(|services| {
        let at = if previous.is_null() {
            0
        } else {
            SCRATCH + 65536
        };
        let value = services.call("SetThreadErrorMode", &[mode as u64, at])? as i32;
        if value == 0 {
            return Err(services
                .memory
                .read_int(TEB + 0x68, 4)
                .map_err(|_| 998u32)? as u32);
        }
        if !previous.is_null() {
            let value = services.memory.read_int(at, 4).map_err(|_| 998u32)? as u32;
            unsafe {
                previous.write_unaligned(value);
            }
        }
        Ok(value)
    })
}
unsafe extern "C" fn set_error_mode(mode: i32) -> i32 {
    with_services(|services| {
        services
            .call("SetErrorMode", &[mode as u32 as u64])
            .map(|x| x as i32)
    })
}
unsafe extern "C" fn full_path_wide(
    path: *const u16,
    capacity: i32,
    output: *mut u16,
    file_part: Object,
) -> i32 {
    with_services(|services| {
        if !file_part.is_null() || !(0..=32768).contains(&capacity) {
            return Err(87);
        }
        let name = unsafe { wide(path) }?;
        let full = services
            .filesystem
            .guest_path(&services.filesystem.resolve(&name)?);
        let units: Vec<u16> = full.encode_utf16().collect();
        if capacity as usize <= units.len() {
            return Ok((units.len() + 1) as i32);
        }
        if output.is_null() {
            return Err(87);
        }
        unsafe {
            ptr::copy_nonoverlapping(units.as_ptr(), output, units.len());
            output.add(units.len()).write(0);
        }
        Ok(units.len() as i32)
    })
}

unsafe extern "C" fn full_path_builder_wide(
    path: *const u16,
    capacity: i32,
    output: *mut u16,
    file_part: *mut Object,
) -> i32 {
    with_services(|services| {
        if !(0..=32768).contains(&capacity) {
            return Err(87);
        }
        let name = unsafe { wide(path) }?;
        let full = services
            .filesystem
            .guest_path(&services.filesystem.resolve(&name)?);
        let units: Vec<u16> = full.encode_utf16().collect();
        if capacity as usize <= units.len() {
            return Ok((units.len() + 1) as i32);
        }
        if output.is_null() {
            return Err(87);
        }
        unsafe {
            ptr::copy_nonoverlapping(units.as_ptr(), output, units.len());
            output.add(units.len()).write(0);
            if !file_part.is_null() {
                let index = units
                    .iter()
                    .rposition(|&unit| unit == b'\\' as u16)
                    .map_or(0, |index| index + 1);
                file_part.write(output.add(index).cast());
            }
        }
        Ok(units.len() as i32)
    })
}

pub fn run(mono: &Mono, assembly: &str, root: &std::path::Path) {
    let corlib = symbol!(
        mono.library,
        "mono_get_corlib",
        unsafe extern "C" fn() -> Object
    );
    let open = symbol!(
        mono.library,
        "mono_domain_assembly_open",
        unsafe extern "C" fn(Object, *const c_char) -> Object
    );
    let image = symbol!(
        mono.library,
        "mono_assembly_get_image",
        unsafe extern "C" fn(Object) -> Object
    );
    let class = symbol!(
        mono.library,
        "mono_class_from_name",
        unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object
    );
    let install = symbol!(
        mono.library,
        "wire_mono_install_pinvoke_adapter",
        unsafe extern "C" fn(
            u32,
            unsafe extern "C" fn(Object, *const c_char, *const c_char, u32, *mut Object) -> i32,
        ) -> i32
    );
    let filename = CString::new(assembly).unwrap();
    let fixture = unsafe { image(open(mono.domain, filename.as_ptr())) };
    assert!(!fixture.is_null());
    assert!(API
        .set(Api {
            method_class: symbol!(
                mono.library,
                "mono_method_get_class",
                unsafe extern "C" fn(Object) -> Object
            ),
            class_image: symbol!(
                mono.library,
                "mono_class_get_image",
                unsafe extern "C" fn(Object) -> Object
            ),
            class_name: symbol!(
                mono.library,
                "mono_class_get_name",
                unsafe extern "C" fn(Object) -> *const c_char
            ),
            class_namespace: symbol!(
                mono.library,
                "mono_class_get_namespace",
                unsafe extern "C" fn(Object) -> *const c_char
            ),
            method_name: symbol!(
                mono.library,
                "mono_method_get_name",
                unsafe extern "C" fn(Object) -> *const c_char
            ),
            signature: symbol!(
                mono.library,
                "mono_method_signature",
                unsafe extern "C" fn(Object) -> Object
            ),
            return_type: symbol!(
                mono.library,
                "mono_signature_get_return_type",
                unsafe extern "C" fn(Object) -> Object
            ),
            parameters: symbol!(
                mono.library,
                "mono_signature_get_params",
                unsafe extern "C" fn(Object, *mut Object) -> Object
            ),
            type_name: symbol!(
                mono.library,
                "mono_type_get_name",
                unsafe extern "C" fn(Object) -> *mut c_char
            ),
            free: symbol!(mono.library, "mono_free", unsafe extern "C" fn(Object)),
            domain_get: symbol!(
                mono.library,
                "mono_domain_get",
                unsafe extern "C" fn() -> Object
            ),
            string_new: symbol!(
                mono.library,
                "mono_string_new",
                unsafe extern "C" fn(Object, *const c_char) -> Object
            ),
            array_addr: symbol!(
                mono.library,
                "mono_array_addr_with_size",
                unsafe extern "C" fn(Object, i32, usize) -> Object
            ),
            array_length: symbol!(
                mono.library,
                "mono_array_length",
                unsafe extern "C" fn(Object) -> usize
            ),
            object_class: symbol!(
                mono.library,
                "mono_object_get_class",
                unsafe extern "C" fn(Object) -> Object
            ),
            element_class: symbol!(
                mono.library,
                "mono_class_get_element_class",
                unsafe extern "C" fn(Object) -> Object
            ),
            byte_class: {
                let get = symbol!(
                    mono.library,
                    "mono_get_byte_class",
                    unsafe extern "C" fn() -> Object
                );
                (unsafe { get() }) as usize
            },
            new_handle: mono.new_handle,
            free_handle: mono.free_handle,
            corlib: unsafe { corlib() } as usize,
            fixture: fixture as usize,
        })
        .is_ok());
    assert_eq!(unsafe { install(2, resolve) }, -1);
    assert_eq!(unsafe { install(1, resolve) }, 0);
    assert_eq!(unsafe { install(1, resolve) }, -1);
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::write(root.join("present.txt"), b"typed native Mono service\n").unwrap();
    std::fs::write(root.join("日本語.txt"), b"utf16\n").unwrap();
    let filesystem = GuestFs::new(root).unwrap();
    let scope = Scope::new(filesystem.clone());
    install_identity(mono);
    let add_icall = symbol!(
        mono.library,
        "mono_add_internal_call",
        unsafe extern "C" fn(*const c_char, Object)
    );
    unsafe {
        add_icall(
            c"NativeServicesProbe::ThreadBarrier".as_ptr(),
            thread_barrier as *const () as Object,
        );
    }
    let class = unsafe { class(fixture, c"".as_ptr(), c"NativeServicesProbe".as_ptr()) };
    assert!(!class.is_null());
    gc::run(mono, class);
    inventory(mono, root.parent().unwrap());
    if std::env::var_os("WIRE_MONO_REFLECTION_INVENTORY").is_some() {
        let method = unsafe { (mono.method)(class, c"Inventory".as_ptr(), 0) };
        let result = mono.invoke(method, ptr::null_mut());
        let to_utf8 = symbol!(
            mono.library,
            "mono_string_to_utf8",
            unsafe extern "C" fn(Object) -> *mut c_char
        );
        let result = unsafe { to_utf8(result) };
        assert!(!result.is_null());
        let text = unsafe { text(result) };
        std::fs::write(
            root.parent()
                .unwrap()
                .join("reflection-pinvoke-contracts.tsv"),
            &text,
        )
        .unwrap();
        unsafe {
            (API.get().unwrap().free)(result.cast());
        }
        println!(
            "PASS managed reflection P/Invoke inventory: methods={}",
            text.lines().count() - 1
        );
    }
    for name in [
        c"InterpreterRoots",
        c"TypedQueries",
        c"ManagedQueries",
        c"ManagedStreams",
    ] {
        let method = unsafe { (mono.method)(class, name.as_ptr(), 0) };
        assert!(!method.is_null());
        let result = mono.invoke(method, ptr::null_mut());
        assert!(!result.is_null());
        let unbox = symbol!(
            mono.library,
            "mono_object_unbox",
            unsafe extern "C" fn(Object) -> Object
        );
        let result = unsafe { unbox(result).cast::<i32>().read() };
        println!("NATIVE_SERVICE_RESULT {}={result}", name.to_str().unwrap());
        assert_eq!(result, 1);
    }
    let bytes = std::fs::read(root.join("日本語-stream.bin")).unwrap();
    assert_eq!(bytes.len(), 180124);
    for (index, byte) in bytes[..180123].iter().enumerate() {
        assert_eq!(*byte, (index * 13 + 7) as u8);
    }
    assert_eq!(bytes[180123], 99);
    let calls = scope.state.calls;
    drop(scope);
    let worker = unsafe { (mono.method)(class, c"ThreadWorker".as_ptr(), 1) } as usize;
    assert_ne!(worker, 0);
    let attach = symbol!(
        mono.library,
        "mono_thread_attach",
        unsafe extern "C" fn(Object) -> Object
    );
    let detach = symbol!(
        mono.library,
        "mono_thread_detach",
        unsafe extern "C" fn(Object)
    );
    let unbox = symbol!(
        mono.library,
        "mono_object_unbox",
        unsafe extern "C" fn(Object) -> Object
    );
    let domain = mono.domain as usize;
    let invoke = mono.invoke;
    let mut workers = Vec::new();
    for id in [1, 2] {
        let filesystem = filesystem.clone();
        workers.push(std::thread::spawn(move || {
            let thread = unsafe { attach(domain as Object) };
            assert!(!thread.is_null());
            let scope = Scope::new(filesystem);
            let mut id: i32 = id;
            let mut parameters = [(&mut id as *mut i32).cast()];
            let mut exception = ptr::null_mut();
            let result = unsafe {
                invoke(
                    worker as Object,
                    ptr::null_mut(),
                    parameters.as_mut_ptr(),
                    &mut exception,
                )
            };
            assert!(
                exception.is_null() && !result.is_null(),
                "service worker failed"
            );
            let result = unsafe { unbox(result).cast::<i32>().read() };
            let calls = scope.state.calls;
            drop(scope);
            unsafe {
                detach(thread);
            }
            assert_eq!(result, 1);
            calls
        }));
    }
    let calls = calls
        + workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .sum::<u64>();
    // Cached static P/Invoke entries cannot outlive their service token: this
    // call returns a checked error and performs no host filesystem access.
    let method = unsafe { (mono.method)(class, c"NoOwner".as_ptr(), 0) };
    let result = mono.invoke(method, ptr::null_mut());
    let unbox = symbol!(
        mono.library,
        "mono_object_unbox",
        unsafe extern "C" fn(Object) -> Object
    );
    assert_eq!(unsafe { unbox(result).cast::<i32>().read() }, 1);
    println!("PASS typed native Windows services: real classlib File.Exists, UTF16/ref struct, LastError, rejected signature, owner revoke, two-thread LastError, confined Runtime64; calls={} guest_instructions=0 native_game_backend_enabled=0", calls);
}

fn inventory(mono: &Mono, output: &std::path::Path) {
    let api = API.get().unwrap();
    let image = api.corlib as Object;
    let table = symbol!(
        mono.library,
        "mono_image_get_table_info",
        unsafe extern "C" fn(Object, i32) -> Object
    );
    let rows = symbol!(
        mono.library,
        "mono_table_info_get_rows",
        unsafe extern "C" fn(Object) -> i32
    );
    let row = symbol!(
        mono.library,
        "mono_metadata_decode_row",
        unsafe extern "C" fn(Object, i32, *mut u32, i32)
    );
    let string = symbol!(
        mono.library,
        "mono_metadata_string_heap",
        unsafe extern "C" fn(Object, u32) -> *const c_char
    );
    let blob = symbol!(
        mono.library,
        "mono_metadata_blob_heap",
        unsafe extern "C" fn(Object, u32) -> *const c_char
    );
    let blob_size = symbol!(
        mono.library,
        "mono_metadata_decode_blob_size",
        unsafe extern "C" fn(*const c_char, *mut *const c_char) -> u32
    );
    let method = symbol!(
        mono.library,
        "mono_get_method",
        unsafe extern "C" fn(Object, u32, Object) -> Object
    );
    let impl_map = unsafe { table(image, 28) };
    let modules = unsafe { table(image, 26) };
    let definitions = unsafe { table(image, 6) };
    let parameters = unsafe { table(image, 8) };
    let field_marshal = unsafe { table(image, 13) };
    let mut marshals = std::collections::BTreeMap::new();
    for i in 0..unsafe { rows(field_marshal) } {
        let mut data = [0u32; 2];
        unsafe {
            row(field_marshal, i, data.as_mut_ptr(), 2);
        }
        if data[0] & 1 == 0 {
            continue;
        }
        let encoded = unsafe { blob(image, data[1]) };
        let mut start = ptr::null();
        let length = unsafe { blob_size(encoded, &mut start) } as usize;
        assert!(length <= 65536);
        let bytes = unsafe { std::slice::from_raw_parts(start.cast::<u8>(), length) };
        let text: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        marshals.insert(data[0] >> 1, text);
    }
    let mut output_text = String::from("class\tmethod\ttoken\tlibrary\tentry\treturn\tparameters\tflags\tmarshal_blobs_by_parameter_sequence\n");
    let mut count = 0;
    for i in 0..unsafe { rows(impl_map) } {
        let mut data = [0u32; 4];
        unsafe {
            row(impl_map, i, data.as_mut_ptr(), 4);
        }
        if data[1] & 1 == 0 {
            continue;
        } // MemberForwarded tag 1 = MethodDef.
        let index = data[1] >> 1;
        let token = 0x06000000 | index;
        let method = unsafe { method(image, token, ptr::null_mut()) };
        assert!(!method.is_null());
        let class = unsafe { (api.method_class)(method) };
        let class_name = unsafe { text((api.class_name)(class)) };
        let namespace = unsafe { text((api.class_namespace)(class)) };
        let name = unsafe { text((api.method_name)(method)) };
        let signature = unsafe { (api.signature)(method) };
        let returns = unsafe { type_name(api, (api.return_type)(signature)) };
        let mut iterator = ptr::null_mut();
        let mut args = Vec::new();
        loop {
            let ty = unsafe { (api.parameters)(signature, &mut iterator) };
            if ty.is_null() {
                break;
            }
            args.push(unsafe { type_name(api, ty) });
        }
        let mut module_name = 0u32;
        unsafe {
            row(modules, data[3] as i32 - 1, &mut module_name, 1);
        }
        let library = unsafe { text(string(image, module_name)) };
        let entry = unsafe { text(string(image, data[2])) };
        let mut definition = [0u32; 6];
        unsafe {
            row(definitions, index as i32 - 1, definition.as_mut_ptr(), 6);
        }
        let end = if index < unsafe { rows(definitions) } as u32 {
            let mut next = [0u32; 6];
            unsafe {
                row(definitions, index as i32, next.as_mut_ptr(), 6);
            }
            next[5]
        } else {
            (unsafe { rows(parameters) }) as u32 + 1
        };
        let mut marshal = Vec::new();
        for parameter in definition[5]..end {
            if let Some(blob) = marshals.get(&parameter) {
                let mut data = [0u32; 3];
                unsafe {
                    row(parameters, parameter as i32 - 1, data.as_mut_ptr(), 3);
                }
                marshal.push(format!("{}:{blob}", data[1]));
            }
        }
        output_text.push_str(&format!("{namespace}.{class_name}\t{name}\t{token:#010x}\t{library}\t{entry}\t{returns}\t{}\t{:#06x}\t{}\n", args.join(";"), data[0], marshal.join(";")));
        count += 1;
    }
    std::fs::write(output.join("pinvoke-contracts.tsv"), output_text).unwrap();
    println!("PINVOKE_CONTRACT_INVENTORY methods={count} source=actual_mscorlib_metadata managed_reflection_used=0");
}
