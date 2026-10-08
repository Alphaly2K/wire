//! Isolated original PE -> ARM64 IL -> original x64 callback prototype.
//! Guest code is interpreted by the existing checked Cpu64. No CPU edits.
use std::{
    cell::Cell,
    ffi::{c_char, c_void, CString},
    ptr,
};
use wire::x64::{
    cpu::Cpu64,
    memory::{Memory, READ, WRITE},
    pe::Pe64,
};

const BASE: u64 = 0x140000000;
const STACK: u64 = 0x7ff000000000;
const GATEWAY: u64 = 0x6fff00000000;
const RETURN: u64 = 0x6fff00000010;
const CALLBACK: u64 = BASE + 0x1100;
const ROOT_SLOT: u64 = STACK + 0x800;
type Object = *mut c_void;
type Handle = *mut c_void;
type Invoke = unsafe extern "C" fn(Object, Object, *mut Object, *mut Object) -> Object;
type Method = unsafe extern "C" fn(Object, *const c_char, i32) -> Object;

struct Mono {
    library: Object,
    invoke: Invoke,
    method: Method,
    domain: Object,
    class: Object,
    get_target: unsafe extern "C" fn(Handle) -> Object,
    new_handle: unsafe extern "C" fn(Object, i32) -> Handle,
    weak_handle: unsafe extern "C" fn(Object, i32) -> Handle,
    free_handle: unsafe extern "C" fn(Handle),
    collect: unsafe extern "C" fn(i32),
    add_roots: unsafe extern "C" fn(Object, Object),
    remove_roots: unsafe extern "C" fn(Object, Object),
    cleanup: unsafe extern "C" fn(Object),
}
macro_rules! symbol {
    ($library:expr, $name:literal, $ty:ty) => {{
        let name = concat!($name, "\0");
        let raw = unsafe { libc::dlsym($library, name.as_ptr().cast()) };
        assert!(!raw.is_null(), "missing Mono symbol {}", $name);
        unsafe { std::mem::transmute::<Object, $ty>(raw) }
    }};
}
impl Mono {
    fn load(library: &str, classlib: &str, assembly: &str) -> Self {
        let path = CString::new(library).unwrap();
        let library = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        assert!(!library.is_null(), "could not load candidate Mono");
        let set_dirs = symbol!(
            library,
            "mono_set_dirs",
            unsafe extern "C" fn(*const c_char, *const c_char)
        );
        let set_mode = symbol!(library, "mono_jit_set_aot_mode", unsafe extern "C" fn(i32));
        let init = symbol!(
            library,
            "mono_jit_init_version",
            unsafe extern "C" fn(*const c_char, *const c_char) -> Object
        );
        let open = symbol!(
            library,
            "mono_domain_assembly_open",
            unsafe extern "C" fn(Object, *const c_char) -> Object
        );
        let image = symbol!(
            library,
            "mono_assembly_get_image",
            unsafe extern "C" fn(Object) -> Object
        );
        let class = symbol!(
            library,
            "mono_class_from_name",
            unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object
        );
        let add_icall = symbol!(
            library,
            "mono_add_internal_call",
            unsafe extern "C" fn(*const c_char, Object)
        );
        let dirs = CString::new(classlib).unwrap();
        let filename = CString::new(assembly).unwrap();
        let domain = unsafe {
            set_dirs(dirs.as_ptr(), dirs.as_ptr());
            // MONO_AOT_MODE_INTERP, public enum value verified against locked jit.h.
            set_mode(5);
            init(c"wire-guest-mono-probe".as_ptr(), c"v4.0.30319".as_ptr())
        };
        assert!(!domain.is_null());
        let class = unsafe {
            add_icall(
                c"GuestBridge::GuestCallback".as_ptr(),
                guest_callback as *const () as Object,
            );
            let assembly = open(domain, filename.as_ptr());
            assert!(!assembly.is_null());
            class(image(assembly), c"".as_ptr(), c"GuestBridge".as_ptr())
        };
        assert!(!class.is_null());
        Self {
            library,
            domain,
            class,
            invoke: symbol!(library, "mono_runtime_invoke", Invoke),
            method: symbol!(library, "mono_class_get_method_from_name", Method),
            get_target: symbol!(
                library,
                "mono_gchandle_get_target_v2",
                unsafe extern "C" fn(Handle) -> Object
            ),
            new_handle: symbol!(
                library,
                "mono_gchandle_new_v2",
                unsafe extern "C" fn(Object, i32) -> Handle
            ),
            weak_handle: symbol!(
                library,
                "mono_gchandle_new_weakref_v2",
                unsafe extern "C" fn(Object, i32) -> Handle
            ),
            free_handle: symbol!(
                library,
                "mono_gchandle_free_v2",
                unsafe extern "C" fn(Handle)
            ),
            collect: symbol!(library, "mono_gc_collect", unsafe extern "C" fn(i32)),
            add_roots: symbol!(
                library,
                "GC_add_roots",
                unsafe extern "C" fn(Object, Object)
            ),
            remove_roots: symbol!(
                library,
                "GC_remove_roots",
                unsafe extern "C" fn(Object, Object)
            ),
            cleanup: symbol!(library, "mono_jit_cleanup", unsafe extern "C" fn(Object)),
        }
    }
    fn find(&self, name: &str, count: i32) -> Object {
        let name = CString::new(name).unwrap();
        let method = unsafe { (self.method)(self.class, name.as_ptr(), count) };
        assert!(!method.is_null());
        method
    }
    fn invoke(&self, method: Object, args: *mut Object) -> Object {
        let mut exception = ptr::null_mut();
        let result = unsafe { (self.invoke)(method, ptr::null_mut(), args, &mut exception) };
        assert!(exception.is_null(), "managed exception crossed as result");
        assert!(!result.is_null());
        result
    }
}

