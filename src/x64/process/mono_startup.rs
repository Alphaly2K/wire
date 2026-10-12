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
const REMOTING_CANDIDATE: &str = "17f7859533bdeb5e636fc4e19a3aa1ac55502dbf089c6ede7c2b506ecdd4dcda";
const REMOTING_SUPPORT: &str = "a916d6f9c9199b524a65ebd93bdea328e31b248c415051a2d2a8609664580d6c";
const INCREMENTAL_CANDIDATE: &str =
    "c89ab8c9cc00a045e998cb9bc528f2959f478729136390a0d07e54b8e8b69bda";
const INCREMENTAL_SUPPORT: &str =
    "edd2730c016afafc55e3dbad617fcaff6332474a9fde6808205de8edb7e53c8e";
const ATTRIBUTES_CANDIDATE: &str =
    "94a8252dfb4bf7aa3584be4b1888afd128431cc7ea70b7bd6773fc299cab0a5e";
const ATTRIBUTES_SUPPORT: &str = "7cd421d0e370b09a865848a59e47aa2b1362fd9ff484804fe3b2c1b4a5bc8029";
const OBJECTS_CANDIDATE: &str = "c152197b7cf1623a3d40a51a36e1e44d888b77f337b1378bb979598d2fad2eb0";
const OBJECTS_SUPPORT: &str = "34705bafc092d1e644b2c7e0873340b398d1a81dd3e51c31af37a9ba0a29cea9";
const EXE: &str = "94cccae89918110514672a6ec249a1252ffaeb28c0ae8d8633a9352988738f12";
const UNITY: &str = "a2022611884470ddab23927a3eb7bbaef3924c18972d30ba7b5330a8fd25bcc4";
const CORELIB_SUPPORT: &str = "b10bb3e59af90e92e240424ac88b1f96eb27906c3fa099958713d93801b4b078";
const URP_2D: &str = "ef6c25659e4daafe2b3526e65154d532b7847a37da49533da0fd85545b92fa92";
const INPUT_SYSTEM: &str = "79aeb3ee230428c0c53102aa8392a306312b0350e1c745be9125dcec160f6dd1";
const CORE_MODULE: &str = "7de56d1259f25d8b0cfdc9f4c77030056886bb9ac119854d3155bd87a54c0af7";
const IMGUI_MODULE: &str = "577712fb19f217f2e9dcc870e6cf430d4a5836c5c4a745ae89466877dfbb9745";
const SUBSYSTEMS_MODULE: &str = "f25824d57c5f394b514df03bab7ace58d9e6eb8b55f52c5895404cb591eacf49";
const CORELIB: &str = "6c76e06e6d44cf0f39bf59e6a90770968b70093ad4c1572dfbc5e46e431c5fbf";
const ROOT_TOKEN: u64 = 0x6ffe00000000;
const PROFILER_TOKEN: u64 = ROOT_TOKEN + 16;
const THREAD_TOKEN: u64 = ROOT_TOKEN + 32;
static USED: AtomicBool = AtomicBool::new(false);
type Object = *mut c_void;
mod animation;
mod arrays;
mod attributes;
mod binding_strings;
mod bindings;
mod burst_shared;
mod callbacks;
mod closed_types;
mod constructors;
mod engine_objects;
mod engine_scalars;
mod errors;
mod field_layouts;
mod fields;
mod gc_scopes;
mod handles;
mod images;
mod input_events;
mod instances;
mod invoke;
mod jobs;
mod json;
mod logging;
mod managed_calls;
mod metadata;
mod names;
mod objects;
mod player_prefs;
mod profiler_recorders;
mod profiler_samples;
mod reentry;
mod reference_arrays;
mod reflection;
mod resources;
mod screen;
mod scriptable;
mod services;
mod subsystems;
mod unitytls;
mod unsafe_buffers;
mod value_layout;
mod virtual_methods;
mod workers;
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
    extra_profilers: Vec<Object>,
    thread_handle: Object,
    gc: gc_scopes::State,
    workers: workers::State,
    closed: bool,
    _owner_only: PhantomData<Rc<()>>,
}
impl Native {
    fn load(path: &Path, expected: &str) -> Result<Self, String> {
        if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            return Err("native Mono startup probe requires macOS ARM64".into());
        }
        if USED.swap(true, Ordering::SeqCst) {
            return Err(
                "native Mono startup probe permits one runtime lifetime per host process".into(),
            );
        }
        fingerprint(path, expected, "native runtime before load")?;
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
        if std::env::var("WIRE_NATIVE_MONO_MANAGED_TRACE")
            .ok()
            .as_deref()
            == Some("1")
        {
            let trace = symbol!(
                library,
                "mono_jit_set_trace_options",
                unsafe extern "C" fn(*const c_char) -> i32
            );
            if unsafe { trace(c"all".as_ptr()) } == 0 {
                return Err("native Mono managed trace setup failed".into());
            }
        }
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
            extra_profilers: Vec::new(),
            thread_handle: ptr::null_mut(),
            gc: gc_scopes::State::default(),
            workers: workers::State::default(),
            closed: false,
            _owner_only: PhantomData,
        })
    }
    fn cleanup(&mut self) -> Result<(), String> {
        if self.owner != thread::current().id() {
            return Err("native Mono startup cleanup on foreign owner thread".into());
        }
        if !self.closed {
            self.gc.restore()?;
            self.workers.close()?;
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
                burst_shared::prepare_vm_cleanup()?;
                unsafe {
                    cleanup(self.domain);
                }
                self.domain = ptr::null_mut();
                self.profiler = ptr::null_mut();
                self.extra_profilers.clear();
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
unsafe extern "C" fn unsupported_plugin(scope: *const c_char, _: *mut i32) -> *const c_char {
    let name = if scope.is_null() {
        "NULL".into()
    } else {
        unsafe { std::ffi::CStr::from_ptr(scope) }.to_string_lossy()
    };
    eprintln!("NATIVE_MONO_STARTUP_REJECT: plugin callback scope={name:?} activated before typed guest reentry is admitted");
    std::process::abort();
}
pub(super) struct Probe {
    library: PathBuf,
    library_hash: &'static str,
    native: Option<Native>,
    replaced: bool,
    root_ready: bool,
    main_thread: bool,
    main_thread_ready: bool,
    pre_bindings: bool,
    pre_bindings_ready: bool,
    bindings: bool,
    bindings_ready: bool,
    scalar_callbacks_verified: usize,
    profiler: bool,
    profiler_ready: bool,
    profiler_targets: [u64; 3],
    legacy_installed: bool,
    gc_events_verified: u32,
    shutdown_callbacks: usize,
    pre_images: bool,
    pre_images_ready: bool,
    tls_table: Option<unitytls::Table>,
    tls_leaf_verified: bool,
    images: bool,
    images_ready: bool,
    image_registry: images::Registry,
    image_api: Option<images::Api>,
    metadata: bool,
    metadata_ready: bool,
    metadata_registry: metadata::Registry,
    strings: bool,
    strings_ready: bool,
    object_registry: objects::Registry,
    object_api: Option<objects::Api>,
    managed_types: bool,
    managed_types_ready: bool,
    get_type_method: u64,
    managed_type_invokes: usize,
    reflection_registry: reflection::Registry,
    handle_api: Option<handles::Api>,
    handle_registry: handles::Registry,
    array_api: Option<arrays::Api>,
    array_registry: arrays::Registry,
    reflection_api: Option<reflection::Api>,
    logging: bool,
    logging_ready: bool,
    exceptions: bool,
    exceptions_ready: bool,
    assets: bool,
    asset_invokes: usize,
    input_map_invokes: usize,
    input_action_invokes: usize,
    virtual_resolutions: usize,
    serialization_invokes: usize,
    static_invokes: usize,
    static_output_writes: usize,
    instance_invokes: usize,
    instance_output_writes: usize,
    reference_array_api: Option<reference_arrays::Api>,
    reference_array_registry: reference_arrays::Registry,
    components: bool,
    component_invokes: usize,
    fields: bool,
    field_registry: fields::Registry,
    curves: bool,
    curves_constructor: Option<animation::Constructor>,
    ordinary_objects: bool,
    ordinary_objects_ready: bool,
    instance_api: Option<instances::Api>,
    instance_registry: instances::Registry,
    assembly_lookup: bool,
    assembly_lookup_ready: bool,
    name_api: Option<names::Api>,
    name_registry: names::Registry,
    attribute_arrays: bool,
    attribute_arrays_ready: bool,
    error_registry: errors::Registry,
    attributes: bool,
    attributes_ready: bool,
    attribute_api: Option<attributes::Api>,
    attribute_registry: attributes::Registry,
    imgui: bool,
    imgui_ready: bool,
    subsystem_map: bool,
    subsystem_map_ready: bool,
    subsystems: bool,
    subsystems_ready: bool,
    synchronization: bool,
    synchronization_ready: bool,
    classlib: bool,
    classlib_ready: bool,
    classlib_invokes: usize,
    registry: bindings::Registry,
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
fn runtime_profile(path: &Path) -> Result<(&'static str, &'static str), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read native diagnostic profile: {e}"))?;
    match format!("{:x}", Sha256::digest(bytes)).as_str() {
        CANDIDATE => Ok((CANDIDATE, CORELIB_SUPPORT)),
        REMOTING_CANDIDATE => Ok((REMOTING_CANDIDATE, REMOTING_SUPPORT)),
        INCREMENTAL_CANDIDATE => Ok((INCREMENTAL_CANDIDATE, INCREMENTAL_SUPPORT)),
        ATTRIBUTES_CANDIDATE => Ok((ATTRIBUTES_CANDIDATE, ATTRIBUTES_SUPPORT)),
        OBJECTS_CANDIDATE => Ok((OBJECTS_CANDIDATE, OBJECTS_SUPPORT)),
        _ => Err("native Mono diagnostic runtime fingerprint mismatch".into()),
    }
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
        let assets = boundary == "assets";
        let components = matches!(boundary, "components" | "assets");
        let fields = matches!(boundary, "fields" | "components" | "assets");
        let curves = matches!(boundary, "curves" | "fields" | "components" | "assets");
        let ordinary_objects = matches!(
            boundary,
            "objects" | "curves" | "fields" | "components" | "assets"
        );
        let assembly_lookup = matches!(
            boundary,
            "assembly-lookup" | "objects" | "curves" | "fields" | "components" | "assets"
        );
        let attribute_arrays = matches!(
            boundary,
            "attribute-arrays"
                | "assembly-lookup"
                | "objects"
                | "curves"
                | "fields"
                | "components"
                | "assets"
        );
        let attributes = matches!(
            boundary,
            "attributes"
                | "attribute-arrays"
                | "assembly-lookup"
                | "objects"
                | "curves"
                | "fields"
                | "components"
                | "assets"
        );
        let imgui = matches!(
            boundary,
            "imgui-cache"
                | "attributes"
                | "attribute-arrays"
                | "assembly-lookup"
                | "objects"
                | "curves"
                | "fields"
                | "components"
                | "assets"
        );
        let subsystem_map = matches!(
            boundary,
            "subsystem-map"
                | "imgui-cache"
                | "attributes"
                | "attribute-arrays"
                | "assembly-lookup"
                | "objects"
                | "curves"
                | "fields"
                | "components"
                | "assets"
        );
        let subsystems = matches!(
            boundary,
            "subsystems"
                | "subsystem-map"
                | "imgui-cache"
                | "attributes"
                | "attribute-arrays"
                | "assembly-lookup"
                | "objects"
                | "curves"
                | "fields"
                | "components"
                | "assets"
        );
        let synchronization = matches!(
            boundary,
            "synchronization"
                | "subsystems"
                | "subsystem-map"
                | "imgui-cache"
                | "attributes"
                | "attribute-arrays"
                | "assembly-lookup"
                | "objects"
                | "curves"
                | "fields"
                | "components"
                | "assets"
        );
        let exceptions = matches!(
            boundary,
            "exceptions"
                | "synchronization"
                | "subsystems"
                | "subsystem-map"
                | "imgui-cache"
                | "attributes"
                | "attribute-arrays"
                | "assembly-lookup"
                | "objects"
                | "curves"
                | "fields"
                | "components"
                | "assets"
        );
        let logging = matches!(
            boundary,
            "logging"
                | "exceptions"
                | "synchronization"
                | "subsystems"
                | "subsystem-map"
                | "imgui-cache"
                | "attributes"
                | "attribute-arrays"
                | "assembly-lookup"
                | "objects"
                | "curves"
                | "fields"
                | "components"
                | "assets"
        );
        let classlib = matches!(
            boundary,
            "classlib"
                | "logging"
                | "exceptions"
                | "synchronization"
                | "subsystems"
                | "subsystem-map"
                | "imgui-cache"
                | "attributes"
                | "attribute-arrays"
                | "assembly-lookup"
                | "objects"
                | "curves"
                | "fields"
                | "components"
                | "assets"
        );
        let managed_types = matches!(
            boundary,
            "managed-types"
                | "classlib"
                | "logging"
                | "exceptions"
                | "synchronization"
                | "subsystems"
                | "subsystem-map"
                | "imgui-cache"
                | "attributes"
                | "attribute-arrays"
                | "assembly-lookup"
                | "objects"
                | "curves"
                | "fields"
                | "components"
                | "assets"
        );
        let strings = matches!(
            boundary,
            "strings"
                | "managed-types"
                | "classlib"
                | "logging"
                | "exceptions"
                | "synchronization"
                | "subsystems"
                | "subsystem-map"
                | "imgui-cache"
                | "attributes"
                | "attribute-arrays"
                | "assembly-lookup"
                | "objects"
                | "curves"
                | "fields"
                | "components"
                | "assets"
        );
        let (main_thread, pre_bindings, bindings, profiler, pre_images, images, metadata) = match boundary {
            "root" => (false, false, false, false, false, false, false),
            "main-thread" => (true, false, false, false, false, false, false),
            "pre-bindings" => (true, true, false, false, false, false, false),
            "bindings" => (true, true, true, false, false, false, false),
            "profiler" => (true, true, true, true, false, false, false),
            "pre-images" => (true, true, true, true, true, false, false),
            "images" => (true, true, true, true, true, true, false),
            "metadata" | "strings" | "managed-types" | "classlib" | "logging" | "exceptions" | "synchronization" | "subsystems" | "subsystem-map" | "imgui-cache" | "attributes" | "attribute-arrays" | "assembly-lookup" | "objects" | "curves" | "fields" | "components" | "assets" => (true, true, true, true, true, true, true),
            _ => {
                return Err(
                    "native Mono startup boundary must be root, main-thread, pre-bindings, bindings, profiler, pre-images, images, metadata, strings, managed-types, classlib, logging, exceptions, synchronization, subsystems, subsystem-map, imgui-cache, attributes, attribute-arrays or assembly-lookup or objects or curves or fields or components or assets".into(),
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
        let library = library.canonicalize().map_err(|e| e.to_string())?;
        let (library_hash, support_hash) = runtime_profile(&library)?;
        if classlib
            && !matches!(
                library_hash,
                REMOTING_CANDIDATE
                    | INCREMENTAL_CANDIDATE
                    | ATTRIBUTES_CANDIDATE
                    | OBJECTS_CANDIDATE
            )
        {
            return Err(
                "classlib diagnostic needs fingerprinted remoting field-wrapper runtime".into(),
            );
        }
        if logging
            && !matches!(
                library_hash,
                INCREMENTAL_CANDIDATE | ATTRIBUTES_CANDIDATE | OBJECTS_CANDIDATE
            )
        {
            return Err(
                "logging diagnostic requires verified incremental GC write-barrier runtime".into(),
            );
        }
        if attributes && !matches!(library_hash, ATTRIBUTES_CANDIDATE | OBJECTS_CANDIDATE) {
            return Err("attributes diagnostic requires checked attribute runtime".into());
        }
        if ordinary_objects && library_hash != OBJECTS_CANDIDATE {
            return Err(
                "ordinary object diagnostic requires checked rooted-allocation runtime".into(),
            );
        }
        fingerprint(
            &root.join("AliceToPapaADV_1-0-0_Data/Managed/mscorlib.dll.dylib"),
            support_hash,
            "precompiled corelib support",
        )?;

        Ok(Self {
            library,
            library_hash,
            native: None,
            replaced: false,
            root_ready: false,
            main_thread,
            main_thread_ready: false,
            pre_bindings,
            pre_bindings_ready: false,
            bindings,
            bindings_ready: false,
            scalar_callbacks_verified: 0,
            profiler,
            profiler_ready: false,
            profiler_targets: [0; 3],
            legacy_installed: false,
            gc_events_verified: 0,
            shutdown_callbacks: 0,
            pre_images,
            pre_images_ready: false,
            tls_table: None,
            tls_leaf_verified: false,
            images,
            images_ready: false,
            image_registry: images::Registry::default(),
            image_api: None,
            metadata,
            metadata_ready: false,
            metadata_registry: metadata::Registry::default(),
            strings,
            strings_ready: false,
            object_registry: objects::Registry::default(),
            object_api: None,
            managed_types,
            managed_types_ready: false,
            get_type_method: 0,
            managed_type_invokes: 0,
            reflection_registry: reflection::Registry::default(),
            handle_api: None,
            handle_registry: handles::Registry::default(),
            array_api: None,
            array_registry: arrays::Registry::default(),
            reflection_api: None,
            logging,
            logging_ready: false,
            exceptions,
            exceptions_ready: false,
            assets,
            asset_invokes: 0,
            input_map_invokes: 0,
            input_action_invokes: 0,
            virtual_resolutions: 0,
            serialization_invokes: 0,
            static_invokes: 0,
            static_output_writes: 0,
            instance_invokes: 0,
            instance_output_writes: 0,
            reference_array_api: None,
            reference_array_registry: reference_arrays::Registry::default(),
            components,
            component_invokes: 0,
            fields,
            field_registry: fields::Registry::default(),
            curves,
            curves_constructor: None,
            ordinary_objects,
            ordinary_objects_ready: false,
            instance_api: None,
            instance_registry: instances::Registry::default(),
            assembly_lookup,
            assembly_lookup_ready: false,
            name_api: None,
            name_registry: names::Registry::default(),
            attribute_arrays,
            attribute_arrays_ready: false,
            error_registry: errors::Registry::default(),
            attributes,
            attributes_ready: false,
            attribute_api: None,
            attribute_registry: attributes::Registry::default(),
            imgui,
            imgui_ready: false,
            subsystem_map,
            subsystem_map_ready: false,
            subsystems,
            subsystems_ready: false,
            synchronization,
            synchronization_ready: false,
            classlib,
            classlib_ready: false,
            classlib_invokes: 0,
            registry: bindings::Registry::default(),
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
        runtime: &Runtime64,
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
            self.native = Some(Native::load(&self.library, self.library_hash)?);
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
                if a[0] != 0
                    || (!native.profiler.is_null()
                        && (!self.legacy_installed || native.extra_profilers.len() >= 7))
                {
                    return Err(
                        "startup profiler needs an owned bounded NULL-payload handle".into(),
                    );
                }
                let f = symbol!(
                    lib,
                    "mono_profiler_create",
                    unsafe extern "C" fn(Object) -> Object
                );
                let handle = unsafe { f(ptr::null_mut()) };
                if handle.is_null() {
                    return Err("native startup profiler creation failed".into());
                }
                if native.profiler.is_null() {
                    native.profiler = handle;
                    result = PROFILER_TOKEN;
                } else {
                    native.extra_profilers.push(handle);
                    result = PROFILER_TOKEN + native.extra_profilers.len() as u64 * 256;
                }
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
            "mono_domain_get" if self.root_ready => {
                let current = symbol!(lib, "mono_domain_get", unsafe extern "C" fn() -> Object);
                if unsafe { current() } != native.domain {
                    return Err("native current domain differs from owned root".into());
                }
                result = ROOT_TOKEN;
            }
            "mono_thread_current" if self.root_ready && self.main_thread => {
                if memory.read_int(
                    cpu.gs_base
                        .checked_add(0x48)
                        .ok_or("guest TEB thread ID overflow")?,
                    8,
                )? != 1
                {
                    return Err(
                        "native startup thread query requires guest main thread identity".into(),
                    );
                }
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
            "mono_unity_gc_disable" | "mono_unity_gc_enable" | "mono_unity_gc_is_disabled"
                if self.assets && self.root_ready && self.main_thread_ready =>
            {
                if memory.read_int(
                    cpu.gs_base
                        .checked_add(0x48)
                        .ok_or("GC scope TEB overflow")?,
                    8,
                )? != 1
                {
                    return Err("Unity GC scope requires guest main thread".into());
                }
                result = native.gc.dispatch(lib, function)?;
                self.events.push(format!(
                    "actual owned Unity GC scope: {function}; result={result}"
                ));
            }
            "mono_gc_wbarrier_set_field" if self.root_ready && self.main_thread => {
                if self.assets && self.reference_array_registry.has_reference(a[0], a[1]) {
                    self.reference_array_registry.store(
                        self.reference_array_api
                            .ok_or("array barrier before typed API")?,
                        self.instance_api
                            .ok_or("array barrier before pinned root API")?,
                        &self.instance_registry,
                        memory,
                        [a[0], a[1], a[2]],
                    )?;
                    cpu.return_integer(memory, 0)?;
                    return Ok(false);
                }
                if self.fields && self.field_registry.has_reference(a[0], a[1]) {
                    let api = self
                        .instance_api
                        .ok_or("field barrier before instance API")?;
                    self.field_registry.store_reference(
                        api,
                        &self.instance_registry,
                        memory,
                        a[0],
                        a[1],
                        a[2],
                    )?;
                    cpu.return_integer(memory, 0)?;
                    return Ok(false);
                }
                let owned = if (a[2] == THREAD_TOKEN && !native.thread_handle.is_null())
                    || self.object_registry.contains(a[2])
                    || self.reflection_registry.contains(a[2])
                    || self.array_registry.contains(a[2])
                    || self.attribute_registry.contains(a[2])
                    || self.instance_registry.contains(a[2])
                {
                    a[2]
                } else {
                    0
                };
                if self.assets
                    && a[0] == 0
                    && (a[2] == 0 || a[2] == owned)
                    && a[1].is_multiple_of(8)
                    && (runtime.live_data_slot(a[1], 8) || memory.guest_private_data_slot(a[1], 8))
                    && memory.owned_noncode_access(a[1], 8)
                {
                    memory.write_int(a[1], 8, a[2])?;
                    self.events.push(format!("owned C++ temporary reference slot {:#x}: token={:#x}; no native pointer/barrier destination", a[1], a[2]));
                    cpu.return_integer(memory, 0)?;
                    return Ok(false);
                }
                store_stack_token(memory, cpu, a, owned)?;
                self.events.push(format!("diagnostic strong-rooted object token stored through checked guest stack slot {:#x}; raw guest destination never passed to native barrier", a[1]));
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
                ready = !self.bindings;
            }
            "mono_profiler_install" => {
                if !self.profiler || self.legacy_installed || a[0] != 0 || a[1] != 0x18005fee0 {
                    return Err("legacy profiler context/shutdown target not admitted".into());
                }
                memory.check(a[1], 1, super::super::memory::EXECUTE)?;
                if memory.read_int(a[1], 1)? != 0xc2 || memory.read_int(a[1] + 1, 2)? != 0 {
                    return Err("legacy shutdown target is not exact RET 0".into());
                }
                let install = symbol!(
                    lib,
                    "mono_profiler_install",
                    unsafe extern "C" fn(Object, unsafe extern "C" fn(Object))
                );
                unsafe {
                    install(ptr::null_mut(), callbacks::profiler_shutdown);
                }
                self.profiler_targets[0] = a[1];
                self.legacy_installed = true;
            }
            "mono_profiler_install_gc" => {
                if !self.legacy_installed || a[0] != 0x180789b60 || a[1] != 0x18005fee0 {
                    return Err("legacy GC/resize targets not admitted".into());
                }
                memory.check(a[0], 1, super::super::memory::EXECUTE)?;
                memory.check(a[1], 1, super::super::memory::EXECUTE)?;
                let install = symbol!(
                    lib,
                    "mono_profiler_install_gc",
                    unsafe extern "C" fn(
                        unsafe extern "C" fn(Object, i32, i32),
                        unsafe extern "C" fn(Object, i64),
                    )
                );
                unsafe {
                    install(callbacks::profiler_gc, callbacks::profiler_resize);
                }
                self.profiler_targets[1] = a[0];
                self.profiler_targets[2] = a[1];
            }
            "mono_unity_install_unitytls_interface" => {
                if !self.pre_images || self.tls_table.is_some() {
                    return Err("UnityTLS install outside pre-images boundary".into());
                }
                let table = unitytls::Table::from_guest(memory, a[0])?;
                let install = symbol!(
                    lib,
                    "mono_unity_install_unitytls_interface",
                    unsafe extern "C" fn(*const c_void)
                );
                unsafe {
                    install(table.pointer());
                }
                self.tls_table = Some(table);
                self.events.push("installed owned native UnityTLS header + 33 fixed typed adapters; only error-state leaves executable, other entries fail-stop on activation".into());
            }
            "mono_profiler_set_events" => {
                if !self.legacy_installed || a[0] != 0x100 {
                    return Err("legacy profiler event flags outside actual GC-only profile".into());
                }
                let set = symbol!(lib, "mono_profiler_set_events", unsafe extern "C" fn(i32));
                unsafe {
                    set(a[0] as i32);
                }
            }
            "mono_add_internal_call" if self.pre_bindings_ready && self.bindings => {
                let name = string(memory, a[0])?;
                let native_name = name.clone();
                if let Some(slot) = self.registry.register(memory, name, a[1])? {
                    let add = symbol!(
                        lib,
                        "mono_add_internal_call",
                        unsafe extern "C" fn(*const c_char, *const c_void)
                    );
                    unsafe {
                        add(native_name.as_ptr(), callbacks::adapter(slot));
                    }
                    self.events.push(format!("fixed scalar adapter registered: {native_name:?}; guest PC is private to provider"));
                }
            }
            _ => {
                return Err(format!(
                    "native Mono startup typed export not admitted: {function}"
                ))
            }
        }
        cpu.return_integer(memory, result)?;
        if function != "mono_add_internal_call" {
            self.events
                .push(format!("guest startup boundary {function} -> {result:#x}"));
        }
        Ok(ready)
    }
    pub(super) fn dispatch_worker(
        &mut self,
        function: &str,
        cpu: &mut Cpu64,
        memory: &mut Memory,
        runtime: &mut Runtime64,
    ) -> Result<(), String> {
        let guest = runtime.active_thread();
        // Frozen-source getters only: these read retained immutable image
        // metadata, with no current-domain lookup, lazy setup or managed IL.
        let immutable_image_leaf = matches!(
            function,
            "mono_get_corlib"
                | "mono_class_get_image"
                | "mono_image_get_name"
                | "mono_image_get_table_rows"
        );
        if self.stopped
            || !self.assets
            || !self.main_thread_ready
            || (!workers::State::handles(function)
                && function != "mono_gc_wbarrier_set_field"
                && !matches!(
                    function,
                    "mono_object_new"
                        | "mono_object_get_class"
                        | "mono_array_new"
                        | "mono_array_addr_with_size"
                )
                && function != "mono_runtime_invoke"
                && function != "mono_object_get_virtual_method"
                && !immutable_image_leaf
                && !matches!(
                    function,
                    "mono_gchandle_new_v2"
                        | "mono_gchandle_new_weakref_v2"
                        | "mono_gchandle_get_target_v2"
                        | "mono_gchandle_free_v2"
                )
                && !matches!(
                    function,
                    "mono_custom_attrs_from_class"
                        | "mono_custom_attrs_from_field"
                        | "mono_custom_attrs_has_attr"
                        | "mono_custom_attrs_free"
                )
                && !metadata::worker_handles(function))
        {
            return Err(format!("native Mono worker boundary: guest-thread={guest}; export={function}; args={:#x?}; worker managed IL/domain switching and other callbacks are not admitted", [cpu.regs[1], cpu.regs[2], cpu.regs[8], cpu.regs[9]]));
        }
        if cpu.gs_base != runtime.teb() || memory.read_int(cpu.gs_base + 0x48, 8)? != guest {
            return Err("Mono worker gateway guest TEB identity mismatch".into());
        }
        memory.read_int(cpu.regs[4], 8)?;
        cpu.regs[4]
            .checked_add(8)
            .ok_or("Mono worker return stack overflow")?;
        let targets = self.callback_targets();
        if function == "mono_array_addr_with_size" {
            let address =
                self.reference_array_registry
                    .address(cpu.regs[1], cpu.regs[2], cpu.regs[8])?;
            return cpu.return_integer(memory, address);
        }
        if function == "mono_array_new" {
            if cpu.regs[1] != ROOT_TOKEN {
                return Err("worker array allocation requires owned root domain".into());
            }
            let native = self
                .native
                .as_mut()
                .ok_or("worker array before native runtime")?;
            let api = self
                .reference_array_api
                .ok_or("worker array before element API")?;
            let roots = self.instance_api.ok_or("worker array before root API")?;
            let element = self
                .metadata_registry
                .native(cpu.regs[2], metadata::Kind::Class)?;
            eprintln!(
                "NATIVE_MONO_WORKER_ARRAY: {}; length={}",
                self.metadata_registry
                    .describe_class(native.library, cpu.regs[2])?,
                cpu.regs[8]
            );
            let request = self.reference_array_registry.prepare_worker(
                api,
                roots,
                native.domain,
                element,
                cpu.regs[8],
            )?;
            let (allocated, stats) =
                callbacks::with_worker_registration_owner(memory, runtime, targets, || {
                    native.workers.allocate_array(guest, request)
                })?;
            self.gc_events_verified |= stats.gc_events;
            let (adopted, _) =
                callbacks::with_worker_registration_owner(memory, runtime, targets, || {
                    self.reference_array_registry.adopt_worker(
                        api,
                        roots,
                        &mut self.instance_registry,
                        allocated?,
                    )
                })?;
            let token = adopted?;
            self.reference_array_registry.publish(memory, token)?;
            return cpu.return_integer(memory, token);
        }
        if function == "mono_object_get_virtual_method" {
            let api = self
                .instance_api
                .ok_or("worker virtual resolution before roots")?;
            let method = self
                .metadata_registry
                .native(cpu.regs[2], metadata::Kind::Method)?;
            let native = self
                .native
                .as_mut()
                .ok_or("worker virtual resolution before runtime")?;
            let library = native.library;
            let (result, stats) =
                callbacks::with_worker_registration_owner(memory, runtime, targets, || {
                    let object = self.instance_registry.native(api, cpu.regs[1])?;
                    let class = self.instance_registry.class(api, cpu.regs[1])?;
                    let request = virtual_methods::Api::load(library)?
                        .prepare_worker(api, object, class, method)?;
                    native.workers.resolve_virtual(guest, request)
                })?;
            self.gc_events_verified |= stats.gc_events;
            let token = self.metadata_registry.wrap(
                memory,
                metadata::Value::Handle(result?.method(), metadata::Kind::Method),
            )?;
            self.virtual_resolutions += 1;
            return cpu.return_integer(memory, token);
        }
        if function == "mono_runtime_invoke" {
            let a = [cpu.regs[1], cpu.regs[2], cpu.regs[8], cpu.regs[9]];
            if a[1] == 0 {
                return Err("worker instance entry requires rooted receiver".into());
            }
            let (stack_base, stack_end) = runtime.native_worker_stack_bounds(memory, cpu)?;
            if a[3] != 0 {
                let end = a[3]
                    .checked_add(8)
                    .ok_or("worker exception slot overflow")?;
                if !a[3].is_multiple_of(8)
                    || !memory.owned_noncode_access(a[3], 8)
                    || !((a[3] >= cpu.regs[4].saturating_add(8)
                        && a[3] >= stack_base
                        && end <= stack_end)
                        || runtime.live_data_slot(a[3], 8)
                        || memory.guest_private_data_slot(a[3], 8))
                {
                    return Err("worker exception output requires a live owned C++ slot".into());
                }
                memory.check(a[3], 8, super::super::memory::WRITE)?;
            }
            let library = self
                .native
                .as_ref()
                .ok_or("worker invoke before runtime")?
                .library;
            let api = self.instance_api.ok_or("worker invoke before root API")?;
            let method = self
                .metadata_registry
                .native(a[0], metadata::Kind::Method)?;
            let images = self.constructor_images()?;
            eprintln!(
                "NATIVE_MONO_WORKER_INVOKE: {}",
                self.metadata_registry.describe_method(library, a[0])?
            );
            let (prepared, _) =
                callbacks::with_worker_registration_owner(memory, runtime, targets, || {
                    callbacks::with_service(|memory, _| {
                        self.instance_registry.sync_from_guest(api, memory)?;
                        self.field_registry.sync(api, &self.instance_registry)?;
                        if let Some(array_api) = self.reference_array_api {
                            self.reference_array_registry.sync(
                                array_api,
                                api,
                                &self.instance_registry,
                                memory,
                            )?;
                        }
                        Ok::<_, String>(())
                    })?;
                    let object = self.instance_registry.native(api, a[1])?;
                    let class = self.instance_registry.class(api, a[1])?;
                    let request = constructors::Api::load(library)?
                        .prepare_worker(library, method, &images, object, class)?;
                    // Zero parameters are proved by actual method metadata,
                    // not by the caller's params pointer. Unity may supply an
                    // unused non-NULL scratch vector even for a void/no-arg IL
                    // callback. It is never dereferenced or forwarded here.
                    Ok::<_, String>((request, object, class))
                })?;
            let (request, object, class) = prepared?;
            let native = self.native.as_mut().ok_or("worker invoke lost runtime")?;
            let (result, stats) =
                callbacks::with_worker_registration_owner(memory, runtime, targets, || {
                    native.workers.invoke(guest, request)
                })?;
            self.gc_events_verified |= stats.gc_events;
            result?;
            let (refreshed, _) =
                callbacks::with_worker_registration_owner(memory, runtime, targets, || {
                    self.refresh_managed_graph(library, api)?;
                    if !self.field_registry.has_owner(a[1]) {
                        let specs =
                            field_layouts::specs(library, images[0], &images, object, class)?;
                        self.install_fields(library, api, specs)?;
                    }
                    let references = self.field_registry.native_references(
                        api,
                        &self.instance_registry,
                        a[1],
                    )?;
                    self.publish_container_graph(library, api, references)
                })?;
            refreshed?;
            if a[3] != 0 {
                memory.write_int(a[3], 8, 0)?;
            }
            self.component_invokes += 1;
            self.events.push(format!("worker {guest} actual metadata-qualified zero-argument instance IL completed; receiver-root and graph refreshed"));
            return cpu.return_integer(memory, 0);
        }
        if matches!(
            function,
            "mono_gchandle_new_v2"
                | "mono_gchandle_new_weakref_v2"
                | "mono_gchandle_get_target_v2"
                | "mono_gchandle_free_v2"
        ) {
            let native = self
                .native
                .as_mut()
                .ok_or("worker handles before runtime")?;
            let library = native.library;
            let (domain, stats) =
                callbacks::with_worker_registration_owner(memory, runtime, targets, || {
                    native
                        .workers
                        .dispatch(library, native.domain, guest, "mono_domain_get", 0)
                })?;
            self.gc_events_verified |= stats.gc_events;
            if domain? != ROOT_TOKEN {
                return Err("worker handles before actual root attachment".into());
            }
            return self.dispatch_handles(function, cpu, memory, runtime, library, true);
        }
        let allocation = if function == "mono_object_new" {
            if cpu.regs[1] != ROOT_TOKEN {
                return Err("worker allocation requires owned root domain".into());
            }
            let library = self
                .native
                .as_ref()
                .ok_or("worker allocation before runtime")?
                .library;
            let class = self
                .metadata_registry
                .native(cpu.regs[2], metadata::Kind::Class)?;
            let api = match self.instance_api {
                Some(api) => api,
                None => {
                    let api = instances::Api::load(
                        library,
                        self.image_registry.unique_native_image(CORE_MODULE)?,
                    )?;
                    self.instance_api = Some(api);
                    api
                }
            };
            let value = symbol!(
                library,
                "mono_class_is_valuetype",
                unsafe extern "C" fn(Object) -> i32
            );
            let boxed = if unsafe { value(class) } != 0 {
                Some(self.boxed_layout(library, class)?.width)
            } else {
                None
            };
            Some((api, class, boxed))
        } else {
            None
        };
        let native = self
            .native
            .as_mut()
            .ok_or("Mono worker gateway before runtime")?;
        if native.owner != thread::current().id() || native.closed || native.domain.is_null() {
            return Err("Mono worker gateway outside live native process owner".into());
        }
        if let Some((api, class, boxed)) = allocation {
            let request =
                self.instance_registry
                    .prepare_worker(api, native.domain, class, boxed)?;
            let (result, stats) =
                callbacks::with_worker_registration_owner(memory, runtime, targets, || {
                    request.run(|request| native.workers.allocate(guest, request))
                })?;
            self.gc_events_verified |= stats.gc_events;
            let token = result?;
            self.instance_registry.publish(memory, token)?;
            self.events.push(format!("worker {guest} actual checked allocation -> {token:#x}; strong root transferred to process owner"));
            return cpu.return_integer(memory, token);
        }
        if function == "mono_object_get_class" {
            let (domain, stats) =
                callbacks::with_worker_registration_owner(memory, runtime, targets, || {
                    native.workers.dispatch(
                        native.library,
                        native.domain,
                        guest,
                        "mono_domain_get",
                        0,
                    )
                })?;
            self.gc_events_verified |= stats.gc_events;
            if domain? != ROOT_TOKEN {
                return Err("worker object class query before actual attachment".into());
            }
            let class = self.instance_registry.class(
                self.instance_api
                    .ok_or("worker object class query has no root API")?,
                cpu.regs[1],
            )?;
            let token = self.metadata_registry.wrap(
                memory,
                metadata::Value::Handle(class, metadata::Kind::Class),
            )?;
            self.metadata_registry
                .publish_class_elements(native.library, memory, token)?;
            return cpu.return_integer(memory, token);
        }
        if matches!(
            function,
            "mono_custom_attrs_from_class"
                | "mono_custom_attrs_from_field"
                | "mono_custom_attrs_has_attr"
                | "mono_custom_attrs_free"
        ) {
            let a = [cpu.regs[1], cpu.regs[2]];
            let api = match self.attribute_api {
                Some(api) => api,
                None => {
                    let api = attributes::Api::load(native.library)?;
                    self.attribute_api = Some(api);
                    api
                }
            };
            let class = if function == "mono_custom_attrs_free" {
                ptr::null_mut()
            } else {
                self.metadata_registry.native(
                    if function == "mono_custom_attrs_has_attr" {
                        a[1]
                    } else {
                        a[0]
                    },
                    metadata::Kind::Class,
                )?
            };
            let field = if function == "mono_custom_attrs_from_field" {
                Some(self.metadata_registry.native(a[1], metadata::Kind::Field)?)
            } else {
                None
            };
            let request = self
                .attribute_registry
                .prepare_worker(api, function, a[0], class, field)?;
            let (result, stats) =
                callbacks::with_worker_registration_owner(memory, runtime, targets, || {
                    request.run(|request| native.workers.attributes(guest, request))
                })?;
            self.gc_events_verified |= stats.gc_events;
            return cpu.return_integer(memory, result?);
        }
        if metadata::worker_handles(function) && !metadata::thread_neutral_leaf(function) {
            let a = [cpu.regs[1], cpu.regs[2], cpu.regs[8]];
            let is_iterator = matches!(
                function,
                "mono_class_get_fields"
                    | "mono_class_get_methods"
                    | "mono_class_get_interfaces"
                    | "mono_class_get_nested_types"
                    | "mono_signature_get_params"
            );
            let iterator = if is_iterator {
                // Native C API receives a private local iterator, never this
                // writable guest address. Validate output before dispatch.
                memory.check(
                    a[1],
                    8,
                    super::super::memory::READ | super::super::memory::WRITE,
                )?;
                Some(memory.read_int(a[1], 8)?)
            } else {
                None
            };
            let request =
                self.metadata_registry
                    .prepare_worker(native.library, function, a, iterator)?;
            let (value, stats) =
                callbacks::with_worker_registration_owner(memory, runtime, targets, || {
                    request.run(|request| native.workers.metadata(guest, request))
                })?;
            self.gc_events_verified |= stats.gc_events;
            let (value, iterator) = value?;
            let class = matches!(value, metadata::Value::Handle(_, metadata::Kind::Class));
            let result = self.metadata_registry.wrap(memory, value)?;
            if class && result != 0 {
                self.metadata_registry
                    .publish_class_elements(native.library, memory, result)?;
            }
            if let Some(token) = iterator {
                memory.write_int(a[1], 8, token)?;
            }
            if self.metadata_registry.count() <= 64 {
                self.events.push(format!(
                    "worker {guest} actual native metadata {function} -> {result:#x}"
                ));
            }
            return cpu.return_integer(memory, result);
        }
        if metadata::thread_neutral_leaf(function) || immutable_image_leaf {
            let a = [cpu.regs[1], cpu.regs[2], cpu.regs[8]];
            let (domain, stats) =
                callbacks::with_worker_registration_owner(memory, runtime, targets, || {
                    native.workers.dispatch(
                        native.library,
                        native.domain,
                        guest,
                        "mono_domain_get",
                        0,
                    )
                })?;
            self.gc_events_verified |= stats.gc_events;
            if domain? != ROOT_TOKEN {
                return Err("worker metadata leaf requires its actual attached root domain".into());
            }
            let (value, stats) =
                callbacks::with_worker_registration_owner(memory, runtime, targets, || {
                    if function == "mono_get_corlib" {
                        let get = symbol!(
                            native.library,
                            "mono_get_corlib",
                            unsafe extern "C" fn() -> Object
                        );
                        let image = unsafe { get() };
                        if image != self.image_registry.unique_native_image(CORELIB)? {
                            return Err(
                                "worker corlib getter differs from retained fingerprinted image"
                                    .into(),
                            );
                        }
                        self.metadata_registry.note_query();
                        Ok(metadata::Value::Scalar(
                            self.image_registry.borrow_registered(image)?,
                        ))
                    } else if function == "mono_class_get_image" {
                        let class = self.metadata_registry.native(a[0], metadata::Kind::Class)?;
                        let get = symbol!(
                            native.library,
                            "mono_class_get_image",
                            unsafe extern "C" fn(Object) -> Object
                        );
                        let image = self
                            .image_registry
                            .borrow_registered(unsafe { get(class) })?;
                        self.metadata_registry.note_query();
                        Ok(metadata::Value::Scalar(image))
                    } else if immutable_image_leaf {
                        let image = self.image_registry.native_image(a[0])?;
                        self.metadata_registry
                            .query_image(native.library, function, image, a[1])
                    } else {
                        self.metadata_registry
                            .query(native.library, function, a[0], a[1], a[2])
                    }
                })?;
            self.gc_events_verified |= stats.gc_events;
            let value = value?;
            let class = matches!(value, metadata::Value::Handle(_, metadata::Kind::Class));
            let result = self.metadata_registry.wrap(memory, value)?;
            if class && result != 0 {
                self.metadata_registry
                    .publish_class_elements(native.library, memory, result)?;
            }
            self.events.push(format!("worker {guest} immutable metadata leaf {function} served by process owner: {result:#x}; actual worker TLS retained"));
            return cpu.return_integer(memory, result);
        }
        if function == "mono_gc_wbarrier_set_field" {
            let a = [cpu.regs[1], cpu.regs[2], cpu.regs[8], cpu.regs[9]];
            let array = self.reference_array_registry.has_reference(a[0], a[1]);
            let field = self.field_registry.has_reference(a[0], a[1]);
            if array || field {
                let api = self
                    .instance_api
                    .ok_or("worker managed reference before roots")?;
                let (domain, stats) =
                    callbacks::with_worker_registration_owner(memory, runtime, targets, || {
                        native.workers.dispatch(
                            native.library,
                            native.domain,
                            guest,
                            "mono_domain_get",
                            0,
                        )
                    })?;
                self.gc_events_verified |= stats.gc_events;
                if domain? != ROOT_TOKEN {
                    return Err("worker managed reference before actual root attachment".into());
                }
                // Only an exact declared reference cell may reach the existing
                // canonical-root barrier. NULL owner is accepted by that API;
                // it recovers the real native owner from the registered cell.
                let (stored, _) =
                    callbacks::with_worker_registration_owner(memory, runtime, targets, || {
                        callbacks::with_service(|memory, _| {
                            if array {
                                self.reference_array_registry.store(
                                    self.reference_array_api
                                        .ok_or("worker array reference before typed API")?,
                                    api,
                                    &self.instance_registry,
                                    memory,
                                    [a[0], a[1], a[2]],
                                )
                            } else {
                                self.field_registry.store_reference(
                                    api,
                                    &self.instance_registry,
                                    memory,
                                    a[0],
                                    a[1],
                                    a[2],
                                )
                            }
                        })
                    })?;
                stored?;
                return cpu.return_integer(memory, 0);
            }
            let (stack_base, stack_end) = runtime.native_worker_stack_bounds(memory, cpu)?;
            let end = a[1]
                .checked_add(8)
                .ok_or("worker reference slot overflow")?;
            let first = cpu.regs[4]
                .checked_add(40)
                .ok_or("worker reference stack overflow")?;
            if a[0] != 0
                || !a[1].is_multiple_of(8)
                || !((a[1] >= first && a[1] >= stack_base && end <= stack_end)
                    || runtime.live_data_slot(a[1], 8)
                    || memory.guest_private_data_slot(a[1], 8))
                || !memory.owned_noncode_access(a[1], 8)
            {
                return Err(format!(
                    "worker Thread reference requires a live owned C++ slot: args={a:#x?}"
                ));
            }
            memory.check(a[1], 8, super::super::memory::WRITE)?;
            if a[2] != 0 {
                let (current, stats) =
                    callbacks::with_worker_registration_owner(memory, runtime, targets, || {
                        native.workers.dispatch(
                            native.library,
                            native.domain,
                            guest,
                            "mono_thread_current",
                            0,
                        )
                    })?;
                self.gc_events_verified |= stats.gc_events;
                if current? != a[2] {
                    let api = self
                        .instance_api
                        .ok_or("worker object reference has no root API")?;
                    self.instance_registry.native(api, a[2])?;
                }
            }
            memory.write_int(a[1], 8, a[2])?;
            self.events.push(format!("worker {guest} stored an actual strong object/Thread token {:#x} in checked C++ slot {:#x}; no native pointer/barrier destination", a[2], a[1]));
            return cpu.return_integer(memory, 0);
        }
        let (result, stats) =
            callbacks::with_worker_registration_owner(memory, runtime, targets, || {
                native
                    .workers
                    .dispatch(native.library, native.domain, guest, function, cpu.regs[1])
            })?;
        self.gc_events_verified |= stats.gc_events;
        cpu.return_integer(memory, result?)
    }
    /// Frozen BDWGC v2 handles use process-wide locked tables. Non-NULL
    /// targets carry their own domain; these calls do not execute managed IL or
    /// change worker TLS. Serialize canonical publication on the process owner.
    fn dispatch_handles(
        &mut self,
        function: &str,
        cpu: &mut Cpu64,
        memory: &mut Memory,
        runtime: &mut Runtime64,
        library: Object,
        worker: bool,
    ) -> Result<(), String> {
        let a = [cpu.regs[1], cpu.regs[2]];
        let api = match self.handle_api {
            Some(api) => api,
            None => {
                let api = handles::Api::load(library)?;
                self.handle_api = Some(api);
                api
            }
        };
        if function == "mono_gchandle_free_v2" {
            self.handle_registry.retire_view(memory, a[0])?;
        }
        let targets = self.callback_targets();
        let operation = || match function {
            "mono_gchandle_new_v2" | "mono_gchandle_new_weakref_v2" => {
                let object = if self.array_registry.contains(a[0]) {
                    self.array_registry
                        .native(self.array_api.ok_or("empty array API absent")?, a[0])?
                } else if self.object_registry.contains(a[0]) {
                    self.object_registry
                        .native_string(self.object_api.ok_or("String API absent")?, a[0])?
                } else if self.reflection_registry.contains(a[0]) {
                    self.reflection_registry
                        .native(self.reflection_api.ok_or("reflection API absent")?, a[0])?
                } else if self.instance_registry.contains(a[0]) {
                    self.instance_registry
                        .native(self.instance_api.ok_or("ordinary object API absent")?, a[0])?
                } else if self.attribute_registry.contains(a[0]) {
                    self.attribute_registry
                        .native(self.attribute_api.ok_or("attribute API absent")?, a[0])?
                } else {
                    return Err(
                        "external strong root requires a live owned managed object token".into(),
                    );
                };
                if function == "mono_gchandle_new_weakref_v2" {
                    self.handle_registry
                        .new_weak_handle(api, object, a[0], a[1])
                } else {
                    self.handle_registry.new_handle(api, object, a[0], a[1])
                }
            }
            "mono_gchandle_get_target_v2" => self.handle_registry.target(api, a[0]),
            "mono_gchandle_free_v2" => self.handle_registry.free(api, a[0]).map(|()| 0),
            _ => Err("unknown external handle operation".into()),
        };
        let (result, _) = if worker {
            callbacks::with_worker_registration_owner(memory, runtime, targets, operation)?
        } else {
            callbacks::with_owner(memory, runtime, targets, operation)?
        };
        let result = result?;
        if matches!(
            function,
            "mono_gchandle_new_v2" | "mono_gchandle_new_weakref_v2"
        ) {
            self.handle_registry.publish(memory, result)?;
        }
        cpu.return_integer(memory, result)
    }
    fn callback_targets(&self) -> [u64; 7] {
        let mut targets = [0; 7];
        targets[..2].copy_from_slice(&self.registry.scalar_targets);
        targets[2..5].copy_from_slice(&self.profiler_targets);
        if let Some(table) = &self.tls_table {
            targets[5..].copy_from_slice(&table.leaf_targets());
        }
        targets
    }
    fn constructor_images(&self) -> Result<Vec<Object>, String> {
        let mut images = vec![
            self.image_registry.unique_native_image(CORE_MODULE)?,
            self.image_registry
                .unique_native_image(animation::RENDER_CORE)?,
            self.image_registry.unique_native_image(animation::URP)?,
        ];
        if self.assets {
            images.push(self.image_registry.unique_native_image(INPUT_SYSTEM)?);
            images.push(self.image_registry.unique_native_image(URP_2D)?);
            for image in self.image_registry.live_native_images()? {
                if !images.contains(&image) {
                    images.push(image);
                }
            }
        }
        Ok(images)
    }
    fn boxed_layout(&self, library: Object, class: Object) -> Result<value_layout::Layout, String> {
        let image = symbol!(
            library,
            "mono_class_get_image",
            unsafe extern "C" fn(Object) -> Object
        );
        let image = unsafe { image(class) };
        if !self.reference_images()?.contains(&image) {
            return Err("boxed value outside fingerprinted image family".into());
        }
        value_layout::Api::load(library, image)?.layout(class)
    }
    fn reference_images(&self) -> Result<[Object; 6], String> {
        Ok([
            self.image_registry.unique_native_image(CORE_MODULE)?,
            self.image_registry
                .unique_native_image(animation::RENDER_CORE)?,
            self.image_registry.unique_native_image(animation::URP)?,
            self.image_registry.unique_native_image(INPUT_SYSTEM)?,
            self.image_registry.unique_native_image(CORELIB)?,
            self.image_registry.unique_native_image(URP_2D)?,
        ])
    }
    fn install_fields(
        &mut self,
        library: Object,
        api: instances::Api,
        specs: Vec<fields::Spec>,
    ) -> Result<(), String> {
        let strings = self
            .object_api
            .ok_or("field references before canonical strings")?;
        callbacks::with_service(|memory, _| {
            self.field_registry.install_with(
                library,
                api,
                &mut self.instance_registry,
                memory,
                specs,
                |roots, native| {
                    self.object_registry
                        .borrow_reference(strings, api, roots, native)
                },
            )?;
            self.object_registry.publish_pending(memory)
        })
    }
    /// Managed IL may mutate any previously published child or typed array.
    /// Both registries validate their complete batches before either commits.
    fn refresh_managed_graph(
        &mut self,
        library: Object,
        api: instances::Api,
    ) -> Result<(), String> {
        let strings = self
            .object_api
            .ok_or("graph refresh before canonical strings")?;
        let fields = self.field_registry.prepare_refresh(
            api,
            &mut self.instance_registry,
            None,
            |roots, native| {
                self.object_registry
                    .borrow_reference(strings, api, roots, native)
            },
        )?;
        let arrays = if let Some(array_api) = self.reference_array_api {
            Some(self.reference_array_registry.prepare_refresh(
                array_api,
                api,
                &mut self.instance_registry,
                |roots, native| {
                    self.object_registry
                        .borrow_reference(strings, api, roots, native)
                },
            )?)
        } else {
            None
        };
        let mut references = fields.references.clone();
        if let Some(arrays) = &arrays {
            references.extend_from_slice(&arrays.references);
        }
        callbacks::with_service(|memory, _| self.object_registry.publish_pending(memory))?;
        self.field_registry.commit_refresh(fields)?;
        if let Some(arrays) = arrays {
            self.reference_array_registry.commit_refresh(arrays)?;
        }
        if self.assets && !references.is_empty() {
            self.publish_container_graph(library, api, references)?;
        }
        Ok(())
    }
    /// Publish bounded backing storage reachable from an admitted managed graph.
    /// Resolved generic reference classes in frozen corlib use actual metadata;
    /// this never admits their methods or exposes native headers.
    fn publish_container_graph(
        &mut self,
        library: Object,
        api: instances::Api,
        mut queue: Vec<Object>,
    ) -> Result<(), String> {
        let core = self.image_registry.unique_native_image(CORE_MODULE)?;
        let corlib = self.image_registry.unique_native_image(CORELIB)?;
        let object_class = symbol!(
            library,
            "mono_object_get_class",
            unsafe extern "C" fn(Object) -> Object
        );
        let rank = symbol!(
            library,
            "mono_class_get_rank",
            unsafe extern "C" fn(Object) -> u32
        );
        let find = symbol!(
            library,
            "mono_class_from_name",
            unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object
        );
        let assignable = symbol!(
            library,
            "mono_class_is_assignable_from",
            unsafe extern "C" fn(Object, Object) -> i32
        );
        let unity = unsafe { find(core, c"UnityEngine".as_ptr(), c"Object".as_ptr()) };
        if unity.is_null() {
            return Err("backing graph lacks owned Unity object declaration".into());
        }
        let mut seen = Vec::new();
        while let Some(object) = queue.pop() {
            if object.is_null() || seen.contains(&object) {
                continue;
            }
            if seen.len() >= 256 {
                return Err("container backing graph exceeds bounded capacity".into());
            }
            seen.push(object);
            let class = unsafe { object_class(object) };
            if unsafe { rank(class) } != 0 {
                let token = self.instance_registry.borrow(api, object)?;
                if self.array_registry.contains(token) {
                    continue;
                }
                let array_api = self
                    .reference_array_api
                    .ok_or("container array before typed array API")?;
                let strings = self
                    .object_api
                    .ok_or("array references before canonical strings")?;
                let token = self.reference_array_registry.borrow_with(
                    array_api,
                    api,
                    &mut self.instance_registry,
                    object,
                    |roots, native| {
                        self.object_registry
                            .borrow_reference(strings, api, roots, native)
                    },
                )?;
                callbacks::with_service(|memory, _| {
                    self.object_registry.publish_pending(memory)?;
                    self.reference_array_registry.publish(memory, token)
                })?;
                queue.extend(self.reference_array_registry.native_references(
                    array_api,
                    api,
                    &self.instance_registry,
                    token,
                )?);
                continue;
            }
            let image_of = symbol!(
                library,
                "mono_class_get_image",
                unsafe extern "C" fn(Object) -> Object
            );
            let owner = self.instance_registry.borrow(api, object)?;
            // Engine-object fields are installed by the admitted constructor /
            // binding path. Discovering a reference does not authorize early
            // layout publication or replace engine lifetime validation.
            if unsafe { assignable(unity, class) } != 0 && !self.field_registry.has_owner(owner) {
                continue;
            }
            let specs = if let Some(specs) =
                field_layouts::storage_specs(library, core, corlib, object, class)?
            {
                specs
            } else if self
                .constructor_images()?
                .contains(&unsafe { image_of(class) })
            {
                field_layouts::specs(library, core, &self.constructor_images()?, object, class)?
            } else {
                continue;
            };
            if !self.field_registry.has_owner(owner) {
                self.install_fields(library, api, specs)?;
            }
            queue.extend(self.field_registry.native_references(
                api,
                &self.instance_registry,
                owner,
            )?);
        }
        Ok(())
    }
    pub(super) fn stop_before(&mut self, function: &str, cpu: &Cpu64) -> Result<bool, String> {
        if self.bindings && function == "mono_profiler_install" {
            self.registry.seal()?;
            self.bindings_ready = true;
            self.events.push(format!("provider registered {} actual InternalCall names; 2 fixed scalar adapters; other entries deferred", self.registry.len()));
            return Ok(!self.profiler);
        }
        if self.profiler && function == "mono_unity_install_unitytls_interface" {
            if !self.legacy_installed || self.profiler_targets.contains(&0) {
                return Err("legacy profiler callback registration incomplete".into());
            }
            self.profiler_ready = true;
            self.events.push(
                "actual legacy profiler registration complete; stop before UnityTLS installation"
                    .into(),
            );
            return Ok(!self.pre_images);
        }
        if self.pre_images && function == "mono_image_open_from_data_with_name" {
            if self.tls_table.is_none() {
                return Err(
                    "managed image boundary reached without owned UnityTLS interface".into(),
                );
            }
            self.pre_images_ready = true;
            self.events.push(
                "actual Unity reached managed-image loading; stop before reading first image"
                    .into(),
            );
            return Ok(!self.images);
        }
        if self.images && function == "mono_class_from_name" && self.image_registry.completed() > 0
        {
            self.images_ready = true;
            self.events.push("actual Unity completed native image/assembly ownership cycles; stop before first class metadata query".into());
            return Ok(!self.metadata);
        }
        if self.ordinary_objects && self.main_thread_ready && function == "mono_thread_current" {
            return Ok(false);
        }
        if self.assets && self.asset_invokes > 0 && function == "mono_array_new" && cpu.regs[8] != 0
        {
            let element = self
                .metadata_registry
                .native(cpu.regs[2], metadata::Kind::Class)?;
            if self
                .reference_array_api
                .is_some_and(|api| api.supports(element))
            {
                return Ok(false);
            }
            self.mark_completed_layers();
            self.events.push("actual input asset reached unadmitted nonempty element allocation; stop before copying unknown value/reference layout".into());
            return Ok(true);
        }
        if self.components && self.curves_constructor.is_some() && function == "mono_runtime_invoke"
        {
            let method = self
                .metadata_registry
                .native(cpu.regs[1], metadata::Kind::Method)?;
            let admission = constructors::Api::load(
                self.native
                    .as_ref()
                    .ok_or("constructor before runtime")?
                    .library,
            )?
            .describe(method, &self.constructor_images()?);
            match admission {
                Ok(Some(_)) => return Ok(false),
                Ok(None) => {
                    if self.assets {
                        let library = self
                            .native
                            .as_ref()
                            .ok_or("typed managed call before runtime")?
                            .library;
                        match managed_calls::Api::load(library)?.describe(
                            method,
                            &self.image_registry.live_native_images()?,
                            &self.image_registry.live_native_images()?,
                        ) {
                            Ok(Some(_)) => return Ok(false),
                            Ok(None) => {}
                            Err(reason) => {
                                self.mark_completed_layers();
                                self.events.push(format!(
                                    "typed managed entry deferred before invocation: {reason}"
                                ));
                                return Ok(true);
                            }
                        }
                    }
                }
                Err(reason) => {
                    self.mark_completed_layers();
                    self.events.push(format!(
                        "managed entry deferred before invocation: {reason}"
                    ));
                    return Ok(true);
                }
            }
        }
        if self.curves
            && self.curves_constructor.is_none()
            && self.classlib_invokes >= self.startup_limit()
            && function == "mono_runtime_invoke"
        {
            return Ok(false);
        }
        if self.assets
            && matches!(
                function,
                "mono_unity_gc_disable" | "mono_unity_gc_enable" | "mono_unity_gc_is_disabled"
            )
        {
            return Ok(false);
        }
        if self.metadata
            && self.metadata_registry.count() > 0
            && !metadata::handles(function)
            && !images::handles(function)
            && !matches!(function, "mono_gc_wbarrier_set_field" | "mono_domain_get")
        {
            self.metadata_ready = true;
            self.events.push(format!("actual typed metadata queries complete; stop before next unadmitted operation {function}"));
            if !self.strings {
                return Ok(true);
            }
            if !self.handles_object(function) {
                self.strings_ready = self.object_registry.count() > 0;
                if self.managed_types
                    && function == "mono_runtime_invoke"
                    && (self.get_type_method == 0
                        || self.get_type_method == cpu.regs[1]
                        || (self.classlib && self.classlib_invokes < self.startup_limit()))
                {
                    return Ok(false);
                }
                self.mark_completed_layers();
                return Ok(true);
            }
        }
        Ok(false)
    }
    fn mark_completed_layers(&mut self) {
        self.managed_types_ready = self.managed_types && self.managed_type_invokes > 0;
        self.classlib_ready = self.classlib && self.classlib_invokes > 0;
        self.logging_ready = self.logging && self.classlib_invokes >= 2;
        self.exceptions_ready = self.exceptions && self.classlib_invokes >= 3;
        self.synchronization_ready = self.synchronization && self.classlib_invokes >= 5;
        self.subsystems_ready = self.subsystems && self.classlib_invokes >= 6;
        self.subsystem_map_ready = self.subsystem_map && self.classlib_invokes >= 7;
        self.imgui_ready = self.imgui && self.classlib_invokes >= 8;
        self.attributes_ready = self.attributes && self.attribute_registry.ready();
        self.attribute_arrays_ready = self.attribute_arrays
            && self.attribute_registry.arrays_ready()
            && self.error_registry.ready();
        self.assembly_lookup_ready = self.assembly_lookup && self.name_registry.ready();
        self.ordinary_objects_ready = self.ordinary_objects && self.instance_registry.count() > 0;
    }
    fn startup_limit(&self) -> usize {
        if self.imgui {
            8
        } else if self.subsystem_map {
            7
        } else if self.subsystems {
            6
        } else if self.synchronization {
            5
        } else if self.exceptions {
            3
        } else if self.logging {
            2
        } else {
            1
        }
    }
    pub(super) fn handles_object(&self, function: &str) -> bool {
        self.strings
            && ((self.assets
                && matches!(
                    function,
                    "mono_object_get_virtual_method"
                        | "mono_object_isinst"
                        | "mono_array_addr_with_size"
                        | "mono_object_unbox"
                ))
                || (self.assets
                    && matches!(
                        function,
                        "mono_string_new_wrapper" | "mono_unity_string_empty_wrapper"
                    ))
                || (self.ordinary_objects && function == "mono_object_new")
                || (self.assembly_lookup && names::handles(function))
                || (self.attribute_arrays && errors::handles(function))
                || (self.attributes
                    && attributes::handles(function)
                    && (function != "mono_unity_custom_attrs_construct" || self.attribute_arrays))
                || matches!(
                    function,
                    "mono_string_new_len" | "mono_string_length" | "mono_string_chars"
                )
                || (self.managed_types
                    && matches!(
                        function,
                        "mono_reflection_type_get_type" | "mono_type_get_object"
                    ))
                || (self.synchronization
                    && matches!(
                        function,
                        "mono_array_new"
                            | "mono_unity_loader_get_last_error_and_error_prepare_exception"
                            | "mono_gchandle_new_v2"
                            | "mono_gchandle_new_weakref_v2"
                            | "mono_gchandle_get_target_v2"
                            | "mono_gchandle_free_v2"
                    )))
    }
    fn dispatch_attributes(
        &mut self,
        function: &str,
        cpu: &mut Cpu64,
        memory: &mut Memory,
        runtime: &mut Runtime64,
        library: Object,
    ) -> Result<bool, String> {
        let api = match self.attribute_api {
            Some(api) => api,
            None => {
                let api = attributes::Api::load(library)?;
                self.attribute_api = Some(api);
                api
            }
        };
        let a = [cpu.regs[1], cpu.regs[2], cpu.regs[8], cpu.regs[9]];
        let targets = self.callback_targets();
        let result = match function {
            "mono_unity_custom_attrs_construct" => {
                self.error_registry.prepare(memory, cpu, a[1])?;
                let (result, _) = callbacks::with_owner(memory, runtime, targets, || {
                    self.attribute_registry.construct(api, a[0])
                })?;
                let (token, status) = result?;
                self.error_registry.complete(memory, cpu, a[1], status)?;
                if token != 0 {
                    self.attribute_registry.publish_array(memory, token)?;
                }
                token
            }
            "mono_custom_attrs_from_class" | "mono_custom_attrs_from_field" => {
                let class = self.metadata_registry.native(a[0], metadata::Kind::Class)?;
                let field = if function == "mono_custom_attrs_from_field" {
                    Some(self.metadata_registry.native(a[1], metadata::Kind::Field)?)
                } else {
                    None
                };
                let (value, _) = callbacks::with_owner(memory, runtime, targets, || {
                    self.attribute_registry.query(api, class, field)
                })?;
                value?
            }
            "mono_custom_attrs_has_attr" | "mono_custom_attrs_get_attr" => {
                let class = self.metadata_registry.native(a[1], metadata::Kind::Class)?;
                let (value, _) = callbacks::with_owner(memory, runtime, targets, || {
                    if function == "mono_custom_attrs_has_attr" {
                        self.attribute_registry.has(api, a[0], class)
                    } else {
                        self.attribute_registry.get(api, a[0], class)
                    }
                })?;
                value?
            }
            "mono_custom_attrs_free" => {
                let (value, _) = callbacks::with_owner(memory, runtime, targets, || {
                    self.attribute_registry.free(api, a[0])
                })?;
                value?;
                0
            }
            "mono_object_get_class" => {
                let (class, _) = callbacks::with_owner(memory, runtime, targets, || {
                    self.attribute_registry.class(api, a[0])
                })?;
                self.metadata_registry.wrap(
                    memory,
                    metadata::Value::Handle(class?, metadata::Kind::Class),
                )?
            }
            "mono_field_get_value" => {
                let field = self.metadata_registry.native(a[1], metadata::Kind::Field)?;
                let (width, _) =
                    callbacks::with_owner(memory, runtime, targets, || api.field_width(field))?;
                memory.check(a[2], width?, super::super::memory::WRITE)?;
                let (bytes, _) = callbacks::with_owner(memory, runtime, targets, || {
                    self.attribute_registry.read_field(api, a[0], field)
                })?;
                memory.write(a[2], &bytes?)?;
                0
            }
            _ => return Err("unadmitted custom-attribute operation".into()),
        };
        self.events.push(format!(
            "actual checked custom-attribute operation {function}: result={result:#x}"
        ));
        cpu.return_integer(memory, result)?;
        Ok(false)
    }
    pub(super) fn dispatch_object(
        &mut self,
        function: &str,
        cpu: &mut Cpu64,
        memory: &mut Memory,
        runtime: &mut Runtime64,
    ) -> Result<bool, String> {
        if self.stopped || !self.handles_object(function) {
            return Err("object call outside live typed provider".into());
        }
        let native = self
            .native
            .as_ref()
            .ok_or("object call has no native runtime")?;
        if native.closed || native.owner != thread::current().id() {
            return Err("object call has closed/foreign owner".into());
        }
        let library = native.library;
        let domain = native.domain;
        let a = [cpu.regs[1], cpu.regs[2], cpu.regs[8], cpu.regs[9]];
        memory.read_int(cpu.regs[4], 8)?;
        cpu.regs[4]
            .checked_add(8)
            .ok_or("object return stack overflow")?;
        if self.assets && function == "mono_object_unbox" {
            let api = self.instance_api.ok_or("unbox before rooted object API")?;
            if !self.instance_registry.is_boxed(a[0]) {
                return Err("unbox requires a live allocated boxed value identity".into());
            }
            let (address, _) =
                callbacks::with_owner(memory, runtime, self.callback_targets(), || {
                    let class = self.instance_registry.class(api, a[0])?;
                    let layout = self.boxed_layout(library, class)?;
                    self.field_registry.sync(api, &self.instance_registry)?;
                    callbacks::with_service(|memory, _| {
                        self.field_registry.unbox(
                            library,
                            api,
                            &mut self.instance_registry,
                            memory,
                            a[0],
                            layout,
                        )
                    })
                })?;
            cpu.return_integer(memory, address?)?;
            return Ok(false);
        }
        if self.assets && function == "mono_array_addr_with_size" {
            let address = self.reference_array_registry.address(a[0], a[1], a[2])?;
            cpu.return_integer(memory, address)?;
            return Ok(false);
        }
        if self.assets && function == "mono_object_get_virtual_method" {
            let api = self
                .instance_api
                .ok_or("virtual resolution before instance API")?;
            let method = self
                .metadata_registry
                .native(a[1], metadata::Kind::Method)?;
            let (resolved, _) =
                callbacks::with_owner(memory, runtime, self.callback_targets(), || {
                    let object = self.instance_registry.native(api, a[0])?;
                    let class = self.instance_registry.class(api, a[0])?;
                    virtual_methods::Api::load(library)?.resolve(object, class, method)
                })?;
            let token = self.metadata_registry.wrap(
                memory,
                metadata::Value::Handle(resolved?, metadata::Kind::Method),
            )?;
            self.virtual_resolutions += 1;
            self.events.push(format!("actual rooted virtual-method resolution: object={:#x} method={:#x} result={token:#x}; no managed invocation", a[0], a[1]));
            cpu.return_integer(memory, token)?;
            return Ok(false);
        }
        if self.fields
            && function == "mono_field_get_value"
            && self.instance_registry.contains(a[0])
        {
            let api = self.instance_api.ok_or("field read before instance API")?;
            let field = self.metadata_registry.native(a[1], metadata::Kind::Field)?;
            let (bytes, _) =
                callbacks::with_owner(memory, runtime, self.callback_targets(), || {
                    self.field_registry.sync(api, &self.instance_registry)?;
                    self.field_registry.read(a[0], field)
                })?;
            memory.write(a[2], &bytes?)?;
            cpu.return_integer(memory, 0)?;
            return Ok(false);
        }
        if self.ordinary_objects
            && (function == "mono_object_new"
                || (function == "mono_object_get_class" && self.instance_registry.contains(a[0])))
        {
            let api = match self.instance_api {
                Some(api) => api,
                None => {
                    let image = self.image_registry.unique_native_image(CORE_MODULE)?;
                    let (api, _) =
                        callbacks::with_owner(memory, runtime, self.callback_targets(), || {
                            instances::Api::load(library, image)
                        })?;
                    let api = api?;
                    self.instance_api = Some(api);
                    api
                }
            };
            let result = if function == "mono_object_new" {
                if a[0] != ROOT_TOKEN || domain.is_null() {
                    return Err("ordinary allocation requires owned root domain".into());
                }
                let class = self.metadata_registry.native(a[1], metadata::Kind::Class)?;
                let (token, _) =
                    callbacks::with_owner(memory, runtime, self.callback_targets(), || {
                        let boxed = if self.assets {
                            let value = symbol!(
                                library,
                                "mono_class_is_valuetype",
                                unsafe extern "C" fn(Object) -> i32
                            );
                            if unsafe { value(class) } != 0 {
                                Some(self.boxed_layout(library, class)?.width)
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        if let Some(size) = boxed {
                            self.instance_registry
                                .allocate_boxed(api, domain, class, size)
                        } else {
                            self.instance_registry.allocate(api, domain, class)
                        }
                    })?;
                let token = token?;
                self.instance_registry.publish(memory, token)?;
                if self.assets {
                    let image = self.image_registry.unique_native_image(INPUT_SYSTEM)?;
                    let (installed, _) =
                        callbacks::with_owner(memory, runtime, self.callback_targets(), || {
                            let object = self.instance_registry.native(api, token)?;
                            let image_of = symbol!(
                                library,
                                "mono_class_get_image",
                                unsafe extern "C" fn(Object) -> Object
                            );
                            let value = symbol!(
                                library,
                                "mono_class_is_valuetype",
                                unsafe extern "C" fn(Object) -> i32
                            );
                            let layout = if unsafe { value(class) } != 0 {
                                Some(self.boxed_layout(library, class)?)
                            } else {
                                None
                            };
                            let find = symbol!(
                                library,
                                "mono_class_from_name",
                                unsafe extern "C" fn(
                                    Object,
                                    *const c_char,
                                    *const c_char,
                                ) -> Object
                            );
                            let assignable = symbol!(
                                library,
                                "mono_class_is_assignable_from",
                                unsafe extern "C" fn(Object, Object) -> i32
                            );
                            let core = self.image_registry.unique_native_image(CORE_MODULE)?;
                            let unity =
                                unsafe { find(core, c"UnityEngine".as_ptr(), c"Object".as_ptr()) };
                            if unity.is_null() {
                                return Err("owned Unity object declaration absent".into());
                            }
                            let ordinary_owned = self
                                .reference_images()?
                                .contains(&unsafe { image_of(class) })
                                && unsafe { assignable(unity, class) } == 0;
                            let specs = if layout.is_some() {
                                Vec::new()
                            } else if ordinary_owned || unsafe { image_of(class) } == image {
                                field_layouts::specs(
                                    library,
                                    self.image_registry.unique_native_image(CORE_MODULE)?,
                                    &self.reference_images()?,
                                    object,
                                    class,
                                )?
                            } else {
                                Vec::new()
                            };
                            self.install_fields(library, api, specs)?;
                            if let Some(layout) = layout {
                                callbacks::with_service(|memory, _| {
                                    self.field_registry.publish_boxed_payload(
                                        api,
                                        &mut self.instance_registry,
                                        memory,
                                        token,
                                        layout,
                                    )
                                })?;
                            }
                            self.publish_container_graph(library, api, vec![object])
                        })?;
                    installed?;
                }
                self.events.push(format!("actual checked object allocated and immediately strong-rooted: {token:#x}; constructor not invoked; no native header published"));
                token
            } else {
                let (class, _) =
                    callbacks::with_owner(memory, runtime, self.callback_targets(), || {
                        self.instance_registry.class(api, a[0])
                    })?;
                self.metadata_registry.wrap(
                    memory,
                    metadata::Value::Handle(class?, metadata::Kind::Class),
                )?
            };
            cpu.return_integer(memory, result)?;
            return Ok(false);
        }
        if self.assets && function == "mono_object_isinst" {
            let class = self.metadata_registry.native(a[1], metadata::Kind::Class)?;
            let api = self.instance_api.ok_or("isinst before rooted object API")?;
            let isinst = symbol!(
                library,
                "mono_object_isinst",
                unsafe extern "C" fn(Object, Object) -> Object
            );
            let (result, _) =
                callbacks::with_owner(memory, runtime, self.callback_targets(), || {
                    let object = self.rooted_call_argument(api, a[0])?;
                    let result = unsafe { isinst(object, class) };
                    if result.is_null() {
                        Ok::<u64, String>(0)
                    } else if result == object {
                        Ok(a[0])
                    } else {
                        Err("isinst returned an unadmitted remoting/proxy identity".into())
                    }
                })?;
            cpu.return_integer(memory, result?)?;
            return Ok(false);
        }
        if self.assembly_lookup && names::handles(function) {
            let api = match self.name_api {
                Some(api) => api,
                None => {
                    let api = names::Api::load(library)?;
                    self.name_api = Some(api);
                    api
                }
            };
            let targets = self.callback_targets();
            let result = match function {
                "mono_assembly_name_parse" => {
                    names::Registry::check_stack(
                        memory,
                        cpu.regs[4],
                        a[1],
                        super::super::memory::WRITE,
                    )?;
                    let text = string(memory, a[0])?;
                    let (snapshot, _) = callbacks::with_owner(memory, runtime, targets, || {
                        self.name_registry.parse(api, &text, a[1])
                    })?;
                    if let Some(mut snapshot) = snapshot? {
                        let name = self
                            .metadata_registry
                            .wrap(memory, metadata::Value::Text(snapshot.name))?;
                        let culture = match snapshot.culture {
                            Some(text) => self
                                .metadata_registry
                                .wrap(memory, metadata::Value::Text(text))?,
                            None => 0,
                        };
                        snapshot.bytes[..8].copy_from_slice(&name.to_le_bytes());
                        snapshot.bytes[8..16].copy_from_slice(&culture.to_le_bytes());
                        self.name_registry.publish(memory, a[1], snapshot.bytes)?;
                        1
                    } else {
                        0
                    }
                }
                "mono_assembly_loaded" => {
                    self.name_registry.validate(memory, cpu.regs[4], a[0])?;
                    let image_api = self.image_api.ok_or("assembly lookup has no image owner")?;
                    let (token, _) = callbacks::with_owner(memory, runtime, targets, || {
                        let native = self.name_registry.loaded(api, a[0])?;
                        self.image_registry.borrow_assembly(image_api, native)
                    })?;
                    token?
                }
                _ => {
                    self.name_registry.validate(memory, cpu.regs[4], a[0])?;
                    let (result, _) = callbacks::with_owner(memory, runtime, targets, || {
                        self.name_registry.free(api, a[0])
                    })?;
                    result?;
                    0
                }
            };
            cpu.return_integer(memory, result)?;
            return Ok(false);
        }
        if self.attribute_arrays && errors::handles(function) {
            let result = if function == "mono_error_init" {
                self.error_registry.init(memory, cpu, a[0])?
            } else {
                self.error_registry.query(memory, cpu, function, a[0])?
            };
            cpu.return_integer(memory, result)?;
            return Ok(false);
        }
        if self.attributes && attributes::handles(function) {
            return self.dispatch_attributes(function, cpu, memory, runtime, library);
        }
        if function == "mono_unity_loader_get_last_error_and_error_prepare_exception" {
            // Locked original PE at RVA 0x270b90 is xor eax,eax; ret. The
            // pinned Unity Mono source exports the same no-argument NULL leaf.
            // A future non-NULL exception is not admitted by this contract.
            let get = symbol!(
                library,
                "mono_unity_loader_get_last_error_and_error_prepare_exception",
                unsafe extern "C" fn() -> Object
            );
            let (result, _) =
                callbacks::with_owner(memory, runtime, self.callback_targets(), || unsafe {
                    get()
                })?;
            if !result.is_null() {
                return Err("loader-error leaf returned an unadmitted managed exception".into());
            }
            self.events.push("exact original/native loader-error NULL leaf forwarded; original x64 bytes 33c0c3 at RVA 0x270b90".into());
            cpu.return_integer(memory, 0)?;
            return Ok(false);
        }
        if matches!(
            function,
            "mono_gchandle_new_v2"
                | "mono_gchandle_new_weakref_v2"
                | "mono_gchandle_get_target_v2"
                | "mono_gchandle_free_v2"
        ) {
            self.dispatch_handles(function, cpu, memory, runtime, library, false)?;
            return Ok(false);
        }
        if function == "mono_array_new" && self.assets && a[2] != 0 {
            if a[0] != ROOT_TOKEN {
                return Err("reference array allocation requires owned domain".into());
            }
            let element = self.metadata_registry.native(a[1], metadata::Kind::Class)?;
            let api = self
                .reference_array_api
                .ok_or("reference array before checked element API")?;
            let root_api = self
                .instance_api
                .ok_or("reference array before pinned root API")?;
            let (token, _) =
                callbacks::with_owner(memory, runtime, self.callback_targets(), || {
                    self.reference_array_registry.allocate(
                        api,
                        root_api,
                        &mut self.instance_registry,
                        domain,
                        element,
                        a[2],
                    )
                })?;
            let token = token?;
            self.reference_array_registry.publish(memory, token)?;
            self.events.push(format!("actual pinned reference SZ array allocated: token={token:#x} length={}; only owned header cells and reference tokens exposed",a[2]));
            cpu.return_integer(memory, token)?;
            return Ok(false);
        }
        if function == "mono_array_new" {
            if a[0] != ROOT_TOKEN || domain.is_null() || a[2] != 0 {
                return Err(
                    "startup arrays admit only real zero-length SZ arrays in the owned root domain"
                        .into(),
                );
            }
            let element = self.metadata_registry.native(a[1], metadata::Kind::Class)?;
            let api = match self.array_api {
                Some(api) => api,
                None => {
                    let api = arrays::Api::load(library)?;
                    self.array_api = Some(api);
                    api
                }
            };
            let targets = self.callback_targets();
            let (result, _) = callbacks::with_owner(memory, runtime, targets, || {
                let object = unsafe { (api.new)(domain, element, 0) };
                let token = self.array_registry.root_empty(api, object, element)?;
                if self.assets {
                    if let Some(instance_api) = self.instance_api {
                        let native = self.array_registry.native(api, token)?;
                        self.instance_registry
                            .adopt_empty_array(instance_api, token, native)?;
                    }
                }
                Ok::<_, String>(token)
            })?;
            let token = result?;
            self.array_registry.publish(memory, token)?;
            self.events.push(format!("actual Mono empty SZ array allocated, strong-rooted and published with only immutable bounds/length cells: {token:#x}"));
            cpu.return_integer(memory, token)?;
            return Ok(false);
        }
        if function == "mono_type_get_object" {
            if a[0] != ROOT_TOKEN || domain.is_null() {
                return Err("reflection type allocation requires owned domain".into());
            }
            let ty = self.metadata_registry.native(a[1], metadata::Kind::Type)?;
            let api = self
                .reflection_api
                .ok_or("type object before reflection API")?;
            let get = symbol!(
                library,
                "mono_type_get_object",
                unsafe extern "C" fn(Object, Object) -> Object
            );
            let (result, _) =
                callbacks::with_owner(memory, runtime, self.callback_targets(), || {
                    let native = unsafe { get(domain, ty) };
                    let (token, actual) = self.reflection_registry.root_type(api, native)?;
                    if actual != ty {
                        return Err("reflection type object changed metadata identity".into());
                    }
                    Ok::<_, String>(token)
                })?;
            let token = result?;
            self.reflection_registry.publish(memory, token, a[1])?;
            self.events.push(format!(
                "actual mono_type_get_object: rooted RuntimeType={token:#x}; type={:#x}",
                a[1]
            ));
            cpu.return_integer(memory, token)?;
            return Ok(false);
        }
        if function == "mono_reflection_type_get_type" {
            let api = self
                .reflection_api
                .ok_or("reflection getter before any rooted RuntimeType")?;
            let (ty, _) = callbacks::with_owner(memory, runtime, self.callback_targets(), || {
                let native = self.reflection_registry.native(api, a[0])?;
                Ok::<_, String>(unsafe { (api.get_type)(native) })
            })?;
            let token = self
                .metadata_registry
                .wrap(memory, metadata::Value::Handle(ty?, metadata::Kind::Type))?;
            cpu.return_integer(memory, token)?;
            return Ok(false);
        }
        let api = match self.object_api {
            Some(api) => api,
            None => {
                let api = objects::Api::load(library)?;
                self.object_api = Some(api);
                api
            }
        };
        let targets = self.callback_targets();
        let mut input_bytes = None;
        let result = if matches!(
            function,
            "mono_string_new_len" | "mono_string_new_wrapper" | "mono_unity_string_empty_wrapper"
        ) {
            let empty = function == "mono_unity_string_empty_wrapper";
            let wrapper = function != "mono_string_new_len";
            if domain.is_null() || (!wrapper && (a[0] != ROOT_TOKEN || a[2] > 32768)) {
                return Err("string allocation requires owned root and bounded input".into());
            }
            let bytes = if empty {
                Vec::new()
            } else if wrapper {
                string(memory, a[0])?.into_bytes()
            } else {
                let mut bytes = vec![0; a[2] as usize];
                memory.read(a[1], &mut bytes)?;
                bytes
            };
            if bytes.len() > 32768 {
                return Err("wrapper string exceeds input bound".into());
            }
            input_bytes = Some(bytes.len());
            let text = (wrapper && !empty)
                .then(|| CString::new(bytes.clone()))
                .transpose()
                .map_err(|e| e.to_string())?;
            let (token, _) = callbacks::with_owner(memory, runtime, targets, || {
                let object = if empty {
                    let current =
                        symbol!(library, "mono_domain_get", unsafe extern "C" fn() -> Object);
                    let new = symbol!(
                        library,
                        "mono_unity_string_empty_wrapper",
                        unsafe extern "C" fn() -> Object
                    );
                    if unsafe { current() } != domain {
                        return Err("empty string current domain mismatch".into());
                    }
                    unsafe { new() }
                } else if let Some(text) = &text {
                    let current =
                        symbol!(library, "mono_domain_get", unsafe extern "C" fn() -> Object);
                    let new = symbol!(
                        library,
                        "mono_string_new_wrapper",
                        unsafe extern "C" fn(*const c_char) -> Object
                    );
                    if unsafe { current() } != domain {
                        return Err("string wrapper current domain mismatch".into());
                    }
                    unsafe { new(text.as_ptr()) }
                } else {
                    unsafe { (api.new_string)(domain, bytes.as_ptr().cast(), bytes.len() as u32) }
                };
                let token = self.object_registry.root_string(api, object)?;
                if self.assets {
                    if let Some(instance_api) = self.instance_api {
                        let native = self.object_registry.native_string(api, token)?;
                        self.instance_registry
                            .adopt_string(instance_api, token, native)?;
                    }
                }
                Ok::<_, String>(token)
            })?;
            let token = token?;
            self.object_registry.publish_string(memory, token)?;
            token
        } else {
            let (native, _) = callbacks::with_owner(memory, runtime, targets, || {
                self.object_registry.native_string(api, a[0])
            })?;
            native?;
            if function == "mono_string_length" {
                memory.read_int(a[0] + 16, 4)?
            } else {
                a[0] + 20
            }
        };
        self.events.push(format!(
            "actual owned string boundary {function} -> {result:#x}; bytes={}",
            input_bytes.unwrap_or(0)
        ));
        cpu.return_integer(memory, result)?;
        Ok(false)
    }
    pub(super) fn handles_invoke(&self, function: &str) -> bool {
        self.managed_types && function == "mono_runtime_invoke"
    }
    fn rooted_call_argument(&self, api: instances::Api, token: u64) -> Result<Object, String> {
        if token == 0 {
            return Ok(ptr::null_mut());
        }
        if self.object_registry.contains(token) {
            self.object_registry
                .native_string(self.object_api.ok_or("argument String API absent")?, token)
        } else if self.reflection_registry.contains(token) {
            self.reflection_registry.native(
                self.reflection_api
                    .ok_or("argument RuntimeType API absent")?,
                token,
            )
        } else if self.instance_registry.contains(token) {
            self.instance_registry.native(api, token)
        } else if self.array_registry.contains(token) {
            self.array_registry.native(
                self.array_api.ok_or("argument empty array API absent")?,
                token,
            )
        } else if self.attribute_registry.contains(token) {
            self.attribute_registry.native(
                self.attribute_api.ok_or("argument attribute API absent")?,
                token,
            )
        } else {
            Err("managed argument is not a live rooted identity".into())
        }
    }
    fn temporary_call_slot(
        memory: &Memory,
        runtime: &Runtime64,
        rsp: u64,
        address: u64,
        width: usize,
    ) -> bool {
        let Some(end) = address.checked_add(width as u64) else {
            return false;
        };
        let stack = address >= rsp.saturating_add(8)
            && address >= super::STACK
            && end <= super::STACK + 64 * 1024 * 1024;
        memory.owned_noncode_access(address, width)
            && (stack
                || runtime.live_data_slot(address, width as u64)
                || memory.guest_private_data_slot(address, width))
    }
    fn dispatch_managed_call(
        &mut self,
        cpu: &mut Cpu64,
        memory: &mut Memory,
        runtime: &mut Runtime64,
    ) -> Result<bool, String> {
        let native = self
            .native
            .as_ref()
            .ok_or("typed managed call before runtime")?;
        let library = native.library;
        let domain = native.domain;
        let thread_handle = native.thread_handle;
        let a = [cpu.regs[1], cpu.regs[2], cpu.regs[8], cpu.regs[9]];
        let method = self
            .metadata_registry
            .native(a[0], metadata::Kind::Method)?;
        let call_api = managed_calls::Api::load(library)?;
        let managed_images = self.image_registry.live_native_images()?;
        if !resources::installed() {
            resources::install(
                library,
                self.image_registry.unique_native_image(CORE_MODULE)?,
                [
                    self.registry
                        .exact_target(resources::NAME, resources::TARGET)?,
                    self.registry
                        .exact_target(resources::FIND_NAME, resources::FIND_TARGET)?,
                ],
                &managed_images,
                &self.registry,
            )?;
        }
        if !scriptable::installed() {
            scriptable::install(
                library,
                self.image_registry.unique_native_image(CORE_MODULE)?,
                self.registry
                    .exact_target(scriptable::NAME, scriptable::TARGET)?,
            )?;
        }
        if !logging::debug_installed() {
            logging::install_debug(
                library,
                self.image_registry.unique_native_image(CORE_MODULE)?,
                &managed_images,
                self.registry
                    .exact_target(logging::DEBUG_NAME, logging::DEBUG_TARGET)?,
            )?;
        }
        if !engine_scalars::installed() {
            engine_scalars::install(
                library,
                self.image_registry.unique_native_image(CORE_MODULE)?,
                &managed_images,
                &self.registry,
            )?;
        }
        if !engine_objects::installed() {
            engine_objects::install(
                library,
                self.image_registry.unique_native_image(CORE_MODULE)?,
                &managed_images,
                &self.registry,
            )?;
        }
        if !input_events::installed() {
            input_events::install(library, &managed_images, &self.registry)?;
        }
        if !binding_strings::installed() {
            binding_strings::install(
                library,
                self.image_registry.unique_native_image(CORE_MODULE)?,
                &managed_images,
                &self.registry,
            )?;
        }
        if !player_prefs::installed() {
            player_prefs::install(
                library,
                self.image_registry.unique_native_image(CORE_MODULE)?,
                &managed_images,
                &self.registry,
            )?;
        }
        if !screen::installed() {
            screen::install(
                library,
                self.image_registry.unique_native_image(CORE_MODULE)?,
                &managed_images,
                &self.registry,
            )?;
        }
        if !profiler_samples::installed() {
            profiler_samples::install(library, &managed_images, &self.registry)?;
        }
        if !profiler_recorders::installed() {
            profiler_recorders::install(
                library,
                self.image_registry.unique_native_image(CORE_MODULE)?,
                &managed_images,
                &self.registry,
            )?;
        }
        if !unsafe_buffers::installed() {
            unsafe_buffers::install(library, &managed_images, &self.registry)?;
        }
        if !burst_shared::installed() {
            burst_shared::install(
                library,
                self.image_registry.unique_native_image(CORE_MODULE)?,
                &managed_images,
                self.registry
                    .exact_target(burst_shared::NAME, burst_shared::TARGET)?,
            )?;
        }
        if !jobs::installed() {
            jobs::install(
                library,
                self.image_registry.unique_native_image(CORE_MODULE)?,
                &managed_images,
                self.registry.exact_target(jobs::NAME, jobs::TARGET)?,
            )?;
        }
        if !json::installed() {
            json::install(
                library,
                self.image_registry.unique_native_image(CORE_MODULE)?,
                &managed_images,
                self.registry.exact_target(json::NAME, json::TARGET)?,
            )?;
        }
        let descriptor = call_api
            .describe(method, &managed_images, &managed_images)?
            .ok_or("managed entry is not an admitted ordinary IL call")?;
        if descriptor.receiver.is_some() != (a[1] != 0) {
            return Err("managed static/instance receiver presence mismatch".into());
        }
        let api = self
            .instance_api
            .ok_or("managed call before rooted instances")?;
        let strings = self
            .object_api
            .ok_or("managed call before canonical strings")?;
        let targets = self.callback_targets();
        let rsp = cpu.regs[4];
        if a[3] != 0 {
            memory.check(a[3], 8, super::super::memory::WRITE)?;
            if !a[3].is_multiple_of(8) || !Self::temporary_call_slot(memory, runtime, rsp, a[3], 8)
            {
                return Err("managed exception output is not an owned temporary slot".into());
            }
        }
        let (prepared, _) = callbacks::with_owner(memory, runtime, targets, || {
            let (receiver, receiver_class) = match descriptor.receiver {
                Some(declared) => {
                    let receiver = self.rooted_call_argument(api, a[1])?;
                    let class = call_api.receiver(receiver, declared, &managed_images)?;
                    (receiver, class)
                }
                None => (ptr::null_mut(), ptr::null_mut()),
            };
            callbacks::with_service(|memory, runtime| {
                self.instance_registry.sync_from_guest(api, memory)?;
                self.field_registry.sync(api, &self.instance_registry)?;
                if let Some(array_api) = self.reference_array_api {
                    self.reference_array_registry.sync(
                        array_api,
                        api,
                        &self.instance_registry,
                        memory,
                    )?;
                }
                let frame = managed_calls::Frame::prepare(
                    memory,
                    &descriptor,
                    a[2],
                    |token, shape| {
                        call_api.compatible(self.rooted_call_argument(api, token)?, shape)
                    },
                    |address, width| {
                        Self::temporary_call_slot(memory, runtime, rsp, address, width)
                    },
                )?;
                Ok::<_, String>((frame, receiver, receiver_class))
            })
        })?;
        let (mut frame, receiver, receiver_class) = prepared?;
        let mut parameters = frame.pointers();
        let initial_outputs = frame.outputs()?;
        if a[3] != 0
            && initial_outputs.iter().any(|out| {
                out.address < a[3] + 8
                    && a[3]
                        < out.address
                            + match out.shape {
                                managed_calls::Shape::Scalar { width, .. } => width as u64,
                                _ => 8,
                            }
            })
        {
            return Err("managed exception slot overlaps byref output".into());
        }
        let (invoked, _) = callbacks::with_metadata_owner(
            memory,
            runtime,
            targets,
            reentry::Metadata {
                library,
                domain,
                thread_handle,
                metadata: &mut self.metadata_registry,
                images: &mut self.image_registry,
                rooted_instance: descriptor.receiver.map(|_| (a[1], receiver_class)),
                objects: Some(reentry::ObjectRoots {
                    api,
                    roots: &mut self.instance_registry,
                    strings,
                    objects: &mut self.object_registry,
                    fields: &mut self.field_registry,
                    handles: (
                        self.handle_api.ok_or("nested handle API absent")?,
                        &mut self.handle_registry,
                    ),
                    reflection: (
                        self.reflection_api.ok_or("nested reflection API absent")?,
                        &mut self.reflection_registry,
                    ),
                    empty_arrays: (
                        self.array_api.ok_or("nested empty array API absent")?,
                        &mut self.array_registry,
                    ),
                    attributes: (
                        self.attribute_api.ok_or("nested attributes API absent")?,
                        &mut self.attribute_registry,
                    ),
                    arrays: self
                        .reference_array_api
                        .map(|api| (api, &mut self.reference_array_registry)),
                }),
            },
            || {
                let mut exception = ptr::null_mut();
                let params = if parameters.is_empty() {
                    ptr::null_mut()
                } else {
                    parameters.as_mut_ptr()
                };
                let result = unsafe { (call_api.invoke)(method, receiver, params, &mut exception) };
                if !exception.is_null() {
                    return Err(
                        "typed managed call raised an exception; guest outputs left unchanged"
                            .into(),
                    );
                }
                Ok::<_, String>(result)
            },
        )?;
        let result = invoked?;
        let (converted, _) = callbacks::with_owner(memory, runtime, targets, || {
            let result = match descriptor.returns {
                None if result.is_null() => 0,
                None => return Err("void managed call returned a boxed value".into()),
                Some(shape @ managed_calls::Shape::Scalar { .. }) => {
                    let class = call_api.scalar_box(result, shape)?;
                    let token = self
                        .instance_registry
                        .borrow_boxed_result(api, result, class)?;
                    let layout = self.boxed_layout(library, class)?;
                    callbacks::with_service(|memory, _| {
                        self.field_registry.publish_boxed_payload(
                            api,
                            &mut self.instance_registry,
                            memory,
                            token,
                            layout,
                        )
                    })?;
                    token
                }
                Some(shape @ managed_calls::Shape::Reference { .. }) => {
                    call_api.compatible(result, shape)?;
                    self.object_registry.borrow_reference(
                        strings,
                        api,
                        &mut self.instance_registry,
                        result,
                    )?
                }
            };
            let mut outputs = Vec::new();
            for out in frame.outputs()? {
                let (word, width) = match out.shape {
                    shape @ managed_calls::Shape::Reference { .. } => {
                        let native = call_api.compatible(out.word as Object, shape)?;
                        (
                            self.object_registry.borrow_reference(
                                strings,
                                api,
                                &mut self.instance_registry,
                                native,
                            )?,
                            8,
                        )
                    }
                    managed_calls::Shape::Scalar { width, .. } => (out.word, width),
                };
                outputs.push((out.address, width, word));
            }
            self.refresh_managed_graph(library, api)?;
            callbacks::with_service(|memory, runtime| {
                self.object_registry.publish_pending(memory)?;
                for &(address, width, _) in &outputs {
                    memory.check(address, width, super::super::memory::WRITE)?;
                    if !Self::temporary_call_slot(memory, runtime, rsp, address, width) {
                        return Err(
                            "managed byref output storage retired during native call".into()
                        );
                    }
                }
                if a[3] != 0 {
                    memory.check(a[3], 8, super::super::memory::WRITE)?;
                    if !Self::temporary_call_slot(memory, runtime, rsp, a[3], 8) {
                        return Err(
                            "managed exception output storage retired during native call".into(),
                        );
                    }
                }
                for &(address, width, word) in &outputs {
                    memory.write_int(address, width, word)?;
                }
                if a[3] != 0 {
                    memory.write_int(a[3], 8, 0)?;
                }
                Ok::<_, String>(())
            })?;
            Ok::<_, String>((result, outputs.len()))
        })?;
        let (result, outputs) = converted?;
        let kind = if descriptor.receiver.is_some() {
            self.instance_invokes += 1;
            self.instance_output_writes += outputs;
            "instance"
        } else {
            self.static_invokes += 1;
            self.static_output_writes += outputs;
            "static"
        };
        self.events.push(format!("metadata-qualified {kind} managed entry: {}; params={}; byref-outputs={outputs}; actual IL and typed return", descriptor.name, descriptor.parameters.len()));
        cpu.return_integer(memory, result)?;
        Ok(false)
    }
    pub(super) fn dispatch_invoke(
        &mut self,
        cpu: &mut Cpu64,
        memory: &mut Memory,
        runtime: &mut Runtime64,
    ) -> Result<bool, String> {
        if self.stopped || !self.managed_types {
            return Err("managed invoke outside live diagnostic provider".into());
        }
        let native = self
            .native
            .as_ref()
            .ok_or("managed invoke has no runtime")?;
        if native.closed || native.owner != thread::current().id() {
            return Err("managed invoke has closed/foreign owner".into());
        }
        let library = native.library;
        let native_domain = native.domain;
        let a = [cpu.regs[1], cpu.regs[2], cpu.regs[8], cpu.regs[9]];
        memory.read_int(cpu.regs[4], 8)?;
        cpu.regs[4]
            .checked_add(8)
            .ok_or("invoke return stack overflow")?;
        if self.curves
            && self.curves_constructor.is_none()
            && self.classlib_invokes >= self.startup_limit()
            && a[0] != self.get_type_method
        {
            if a[1] == 0 || a[2] != 0 {
                return Err("ColorCurves ctor requires owned this and NULL parameters".into());
            }
            if a[3] != 0 {
                memory.check(a[3], 8, super::super::memory::WRITE)?;
            }
            for (name, target) in animation::BINDINGS {
                self.registry.exact_target(name, target)?;
            }
            let method = self
                .metadata_registry
                .native(a[0], metadata::Kind::Method)?;
            let images = [
                self.image_registry.unique_native_image(CORE_MODULE)?,
                self.image_registry
                    .unique_native_image(animation::RENDER_CORE)?,
                self.image_registry.unique_native_image(animation::URP)?,
            ];
            let api = self
                .instance_api
                .ok_or("ctor before rooted allocation API")?;
            let targets = self.callback_targets();
            let (prepared, _) = callbacks::with_owner(memory, runtime, targets, || {
                callbacks::with_service(|memory, _| {
                    self.instance_registry.sync_from_guest(api, memory)
                })?;
                Ok::<_, String>((
                    self.instance_registry.native(api, a[1])?,
                    self.instance_registry.class(api, a[1])?,
                ))
            })?;
            let (object, class) = prepared?;
            let (constructed, stats) = callbacks::with_metadata_owner(
                memory,
                runtime,
                targets,
                reentry::Metadata {
                    library,
                    domain: native_domain,
                    thread_handle: self
                        .native
                        .as_ref()
                        .ok_or("nested thread owner absent")?
                        .thread_handle,
                    metadata: &mut self.metadata_registry,
                    images: &mut self.image_registry,
                    rooted_instance: Some((a[1], class)),
                    objects: None,
                },
                || animation::Constructor::execute(library, images, method, object, class, a[1]),
            )?;
            self.curves_constructor = Some(constructed?);
            if self.fields {
                let specs = self
                    .curves_constructor
                    .as_mut()
                    .unwrap()
                    .serialized_fields()?;
                let (installed, _) = callbacks::with_owner(memory, runtime, targets, || {
                    self.install_fields(library, api, specs)
                })?;
                installed?;
            }
            self.events.push(format!(
                "actual ColorCurves ctor executed; nested Mono reentries={}",
                stats.mono_reentries
            ));
            if a[3] != 0 {
                memory.write_int(a[3], 8, 0)?;
            }
            cpu.return_integer(memory, 0)?;
            if self.fields {
                self.events.push(
                    "initialized typed fields published; continue actual Unity deserialization"
                        .into(),
                );
                return Ok(false);
            }
            // `curves` is a finite constructor checkpoint. Verify this exact
            // initialized graph across real GC before returning diagnostic
            // success; serialized instance field views are a subsequent scope.
            self.mark_completed_layers();
            self.validate_scalar_callbacks(memory, runtime)?;
            self.validate_profiler_callbacks(memory, runtime)?;
            self.validate_unitytls(memory, runtime)?;
            self.events.push("finite ColorCurves constructor checkpoint completed; stop before initialized instance-field serialization".into());
            return Ok(true);
        }
        if self.assets && self.curves_constructor.is_some() && a[0] != self.get_type_method {
            let method = self
                .metadata_registry
                .native(a[0], metadata::Kind::Method)?;
            if constructors::Api::load(library)?
                .describe(method, &self.constructor_images()?)?
                .is_none()
            {
                return self.dispatch_managed_call(cpu, memory, runtime);
            }
        }
        if self.components && self.curves_constructor.is_some() && a[0] != self.get_type_method {
            if a[1] == 0 {
                return Err("managed instance call requires rooted this".into());
            }
            if a[3] != 0 {
                memory.check(a[3], 8, super::super::memory::WRITE)?;
            }
            let method = self
                .metadata_registry
                .native(a[0], metadata::Kind::Method)?;
            let api = self
                .instance_api
                .ok_or("component before rooted allocation API")?;
            let targets = self.callback_targets();
            let images = [
                self.image_registry.unique_native_image(CORE_MODULE)?,
                self.image_registry
                    .unique_native_image(animation::RENDER_CORE)?,
                self.image_registry.unique_native_image(animation::URP)?,
            ];
            let (prepared, _) = callbacks::with_owner(memory, runtime, targets, || {
                callbacks::with_service(|memory, _| {
                    self.instance_registry.sync_from_guest(api, memory)?;
                    self.field_registry.sync(api, &self.instance_registry)?;
                    if let Some(array_api) = self.reference_array_api {
                        self.reference_array_registry.sync(
                            array_api,
                            api,
                            &self.instance_registry,
                            memory,
                        )?;
                    }
                    Ok::<_, String>(())
                })?;
                Ok::<_, String>((
                    self.instance_registry.native(api, a[1])?,
                    self.instance_registry.class(api, a[1])?,
                ))
            })?;
            let (object, class) = prepared?;
            let descriptor_api = constructors::Api::load(library)?;
            let descriptor = descriptor_api
                .describe(method, &self.constructor_images()?)?
                .ok_or("managed entry is not a constructor")?;
            descriptor_api.receiver(&descriptor, object, class)?;
            if descriptor.callback.is_none() && a[2] != 0 {
                return Err("constructor parameters must be NULL for the zero-argument ABI".into());
            }
            let input_image = if self.assets {
                self.image_registry.unique_native_image(INPUT_SYSTEM)?
            } else {
                ptr::null_mut()
            };
            let is_input = !input_image.is_null() && descriptor.image == input_image;
            if self.assets && self.reference_array_api.is_none() {
                let (api, _) = callbacks::with_owner(memory, runtime, targets, || {
                    reference_arrays::Api::load(library, self.reference_images()?)
                })?;
                self.reference_array_api = Some(api?);
            }
            self.registry
                .exact_target(animation::MARKER_BINDING.0, animation::MARKER_BINDING.1)?;
            let (constructed, _) = callbacks::with_metadata_owner(
                memory,
                runtime,
                targets,
                reentry::Metadata {
                    library,
                    domain: native_domain,
                    thread_handle: self
                        .native
                        .as_ref()
                        .ok_or("nested thread owner absent")?
                        .thread_handle,
                    metadata: &mut self.metadata_registry,
                    images: &mut self.image_registry,
                    rooted_instance: Some((a[1], class)),
                    objects: None,
                },
                || {
                    self.curves_constructor
                        .as_ref()
                        .unwrap()
                        .execute_managed(images[0], method, object, a[1])
                },
            )?;
            constructed?;
            let (installed, _) = callbacks::with_owner(memory, runtime, targets, || {
                self.refresh_managed_graph(library, api)?;
                let specs = if self.field_registry.has_owner(a[1]) {
                    Vec::new()
                } else if fields::is_component(library, images[1], class)? {
                    fields::component_specs(library, images, object, class)?
                } else {
                    field_layouts::specs(
                        library,
                        images[0],
                        &self.constructor_images()?,
                        object,
                        class,
                    )
                    .map_err(|e| format!("{} managed entry field layout: {e}", descriptor.name))?
                };
                self.install_fields(library, api, specs)?;
                if self.assets {
                    let references = self.field_registry.native_references(
                        api,
                        &self.instance_registry,
                        a[1],
                    )?;
                    self.publish_container_graph(library, api, references)?;
                }
                Ok::<_, String>(())
            })?;
            installed?;
            // Keep historical per-class counters for probe comparisons only.
            // These names do not select or admit a managed implementation.
            if descriptor.callback.is_some() {
                self.serialization_invokes += 1;
            } else {
                match (is_input, descriptor.name.as_str()) {
                    (true, "InputAction") => self.input_action_invokes += 1,
                    (true, "InputActionMap") => self.input_map_invokes += 1,
                    (true, "InputActionAsset") => self.asset_invokes += 1,
                    _ => self.component_invokes += 1,
                }
            }
            self.events.push(format!("metadata-qualified managed entry: {}; serialization={}; real IL, rooted this, typed engine boundaries",descriptor.name,descriptor.callback.is_some()));
            if a[3] != 0 {
                memory.write_int(a[3], 8, 0)?;
            }
            cpu.return_integer(memory, 0)?;
            return Ok(false);
        }
        if self.classlib && self.get_type_method != 0 && a[0] != self.get_type_method {
            let logging_init = self.logging && self.classlib_invokes == 1;
            let project_folder = self.classlib_invokes == 3;
            let argument = if project_folder {
                if a[2] == 0 || !a[2].is_multiple_of(8) {
                    return Err("project folder requires an owned parameter array".into());
                }
                Some(memory.read_int(a[2], 8)?)
            } else {
                None
            };
            if self.classlib_invokes >= self.startup_limit() || a[1] != 0 {
                return Err("classlib init outside ordered static diagnostic calls".into());
            }
            if a[3] != 0 {
                memory.check(a[3], 8, super::super::memory::WRITE)?;
            }
            let method = self
                .metadata_registry
                .native(a[0], metadata::Kind::Method)?;
            let image = self
                .image_registry
                .unique_native_image(if self.classlib_invokes == 7 {
                    IMGUI_MODULE
                } else if self.classlib_invokes >= 5 {
                    SUBSYSTEMS_MODULE
                } else {
                    CORE_MODULE
                })?;
            let call = symbol!(
                library,
                "mono_runtime_invoke",
                unsafe extern "C" fn(Object, Object, *mut Object, *mut Object) -> Object
            );
            let step = self.classlib_invokes;
            let targets = self.callback_targets();
            let (arguments, _) = callbacks::with_owner(memory, runtime, targets, || {
                if let Err(error) =
                    invoke::validate_classlib_init(library, method, image, self.classlib_invokes)
                {
                    eprintln!(
                        "NATIVE_MONO_REJECTED_INVOKE: {}",
                        self.metadata_registry.describe_method(library, a[0])?
                    );
                    for (label, _, listing) in invoke::owned_bodies(library, method)? {
                        eprintln!("NATIVE_MONO_REJECTED_IL {label}:\n{listing}");
                    }
                    return Err(error);
                }
                if logging_init {
                    services::install(library)?;
                    logging::install(
                        library,
                        image,
                        self.registry.exact_target(logging::NAME, logging::TARGET)?,
                    )?;
                }
                if step == 6 {
                    subsystems::install_analytics(
                        library,
                        image,
                        self.image_registry.unique_native_image(CORE_MODULE)?,
                        self.registry.exact_target(
                            subsystems::ANALYTICS_NAME,
                            subsystems::ANALYTICS_TARGET,
                        )?,
                    )?;
                    subsystems::install(
                        library,
                        image,
                        self.registry
                            .exact_target(subsystems::NAME, subsystems::TARGET)?,
                    )?;
                }
                Ok::<_, String>([if let Some(token) = argument {
                    if token == 0 {
                        ptr::null_mut()
                    } else {
                        self.object_registry.native_string(
                            self.object_api.ok_or("project folder before string API")?,
                            token,
                        )?
                    }
                } else {
                    ptr::null_mut()
                }])
            })?;
            let mut args = arguments?;
            // No provider registry borrow spans the native invoke. The optional
            // nested route owns only metadata/image registry borrows, while the
            // prepared method and argument pointers are stable rooted identities.
            let operation = || {
                let mut exception = ptr::null_mut();
                eprintln!(
                    "NATIVE_MONO_MANAGED_INVOKE_BEGIN: startup step={step} static void({})",
                    if project_folder { "String" } else { "" }
                );
                let parameters = if project_folder {
                    args.as_mut_ptr()
                } else {
                    ptr::null_mut()
                };
                let result = unsafe { call(method, ptr::null_mut(), parameters, &mut exception) };
                if !exception.is_null() {
                    return Err(
                        "native startup invoke raised an unadmitted managed exception".into(),
                    );
                }
                if !result.is_null() {
                    return Err("void native startup invoke returned an object".into());
                }
                if logging_init {
                    logging::verify(library, image)?;
                }
                if project_folder {
                    invoke::verify_project_folder(library, image, args[0])?;
                }
                if step == 4 {
                    invoke::verify_synchronization(library, image)?;
                }
                if step == 5 {
                    invoke::verify_subsystems(library, image, false)?;
                }
                if step == 7 {
                    invoke::verify_imgui(library, image)?;
                }
                if step == 6 {
                    subsystems::verify()?;
                    invoke::verify_subsystems(library, image, true)?;
                }
                Ok::<_, String>(())
            };
            let (result, stats) = if step == 6 {
                callbacks::with_metadata_owner(
                    memory,
                    runtime,
                    targets,
                    reentry::Metadata {
                        library,
                        domain: native_domain,
                        thread_handle: self
                            .native
                            .as_ref()
                            .ok_or("nested thread owner absent")?
                            .thread_handle,
                        metadata: &mut self.metadata_registry,
                        images: &mut self.image_registry,
                        rooted_instance: None,
                        objects: None,
                    },
                    operation,
                )?
            } else {
                callbacks::with_owner(memory, runtime, targets, operation)?
            };
            result?;
            if step == 6 {
                eprintln!(
                    "NATIVE_MONO_SUBSYSTEM_MAP_REENTRIES: {} managed-invokes={}",
                    stats.mono_reentries, stats.managed_reentries
                );
                self.events.push(format!(
                    "actual subsystem scripting map nested Mono reentries={} managed-invokes={}",
                    stats.mono_reentries, stats.managed_reentries
                ));
            }
            if a[3] != 0 {
                memory.write_int(a[3], 8, 0)?;
            }
            self.classlib_invokes += 1;
            self.events.push(format!("actual native Unity startup step={} executed; typed static void({}); guest object identity resolved through owned roots", self.classlib_invokes - 1, if project_folder { "String" } else { "" }));
            cpu.return_integer(memory, 0)?;
            return Ok(false);
        }
        if a[1] != 0 || a[2] == 0 || !a[2].is_multiple_of(8) {
            return Err("GetType invoke requires static this and owned parameter array".into());
        }
        let argument = memory.read_int(a[2], 8)?;
        if a[3] != 0 {
            memory.check(a[3], 8, super::super::memory::WRITE)?;
        }
        let method = self
            .metadata_registry
            .native(a[0], metadata::Kind::Method)?;
        let string_api = self
            .object_api
            .ok_or("managed invoke before rooted string")?;
        let reflection_api = match self.reflection_api {
            Some(api) => api,
            None => {
                let api = reflection::Api::load(library)?;
                self.reflection_api = Some(api);
                api
            }
        };
        let call = symbol!(
            library,
            "mono_runtime_invoke",
            unsafe extern "C" fn(Object, Object, *mut Object, *mut Object) -> Object
        );
        let (result, _) = callbacks::with_owner(memory, runtime, self.callback_targets(), || {
            invoke::validate_get_type(library, method)?;
            let string = self.object_registry.native_string(string_api, argument)?;
            let mut args = [string];
            let mut exception = ptr::null_mut();
            let result =
                unsafe { call(method, ptr::null_mut(), args.as_mut_ptr(), &mut exception) };
            if !exception.is_null() {
                return Err("native System.Type.GetType raised a managed exception; exception ABI not yet admitted".into());
            }
            self.reflection_registry.root_type(reflection_api, result)
        })?;
        let (object, ty) = result?;
        let ty_token = self
            .metadata_registry
            .wrap(memory, metadata::Value::Handle(ty, metadata::Kind::Type))?;
        self.reflection_registry.publish(memory, object, ty_token)?;
        if a[3] != 0 {
            memory.write_int(a[3], 8, 0)?;
        }
        self.get_type_method = a[0];
        self.managed_type_invokes += 1;
        self.events.push(format!("actual native System.Type.GetType(String) invoke -> rooted RuntimeType={object:#x}, typed metadata={ty_token:#x}"));
        cpu.return_integer(memory, object)?;
        Ok(false)
    }
    pub(super) fn observe_stop(&mut self, function: &str, cpu: &Cpu64) {
        self.events.push(format!(
            "pending boundary {function}: args={:x?}; rsp={:#x}",
            [cpu.regs[1], cpu.regs[2], cpu.regs[8], cpu.regs[9]],
            cpu.regs[4]
        ));
    }
    pub(super) fn inspect_pending(
        &mut self,
        function: &str,
        cpu: &Cpu64,
        memory: &mut Memory,
        runtime: &mut Runtime64,
    ) -> Result<(), String> {
        if function == "mono_array_new" {
            let library = self
                .native
                .as_ref()
                .ok_or("pending array has no runtime")?
                .library;
            let (description, _) =
                callbacks::with_owner(memory, runtime, self.callback_targets(), || {
                    self.metadata_registry.describe_class(library, cpu.regs[2])
                })?;
            self.events.push(format!(
                "pending typed nonempty array: length={} element={}; guest-caller={:#x}",
                cpu.regs[8],
                description?,
                memory.read_int(cpu.regs[4], 8)?
            ));
            return Ok(());
        }
        if function == "mono_object_new" {
            if cpu.regs[1] != ROOT_TOKEN {
                return Err("pending object allocation has unknown domain".into());
            }
            let library = self
                .native
                .as_ref()
                .ok_or("pending object has no runtime")?
                .library;
            let (description, _) =
                callbacks::with_owner(memory, runtime, self.callback_targets(), || {
                    self.metadata_registry.describe_class(library, cpu.regs[2])
                })?;
            self.events
                .push(format!("pending typed object allocation: {}", description?));
            return Ok(());
        }
        if function != "mono_runtime_invoke" {
            return Ok(());
        }
        let library = self
            .native
            .as_ref()
            .ok_or("pending invoke has no runtime")?
            .library;
        let (description, _) =
            callbacks::with_owner(memory, runtime, self.callback_targets(), || {
                self.metadata_registry.describe_method(library, cpu.regs[1])
            })?;
        self.events
            .push(format!("pending typed invoke: {}", description?));
        let method = self
            .metadata_registry
            .native(cpu.regs[1], metadata::Kind::Method)?;
        let (bodies, _) = callbacks::with_owner(memory, runtime, self.callback_targets(), || {
            invoke::owned_bodies(library, method)
        })?;
        for (label, bytes, listing) in bodies? {
            self.events
                .push(format!("pending resolved IL {label}:\n{listing}"));
            self.events.push(format!(
                "pending owned IL {label}: {}",
                bytes
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            ));
        }

        if cpu.regs[8] != 0 {
            let token = memory.read_int(cpu.regs[8], 8)?;
            self.events.push(format!(
                "pending first managed parameter token={token:#x}; owned-string={}",
                self.object_registry.contains(token)
            ));
        }
        Ok(())
    }
    pub(super) fn handles_metadata(&self, function: &str) -> bool {
        self.metadata && metadata::handles(function)
    }
    pub(super) fn dispatch_metadata(
        &mut self,
        function: &str,
        cpu: &mut Cpu64,
        memory: &mut Memory,
        runtime: &mut Runtime64,
    ) -> Result<bool, String> {
        if self.stopped || !self.handles_metadata(function) {
            return Err("metadata call outside live typed provider".into());
        }
        let native = self
            .native
            .as_ref()
            .ok_or("metadata query has no native runtime")?;
        if native.closed || native.owner != thread::current().id() {
            return Err("metadata query has closed/foreign owner".into());
        }
        let library = native.library;
        let a = [cpu.regs[1], cpu.regs[2], cpu.regs[8], cpu.regs[9]];
        memory.read_int(cpu.regs[4], 8)?;
        cpu.regs[4]
            .checked_add(8)
            .ok_or("metadata return stack overflow")?;
        let targets = self.callback_targets();
        let mut iterator_output = None;
        let value = if matches!(
            function,
            "mono_class_get_userdata" | "mono_class_set_userdata"
        ) {
            metadata::Value::Scalar(self.metadata_registry.userdata(
                memory,
                a[0],
                (function == "mono_class_set_userdata").then_some(a[1]),
            )?)
        } else if function == "mono_get_corlib" {
            let get = symbol!(library, "mono_get_corlib", unsafe extern "C" fn() -> Object);
            let api = match self.image_api {
                Some(api) => api,
                None => {
                    let api = images::Api::load(library)?;
                    self.image_api = Some(api);
                    api
                }
            };
            let (result, _) = callbacks::with_owner(memory, runtime, targets, || {
                let image = unsafe { get() };
                self.image_registry.borrowed_image(
                    api,
                    image,
                    CString::new("mscorlib.dll").unwrap(),
                    CORELIB.into(),
                )
            })?;
            self.metadata_registry.note_query();
            metadata::Value::Scalar(result?)
        } else if matches!(
            function,
            "mono_image_get_name" | "mono_image_get_table_rows"
        ) {
            let image = self.image_registry.native_image(a[0])?;
            let (result, _) = callbacks::with_owner(memory, runtime, targets, || {
                self.metadata_registry
                    .query_image(library, function, image, a[1])
            })?;
            result?
        } else if function == "mono_unity_class_get" {
            let image = self.image_registry.native_image(a[0])?;
            let (result, _) = callbacks::with_owner(memory, runtime, targets, || {
                self.metadata_registry.unity_class_get(library, image, a[1])
            })?;
            result?
        } else if function == "mono_class_get_image" {
            let class = self.metadata_registry.native(a[0], metadata::Kind::Class)?;
            let get = symbol!(
                library,
                "mono_class_get_image",
                unsafe extern "C" fn(Object) -> Object
            );
            let (result, _) = callbacks::with_owner(memory, runtime, targets, || {
                self.image_registry.borrow_registered(unsafe { get(class) })
            })?;
            self.metadata_registry.note_query();
            metadata::Value::Scalar(result?)
        } else if matches!(
            function,
            "mono_class_get_field_from_name" | "mono_class_get_method_from_name"
        ) {
            let class = self.metadata_registry.native(a[0], metadata::Kind::Class)?;
            let name = string(memory, a[1])?;
            let (result, _) = callbacks::with_owner(memory, runtime, targets, || {
                if function == "mono_class_get_field_from_name" {
                    let get = symbol!(
                        library,
                        "mono_class_get_field_from_name",
                        unsafe extern "C" fn(Object, *const c_char) -> Object
                    );
                    Ok::<_, String>(metadata::Value::Handle(
                        unsafe { get(class, name.as_ptr()) },
                        metadata::Kind::Field,
                    ))
                } else {
                    let argc = a[2] as u32 as i32;
                    if !(-1..=16).contains(&argc) {
                        return Err("method lookup arity outside bounded contract".into());
                    }
                    let get = symbol!(
                        library,
                        "mono_class_get_method_from_name",
                        unsafe extern "C" fn(Object, *const c_char, i32) -> Object
                    );
                    Ok(metadata::Value::Handle(
                        unsafe { get(class, name.as_ptr(), argc) },
                        metadata::Kind::Method,
                    ))
                }
            })?;
            self.metadata_registry.note_query();
            result?
        } else if function == "mono_class_from_name" {
            let image = self.image_registry.native_image(a[0])?;
            let namespace = string(memory, a[1])?;
            let name = string(memory, a[2])?;
            let (result, _) = callbacks::with_owner(memory, runtime, targets, || {
                self.metadata_registry
                    .class_from_name(library, image, namespace, name)
            })?;
            result?
        } else if matches!(
            function,
            "mono_class_get_methods"
                | "mono_class_get_fields"
                | "mono_class_get_nested_types"
                | "mono_class_get_interfaces"
                | "mono_signature_get_params"
        ) {
            memory.check(
                a[1],
                8,
                super::super::memory::READ | super::super::memory::WRITE,
            )?;
            let iterator = memory.read_int(a[1], 8)?;
            let (result, _) = callbacks::with_owner(memory, runtime, targets, || {
                self.metadata_registry
                    .iterate(library, function, a[0], iterator)
            })?;
            let (value, token) = result?;
            iterator_output = Some((a[1], token));
            value
        } else {
            let (result, _) = callbacks::with_owner(memory, runtime, targets, || {
                self.metadata_registry
                    .query(library, function, a[0], a[1], a[2])
            })?;
            result?
        };
        let class_result = matches!(&value, metadata::Value::Handle(_, metadata::Kind::Class));
        let result = self.metadata_registry.wrap(memory, value)?;
        if class_result && result != 0 {
            let (published, _) = callbacks::with_owner(memory, runtime, targets, || {
                callbacks::with_service(|memory, _| {
                    self.metadata_registry
                        .publish_class_elements(library, memory, result)
                })
            })?;
            published?;
        }
        if let Some((slot, token)) = iterator_output {
            memory.write_int(slot, 8, token)?;
        }
        cpu.return_integer(memory, result)?;
        if self.metadata_registry.count() <= 32 {
            self.events.push(format!(
                "actual typed metadata boundary {function} -> {result:#x}"
            ));
        }
        Ok(false)
    }
    pub(super) fn handles_image(&self, function: &str) -> bool {
        self.images && images::handles(function)
    }
    pub(super) fn dispatch_image(
        &mut self,
        function: &str,
        cpu: &mut Cpu64,
        memory: &mut Memory,
        runtime: &mut Runtime64,
        fs: &GuestFs,
    ) -> Result<bool, String> {
        if self.stopped || !self.images || !images::handles(function) {
            return Err("managed image call outside live typed provider".into());
        }
        let native = self
            .native
            .as_ref()
            .ok_or("managed image call has no native runtime")?;
        if native.closed || native.owner != thread::current().id() {
            return Err("managed image call on closed/foreign native owner".into());
        }
        let api = match self.image_api {
            Some(api) => api,
            None => {
                let api = images::Api::load(native.library)?;
                self.image_api = Some(api);
                api
            }
        };
        let a = [cpu.regs[1], cpu.regs[2], cpu.regs[8], cpu.regs[9]];
        let rsp = cpu.regs[4];
        memory.read_int(rsp, 8)?;
        rsp.checked_add(8).ok_or("image return slot overflow")?;
        let mut output_status = None;
        let result = match function {
            "mono_image_open_from_data_with_name" => {
                let len = a[1] as u32 as usize;
                if len == 0
                    || len > 64 * 1024 * 1024
                    || a[2] as u32 != 1
                    || memory.read_int(rsp.checked_add(40).ok_or("image stack overflow")?, 8)?
                        as u32
                        != 0
                {
                    return Err("managed image needs bounded copy and non-refonly input".into());
                }
                memory.check(a[3], 4, super::super::memory::WRITE)?;
                let name = string(
                    memory,
                    memory.read_int(rsp.checked_add(48).ok_or("image name stack overflow")?, 8)?,
                )?;
                let host = host_path(fs, &name)?;
                let mut bytes = vec![0; len];
                memory.read(a[0], &mut bytes)?;
                let disk = std::fs::read(Path::new(host.to_str().map_err(|e| e.to_string())?))
                    .map_err(|e| format!("managed image source: {e}"))?;
                if bytes != disk {
                    return Err("managed image bytes differ from resolved immutable source".into());
                }
                let hash = format!("{:x}", Sha256::digest(&bytes));
                let targets = self.callback_targets();
                let (value, _) = callbacks::with_owner(memory, runtime, targets, || {
                    self.image_registry.open(api, bytes, host, hash)
                })?;
                let (token, status) = value?;
                output_status = Some((a[3], status));
                token
            }
            "mono_assembly_load_from_full" => {
                if a[3] as u32 != 0 {
                    return Err("refonly assembly load not admitted".into());
                }
                memory.check(a[2], 4, super::super::memory::WRITE)?;
                let name = host_path(fs, &string(memory, a[1])?)?;
                let targets = self.callback_targets();
                let (value, _) = callbacks::with_owner(memory, runtime, targets, || {
                    self.image_registry.load_assembly(api, a[0], name)
                })?;
                let (token, status) = value?;
                output_status = Some((a[2], status));
                token
            }
            "mono_image_close" => {
                let targets = self.callback_targets();
                let (value, _) = callbacks::with_owner(memory, runtime, targets, || {
                    self.image_registry.close(api, a[0])
                })?;
                value?;
                0
            }
            "mono_assembly_get_image" => {
                let targets = self.callback_targets();
                let (value, _) = callbacks::with_owner(memory, runtime, targets, || {
                    self.image_registry.assembly_image(api, a[0])
                })?;
                value?
            }
            _ => return Err("untyped managed image operation".into()),
        };
        if let Some((address, status)) = output_status {
            memory.write_int(address, 4, status as u32 as u64)?;
        }
        cpu.return_integer(memory, result)?;
        self.events.push(format!(
            "actual typed managed image boundary {function} -> {result:#x}"
        ));
        Ok(false)
    }
    pub(super) fn images_tsv(&self) -> String {
        self.image_registry.tsv()
    }
    pub(super) fn validate_scalar_callbacks(
        &mut self,
        memory: &mut Memory,
        runtime: &mut Runtime64,
    ) -> Result<(), String> {
        if let Some(native) = &mut self.native {
            native.gc.restore()?;
        }
        if !self.bindings_ready {
            return Err("scalar verification needs a sealed actual registry".into());
        }
        let native = self
            .native
            .as_ref()
            .ok_or("scalar verification has no native runtime")?;
        if native.closed || native.owner != thread::current().id() {
            return Err("scalar verification runtime is closed/foreign".into());
        }
        let lib = native.library;
        let fs = runtime.filesystem()?;
        let path = fs
            .resolve("C:\\AliceToPapaADV_1-0-0_Data\\Managed\\UnityEngine.CoreModule.dll")
            .map_err(|e| format!("CoreModule path: {e}"))?;
        fingerprint(
            &path,
            CORE_MODULE,
            "UnityEngine.CoreModule before scalar execution",
        )?;
        let path = CString::new(path.as_os_str().as_encoded_bytes()).map_err(|e| e.to_string())?;
        let open = symbol!(
            lib,
            "mono_domain_assembly_open",
            unsafe extern "C" fn(Object, *const c_char) -> Object
        );
        let image = symbol!(
            lib,
            "mono_assembly_get_image",
            unsafe extern "C" fn(Object) -> Object
        );
        let class = symbol!(
            lib,
            "mono_class_from_name",
            unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object
        );
        let method = symbol!(
            lib,
            "mono_class_get_method_from_name",
            unsafe extern "C" fn(Object, *const c_char, i32) -> Object
        );
        let flags = symbol!(
            lib,
            "mono_method_get_flags",
            unsafe extern "C" fn(Object, *mut u32) -> u32
        );
        let signature = symbol!(
            lib,
            "mono_method_signature",
            unsafe extern "C" fn(Object) -> Object
        );
        let count = symbol!(
            lib,
            "mono_signature_get_param_count",
            unsafe extern "C" fn(Object) -> u32
        );
        let returns = symbol!(
            lib,
            "mono_signature_get_return_type",
            unsafe extern "C" fn(Object) -> Object
        );
        let instance = symbol!(
            lib,
            "mono_signature_is_instance",
            unsafe extern "C" fn(Object) -> i32
        );
        let kind = symbol!(
            lib,
            "mono_type_get_type",
            unsafe extern "C" fn(Object) -> i32
        );
        let byref = symbol!(
            lib,
            "mono_type_is_byref",
            unsafe extern "C" fn(Object) -> i32
        );
        let invoke = symbol!(
            lib,
            "mono_runtime_invoke",
            unsafe extern "C" fn(Object, Object, *mut Object, *mut Object) -> Object
        );
        let unbox = symbol!(
            lib,
            "mono_object_unbox",
            unsafe extern "C" fn(Object) -> Object
        );
        let (result, stats) = callbacks::with_owner(
            memory,
            runtime,
            self.callback_targets(),
            || -> Result<usize, String> {
                let assembly = unsafe { open(native.domain, path.as_ptr()) };
                if assembly.is_null() {
                    return Err("native Unity CoreModule metadata load failed".into());
                }
                let image = unsafe { image(assembly) };
                if image.is_null() {
                    return Err("native Unity CoreModule has no image".into());
                }
                let mut methods = [ptr::null_mut(); 2];
                for (slot, name) in bindings::SCALAR_NAMES.iter().enumerate() {
                    let (type_name, method_name) =
                        name.split_once("::").ok_or("invalid scalar name")?;
                    let (namespace, type_name) =
                        type_name.rsplit_once('.').ok_or("invalid scalar type")?;
                    let namespace = CString::new(namespace).map_err(|e| e.to_string())?;
                    let type_name = CString::new(type_name).map_err(|e| e.to_string())?;
                    let class = unsafe { class(image, namespace.as_ptr(), type_name.as_ptr()) };
                    if class.is_null()
                        || !unsafe { method(class, c".cctor".as_ptr(), -1) }.is_null()
                    {
                        return Err(
                            "scalar diagnostic class missing or initializer not admitted".into(),
                        );
                    }
                    let name = CString::new(method_name).map_err(|e| e.to_string())?;
                    let argc = 0;
                    let m = unsafe { method(class, name.as_ptr(), argc as i32) };
                    if m.is_null() {
                        return Err(format!("scalar method missing: {name:?}"));
                    }
                    let mut implementation = 0;
                    let method_flags = unsafe { flags(m, &mut implementation) };
                    let sig = unsafe { signature(m) };
                    if sig.is_null()
                        || method_flags & 0x10 == 0
                        || implementation & 0x1000 == 0
                        || unsafe { instance(sig) } != 0
                        || unsafe { count(sig) } != argc
                    {
                        return Err(format!(
                            "scalar static InternalCall signature mismatch: {name:?}"
                        ));
                    }
                    let ret = unsafe { returns(sig) };
                    let expected = 2; // BOOLEAN.
                    if ret.is_null()
                        || unsafe { kind(ret) } != expected
                        || unsafe { byref(ret) } != 0
                    {
                        return Err("scalar return type/byref mismatch".into());
                    }
                    methods[slot] = m;
                }
                let mut verified = 0;
                for (slot, m) in methods.into_iter().enumerate() {
                    let reference = callbacks::reference(slot, &[])?;
                    let mut exception = ptr::null_mut();
                    let result =
                        unsafe { invoke(m, ptr::null_mut(), ptr::null_mut(), &mut exception) };
                    if !exception.is_null() {
                        return Err(format!(
                            "actual Unity scalar invocation threw at slot {slot}"
                        ));
                    }
                    if slot < 2 {
                        if result.is_null() {
                            return Err("actual Unity bool invocation did not box result".into());
                        }
                        let data = unsafe { unbox(result) };
                        if data.is_null()
                            || unsafe { data.cast::<u8>().read() } != u8::from(reference as u8 != 0)
                        {
                            return Err(
                                "actual Unity bool result differs from original Win64 reference"
                                    .into(),
                            );
                        }
                    } else if !result.is_null() {
                        return Err("actual Unity void invocation returned a value".into());
                    }
                    verified += 1;
                }
                Ok(verified)
            },
        )?;
        let verified = result?;
        if stats.calls[..2] != [2, 2] || verified != 2 {
            return Err("actual Unity scalar callback/reference counts mismatch".into());
        }
        self.scalar_callbacks_verified = verified;
        self.events.push("actual native Unity CoreModule -> 2 fixed ARM64 scalar adapters -> original Win64 targets verified against 2 reference calls; checked Boolean signatures, same-thread only; full game admission remains closed".into());
        Ok(())
    }
    pub(super) fn validate_profiler_callbacks(
        &mut self,
        memory: &mut Memory,
        runtime: &mut Runtime64,
    ) -> Result<(), String> {
        if let Some(native) = &mut self.native {
            native.gc.restore()?;
        }
        if !self.profiler_ready {
            return Ok(());
        }
        let native = self
            .native
            .as_ref()
            .ok_or("profiler probe has no runtime")?;
        let collect = symbol!(native.library, "mono_gc_collect", unsafe extern "C" fn(i32));
        let max = symbol!(
            native.library,
            "mono_gc_max_generation",
            unsafe extern "C" fn() -> i32
        );
        let (collected, stats) =
            callbacks::with_owner(memory, runtime, self.callback_targets(), || {
                if let Some(api) = self.instance_api {
                    callbacks::with_service(|memory, _| {
                        self.instance_registry.sync_from_guest(api, memory)?;
                        self.field_registry.sync(api, &self.instance_registry)?;
                        if let Some(array_api) = self.reference_array_api {
                            self.reference_array_registry.sync(
                                array_api,
                                api,
                                &self.instance_registry,
                                memory,
                            )?;
                        }
                        Ok::<_, String>(())
                    })?;
                }
                unsafe {
                    collect(max());
                }
                Ok::<_, String>(())
            })?;
        collected?;
        let required =
            (1 << 0) | (1 << 5) | (1 << 6) | (1 << 7) | (1 << 8) | (1 << 9) | (1 << 10) | (1 << 11);
        if stats.calls[3] < 6 || stats.gc_events & required != required {
            return Err(format!(
                "actual native GC profiler cycle incomplete: calls={}, events={:#x}",
                stats.calls[3], stats.gc_events
            ));
        }
        self.gc_events_verified |= stats.gc_events;
        if let Some(api) = self.object_api {
            let (verified, _) =
                callbacks::with_owner(memory, runtime, self.callback_targets(), || {
                    self.object_registry.verify(api)
                })?;
            verified?;
        }
        if let Some(api) = self.reflection_api {
            let (verified, _) =
                callbacks::with_owner(memory, runtime, self.callback_targets(), || {
                    self.reflection_registry.verify(api)
                })?;
            verified?;
        }
        if let Some(api) = self.handle_api {
            let (verified, _) =
                callbacks::with_owner(memory, runtime, self.callback_targets(), || {
                    self.handle_registry.verify(api)
                })?;
            verified?;
        }
        if let Some(api) = self.array_api {
            let (verified, _) =
                callbacks::with_owner(memory, runtime, self.callback_targets(), || {
                    self.array_registry.verify(api)
                })?;
            verified?;
        }
        if let Some(api) = self.instance_api {
            let (verified, _) =
                callbacks::with_owner(memory, runtime, self.callback_targets(), || {
                    self.instance_registry.verify(api)?;
                    self.field_registry.verify(api, &self.instance_registry)?;
                    if let Some(array_api) = self.reference_array_api {
                        self.reference_array_registry.verify(
                            array_api,
                            api,
                            &self.instance_registry,
                        )?;
                    }
                    Ok::<_, String>(())
                })?;
            verified?;
        }
        if self.curves_constructor.is_some() {
            let (verified, _) =
                callbacks::with_owner(memory, runtime, self.callback_targets(), || {
                    self.curves_constructor.as_mut().unwrap().verify()
                })?;
            verified?;
        }
        if let Some(api) = self.attribute_api {
            let (verified, _) =
                callbacks::with_owner(memory, runtime, self.callback_targets(), || {
                    self.attribute_registry.verify(api)
                })?;
            verified?;
        }
        self.events.push(format!("actual native BDWGC collection -> compiled GC adapter -> original Unity Win64 callback: {} calls, event mask {:#x}; no guest pointer passed to native profiler", stats.calls[3], stats.gc_events));
        Ok(())
    }
    pub(super) fn validate_unitytls(
        &mut self,
        memory: &mut Memory,
        runtime: &mut Runtime64,
    ) -> Result<(), String> {
        let Some(table) = self.tls_table.as_ref() else {
            return Ok(());
        };
        let native = self
            .native
            .as_ref()
            .ok_or("UnityTLS verification has no runtime")?;
        let get = symbol!(
            native.library,
            "mono_unity_get_unitytls_interface",
            unsafe extern "C" fn() -> *const c_void
        );
        if unsafe { get() } != table.pointer() {
            return Err("native UnityTLS getter did not retain owned table".into());
        }
        let (result, stats) =
            callbacks::with_owner(memory, runtime, self.callback_targets(), || unsafe {
                table.validate_leaf()
            })?;
        result?;
        if stats.calls[5..] != [2, 6] {
            return Err("UnityTLS actual adapter/reference counts differ".into());
        }
        self.tls_leaf_verified = true;
        self.events.push("native UnityTLS interface create16B hidden-sret and ref error-state match original Win64 callbacks; no networking admitted".into());
        Ok(())
    }
    pub(super) fn unitytls_tsv(&self) -> Option<String> {
        self.tls_table.as_ref().map(|table| table.tsv())
    }
    pub(super) fn finish_owned(
        &mut self,
        memory: &mut Memory,
        runtime: &mut Runtime64,
    ) -> Result<(), String> {
        if !self.legacy_installed || self.stopped {
            let result = self.finish();
            self.metadata_registry.retire(memory)?;
            self.object_registry.retire_views(memory)?;
            self.reflection_registry.retire_views(memory)?;
            self.array_registry.retire(memory)?;
            self.attribute_registry.retire_views(memory)?;
            self.instance_registry.retire_views(memory)?;
            self.field_registry
                .retire(memory, &self.instance_registry)?;
            self.reference_array_registry
                .retire(memory, &self.instance_registry)?;
            self.handle_registry.retire_views(memory)?;
            self.error_registry.retire();
            return result;
        }
        let targets = self.callback_targets();
        let (result, stats) = callbacks::with_owner(memory, runtime, targets, || self.finish())?;
        result?;
        self.metadata_registry.retire(memory)?;
        self.object_registry.retire_views(memory)?;
        self.reflection_registry.retire_views(memory)?;
        self.array_registry.retire(memory)?;
        self.attribute_registry.retire_views(memory)?;
        self.instance_registry.retire_views(memory)?;
        self.field_registry
            .retire(memory, &self.instance_registry)?;
        self.reference_array_registry
            .retire(memory, &self.instance_registry)?;
        self.handle_registry.retire_views(memory)?;
        self.error_registry.retire();
        self.shutdown_callbacks += stats.calls[2];
        self.gc_events_verified |= stats.gc_events;
        if stats.calls[2] != 1 {
            return Err("native cleanup did not invoke exact legacy shutdown callback once".into());
        }
        self.events.push(format!(
            "owner-scoped native cleanup interpreted legacy shutdown {} time; GC callbacks={}",
            stats.calls[2], stats.calls[3]
        ));
        Ok(())
    }
    pub(super) fn bindings_tsv(&self) -> String {
        self.registry.tsv()
    }
    pub(super) fn finish(&mut self) -> Result<(), String> {
        if self.stopped {
            return Ok(());
        }
        if let Some(native) = &mut self.native {
            native.gc.restore()?;
            native.workers.verify_gc()?;
            native.workers.close()?;
        }
        if let Some(constructor) = &mut self.curves_constructor {
            constructor.cleanup()?;
        }
        profiler_samples::verify_balanced()?;
        self.stopped = true;
        if let Some(api) = self.handle_api {
            self.handle_registry.release(api);
        }
        if let Some(api) = self.instance_api {
            self.instance_registry.release(api);
        }
        if let Some(api) = self.object_api {
            self.object_registry.release_roots(api);
        }
        if let Some(api) = self.reflection_api {
            self.reflection_registry.release_roots(api);
        }
        if let Some(api) = self.array_api {
            self.array_registry.release(api);
        }
        if let Some(api) = self.attribute_api {
            self.attribute_registry.release(api);
        }
        if let Some(api) = self.name_api {
            self.name_registry.release(api);
        }
        if let Some(api) = self.image_api {
            self.image_registry.retire(api);
        }
        if let Some(native) = &mut self.native {
            native.cleanup()?;
            // Raw native IL pointers remain owned until the VM can no longer
            // execute IL/finalizers. Only then release original guest records.
            unsafe_buffers::retire_after_vm_cleanup()?;
            binding_strings::retire_after_vm_cleanup()?;
            profiler_recorders::retire_after_vm_cleanup()?;
            burst_shared::retire_after_vm_cleanup()?;
            if self.tls_table.is_some() {
                let clear = symbol!(
                    native.library,
                    "mono_unity_install_unitytls_interface",
                    unsafe extern "C" fn(*const c_void)
                );
                unsafe {
                    clear(ptr::null());
                }
            }
        }
        self.events
            .push("native startup probe closed on owner thread; guest resume prohibited".into());
        Ok(())
    }
    pub(super) fn report(&self) -> String {
        format!(
            "native-root-ready={}\nnative-main-thread-ready={}\nnative-pre-bindings-ready={}\nnative-bindings-ready={}\nprovider-bindings={}\nnative-scalar-adapters={}\nactual-unity-scalar-callbacks-verified={}\nnative-profiler-ready={}\nactual-gc-event-mask={}\nactual-shutdown-callbacks={}\nnative-pre-images-ready={}\nactual-unitytls-leaf-verified={}\nnative-images-ready={}\nnative-metadata-ready={}\nnative-strings-ready={}\nnative-managed-types-ready={}\nnative-managed-type-invokes={}\nnative-classlib-ready={}\nnative-classlib-invokes={}\nnative-logging-ready={}\nnative-exceptions-ready={}\nnative-synchronization-ready={}\nnative-subsystems-ready={}\nnative-subsystem-map-ready={}\nnative-imgui-ready={}\nnative-attributes-ready={}\nnative-attribute-arrays-ready={}\nnative-assembly-lookup-ready={}\nnative-ordinary-objects-ready={}\nfull-game-admission=false\n{}{}{}{}{}{}{}{}{}{}{}{}\n",
            self.root_ready,
            self.main_thread_ready,
            self.pre_bindings_ready,
            self.bindings_ready,
            self.registry.len(),
            self.registry.scalar_targets.iter().filter(|target| **target != 0).count(),
            self.scalar_callbacks_verified,
            self.profiler_ready,
            self.gc_events_verified,
            self.shutdown_callbacks,
            self.pre_images_ready,
            self.tls_leaf_verified,
            self.images_ready,
            self.metadata_ready,
            self.strings_ready,
            self.managed_types_ready,
            self.managed_type_invokes,
            self.classlib_ready,
            self.classlib_invokes,
            self.logging_ready,
            self.exceptions_ready,
            self.synchronization_ready,
            self.subsystems_ready,
            self.subsystem_map_ready,
            self.imgui_ready,
            self.attributes_ready,
            self.attribute_arrays_ready,
            self.assembly_lookup_ready,
            self.ordinary_objects_ready,
            self.curves_constructor.as_ref().map_or_else(String::new, animation::Constructor::report),
            self.instance_registry.report() + &self.field_registry.report() + &format!("native-component-constructors={}\nnative-input-asset-constructors={}\nnative-virtual-method-resolutions={}\nnative-input-map-constructors={}\nnative-input-action-constructors={}\nnative-serialization-callback-invokes={}\n", self.component_invokes, self.asset_invokes, self.virtual_resolutions, self.input_map_invokes, self.input_action_invokes, self.serialization_invokes),
            self.name_registry.report(),
            self.error_registry.report(),
            self.attribute_registry.report(),
            self.handle_registry.report(),
            self.array_registry.report() + &self.reference_array_registry.report(),
            self.reflection_registry.report(),
            self.object_registry.report(),
            self.image_registry.report(),
            self.metadata_registry.report(),
            self.events.join("\n")
        ) + "\n" + &self.native.as_ref().map_or_else(String::new, |n| n.gc.report())
            + &format!("native-static-managed-invokes={}\nnative-static-byref-output-writes={}\n", self.static_invokes, self.static_output_writes)
            + &format!("native-instance-managed-invokes={}\nnative-instance-byref-output-writes={}\n", self.instance_invokes, self.instance_output_writes)
            + &subsystems::report()
            + &resources::report()
            + &scriptable::report()
            + &engine_scalars::report()
            + &json::report()
            + &profiler_samples::report()
            + &profiler_recorders::report()
            + &engine_objects::report()
            + &unsafe_buffers::report()
            + &logging::debug_report()
            + &burst_shared::report()
            + &jobs::report()
            + &self.native.as_ref().map_or_else(String::new, |n| n.workers.report())
    }
    pub(super) fn stopped(&self) -> bool {
        self.stopped
            || (self.root_ready && !self.main_thread)
            || (self.main_thread_ready && !self.pre_bindings)
            || (self.pre_bindings_ready && !self.bindings)
            || (self.bindings_ready && !self.profiler)
            || (self.profiler_ready && !self.pre_images)
            || (self.pre_images_ready && !self.images)
            || (self.images_ready && !self.metadata)
            || (self.metadata_ready && !self.strings)
            || (self.strings_ready && !self.managed_types)
            || (self.managed_types_ready && !self.classlib)
            || (self.classlib_ready && !self.logging)
            || (self.logging_ready && !self.exceptions)
            || (self.exceptions_ready && !self.synchronization)
            || (self.synchronization_ready && !self.subsystems)
            || (self.subsystems_ready && !self.subsystem_map)
            || (self.subsystem_map_ready && !self.imgui)
            || (self.imgui_ready && !self.attributes)
            || (self.attributes_ready && !self.attribute_arrays)
            || (self.attribute_arrays_ready && !self.assembly_lookup)
            || self.assembly_lookup_ready
    }
    pub(super) fn shutdown_complete(&self) -> bool {
        self.stopped
    }
}
fn store_stack_token(
    memory: &mut Memory,
    cpu: &Cpu64,
    a: [u64; 4],
    owned: u64,
) -> Result<(), String> {
    // Registry-validated strong identities stay rooted through finite cleanup.
    // This guest stack scalar write never crosses native FFI or admits heap
    // destinations, arbitrary object references, or interior managed byrefs.
    let first = cpu.regs[4]
        .checked_add(40)
        .ok_or("startup stack overflow")?;
    let end = a[1].checked_add(8).ok_or("startup root slot overflow")?;
    if a[0] != 0
        || (a[2] != 0 && a[2] != owned)
        || !a[1].is_multiple_of(8)
        || a[1] < first
        || a[1] < super::STACK
        || end > super::STACK + 64 * 1024 * 1024
    {
        return Err(
            "startup barrier admits only an owned strong-rooted token in a live main-stack slot"
                .into(),
        );
    }
    memory.write_int(a[1], 8, a[2])
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
        for (args, owned) in [
            ([0, slot, THREAD_TOKEN, 0], 0),
            ([1, slot, THREAD_TOKEN, 0], THREAD_TOKEN),
            ([0, slot, 0x1234, 0], THREAD_TOKEN),
            ([0, cpu.regs[4], THREAD_TOKEN, 0], THREAD_TOKEN),
            ([0, 0x1234, THREAD_TOKEN, 0], THREAD_TOKEN),
            ([0, u64::MAX - 7, THREAD_TOKEN, 0], THREAD_TOKEN),
            ([0, slot + 1, THREAD_TOKEN, 0], THREAD_TOKEN),
        ] {
            assert!(store_stack_token(&mut memory, &cpu, args, owned).is_err());
            assert_eq!(memory.read_int(slot, 8).unwrap(), 77);
        }
        store_stack_token(&mut memory, &cpu, [0, slot, THREAD_TOKEN, 0], THREAD_TOKEN).unwrap();
        assert_eq!(memory.read_int(slot, 8).unwrap(), THREAD_TOKEN);
        store_stack_token(&mut memory, &cpu, [0, slot, 0, 0], THREAD_TOKEN).unwrap();
        assert_eq!(memory.read_int(slot, 8).unwrap(), 0);
        store_stack_token(&mut memory, &cpu, [0, slot, THREAD_TOKEN, 0], THREAD_TOKEN).unwrap();
        memory.protect(super::super::STACK, 4096, READ).unwrap();
        assert!(
            store_stack_token(&mut memory, &cpu, [0, slot, THREAD_TOKEN, 0], THREAD_TOKEN).is_err()
        );
        assert_eq!(memory.read_int(slot, 8).unwrap(), THREAD_TOKEN);
    }
    #[test]
    fn exact_allocator_return_never_reads_guest_table_and_closed_probe_rejects_resume() {
        let mut probe = Probe {
            library: PathBuf::new(),
            library_hash: CANDIDATE,
            native: None,
            replaced: true,
            root_ready: false,
            main_thread: false,
            main_thread_ready: false,
            pre_bindings: false,
            pre_bindings_ready: false,
            bindings: false,
            bindings_ready: false,
            scalar_callbacks_verified: 0,
            profiler: false,
            profiler_ready: false,
            profiler_targets: [0; 3],
            legacy_installed: false,
            gc_events_verified: 0,
            shutdown_callbacks: 0,
            pre_images: false,
            pre_images_ready: false,
            tls_table: None,
            tls_leaf_verified: false,
            images: false,
            images_ready: false,
            image_registry: images::Registry::default(),
            image_api: None,
            metadata: false,
            metadata_ready: false,
            metadata_registry: metadata::Registry::default(),
            strings: false,
            strings_ready: false,
            object_registry: objects::Registry::default(),
            object_api: None,
            managed_types: false,
            managed_types_ready: false,
            get_type_method: 0,
            managed_type_invokes: 0,
            reflection_registry: reflection::Registry::default(),
            handle_api: None,
            handle_registry: handles::Registry::default(),
            array_api: None,
            array_registry: arrays::Registry::default(),
            reflection_api: None,
            logging: false,
            logging_ready: false,
            exceptions: false,
            exceptions_ready: false,
            assets: false,
            asset_invokes: 0,
            input_map_invokes: 0,
            input_action_invokes: 0,
            virtual_resolutions: 0,
            serialization_invokes: 0,
            static_invokes: 0,
            static_output_writes: 0,
            instance_invokes: 0,
            instance_output_writes: 0,
            reference_array_api: None,
            reference_array_registry: reference_arrays::Registry::default(),
            components: false,
            component_invokes: 0,
            fields: false,
            field_registry: fields::Registry::default(),
            curves: false,
            curves_constructor: None,
            ordinary_objects: false,
            ordinary_objects_ready: false,
            instance_api: None,
            instance_registry: instances::Registry::default(),
            assembly_lookup: false,
            assembly_lookup_ready: false,
            name_api: None,
            name_registry: names::Registry::default(),
            attribute_arrays: false,
            attribute_arrays_ready: false,
            error_registry: errors::Registry::default(),
            attributes: false,
            attributes_ready: false,
            attribute_api: None,
            attribute_registry: attributes::Registry::default(),
            imgui: false,
            imgui_ready: false,
            subsystem_map: false,
            subsystem_map_ready: false,
            subsystems: false,
            subsystems_ready: false,
            synchronization: false,
            synchronization_ready: false,
            classlib: false,
            classlib_ready: false,
            classlib_invokes: 0,
            registry: bindings::Registry::default(),
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
        let mut runtime_memory = Memory::default();
        runtime_memory
            .map(super::super::STACK, 65536, READ | WRITE)
            .unwrap();
        let runtime = Runtime64::new(
            &mut runtime_memory,
            0x140000000,
            super::super::STACK,
            65536,
            "dispatch",
        )
        .unwrap();
        let fs = GuestFs::new(&std::env::current_dir().unwrap()).unwrap();
        assert!(!probe
            .dispatch(
                "mono_unity_install_memory_callbacks",
                &mut cpu,
                &mut memory,
                &fs,
                &runtime
            )
            .unwrap());
        assert_eq!(cpu.rip, 0x12345);
        assert_eq!(cpu.regs[0], 0xabcdef);
        assert!(probe.native.is_none());
        probe.assembly_lookup_ready = true;
        assert!(probe.stopped());
        assert!(!probe.shutdown_complete());
        probe.finish().unwrap();
        assert!(probe.shutdown_complete());
        assert!(probe
            .dispatch(
                "mono_unity_install_memory_callbacks",
                &mut cpu,
                &mut memory,
                &fs,
                &runtime
            )
            .is_err());
    }
}
