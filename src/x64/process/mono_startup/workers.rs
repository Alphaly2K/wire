//! Bounded diagnostic Mono TLS workers. Commands contain scalars or checked
//! non-moving metadata capabilities; managed Thread roots stay on their OS thread.
//! Zero-argument metadata-qualified instance IL uses rooted receivers. Fast
//! domain switching and ordinary worker engine callbacks are not admitted.
use super::{Object, ROOT_TOKEN};
use std::{
    cell::Cell,
    collections::BTreeMap,
    ptr,
    sync::mpsc::{self, Receiver, SyncSender},
    thread::{self, JoinHandle, ThreadId},
};

const CAPACITY: usize = 32;
const TOKEN_BASE: u64 = ROOT_TOKEN + 0x1000;

#[derive(Clone, Copy)]
struct Api {
    domain: unsafe extern "C" fn() -> Object,
    root: unsafe extern "C" fn() -> Object,
    current: unsafe extern "C" fn() -> Object,
    attach: unsafe extern "C" fn(Object) -> Object,
    detach: unsafe extern "C" fn(Object),
    strong: unsafe extern "C" fn(Object, i32) -> Object,
    target: unsafe extern "C" fn(Object) -> Object,
    free: unsafe extern "C" fn(Object),
    enter_safe: unsafe extern "C" fn(*mut Object) -> Object,
    exit_safe: unsafe extern "C" fn(Object, *mut Object),
    collect: unsafe extern "C" fn(i32),
    max_generation: unsafe extern "C" fn() -> i32,
}
impl Api {
    fn load(library: Object) -> Result<Self, String> {
        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return Err(concat!("missing native worker API: ", $name).into());
                }
                unsafe { std::mem::transmute::<Object, $ty>(p) }
            }};
        }
        Ok(Self {
            domain: sym!("mono_domain_get", unsafe extern "C" fn() -> Object),
            root: sym!("mono_get_root_domain", unsafe extern "C" fn() -> Object),
            current: sym!("mono_thread_current", unsafe extern "C" fn() -> Object),
            attach: sym!("mono_thread_attach", unsafe extern "C" fn(Object) -> Object),
            detach: sym!("mono_thread_detach", unsafe extern "C" fn(Object)),
            strong: sym!(
                "mono_gchandle_new_v2",
                unsafe extern "C" fn(Object, i32) -> Object
            ),
            target: sym!(
                "mono_gchandle_get_target_v2",
                unsafe extern "C" fn(Object) -> Object
            ),
            free: sym!("mono_gchandle_free_v2", unsafe extern "C" fn(Object)),
            enter_safe: sym!(
                "mono_threads_enter_gc_safe_region",
                unsafe extern "C" fn(*mut Object) -> Object
            ),
            exit_safe: sym!(
                "mono_threads_exit_gc_safe_region",
                unsafe extern "C" fn(Object, *mut Object)
            ),
            collect: sym!("mono_gc_collect", unsafe extern "C" fn(i32)),
            max_generation: sym!("mono_gc_max_generation", unsafe extern "C" fn() -> i32),
        })
    }
}
struct SafeWait<'a> {
    api: Api,
    cookie: Object,
    marker: &'a mut Object,
}
impl<'a> SafeWait<'a> {
    fn enter(api: Api, marker: &'a mut Object) -> Self {
        let cookie = unsafe { (api.enter_safe)(marker) };
        Self {
            api,
            cookie,
            marker,
        }
    }
}
impl Drop for SafeWait<'_> {
    fn drop(&mut self) {
        unsafe { (self.api.exit_safe)(self.cookie, self.marker) };
    }
}

