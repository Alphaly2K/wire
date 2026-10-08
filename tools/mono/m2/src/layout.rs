//! Original Unity x64 leaf ABI checks against real native Mono allocations.
use super::*;

pub(super) fn run(mono: &Mono, sample: &str) {
    let bytes = std::fs::read(sample).unwrap();
    let pe = Pe64::parse(&bytes).unwrap();
    let exports = pe.exports(&bytes, pe.image_base).unwrap();
    // No DllMain, original VM/GC, TLS, or imported function is initialized or
    // invoked. Any such dependency makes this leaf probe fail immediately.
    const IMPORT_TRAP: u64 = GATEWAY + 0x1000;
    let mut memory = pe
        .load_with_runtime(&bytes, pe.image_base, |_, _| Ok(IMPORT_TRAP))
        .unwrap();
    memory.map(STACK, 0x3000, READ | WRITE).unwrap();
    fn run(memory: &mut Memory, pc: u64, args: &[u64]) -> Result<u64, String> {
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
                return Err("leaf probe unexpectedly required an import".into());
            }
            cpu.step(memory)?;
        }
        Err("leaf probe did not return".into())
    }
    for (name, expected) in [
        ("mono_unity_object_header_size", 16),
        ("mono_unity_array_object_header_size", 32),
        (
            "mono_unity_offset_of_array_length_in_array_object_header",
            24,
        ),
        (
            "mono_unity_offset_of_array_bounds_in_array_object_header",
            16,
        ),
        ("mono_unity_allocation_granularity", 16),
    ] {
        let pc = exports.names[name];
        assert_eq!(run(&mut memory, pc, &[]).unwrap(), expected);
        let native = CString::new(name).unwrap();
        let address = unsafe { libc::dlsym(mono.library, native.as_ptr()) };
        assert!(!address.is_null());
        let call: unsafe extern "C" fn() -> i32 = unsafe { std::mem::transmute(address) };
        assert_eq!(unsafe { call() } as u64, expected);
    }
    let array = mono.invoke(mono.find("NewArray", 0), ptr::null_mut());
    let array_addr = symbol!(
        mono.library,
        "mono_array_addr_with_size",
        unsafe extern "C" fn(Object, i32, usize) -> Object
    );
    let array_length = symbol!(
        mono.library,
        "mono_array_length",
        unsafe extern "C" fn(Object) -> usize
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
    let object_class = symbol!(
        mono.library,
        "mono_object_get_class",
        unsafe extern "C" fn(Object) -> Object
    );
    let unbox = symbol!(
        mono.library,
        "mono_object_unbox",
        unsafe extern "C" fn(Object) -> Object
    );
    let mut regions = Vec::new();
    let pin = |object| {
        Arc::new(PinnedOwner {
            domain: mono.domain as usize,
            handle: unsafe { (mono.new_handle)(object, 1) } as usize,
            attach,
            detach,
            current_domain: symbol!(
                mono.library,
                "mono_domain_get",
                unsafe extern "C" fn() -> Object
            ),
            free: mono.free_handle,
        })
    };
    let array_pin = pin(array);
    let length_at = array as u64 + 24;
    // Register only the immutable array length field and scalar element payload.
    let length = unsafe {
        HostRegion::foreign(
            NonNull::new(length_at as *mut u8).unwrap(),
            8,
            array_pin.clone(),
        )
    }
    .unwrap();
    memory
        .map_shared_data(length_at, length.clone(), READ, READ)
        .unwrap();
    regions.push((length_at, length));
    let data = unsafe { array_addr(array, 4, 0) };
    let region = unsafe {
        HostRegion::foreign(
            NonNull::new(data.cast::<u8>()).unwrap(),
            16,
            array_pin.clone(),
        )
    }
    .unwrap();
    memory
        .map_shared_data(data as u64, region.clone(), READ, READ)
        .unwrap();
    regions.push((data as u64, region));
    assert_eq!(
        run(
            &mut memory,
            exports.names["mono_array_length"],
            &[array as u64]
        )
        .unwrap(),
        unsafe { array_length(array) } as u64
    );
    for index in 0..4 {
        let expected = unsafe { array_addr(array, 4, index) } as u64;
        let actual = run(
            &mut memory,
            exports.names["mono_array_addr_with_size"],
            &[array as u64, 4, index as u64],
        )
        .unwrap();
        assert_eq!(actual, expected);
        assert_eq!(memory.read_int(actual, 4).unwrap(), [17, 29, 43, 71][index]);
    }
    let object = mono.invoke(mono.find("BoxInt", 0), ptr::null_mut());
    let object_pin = pin(object);
    let class = unsafe { object_class(object) };
    let vtable = unsafe { object.cast::<u64>().read_unaligned() };
    assert_eq!(
        unsafe { (vtable as *const u64).read_unaligned() },
        class as u64
    );
    // These are exact immutable metadata fields. A real profile must validate
    // their complete layouts/lifetimes, rather than mapping host metadata pages.
    for (at, len) in [
        (object as u64, 8),
        (vtable, 8),
        (class as u64 + 32, 1),
        (object as u64 + 16, 4),
    ] {
        let region = unsafe {
            HostRegion::foreign(
                NonNull::new(at as *mut u8).unwrap(),
                len,
                object_pin.clone(),
            )
        }
        .unwrap();
        memory
            .map_shared_data(at, region.clone(), READ, READ)
            .unwrap();
        regions.push((at, region));
    }
    let profile_field = symbol!(
        mono.library,
        "wire_mono_profile_class_field",
        unsafe extern "C" fn(u32, Object, u32, *mut u32) -> Object
    );
    for field in 1..=6 {
        let mut size = 0;
        let pointer = unsafe { profile_field(1, class, field, &mut size) };
        assert!(!pointer.is_null() && size > 0);
        let region = unsafe {
            HostRegion::foreign(
                NonNull::new(pointer.cast()).unwrap(),
                size as usize,
                object_pin.clone(),
            )
        }
        .unwrap();
        memory
            .map_shared_data(pointer as u64, region.clone(), READ, READ)
            .unwrap();
        regions.push((pointer as u64, region));
        println!(
            "NATIVE_CLASS_PROFILE field={field} offset={:#x} size={size}",
            pointer as u64 - class as u64
        );
    }
    let mut compatible = true;
    for name in [
        "mono_class_get_name",
        "mono_class_get_namespace",
        "mono_class_get_image",
        "mono_class_get_parent",
        "mono_class_get_type",
    ] {
        let symbol_name = CString::new(name).unwrap();
        let pointer = unsafe { libc::dlsym(mono.library, symbol_name.as_ptr()) };
        assert!(!pointer.is_null());
        let native: unsafe extern "C" fn(Object) -> Object =
            unsafe { std::mem::transmute(pointer) };
        let expected = unsafe { native(class) } as u64;
        match run(&mut memory, exports.names[name], &[class as u64]) {
            Ok(actual) if actual == expected => {
                println!("PASS original class metadata leaf {name}")
            }
            result => {
                compatible = false;
                println!("CLASS_ABI_INCOMPATIBLE export={name} native={expected:#x} original={result:?}; profile rejected, no mapping widened");
            }
        }
    }
    let class_type = symbol!(
        mono.library,
        "mono_class_get_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let type_kind = symbol!(
        mono.library,
        "mono_type_get_type",
        unsafe extern "C" fn(Object) -> i32
    );
    let ty = unsafe { class_type(class) };
    assert_eq!(
        run(
            &mut memory,
            exports.names["mono_type_get_type"],
            &[ty as u64]
        )
        .unwrap(),
        unsafe { type_kind(ty) } as u64
    );
    println!("PASS original type-kind metadata leaf mono_type_get_type");
    println!(
        "CLASS_ABI_PROFILE compatible={} native_game_admission=0",
        u8::from(compatible)
    );
    let mut size = 99;
    assert!(unsafe { profile_field(2, class, 1, &mut size) }.is_null());
    assert_eq!(size, 0);
    size = 99;
    assert!(unsafe { profile_field(1, class, 99, &mut size) }.is_null());
    assert_eq!(size, 0);
    let expected = unsafe { unbox(object) } as u64;
    let result = run(
        &mut memory,
        exports.names["mono_object_unbox"],
        &[object as u64],
    );
    assert_eq!(
        result.unwrap_or_else(|error| panic!(
            "original unbox metadata ABI mismatch: {error}; native class flag[0x20]={:#x}",
            unsafe { *((class as *const u8).add(32)) }
        )),
        expected
    );
    assert_eq!(memory.read_int(expected, 4).unwrap(), 73);
    for (at, region) in regions {
        region.revoke().unwrap();
        drop(memory.unmap_shared_data(at).unwrap());
    }
    drop((array_pin, object_pin));
    println!("PASS original Unity Mono leaf exports on real native objects: object/array/bounds headers, array length/data, unbox metadata and value; original VM never initialized");
}
