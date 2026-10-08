//! Verify that BDWGC scans active interpreter locals, then forgets a popped
//! frame while that same native Mono thread stays attached and idle.
use super::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    OnceLock,
};
static WEAK_NEW: OnceLock<unsafe extern "C" fn(Object, i32) -> Handle> = OnceLock::new();
static WEAK: AtomicUsize = AtomicUsize::new(0);
unsafe extern "C" fn capture(value: Object) -> i32 {
    match std::panic::catch_unwind(|| {
        assert!(!value.is_null());
        let handle = unsafe { WEAK_NEW.get().unwrap()(value, 0) };
        assert!(!handle.is_null());
        assert!(WEAK
            .compare_exchange(0, handle as usize, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok());
        1
    }) {
        Ok(value) => value,
        Err(_) => std::process::abort(),
    }
}
#[inline(never)]
fn wipe_native_stack(depth: usize) {
    let words = [0u64; 4096];
    std::hint::black_box(&words);
    if depth > 0 {
        wipe_native_stack(depth - 1);
    }
    std::hint::black_box(&words);
}
pub(super) fn run(mono: &Mono, class: Object) {
    assert!(WEAK_NEW.set(mono.weak_handle).is_ok());
    let add = symbol!(
        mono.library,
        "mono_add_internal_call",
        unsafe extern "C" fn(*const c_char, Object)
    );
    unsafe {
        add(
            c"NativeServicesProbe::CaptureInterpreterWeak".as_ptr(),
            capture as *const () as Object,
        );
    }
    let factory = unsafe { (mono.method)(class, c"InterpreterWeakFactory".as_ptr(), 0) } as usize;
    assert_ne!(factory, 0);
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
    let (ready_send, ready) = std::sync::mpsc::channel();
    let (finish, finish_receive) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let thread = unsafe { attach(domain as Object) };
        assert!(!thread.is_null());
        let mut exception = ptr::null_mut();
        let result = unsafe {
            invoke(
                factory as Object,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut exception,
            )
        };
        assert!(exception.is_null() && !result.is_null());
        assert_eq!(unsafe { unbox(result).cast::<i32>().read() }, 12345);
        wipe_native_stack(7);
        ready_send.send(()).unwrap();
        // Remain attached. Thread detach cannot explain collection of this
        // object; its old reference is still in the inactive interpreter mmap.
        finish_receive
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        unsafe {
            detach(thread);
        }
    });
    ready
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    let handle = WEAK.load(Ordering::SeqCst) as Handle;
    assert!(!handle.is_null());
    // Do not read the target between collections: a native register/stack
    // copy of a successful read would conservatively keep it alive itself.
    for _ in 0..8 {
        unsafe {
            (mono.collect)(0);
        }
    }
    let cleared = unsafe { (mono.get_target)(handle) }.is_null();
    finish.send(()).unwrap();
    worker.join().unwrap();
    unsafe {
        (mono.free_handle)(handle);
    }
    WEAK.store(0, Ordering::SeqCst);
    assert!(
        cleared,
        "inactive interpreter frame permanently retained its object"
    );
    println!("PASS BDWGC interpreter roots: active locals survived GC; popped frame reclaimed while its owner thread remained attached");
}
