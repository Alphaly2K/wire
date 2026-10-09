//! Closed primitive unsafe-IL scope; every guest access remains checked.
use super::*;
use std::ffi::CStr;

type MemoryAccess = unsafe extern "C" fn(i32, u64, u64, Object, usize, Object) -> i32;
type InstallMemory = unsafe extern "C" fn(u32, Object, Option<MemoryAccess>, Object) -> i32;
pub(super) unsafe extern "C" fn checked_access(
    op: i32,
    address: u64,
    second: u64,
    buffer: Object,
    len: usize,
    user: Object,
) -> i32 {
    let result = std::panic::catch_unwind(|| {
        let guest = unsafe { &mut *user.cast::<Guest>() };
        if !(1..=4).contains(&op) || len > 256 * 1024 * 1024 {
            return Err("invalid memory operation/size".to_string());
        }
        guest.checked_calls[(op - 1) as usize] += 1;
        let result = match op {
            1 => guest.memory.read(address, unsafe {
                std::slice::from_raw_parts_mut(buffer.cast::<u8>(), len)
            }),
            2 => guest.memory.write(address, unsafe {
                std::slice::from_raw_parts(buffer.cast::<u8>(), len)
            }),
            3 | 4 => (|| {
                // Preflight every byte before allocation/copy: no partial write
                // even when a later page/interval has a protection or guard fault.
                guest.memory.check(address, len, WRITE)?;
                if op == 3 {
                    guest.memory.check(second, len, READ)?;
                }
                let mut bytes = vec![second as u8; len];
                if op == 3 {
                    guest.memory.read(second, &mut bytes)?;
                }
                guest.memory.write(address, &bytes)
            })(),
            _ => unreachable!(),
        };
        if let Err(ref message) = result {
            eprintln!("checked fault op={op} address={address:#x} len={len}: {message}");
            guest.memory_faults.push(message.clone());
        }
        result
    });
    match result {
        Ok(Ok(())) => 0,
        _ => -1,
    }
}

pub(super) fn invoke_i32(
    mono: &Mono,
    method: Object,
    parameters: &mut [Object],
    exception_name: Option<&str>,
) -> i32 {
    let mut exception = ptr::null_mut();
    let boxed = unsafe {
        (mono.invoke)(
            method,
            ptr::null_mut(),
            parameters.as_mut_ptr(),
            &mut exception,
        )
    };
    if let Some(expected) = exception_name {
        assert!(
            !exception.is_null(),
            "checked memory fault did not return as managed exception"
        );
        let class = symbol!(
            mono.library,
            "mono_object_get_class",
            unsafe extern "C" fn(Object) -> Object
        );
        let name = symbol!(
            mono.library,
            "mono_class_get_name",
            unsafe extern "C" fn(Object) -> *const c_char
        );
        let actual = unsafe { CStr::from_ptr(name(class(exception))) }
            .to_str()
            .unwrap();
        assert_eq!(actual, expected);
        return 0;
    }
    if !exception.is_null() {
        let method_name = symbol!(
            mono.library,
            "mono_method_get_name",
            unsafe extern "C" fn(Object) -> *const c_char
        );
        panic!(
            "unexpected checked IL exception in {}",
            unsafe { CStr::from_ptr(method_name(method)) }
                .to_str()
                .unwrap()
        );
    }
    assert!(!boxed.is_null());
    let unbox = symbol!(
        mono.library,
        "mono_object_unbox",
        unsafe extern "C" fn(Object) -> Object
    );
    unsafe { *(unbox(boxed) as *const i32) }
}

