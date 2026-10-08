//! Fixed signatures, authored PE code and precompiled ARM64 callbacks. No
//! dynamic native closures, generic ABI guessing or production gateway tokens.
use super::*;
const FLOAT_CALLBACK: u64 = BASE + 0x1300;
const WIDE_CALLBACK: u64 = BASE + 0x1340;
const FLOAT_GATEWAY: u64 = GATEWAY + 0x40;
const WIDE_GATEWAY: u64 = GATEWAY + 0x50;
const DATA: u64 = STACK + 0x400;
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct Wide {
    value: f64,
    count: i64,
    tag: u32,
    padding: u32,
}
struct Owner;
impl Owner {
    fn install(state: *mut Guest) -> Self {
        OWNER.with(|owner| {
            assert!(owner.get().is_null());
            owner.set(state);
        });
        Self
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        OWNER.with(|owner| owner.set(ptr::null_mut()));
    }
}
fn execute(guest: &mut Guest, cpu: &mut Cpu64) {
    for _ in 0..100 {
        if cpu.rip == RETURN {
            guest.callbacks += 1;
            return;
        }
        cpu.step(&mut guest.memory).unwrap();
    }
    panic!("typed ABI PE did not return");
}
unsafe extern "C" fn float_callback(value: f64, scale: f32, bias: i64) -> f64 {
    let result = std::panic::catch_unwind(|| {
        OWNER.with(|owner| {
            let state = owner.get();
            assert!(!state.is_null());
            let guest = unsafe { &mut *state };
            let rsp = STACK + 0x2ff8;
            guest.memory.write_int(rsp, 8, RETURN).unwrap();
            let mut cpu = Cpu64::new(FLOAT_CALLBACK, rsp);
            cpu.xmm[0][..8].copy_from_slice(&value.to_le_bytes());
            cpu.xmm[1][..4].copy_from_slice(&scale.to_le_bytes());
            cpu.regs[8] = bias as u64;
            execute(guest, &mut cpu);
            f64::from_le_bytes(cpu.xmm[0][..8].try_into().unwrap())
        })
    });
    result.unwrap_or_else(|_| std::process::abort())
}
unsafe extern "C" fn wide_callback(input: *const Wide, scale: f64, bias: i64, output: *mut Wide) {
    let result = std::panic::catch_unwind(|| {
        OWNER.with(|owner| {
            assert!(!input.is_null() && !output.is_null());
            let state = owner.get();
            assert!(!state.is_null());
            let guest = unsafe { &mut *state };
            // Primitive fields only. Padding is deterministically zeroed; no
            // object references or borrowed raw native address enter guest Memory.
            let input = unsafe { input.read_unaligned() };
            guest
                .memory
                .write_int(DATA, 8, input.value.to_bits())
                .unwrap();
            guest
                .memory
                .write_int(DATA + 8, 8, input.count as u64)
                .unwrap();
            guest
                .memory
                .write_int(DATA + 16, 8, input.tag as u64)
                .unwrap();
            guest.memory.write(DATA + 32, &[0; 24]).unwrap();
            let rsp = STACK + 0x2ff8;
            guest.memory.write_int(rsp, 8, RETURN).unwrap();
            let mut cpu = Cpu64::new(WIDE_CALLBACK, rsp);
            // Win64 hidden sret occupies positional slot0, shifting subsequent
            // arguments. ARM64's ref/out ABI is explicitly converted here.
            cpu.regs[1] = DATA + 32;
            cpu.regs[2] = DATA;
            cpu.xmm[2][..8].copy_from_slice(&scale.to_le_bytes());
            cpu.regs[9] = bias as u64;
            execute(guest, &mut cpu);
            assert_eq!(cpu.regs[0], DATA + 32);
            let value = Wide {
                value: f64::from_bits(guest.memory.read_int(DATA + 32, 8).unwrap()),
                count: guest.memory.read_int(DATA + 40, 8).unwrap() as i64,
                tag: guest.memory.read_int(DATA + 48, 4).unwrap() as u32,
                padding: 0,
            };
            unsafe {
                output.write_unaligned(value);
            }
        })
    });
    if result.is_err() {
        std::process::abort();
    }
}
pub(super) fn extend_pe(bytes: &mut [u8]) {
    // cvtss2sd xmm1,xmm1; mulsd xmm0,xmm1; cvtsi2sd xmm2,r8;
    // addsd xmm0,xmm2; ret. Original authored x64, interpreted only.
    let float = [
        0xf3, 0x0f, 0x5a, 0xc9, 0xf2, 0x0f, 0x59, 0xc1, 0xf2, 0x49, 0x0f, 0x2a, 0xd0, 0xf2, 0x0f,
        0x58, 0xc2, 0xc3,
    ];
    bytes[0x500..0x500 + float.len()].copy_from_slice(&float);
    // Wide GuestWide(Wide input,double scale,long bias): RCX=sret,RDX=input,
    // XMM2=scale,R9=bias. Returns the destination in RAX.
    let wide = [
        0xf2, 0x0f, 0x10, 0x02, 0xf2, 0x0f, 0x59, 0xc2, 0xf2, 0x0f, 0x11, 0x01, 0x48, 0x8b, 0x42,
        0x08, 0x4c, 0x01, 0xc8, 0x48, 0x89, 0x41, 0x08, 0x8b, 0x42, 0x10, 0x83, 0xc0, 7, 0x89,
        0x41, 0x10, 0x48, 0x89, 0xc8, 0xc3,
    ];
    bytes[0x540..0x540 + wide.len()].copy_from_slice(&wide);
    for (at, gateway) in [(0x580, FLOAT_GATEWAY), (0x590, WIDE_GATEWAY)] {
        bytes[at..at + 2].copy_from_slice(&[0x48, 0xb8]);
        bytes[at + 2..at + 10].copy_from_slice(&gateway.to_le_bytes());
        bytes[at + 10..at + 12].copy_from_slice(&[0xff, 0xe0]);
    }
}
pub(super) fn run(mono: &Mono, state: *mut Guest) {
    assert_eq!(std::mem::size_of::<Wide>(), 24);
    let add = symbol!(
        mono.library,
        "mono_add_internal_call",
        unsafe extern "C" fn(*const c_char, Object)
    );
    let unbox = symbol!(
        mono.library,
        "mono_object_unbox",
        unsafe extern "C" fn(Object) -> Object
    );
    unsafe {
        add(
            c"GuestBridge::GuestFloatCallback".as_ptr(),
            float_callback as *const () as Object,
        );
        add(
            c"GuestBridge::GuestWideCallback".as_ptr(),
            wide_callback as *const () as Object,
        );
    }
    let floats = mono.find("FloatRoundTrip", 3);
    let wide = mono.find("WideRoundTrip", 3);
    let throw = mono.find("ThrowFloatRoundTrip", 3);
    for (value, scale, bias) in [(1.5, 4f32, 3i64), (-2.25, -0.5, -9), (0.0, 0.25, 17)] {
        let rsp = STACK + 0xff8;
        unsafe { &mut (*state).memory }
            .write_int(rsp, 8, RETURN)
            .unwrap();
        let mut cpu = Cpu64::new(BASE + 0x1380, rsp);
        cpu.xmm[0][..8].copy_from_slice(&f64::to_le_bytes(value));
        cpu.xmm[1][..4].copy_from_slice(&scale.to_le_bytes());
        cpu.regs[8] = bias as u64;
        for _ in 0..8 {
            if cpu.rip == RETURN {
                break;
            }
            if cpu.rip == FLOAT_GATEWAY {
                let mut value = f64::from_le_bytes(cpu.xmm[0][..8].try_into().unwrap());
                let mut scale = f32::from_le_bytes(cpu.xmm[1][..4].try_into().unwrap());
                let mut bias = cpu.regs[8] as i64;
                let mut args = [
                    (&mut value as *mut f64).cast(),
                    (&mut scale as *mut f32).cast(),
                    (&mut bias as *mut i64).cast(),
                ];
                let owner = Owner::install(state);
                let boxed = mono.invoke(floats, args.as_mut_ptr());
                drop(owner);
                let value = unsafe { unbox(boxed).cast::<f64>().read_unaligned() };
                cpu.xmm[0][..8].copy_from_slice(&value.to_le_bytes());
                cpu.rip = unsafe { &(*state).memory }
                    .read_int(cpu.regs[4], 8)
                    .unwrap();
                cpu.regs[4] += 8;
            } else {
                cpu.step(unsafe { &mut (*state).memory }).unwrap();
            }
        }
        assert_eq!(cpu.rip, RETURN);
        let actual = f64::from_le_bytes(cpu.xmm[0][..8].try_into().unwrap());
        assert_eq!(actual, value * scale as f64 + bias as f64 + 0.25);
    }
    for (value, count, tag, scale, bias) in [
        (2.5, 0x100000031i64, 13u32, 1.5, -7i64),
        (-4.0, -123, 0xfffffff9, 0.25, 1000),
    ] {
        let memory = unsafe { &mut (*state).memory };
        memory
            .write_int(DATA + 128, 8, f64::to_bits(value))
            .unwrap();
        memory.write_int(DATA + 136, 8, count as u64).unwrap();
        memory.write_int(DATA + 144, 8, tag as u64).unwrap();
        memory.write(DATA + 160, &[0xa5; 24]).unwrap();
        let rsp = STACK + 0xff8;
        memory.write_int(rsp, 8, RETURN).unwrap();
        let mut cpu = Cpu64::new(BASE + 0x1390, rsp);
        cpu.regs[1] = DATA + 160;
        cpu.regs[2] = DATA + 128;
        cpu.xmm[2][..8].copy_from_slice(&f64::to_le_bytes(scale));
        cpu.regs[9] = bias as u64;
        for _ in 0..8 {
            if cpu.rip == RETURN {
                break;
            }
            if cpu.rip == WIDE_GATEWAY {
                let memory = unsafe { &(*state).memory };
                // Preflight both complete intervals before invoking managed code.
                memory.check(cpu.regs[2], 24, READ).unwrap();
                memory.check(cpu.regs[1], 24, WRITE).unwrap();
                let mut input = Wide {
                    value: f64::from_bits(memory.read_int(cpu.regs[2], 8).unwrap()),
                    count: memory.read_int(cpu.regs[2] + 8, 8).unwrap() as i64,
                    tag: memory.read_int(cpu.regs[2] + 16, 4).unwrap() as u32,
                    padding: 0,
                };
                let mut scale = f64::from_le_bytes(cpu.xmm[2][..8].try_into().unwrap());
                let mut bias = cpu.regs[9] as i64;
                let mut args = [
                    (&mut input as *mut Wide).cast(),
                    (&mut scale as *mut f64).cast(),
                    (&mut bias as *mut i64).cast(),
                ];
                let owner = Owner::install(state);
                let boxed = mono.invoke(wide, args.as_mut_ptr());
                drop(owner);
                let output = unsafe { unbox(boxed).cast::<Wide>().read_unaligned() };
                let memory = unsafe { &mut (*state).memory };
                memory
                    .write_int(cpu.regs[1], 8, output.value.to_bits())
                    .unwrap();
                memory
                    .write_int(cpu.regs[1] + 8, 8, output.count as u64)
                    .unwrap();
                memory
                    .write_int(cpu.regs[1] + 16, 8, output.tag as u64)
                    .unwrap();
                cpu.regs[0] = cpu.regs[1];
                cpu.rip = memory.read_int(cpu.regs[4], 8).unwrap();
                cpu.regs[4] += 8;
            } else {
                cpu.step(unsafe { &mut (*state).memory }).unwrap();
            }
        }
        assert_eq!(cpu.rip, RETURN);
        assert_eq!(cpu.regs[0], DATA + 160);
        let memory = unsafe { &(*state).memory };
        assert_eq!(
            f64::from_bits(memory.read_int(DATA + 160, 8).unwrap()),
            value * scale + 0.5
        );
        assert_eq!(memory.read_int(DATA + 168, 8).unwrap() as i64, count + bias);
        assert_eq!(
            memory.read_int(DATA + 176, 4).unwrap() as u32,
            tag.wrapping_add(7)
        );
    }
    let mut value = 1.5f64;
    let mut scale = 4f32;
    let mut bias = 3i64;
    let mut args = [
        (&mut value as *mut f64).cast(),
        (&mut scale as *mut f32).cast(),
        (&mut bias as *mut i64).cast(),
    ];
    let owner = Owner::install(state);
    let mut exception = ptr::null_mut();
    let result =
        unsafe { (mono.invoke)(throw, ptr::null_mut(), args.as_mut_ptr(), &mut exception) };
    drop(owner);
    assert!(result.is_null() && !exception.is_null());
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
    assert_eq!(
        unsafe { std::ffi::CStr::from_ptr(name(class(exception))) }
            .to_str()
            .unwrap(),
        "ArgumentException"
    );
    OWNER.with(|owner| assert!(owner.get().is_null()));
    println!("PASS typed PE/IL ABI: mixed double/float/int64 XMM positions, 24B struct hidden sret/ref/out, signed and wrapping values, nested guest callbacks, returned managed exception; no runtime code generation");
}
