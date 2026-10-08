//! Version-3 exact pinned Int32[] operations. Object identities are admitted
//! before any metadata/payload access; arrays never fall back to raw pointers.
use super::*;
const ALIAS: u64 = 0x56000003;
type Install = unsafe extern "C" fn(
    u32,
    Object,
    Option<unsafe extern "C" fn(i32, u64, u64, Object, usize, Object) -> i32>,
    Object,
) -> i32;
struct State {
    guest: Box<Guest>,
    region: HostRegion,
    owner: Arc<PinnedOwner>,
    object: u64,
    address: u64,
    length: usize,
    target: unsafe extern "C" fn(Handle) -> Object,
    calls: [u64; 4],
}
unsafe extern "C" fn access(
    op: i32,
    object: u64,
    second: u64,
    buffer: Object,
    length: usize,
    user: Object,
) -> i32 {
    let result = std::panic::catch_unwind(|| {
        let state = unsafe { &mut *user.cast::<State>() };
        if (1..=4).contains(&op) {
            return unsafe {
                checked::checked_access(
                    op,
                    object,
                    second,
                    buffer,
                    length,
                    (&mut *state.guest as *mut Guest).cast(),
                )
            };
        }
        if !(7..=10).contains(&op)
            || buffer.is_null()
            || object != state.object
            || unsafe { (state.target)(state.owner.handle as Handle) } as u64 != object
        {
            return -1;
        }
        state.calls[(op - 7) as usize] += 1;
        if op == 7 {
            if length != 8 || state.region.with_native(|_| ()).is_err() {
                return -1;
            }
            unsafe {
                buffer.cast::<u64>().write_unaligned(state.length as u64);
            }
            return 0;
        }
        let index = second as u32 as i32;
        if index < 0 || index as usize >= state.length {
            return 1;
        }
        let address = state.address + (index as u64) * 4;
        match op {
            8 if length == 4 => {
                let value = match state.guest.memory.read_int(address, 4) {
                    Ok(value) => value,
                    Err(_) => return -1,
                };
                unsafe {
                    buffer.cast::<u32>().write_unaligned(value as u32);
                }
                0
            }
            9 if length == 4 => {
                let value = unsafe { buffer.cast::<u32>().read_unaligned() };
                if state
                    .guest
                    .memory
                    .write_int(address, 4, value as u64)
                    .is_ok()
                {
                    0
                } else {
                    -1
                }
            }
            10 if length == 8 && second >> 32 == 4 => {
                if state.guest.memory.check(address, 4, READ).is_err() {
                    return -1;
                }
                unsafe {
                    buffer.cast::<u64>().write_unaligned(address);
                }
                0
            }
            _ => -1,
        }
    });
    result.unwrap_or(-1)
}
#[derive(Clone, Copy)]
struct Api {
    call: Invoke,
    unbox: unsafe extern "C" fn(Object) -> Object,
    class: unsafe extern "C" fn(Object) -> Object,
    name: unsafe extern "C" fn(Object) -> *const c_char,
    method_name: unsafe extern "C" fn(Object) -> *const c_char,
}
fn invoke(api: Api, method: usize, args: &mut [Object], expected: Option<&str>) -> i32 {
    let mut exception = ptr::null_mut();
    let result = unsafe {
        (api.call)(
            method as Object,
            ptr::null_mut(),
            args.as_mut_ptr(),
            &mut exception,
        )
    };
    if let Some(expected) = expected {
        assert!(
            !exception.is_null() && result.is_null(),
            "array failure did not cross as managed exception"
        );
        let actual = unsafe { std::ffi::CStr::from_ptr((api.name)((api.class)(exception))) };
        assert_eq!(actual.to_str().unwrap(), expected);
        return 0;
    }
    if !exception.is_null() {
        let name = unsafe { std::ffi::CStr::from_ptr((api.method_name)(method as Object)) };
        let kind = unsafe { std::ffi::CStr::from_ptr((api.name)((api.class)(exception))) };
        panic!("unexpected registered array exception in {name:?}: {kind:?}");
    }
    assert!(!result.is_null());
    unsafe { (api.unbox)(result).cast::<i32>().read_unaligned() }
}
pub(super) fn run(mono: &Mono, guest: Box<Guest>) -> Box<Guest> {
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
    let install = symbol!(mono.library, "wire_mono_set_memory_adapter", Install);
    let addr = symbol!(
        mono.library,
        "mono_array_addr_with_size",
        unsafe extern "C" fn(Object, i32, usize) -> Object
    );
    let len = symbol!(
        mono.library,
        "mono_array_length",
        unsafe extern "C" fn(Object) -> usize
    );
    let rank = symbol!(
        mono.library,
        "mono_class_get_rank",
        unsafe extern "C" fn(Object) -> i32
    );
    let element = symbol!(
        mono.library,
        "mono_class_get_element_class",
        unsafe extern "C" fn(Object) -> Object
    );
    let int32 = symbol!(
        mono.library,
        "mono_get_int32_class",
        unsafe extern "C" fn() -> Object
    );
    let domain = mono.domain as usize;
    let image = mono.image as usize;
    let factory = mono.find("NewArray", 0) as usize;
    let methods = [
        mono.find("CheckedArrayLength", 1) as usize,
        mono.find("CheckedArrayRead", 2) as usize,
        mono.find("CheckedArrayWrite", 3) as usize,
        mono.find("CheckedArrayByRef", 3) as usize,
        mono.find("CheckedArraySum", 1) as usize,
        mono.find("CheckedArrayWrongWidth", 2) as usize,
    ];
    let api = Api {
        call: mono.invoke,
        unbox: symbol!(
            mono.library,
            "mono_object_unbox",
            unsafe extern "C" fn(Object) -> Object
        ),
        class: symbol!(
            mono.library,
            "mono_object_get_class",
            unsafe extern "C" fn(Object) -> Object
        ),
        method_name: symbol!(
            mono.library,
            "mono_method_get_name",
            unsafe extern "C" fn(Object) -> *const c_char
        ),
        name: symbol!(
            mono.library,
            "mono_class_get_name",
            unsafe extern "C" fn(Object) -> *const c_char
        ),
    };
    let new = mono.new_handle;
    let weak_new = mono.weak_handle;
    let target = mono.get_target;
    let free = mono.free_handle;
    let current_domain = symbol!(
        mono.library,
        "mono_domain_get",
        unsafe extern "C" fn() -> Object
    );
    let collect = mono.collect;
    // All managed addresses stay on short-lived attached workers. The main
    // native stack cannot accidentally keep the array alive after revocation.
    let (state, weak) = std::thread::spawn(move || unsafe {
        let thread = attach(domain as Object);
        let mut exception = ptr::null_mut();
        let object = (api.call)(
            factory as Object,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut exception,
        );
        assert!(!object.is_null() && exception.is_null());
        assert_eq!(rank((api.class)(object)), 1);
        assert_eq!(element((api.class)(object)), int32());
        assert_eq!(len(object), 4);
        let handle = new(object, 1);
        let weak = weak_new(object, 0);
        assert!(!handle.is_null() && !weak.is_null());
        let owner = Arc::new(PinnedOwner {
            domain,
            handle: handle as usize,
            attach,
            detach,
            current_domain,
            free,
        });
        let pointer = NonNull::new(addr(object, 4, 0).cast::<u8>()).unwrap();
        let address = pointer.as_ptr() as u64;
        let region = HostRegion::foreign(pointer, 16, owner.clone()).unwrap();
        let mut state = Box::new(State {
            guest,
            region,
            owner,
            object: object as u64,
            address,
            length: 4,
            target,
            calls: [0; 4],
        });
        state
            .guest
            .memory
            .map_shared_data(address, state.region.clone(), READ | WRITE, READ | WRITE)
            .unwrap();
        state
            .guest
            .memory
            .map_shared_data(ALIAS, state.region.clone(), READ | WRITE, READ | WRITE)
            .unwrap();
        detach(thread);
        (state, weak as usize)
    })
    .join()
    .unwrap();
    let mut state = std::thread::spawn(move || unsafe {
        let thread = attach(domain as Object);
        let mut state = state;
        for _ in 0..4 {
            collect(0);
        }
        assert_eq!(target(weak as Handle) as u64, state.object);
        assert_eq!(
            install(
                3,
                image as Object,
                Some(access),
                (&mut *state as *mut State).cast()
            ),
            0
        );
        let mut index = 2i32;
        let mut value = -2147483629i32;
        let mut args = [
            state.object as Object,
            (&mut index as *mut i32).cast(),
            (&mut value as *mut i32).cast(),
        ];
        assert_eq!(invoke(api, methods[0], &mut args[..1], None), 4);
        assert_eq!(invoke(api, methods[1], &mut args[..2], None), 43);
        assert_eq!(invoke(api, methods[2], &mut args, None), value);
        assert_eq!(
            state.guest.memory.read_int(ALIAS + 8, 4).unwrap() as i32,
            value
        );
        value = 0x12345678;
        std::hint::black_box(value);
        assert_eq!(invoke(api, methods[3], &mut args, None), value);
        state.guest.memory.write_int(ALIAS, 4, 101).unwrap();
        assert_eq!(
            invoke(api, methods[4], &mut args[..1], None),
            101i32.wrapping_add(29).wrapping_add(value).wrapping_add(71)
        );
        value = -9173;
        std::hint::black_box(value);
        let snapshot = state.guest.memory.read_int(state.address + 8, 4).unwrap();
        state
            .guest
            .memory
            .protect_shared_data(state.address, READ, false)
            .unwrap();
        invoke(api, methods[2], &mut args, Some("AccessViolationException"));
        invoke(api, methods[3], &mut args, Some("AccessViolationException"));
        assert_eq!(
            state.guest.memory.read_int(state.address + 8, 4).unwrap(),
            snapshot
        );
        state
            .guest
            .memory
            .protect_shared_data(state.address, READ | WRITE, true)
            .unwrap();
        invoke(api, methods[2], &mut args, Some("AccessViolationException"));
        assert_eq!(
            invoke(api, methods[1], &mut args[..2], None),
            snapshot as i32
        );
        for invalid in [-1, 4, i32::MAX, i32::MIN] {
            index = invalid;
            std::hint::black_box(index);
            invoke(
                api,
                methods[1],
                &mut args[..2],
                Some("IndexOutOfRangeException"),
            );
            invoke(api, methods[2], &mut args, Some("IndexOutOfRangeException"));
            invoke(api, methods[3], &mut args, Some("IndexOutOfRangeException"));
        }
        index = 2;
        std::hint::black_box(index);
        invoke(
            api,
            methods[5],
            &mut args[..2],
            Some("NotSupportedException"),
        );
        args[0] = ptr::null_mut();
        invoke(
            api,
            methods[0],
            &mut args[..1],
            Some("NullReferenceException"),
        );
        invoke(
            api,
            methods[1],
            &mut args[..2],
            Some("NullReferenceException"),
        );
        invoke(api, methods[2], &mut args, Some("NullReferenceException"));
        assert_eq!(install(1, ptr::null_mut(), None, ptr::null_mut()), 0);
        let unknown = (api.call)(
            factory as Object,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut ptr::null_mut(),
        );
        assert!(!unknown.is_null());
        assert_eq!(
            install(
                3,
                image as Object,
                Some(access),
                (&mut *state as *mut State).cast()
            ),
            0
        );
        args[0] = unknown;
        invoke(
            api,
            methods[0],
            &mut args[..1],
            Some("AccessViolationException"),
        );
        invoke(
            api,
            methods[1],
            &mut args[..2],
            Some("AccessViolationException"),
        );
        args[0] = state.object as Object;
        state.region.revoke().unwrap();
        invoke(
            api,
            methods[0],
            &mut args[..1],
            Some("AccessViolationException"),
        );
        invoke(
            api,
            methods[1],
            &mut args[..2],
            Some("AccessViolationException"),
        );
        invoke(api, methods[2], &mut args, Some("AccessViolationException"));
        invoke(api, methods[3], &mut args, Some("AccessViolationException"));
        assert!(state.guest.memory.read_int(ALIAS, 4).is_err());
        assert_eq!(install(1, ptr::null_mut(), None, ptr::null_mut()), 0);
        detach(thread);
        state
    })
    .join()
    .unwrap();
    assert!(state.calls.iter().all(|&n| n > 0));
    let calls = state.calls;
    drop(state.guest.memory.unmap_shared_data(state.address).unwrap());
    drop(state.guest.memory.unmap_shared_data(ALIAS).unwrap());
    let State {
        guest,
        region,
        owner,
        ..
    } = *state;
    drop(region);
    drop(owner);
    let reclaimed = std::thread::spawn(move || unsafe {
        let thread = attach(domain as Object);
        for _ in 0..8 {
            collect(0);
        }
        let dead = target(weak as Handle).is_null();
        free(weak as Handle);
        detach(thread);
        dead
    })
    .join()
    .unwrap();
    assert!(reclaimed, "retired pinned array remained rooted");
    println!("PASS exact checked Int32 arrays: ldlen/ldelem/stelem/ldelema checked byref, payload alias, GC pin/reclaim, signed bounds/null/unknown/type/readonly/guard/revoke, no raw fallback; calls={calls:?}");
    guest
}