pub(super) fn run(mono: &Mono, state: *mut Guest, blocks: &str) {
    const DATA: u64 = 0x50000000;
    const AT: u64 = DATA + 3; // Deliberately unaligned and not a host pointer.
    let install = symbol!(mono.library, "wire_mono_set_memory_adapter", InstallMemory);
    unsafe { &mut (*state).memory }
        .map(DATA, 8192, READ | WRITE)
        .unwrap();
    // Compile once outside the checked scope. Later activation must reject
    // this cached transform rather than trust an optimized instruction stream.
    let mut cached_arg = 31i32;
    let mut cached_args = [(&mut cached_arg as *mut i32).cast()];
    let cached = mono.find("CachedPrimitive", 1);
    assert_eq!(invoke_i32(mono, cached, &mut cached_args, None), 32);
    let cached_region = HostRegion::owned(8).unwrap();
    let mut cached_address = cached_region.with_native(|p| p.as_ptr() as u64).unwrap();
    let mut cached_value = 71i32;
    let mut cached_write_args = [
        (&mut cached_address as *mut u64).cast(),
        (&mut cached_value as *mut i32).cast(),
    ];
    let cached_write = mono.find("CachedWrite", 2);
    assert_eq!(
        invoke_i32(mono, cached_write, &mut cached_write_args, None),
        71
    );
    unsafe { &mut (*state).memory }
        .map_shared_data(cached_address, cached_region.clone(), READ, READ)
        .unwrap();

    assert_eq!(
        unsafe { install(99, mono.image, Some(checked_access), state.cast(),) },
        -1
    );
    assert_eq!(
        unsafe { install(1, mono.image, Some(checked_access), state.cast(),) },
        0
    );
    invoke_i32(
        mono,
        cached,
        &mut cached_args,
        Some("NotSupportedException"),
    );
    cached_value = 99;
    std::hint::black_box(cached_value);
    cached_region
        .with_native(|_| {
            invoke_i32(
                mono,
                cached_write,
                &mut cached_write_args,
                Some("NotSupportedException"),
            );
        })
        .unwrap();
    assert_eq!(
        unsafe { &(*state).memory }
            .read_int(cached_address, 4)
            .unwrap(),
        71
    );
    cached_region.revoke().unwrap();
    drop(
        unsafe { &mut (*state).memory }
            .unmap_shared_data(cached_address)
            .unwrap(),
    );
    println!("PASS checked scope rejects preexisting unchecked method transform");
    let mut seed = 3i32;
    let mut nested_args = [(&mut seed as *mut i32).cast()];
    assert_eq!(
        invoke_i32(
            mono,
            mono.find("CheckedNestedLocal", 1),
            &mut nested_args,
            None
        ),
        18
    );
    invoke_i32(
        mono,
        mono.find("CheckedNestedCached", 1),
        &mut nested_args,
        Some("NotSupportedException"),
    );
    let mut at = AT;
    let mut value = 37i32;
    let mut parameters = [
        (&mut at as *mut u64).cast(),
        (&mut value as *mut i32).cast(),
    ];
    assert_eq!(
        invoke_i32(mono, mono.find("WriteI32", 2), &mut parameters, None),
        37
    );
    assert_eq!(unsafe { &mut (*state).memory }.read_int(AT, 4).unwrap(), 37);
    assert_eq!(
        invoke_i32(mono, mono.find("ReadI32", 1), &mut parameters[..1], None),
        37
    );
    for (store, read_signed, read_unsigned, n, size) in [
        ("WriteI8", "ReadI8", "ReadU8", -3, 1),
        ("WriteI16", "ReadI16", "ReadU16", -3, 2),
    ] {
        value = n;
        std::hint::black_box(value);
        invoke_i32(mono, mono.find(store, 2), &mut parameters, None);
        assert_eq!(
            invoke_i32(mono, mono.find(read_signed, 1), &mut parameters[..1], None),
            -3
        );
        assert_eq!(
            invoke_i32(
                mono,
                mono.find(read_unsigned, 1),
                &mut parameters[..1],
                None
            ),
            if size == 1 { 253 } else { 65533 }
        );
    }
    let mut wide = 0x1234567812345678i64;
    let mut float = 17.5f32;
    let mut double = 29.25f64;
    for (name, pointer, expected, bytes) in [
        (
            "WriteI64",
            (&mut wide as *mut i64).cast(),
            wide as i32,
            wide.to_le_bytes().to_vec(),
        ),
        (
            "WriteFloat",
            (&mut float as *mut f32).cast(),
            17,
            float.to_le_bytes().to_vec(),
        ),
        (
            "WriteDouble",
            (&mut double as *mut f64).cast(),
            29,
            double.to_le_bytes().to_vec(),
        ),
    ] {
        let mut args = [(&mut at as *mut u64).cast(), pointer];
        assert_eq!(
            invoke_i32(mono, mono.find(name, 2), &mut args, None),
            expected
        );
        let mut observed = vec![0; bytes.len()];
        unsafe { &mut (*state).memory }
            .read(AT, &mut observed)
            .unwrap();
        assert_eq!(observed, bytes);
    }
    value = 113;
    std::hint::black_box(value);
    assert_eq!(
        invoke_i32(
            mono,
            mono.find("CheckedNestedWrite", 2),
            &mut parameters,
            None
        ),
        113
    );
    assert_eq!(unsafe { &(*state).memory }.read_int(AT, 4).unwrap(), 113);
    unsafe { &mut (*state).memory }
        .protect(DATA, 4096, READ)
        .unwrap();
    invoke_i32(
        mono,
        mono.find("CheckedNestedWrite", 2),
        &mut parameters,
        Some("AccessViolationException"),
    );
    assert_eq!(unsafe { &(*state).memory }.read_int(AT, 4).unwrap(), 113);
    unsafe { &mut (*state).memory }
        .protect(DATA, 4096, READ | WRITE)
        .unwrap();
    println!("PASS checked static IL calls: same image, recursive parent byref, checked child access, cached target rejection");
    let mut left = 31i32;
    let mut right = 43i32;
    let mut pair_args = [
        (&mut at as *mut u64).cast(),
        (&mut left as *mut i32).cast(),
        (&mut right as *mut i32).cast(),
    ];
    assert_eq!(
        invoke_i32(mono, mono.find("StorePair", 3), &mut pair_args, None),
        74
    );
    assert_eq!(
        invoke_i32(mono, mono.find("LoadPair", 1), &mut pair_args[..1], None),
        74
    );
    assert_eq!(
        invoke_i32(mono, mono.find("UpdatePair", 1), &mut pair_args[..1], None),
        124
    );
    assert_eq!(unsafe { &mut (*state).memory }.read_int(AT, 4).unwrap(), 38);
    assert_eq!(
        unsafe { &mut (*state).memory }.read_int(AT + 4, 4).unwrap(),
        86
    );
    let read = mono.find("ReadI32", 1);
    let write = mono.find("WriteI32", 2);
    let before = unsafe { &mut (*state).memory }.read_int(AT, 4).unwrap();
    unsafe { &mut (*state).memory }
        .protect(DATA, 4096, READ)
        .unwrap();
    invoke_i32(
        mono,
        write,
        &mut parameters,
        Some("AccessViolationException"),
    );
    assert_eq!(
        unsafe { &mut (*state).memory }.read_int(AT, 4).unwrap(),
        before
    );
    unsafe { &mut (*state).memory }
        .protect(DATA, 4096, 0)
        .unwrap();
    invoke_i32(
        mono,
        read,
        &mut parameters[..1],
        Some("AccessViolationException"),
    );
    unsafe { &mut (*state).memory }
        .protect(DATA, 4096, READ | WRITE)
        .unwrap();
    for bad in [DATA - 1, DATA + 8190, u64::MAX - 1, 1 << 47] {
        at = bad;
        std::hint::black_box(at);
        invoke_i32(
            mono,
            read,
            &mut parameters[..1],
            Some("AccessViolationException"),
        );
    }
    at = AT;
    std::hint::black_box(at);
    // A shared registered byte interval has the same checks when reached by IL.
    const SHARED: u64 = DATA + 0x10003;
    let region = HostRegion::owned(8).unwrap();
    unsafe { &mut (*state).memory }
        .map_shared_data(SHARED, region.clone(), READ | WRITE, READ | WRITE)
        .unwrap();
    let native_address = region
        .with_native(|pointer| pointer.as_ptr() as u64)
        .unwrap();
    unsafe { &mut (*state).memory }
        .map_shared_data(native_address, region.clone(), READ, READ)
        .unwrap();
    at = SHARED;
    std::hint::black_box(at);
    value = 37;
    std::hint::black_box(value);
    invoke_i32(mono, write, &mut parameters, None);
    region
        .with_native(|p| assert_eq!(unsafe { p.as_ptr().cast::<i32>().read_unaligned() }, 37))
        .unwrap();
    unsafe { &mut (*state).memory }
        .protect_shared_data(SHARED, READ | WRITE, true)
        .unwrap();
    invoke_i32(
        mono,
        read,
        &mut parameters[..1],
        Some("AccessViolationException"),
    );
    assert_eq!(invoke_i32(mono, read, &mut parameters[..1], None), 37);
    at = SHARED + 6;
    std::hint::black_box(at);
    invoke_i32(
        mono,
        write,
        &mut parameters,
        Some("AccessViolationException"),
    );
    assert_eq!(
        unsafe { &mut (*state).memory }.read_int(SHARED, 8).unwrap(),
        37
    );
    unsafe { &mut (*state).memory }
        .protect_shared_data(SHARED, READ, false)
        .unwrap();
    at = SHARED;
    std::hint::black_box(at);
    invoke_i32(
        mono,
        write,
        &mut parameters,
        Some("AccessViolationException"),
    );
    // The same rejection must hold through an admitted same-image helper.
    at = native_address;
    std::hint::black_box(at);
    region
        .with_native(|_| {
            invoke_i32(
                mono,
                mono.find("CheckedNestedExternal", 2),
                &mut parameters,
                Some("NotSupportedException"),
            );
        })
        .unwrap();
    assert_eq!(
        unsafe { &(*state).memory }
            .read_int(native_address, 8)
            .unwrap(),
        37
    );
    // An unadapted Marshal helper must be rejected before it can write to the
    // RW host allocation behind the registered READ-only guest alias.
    at = native_address;
    std::hint::black_box(at);
    region
        .with_native(|_| {
            invoke_i32(
                mono,
                mono.find("MarshalHelperWrite", 2),
                &mut parameters,
                Some("NotSupportedException"),
            )
        })
        .unwrap();
    assert_eq!(
        unsafe { &mut (*state).memory }
            .read_int(native_address, 8)
            .unwrap(),
        37
    );
    region.revoke().unwrap();
    invoke_i32(
        mono,
        read,
        &mut parameters[..1],
        Some("AccessViolationException"),
    );
    drop(
        unsafe { &mut (*state).memory }
            .unmap_shared_data(SHARED)
            .unwrap(),
    );
    drop(
        unsafe { &mut (*state).memory }
            .unmap_shared_data(native_address)
            .unwrap(),
    );
    drop(region);

    // A scalar store crossing a read-only second page faults before any byte.
    unsafe { &mut (*state).memory }
        .write(DATA + 4092, &[0x55; 8])
        .unwrap();
    unsafe { &mut (*state).memory }
        .protect(DATA + 4096, 4096, READ)
        .unwrap();
    at = DATA + 4094;
    std::hint::black_box(at);
    invoke_i32(
        mono,
        write,
        &mut parameters,
        Some("AccessViolationException"),
    );
    assert_eq!(
        unsafe { &mut (*state).memory }
            .read_int(DATA + 4092, 8)
            .unwrap(),
        0x5555555555555555
    );
    unsafe { &mut (*state).memory }
        .protect(DATA + 4096, 4096, READ | WRITE)
        .unwrap();
    assert_eq!(
        unsafe { install(1, ptr::null_mut(), None, ptr::null_mut()) },
        0
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
    let path = CString::new(blocks).unwrap();
    let image = unsafe { image(open(mono.domain, path.as_ptr())) };
    assert!(!image.is_null());
    let class = unsafe { class(image, c"".as_ptr(), c"CheckedBlocks".as_ptr()) };
    let copy = unsafe { (mono.method)(class, c"Copy".as_ptr(), 3) };
    let fill = unsafe { (mono.method)(class, c"Fill".as_ptr(), 3) };
    assert!(!copy.is_null() && !fill.is_null());
    assert_eq!(
        unsafe { install(1, image, Some(checked_access), state.cast(),) },
        0
    );
    let mut dest = DATA + 0x100;
    let mut src = DATA + 0x200;
    let mut length = 16i32;
    let mut byte = 0xa5i32;
    unsafe { &mut (*state).memory }
        .write(src, &[0x37; 16])
        .unwrap();
    let mut args = [
        (&mut dest as *mut u64).cast(),
        (&mut src as *mut u64).cast(),
        (&mut length as *mut i32).cast(),
    ];
    invoke_i32(mono, copy, &mut args, None);
    assert_eq!(
        unsafe { &mut (*state).memory }.read_int(dest, 8).unwrap(),
        0x3737373737373737
    );
    args[1] = (&mut byte as *mut i32).cast();
    invoke_i32(mono, fill, &mut args, None);
    assert_eq!(
        unsafe { &mut (*state).memory }.read_int(dest, 8).unwrap(),
        0xa5a5a5a5a5a5a5a5
    );
    dest = DATA + 4092;
    std::hint::black_box(dest);
    unsafe { &mut (*state).memory }
        .write(dest, &[0x55; 16])
        .unwrap();
    unsafe { &mut (*state).memory }
        .protect(DATA + 4096, 4096, READ)
        .unwrap();
    invoke_i32(mono, fill, &mut args, Some("AccessViolationException"));
    args[1] = (&mut src as *mut u64).cast();
    invoke_i32(mono, copy, &mut args, Some("AccessViolationException"));
    assert_eq!(
        unsafe { &mut (*state).memory }
            .read_int(DATA + 4092, 8)
            .unwrap(),
        0x5555555555555555
    );
    unsafe { &mut (*state).memory }
        .protect(DATA + 4096, 4096, READ | WRITE)
        .unwrap();
    src = DATA - 16;
    std::hint::black_box(src);
    invoke_i32(mono, copy, &mut args, Some("AccessViolationException"));
    assert_eq!(
        unsafe { &mut (*state).memory }
            .read_int(DATA + 4092, 8)
            .unwrap(),
        0x5555555555555555
    );
    length = -1;
    std::hint::black_box(length);
    invoke_i32(mono, copy, &mut args, Some("AccessViolationException"));
    assert_eq!(
        unsafe { install(1, ptr::null_mut(), None, ptr::null_mut()) },
        0
    );
    assert_eq!(
        unsafe { install(1, image, Some(checked_access), state.cast(),) },
        0
    );
    let reference_store = unsafe { (mono.method)(class, c"ReferenceStore".as_ptr(), 2) };
    assert!(!reference_store.is_null());
    let object = mono.invoke(mono.find("NewArray", 0), ptr::null_mut());
    let mut ref_args = [(&mut dest as *mut u64).cast(), object];
    invoke_i32(
        mono,
        reference_store,
        &mut ref_args,
        Some("NotSupportedException"),
    );
    assert_eq!(
        unsafe { &mut (*state).memory }
            .read_int(DATA + 4092, 8)
            .unwrap(),
        0x5555555555555555
    );
    assert_eq!(
        unsafe { install(1, ptr::null_mut(), None, ptr::null_mut()) },
        0
    );
    assert!(unsafe { &(*state).checked_calls }.iter().all(|&n| n != 0));
    assert!(unsafe { &(*state).memory_faults }.len() >= 10);
    println!("PASS scoped checked IL: scalar widths/float/struct/cpblk/initblk, checked faults, no partial writes; calls={:?} faults={}", unsafe { &(*state).checked_calls }, unsafe { &(*state).memory_faults }.len());
}