#[derive(Clone, Copy)]
pub(super) enum Profiler {
    Gc(i32, i32),
    Resize(i64),
}
enum Command {
    Domain,
    Root,
    Current,
    Attach,
    Detach(u64),
    Metadata(super::metadata::WorkerRequest),
    Attributes(super::attributes::WorkerRequest),
    Allocation(super::instances::WorkerRequest),
    Invoke(super::constructors::WorkerRequest),
    Virtual(super::virtual_methods::WorkerRequest),
    Array(Box<super::reference_arrays::WorkerRequest>),
    Close,
}
enum Reply {
    Scalar(u64),
    Metadata(super::metadata::WorkerResult),
    Attributes(super::attributes::WorkerResult),
    Allocation(super::instances::WorkerAllocation),
    Virtual(super::virtual_methods::WorkerResult),
    Array(super::reference_arrays::WorkerAllocation),
}
struct Completion {
    result: Result<Reply, String>,
    attaches: usize,
    detaches: usize,
    attached: bool,
    token: u64,
}
enum Event {
    Completed(Completion),
    Profiler(Profiler, SyncSender<Result<(), String>>),
}
thread_local! {
    static EVENTS: Cell<*const SyncSender<Event>> = const { Cell::new(ptr::null()) };
}
/// Only the native worker that installed this TLS route can emit a profiler
/// request. It sends no managed pointers, and holds no shared-region lease.
pub(super) fn forward_profiler(event: Profiler) -> Result<bool, String> {
    EVENTS.with(|slot| {
        if slot.get().is_null() {
            return Ok(false);
        }
        let send = unsafe { &*slot.get() };
        let (reply, receive) = mpsc::sync_channel(1);
        let _shared_wait = super::burst_shared::suspend();
        send.send(Event::Profiler(event, reply))
            .map_err(|_| "native worker profiler owner closed")?;
        receive
            .recv()
            .map_err(|_| "native worker profiler reply closed")??;
        Ok(true)
    })
}

struct Attachment {
    api: Api,
    root: usize,
    handle: Object,
    token: u64,
    token_base: u64,
    attaches: usize,
    detaches: usize,
}
impl Attachment {
    fn current(&self) -> Result<Object, String> {
        // The frozen mono_thread_current asserts on unattached callers.
        if self.handle.is_null() {
            return Err("Mono worker current-thread query requires actual attachment".into());
        }
        let current = unsafe { (self.api.current)() };
        if current.is_null() || unsafe { (self.api.target)(self.handle) } != current {
            return Err("Mono worker current-thread strong identity changed".into());
        }
        Ok(current)
    }
    fn detach(&mut self) -> Result<(), String> {
        if !self.handle.is_null() {
            let current = self.current()?;
            unsafe { (self.api.free)(self.handle) };
            self.handle = ptr::null_mut();
            self.token = 0;
            unsafe { (self.api.detach)(current) };
            self.detaches += 1;
            if !unsafe { (self.api.domain)() }.is_null() {
                return Err("Mono worker detach did not clear its actual domain TLS".into());
            }
        }
        Ok(())
    }
    fn execute(&mut self, command: Command) -> Result<Reply, String> {
        if matches!(
            command,
            Command::Metadata(_)
                | Command::Attributes(_)
                | Command::Allocation(_)
                | Command::Invoke(_)
                | Command::Virtual(_)
                | Command::Array(_)
        ) {
            self.current()?;
            if unsafe { (self.api.domain)() } as usize != self.root {
                return Err("worker metadata requires its actual attached root domain".into());
            }
            return match command {
                Command::Metadata(request) => request.execute().map(Reply::Metadata),
                Command::Attributes(request) => request.execute().map(Reply::Attributes),
                Command::Allocation(request) => request.execute().map(Reply::Allocation),
                Command::Invoke(request) => request.execute().map(|()| Reply::Scalar(0)),
                Command::Virtual(request) => request.execute().map(Reply::Virtual),
                Command::Array(request) => (*request).execute().map(Reply::Array),
                _ => unreachable!(),
            };
        }
        self.lifecycle(&command).map(Reply::Scalar)
    }
    fn lifecycle(&mut self, command: &Command) -> Result<u64, String> {
        match command {
            Command::Domain => {
                let actual = unsafe { (self.api.domain)() };
                if actual.is_null() {
                    if !self.handle.is_null() {
                        return Err("attached Mono worker lost its domain TLS".into());
                    }
                    Ok(0)
                } else if actual as usize == self.root && !self.handle.is_null() {
                    Ok(ROOT_TOKEN)
                } else {
                    Err("Mono worker current domain is outside its admitted attachment".into())
                }
            }
            Command::Root => {
                if unsafe { (self.api.root)() } as usize != self.root {
                    return Err("Mono worker root-domain identity changed".into());
                }
                Ok(ROOT_TOKEN)
            }
            Command::Current => self.current().map(|_| self.token),
            Command::Attach => {
                if self.attaches >= 128 {
                    return Err("Mono worker attachment generation capacity exceeded".into());
                }
                let thread = unsafe { (self.api.attach)(self.root as Object) };
                if thread.is_null() || unsafe { (self.api.domain)() } as usize != self.root {
                    return Err("Mono worker actual root attachment failed".into());
                }
                if self.handle.is_null() {
                    self.handle = unsafe { (self.api.strong)(thread, 0) };
                    if self.handle.is_null() {
                        unsafe { (self.api.detach)(thread) };
                        return Err("Mono worker strong Thread root allocation failed".into());
                    }
                    self.attaches += 1;
                    self.token = self.token_base + self.attaches as u64 * 8;
                }
                if self.current()? != thread {
                    return Err("Mono worker attach/current identities differ".into());
                }
                Ok(self.token)
            }
            Command::Detach(token) => {
                if self.handle.is_null() || *token != self.token {
                    return Err("Mono worker detach requires its exact live Thread token".into());
                }
                self.detach()?;
                Ok(0)
            }
            Command::Close => {
                self.detach()?;
                Ok(0)
            }
            Command::Metadata(_)
            | Command::Attributes(_)
            | Command::Allocation(_)
            | Command::Invoke(_) => {
                unreachable!()
            }
            Command::Virtual(_) => unreachable!(),
            Command::Array(_) => unreachable!(),
        }
    }
}
fn run(
    api: Api,
    root: usize,
    token_base: u64,
    receive: Receiver<Command>,
    send: SyncSender<Event>,
) {
    let mut state = Attachment {
        api,
        root,
        handle: ptr::null_mut(),
        token: 0,
        token_base,
        attaches: 0,
        detaches: 0,
    };
    EVENTS.with(|slot| slot.set(&send));
    let mut next = receive.recv();
    while let Ok(command) = next {
        let closing = matches!(command, Command::Close);
        let result = state.execute(command);
        // Keep the marker at this stable address for the whole channel wait.
        let mut marker = ptr::null_mut();
        let safe = (!state.handle.is_null()).then(|| SafeWait::enter(api, &mut marker));
        let completion = Completion {
            result,
            attaches: state.attaches,
            detaches: state.detaches,
            attached: !state.handle.is_null(),
            token: state.token,
        };
        if send.send(Event::Completed(completion)).is_err() {
            drop(safe);
            break;
        }
        if closing {
            drop(safe);
            break;
        }
        // Receive the next command while GC Safe, then return to native C API.
        next = receive.recv();
        drop(safe);
    }
    if let Err(error) = state.detach() {
        eprintln!("NATIVE_MONO_WORKER_CLEANUP_REJECT: {error}");
        std::process::abort();
    }
    EVENTS.with(|slot| slot.set(ptr::null()));
}

