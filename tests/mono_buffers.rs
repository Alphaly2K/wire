#![cfg(feature = "native-mono-backend")]
use wire::{
    mono::{Admission, BackendSession, SharedBuffers},
    x64::memory::{Memory, EXECUTE, READ, WRITE},
};
fn fixture_session() -> BackendSession {
    // Synthetic admission is local to these lifecycle tests; no runtime/game
    // profile or provider is selected by this helper.
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
    session
}
#[test]
fn aligned_buffers_share_exact_bytes_with_aliases_and_reject_code() {
    let mut memory = Memory::default();
    let mut buffers = SharedBuffers::new(fixture_session(), 4096).unwrap();
    let handle = buffers.allocate(&mut memory, 37, 64).unwrap();
    assert_eq!(handle.address() % 64, 0);
    let mut zero = [1; 37];
    memory.read(handle.address(), &mut zero).unwrap();
    assert_eq!(zero, [0; 37]);
    buffers
        .alias(&mut memory, handle, 0x57000003, READ | WRITE)
        .unwrap();
    let native = buffers.native_view(handle).unwrap();
    native
        .with_native(|p| unsafe { p.as_ptr().add(36).write(99) })
        .unwrap();
    assert_eq!(memory.read_int(0x57000003 + 36, 1).unwrap(), 99);
    memory.write_int(0x57000003, 4, 0x12345678).unwrap();
    native
        .with_native(|p| assert_eq!(unsafe { p.as_ptr().cast::<u32>().read() }, 0x12345678))
        .unwrap();
    assert!(memory.read_int(handle.address() + 36, 2).is_err());
    assert!(memory.check(handle.address(), 1, EXECUTE).is_err());
    assert!(buffers
        .alias(&mut memory, handle, 0x57001000, EXECUTE)
        .is_err());
    buffers.free(&mut memory, handle).unwrap();
    assert!(memory.read_int(0x57000003, 1).is_err());
    assert!(native.with_native(|_| ()).is_err());
    assert_eq!(buffers.live_bytes(), 0);
}
#[test]
fn failed_reallocation_and_alias_leave_the_original_buffer_intact() {
    let mut memory = Memory::default();
    let mut buffers = SharedBuffers::new(fixture_session(), 64).unwrap();
    let handle = buffers.allocate(&mut memory, 32, 32).unwrap();
    memory
        .write_int(handle.address(), 8, 0x12345678abcdef)
        .unwrap();
    assert!(buffers.reallocate(&mut memory, handle, 65).is_err());
    assert!(buffers.reallocate(&mut memory, handle, 0).is_err());
    assert!(buffers
        .alias(&mut memory, handle, handle.address(), READ)
        .is_err());
    memory
        .protect_shared_data(handle.address(), WRITE, false)
        .unwrap();
    assert!(buffers.reallocate(&mut memory, handle, 48).is_err());
    memory
        .protect_shared_data(handle.address(), READ | WRITE, true)
        .unwrap();
    assert!(buffers.reallocate(&mut memory, handle, 48).is_err());
    assert_eq!(
        memory.read_int(handle.address(), 8).unwrap(),
        0x12345678abcdef
    );
    assert_eq!(buffers.live_bytes(), 32);
    assert!(buffers.allocate(&mut memory, 8, 3).is_err());
    assert!(buffers.allocate(&mut memory, 8, 8192).is_err());
    assert_eq!(buffers.live_bytes(), 32);
    buffers.retire_all(&mut memory).unwrap();
}
#[test]
fn reallocation_retires_all_old_aliases_views_and_tokens_but_retains_actual_storage() {
    let session = fixture_session();
    let mut memory = Memory::default();
    let mut buffers = SharedBuffers::new(session.clone(), 128).unwrap();
    let old = buffers.allocate(&mut memory, 48, 128).unwrap();
    memory
        .write_int(old.address(), 8, 0x12345678abcdef)
        .unwrap();
    buffers.alias(&mut memory, old, 0x57000000, READ).unwrap();
    let view = buffers.native_view(old).unwrap();
    let new = buffers.reallocate(&mut memory, old, 96).unwrap();
    assert_eq!(new.address() % 128, 0);
    assert_eq!(memory.read_int(new.address(), 8).unwrap(), 0x12345678abcdef);
    assert_eq!(memory.read_int(new.address() + 88, 8).unwrap(), 0);
    assert!(memory.read_int(old.address(), 1).is_err());
    assert!(memory.read_int(0x57000000, 1).is_err());
    assert!(buffers.free(&mut memory, old).is_err());
    assert!(view.with_native(|_| ()).is_err());
    session.begin_shutdown().unwrap();
    assert!(buffers.allocate(&mut memory, 1, 1).is_err());
    buffers.retire_all(&mut memory).unwrap();
    assert!(
        !session.cleanup_ready(),
        "retired native view still owns actual backing"
    );
    drop(view);
    assert!(session.cleanup_ready());
    session.cleaned_up().unwrap();
}
#[test]
fn wrong_memory_and_cross_arena_tokens_cannot_release_another_allocation() {
    let mut memory = Memory::default();
    let mut other_memory = Memory::default();
    let session = fixture_session();
    let mut buffers = SharedBuffers::new(session.clone(), 128).unwrap();
    let mut other = SharedBuffers::new(session.clone(), 128).unwrap();
    let handle = buffers.allocate(&mut memory, 8, 16).unwrap();
    assert!(buffers.free(&mut other_memory, handle).is_err());
    assert!(buffers.reallocate(&mut other_memory, handle, 16).is_err());
    assert!(other.free(&mut memory, handle).is_err());
    assert_eq!(memory.read_int(handle.address(), 8).unwrap(), 0);
    drop(buffers); // orphan mappings are revoked, but still retain the VM owner.
    assert!(memory.read_int(handle.address(), 1).is_err());
    session.begin_shutdown().unwrap();
    assert!(!session.cleanup_ready());
    drop(memory);
    assert!(session.cleanup_ready());
}
#[test]
fn shutdown_and_free_wait_for_real_native_access_and_retained_view() {
    use std::sync::mpsc;
    let session = fixture_session();
    let mut memory = Memory::default();
    let mut buffers = SharedBuffers::new(session.clone(), 128).unwrap();
    let handle = buffers.allocate(&mut memory, 16, 16).unwrap();
    let view = buffers.native_view(handle).unwrap();
    let (started, ready) = mpsc::channel();
    let (release, done) = mpsc::channel();
    let remote = std::thread::spawn(move || {
        view.with_native(|p| {
            unsafe {
                p.as_ptr().write(17);
            }
            started.send(()).unwrap();
            done.recv().unwrap();
            // This write is allowed because its call/region lease began before
            // shutdown. Storage cannot be cleaned up while it remains active.
            unsafe {
                p.as_ptr().add(1).write(18);
            }
        })
        .unwrap();
        view
    });
    ready.recv().unwrap();
    session.begin_shutdown().unwrap();
    assert!(!session.cleanup_ready());
    release.send(()).unwrap();
    buffers.free(&mut memory, handle).unwrap();
    let view = remote.join().unwrap();
    assert!(!session.cleanup_ready());
    assert!(view.with_native(|_| ()).is_err());
    drop(view);
    assert!(session.cleanup_ready());
}