struct Guest {
    memory: Memory,
    callbacks: u64,
    paused_regs: [u64; 16],
}
thread_local! { static OWNER: Cell<*mut Guest> = const { Cell::new(ptr::null_mut()) }; }
unsafe extern "C" fn guest_callback(value: i32) -> i32 {
    // Catch Rust failures here: never unwind through Mono or the C ABI.
    let result = std::panic::catch_unwind(|| {
        OWNER.with(|owner| {
            let state = owner.get();
            assert!(!state.is_null(), "callback without owner-thread token");
            let guest = unsafe { &mut *state };
            let rsp = STACK + 0x1ff8;
            guest.memory.write_int(rsp, 8, RETURN).unwrap();
            let mut cpu = Cpu64::new(CALLBACK, rsp);
            cpu.regs[1] = value as u32 as u64;
            for _ in 0..32 {
                if cpu.rip == RETURN {
                    guest.callbacks += 1;
                    return cpu.regs[0] as i32;
                }
                cpu.step(&mut guest.memory).unwrap();
            }
            panic!("guest callback did not return");
        })
    });
    match result {
        Ok(value) => value,
        Err(_) => {
            eprintln!("guest callback rejected");
            std::process::abort();
        }
    }
}

fn pe_fixture() -> Vec<u8> {
    let mut bytes = vec![0; 0x400];
    fn u16at(bytes: &mut [u8], at: usize, value: u16) {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn u32at(bytes: &mut [u8], at: usize, value: u32) {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn u64at(bytes: &mut [u8], at: usize, value: u64) {
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes[..2].copy_from_slice(b"MZ");
    u32at(&mut bytes, 0x3c, 0x80);
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    u16at(&mut bytes, 0x84, 0x8664);
    u16at(&mut bytes, 0x86, 1);
    u16at(&mut bytes, 0x94, 0xf0);
    u16at(&mut bytes, 0x96, 0x22);
    let opt = 0x98;
    u16at(&mut bytes, opt, 0x20b);
    u32at(&mut bytes, opt + 16, 0x1000);
    u64at(&mut bytes, opt + 24, BASE);
    u32at(&mut bytes, opt + 32, 4096);
    u32at(&mut bytes, opt + 36, 512);
    u32at(&mut bytes, opt + 56, 8192);
    u32at(&mut bytes, opt + 60, 512);
    u16at(&mut bytes, opt + 68, 3);
    u64at(&mut bytes, opt + 72, 0x10000);
    u64at(&mut bytes, opt + 80, 0x1000);
    u32at(&mut bytes, opt + 108, 16);
    let section = opt + 0xf0;
    bytes[section..section + 5].copy_from_slice(b".text");
    u32at(&mut bytes, section + 8, 512);
    u32at(&mut bytes, section + 12, 4096);
    u32at(&mut bytes, section + 16, 512);
    u32at(&mut bytes, section + 20, 512);
    u32at(&mut bytes, section + 36, 0x60000020);
    // Win64 shadow space, typed Int32 argument, data gateway, return.
    let mut entry = vec![0x48, 0x83, 0xec, 0x28, 0xb9, 5, 0, 0, 0, 0x48, 0xb8];
    entry.extend_from_slice(&GATEWAY.to_le_bytes());
    entry.extend_from_slice(&[0xff, 0xd0, 0x48, 0x83, 0xc4, 0x28, 0xc3]);
    bytes[512..512 + entry.len()].copy_from_slice(&entry);
    // Original x64 callback: mov eax,ecx; add eax,11; ret.
    bytes[0x300..0x306].copy_from_slice(&[0x89, 0xc8, 0x83, 0xc0, 11, 0xc3]);
    bytes
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(
        args.len(),
        5,
        "usage: probe runtime.dylib classlib-root GuestBridge.exe fixture.exe"
    );
    let mono = Mono::load(&args[1], &args[2], &args[3]);
    let bytes = pe_fixture();
    std::fs::write(&args[4], &bytes).unwrap();
    let pe = Pe64::parse(&bytes).unwrap();
    let memory = pe
        .load(&bytes, BASE, |_, _| Err("unexpected fixture import".into()))
        .unwrap();
    let mut guest = Box::new(Guest {
        memory,
        callbacks: 0,
        paused_regs: [0; 16],
    });
    guest.memory.map(STACK, 0x3000, READ | WRITE).unwrap();
    let rsp = STACK + 0xff8;
    guest.memory.write_int(rsp, 8, RETURN).unwrap();
    let mut cpu = Cpu64::new(BASE + pe.entry_rva as u64, rsp);
    let roundtrip = mono.find("RoundTrip", 1);
    let unbox = symbol!(
        mono.library,
        "mono_object_unbox",
        unsafe extern "C" fn(Object) -> Object
    );
    for _ in 0..100 {
        if cpu.rip == RETURN {
            break;
        }
        if cpu.rip == GATEWAY {
            let mut value = cpu.regs[1] as i32;
            let mut parameters = [(&mut value as *mut i32).cast::<c_void>()];
            OWNER.with(|owner| {
                assert!(owner.get().is_null());
                owner.set(&mut *guest);
            });
            let boxed = mono.invoke(roundtrip, parameters.as_mut_ptr());
            OWNER.with(|owner| owner.set(ptr::null_mut()));
            cpu.regs[0] = unsafe { *(unbox(boxed) as *const i32) } as u32 as u64;
            cpu.rip = guest.memory.read_int(cpu.regs[4], 8).unwrap();
            cpu.regs[4] += 8;
        } else {
            cpu.step(&mut guest.memory).unwrap();
        }
    }
    assert_eq!(cpu.rip, RETURN);
    assert_eq!(cpu.regs[0], 19);
    assert_eq!(guest.callbacks, 1);
    println!("PASS original PE -> native IL -> interpreted x64 callback: checksum=19");

    // Root snapshots are scoped data regions, never a root for the whole heap.
    // This verifies guest register/stack reference ownership before shared backing.
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
    let domain = mono.domain as usize;
    let factory = mono.find("NewArray", 0) as usize;
    let invoke = mono.invoke;
    let new_handle = mono.new_handle;
    let weak_handle = mono.weak_handle;
    let free_handle = mono.free_handle;
    let add_roots = mono.add_roots;
    let remove_roots = mono.remove_roots;
    let collect = mono.collect;
    let get_target = mono.get_target;
    guest.paused_regs = cpu.regs;
    // Never bring the managed object address onto the main host stack. The
    // creator and validation threads exit between each collection, so stale
    // conservative stack/register words cannot manufacture a successful root.
    let (guest, roots, weak) = std::thread::spawn(move || unsafe {
        let thread = attach(domain as Object);
        let mut exception = ptr::null_mut();
        let array = invoke(
            factory as Object,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut exception,
        );
        assert!(!array.is_null() && exception.is_null());
        let strong = new_handle(array, 1);
        let weak = weak_handle(array, 0);
        guest.paused_regs[3] = array as u64;
        guest.memory.write_int(ROOT_SLOT, 8, array as u64).unwrap();
        let mut roots = Box::new([0u64; 17]);
        roots[..16].copy_from_slice(&guest.paused_regs);
        roots[16] = guest.memory.read_int(ROOT_SLOT, 8).unwrap();
        add_roots(roots.as_mut_ptr().cast(), roots.as_mut_ptr().add(17).cast());
        free_handle(strong);
        detach(thread);
        (guest, roots, weak as usize)
    })
    .join()
    .unwrap();
    let preserved = std::thread::spawn(move || unsafe {
        let thread = attach(domain as Object);
        collect(0);
        let preserved = !get_target(weak as Handle).is_null();
        detach(thread);
        preserved
    })
    .join()
    .unwrap();
    assert!(
        preserved,
        "guest snapshots preserve object without strong handle"
    );
    let (guest, roots) = std::thread::spawn(move || unsafe {
        let thread = attach(domain as Object);
        let mut guest = guest;
        let mut roots = roots;
        guest.paused_regs.fill(0);
        guest.memory.write_int(ROOT_SLOT, 8, 0).unwrap();
        roots.fill(0);
        remove_roots(roots.as_mut_ptr().cast(), roots.as_mut_ptr().add(17).cast());
        detach(thread);
        (guest, roots)
    })
    .join()
    .unwrap();
    let reclaimed = std::thread::spawn(move || unsafe {
        let thread = attach(domain as Object);
        for _ in 0..8 {
            collect(0);
        }
        let reclaimed = get_target(weak as Handle).is_null();
        free_handle(weak as Handle);
        detach(thread);
        reclaimed
    })
    .join()
    .unwrap();
    assert!(
        reclaimed,
        "removed snapshots do not permanently preserve object"
    );
    drop((guest, roots));
    unsafe {
        (mono.cleanup)(mono.domain);
    }
    println!("PASS scoped guest register/stack roots: preserved then reclaimed");
    // Do not dlclose: process-lifetime Mono/BDWGC TLS teardown owns the library.
}
