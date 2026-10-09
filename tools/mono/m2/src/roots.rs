//! A paused guest register and interior stack pointer are the only strong roots.
//! The creator remains attached across GC so detach cannot explain collection.
use super::*;
use std::sync::{
    atomic::{AtomicIsize, Ordering},
    Mutex, OnceLock,
};
use wire::mono::{
    Admission, BackendSession, GcHandleApi, GuestObjectToken, GuestRootRange, GuestRootSnapshot,
    GuestRoots,
};
static REAL: OnceLock<GcHandleApi> = OnceLock::new();
static FAIL_AFTER: AtomicIsize = AtomicIsize::new(-1);
static STRONG: Mutex<std::collections::BTreeSet<usize>> =
    Mutex::new(std::collections::BTreeSet::new());
unsafe extern "C" fn new_strong(object: Object, pinned: i32) -> Handle {
    let remaining = FAIL_AFTER.load(Ordering::SeqCst);
    if remaining == 0 {
        FAIL_AFTER.store(-1, Ordering::SeqCst);
        return ptr::null_mut();
    }
    if remaining > 0 {
        FAIL_AFTER.fetch_sub(1, Ordering::SeqCst);
    }
    let handle = unsafe { (REAL.get().unwrap().strong)(object, pinned) };
    if !handle.is_null() {
        assert!(STRONG.lock().unwrap().insert(handle as usize));
    }
    handle
}
unsafe extern "C" fn free(handle: Handle) {
    STRONG.lock().unwrap().remove(&(handle as usize));
    unsafe { (REAL.get().unwrap().free)(handle) };
}
#[inline(never)]
fn wipe(depth: usize) {
    let words = [0u64; 4096];
    std::hint::black_box(&words);
    if depth != 0 {
        wipe(depth - 1);
    }
    std::hint::black_box(&words);
}
#[inline(never)]
unsafe fn create(
    invoke: Invoke,
    factory: usize,
    api: GcHandleApi,
    array_addr: unsafe extern "C" fn(Object, i32, usize) -> Object,
    roots: &mut GuestRoots,
    memory: &mut Memory,
) -> ([GuestObjectToken; 3], [usize; 3]) {
    let mut tokens = Vec::new();
    let mut observers = Vec::new();
    let mut keepers = Vec::new();
    let mut registers = [0; 16];
    for id in 0..3 {
        let mut exception = ptr::null_mut();
        let object = unsafe {
            invoke(
                factory as Object,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut exception,
            )
        };
        assert!(!object.is_null() && exception.is_null());
        let payload = unsafe { array_addr(object, 4, 0) } as u64;
        let extent = (payload - object as u64) as usize + 16;
        let token =
            unsafe { roots.admit(NonNull::new(object).unwrap(), &[(object as u64, extent)]) }
                .unwrap();
        let weak = unsafe { (api.weak)(object, 0) } as usize;
        assert_ne!(weak, 0);
        observers.push(weak);
        if id < 2 {
            keepers.push(unsafe { (REAL.get().unwrap().strong)(object, 0) });
        }
        tokens.push(token);
        if id == 0 || id == 2 {
            registers[if id == 0 { 0 } else { 1 }] = object as u64;
        } else {
            memory.write_int(0x59000000, 8, payload + 4).unwrap();
        }
    }
    let ranges = [GuestRootRange {
        start: 0x59000000,
        bytes: 8,
    }];
    let snapshot = GuestRootSnapshot::capture(&registers, memory, &ranges).unwrap();
    unsafe { roots.synchronize(snapshot) }.unwrap();
    assert_eq!(roots.strong_roots(), 3);
    // A bad/incomplete readable range cannot be used to shrink old roots.
    assert!(GuestRootSnapshot::capture(
        &[],
        memory,
        &[GuestRootRange {
            start: 0xdead0000,
            bytes: 8
        }]
    )
    .is_err());
    assert_eq!(roots.strong_roots(), 3);
    // Remove two registry roots while separate temporary native handles keep
    // their weak admission alive. Fail the second re-add: the first allocation
    // must roll back and the third existing root must remain untouched.
    let third_only = GuestRootSnapshot::capture(&registers[1..2], memory, &[]).unwrap();
    unsafe { roots.synchronize(third_only) }.unwrap();
    assert_eq!(roots.strong_roots(), 1);
    assert_eq!(STRONG.lock().unwrap().len(), 1);
    let all = GuestRootSnapshot::capture(&registers, memory, &ranges).unwrap();
    FAIL_AFTER.store(1, Ordering::SeqCst);
    assert!(unsafe { roots.synchronize(all) }.is_err());
    assert_eq!(roots.strong_roots(), 1);
    assert_eq!(
        STRONG.lock().unwrap().len(),
        1,
        "failed root acquisition leaked a native handle"
    );
    assert!(roots.token_is_rooted(tokens[2]));
    let all = GuestRootSnapshot::capture(&registers, memory, &ranges).unwrap();
    unsafe { roots.synchronize(all) }.unwrap();
    assert_eq!(roots.strong_roots(), 3);
    for keeper in keepers {
        unsafe { (REAL.get().unwrap().free)(keeper) };
    }
    (tokens.try_into().unwrap(), observers.try_into().unwrap())
}
#[inline(never)]
fn observer_count(api: GcHandleApi, observers: &[usize; 3]) -> usize {
    observers
        .iter()
        .filter(|&&handle| !unsafe { (api.target)(handle as Handle) }.is_null())
        .count()
}
pub(super) fn run(mono: &Mono) {
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
    let real = GcHandleApi {
        current_thread: symbol!(
            mono.library,
            "mono_thread_current",
            unsafe extern "C" fn() -> Object
        ),
        strong: mono.new_handle,
        weak: mono.weak_handle,
        target: mono.get_target,
        free: mono.free_handle,
    };
    assert!(REAL.set(real).is_ok());
    let api = GcHandleApi {
        strong: new_strong,
        free,
        ..real
    };
    let array_addr = symbol!(
        mono.library,
        "mono_array_addr_with_size",
        unsafe extern "C" fn(Object, i32, usize) -> Object
    );
    let domain = mono.domain as usize;
    let factory = mono.find("NewArray", 0) as usize;
    let invoke = mono.invoke;
    let (ready_send, ready) = std::sync::mpsc::channel();
    let (resume, resumed) = std::sync::mpsc::channel();
    let owner = std::thread::spawn(move || unsafe {
        let thread = attach(domain as Object);
        let session = BackendSession::select(&Admission {
            requested: true,
            known_binary_fingerprint: true,
            full_native_abi: true,
            checked_memory_and_roots: true,
            windows_services_and_scheduler: true,
            unity_initialization_callbacks: true,
            required_exports_covered: true,
            no_dynamic_code: true,
        });
        session.begin_initialization().unwrap();
        session.initialized().unwrap();
        let mut roots = Box::new(GuestRoots::new(session.clone(), api).unwrap());
        let mut memory = Memory::default();
        memory.map(0x59000000, 4096, READ | WRITE).unwrap();
        let (tokens, observers) = create(invoke, factory, api, array_addr, &mut roots, &mut memory);
        assert!(tokens.iter().all(|&token| roots.token_is_rooted(token)));
        wipe(8);
        ready_send.send(0).unwrap();
        resumed
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        assert_eq!(
            observer_count(api, &observers),
            3,
            "guest roots did not preserve objects"
        );
        memory.write_int(0x59000000, 8, 0).unwrap();
        let snapshot = GuestRootSnapshot::capture(
            &[0; 16],
            &memory,
            &[GuestRootRange {
                start: 0x59000000,
                bytes: 8,
            }],
        )
        .unwrap();
        roots.synchronize(snapshot).unwrap();
        assert_eq!(roots.strong_roots(), 0);
        wipe(8);
        ready_send.send(1).unwrap();
        resumed
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        assert_eq!(
            observer_count(api, &observers),
            0,
            "empty snapshot permanently retained guest objects"
        );
        assert_eq!(roots.forget_reclaimed().unwrap(), 3);
        assert!(tokens.iter().all(|&token| !roots.token_is_rooted(token)));
        session.begin_shutdown().unwrap();
        assert!(
            !session.cleanup_ready(),
            "weak handle registry still owns VM resources"
        );
        roots.retire_all().unwrap();
        drop(roots);
        assert!(session.cleanup_ready());
        session.cleaned_up().unwrap();
        for observer in observers {
            (api.free)(observer as Handle);
        }
        detach(thread);
    });
    for phase in 0..2 {
        assert_eq!(
            ready
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap(),
            phase
        );
        for _ in 0..8 {
            unsafe { (mono.collect)(0) };
        }
        resume.send(()).unwrap();
    }
    owner.join().unwrap();
    assert!(STRONG.lock().unwrap().is_empty());
    println!("PASS lifecycle-owned guest root snapshots: exposed object admission, paused GPR/interior stack pointer, failed range snapshot keeps roots, second-acquisition failure rolls back new handles and retains existing roots, 16 GC while creator remains attached, complete empty snapshot reclaims three objects, expired weak/token retirement, actual registry lease blocks cleanup; production safepoints not integrated");
}
