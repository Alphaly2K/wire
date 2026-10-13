//! Keep engine pointers as guest data while the original managed IL executes.
use super::{callbacks, managed_calls, Memory, Object};
use std::{cell::Cell, sync::OnceLock};

struct Binding {
    method: usize,
    owner: usize,
    offset: usize,
    vtable: unsafe extern "C" fn(Object, Object) -> Object,
    domain: unsafe extern "C" fn() -> Object,
    get: unsafe extern "C" fn(Object, Object, Object),
}
static BINDING: OnceLock<Vec<Binding>> = OnceLock::new();
thread_local! { static READS: Cell<usize> = const { Cell::new(0) }; }
pub(super) fn installed() -> bool {
    BINDING.get().is_some()
}

pub(super) fn install(library: Object, core: Object, images: &[Object]) -> Result<(), String> {
    macro_rules! sym {
        ($name:literal, $ty:ty) => {{
            let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
            if p.is_null() {
                return Err(concat!("missing engine header API: ", $name).into());
            }
            unsafe { std::mem::transmute::<Object, $ty>(p) }
        }};
    }
    let find = sym!(
        "mono_class_from_name",
        unsafe extern "C" fn(Object, *const super::c_char, *const super::c_char) -> Object
    );
    let method = sym!(
        "mono_class_get_method_from_name",
        unsafe extern "C" fn(Object, *const super::c_char, i32) -> Object
    );
    let owner = unsafe { find(core, c"UnityEngine".as_ptr(), c"Object".as_ptr()) };
    if owner.is_null() {
        return Err("engine header base class absent".into());
    }
    let register = sym!(
        "wire_mono_install_scalar_read_adapter",
        unsafe extern "C" fn(
            u32,
            Object,
            unsafe extern "C" fn(Object, Object, u64, *mut i32, usize) -> i32,
        ) -> i32
    );
    let vtable = sym!(
        "mono_class_vtable",
        unsafe extern "C" fn(Object, Object) -> Object
    );
    let domain = sym!("mono_domain_get", unsafe extern "C" fn() -> Object);
    let get = sym!(
        "mono_field_static_get_value",
        unsafe extern "C" fn(Object, Object, Object)
    );
    let api = managed_calls::Api::load(library)?;
    let mut bindings = Vec::new();
    for name in [c"GetInstanceID", c"GetEntityId"] {
        let getter = unsafe { method(owner, name.as_ptr(), 0) };
        if getter.is_null() && name == c"GetEntityId" {
            continue; // Older CoreModule versions have no EntityId getter.
        }
        let managed_calls::HeaderRead {
            method,
            owner,
            offset,
        } = api.engine_header_read(getter, owner, images)?;
        bindings.push(Binding {
            method: method as usize,
            owner: owner as usize,
            offset: offset as usize,
            vtable,
            domain,
            get,
        });
    }
    BINDING
        .set(bindings)
        .map_err(|_| "engine header read already installed")?;
    let bindings = BINDING.get().unwrap();
    for binding in bindings {
        let status = unsafe { register(2, binding.method as Object, read) };
        if status != 0 {
            return Err(format!(
                "engine header method read registration rejected: {status}"
            ));
        }
    }
    eprintln!("NATIVE_MONO_ENGINE_HEADER_READ_INSTALLED: original-managed-IL=true; actual-offset-field=true; methods={}", bindings.len());
    Ok(())
}

fn checked_read(
    memory: &Memory,
    pointer: u64,
    address: u64,
    offset: i32,
    width: usize,
    live: impl FnOnce(u64, u64) -> bool,
) -> Result<i32, String> {
    if pointer == 0
        || !pointer.is_multiple_of(8)
        || width != 4
        || !(0..=65532).contains(&offset)
        || pointer.checked_add(offset as u64) != Some(address)
    {
        return Err("engine header pointer/offset/width provenance mismatch".into());
    }
    let extent = offset as u64 + width as u64;
    if !memory.owned_noncode_access(pointer, extent as usize) || !live(pointer, extent) {
        return Err("engine header read is outside live original engine data".into());
    }
    Ok(memory.read_int(address, width)? as u32 as i32)
}

