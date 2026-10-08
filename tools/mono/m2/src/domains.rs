//! Native VM child-domain load/reset/unload prototype. Retained mappings and
//! handles are retired before domain metadata becomes invalid.
use super::*;
#[derive(Clone, Copy)]
struct Api {
    invoke: Invoke,
    method: Method,
    unbox: unsafe extern "C" fn(Object) -> Object,
    set: unsafe extern "C" fn(Object, i32) -> i32,
    class: unsafe extern "C" fn(Object) -> Object,
    class_name: unsafe extern "C" fn(Object) -> *const c_char,
}
fn invoke(api: Api, method: Object, params: &mut [Object], expected: Option<&str>) -> i32 {
    let mut exception = ptr::null_mut();
    let object =
        unsafe { (api.invoke)(method, ptr::null_mut(), params.as_mut_ptr(), &mut exception) };
    if let Some(expected) = expected {
        assert!(object.is_null() && !exception.is_null());
        let actual = unsafe { std::ffi::CStr::from_ptr((api.class_name)((api.class)(exception))) };
        assert_eq!(actual.to_str().unwrap(), expected);
        return 0;
    }
    assert!(
        !object.is_null() && exception.is_null(),
        "child domain invocation failed"
    );
    unsafe { (api.unbox)(object).cast::<i32>().read_unaligned() }
}
// Native Mono may reuse both domain IDs and addresses. Only a bridge-owned
// generation can identify a domain lease across unload/reload.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Token {
    generation: u64,
    native_id: i32,
}
#[derive(Default)]
struct Registry {
    generation: u64,
    live: Option<(Token, usize)>,
}
impl Registry {
    fn install(&mut self, native_id: i32, pointer: usize) -> Token {
        assert!(self.live.is_none() && pointer != 0);
        self.generation = self.generation.checked_add(1).unwrap();
        let token = Token {
            generation: self.generation,
            native_id,
        };
        self.live = Some((token, pointer));
        token
    }
    fn resolve(&self, token: Token) -> Option<usize> {
        self.live
            .filter(|(current, _)| *current == token)
            .map(|(_, pointer)| pointer)
    }
    fn retire(&mut self, token: Token) -> usize {
        let pointer = self.resolve(token).expect("domain token already retired");
        self.live = None;
        pointer
    }
}
pub(super) fn run(mono: &Mono, assembly: &str) {
    let create = symbol!(
        mono.library,
        "mono_domain_create_appdomain",
        unsafe extern "C" fn(*mut c_char, *mut c_char) -> Object
    );
    let unload = symbol!(
        mono.library,
        "mono_domain_try_unload",
        unsafe extern "C" fn(Object, *mut Object, Object)
    );
    let id = symbol!(
        mono.library,
        "mono_domain_get_id",
        unsafe extern "C" fn(Object) -> i32
    );
    let by_id = symbol!(
        mono.library,
        "mono_domain_get_by_id",
        unsafe extern "C" fn(i32) -> Object
    );
    let current_domain = symbol!(
        mono.library,
        "mono_domain_get",
        unsafe extern "C" fn() -> Object
    );
    let open = symbol!(
        mono.library,
        "mono_domain_assembly_open",
        unsafe extern "C" fn(Object, *const c_char) -> Object
    );
    let image = symbol!(
        mono.library,
        "mono_assembly_get_image",
        unsafe extern "C" fn(Object) -> Object
    );
    let class = symbol!(
        mono.library,
        "mono_class_from_name",
        unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object
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
    let array_addr = symbol!(
        mono.library,
        "mono_array_addr_with_size",
        unsafe extern "C" fn(Object, i32, usize) -> Object
    );
    let api = Api {
        invoke: mono.invoke,
        method: mono.method,
        unbox: symbol!(
            mono.library,
            "mono_object_unbox",
            unsafe extern "C" fn(Object) -> Object
        ),
        set: symbol!(
            mono.library,
            "mono_domain_set",
            unsafe extern "C" fn(Object, i32) -> i32
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
    let path = CString::new(assembly).unwrap();
    let root = mono.domain as usize;
    let new = mono.new_handle;
    let weak_new = mono.weak_handle;
    let target = mono.get_target;
    let free = mono.free_handle;
    let collect = mono.collect;
    let mut ids = std::collections::BTreeSet::new();
    let mut recycled_ids = 0;
    let mut registry = Registry::default();
    let mut retired = Vec::new();
    for generation in 1..=3 {
        let name = CString::new(format!("wire-native-domain-{generation}")).unwrap();
        let domain = unsafe { create(name.as_ptr() as *mut c_char, ptr::null_mut()) };
        assert!(!domain.is_null());
        let domain_id = unsafe { id(domain) };
        if !ids.insert(domain_id) {
            recycled_ids += 1;
        }
        let token = registry.install(domain_id, domain as usize);
        for &previous in &retired {
            assert!(registry.resolve(previous).is_none());
        }
        assert_eq!(registry.resolve(token), Some(domain as usize));
        assert_eq!(unsafe { by_id(domain_id) }, domain);
        let domain = domain as usize;
        let path = path.clone();
        let weak = std::thread::spawn(move || unsafe {
            let thread = attach(domain as Object);
            assert_eq!((api.set)(domain as Object, 0), 1);
            assert_eq!(current_domain(), domain as Object);
            let assembly = open(domain as Object, path.as_ptr());
            assert!(!assembly.is_null());
            let class = class(image(assembly), c"".as_ptr(), c"GuestBridge".as_ptr());
            assert!(!class.is_null());
            let counter = (api.method)(class, c"DomainCounter".as_ptr(), 1);
            let mut delta = 7i32;
            let mut args = [(&mut delta as *mut i32).cast()];
            assert_eq!(invoke(api, counter, &mut args, None), 7);
            assert_eq!(invoke(api, counter, &mut args, None), 14);
            let raise = (api.method)(class, c"DomainRaise".as_ptr(), 0);
            invoke(api, raise, &mut [], Some("ArgumentException"));
            let factory = (api.method)(class, c"NewArray".as_ptr(), 0);
            let mut exception = ptr::null_mut();
            let object = (api.invoke)(factory, ptr::null_mut(), ptr::null_mut(), &mut exception);
            assert!(!object.is_null() && exception.is_null());
            let handle = new(object, 1);
            let weak = weak_new(object, 0);
            assert!(!handle.is_null() && !weak.is_null());
            let owner = Arc::new(PinnedOwner {
                domain,
                handle: handle as usize,
                attach,
                detach,
                current_domain,
                free,
            });
            let data = NonNull::new(array_addr(object, 4, 0).cast::<u8>()).unwrap();
            let region = HostRegion::foreign(data, 16, owner.clone()).unwrap();
            let mut memory = Memory::default();
            const PAYLOAD: u64 = 0x57000003;
            memory
                .map_shared_data(PAYLOAD, region.clone(), READ | WRITE, READ | WRITE)
                .unwrap();
            memory.write_int(PAYLOAD + 4, 4, 703 + generation).unwrap();
            collect(0);
            assert_eq!(memory.read_int(PAYLOAD + 4, 4).unwrap(), 703 + generation);
            region.revoke().unwrap();
            assert!(memory.read_int(PAYLOAD, 4).is_err());
            assert!(region.with_native(|_| ()).is_err());
            drop(memory.unmap_shared_data(PAYLOAD).unwrap());
            drop((memory, region, owner));
            assert_eq!((api.set)(root as Object, 0), 1);
            detach(thread);
            weak as usize
        })
        .join()
        .unwrap();
        for _ in 0..8 {
            unsafe { collect(0) };
        }
        assert!(
            unsafe { target(weak as Handle) }.is_null(),
            "child-domain retired payload remained rooted"
        );
        unsafe { free(weak as Handle) };
        assert_eq!(unsafe { current_domain() } as usize, root);
        let mut exception = ptr::null_mut();
        let retired_pointer = registry.retire(token);
        assert!(registry.resolve(token).is_none());
        assert_eq!(retired_pointer, domain);
        unsafe { unload(retired_pointer as Object, &mut exception, ptr::null_mut()) };
        assert!(
            exception.is_null(),
            "native child domain unload raised an exception"
        );
        assert!(unsafe { by_id(domain_id) }.is_null());
        retired.push(token);
        // No pointer/MonoMethod from the retired domain is dereferenced here.
        assert_eq!(unsafe { current_domain() } as usize, root);
    }
    let mut delta = 2i32;
    let mut args = [(&mut delta as *mut i32).cast()];
    assert_eq!(
        invoke(api, mono.find("DomainCounter", 1), &mut args, None),
        2
    );
    println!("PASS native Mono child-domain lifecycle: 3 load/reset/exception/GC/revoke/unmap/reclaim/unload generations, isolated static counters, retired generation tokens rejected, root domain still usable; recycled_native_ids={recycled_ids}, no original VM");
}
