//! Optional metadata/registration audit. Look up declared icalls without
//! executing them, creating wrappers, resolving PInvokes or publishing code.
use super::{bindings, Object};
use std::{ffi::CStr, path::PathBuf, ptr, sync::OnceLock};
static OUTPUT: OnceLock<Option<PathBuf>> = OnceLock::new();
static COMPLETE: OnceLock<()> = OnceLock::new();
pub(super) fn requested() -> bool {
    OUTPUT
        .get_or_init(|| std::env::var_os("WIRE_NATIVE_MONO_BRIDGE_AUDIT").map(PathBuf::from))
        .is_some()
        && COMPLETE.get().is_none()
}
fn text(value: *const libc::c_char) -> Result<String, String> {
    if value.is_null() {
        return Err("bridge audit metadata string absent".into());
    }
    Ok(unsafe { CStr::from_ptr(value) }
        .to_str()
        .map_err(|e| e.to_string())?
        .replace('\t', "\\t")
        .replace('\r', "\\r")
        .replace('\n', "\\n"))
}
pub(super) fn run_once(
    library: Object,
    images: &[Object],
    registry: &bindings::Registry,
    image_evidence: &str,
) -> Result<(), String> {
    let output =
        OUTPUT.get_or_init(|| std::env::var_os("WIRE_NATIVE_MONO_BRIDGE_AUDIT").map(PathBuf::from));
    let Some(output) = output else {
        return Ok(());
    };
    if COMPLETE.get().is_some() {
        return Ok(());
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("wire-out")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !output.is_absolute()
        || !output.starts_with(&root)
        || output
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err("bridge audit output must remain under wire-out".into());
    }
    std::fs::create_dir_all(output).map_err(|e| e.to_string())?;
    let output = output.canonicalize().map_err(|e| e.to_string())?;
    if !output.starts_with(root) {
        return Err("bridge audit output must remain under wire-out".into());
    }
    macro_rules! sym {
        ($name:literal, $ty:ty) => {{
            let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
            if p.is_null() {
                return Err(concat!("missing bridge audit API: ", $name).into());
            }
            unsafe { std::mem::transmute::<Object, $ty>(p) }
        }};
    }
    let table = sym!(
        "mono_image_get_table_info",
        unsafe extern "C" fn(Object, i32) -> Object
    );
    let rows = sym!(
        "mono_table_info_get_rows",
        unsafe extern "C" fn(Object) -> i32
    );
    let decode = sym!(
        "mono_metadata_decode_row",
        unsafe extern "C" fn(Object, i32, *mut u32, i32)
    );
    let get = sym!(
        "mono_get_method",
        unsafe extern "C" fn(Object, u32, Object) -> Object
    );
    let lookup = sym!(
        "mono_lookup_internal_call",
        unsafe extern "C" fn(Object) -> Object
    );
    let image_name = sym!(
        "mono_image_get_filename",
        unsafe extern "C" fn(Object) -> *const libc::c_char
    );
    let method_name = sym!(
        "mono_method_full_name",
        unsafe extern "C" fn(Object, i32) -> *mut libc::c_char
    );
    let free = sym!("mono_free", unsafe extern "C" fn(Object));
    let mut report = String::from("assembly\ttoken\tkind\tlookup\tfull_signature\n");
    let mut total = 0;
    let mut missing = 0;
    for &image in images {
        let filename = text(unsafe { image_name(image) })?;
        let filename = PathBuf::from(filename);
        let filename = filename
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or("bridge audit image filename absent")?;
        let table = unsafe { table(image, 6) };
        if table.is_null() {
            return Err("bridge audit method table absent".into());
        }
        let count = unsafe { rows(table) };
        if !(0..=1_000_000).contains(&count) {
            return Err("bridge audit method count exceeds bound".into());
        }
        for index in 0..count {
            let mut data = [0; 6];
            unsafe {
                decode(table, index, data.as_mut_ptr(), 6);
            }
            let icall = data[1] & 0x1000 != 0;
            let pinvoke = data[2] & 0x2000 != 0;
            if !icall && !pinvoke {
                continue;
            }
            let token = 0x06000000 | (index as u32 + 1);
            let method = unsafe { get(image, token, ptr::null_mut()) };
            let (status, signature) = if method.is_null() {
                ("unresolved_metadata", String::new())
            } else {
                let name = unsafe { method_name(method, 1) };
                let signature = text(name);
                if !name.is_null() {
                    unsafe { free(name.cast()) };
                }
                let status = if pinvoke {
                    // Resolving a PInvoke could load a native library. Inventory
                    // its declaration separately without doing that here.
                    "not_resolved_by_audit"
                } else if unsafe { lookup(method) }.is_null() {
                    missing += 1;
                    "missing"
                } else {
                    "resolved"
                };
                (status, signature?)
            };
            total += 1;
            report.push_str(&format!(
                "{filename}\t{token:#010x}\t{}\t{status}\t{signature}\n",
                if icall { "InternalCall" } else { "PInvoke" }
            ));
        }
    }
    std::fs::write(output.join("runtime-lookup.tsv"), report).map_err(|e| e.to_string())?;
    std::fs::write(output.join("engine-registrations.tsv"), registry.tsv())
        .map_err(|e| e.to_string())?;
    std::fs::write(output.join("runtime-images.tsv"), image_evidence).map_err(|e| e.to_string())?;
    COMPLETE
        .set(())
        .map_err(|_| "bridge audit already completed")?;
    eprintln!("NATIVE_MONO_BRIDGE_AUDIT: images={}; declarations={total}; missing_icalls={missing}; game_methods_executed=0", images.len());
    Ok(())
}
