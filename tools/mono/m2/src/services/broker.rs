//! Process-owned Win32 service route. All queued captures/results are owned
//! Send values. Native/managed buffers and Runtime64 never cross the queue.
use super::*;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
type Job = Box<dyn FnOnce(&mut Services) + Send>;
#[derive(Clone, Copy)]
struct WaitApi {
    enter: unsafe extern "C" fn(*mut Object) -> Object,
    exit: unsafe extern "C" fn(Object, *mut Object),
}
struct Route {
    send: mpsc::SyncSender<Job>,
    api: WaitApi,
}
static ROUTE: Mutex<Option<Arc<Route>>> = Mutex::new(None);
struct SafeWait<'a> {
    api: WaitApi,
    cookie: Object,
    stack: &'a mut Object,
}
impl Drop for SafeWait<'_> {
    fn drop(&mut self) {
        unsafe { (self.api.exit)(self.cookie, self.stack) };
    }
}
pub(super) fn request<T: Send + 'static>(
    operation: impl FnOnce(&mut Services) -> Result<T, u32> + Send + 'static,
) -> Result<T, u32> {
    let route = ROUTE.lock().unwrap().clone().ok_or(50u32)?;
    request_route(route, operation)
}
fn request_route<T: Send + 'static>(
    route: Arc<Route>,
    operation: impl FnOnce(&mut Services) -> Result<T, u32> + Send + 'static,
) -> Result<T, u32> {
    let (reply, receive) = mpsc::sync_channel(1);
    let job: Job = Box::new(move |services| {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| operation(services)))
            .unwrap_or(Err(50));
        let _ = reply.send(result);
    });
    let mut marker = ptr::null_mut();
    let cookie = unsafe { (route.api.enter)(&mut marker) };
    let _safe = SafeWait {
        api: route.api,
        cookie,
        stack: &mut marker,
    };
    route.send.send(job).map_err(|_| 50u32)?;
    // Owned captures make revocation/cancellation safe; this wait never
    // returns a borrowed stack/payload pointer that a late job could access.
    receive.recv().map_err(|_| 50u32)?
}
pub(super) struct Queue {
    receive: mpsc::Receiver<Job>,
    route: Arc<Route>,
    pub jobs: usize,
}
impl Queue {
    fn new(mono: &Mono) -> Self {
        let api = WaitApi {
            enter: symbol!(
                mono.library,
                "mono_threads_enter_gc_safe_region",
                unsafe extern "C" fn(*mut Object) -> Object
            ),
            exit: symbol!(
                mono.library,
                "mono_threads_exit_gc_safe_region",
                unsafe extern "C" fn(Object, *mut Object)
            ),
        };
        let (send, receive) = mpsc::sync_channel(4);
        let route = Arc::new(Route { send, api });
        let mut global = ROUTE.lock().unwrap();
        assert!(global.is_none());
        *global = Some(route.clone());
        Self {
            receive,
            route,
            jobs: 0,
        }
    }
}
impl Drop for Queue {
    fn drop(&mut self) {
        let mut global = ROUTE.lock().unwrap();
        assert!(global
            .as_ref()
            .is_some_and(|route| Arc::ptr_eq(route, &self.route)));
        *global = None;
        // Receiver drop cancels pending owned jobs. Existing callers get a
        // checked error when their reply sender disappears, without VM access.
    }
}
pub(super) fn run(mono: &Mono, class: Object, filesystem: GuestFs) -> u64 {
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
    let method = unsafe { (mono.method)(class, c"ManagedThreadStreams".as_ptr(), 0) } as usize;
    assert_ne!(method, 0);
    let mut scope = Scope::new(filesystem);
    let mut queue = Queue::new(mono);
    let domain = mono.domain as usize;
    let invoke = mono.invoke;
    let worker = std::thread::spawn(move || unsafe {
        let thread = attach(domain as Object);
        // No Services TLS setup. The managed method starts its own native
        // Mono threads, which inherit the process route rather than a pointer.
        let mut exception = ptr::null_mut();
        let boxed = invoke(
            method as Object,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut exception,
        );
        assert!(
            exception.is_null() && !boxed.is_null(),
            "managed-created service threads failed"
        );
        let result = unbox(boxed).cast::<i32>().read_unaligned();
        detach(thread);
        result
    });
    let mut forced_gc = 0;
    while !worker.is_finished() {
        match queue.receive.recv_timeout(Duration::from_millis(10)) {
            Ok(job) => {
                if queue.jobs.is_multiple_of(7) {
                    unsafe { (mono.collect)(0) };
                    forced_gc += 1;
                }
                job(&mut scope.state);
                queue.jobs += 1;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("service owner queue disconnected: {error:?}"),
        }
    }
    assert_eq!(worker.join().unwrap(), 1);
    assert!(queue.jobs >= 10 && forced_gc >= 2);
    let jobs = queue.jobs;
    drop(queue);
    let calls = scope.state.calls;
    drop(scope);
    cancellation(mono);
    println!("PASS process-owned native Windows service queue: Mono-created threads, shared file handles/cwd/VFS, owned Send parameters/results, caller-side pinned copies/LastError, {jobs} jobs/{forced_gc} wait-GC, no Services TLS on workers; scope=fixture");
    calls
}

fn cancellation(mono: &Mono) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Capture(Arc<AtomicUsize>);
    impl Drop for Capture {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
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
    let queue = Queue::new(mono);
    let domain = mono.domain as usize;
    let dropped = Arc::new(AtomicUsize::new(0));
    let mut workers = Vec::new();
    // Every worker owns an in-flight route before revocation. Capacity four
    // exercises both send/reply cancellation; no capture borrows caller data.
    for _ in 0..12 {
        let route = queue.route.clone();
        let capture = Capture(dropped.clone());
        workers.push(std::thread::spawn(move || unsafe {
            let thread = attach(domain as Object);
            let result = request_route(route, move |_| -> Result<(), u32> {
                drop(capture);
                panic!("revoked service job executed");
            });
            assert_eq!(result, Err(50));
            detach(thread);
        }));
    }
    let pending = queue.receive.recv_timeout(Duration::from_secs(10)).unwrap();
    // Even a job already removed from the queue remains owned and can be
    // cancelled after route/receiver revocation without touching Mono buffers.
    drop(queue);
    drop(pending);
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(dropped.load(Ordering::SeqCst), 12);
    assert_eq!(request(|_| Ok(1i32)), Err(50));
    println!("PASS native service queue cancellation: 12 in-flight routes, pending/closed sends and replies, all owned captures dropped, zero jobs executed");
}
