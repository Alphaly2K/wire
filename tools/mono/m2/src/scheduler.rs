//! Explicit owner-thread queued guest callbacks and same-thread nested reentry.
//! Scalar fixture only; no worker ever receives a Guest/Memory pointer.
use super::*;
use std::{cell::RefCell, collections::BTreeSet, sync::mpsc, time::Duration};
#[derive(Clone, Copy)]
struct Api {
    invoke: Invoke,
    unbox: unsafe extern "C" fn(Object) -> Object,
    current: unsafe extern "C" fn() -> Object,
    collect: unsafe extern "C" fn(i32),
    enter_safe: unsafe extern "C" fn(*mut Object) -> Object,
    exit_safe: unsafe extern "C" fn(Object, *mut Object),
}
struct Request {
    value: i32,
    depth: i32,
    mono_thread: usize,
    reply: mpsc::SyncSender<Result<i32, &'static str>>,
}
struct Client {
    send: mpsc::SyncSender<Request>,
    api: Api,
}
struct Owner {
    guest: RefCell<Guest>,
    api: Api,
    nested: usize,
    owner_thread: usize,
    dispatches: Cell<usize>,
}
thread_local! {
    static CLIENT: Cell<*const Client> = const { Cell::new(ptr::null()) };
    static EXECUTOR: Cell<*const Owner> = const { Cell::new(ptr::null()) };
}
struct SafeWait<'a> {
    api: Api,
    cookie: Object,
    // Stable address retained for the complete enter/exit interval.
    stack: &'a mut Object,
}
impl<'a> SafeWait<'a> {
    unsafe fn enter(api: Api, stack: &'a mut Object) -> Self {
        let cookie = unsafe { (api.enter_safe)(stack) };
        Self { api, cookie, stack }
    }
}
impl Drop for SafeWait<'_> {
    fn drop(&mut self) {
        unsafe { (self.api.exit_safe)(self.cookie, &mut *self.stack) }
    }
}
pub(super) unsafe extern "C" fn queued_callback(value: i32, depth: i32) -> i32 {
    let result = std::panic::catch_unwind(|| {
        EXECUTOR.with(|slot| {
            if !slot.get().is_null() {
                return execute(unsafe { &*slot.get() }, value, depth);
            }
            CLIENT.with(|slot| {
                let pointer = slot.get();
                if pointer.is_null() {
                    return Err("callback has no execution owner");
                }
                let client = unsafe { &*pointer };
                let thread = unsafe { (client.api.current)() } as usize;
                let (reply, receive) = mpsc::sync_channel(1);
                // The whole bounded send + reply wait is in the runtime's GC
                // safe region. No managed pointer/native payload is accessed.
                let mut stack_marker = ptr::null_mut();
                let _safe = unsafe { SafeWait::enter(client.api, &mut stack_marker) };
                client
                    .send
                    .send(Request {
                        value,
                        depth,
                        mono_thread: thread,
                        reply,
                    })
                    .map_err(|_| "execution owner queue closed")?;
                receive
                    .recv_timeout(Duration::from_secs(10))
                    .map_err(|_| "execution owner reply unavailable")?
            })
        })
    });
    match result {
        Ok(Ok(value)) => value,
        other => {
            eprintln!("queued callback returned rejection: {other:?}");
            i32::MIN
        }
    }
}
fn execute(owner: &Owner, value: i32, depth: i32) -> Result<i32, &'static str> {
    if !(0..=2).contains(&depth) {
        return Err("queued recursion depth outside fixture capacity");
    }
    assert_eq!(
        unsafe { (owner.api.current)() } as usize,
        owner.owner_thread
    );
    owner.dispatches.set(owner.dispatches.get() + 1);
    // Distinct checked guest stack slices for each nested activation.
    let rsp = STACK + 0x8ff8 - depth as u64 * 0x2000;
    let mut cpu = Cpu64::new(if depth == 0 { CALLBACK } else { BASE + 0x1000 }, rsp);
    cpu.regs[1] = value as u32 as u64;
    cpu.regs[2] = depth as u64;
    owner
        .guest
        .borrow_mut()
        .memory
        .write_int(rsp, 8, RETURN)
        .unwrap();
    for _ in 0..64 {
        if cpu.rip == RETURN {
            owner.guest.borrow_mut().callbacks += 1;
            return Ok(cpu.regs[0] as i32);
        }
        if cpu.rip == GATEWAY {
            let mut value = cpu.regs[1] as i32;
            let mut depth = cpu.regs[2] as i32;
            let mut args = [
                (&mut value as *mut i32).cast(),
                (&mut depth as *mut i32).cast(),
            ];
            let mut exception = ptr::null_mut();
            // No RefCell borrow, mutable Guest reference, lease, or queue lock
            // survives this managed call. Its callback can run inline safely.
            let boxed = unsafe {
                (owner.api.invoke)(
                    owner.nested as Object,
                    ptr::null_mut(),
                    args.as_mut_ptr(),
                    &mut exception,
                )
            };
            if !exception.is_null() || boxed.is_null() {
                return Err("nested managed invocation failed");
            }
            cpu.regs[0] =
                unsafe { (owner.api.unbox)(boxed).cast::<i32>().read_unaligned() } as u32 as u64;
            cpu.rip = owner
                .guest
                .borrow()
                .memory
                .read_int(cpu.regs[4], 8)
                .unwrap();
            cpu.regs[4] += 8;
        } else {
            cpu.step(&mut owner.guest.borrow_mut().memory).unwrap();
        }
    }
    Err("queued guest callback did not return")
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
    let object_class = symbol!(
        mono.library,
        "mono_object_get_class",
        unsafe extern "C" fn(Object) -> Object
    );
    let class_name = symbol!(
        mono.library,
        "mono_class_get_name",
        unsafe extern "C" fn(Object) -> *const c_char
    );
    let api = Api {
        invoke: mono.invoke,
        unbox: symbol!(
            mono.library,
            "mono_object_unbox",
            unsafe extern "C" fn(Object) -> Object
        ),
        current: symbol!(
            mono.library,
            "mono_thread_current",
            unsafe extern "C" fn() -> Object
        ),
        collect: mono.collect,
        enter_safe: symbol!(
            mono.library,
            "mono_threads_enter_gc_safe_region",
            unsafe extern "C" fn(*mut Object) -> Object
        ),
        exit_safe: symbol!(
            mono.library,
            "mono_threads_exit_gc_safe_region",
            unsafe extern "C" fn(Object, *mut Object)
        ),
    };
    let mut bytes = pe_fixture();
    // Original Win64 guest: reserve shadow space, increment value/decrement
    // depth, call the managed data gateway, add5, restore frame, return.
    let mut code = vec![
        0x48, 0x83, 0xec, 0x28, 0x83, 0xc1, 1, 0x83, 0xea, 1, 0x48, 0xb8,
    ];
    code.extend_from_slice(&GATEWAY.to_le_bytes());
    code.extend_from_slice(&[0xff, 0xd0, 0x83, 0xc0, 5, 0x48, 0x83, 0xc4, 0x28, 0xc3]);
    bytes[512..512 + code.len()].copy_from_slice(&code);
    let pe = Pe64::parse(&bytes).unwrap();
    let mut memory = pe
        .load(&bytes, BASE, |_, _| {
            Err("unexpected queue fixture import".into())
        })
        .unwrap();
    memory.map(STACK, 0xa000, READ | WRITE).unwrap();
    let owner = Box::new(Owner {
        guest: RefCell::new(Guest {
            memory,
            callbacks: 0,
            paused_regs: [0; 16],
            checked_calls: [0; 4],
            memory_faults: Vec::new(),
        }),
        api,
        nested: mono.find("QueuedRoundTrip", 2) as usize,
        owner_thread: unsafe { (api.current)() } as usize,
        dispatches: Cell::new(0),
    });
    let (send, receive) = mpsc::sync_channel(4);
    let client = Arc::new(Client { send, api });
    let domain = mono.domain as usize;
    let method = mono.find("QueuedWorker", 1) as usize;
    let mut workers = Vec::new();
    for id in 1..=2 {
        let client = client.clone();
        workers.push(std::thread::spawn(move || unsafe {
            let thread = attach(domain as Object);
            CLIENT.with(|slot| slot.set(Arc::as_ptr(&client)));
            let mut id = id;
            let mut args = [(&mut id as *mut i32).cast()];
            let mut exception = ptr::null_mut();
            let boxed = (api.invoke)(
                method as Object,
                ptr::null_mut(),
                args.as_mut_ptr(),
                &mut exception,
            );
            assert!(
                exception.is_null() && !boxed.is_null(),
                "queued worker exception"
            );
            let result = (api.unbox)(boxed).cast::<i32>().read_unaligned();
            CLIENT.with(|slot| slot.set(ptr::null()));
            detach(thread);
            result
        }));
    }
    EXECUTOR.with(|slot| slot.set(&*owner));
    let mut identities = BTreeSet::new();
    let mut forced_gc = 0;
    for n in 0..100 {
        let request = receive.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_ne!(request.mono_thread, owner.owner_thread);
        identities.insert(request.mono_thread);
        if n % 10 == 0 {
            unsafe { (api.collect)(0) };
            forced_gc += 1;
        }
        let result = execute(&owner, request.value, request.depth);
        request.reply.send(result).unwrap();
    }
    EXECUTOR.with(|slot| slot.set(ptr::null()));
    for (n, worker) in workers.into_iter().enumerate() {
        assert_eq!(worker.join().unwrap(), 5000 * (n as i32 + 1) + 2825);
    }
    assert_eq!(identities.len(), 2);
    assert_eq!(owner.dispatches.get(), 300);
    assert_eq!(owner.guest.borrow().callbacks, 300);
    // Pending and newly submitted requests fail explicitly during teardown.
    let owner_method = owner.nested;
    let cancelled = |client: Arc<Client>| {
        std::thread::spawn(move || unsafe {
            let thread = attach(domain as Object);
            CLIENT.with(|slot| slot.set(Arc::as_ptr(&client)));
            let mut value = 9i32;
            let mut depth = 0i32;
            let mut args = [
                (&mut value as *mut i32).cast(),
                (&mut depth as *mut i32).cast(),
            ];
            let mut exception = ptr::null_mut();
            let result = (api.invoke)(
                owner_method as Object,
                ptr::null_mut(),
                args.as_mut_ptr(),
                &mut exception,
            );
            assert!(result.is_null() && !exception.is_null());
            let name = std::ffi::CStr::from_ptr(class_name(object_class(exception)));
            assert_eq!(name.to_str().unwrap(), "InvalidOperationException");
            CLIENT.with(|slot| slot.set(ptr::null()));
            detach(thread);
        })
    };
    let pending = cancelled(client.clone());
    receive
        .recv_timeout(Duration::from_secs(10))
        .unwrap()
        .reply
        .send(Err("execution owner revoked"))
        .unwrap();
    pending.join().unwrap();
    drop(receive);
    cancelled(client.clone()).join().unwrap();
    drop(client);
    println!("PASS queued owner-thread Mono callbacks: distinct worker TLS, bounded queue, 100 requests/300 nested guest activations, guest -> IL -> guest inline reentry, {forced_gc} GC during waits, managed local roots, pending/closed rejection; GC mode=preemptive");
}
