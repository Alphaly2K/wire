//! One-use static callback pool for an exact Int32 -> Int32 managed delegate.
//! Entry addresses are compiled into the binary and never recycled in a VM.
use super::*;
use std::sync::Mutex;
const CAPACITY: usize = 4;
#[derive(Clone, Copy)]
struct Api {
    invoke: Invoke,
    target: unsafe extern "C" fn(Handle) -> Object,
    unbox: unsafe extern "C" fn(Object) -> Object,
    class: unsafe extern "C" fn(Object) -> Object,
    class_name: unsafe extern "C" fn(Object) -> *const c_char,
    collect: unsafe extern "C" fn(i32),
}
struct Entry {
    owner: Arc<PinnedOwner>,
    method: usize,
    api: Api,
}
impl Entry {
    fn invoke(&self, value: i32) -> Result<i32, String> {
        let object = unsafe { (self.api.target)(self.owner.handle as Handle) };
        if object.is_null() {
            return Err("delegate root lost".into());
        }
        let mut value = value;
        let mut args = [(&mut value as *mut i32).cast()];
        let mut exception = ptr::null_mut();
        let boxed = unsafe {
            (self.api.invoke)(
                self.method as Object,
                object,
                args.as_mut_ptr(),
                &mut exception,
            )
        };
        if !exception.is_null() {
            return Err(unsafe {
                std::ffi::CStr::from_ptr((self.api.class_name)((self.api.class)(exception)))
            }
            .to_string_lossy()
            .into_owned());
        }
        if boxed.is_null() {
            return Err("delegate invocation returned no Int32".into());
        }
        Ok(unsafe { (self.api.unbox)(boxed).cast::<i32>().read_unaligned() })
    }
}
#[derive(Default)]
struct Pool {
    collect: Option<unsafe extern "C" fn(i32)>,
    entries: Mutex<Vec<Option<Arc<Entry>>>>,
    error: Mutex<Option<String>>,
}
impl Pool {
    fn install(&self, entry: Arc<Entry>) -> Result<usize, &'static str> {
        let mut entries = self.entries.lock().unwrap();
        if entries.len() == CAPACITY {
            return Err("static delegate callback capacity exhausted");
        }
        let slot = entries.len();
        entries.push(Some(entry));
        Ok(slot)
    }
    fn invoke(&self, slot: usize, value: i32) -> Result<i32, String> {
        // Clone the root owner and drop the registry lock before managed
        // execution; nested reentry/retirement cannot alias or deadlock it.
        let entry = self
            .entries
            .lock()
            .unwrap()
            .get(slot)
            .and_then(|entry| entry.clone())
            .ok_or_else(|| "delegate callback entry retired".to_string())?;
        entry.invoke(value)
    }
    fn retire(&self, slot: usize) -> bool {
        self.entries
            .lock()
            .unwrap()
            .get_mut(slot)
            .is_some_and(|entry| entry.take().is_some())
    }
}
thread_local! {static ACTIVE:Cell<*const Pool>=const{Cell::new(ptr::null())};}
fn callback(slot: usize, value: i32) -> i32 {
    let result = std::panic::catch_unwind(|| {
        ACTIVE.with(|active| {
            let pool = active.get();
            if pool.is_null() {
                return Err("delegate callback without pool owner".into());
            }
            unsafe { &*pool }.invoke(slot, value)
        })
    });
    match result {
        Ok(Ok(value)) => value,
        other => {
            let message = match other {
                Ok(Err(message)) => message,
                Err(_) => "delegate callback Rust failure".into(),
                _ => unreachable!(),
            };
            ACTIVE.with(|active| {
                if !active.get().is_null() {
                    *unsafe { &*active.get() }.error.lock().unwrap() = Some(message);
                }
            });
            i32::MIN
        }
    }
}
pub(super) unsafe extern "C" fn nested_callback(value: i32) -> i32 {
    callback(1, value)
}
pub(super) unsafe extern "C" fn retire_callback(slot: i32) -> i32 {
    let result = std::panic::catch_unwind(|| {
        ACTIVE.with(|active| {
            if active.get().is_null() || slot < 0 {
                return 0;
            }
            let pool = unsafe { &*active.get() };
            if !pool.retire(slot as usize) {
                return 0;
            }
            // The active callback clone still owns the delegate root during GC.
            unsafe { (pool.collect.expect("pool GC API missing"))(0) };
            1
        })
    });
    result.unwrap_or(0)
}
unsafe extern "C" fn entry0(value: i32) -> i32 {
    callback(0, value)
}
unsafe extern "C" fn entry1(value: i32) -> i32 {
    callback(1, value)
}
unsafe extern "C" fn entry2(value: i32) -> i32 {
    callback(2, value)
}
unsafe extern "C" fn entry3(value: i32) -> i32 {
    callback(3, value)
}
const ENTRIES: [unsafe extern "C" fn(i32) -> i32; CAPACITY] = [entry0, entry1, entry2, entry3];
pub(super) fn run(mono: &Mono) {
    let api = Api {
        collect: mono.collect,
        invoke: mono.invoke,
        target: mono.get_target,
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
        class_name: symbol!(
            mono.library,
            "mono_class_get_name",
            unsafe extern "C" fn(Object) -> *const c_char
        ),
    };
    let signature = symbol!(
        mono.library,
        "mono_method_signature",
        unsafe extern "C" fn(Object) -> Object
    );
    let ret = symbol!(
        mono.library,
        "mono_signature_get_return_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let params = symbol!(
        mono.library,
        "mono_signature_get_params",
        unsafe extern "C" fn(Object, *mut Object) -> Object
    );
    let count = symbol!(
        mono.library,
        "mono_signature_get_param_count",
        unsafe extern "C" fn(Object) -> u32
    );
    let kind = symbol!(
        mono.library,
        "mono_type_get_type",
        unsafe extern "C" fn(Object) -> i32
    );
    let byref = symbol!(
        mono.library,
        "mono_type_is_byref",
        unsafe extern "C" fn(Object) -> i32
    );
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
    let current_domain = symbol!(
        mono.library,
        "mono_domain_get",
        unsafe extern "C" fn() -> Object
    );
    let domain = mono.domain as usize;
    let factory = mono.find("NewScalarDelegate", 1) as usize;
    let new = mono.new_handle;
    let weak_new = mono.weak_handle;
    let free = mono.free_handle;
    let collect = mono.collect;
    let method = mono.method;
    // Creation/usage/retirement leave separate attached stacks so the GC
    // lifecycle evidence cannot be manufactured by stale factory stack words.
    let (pool, weaks) = std::thread::spawn(move || unsafe {
        let thread = attach(domain as Object);
        let pool = Box::new(Pool {
            collect: Some(api.collect),
            ..Pool::default()
        });
        let mut weaks = Vec::new();
        for slot in 0..CAPACITY {
            let mut seed = 100 * (slot as i32 + 1);
            let mut args = [(&mut seed as *mut i32).cast()];
            let mut exception = ptr::null_mut();
            let delegate = (api.invoke)(
                factory as Object,
                ptr::null_mut(),
                args.as_mut_ptr(),
                &mut exception,
            );
            assert!(!delegate.is_null() && exception.is_null());
            let invoke = method((api.class)(delegate), c"Invoke".as_ptr(), 1);
            assert!(!invoke.is_null());
            let sig = signature(invoke);
            assert_eq!(count(sig), 1);
            assert_eq!(kind(ret(sig)), 8); // MONO_TYPE_I4, no conversion/byref.
            assert_eq!(byref(ret(sig)), 0);
            let mut iter = ptr::null_mut();
            let param = params(sig, &mut iter);
            assert_eq!(kind(param), 8);
            assert_eq!(byref(param), 0);
            let weak = weak_new(delegate, 0);
            let owner = Arc::new(PinnedOwner {
                domain,
                handle: new(delegate, 0) as usize,
                attach,
                detach,
                current_domain,
                free,
            });
            assert_ne!(owner.handle, 0);
            let entry = Arc::new(Entry {
                owner,
                method: invoke as usize,
                api,
            });
            assert_eq!(pool.install(entry), Ok(slot));
            weaks.push(weak as usize);
        }
        detach(thread);
        (pool, weaks)
    })
    .join()
    .unwrap();
    let pool = std::thread::spawn(move || unsafe {
        let thread = attach(domain as Object);
        ACTIVE.with(|active| active.set(&*pool));
        for _ in 0..4 {
            collect(0);
        }
        for (slot, entry) in ENTRIES.iter().enumerate() {
            assert_eq!(entry(7), 100 * (slot as i32 + 1) + 7);
            assert_eq!(entry(5), 100 * (slot as i32 + 1) + 12);
        }
        assert_eq!(ENTRIES[1](-1), i32::MIN);
        assert_eq!(
            pool.error.lock().unwrap().take().unwrap(),
            "ArgumentOutOfRangeException"
        );
        assert_eq!(ENTRIES[1](3), 215); // Exception did not advance the closure.
        assert_eq!(ENTRIES[2](9), 536); // Real nested managed -> native -> managed callback.
        assert_eq!(ENTRIES[3](11), 423); // Real self-retirement + GC during an active callback.
        assert_eq!(ENTRIES[3](1), i32::MIN);
        assert_eq!(
            pool.error.lock().unwrap().take().unwrap(),
            "delegate callback entry retired"
        );
        let retained = pool.entries.lock().unwrap()[0].as_ref().unwrap().clone();
        assert!(pool.retire(0));
        assert_eq!(ENTRIES[0](1), i32::MIN);
        assert_eq!(
            pool.error.lock().unwrap().take().unwrap(),
            "delegate callback entry retired"
        );
        assert_eq!(retained.invoke(1).unwrap(), 113); // In-flight clone retains root.
        assert_eq!(
            pool.install(retained.clone()),
            Err("static delegate callback capacity exhausted")
        );
        drop(retained);
        for slot in 1..CAPACITY {
            pool.retire(slot);
        }
        ACTIVE.with(|active| active.set(ptr::null()));
        detach(thread);
        pool
    })
    .join()
    .unwrap();
    assert!(pool
        .entries
        .lock()
        .unwrap()
        .iter()
        .all(|entry| entry.is_none()));
    let dead = std::thread::spawn(move || unsafe {
        let thread = attach(domain as Object);
        for _ in 0..8 {
            collect(0);
        }
        let mut all_dead = true;
        for weak in weaks {
            all_dead &= (api.target)(weak as Handle).is_null();
            free(weak as Handle);
        }
        detach(thread);
        all_dead
    })
    .join()
    .unwrap();
    assert!(
        dead,
        "retired static callback pool retained a managed delegate"
    );
    assert_eq!(unsafe { ENTRIES[0](1) }, i32::MIN); // No owner installed, no VM access.
    println!("PASS one-use static Mono delegate callbacks: exact Int32 ABI, 4 independent closures, mutable captured state, rooted GC, exception status, nested reentry/self-retirement GC, retained in-flight owner, retired/no-owner rejection, exhaustion without slot reuse, all roots reclaimed; no runtime trampoline");
}
