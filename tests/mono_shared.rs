//! Safety contracts for the optional native Mono data bridge.
#![cfg(feature = "native-mono-memory")]
use std::{
    ptr::NonNull,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};
use wire::x64::{
    cpu::Cpu64,
    memory::{shared::HostRegion, Memory, EXECUTE, READ, WRITE},
};
const BASE: u64 = 0x100003;

#[test]
fn native_and_guest_use_identical_bytes_and_callback_reentry_is_allowed() {
    let region = HostRegion::owned(8192).unwrap();
    let mut memory = Memory::default();
    memory
        .map_shared_data(BASE, region.clone(), READ | WRITE, READ | WRITE)
        .unwrap();
    memory
        .write_int(BASE + 4093, 8, 0x123456789abcdef0)
        .unwrap();
    region
        .with_native(|p| unsafe {
            assert_eq!(
                p.as_ptr().add(4093).cast::<u64>().read_unaligned(),
                0x123456789abcdef0
            );
            p.as_ptr().cast::<u32>().write_unaligned(17);
            // Equivalent to native IL -> guest callback while native holds a lease.
            assert_eq!(memory.read_int(BASE, 4).unwrap(), 17);
            memory.write_int(BASE + 4, 4, 29).unwrap();
            assert_eq!(p.as_ptr().add(4).cast::<u32>().read_unaligned(), 29);
        })
        .unwrap();
}

#[test]
fn guest_cpu_can_load_and_store_shared_data_but_never_execute_it() {
    let region = HostRegion::owned(16).unwrap();
    let mut memory = Memory::default();
    memory
        .map_shared_data(BASE, region.clone(), READ | WRITE, READ | WRITE)
        .unwrap();
    memory.map(0x200000, 4096, READ | WRITE | EXECUTE).unwrap();
    // mov eax,[rcx]; add eax,11; mov [rcx+4],eax
    memory
        .write(0x200000, &[0x8b, 0x01, 0x83, 0xc0, 11, 0x89, 0x41, 4])
        .unwrap();
    memory.write_int(BASE, 4, 31).unwrap();
    let mut cpu = Cpu64::new(0x200000, 0);
    cpu.regs[1] = BASE;
    for _ in 0..3 {
        cpu.step(&mut memory).unwrap();
    }
    assert_eq!(memory.read_int(BASE + 4, 4).unwrap(), 42);
    cpu.rip = BASE;
    assert!(cpu.step(&mut memory).is_err());
    assert!(memory
        .protect_shared_data(BASE, READ | EXECUTE, false)
        .is_err());
    assert!(Memory::default()
        .map_shared_data(BASE, region, EXECUTE, EXECUTE)
        .is_err());
}

#[test]
fn exact_bounds_readonly_noaccess_and_crossing_faults_are_atomic() {
    let region = HostRegion::owned(8192).unwrap();
    let mut memory = Memory::default();
    memory
        .map_shared_data(BASE, region, READ | WRITE, READ | WRITE)
        .unwrap();
    memory.write(BASE, &vec![0x55; 8192]).unwrap();
    for (at, len) in [(BASE - 1, 4), (BASE + 8190, 4), (u64::MAX - 1, 8)] {
        assert!(memory.write(at, &vec![0xaa; len]).is_err());
        let mut output = vec![0xcc; len];
        assert!(memory.read(at, &mut output).is_err());
        assert_eq!(output, vec![0xcc; len]);
    }
    assert_eq!(memory.read_int(BASE, 8).unwrap(), 0x5555555555555555);
    assert_eq!(memory.read_int(BASE + 8184, 8).unwrap(), 0x5555555555555555);
    memory.protect_shared_data(BASE, READ, false).unwrap();
    assert!(memory.write_int(BASE + 4092, 8, 0).is_err());
    assert_eq!(memory.read_int(BASE + 4092, 8).unwrap(), 0x5555555555555555);
    memory.protect_shared_data(BASE, 0, false).unwrap();
    assert!(memory.read_int(BASE, 1).is_err());
    assert!(memory.write_int(BASE, 1, 0).is_err());
    memory.read(BASE, &mut []).unwrap();
    memory.write(BASE, &[]).unwrap();
}

#[test]
fn guard_is_one_shot_per_touched_page_without_partial_writes() {
    let mut memory = Memory::default();
    memory
        .map_shared_data(
            BASE,
            HostRegion::owned(8192).unwrap(),
            READ | WRITE,
            READ | WRITE,
        )
        .unwrap();
    memory.write(BASE, &vec![0x55; 8192]).unwrap();
    memory
        .protect_shared_data(BASE, READ | WRITE, true)
        .unwrap();
    memory.read(BASE, &mut []).unwrap(); // Does not consume the first guard.
    let at = (BASE & !4095) + 4092;
    // First faults on page 0, then page 1. Neither writes even the first byte.
    assert!(memory
        .write_int(at, 8, 0)
        .unwrap_err()
        .contains("guard-page"));
    assert!(memory
        .write_int(at, 8, 0)
        .unwrap_err()
        .contains("guard-page"));
    assert_eq!(memory.read_int(at, 8).unwrap(), 0x5555555555555555);
    memory.write_int(at, 8, 42).unwrap();
    assert_eq!(memory.read_int(at, 8).unwrap(), 42);
    assert!(memory
        .read_int(BASE + 8191, 1)
        .unwrap_err()
        .contains("guard-page"));
    assert_eq!(memory.read_int(BASE + 8191, 1).unwrap(), 0x55);
}