unsafe extern "C" fn read(
    method: Object,
    receiver: Object,
    address: u64,
    output: *mut i32,
    width: usize,
) -> i32 {
    let result = std::panic::catch_unwind(|| -> Result<i32, String> {
        let b = BINDING
            .get()
            .ok_or("engine header read not installed")?
            .iter()
            .find(|b| b.method == method as usize)
            .ok_or("engine header read method identity mismatch")?;
        if receiver.is_null() || output.is_null() || width != 4 || !callbacks::has_owner() {
            return Err("engine header read method/owner/output mismatch".into());
        }
        let pointer = callbacks::with_metadata_service(|route, _| {
            let roots = route
                .objects
                .as_ref()
                .ok_or("engine header read has no canonical roots")?;
            let token = roots
                .roots
                .token_for_native(roots.api, receiver)?
                .ok_or("engine header receiver has no canonical root")?;
            roots
                .roots
                .engine_cached_pointer(roots.api, token, b.owner as Object)
        })?;
        let mut offset = -1i32;
        let table = unsafe { (b.vtable)((b.domain)(), b.owner as Object) };
        if table.is_null() {
            return Err("engine header static offset table absent".into());
        }
        // The original method already loaded this initialized static field.
        // Reading its actual value does not run a replacement constructor.
        unsafe { (b.get)(table, b.offset as Object, (&mut offset as *mut i32).cast()) };
        callbacks::with_service(|memory, runtime| {
            checked_read(memory, pointer, address, offset, width, |base, extent| {
                runtime.live_data_slot(base, extent)
                    || memory.guest_private_data_slot(base, extent as usize)
            })
        })
    });
    match result {
        Ok(Ok(value)) => {
            unsafe { output.write(value) };
            READS.with(|reads| {
                let count = reads.get() + 1;
                reads.set(count);
                if count <= 4 {
                    eprintln!(
                        "NATIVE_MONO_ENGINE_HEADER_READ: checked-guest-i4=true; reads={count}"
                    );
                }
            });
            0
        }
        other => {
            eprintln!("NATIVE_MONO_ENGINE_HEADER_READ_REJECT: {other:?}");
            1
        }
    }
}

pub(super) fn report() -> String {
    format!(
        "native-engine-header-checked-reads={}\n",
        READS.with(Cell::get)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::x64::memory::{READ, WRITE};
    #[test]
    fn engine_header_reads_actual_guest_bits_and_rejects_aliases_and_dead_storage() {
        let mut memory = Memory::default();
        memory.map(0x2000, 4096, READ | WRITE).unwrap();
        memory.write_int(0x201c, 4, 0xfedcba98).unwrap();
        let live = |base, extent| base == 0x2000 && extent == 32;
        assert_eq!(
            checked_read(&memory, 0x2000, 0x201c, 28, 4, live).unwrap() as u32,
            0xfedcba98
        );
        for (pointer, address, offset, width) in [
            (0, 28, 28, 4),
            (0x2001, 0x201d, 28, 4),
            (0x2000, 0x2018, 28, 4),
            (0x2000, 0x201c, 28, 8),
            (0x2000, 0x201c, -1, 4),
            (u64::MAX - 7, 20, 28, 4),
        ] {
            assert!(checked_read(&memory, pointer, address, offset, width, |_, _| true).is_err());
        }
        assert!(checked_read(&memory, 0x2000, 0x201c, 28, 4, |_, _| false).is_err());
        assert!(checked_read(&memory, 0x2000, 0x3000, 4096, 4, |_, _| true).is_err());
        let host = [0x12345678i32; 4];
        let pointer = host.as_ptr() as u64;
        assert!(checked_read(&memory, pointer, pointer, 0, 4, |_, _| true).is_err());
        assert_eq!(host, [0x12345678; 4]);
    }
}
