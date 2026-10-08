//! Original DLL leaf access to resolved native method/signature/type metadata.
//! Views are exact and read-only; domain remains live for this entire probe.
use super::*;
use std::collections::BTreeMap;
const IMPORT_TRAP: u64 = GATEWAY + 0x1000;
fn leaf(memory: &mut Memory, pc: u64, args: &[u64]) -> Result<u64, String> {
    let rsp = STACK + 0x2ff8;
    memory.write_int(rsp, 8, RETURN)?;
    let mut cpu = Cpu64::new(pc, rsp);
    for (&value, reg) in args.iter().zip([1, 2, 8, 9]) {
        cpu.regs[reg] = value;
    }
    for _ in 0..2000 {
        if cpu.rip == RETURN {
            return Ok(cpu.regs[0]);
        }
        if cpu.rip == IMPORT_TRAP {
            return Err("metadata leaf required original runtime/import".into());
        }
        cpu.step(memory)?;
    }
    Err("metadata leaf budget exhausted".into())
}
fn view(
    memory: &mut Memory,
    regions: &mut BTreeMap<u64, (usize, HostRegion)>,
    owner: &Arc<PinnedOwner>,
    pointer: Object,
    size: u32,
) {
    assert!(!pointer.is_null() && size > 0);
    if let Some((registered, _)) = regions.get(&(pointer as u64)) {
        assert_eq!(
            *registered, size as usize,
            "same pointer changed diagnostic extent"
        );
        return;
    }
    let region = unsafe {
        HostRegion::foreign(
            NonNull::new(pointer.cast::<u8>()).unwrap(),
            size as usize,
            owner.clone(),
        )
    }
    .unwrap();
    memory
        .map_shared_data(pointer as u64, region.clone(), READ, READ)
        .unwrap();
    regions.insert(pointer as u64, (size as usize, region));
}
pub(super) fn run(mono: &Mono, sample: &str) {
    let bytes = std::fs::read(sample).unwrap();
    let pe = Pe64::parse(&bytes).unwrap();
    let exports = pe.exports(&bytes, pe.image_base).unwrap();
    let mut memory = pe
        .load_with_runtime(&bytes, pe.image_base, |_, _| Ok(IMPORT_TRAP))
        .unwrap();
    memory.map(STACK, 0x3000, READ | WRITE).unwrap();
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
    let owner = Arc::new(PinnedOwner {
        domain: mono.domain as usize,
        handle: unsafe {
            (mono.new_handle)(mono.invoke(mono.find("BoxInt", 0), ptr::null_mut()), 1)
        } as usize,
        attach,
        detach,
        current_domain: symbol!(
            mono.library,
            "mono_domain_get",
            unsafe extern "C" fn() -> Object
        ),
        free: mono.free_handle,
    });
    let method_view = symbol!(
        mono.library,
        "wire_mono_profile_method_field",
        unsafe extern "C" fn(u32, Object, u32, *mut u32) -> Object
    );
    let signature_view = symbol!(
        mono.library,
        "wire_mono_profile_signature_region",
        unsafe extern "C" fn(u32, Object, u32, *mut u32) -> Object
    );
    let type_view = symbol!(
        mono.library,
        "wire_mono_profile_type_region",
        unsafe extern "C" fn(u32, Object, *mut u32) -> Object
    );
    let signature = symbol!(
        mono.library,
        "mono_method_signature",
        unsafe extern "C" fn(Object) -> Object
    );
    let flags = symbol!(
        mono.library,
        "mono_method_get_flags",
        unsafe extern "C" fn(Object, *mut u32) -> u32
    );
    let token = symbol!(
        mono.library,
        "mono_method_get_token",
        unsafe extern "C" fn(Object) -> u32
    );
    let param_count = symbol!(
        mono.library,
        "mono_signature_get_param_count",
        unsafe extern "C" fn(Object) -> u32
    );
    let params = symbol!(
        mono.library,
        "mono_signature_get_params",
        unsafe extern "C" fn(Object, *mut Object) -> Object
    );
    let ret = symbol!(
        mono.library,
        "mono_signature_get_return_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let instance = symbol!(
        mono.library,
        "mono_signature_is_instance",
        unsafe extern "C" fn(Object) -> i32
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
    let nested = symbol!(
        mono.library,
        "mono_class_get_nested_types",
        unsafe extern "C" fn(Object, *mut Object) -> Object
    );
    let name = symbol!(
        mono.library,
        "mono_class_get_name",
        unsafe extern "C" fn(Object) -> *const c_char
    );
    let mut iterator = ptr::null_mut();
    let nested_class = loop {
        let class = unsafe { nested(mono.class, &mut iterator) };
        assert!(!class.is_null(), "metadata instance fixture missing");
        if unsafe { std::ffi::CStr::from_ptr(name(class)) }.to_bytes() == b"MetadataInstance" {
            break class;
        }
    };
    let echo = unsafe { (mono.method)(nested_class, c"Echo".as_ptr(), 2) };
    assert!(!echo.is_null());
    let methods = [
        mono.find("FloatRoundTrip", 3),
        mono.find("GuestWideCallback", 4),
        mono.find("DomainCounter", 1),
        echo,
    ];
    let mut regions = BTreeMap::new();
    let mut checks = 0;
    let mut byref_seen = 0;
    let mut instance_seen = 0;
    for method in methods {
        let sig = unsafe { signature(method) };
        assert!(!sig.is_null());
        for field in 1..=6 {
            let mut size = 0;
            let pointer = unsafe { method_view(1, method, field, &mut size) };
            view(&mut memory, &mut regions, &owner, pointer, size);
            println!(
                "NATIVE_METHOD_PROFILE field={field} offset={:#x} size={size}",
                pointer as u64 - method as u64
            );
        }
        for field in 1..=2 {
            let mut size = 0;
            let pointer = unsafe { signature_view(1, sig, field, &mut size) };
            view(&mut memory, &mut regions, &owner, pointer, size);
        }
        for export in ["mono_method_get_name", "mono_method_get_class"] {
            let symbol = CString::new(export).unwrap();
            let raw = unsafe { libc::dlsym(mono.library, symbol.as_ptr()) };
            assert!(!raw.is_null());
            let native: unsafe extern "C" fn(Object) -> Object =
                unsafe { std::mem::transmute(raw) };
            assert_eq!(
                leaf(&mut memory, exports.names[export], &[method as u64]).unwrap(),
                unsafe { native(method) } as u64,
                "{export}"
            );
            checks += 1;
        }
        let mut iflags = 0;
        let expected = unsafe { flags(method, &mut iflags) };
        let output = STACK + 0x100;
        memory.write_int(output, 4, 0xffffffff).unwrap();
        assert_eq!(
            leaf(
                &mut memory,
                exports.names["mono_method_get_flags"],
                &[method as u64, output]
            )
            .unwrap(),
            expected as u64
        );
        assert_eq!(memory.read_int(output, 4).unwrap(), iflags as u64);
        assert_eq!(
            leaf(
                &mut memory,
                exports.names["mono_method_get_token"],
                &[method as u64]
            )
            .unwrap(),
            unsafe { token(method) } as u64
        );
        assert_eq!(
            leaf(
                &mut memory,
                exports.names["mono_method_signature"],
                &[method as u64]
            )
            .unwrap(),
            sig as u64
        );
        checks += 3;
        assert_eq!(
            leaf(
                &mut memory,
                exports.names["mono_signature_get_param_count"],
                &[sig as u64]
            )
            .unwrap(),
            unsafe { param_count(sig) } as u64
        );
        let has_this = unsafe { instance(sig) };
        instance_seen += usize::from(has_this != 0);
        assert_eq!(
            leaf(
                &mut memory,
                exports.names["mono_signature_is_instance"],
                &[sig as u64]
            )
            .unwrap(),
            has_this as u64
        );
        let return_type = unsafe { ret(sig) };
        assert_eq!(
            leaf(
                &mut memory,
                exports.names["mono_signature_get_return_type"],
                &[sig as u64]
            )
            .unwrap(),
            return_type as u64
        );
        checks += 3;
        let mut types = vec![return_type];
        let mut iter = ptr::null_mut();
        let iter_at = STACK + 0x108;
        memory.write_int(iter_at, 8, 0).unwrap();
        for _ in 0..unsafe { param_count(sig) } {
            let ty = unsafe { params(sig, &mut iter) };
            assert!(!ty.is_null());
            assert_eq!(
                leaf(
                    &mut memory,
                    exports.names["mono_signature_get_params"],
                    &[sig as u64, iter_at]
                )
                .unwrap(),
                ty as u64
            );
            assert_eq!(memory.read_int(iter_at, 8).unwrap(), iter as u64);
            types.push(ty);
            checks += 1;
        }
        assert!(unsafe { params(sig, &mut iter) }.is_null());
        assert_eq!(
            leaf(
                &mut memory,
                exports.names["mono_signature_get_params"],
                &[sig as u64, iter_at]
            )
            .unwrap(),
            0
        );
        checks += 1;
        for ty in types {
            let mut size = 0;
            let pointer = unsafe { type_view(1, ty, &mut size) };
            view(&mut memory, &mut regions, &owner, pointer, size);
            assert_eq!(
                leaf(
                    &mut memory,
                    exports.names["mono_type_get_type"],
                    &[ty as u64]
                )
                .unwrap(),
                unsafe { kind(ty) } as u64
            );
            let ref_kind = unsafe { byref(ty) };
            byref_seen += usize::from(ref_kind != 0);
            assert_eq!(
                leaf(
                    &mut memory,
                    exports.names["mono_type_is_byref"],
                    &[ty as u64]
                )
                .unwrap(),
                ref_kind as u64
            );
            checks += 2;
        }
    }
    assert_eq!(instance_seen, 1);
    assert!(byref_seen >= 3);
    let mut size = 99;
    assert!(unsafe { method_view(2, methods[0], 1, &mut size) }.is_null());
    assert_eq!(size, 0);
    size = 99;
    assert!(unsafe { signature_view(1, ptr::null_mut(), 1, &mut size) }.is_null());
    assert_eq!(size, 0);
    for (at, (_, region)) in regions {
        region.revoke().unwrap();
        drop(memory.unmap_shared_data(at).unwrap());
    }
    drop(owner);
    println!("PASS original Unity method/signature/type metadata leaves: {checks} comparisons, static/instance, mixed floats/int64/struct/ref/out, param iterator/return/flags/token; exact readonly views, original VM never initialized; native_game_admission=0");
}
