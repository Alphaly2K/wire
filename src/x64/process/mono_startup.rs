//! Exact-sample, finite startup diagnostic provider. This is deliberately
//! separate from complete-game Admission and cannot resume into managed play.
use super::{Cpu64, Exports64, Memory, Runtime64};
use crate::fs::GuestFs;
use sha2::{Digest, Sha256};
use std::{
    ffi::{c_char, c_void, CString},
    marker::PhantomData,
    path::{Path, PathBuf},
    ptr,
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
    thread::{self, ThreadId},
};
const ORIGINAL: &str = "86a5a4de6bcd74a637d63c96d822dc989d4ffaed6ef1c15c0e6e1f528ae6708f";
const CANDIDATE: &str = "2cfdc2552a60ac42729bf73ef3eeb7e6b2f25031835f6a8e639ec3fa61ad11e7";
const EXE: &str = "94cccae89918110514672a6ec249a1252ffaeb28c0ae8d8633a9352988738f12";
const UNITY: &str = "a2022611884470ddab23927a3eb7bbaef3924c18972d30ba7b5330a8fd25bcc4";
const CORELIB_SUPPORT: &str = "b10bb3e59af90e92e240424ac88b1f96eb27906c3fa099958713d93801b4b078";
const CORELIB: &str = "6c76e06e6d44cf0f39bf59e6a90770968b70093ad4c1572dfbc5e46e431c5fbf";
const ROOT_TOKEN: u64 = 0x6ffe00000000;
const PROFILER_TOKEN: u64 = ROOT_TOKEN + 16;
const THREAD_TOKEN: u64 = ROOT_TOKEN + 32;
static USED: AtomicBool = AtomicBool::new(false);
type Object = *mut c_void;
macro_rules! symbol {
    ($library:expr, $name:literal, $ty:ty) => {{
        let pointer = unsafe { libc::dlsym($library, concat!($name, "\0").as_ptr().cast()) };
        if pointer.is_null() {
            return Err(concat!("native Mono startup symbol missing: ", $name).into());
        }
        unsafe { std::mem::transmute::<Object, $ty>(pointer) }
    }};
}
struct Native {
    // Keep the image resident even after cleanup: Mono/GC global code pointers
    // and helper threads must never point into an unloaded dylib.
    library: Object,
    owner: ThreadId,
    domain: Object,
    profiler: Object,
    thread_handle: Object,
    closed: bool,
    _owner_only: PhantomData<Rc<()>>,
}
impl Native {
    fn load(path: &Path) -> Result<Self, String> {
        if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            return Err("native Mono startup probe requires macOS ARM64".into());
        }
        if USED.swap(true, Ordering::SeqCst) {
            return Err(
                "native Mono startup probe permits one runtime lifetime per host process".into(),
            );
        }
        fingerprint(path, CANDIDATE, "native runtime before load")?;
        let path = CString::new(path.as_os_str().as_encoded_bytes()).map_err(|e| e.to_string())?;
        let library = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if library.is_null() {
            return Err("cannot load fingerprinted native Mono startup runtime".into());
        }
        let mode = symbol!(library, "mono_jit_set_aot_mode", unsafe extern "C" fn(i32));
        let prepare = symbol!(
            library,
            "wire_mono_prepare_memory_adapter",
            unsafe extern "C" fn(u32) -> i32
        );
        let userdata = symbol!(
            library,
            "mono_class_get_userdata_offset",
            unsafe extern "C" fn() -> i32
        );
        let cleanup = symbol!(library, "mono_jit_cleanup", unsafe extern "C" fn(Object));
        // Pre-resolve cleanup before any domain exists. The locked enum is INTERP.
        let _ = cleanup;
        unsafe {
            mode(5);
        }
        if unsafe { prepare(1) } != 0 || unsafe { userdata() } != 232 {
            return Err("native Mono startup preparation/class userdata ABI mismatch".into());
        }
        Ok(Self {
            library,
            owner: thread::current().id(),
            domain: ptr::null_mut(),
            profiler: ptr::null_mut(),
            thread_handle: ptr::null_mut(),
            closed: false,
            _owner_only: PhantomData,
        })
    }
    fn cleanup(&mut self) -> Result<(), String> {
        if self.owner != thread::current().id() {
            return Err("native Mono startup cleanup on foreign owner thread".into());
        }
        if !self.closed {
            self.closed = true;
            if !self.domain.is_null() {
                if !self.thread_handle.is_null() {
                    let free = symbol!(
                        self.library,
                        "mono_gchandle_free_v2",
                        unsafe extern "C" fn(Object)
                    );
                    unsafe {
                        free(self.thread_handle);
                    }
                    self.thread_handle = ptr::null_mut();
                }
                let cleanup = symbol!(
                    self.library,
                    "mono_jit_cleanup",
                    unsafe extern "C" fn(Object)
                );
                unsafe {
                    cleanup(self.domain);
                }
                self.domain = ptr::null_mut();
                self.profiler = ptr::null_mut();
            }
        }
        Ok(())
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            eprintln!("native Mono startup cleanup failed: {error}");
            std::process::abort();
        }
    }
}
// Only the finite initialization sequence is admitted to native execution. Plugin activation
// needs the owner scheduler and typed guest reentry, not a fake NULL result.
// Fail-stop also prevents Mono's native-library fallback after callback failure.
unsafe extern "C" fn unsupported_plugin(_: *const c_char, _: *mut i32) -> *const c_char {
    eprintln!("NATIVE_MONO_STARTUP_REJECT: plugin callback activated before typed guest reentry is admitted");
    std::process::abort();
}
pub(super) struct Probe {
    library: PathBuf,
    native: Option<Native>,
    replaced: bool,
    root_ready: bool,
    main_thread: bool,
    main_thread_ready: bool,
    pre_bindings: bool,
    pre_bindings_ready: bool,
    stopped: bool,
    skip_etw: bool,
    plugin_registered: bool,
    events: Vec<String>,
}
fn remap_exports(exports: &mut Exports64, runtime: &mut Runtime64) -> Result<(), String> {
    // Reject folded addresses with multiple export names rather than guessing
    // that identical original machine code implies identical native signatures.
    let mut addresses = std::collections::BTreeMap::new();
    for (name, address) in &exports.names {
        if addresses.insert(*address, name.clone()).is_some() {
            return Err("native startup export has ambiguous original address aliases".into());
        }
    }
    let gateways: std::collections::BTreeMap<_, _> = addresses
        .into_iter()
        .map(|(address, name)| (address, runtime.register("mono-2.0-bdwgc.dll", &name)))
        .collect();
    for address in exports.names.values_mut() {
        *address = gateways[address];
    }
    for (ordinal, address) in &mut exports.ordinals {
        *address = gateways.get(address).copied().unwrap_or_else(|| {
            runtime.register("mono-2.0-bdwgc.dll", &format!("ordinal-{ordinal}"))
        });
    }
    Ok(())
}
fn fingerprint(path: &Path, expected: &str, label: &str) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read startup probe {label}: {e}"))?;
    fingerprint_bytes(&bytes, expected, label)
}
fn fingerprint_bytes(bytes: &[u8], expected: &str, label: &str) -> Result<(), String> {
    if format!("{:x}", Sha256::digest(bytes)) != expected {
        return Err(format!(
            "native Mono startup probe {label} fingerprint mismatch"
        ));
    }
    Ok(())
}
fn string(memory: &Memory, address: u64) -> Result<CString, String> {
    if address == 0 {
        return Err("NULL startup string".into());
    }
    let mut bytes = Vec::new();
    for offset in 0..32768 {
        let byte = memory.read_int(
            address
                .checked_add(offset)
                .ok_or("startup string overflow")?,
            1,
        )? as u8;
        if byte == 0 {
            return CString::new(bytes).map_err(|e| e.to_string());
        }
        bytes.push(byte);
    }
    Err("startup string exceeds 32768 bytes".into())
}
fn nullable_string(memory: &Memory, address: u64) -> Result<Option<CString>, String> {
    if address == 0 {
        Ok(None)
    } else {
        string(memory, address).map(Some)
    }
}
fn pointer(value: &Option<CString>) -> *const c_char {
    value.as_ref().map_or(ptr::null(), |s| s.as_ptr())
}
fn host_path(fs: &GuestFs, value: &CString) -> Result<CString, String> {
    let value = value.to_str().map_err(|e| e.to_string())?;
    let path = fs
        .resolve(value)
        .map_err(|e| format!("startup path error {e}: {value}"))?;
    CString::new(path.as_os_str().as_encoded_bytes()).map_err(|e| e.to_string())
}
fn argv(memory: &Memory, count: u64, address: u64) -> Result<Vec<CString>, String> {
    let count = count as u32 as i32;
    if !(0..=64).contains(&count) {
        return Err("startup argc outside 0..64".into());
    }
    (0..count as u64)
        .map(|index| {
            let slot = address
                .checked_add(index * 8)
                .ok_or("startup argv overflow")?;
            string(memory, memory.read_int(slot, 8)?)
        })
        .collect()
}
impl Probe {
    pub(super) fn new(
        exe: &Path,
        library: &Path,
        skip_etw: bool,
        boundary: &str,
    ) -> Result<Self, String> {
        let (main_thread, pre_bindings) = match boundary {
            "root" => (false, false),
            "main-thread" => (true, false),
            "pre-bindings" => (true, true),
            _ => {
                return Err(
                    "native Mono startup boundary must be root, main-thread or pre-bindings".into(),
                )
            }
        };
        fingerprint(exe, EXE, "EXE")?;
        let root = exe.parent().ok_or("startup probe missing game root")?;
        fingerprint(&root.join("UnityPlayer.dll"), UNITY, "UnityPlayer")?;
        fingerprint(
            &root.join("AliceToPapaADV_1-0-0_Data/Managed/mscorlib.dll"),
            CORELIB,
            "corelib",
        )?;
        fingerprint(
            &root.join("AliceToPapaADV_1-0-0_Data/Managed/mscorlib.dll.dylib"),
            CORELIB_SUPPORT,
            "precompiled corelib support",
        )?;
        let library = library.canonicalize().map_err(|e| e.to_string())?;
        fingerprint(&library, CANDIDATE, "native runtime")?;
        Ok(Self {
            library,
            native: None,
            replaced: false,
            root_ready: false,
            main_thread,
            main_thread_ready: false,
            pre_bindings,
            pre_bindings_ready: false,
            stopped: false,
            skip_etw,
            plugin_registered: false,
            events: Vec::new(),
        })
    }
    pub(super) fn replace(
        &mut self,
        bytes: &[u8],
        exports: &mut Exports64,
        runtime: &mut Runtime64,
    ) -> Result<(), String> {
        if self.replaced {
            return Err("second original Mono image rejected".into());
        }
        fingerprint_bytes(bytes, ORIGINAL, "original Mono")?;
        remap_exports(exports, runtime)?;
        self.replaced = true;
        self.events.push("selected native root-only diagnostic provider before original Mono mapping/TLS/DllMain; full game admission remains closed".into());
        Ok(())
    }
    pub(super) fn dispatch(
        &mut self,
        function: &str,
        cpu: &mut Cpu64,
        memory: &mut Memory,
        fs: &GuestFs,
    ) -> Result<bool, String> {
        if self.stopped {
            return Err("native Mono startup probe is closed; resume rejected".into());
        }
        // Check the Win64 return before mutating native state. Every guest input
        // is copied into owned storage before FFI; no guest pointer crosses it.
        let rsp = cpu.regs[4];
        memory.read_int(rsp, 8)?;
        rsp.checked_add(8).ok_or("startup return stack overflow")?;
        let a = [cpu.regs[1], cpu.regs[2], cpu.regs[8], cpu.regs[9]];
        // Exact original export is RET 0 and does not dereference the table.
        if function == "mono_unity_install_memory_callbacks" {
            cpu.return_integer(memory, cpu.regs[0])?;
            self.events.push("mono_unity_install_memory_callbacks: exact original immediate return; guest table not read".into());
            return Ok(false);
        }
        if self.native.is_none() {
            self.native = Some(Native::load(&self.library)?);
        }
        let native = self.native.as_mut().unwrap();
        if native.owner != thread::current().id() {
            return Err("native Mono startup call on foreign owner thread".into());
        }
        let lib = native.library;
        let mut result = 0;
        let mut ready = false;
        match function {
            "mono_set_find_plugin_callback" => {
                if a[0] == 0 {
                    return Err("sample startup requires a plugin callback".into());
                }
                memory.check(a[0], 1, super::super::memory::EXECUTE)?;
                let f = symbol!(
                    lib,
                    "mono_set_find_plugin_callback",
                    unsafe extern "C" fn(
                        unsafe extern "C" fn(*const c_char, *mut i32) -> *const c_char,
                    )
                );
                unsafe {
                    f(unsupported_plugin);
                }
                self.plugin_registered = true;
                self.events.push(format!("plugin callback guest={:#x}; activation fail-stops until typed guest reentry is admitted", a[0]));
            }
            "mono_class_get_userdata_offset" => {
                result = 232;
            }
            "mono_set_dirs" => {
                let dirs = [
                    host_path(fs, &string(memory, a[0])?)?,
                    host_path(fs, &string(memory, a[1])?)?,
                ];
                let f = symbol!(
                    lib,
                    "mono_set_dirs",
                    unsafe extern "C" fn(*const c_char, *const c_char)
                );
                unsafe {
                    f(dirs[0].as_ptr(), dirs[1].as_ptr());
                }
                self.events
                    .push(format!("native directories: {:?}, {:?}", dirs[0], dirs[1]));
            }
            "mono_set_assemblies_path_null_separated" => {
                let mut data = Vec::new();
                let mut address = a[0];
                let mut terminated = false;
                for _ in 0..64 {
                    let item = string(memory, address)?;
                    if item.as_bytes().is_empty() {
                        terminated = true;
                        break;
                    }
                    address = address
                        .checked_add(item.as_bytes_with_nul().len() as u64)
                        .ok_or("startup path list overflow")?;
                    data.extend_from_slice(host_path(fs, &item)?.as_bytes_with_nul());
                }
                if !terminated {
                    return Err("startup path list exceeds 64 entries".into());
                }
                data.push(0);
                let f = symbol!(
                    lib,
                    "mono_set_assemblies_path_null_separated",
                    unsafe extern "C" fn(*const c_char)
                );
                unsafe {
                    f(data.as_ptr().cast());
                }
            }
            "mono_set_signal_chaining" | "mono_set_crash_chaining" => {
                let f = if function == "mono_set_signal_chaining" {
                    symbol!(lib, "mono_set_signal_chaining", unsafe extern "C" fn(i32))
                } else {
                    symbol!(lib, "mono_set_crash_chaining", unsafe extern "C" fn(i32))
                };
                unsafe {
                    f(a[0] as i32);
                }
            }
            "mono_parse_default_optimizations" => {
                let value = nullable_string(memory, a[0])?;
                if value.is_some() {
                    return Err("sample startup only admits default optimization query".into());
                }
                let f = symbol!(
                    lib,
                    "mono_parse_default_optimizations",
                    unsafe extern "C" fn(*const c_char) -> u32
                );
                result = unsafe { f(pointer(&value)) } as u64;
            }
            "mono_set_defaults" => {
                let f = symbol!(lib, "mono_set_defaults", unsafe extern "C" fn(i32, u32));
                unsafe {
                    f(a[0] as i32, a[1] as u32);
                }
            }
            "mono_jit_parse_options" => {
                let values = argv(memory, a[0], a[1])?;
                // Unknown CLI options can exit the runtime. Admit the exact
                // options observed in Unity and reject everything else first.
                for value in &values {
                    if value.as_bytes() != b"--gc-params=incremental=3"
                        && value.as_bytes() != b"-O=-float32"
                    {
                        return Err(format!(
                            "native Mono startup option not admitted: {value:?}"
                        ));
                    }
                }
                self.events.push(format!("native options: {values:?}"));
                let mut data: Vec<_> = values
                    .into_iter()
                    .map(CString::into_bytes_with_nul)
                    .collect();
                let mut pointers: Vec<_> = data
                    .iter_mut()
                    .map(|s| s.as_mut_ptr().cast::<c_char>())
                    .collect();
                let f = symbol!(
                    lib,
                    "mono_jit_parse_options",
                    unsafe extern "C" fn(i32, *mut *mut c_char)
                );
                unsafe {
                    f(pointers.len() as i32, pointers.as_mut_ptr());
                }
            }
            "mono_profiler_load" => {
                let value = string(memory, a[0])?;
                if value.as_bytes() != b"etw" || !self.skip_etw {
                    return Err(format!("native Mono startup profiler unsupported: {value:?}; ETW may only be explicitly disabled in the finite startup diagnostic probe"));
                }
                self.events.push("diagnostic platform divergence: Windows ETW explicitly disabled; not an implemented profiler contract".into());
            }
            "mono_unity_runtime_set_main_args" => {
                let values = argv(memory, a[0], a[1])?;
                let pointers: Vec<_> = values.iter().map(|s| s.as_ptr()).collect();
                let f = symbol!(
                    lib,
                    "mono_unity_runtime_set_main_args",
                    unsafe extern "C" fn(i32, *const *const c_char)
                );
                unsafe {
                    f(pointers.len() as i32, pointers.as_ptr());
                }
            }
            "mono_unity_set_data_dir" => {
                let value = host_path(fs, &string(memory, a[0])?)?;
                let f = symbol!(
                    lib,
                    "mono_unity_set_data_dir",
                    unsafe extern "C" fn(*const c_char)
                );
                unsafe {
                    f(value.as_ptr());
                }
            }
            "mono_profiler_enable_coverage" => {
                let f = symbol!(
                    lib,
                    "mono_profiler_enable_coverage",
                    unsafe extern "C" fn() -> i32
                );
                result = unsafe { f() } as u32 as u64;
            }
            "mono_profiler_create" => {
                if a[0] != 0 || !native.profiler.is_null() {
                    return Err("startup profiler only admits one NULL payload".into());
                }
                let f = symbol!(
                    lib,
                    "mono_profiler_create",
                    unsafe extern "C" fn(Object) -> Object
                );
                native.profiler = unsafe { f(ptr::null_mut()) };
                if native.profiler.is_null() {
                    return Err("native startup profiler creation failed".into());
                }
                result = PROFILER_TOKEN;
            }
            "mono_profiler_set_coverage_filter_callback" => {
                if a[0] != PROFILER_TOKEN || native.profiler.is_null() || a[1] != 0 {
                    return Err(
                        "startup coverage filter requires owned profiler token and NULL callback"
                            .into(),
                    );
                }
                let f = symbol!(
                    lib,
                    "mono_profiler_set_coverage_filter_callback",
                    unsafe extern "C" fn(Object, Object)
                );
                unsafe {
                    f(native.profiler, ptr::null_mut());
                }
            }
            "mono_jit_init_version" => {
                let name = string(memory, a[0])?;
                let version = string(memory, a[1])?;
                if self.root_ready
                    || !native.domain.is_null()
                    || name.as_bytes() != b"Unity Root Domain"
                    || version.as_bytes() != b"v4.0.30319"
                    || !self.plugin_registered
                {
                    return Err("unexpected sample root-domain initialization inputs".into());
                }
                let f = symbol!(
                    lib,
                    "mono_jit_init_version",
                    unsafe extern "C" fn(*const c_char, *const c_char) -> Object
                );
                native.domain = unsafe { f(name.as_ptr(), version.as_ptr()) };
                if native.domain.is_null() {
                    return Err("native Mono root-domain initialization failed".into());
                }
                result = ROOT_TOKEN;
                ready = !self.main_thread;
                self.root_ready = true;
            }
            "mono_config_parse" if self.root_ready && self.main_thread => {
                if a[0] != 0 {
                    return Err("startup config only admits the configured default file".into());
                }
                let f = symbol!(
                    lib,
                    "mono_config_parse",
                    unsafe extern "C" fn(*const c_char)
                );
                unsafe {
                    f(ptr::null());
                }
            }
            "mono_thread_current" if self.root_ready && self.main_thread => {
                let current = symbol!(lib, "mono_thread_current", unsafe extern "C" fn() -> Object);
                let strong = symbol!(
                    lib,
                    "mono_gchandle_new_v2",
                    unsafe extern "C" fn(Object, i32) -> Object
                );
                let target = symbol!(
                    lib,
                    "mono_gchandle_get_target_v2",
                    unsafe extern "C" fn(Object) -> Object
                );
                let _free = symbol!(lib, "mono_gchandle_free_v2", unsafe extern "C" fn(Object));
                let thread = unsafe { current() };
                if thread.is_null() {
                    return Err("native main thread is not attached".into());
                }
                if native.thread_handle.is_null() {
                    native.thread_handle = unsafe { strong(thread, 0) };
                    if native.thread_handle.is_null() {
                        return Err("native main thread root allocation failed".into());
                    }
                } else if unsafe { target(native.thread_handle) } != thread {
                    return Err("native startup thread identity changed".into());
                }
                result = THREAD_TOKEN;
                self.events.push("native thread object retained by v2 strong handle through complete diagnostic exposure; opaque guest token has no mapped object/header".into());
            }
            "mono_gc_wbarrier_set_field" if self.root_ready && self.main_thread => {
                store_thread_token(memory, cpu, a, !native.thread_handle.is_null())?;
                self.events.push(format!("diagnostic thread root token stored through checked guest stack slot {:#x}; raw guest destination never passed to native barrier", a[1]));
            }
            "mono_thread_set_main" if self.root_ready && self.main_thread => {
                if a[0] != THREAD_TOKEN || native.thread_handle.is_null() {
                    return Err("startup main thread requires owned thread token".into());
                }
                let target = symbol!(
                    lib,
                    "mono_gchandle_get_target_v2",
                    unsafe extern "C" fn(Object) -> Object
                );
                let set = symbol!(lib, "mono_thread_set_main", unsafe extern "C" fn(Object));
                let thread = unsafe { target(native.thread_handle) };
                if thread.is_null() {
                    return Err("startup main thread root lost target".into());
                }
                unsafe {
                    set(thread);
                }
                self.main_thread_ready = true;
                ready = !self.pre_bindings;
            }
            "mono_unity_domain_set_config" if self.main_thread_ready && self.pre_bindings => {
                if a[0] != ROOT_TOKEN || native.domain.is_null() {
                    return Err("startup domain config requires owned root-domain token".into());
                }
                let base = host_path(fs, &string(memory, a[1])?)?;
                let config = host_path(fs, &string(memory, a[2])?)?;
                let f = symbol!(
                    lib,
                    "mono_unity_domain_set_config",
                    unsafe extern "C" fn(Object, *const c_char, *const c_char)
                );
                unsafe {
                    f(native.domain, base.as_ptr(), config.as_ptr());
                }
                self.events.push(format!(
                    "native domain config: base={base:?} config={config:?}"
                ));
            }
            "mono_unity_set_embeddinghostname" if self.main_thread_ready && self.pre_bindings => {
                let name = string(memory, a[0])?;
                let f = symbol!(
                    lib,
                    "mono_unity_set_embeddinghostname",
                    unsafe extern "C" fn(*const c_char)
                );
                unsafe {
                    f(name.as_ptr());
                }
                self.events.push(format!("native embedding host: {name:?}"));
            }
            "mono_runtime_unhandled_exception_policy_set"
                if self.main_thread_ready && self.pre_bindings =>
            {
                if a[0] as u32 != 0 {
                    return Err("startup only admits original legacy exception policy".into());
                }
                let f = symbol!(
                    lib,
                    "mono_runtime_unhandled_exception_policy_set",
                    unsafe extern "C" fn(i32)
                );
                unsafe {
                    f(0);
                }
                self.pre_bindings_ready = true;
                ready = true;
            }
            _ => {
                return Err(format!(
                    "native Mono startup typed export not admitted: {function}"
                ))
            }
        }
        cpu.return_integer(memory, result)?;
        self.events
            .push(format!("guest startup boundary {function} -> {result:#x}"));
        Ok(ready)
    }
    pub(super) fn finish(&mut self) -> Result<(), String> {
        if self.stopped {
            return Ok(());
        }
        self.stopped = true;
        if let Some(native) = &mut self.native {
            native.cleanup()?;
        }
        self.events
            .push("native startup probe closed on owner thread; guest resume prohibited".into());
        Ok(())
    }
    pub(super) fn report(&self) -> String {
        format!(
            "native-root-ready={}\nnative-main-thread-ready={}\nnative-pre-bindings-ready={}\nfull-game-admission=false\n{}\n",
            self.root_ready,
            self.main_thread_ready,
            self.pre_bindings_ready,
            self.events.join("\n")
        )
    }
    pub(super) fn stopped(&self) -> bool {
        self.stopped
            || (self.root_ready && !self.main_thread)
            || (self.main_thread_ready && !self.pre_bindings)
            || self.pre_bindings_ready
    }
}
fn store_thread_token(
    memory: &mut Memory,
    cpu: &Cpu64,
    a: [u64; 4],
    rooted: bool,
) -> Result<(), String> {
    // This bounded diagnostic exposes only an opaque main-thread identity. It
    // retains a real strong handle until cleanup, admits no interior byrefs,
    // and never resumes into managed/object dereferences. Production roots and
    // write barriers still require the independent complete-game admission.
    let first = cpu.regs[4]
        .checked_add(40)
        .ok_or("startup stack overflow")?;
    let end = a[1].checked_add(8).ok_or("startup root slot overflow")?;
    if !rooted
        || a[0] != 0
        || a[2] != THREAD_TOKEN
        || !a[1].is_multiple_of(8)
        || a[1] < first
        || a[1] < super::STACK
        || end > super::STACK + 64 * 1024 * 1024
    {
        return Err(
            "startup barrier admits only an owned thread token in a live main-stack slot".into(),
        );
    }
    memory.write_int(a[1], 8, THREAD_TOKEN)
}
#[cfg(test)]
mod tests {
    use super::super::super::memory::{READ, WRITE};
    use super::*;
    #[test]
    fn gateway_exports_preserve_named_ordinals_and_reject_ambiguous_signature_aliases() {
        let mut memory = Memory::default();
        memory
            .map(super::super::STACK, 65536, READ | WRITE)
            .unwrap();
        let mut runtime =
            Runtime64::new(&mut memory, 0x140000000, super::super::STACK, 65536, "test").unwrap();
        let mut ambiguous = Exports64 {
            names: [("a".into(), 0x1000), ("b".into(), 0x1000)].into(),
            ordinals: [(1, 0x1000)].into(),
        };
        let previous = runtime.gateways.len();
        assert!(remap_exports(&mut ambiguous, &mut runtime).is_err());
        assert_eq!(runtime.gateways.len(), previous);
        assert_eq!(ambiguous.names["a"], 0x1000);
        let mut exports = Exports64 {
            names: [("mono_thread_current".into(), 0x1000)].into(),
            ordinals: [(1, 0x1000), (2, 0x2000)].into(),
        };
        remap_exports(&mut exports, &mut runtime).unwrap();
        let named = exports.names["mono_thread_current"];
        assert_eq!(exports.ordinals[&1], named);
        assert_ne!(exports.ordinals[&2], named);
        assert_eq!(
            runtime.gateways.get(&named).unwrap(),
            "mono-2.0-bdwgc.dll!mono_thread_current"
        );
        assert!(memory.fetch(named, &mut [0]).is_err());
    }
    #[test]
    fn startup_inputs_reject_faults_overflows_and_unbounded_argc() {
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE).unwrap();
        memory.write(0x1000, b"abc\0").unwrap();
        memory.write_int(0x1100, 8, 0x1000).unwrap();
        assert_eq!(argv(&memory, 1, 0x1100).unwrap()[0].as_bytes(), b"abc");
        assert!(argv(&memory, u32::MAX as u64, 0x1100).is_err());
        assert!(argv(&memory, 65, 0x1100).is_err());
        assert!(argv(&memory, 2, u64::MAX - 7).is_err());
        assert!(string(&memory, 0).is_err());
        assert!(string(&memory, 0x2000).is_err());
        assert!(fingerprint_bytes(b"untrusted", ORIGINAL, "original").is_err());
    }
    #[test]
    fn diagnostic_thread_root_stores_reject_unowned_objects_and_nonstack_destinations() {
        let mut memory = Memory::default();
        memory.map(super::super::STACK, 4096, READ | WRITE).unwrap();
        let cpu = Cpu64::new(0, super::super::STACK + 8);
        let slot = super::super::STACK + 0x100;
        memory.write_int(slot, 8, 77).unwrap();
        for (args, rooted) in [
            ([0, slot, THREAD_TOKEN, 0], false),
            ([1, slot, THREAD_TOKEN, 0], true),
            ([0, slot, 0x1234, 0], true),
            ([0, cpu.regs[4], THREAD_TOKEN, 0], true),
            ([0, 0x1234, THREAD_TOKEN, 0], true),
            ([0, u64::MAX - 7, THREAD_TOKEN, 0], true),
            ([0, slot + 1, THREAD_TOKEN, 0], true),
        ] {
            assert!(store_thread_token(&mut memory, &cpu, args, rooted).is_err());
            assert_eq!(memory.read_int(slot, 8).unwrap(), 77);
        }
        store_thread_token(&mut memory, &cpu, [0, slot, THREAD_TOKEN, 0], true).unwrap();
        assert_eq!(memory.read_int(slot, 8).unwrap(), THREAD_TOKEN);
        memory.protect(super::super::STACK, 4096, READ).unwrap();
        assert!(store_thread_token(&mut memory, &cpu, [0, slot, THREAD_TOKEN, 0], true).is_err());
        assert_eq!(memory.read_int(slot, 8).unwrap(), THREAD_TOKEN);
    }
    #[test]
    fn exact_allocator_return_never_reads_guest_table_and_closed_probe_rejects_resume() {
        let mut probe = Probe {
            library: PathBuf::new(),
            native: None,
            replaced: true,
            root_ready: false,
            main_thread: false,
            main_thread_ready: false,
            pre_bindings: false,
            pre_bindings_ready: false,
            stopped: false,
            skip_etw: false,
            plugin_registered: false,
            events: Vec::new(),
        };
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE).unwrap();
        memory.write_int(0x1008, 8, 0x12345).unwrap();
        let mut cpu = Cpu64::new(0, 0x1008);
        cpu.regs[0] = 0xabcdef;
        cpu.regs[1] = u64::MAX; // Invalid table is ignored by this exact DLL.
        let fs = GuestFs::new(&std::env::current_dir().unwrap()).unwrap();
        assert!(!probe
            .dispatch(
                "mono_unity_install_memory_callbacks",
                &mut cpu,
                &mut memory,
                &fs
            )
            .unwrap());
        assert_eq!(cpu.rip, 0x12345);
        assert_eq!(cpu.regs[0], 0xabcdef);
        assert!(probe.native.is_none());
        probe.finish().unwrap();
        assert!(probe
            .dispatch(
                "mono_unity_install_memory_callbacks",
                &mut cpu,
                &mut memory,
                &fs
            )
            .is_err());
    }
}