struct Worker {
    send: SyncSender<Command>,
    receive: Receiver<Event>,
    join: Option<JoinHandle<()>>,
    attached: bool,
    attaches: usize,
    detaches: usize,
    token: u64,
}
impl Worker {
    fn start(api: Api, root: usize, ordinal: usize) -> Result<Self, String> {
        let (send, commands) = mpsc::sync_channel(1);
        let (events, receive) = mpsc::sync_channel(4);
        let join = thread::Builder::new()
            .name(format!("wire-mono-tls-{ordinal}"))
            .spawn(move || {
                run(
                    api,
                    root,
                    TOKEN_BASE + ordinal as u64 * 0x1000,
                    commands,
                    events,
                )
            })
            .map_err(|e| format!("cannot create native Mono TLS worker: {e}"))?;
        Ok(Self {
            send,
            receive,
            join: Some(join),
            attached: false,
            attaches: 0,
            detaches: 0,
            token: 0,
        })
    }
    fn request(
        &mut self,
        api: Api,
        command: Command,
        callback: &mut impl FnMut(Profiler) -> Result<(), String>,
    ) -> Result<Reply, String> {
        let _shared_wait = super::burst_shared::suspend();
        let mut marker = ptr::null_mut();
        let _safe = SafeWait::enter(api, &mut marker);
        self.send
            .send(command)
            .map_err(|_| "native Mono TLS worker command closed")?;
        let mut callback_error = None;
        loop {
            match self
                .receive
                .recv()
                .map_err(|_| "native Mono TLS worker result closed")?
            {
                Event::Completed(completion) => {
                    self.attached = completion.attached;
                    self.attaches = completion.attaches;
                    self.detaches = completion.detaches;
                    self.token = completion.token;
                    // Rooted allocation replies may be dropped on callback
                    // failure. Leave GC Safe before freeing their actual roots.
                    drop(_safe);
                    // Always wait for command completion before releasing a
                    // registry capability, even when a profiler reply failed.
                    return callback_error.map_or(completion.result, Err);
                }
                Event::Profiler(event, reply) => {
                    let result = callback(event);
                    let _ = reply.send(result.clone());
                    if let Err(error) = result {
                        callback_error.get_or_insert(error);
                    }
                }
            }
        }
    }
    fn scalar(
        &mut self,
        api: Api,
        command: Command,
        callback: &mut impl FnMut(Profiler) -> Result<(), String>,
    ) -> Result<u64, String> {
        match self.request(api, command, callback)? {
            Reply::Scalar(value) => Ok(value),
            _ => Err("worker lifecycle received non-scalar result".into()),
        }
    }
}
#[derive(Default)]
pub(super) struct State {
    api: Option<Api>,
    owner: Option<ThreadId>,
    root: usize,
    workers: BTreeMap<u64, Worker>,
    calls: usize,
    gc_verifications: usize,
    closed: bool,
}
impl State {
    pub(super) fn observed_lifecycle(
        &self,
        domain: Object,
        guest: u64,
        function: &str,
    ) -> Result<u64, String> {
        if self.closed
            || guest <= 1
            || domain.is_null()
            || self.root != 0 && self.root != domain as usize
            || self
                .owner
                .is_some_and(|owner| owner != thread::current().id())
        {
            return Err("cooperative worker lifecycle outside retained domain/owner".into());
        }
        let token = self
            .workers
            .get(&guest)
            .filter(|worker| self.owns_thread_token(guest, worker.token))
            .map(|worker| worker.token);
        match function {
            "mono_get_root_domain" => Ok(ROOT_TOKEN),
            "mono_domain_get" => Ok(if token.is_some() { ROOT_TOKEN } else { 0 }),
            "mono_thread_current" => token
                .ok_or_else(|| "cooperative current-thread query before actual attachment".into()),
            _ => Err("cooperative worker lifecycle is not a read-only query".into()),
        }
    }
    /// Attachment receipts are updated only by synchronous owner commands.
    /// Borrowing State freezes detach/re-attach until the temporary store ends;
    /// no managed Thread pointer or native TLS query crosses this boundary.
    pub(super) fn owns_thread_token(&self, guest: u64, token: u64) -> bool {
        !self.closed
            && guest > 1
            && token != 0
            && self.owner == Some(thread::current().id())
            && self.workers.get(&guest).is_some_and(|worker| {
                worker.attached && worker.token == token && worker.join.is_some()
            })
    }
    pub(super) fn handles(function: &str) -> bool {
        matches!(
            function,
            "mono_domain_get"
                | "mono_get_root_domain"
                | "mono_thread_current"
                | "mono_thread_attach"
                | "mono_thread_detach"
        )
    }
    pub(super) fn dispatch(
        &mut self,
        library: Object,
        root: Object,
        guest: u64,
        function: &str,
        argument: u64,
    ) -> Result<u64, String> {
        if self.closed || root.is_null() || guest <= 1 || !Self::handles(function) {
            return Err("native Mono worker command/domain/lifetime is not admitted".into());
        }
        if !matches!(
            std::env::var("MONO_THREADS_SUSPEND").ok().as_deref(),
            Some("preemptive")
        ) {
            return Err(
                "finite Mono TLS workers require the verified preemptive suspend mode".into(),
            );
        }
        if self
            .owner
            .is_some_and(|owner| owner != thread::current().id())
        {
            return Err("native Mono worker dispatch on foreign process owner".into());
        }
        let command = match function {
            "mono_domain_get" => Command::Domain,
            "mono_get_root_domain" => Command::Root,
            "mono_thread_current" => Command::Current,
            "mono_thread_attach" if argument == ROOT_TOKEN => Command::Attach,
            "mono_thread_attach" => {
                return Err("Mono worker attach requires the owned root-domain token".into())
            }
            "mono_thread_detach" => Command::Detach(argument),
            _ => unreachable!(),
        };
        let api = if let Some(api) = self.api {
            if self.root != root as usize {
                return Err("native Mono worker root lifetime changed".into());
            }
            api
        } else {
            let api = Api::load(library)?;
            self.api = Some(api);
            self.owner = Some(thread::current().id());
            self.root = root as usize;
            api
        };
        if !self.workers.contains_key(&guest) {
            if self.workers.len() >= CAPACITY {
                return Err("native Mono TLS worker capacity exceeded".into());
            }
            let worker = Worker::start(api, self.root, self.workers.len())?;
            self.workers.insert(guest, worker);
        }
        let result = self.workers.get_mut(&guest).unwrap().scalar(
            api,
            command,
            &mut super::callbacks::worker_profiler,
        )?;
        self.calls += 1;
        if !matches!(
            function,
            "mono_domain_get" | "mono_get_root_domain" | "mono_thread_current"
        ) || self.calls <= 16
            || self.calls.is_power_of_two()
        {
            eprintln!("NATIVE_MONO_WORKER_TLS: guest-thread={guest}; export={function}; opaque-result={result:#x}; calls={}", self.calls);
        }
        Ok(result)
    }
    pub(super) fn metadata(
        &mut self,
        guest: u64,
        request: super::metadata::WorkerRequest,
    ) -> Result<super::metadata::WorkerResult, String> {
        if self.closed || self.owner != Some(thread::current().id()) {
            return Err("worker metadata command outside live process owner".into());
        }
        let api = self.api.ok_or("worker metadata before native TLS setup")?;
        let worker = self
            .workers
            .get_mut(&guest)
            .ok_or("worker metadata before worker creation")?;
        if !worker.attached {
            return Err("worker metadata before actual root attachment".into());
        }
        match worker.request(
            api,
            Command::Metadata(request),
            &mut super::callbacks::worker_profiler,
        )? {
            Reply::Metadata(value) => {
                self.calls += 1;
                Ok(value)
            }
            _ => Err("worker metadata received mismatched result".into()),
        }
    }
    pub(super) fn attributes(
        &mut self,
        guest: u64,
        request: super::attributes::WorkerRequest,
    ) -> Result<super::attributes::WorkerResult, String> {
        if self.closed || self.owner != Some(thread::current().id()) {
            return Err("worker attributes outside live process owner".into());
        }
        let api = self
            .api
            .ok_or("worker attributes before native TLS setup")?;
        let worker = self
            .workers
            .get_mut(&guest)
            .ok_or("worker attributes before worker creation")?;
        if !worker.attached {
            return Err("worker attributes before actual root attachment".into());
        }
        match worker.request(
            api,
            Command::Attributes(request),
            &mut super::callbacks::worker_profiler,
        )? {
            Reply::Attributes(value) => {
                self.calls += 1;
                Ok(value)
            }
            _ => Err("worker attributes received mismatched result".into()),
        }
    }
    pub(super) fn allocate(
        &mut self,
        guest: u64,
        request: super::instances::WorkerRequest,
    ) -> Result<super::instances::WorkerAllocation, String> {
        if self.closed || self.owner != Some(thread::current().id()) {
            return Err("worker allocation outside live process owner".into());
        }
        let api = self
            .api
            .ok_or("worker allocation before native TLS setup")?;
        let worker = self
            .workers
            .get_mut(&guest)
            .filter(|w| w.attached)
            .ok_or("worker allocation before actual root attachment")?;
        match worker.request(
            api,
            Command::Allocation(request),
            &mut super::callbacks::worker_profiler,
        )? {
            Reply::Allocation(value) => {
                self.calls += 1;
                Ok(value)
            }
            _ => Err("worker allocation received mismatched result".into()),
        }
    }
    pub(super) fn invoke(
        &mut self,
        guest: u64,
        request: super::constructors::WorkerRequest,
    ) -> Result<(), String> {
        if self.closed || self.owner != Some(thread::current().id()) {
            return Err("worker invocation outside live process owner".into());
        }
        let api = self
            .api
            .ok_or("worker invocation before native TLS setup")?;
        let worker = self
            .workers
            .get_mut(&guest)
            .filter(|w| w.attached)
            .ok_or("worker invocation before actual root attachment")?;
        worker.scalar(
            api,
            Command::Invoke(request),
            &mut super::callbacks::worker_profiler,
        )?;
        self.calls += 1;
        Ok(())
    }
    pub(super) fn resolve_virtual(
        &mut self,
        guest: u64,
        request: super::virtual_methods::WorkerRequest,
    ) -> Result<super::virtual_methods::WorkerResult, String> {
        if self.closed || self.owner != Some(thread::current().id()) {
            return Err("worker virtual resolution outside live process owner".into());
        }
        let api = self
            .api
            .ok_or("worker virtual resolution before native TLS setup")?;
        let worker = self
            .workers
            .get_mut(&guest)
            .filter(|w| w.attached)
            .ok_or("worker virtual resolution before actual root attachment")?;
        match worker.request(
            api,
            Command::Virtual(request),
            &mut super::callbacks::worker_profiler,
        )? {
            Reply::Virtual(value) => {
                self.calls += 1;
                Ok(value)
            }
            _ => Err("worker virtual resolution received mismatched result".into()),
        }
    }
    pub(super) fn allocate_array(
        &mut self,
        guest: u64,
        request: super::reference_arrays::WorkerRequest,
    ) -> Result<super::reference_arrays::WorkerAllocation, String> {
        if self.closed || self.owner != Some(thread::current().id()) {
            return Err("worker array allocation outside live process owner".into());
        }
        let api = self
            .api
            .ok_or("worker array allocation before native TLS setup")?;
        let worker = self
            .workers
            .get_mut(&guest)
            .filter(|w| w.attached)
            .ok_or("worker array allocation before actual root attachment")?;
        match worker.request(
            api,
            Command::Array(Box::new(request)),
            &mut super::callbacks::worker_profiler,
        )? {
            Reply::Array(value) => {
                self.calls += 1;
                Ok(value)
            }
            _ => Err("worker array allocation received mismatched result".into()),
        }
    }
    pub(super) fn close(&mut self) -> Result<(), String> {
        if self.closed {
            return Ok(());
        }
        if self
            .owner
            .is_some_and(|owner| owner != thread::current().id())
        {
            return Err("native Mono TLS workers must close on their process owner".into());
        }
        if let Some(api) = self.api {
            for worker in self.workers.values_mut() {
                worker.scalar(api, Command::Close, &mut super::callbacks::worker_profiler)?;
                if worker.attached || worker.attaches != worker.detaches {
                    return Err("native Mono TLS worker attachment teardown is incomplete".into());
                }
                if let Some(join) = worker.join.take() {
                    join.join().map_err(|_| "native Mono TLS worker panicked")?;
                }
            }
        }
        self.closed = true;
        Ok(())
    }
    pub(super) fn verify_gc(&mut self) -> Result<(), String> {
        if self.closed || self.workers.values().all(|w| !w.attached) {
            return Ok(());
        }
        if self.owner != Some(thread::current().id()) {
            return Err("native Mono worker GC verification on foreign process owner".into());
        }
        let api = self
            .api
            .ok_or("native Mono worker GC verification has no API")?;
        unsafe { (api.collect)((api.max_generation)()) };
        for worker in self.workers.values_mut().filter(|w| w.attached) {
            let expected = worker.token;
            let current = worker.scalar(
                api,
                Command::Current,
                &mut super::callbacks::worker_profiler,
            )?;
            if current == 0 || current != expected {
                return Err("native Mono worker strong identity changed across actual GC".into());
            }
            self.gc_verifications += 1;
        }
        Ok(())
    }
    pub(super) fn report(&self) -> String {
        format!("native-worker-tls-calls={}\nnative-worker-tls-threads={}\nnative-worker-tls-attaches={}\nnative-worker-tls-detaches={}\nnative-worker-tls-live={}\nnative-worker-tls-gc-verifications={}\nnative-worker-tls-closed={}\n", self.calls, self.workers.len(), self.workers.values().map(|w| w.attaches).sum::<usize>(), self.workers.values().map(|w| w.detaches).sum::<usize>(), self.workers.values().filter(|w| w.attached).count(), self.gc_verifications, self.closed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const ROOT: usize = 0x1234;
    static NEXT_THREAD: AtomicUsize = AtomicUsize::new(0x55000000);
    thread_local! {
        static DOMAIN: Cell<usize> = const { Cell::new(0) };
        static THREAD: Cell<usize> = const { Cell::new(0) };
        static SAFE_MARKER: Cell<usize> = const { Cell::new(0) };
        static ROOTED: Cell<bool> = const { Cell::new(false) };
    }
    unsafe extern "C" fn domain() -> Object {
        DOMAIN.with(Cell::get) as Object
    }
    unsafe extern "C" fn root() -> Object {
        ROOT as Object
    }
    unsafe extern "C" fn current() -> Object {
        assert_eq!(DOMAIN.with(Cell::get), ROOT);
        THREAD.with(Cell::get) as Object
    }
    unsafe extern "C" fn attach(value: Object) -> Object {
        assert_eq!(value as usize, ROOT);
        assert_eq!(SAFE_MARKER.with(Cell::get), 0);
        DOMAIN.with(|v| v.set(ROOT));
        THREAD.with(|v| {
            if v.get() == 0 {
                v.set(NEXT_THREAD.fetch_add(8, Ordering::Relaxed));
            }
        });
        // Attach can cause collector callbacks. This synchronous queue must
        // execute on the owner while the native worker waits without VM access.
        assert!(forward_profiler(Profiler::Resize(4096)).unwrap());
        unsafe { current() }
    }
    unsafe extern "C" fn detach(value: Object) {
        assert_eq!(value, unsafe { current() });
        assert!(!ROOTED.with(Cell::get));
        DOMAIN.with(|v| v.set(0));
        THREAD.with(|v| v.set(0));
    }
    unsafe extern "C" fn strong(value: Object, pinned: i32) -> Object {
        assert_eq!(value, unsafe { current() });
        assert_eq!(pinned, 0);
        assert!(!ROOTED.with(|v| v.replace(true)));
        value
    }
    unsafe extern "C" fn target(value: Object) -> Object {
        assert!(ROOTED.with(Cell::get));
        assert_eq!(value, unsafe { current() });
        value
    }
    unsafe extern "C" fn free(value: Object) {
        assert_eq!(value, unsafe { current() });
        assert!(ROOTED.with(|v| v.replace(false)));
    }
    unsafe extern "C" fn enter_safe(marker: *mut Object) -> Object {
        assert_eq!(SAFE_MARKER.with(|v| v.replace(marker as usize)), 0);
        marker.cast()
    }
    unsafe extern "C" fn exit_safe(cookie: Object, marker: *mut Object) {
        assert_eq!(cookie, marker.cast());
        assert_eq!(SAFE_MARKER.with(|v| v.replace(0)), marker as usize);
    }
    unsafe extern "C" fn collect(_: i32) {}
    unsafe extern "C" fn max_generation() -> i32 {
        0
    }
    fn api() -> Api {
        Api {
            domain,
            root,
            current,
            attach,
            detach,
            strong,
            target,
            free,
            enter_safe,
            exit_safe,
            collect,
            max_generation,
        }
    }
    #[test]
    fn os_worker_tls_isolation_live_tokens_gc_queue_and_joined_detach() {
        let api = api();
        let owner = thread::current().id();
        let mut first = Worker::start(api, ROOT, 0).unwrap();
        let mut second = Worker::start(api, ROOT, 1).unwrap();
        assert_ne!(first.join.as_ref().unwrap().thread().id(), owner);
        assert_ne!(
            first.join.as_ref().unwrap().thread().id(),
            second.join.as_ref().unwrap().thread().id()
        );
        let mut profiler_calls = 0;
        let mut callback = |event| {
            assert_eq!(thread::current().id(), owner);
            assert!(matches!(event, Profiler::Resize(4096)));
            assert_ne!(SAFE_MARKER.with(Cell::get), 0);
            profiler_calls += 1;
            Ok(())
        };
        assert_eq!(
            first.scalar(api, Command::Domain, &mut callback).unwrap(),
            0
        );
        assert_eq!(
            first.scalar(api, Command::Root, &mut callback).unwrap(),
            ROOT_TOKEN
        );
        assert!(first.scalar(api, Command::Current, &mut callback).is_err());
        let old = first.scalar(api, Command::Attach, &mut callback).unwrap();
        let other = second.scalar(api, Command::Attach, &mut callback).unwrap();
        assert_ne!(old, other);
        assert_eq!(
            first.scalar(api, Command::Domain, &mut callback).unwrap(),
            ROOT_TOKEN
        );
        assert_eq!(
            first.scalar(api, Command::Current, &mut callback).unwrap(),
            old
        );
        assert!(first
            .scalar(api, Command::Detach(other), &mut callback)
            .is_err());
        assert!(first.attached);
        first
            .scalar(api, Command::Detach(old), &mut callback)
            .unwrap();
        assert_eq!(
            first.scalar(api, Command::Domain, &mut callback).unwrap(),
            0
        );
        assert_eq!(
            second.scalar(api, Command::Current, &mut callback).unwrap(),
            other
        );
        let new = first.scalar(api, Command::Attach, &mut callback).unwrap();
        assert_ne!(old, new);
        assert!(first
            .scalar(api, Command::Detach(old), &mut callback)
            .is_err());
        for worker in [&mut first, &mut second] {
            worker.scalar(api, Command::Close, &mut callback).unwrap();
            worker.join.take().unwrap().join().unwrap();
            assert!(!worker.attached);
            assert_eq!(worker.attaches, worker.detaches);
        }
        assert_eq!(profiler_calls, 3);
        assert_eq!(DOMAIN.with(Cell::get), 0);
        assert_eq!(SAFE_MARKER.with(Cell::get), 0);
        assert!(!forward_profiler(Profiler::Resize(1)).unwrap());
    }
    #[test]
    fn temporary_thread_tokens_follow_exact_guest_attachment_generation_and_owner() {
        let api = api();
        let mut state = State {
            api: Some(api),
            owner: Some(thread::current().id()),
            root: ROOT,
            ..State::default()
        };
        state
            .workers
            .insert(3, Worker::start(api, ROOT, 0).unwrap());
        state
            .workers
            .insert(7, Worker::start(api, ROOT, 1).unwrap());
        assert_eq!(
            state
                .observed_lifecycle(ROOT as Object, 3, "mono_domain_get")
                .unwrap(),
            0
        );
        assert!(state
            .observed_lifecycle(ROOT as Object, 3, "mono_thread_current")
            .is_err());
        assert_eq!(
            state
                .observed_lifecycle(ROOT as Object, 3, "mono_get_root_domain")
                .unwrap(),
            ROOT_TOKEN
        );
        let mut callback = |_| Ok(());
        let token = state
            .workers
            .get_mut(&3)
            .unwrap()
            .scalar(api, Command::Attach, &mut callback)
            .unwrap();
        assert!(state.owns_thread_token(3, token));
        assert_eq!(
            state
                .observed_lifecycle(ROOT as Object, 3, "mono_thread_current")
                .unwrap(),
            token
        );
        assert_eq!(
            state
                .observed_lifecycle(ROOT as Object, 3, "mono_domain_get")
                .unwrap(),
            ROOT_TOKEN
        );
        assert!(state
            .observed_lifecycle((ROOT + 1) as Object, 3, "mono_thread_current")
            .is_err());
        assert!(state
            .observed_lifecycle(ROOT as Object, 1, "mono_domain_get")
            .is_err());
        assert!(state
            .observed_lifecycle(ROOT as Object, 3, "mono_thread_detach")
            .is_err());
        for (guest, value) in [(1, token), (7, token), (3, token + 8), (3, 0)] {
            assert!(!state.owns_thread_token(guest, value));
        }
        state.owner = None;
        assert!(!state.owns_thread_token(3, token));
        state.owner = Some(thread::current().id());
        state
            .workers
            .get_mut(&3)
            .unwrap()
            .scalar(api, Command::Detach(token), &mut callback)
            .unwrap();
        assert!(!state.owns_thread_token(3, token));
        let current = state
            .workers
            .get_mut(&3)
            .unwrap()
            .scalar(api, Command::Attach, &mut callback)
            .unwrap();
        assert_ne!(token, current);
        assert!(!state.owns_thread_token(3, token));
        assert!(state.owns_thread_token(3, current));
        state.close().unwrap();
        assert!(!state.owns_thread_token(3, current));
        assert!(state
            .observed_lifecycle(ROOT as Object, 3, "mono_get_root_domain")
            .is_err());
    }
}
