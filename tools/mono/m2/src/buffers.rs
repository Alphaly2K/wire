//! Exercise the reusable engine-byte arena in real checked unsafe IL.
use super::*;
use wire::mono::{Admission, BackendSession, SharedBuffers};
pub(super) fn run(mono: &Mono, state: *mut Guest) {
    // Explicitly synthetic lifecycle proof for this fixture only. The actual
    // game preflight and provider remain closed.
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
    let mut buffers = SharedBuffers::new(session.clone(), 256 * 1024).unwrap();
    let install = symbol!(
        mono.library,
        "wire_mono_set_memory_adapter",
        unsafe extern "C" fn(
            u32,
            Object,
            Option<unsafe extern "C" fn(i32, u64, u64, Object, usize, Object) -> i32>,
            Object,
        ) -> i32
    );
    assert_eq!(
        unsafe { install(1, mono.image, Some(checked::checked_access), state.cast()) },
        0
    );
    let handle = buffers
        .allocate(unsafe { &mut (*state).memory }, 65537, 64)
        .unwrap();
    const ALIAS: u64 = 0x56000003;
    buffers
        .alias(unsafe { &mut (*state).memory }, handle, ALIAS, READ | WRITE)
        .unwrap();
    let view = buffers.native_view(handle).unwrap();
    let mut at = handle.address();
    let mut len = handle.len() as i32;
    let mut args = [(&mut at as *mut u64).cast(), (&mut len as *mut i32).cast()];
    let expected: i32 = (0..len).map(|i| (i * 13 + 7) as u8 as i32).sum();
    assert_eq!(
        checked::invoke_i32(mono, mono.find("BufferPattern", 2), &mut args, None),
        expected
    );
    unsafe { (mono.collect)(0) };
    view.with_native(|p| {
        assert_eq!(p.as_ptr() as u64, handle.address());
        assert_eq!(p.as_ptr() as usize % 64, 0);
        for i in 0..handle.len() {
            assert_eq!(unsafe { p.as_ptr().add(i).read() }, (i * 13 + 7) as u8);
        }
    })
    .unwrap();
    assert_eq!(
        unsafe { &(*state).memory }
            .read_int(ALIAS + 65536, 1)
            .unwrap(),
        7
    );
    let read = mono.find("ReadI32", 1);
    let write = mono.find("WriteI32", 2);
    let mut value = 0x12345678i32;
    let mut scalar = [
        (&mut at as *mut u64).cast(),
        (&mut value as *mut i32).cast(),
    ];
    at = ALIAS;
    std::hint::black_box(at);
    assert_eq!(checked::invoke_i32(mono, write, &mut scalar, None), value);
    assert_eq!(
        unsafe { &(*state).memory }
            .read_int(handle.address(), 4)
            .unwrap(),
        value as u64
    );
    unsafe { &mut (*state).memory }
        .protect_shared_data(ALIAS, READ, false)
        .unwrap();
    value = 99;
    std::hint::black_box(value);
    checked::invoke_i32(mono, write, &mut scalar, Some("AccessViolationException"));
    assert_eq!(
        unsafe { &(*state).memory }
            .read_int(handle.address(), 4)
            .unwrap(),
        0x12345678
    );
    at = handle.address() + handle.len() as u64 - 2;
    std::hint::black_box(at);
    checked::invoke_i32(mono, write, &mut scalar, Some("AccessViolationException"));
    assert!(buffers
        .reallocate(unsafe { &mut (*state).memory }, handle, 256 * 1024 + 1)
        .is_err());
    let new = buffers
        .reallocate(unsafe { &mut (*state).memory }, handle, 131071)
        .unwrap();
    assert!(view.with_native(|_| ()).is_err());
    at = handle.address();
    std::hint::black_box(at);
    checked::invoke_i32(
        mono,
        read,
        &mut scalar[..1],
        Some("AccessViolationException"),
    );
    at = ALIAS;
    std::hint::black_box(at);
    checked::invoke_i32(
        mono,
        read,
        &mut scalar[..1],
        Some("AccessViolationException"),
    );
    at = new.address();
    std::hint::black_box(at);
    assert_eq!(
        checked::invoke_i32(mono, read, &mut scalar[..1], None),
        0x12345678
    );
    assert_eq!(
        unsafe { &(*state).memory }
            .read_int(new.address() + 131063, 8)
            .unwrap(),
        0
    );
    session.begin_shutdown().unwrap();
    buffers.retire_all(unsafe { &mut (*state).memory }).unwrap();
    checked::invoke_i32(
        mono,
        read,
        &mut scalar[..1],
        Some("AccessViolationException"),
    );
    assert!(
        !session.cleanup_ready(),
        "native view still retains old backing"
    );
    drop(view);
    assert!(session.cleanup_ready());
    session.cleaned_up().unwrap();
    assert_eq!(
        unsafe { install(1, ptr::null_mut(), None, ptr::null_mut()) },
        0
    );
    println!("PASS lifecycle-owned shared buffer allocation: checked 65537B unsafe IL/native/guest pattern, pointer identity/alignment, aliases, readonly/bounds, failed realloc, 131071B grow/zero-tail, retired pointers/tokens, actual backing leases block cleanup; fixture lifecycle only, Unity allocator/managed heap not integrated");
}