#[test]
fn independent_aliases_serialize_and_revocation_invalidates_every_view() {
    let region = HostRegion::owned(8).unwrap();
    let mut joins = Vec::new();
    for i in 0..2 {
        let region = region.clone();
        joins.push(std::thread::spawn(move || {
            let mut memory = Memory::default();
            let at = BASE + i * 0x10000;
            memory
                .map_shared_data(at, region.clone(), READ | WRITE, READ | WRITE)
                .unwrap();
            for _ in 0..1000 {
                region
                    .with_native(|_| {
                        let n = memory.read_int(at, 8).unwrap();
                        memory.write_int(at, 8, n + 1).unwrap();
                    })
                    .unwrap();
            }
            memory
        }));
    }
    let a = joins.remove(0).join().unwrap();
    let mut b = joins.remove(0).join().unwrap();
    assert_eq!(a.read_int(BASE, 8).unwrap(), 2000);
    region.revoke().unwrap();
    assert!(a.read_int(BASE, 8).is_err());
    assert!(b.write_int(BASE + 0x10000, 8, 0).is_err());
    assert!(region
        .with_native(|_| panic!("revoked native callback ran"))
        .is_err());
}

#[test]
fn competing_native_lease_blocks_guest_and_panic_releases_lease() {
    let region = HostRegion::owned(8).unwrap();
    let (read_tx, read_rx) = mpsc::channel();
    let mut memory = Memory::default();
    memory
        .map_shared_data(BASE, region.clone(), READ | WRITE, READ | WRITE)
        .unwrap();
    let native = region.clone();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let join = std::thread::spawn(move || {
        native
            .with_native(|_| {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            })
            .unwrap()
    });
    entered_rx.recv().unwrap();
    let guest = std::thread::spawn(move || {
        read_tx.send(memory.read_int(BASE, 8)).unwrap();
    });
    assert!(read_rx.recv_timeout(Duration::from_millis(25)).is_err());
    release_tx.send(()).unwrap();
    assert_eq!(
        read_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap(),
        0
    );
    guest.join().unwrap();
    join.join().unwrap();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        region.with_native(|_| panic!("fixture unwind")).unwrap();
    }));
    region.with_native(|_| ()).unwrap();
}

#[test]
fn foreign_owner_is_retained_until_last_mapping_and_unmap_handle_drop() {
    struct Owner {
        bytes: Box<[u8; 8]>,
        dropped: Arc<AtomicUsize>,
    }
    impl Drop for Owner {
        fn drop(&mut self) {
            self.dropped.fetch_add(1, Ordering::SeqCst);
        }
    }
    let dropped = Arc::new(AtomicUsize::new(0));
    let mut owner = Arc::new(Owner {
        bytes: Box::new([0; 8]),
        dropped: dropped.clone(),
    });
    let pointer = NonNull::new(Arc::get_mut(&mut owner).unwrap().bytes.as_mut_ptr()).unwrap();
    // No external alias touches bytes; owner drop is thread-safe.
    let region = unsafe { HostRegion::foreign(pointer, 8, owner.clone()) }.unwrap();
    let mut a = Memory::default();
    let mut b = Memory::default();
    a.map_shared_data(BASE, region.clone(), READ, READ).unwrap();
    b.map_shared_data(BASE, region.clone(), READ, READ).unwrap();
    drop(owner);
    drop(region);
    let retained = a.unmap_shared_data(BASE).unwrap();
    drop(a);
    drop(b);
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    drop(retained);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}

#[test]
fn virtual_alloc_merge_and_query_respect_registered_intervals() {
    let mut memory = Memory::default();
    let region = HostRegion::owned(16).unwrap();
    memory
        .map_shared_data(0x10003, region.clone(), READ, READ)
        .unwrap();
    assert!(!memory.available(0x10000, 4096));
    assert!(memory.map(0x10000, 4096, READ).is_err());
    assert!(memory.map_shared_data(0x10004, region, READ, READ).is_err());
    assert!(memory.virtual_query(0x10003).is_err()); // Cannot expose a whole host page.
    assert_eq!(
        u64::from_le_bytes(memory.virtual_query(0).unwrap()[24..32].try_into().unwrap()),
        0x10000
    );
    let alloc = memory.virtual_alloc(0, 4096, 0x3000, 4).unwrap();
    assert!(alloc >= 0x20000);
    let mut other = Memory::default();
    other
        .map_shared_data(
            BASE,
            HostRegion::owned(8).unwrap(),
            READ | WRITE,
            READ | WRITE,
        )
        .unwrap();
    other.write_int(BASE, 8, 99).unwrap();
    memory.merge(other).unwrap();
    assert_eq!(memory.read_int(BASE, 8).unwrap(), 99);
    let used = 4096 + 24;
    assert_eq!(
        u64::from_le_bytes(memory.memory_status()[16..24].try_into().unwrap()),
        512 * 1024 * 1024 - used
    );
    assert!(memory
        .map_shared_data((1 << 47) - 4, HostRegion::owned(8).unwrap(), READ, READ)
        .is_err());
    let mut overlapping = Memory::default();
    overlapping.map(BASE & !4095, 4096, READ).unwrap();
    assert!(memory.merge(overlapping).is_err());
}
