//! Exact shared reference-cell prototype. Each cell owns one strong v2 handle;
//! source admission uses weak handles. GC never reads HostRegion bytes outside
//! their lease, so this does not relax the shared backing contract.
use super::*;
use std::collections::BTreeMap;
const CELL: u64 = 0x53000003;
const DATA: u64 = 0x54000000;
type Install = unsafe extern "C" fn(
    u32,
    Object,
    Option<unsafe extern "C" fn(i32, u64, u64, Object, usize, Object) -> i32>,
    Object,
) -> i32;
#[derive(Clone, Copy)]
struct Api {
    new: unsafe extern "C" fn(Object, i32) -> Handle,
    target: unsafe extern "C" fn(Handle) -> Object,
    free: unsafe extern "C" fn(Handle),
    barrier: unsafe extern "C" fn(Object, Object),
    object_class: unsafe extern "C" fn(Object) -> Object,
    class_name: unsafe extern "C" fn(Object) -> *const c_char,
}
struct Slot {
    region: HostRegion,
    strong: usize,
}
struct State {
    guest: Box<Guest>,
    slot: Slot,
    known: BTreeMap<u64, usize>,
    api: Api,
    reference_calls: [u64; 2],
}
unsafe extern "C" fn access(
    op: i32,
    address: u64,
    second: u64,
    buffer: Object,
    length: usize,
    user: Object,
) -> i32 {
    let result = std::panic::catch_unwind(|| {
        let state = unsafe { &mut *user.cast::<State>() };
        if (1..=4).contains(&op) {
            if op != 1 && address < CELL + 8 && address.saturating_add(length as u64) > CELL {
                return Err("scalar write/copy cannot modify a reference slot".to_string());
            }
            let result = unsafe {
                checked::checked_access(
                    op,
                    address,
                    second,
                    buffer,
                    length,
                    (&mut *state.guest as *mut Guest).cast(),
                )
            };
            return if result == 0 {
                Ok(())
            } else {
                Err("checked scalar operation failed".into())
            };
        }
        if !(5..=6).contains(&op) || address != CELL || length != 8 || buffer.is_null() {
            return Err("unregistered reference slot/operation".into());
        }
        state.reference_calls[(op - 5) as usize] += 1;
        state
            .guest
            .memory
            .check(address, 8, if op == 5 { WRITE } else { READ })?;
        let value = if op == 5 {
            unsafe { buffer.cast::<u64>().read_unaligned() }
        } else {
            state.guest.memory.read_int(address, 8)?
        };
        if value != 0 {
            let handle = state
                .known
                .get(&value)
                .ok_or("reference value has no admission token")?;
            if unsafe { (state.api.target)(*handle as Handle) } as u64 != value {
                return Err("reference admission token expired".into());
            }
        }
        if op == 6 {
            let rooted = if state.slot.strong == 0 {
                0
            } else {
                (unsafe { (state.api.target)(state.slot.strong as Handle) }) as u64
            };
            if value != rooted {
                return Err("reference bytes do not match the slot's strong root".into());
            }
            unsafe {
                buffer.cast::<u64>().write_unaligned(value);
            }
            return Ok(());
        }
        // Create the new root before publishing a pointer, then release the
        // previous root. No managed invoke or guest reentry occurs here.
        let handle = if value == 0 {
            0
        } else {
            (unsafe { (state.api.new)(value as Object, 0) }) as usize
        };
        if value != 0 && handle == 0 {
            return Err("reference root allocation failed".into());
        }
        let written = state.slot.region.with_native(|pointer| unsafe {
            (state.api.barrier)(pointer.as_ptr().cast(), value as Object)
        });
        if let Err(error) = written {
            if handle != 0 {
                unsafe {
                    (state.api.free)(handle as Handle);
                }
            }
            return Err(error);
        }
        let previous = std::mem::replace(&mut state.slot.strong, handle);
        if previous != 0 {
            unsafe {
                (state.api.free)(previous as Handle);
            }
        }
        Ok(())
    });
    match result {
        Ok(Ok(())) => 0,
        other => {
            eprintln!("checked reference rejection: {other:?}");
            -1
        }
    }
}
fn invoke(
    invoke: Invoke,
    api: Api,
    method: usize,
    args: &mut [Object],
    expected_exception: bool,
) -> Object {
    let mut exception = ptr::null_mut();
    let result = unsafe {
        invoke(
            method as Object,
            ptr::null_mut(),
            args.as_mut_ptr(),
            &mut exception,
        )
    };
    assert_eq!(
        !exception.is_null(),
        expected_exception,
        "rooted reference exception contract"
    );
    if expected_exception {
        assert!(result.is_null());
        let actual =
            unsafe { std::ffi::CStr::from_ptr((api.class_name)((api.object_class)(exception))) };
        assert_eq!(actual.to_str().unwrap(), "AccessViolationException");
    }
    result
}
pub(super) fn run(mono: &Mono, guest: Box<Guest>, blocks: &str) -> Box<Guest> {
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
    let install = symbol!(mono.library, "wire_mono_set_memory_adapter", Install);
    let path = CString::new(blocks).unwrap();
    let image = unsafe { image(open(mono.domain, path.as_ptr())) } as usize;
    let class = unsafe { class(image as Object, c"".as_ptr(), c"CheckedBlocks".as_ptr()) };
    let store = unsafe { (mono.method)(class, c"ReferenceStore".as_ptr(), 2) } as usize;
    let load = unsafe { (mono.method)(class, c"ReferenceLoad".as_ptr(), 1) } as usize;
    let fill = unsafe { (mono.method)(class, c"Fill".as_ptr(), 3) } as usize;
    assert_ne!(store, 0);
    assert_ne!(load, 0);
    let api = Api {
        new: mono.new_handle,
        target: mono.get_target,
        free: mono.free_handle,
        object_class: symbol!(
            mono.library,
            "mono_object_get_class",
            unsafe extern "C" fn(Object) -> Object
        ),
        class_name: symbol!(
            mono.library,
            "mono_class_get_name",
            unsafe extern "C" fn(Object) -> *const c_char
        ),
        barrier: symbol!(
            mono.library,
            "mono_gc_wbarrier_generic_store",
            unsafe extern "C" fn(Object, Object)
        ),
    };
    let region = HostRegion::owned(8).unwrap();
    region
        .with_native(|p| assert_eq!(p.as_ptr() as usize % 8, 0))
        .unwrap();
    let mut state = Box::new(State {
        guest,
        slot: Slot { region, strong: 0 },
        known: BTreeMap::new(),
        api,
        reference_calls: [0; 2],
    });
    state
        .guest
        .memory
        .map_shared_data(CELL, state.slot.region.clone(), READ | WRITE, READ | WRITE)
        .unwrap();
    state.guest.memory.map(DATA, 4096, READ | WRITE).unwrap();
    state
        .guest
        .memory
        .write_int(DATA, 8, 0x5555555555555555)
        .unwrap();
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
    let array_length = symbol!(
        mono.library,
        "mono_array_length",
        unsafe extern "C" fn(Object) -> usize
    );
    let domain = mono.domain as usize;
    let factory = mono.find("NewArray", 0) as usize;
    let call = mono.invoke;
    let weak_new = mono.weak_handle;
    let collect = mono.collect;
    // The main native stack never observes the managed address. Creation,
    // validation and clearing use distinct attached threads that exit.
    let (state, weak) = std::thread::spawn(move || unsafe {
        let thread = attach(domain as Object);
        let mut exception = ptr::null_mut();
        let object = call(
            factory as Object,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut exception,
        );
        assert!(!object.is_null() && exception.is_null());
        let strong = (api.new)(object, 1);
        let weak = weak_new(object, 0);
        assert!(!strong.is_null() && !weak.is_null());
        state.known.insert(object as u64, weak as usize);
        assert_eq!(
            install(
                2,
                image as Object,
                Some(access),
                (&mut *state as *mut State).cast()
            ),
            0
        );
        let mut address = CELL;
        let mut args = [(&mut address as *mut u64).cast(), object];
        assert!(!invoke(call, api, store, &mut args, false).is_null());
        let loaded = invoke(call, api, load, &mut args[..1], false);
        assert_eq!(loaded, object);
        assert_eq!(array_length(loaded), 4);
        state
            .guest
            .memory
            .protect_shared_data(CELL, READ, false)
            .unwrap();
        args[1] = ptr::null_mut();
        invoke(call, api, store, &mut args, true);
        assert_eq!(invoke(call, api, load, &mut args[..1], false), object);
        state
            .guest
            .memory
            .protect_shared_data(CELL, READ | WRITE, true)
            .unwrap();
        invoke(call, api, store, &mut args, true); // One-shot guard, no root/data update.
        assert_eq!(invoke(call, api, load, &mut args[..1], false), object);
        address = DATA;
        std::hint::black_box(address);
        invoke(call, api, store, &mut args, true);
        assert_eq!(
            state.guest.memory.read_int(DATA, 8).unwrap(),
            0x5555555555555555
        );
        address = CELL + 1;
        std::hint::black_box(address);
        invoke(call, api, store, &mut args, true);
        address = CELL;
        std::hint::black_box(address);
        let root_before = state.slot.strong;
        let mut fill_byte = 0i32;
        let mut fill_len = 8i32;
        let mut fill_args = [
            (&mut address as *mut u64).cast(),
            (&mut fill_byte as *mut i32).cast(),
            (&mut fill_len as *mut i32).cast(),
        ];
        invoke(call, api, fill, &mut fill_args, true);
        assert_eq!(state.slot.strong, root_before);
        assert_eq!(invoke(call, api, load, &mut args[..1], false), object);
        state.guest.memory.write_int(CELL, 8, 0).unwrap();
        invoke(call, api, load, &mut args[..1], true); // Known NULL mismatches the live root.
        assert_eq!(state.slot.strong, root_before);
        state.guest.memory.write_int(CELL, 8, 0x12345678).unwrap();
        invoke(call, api, load, &mut args[..1], true); // Unknown bits never become an object.
        args[1] = object;
        invoke(call, api, store, &mut args, false);
        assert_eq!(install(1, ptr::null_mut(), None, ptr::null_mut()), 0);
        (api.free)(strong);
        detach(thread);
        (state, weak as usize)
    })
    .join()
    .unwrap();
    let preserved = std::thread::spawn(move || unsafe {
        let thread = attach(domain as Object);
        for _ in 0..4 {
            collect(0);
        }
        let preserved = !(api.target)(weak as Handle).is_null();
        detach(thread);
        preserved
    })
    .join()
    .unwrap();
    assert!(
        preserved,
        "slot's only strong handle did not preserve the reference"
    );
    let (state, replacement_weak) = std::thread::spawn(move || unsafe {
        let thread = attach(domain as Object);
        let mut state = state;
        let mut exception = ptr::null_mut();
        let object = call(
            factory as Object,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut exception,
        );
        assert!(!object.is_null() && exception.is_null());
        let strong = (api.new)(object, 1);
        let replacement_weak = weak_new(object, 0);
        assert!(!strong.is_null() && !replacement_weak.is_null());
        state.known.insert(object as u64, replacement_weak as usize);
        assert_eq!(
            install(
                2,
                image as Object,
                Some(access),
                (&mut *state as *mut State).cast()
            ),
            0
        );
        let mut address = CELL;
        let mut args = [(&mut address as *mut u64).cast(), object];
        invoke(call, api, store, &mut args, false);
        assert_eq!(invoke(call, api, load, &mut args[..1], false), object);
        assert_eq!(install(1, ptr::null_mut(), None, ptr::null_mut()), 0);
        (api.free)(strong);
        detach(thread);
        (state, replacement_weak as usize)
    })
    .join()
    .unwrap();
    let replaced_correctly = std::thread::spawn(move || unsafe {
        let thread = attach(domain as Object);
        for _ in 0..8 {
            collect(0);
        }
        let result = (api.target)(weak as Handle).is_null()
            && !(api.target)(replacement_weak as Handle).is_null();
        detach(thread);
        result
    })
    .join()
    .unwrap();
    assert!(
        replaced_correctly,
        "slot replacement retained the old value or lost the new value"
    );
    let state = std::thread::spawn(move || unsafe {
        let thread = attach(domain as Object);
        let mut state = state;
        assert_eq!(
            install(
                2,
                image as Object,
                Some(access),
                (&mut *state as *mut State).cast()
            ),
            0
        );
        let mut address = CELL;
        let mut args = [(&mut address as *mut u64).cast(), ptr::null_mut()];
        invoke(call, api, store, &mut args, false);
        assert_eq!(state.slot.strong, 0);
        assert!(invoke(call, api, load, &mut args[..1], false).is_null());
        state.slot.region.revoke().unwrap();
        invoke(call, api, store, &mut args, true);
        invoke(call, api, load, &mut args[..1], true);
        drop(state.guest.memory.unmap_shared_data(CELL).unwrap());
        assert_eq!(install(1, ptr::null_mut(), None, ptr::null_mut()), 0);
        detach(thread);
        state
    })
    .join()
    .unwrap();
    let reclaimed = std::thread::spawn(move || unsafe {
        let thread = attach(domain as Object);
        for _ in 0..8 {
            collect(0);
        }
        let reclaimed = (api.target)(weak as Handle).is_null()
            && (api.target)(replacement_weak as Handle).is_null();
        (api.free)(weak as Handle);
        (api.free)(replacement_weak as Handle);
        detach(thread);
        reclaimed
    })
    .join()
    .unwrap();
    assert!(
        reclaimed,
        "cleared reference cell permanently retained its former value"
    );
    assert!(state.reference_calls.iter().all(|&n| n >= 4));
    println!("PASS exact rooted reference slots: typed ldind.ref/stind.ref, known weak admission, native write barrier, per-cell strong v2 handle, readonly/guard/range/unknown value/root mismatch/scalar overlap/revoke; preserved, replaced then reclaimed; calls={:?}",state.reference_calls);
    let State {
        guest, slot, known, ..
    } = *state;
    assert_eq!(slot.strong, 0);
    drop((slot, known));
    guest
}
