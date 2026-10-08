//! Two native Mono threads hand off a single guest execution owner.
use super::*;
use std::{
    sync::{mpsc, Mutex},
    time::Duration,
};

struct ConcurrentContext {
    guest: Mutex<Guest>,
    address: u64,
    ready: mpsc::Sender<(i32, usize)>,
    totals: Mutex<[i64; 2]>,
    current: unsafe extern "C" fn() -> Object,
}
thread_local! { static CONCURRENT: Cell<*const ConcurrentContext> = const { Cell::new(ptr::null()) }; }
pub(super) unsafe extern "C" fn guest_thread_ready(id: i32) {
    let result = std::panic::catch_unwind(|| {
        CONCURRENT.with(|slot| {
            let context = unsafe { &*slot.get() };
            context
                .ready
                .send((id, unsafe { (context.current)() } as usize))
                .unwrap();
        })
    });
    if result.is_err() {
        std::process::abort();
    }
}
pub(super) unsafe extern "C" fn guest_concurrent_callback(id: i32) -> i32 {
    let result = std::panic::catch_unwind(|| {
        CONCURRENT.with(|slot| {
            let context = unsafe { &*slot.get() };
            assert!((1..=2).contains(&id));
            // VM execution/waiting holds no mutable Guest reference or owner lock.
            // Only the short interpreted callback claims the single Guest owner.
            let mut guest = context.guest.lock().unwrap();
            let rsp = STACK + 0x2ff8;
            guest.memory.write_int(rsp, 8, RETURN).unwrap();
            let mut cpu = Cpu64::new(SHARED_CALLBACK, rsp);
            cpu.regs[1] = context.address;
            cpu.regs[2] = id as u64;
            for _ in 0..16 {
                if cpu.rip == RETURN {
                    guest.callbacks += 1;
                    context.totals.lock().unwrap()[(id - 1) as usize] += cpu.regs[0] as i64;
                    return cpu.regs[0] as i32;
                }
                cpu.step(&mut guest.memory).unwrap();
            }
            panic!("concurrent guest callback did not return");
        })
    });
    match result {
        Ok(n) => n,
        Err(_) => std::process::abort(),
    }
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
    let current = symbol!(
        mono.library,
        "mono_thread_current",
        unsafe extern "C" fn() -> Object
    );
    let array_addr = symbol!(
        mono.library,
        "mono_array_addr_with_size",
        unsafe extern "C" fn(Object, i32, usize) -> Object
    );
    let unbox = symbol!(
        mono.library,
        "mono_object_unbox",
        unsafe extern "C" fn(Object) -> Object
    );
    let array = mono.invoke(mono.find("NewArray", 0), ptr::null_mut());
    let handle = unsafe { (mono.new_handle)(array, 1) };
    let owner = Arc::new(PinnedOwner {
        domain: mono.domain as usize,
        handle: handle as usize,
        attach,
        detach,
        current_domain: symbol!(
            mono.library,
            "mono_domain_get",
            unsafe extern "C" fn() -> Object
        ),
        free: mono.free_handle,
    });
    let pointer = NonNull::new(unsafe { array_addr(array, 4, 0) }.cast::<u8>()).unwrap();
    let address = pointer.as_ptr() as u64;
    // Workers never modify this scalar array in managed code. GC cannot write
    // pinned int payload; every callback/host payload access uses the region.
    let region = unsafe { HostRegion::foreign(pointer, 16, owner.clone()) }.unwrap();
    let bytes = pe_fixture();
    let pe = Pe64::parse(&bytes).unwrap();
    let mut memory = pe
        .load(&bytes, BASE, |_, _| Err("unexpected import".into()))
        .unwrap();
    memory.map(STACK, 0x3000, READ | WRITE).unwrap();
    memory
        .map_shared_data(address, region.clone(), READ | WRITE, READ | WRITE)
        .unwrap();
    memory.write_int(address, 4, 0).unwrap();
    let (ready, receive) = mpsc::channel();
    let context = Arc::new(ConcurrentContext {
        guest: Mutex::new(Guest {
            memory,
            callbacks: 0,
            paused_regs: [0; 16],
            checked_calls: [0; 4],
            memory_faults: Vec::new(),
        }),
        address,
        ready,
        totals: Mutex::new([0; 2]),
        current,
    });
    let method = mono.find("ConcurrentWorker", 1) as usize;
    let domain = mono.domain as usize;
    let invoke = mono.invoke;
    let mut workers = Vec::new();
    for id in 1..=2 {
        let context = context.clone();
        workers.push(std::thread::spawn(move || unsafe {
            let thread = attach(domain as Object);
            CONCURRENT.with(|slot| slot.set(Arc::as_ptr(&context)));
            let mut value = id;
            let mut args = [(&mut value as *mut i32).cast()];
            let mut exception = ptr::null_mut();
            let boxed = invoke(
                method as Object,
                ptr::null_mut(),
                args.as_mut_ptr(),
                &mut exception,
            );
            assert!(
                exception.is_null(),
                "blocked Mono worker raised an exception"
            );
            let total = *(unbox(boxed) as *const i32);
            CONCURRENT.with(|slot| slot.set(ptr::null()));
            detach(thread);
            total
        }));
    }
    let first = receive.recv_timeout(Duration::from_secs(5)).unwrap();
    let second = receive.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_ne!(first.0, second.0);
    assert_ne!(first.1, second.1);
    assert_ne!(first.1, unsafe { current() } as usize);
    // Forced GC while both worker methods are waiting, then host -> managed
    // Set releases the waiters. The Guest owner is free during both waits.
    unsafe {
        (mono.collect)(0);
    }
    let release = mono.invoke(mono.find("ReleaseWorkers", 0), ptr::null_mut());
    assert_eq!(unsafe { *(unbox(release) as *const i32) }, 1);
    for (index, worker) in workers.into_iter().enumerate() {
        assert_eq!(
            worker.join().unwrap() as i64,
            context.totals.lock().unwrap()[index]
        );
    }
    let mut guest = context.guest.lock().unwrap();
    assert_eq!(guest.callbacks, 200);
    assert_eq!(guest.memory.read_int(address, 4).unwrap(), 300);
    region
        .with_native(|p| assert_eq!(unsafe { p.as_ptr().cast::<i32>().read_unaligned() }, 300))
        .unwrap();
    region.revoke().unwrap();
    drop(guest.memory.unmap_shared_data(address).unwrap());
    drop(guest);
    drop(context);
    drop(region);
    drop(owner);
    println!("PASS two attached Mono threads: distinct TLS, managed wait/release, 200 serialized guest callbacks, forced GC, shared checksum=300");
}
