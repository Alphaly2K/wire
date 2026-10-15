//! Behavioral baselines for the cooperative interlocked and SLIST adapters.
use super::*;

#[test]
fn interlocked_contract_baseline_keeps_pe32_wrap_readonly_compare_and_faults() {
    let mut v = Vm::new();
    v.k("SetLastError", &[0x55]);
    v.mem.write_u32(DATA, u32::MAX).unwrap();
    assert_eq!(v.k("InterlockedIncrement", &[DATA]), 0);
    assert_eq!(v.k("InterlockedDecrement", &[DATA]), u32::MAX);
    assert_eq!(v.k("InterlockedExchange", &[DATA, 0x8000_0000]), u32::MAX);
    v.mem
        .virtual_protect(DATA, 4096, wire::mem::PAGE_READONLY)
        .unwrap();
    // An unequal comparand does not require write permission.
    assert_eq!(
        v.k("InterlockedCompareExchange", &[DATA, 42, 17]),
        0x8000_0000
    );
    for (name, args) in [
        ("InterlockedIncrement", vec![DATA]),
        ("InterlockedDecrement", vec![DATA]),
        ("InterlockedExchange", vec![DATA, 42]),
        ("InterlockedCompareExchange", vec![DATA, 42, 0x8000_0000]),
        ("InitializeSListHead", vec![DATA]),
        ("InterlockedFlushSList", vec![DATA]),
    ] {
        v.cpu.regs[0] = 0x1234;
        assert!(v.try_call("kernel32.dll", name, &args).is_err());
        assert_eq!(v.cpu.regs[0], 0x1234);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256);
        assert_eq!(v.mem.read_u32(DATA).unwrap(), 0x8000_0000);
        assert_eq!(v.mem.read_u32(TEB_ADDR + 0x34).unwrap(), 0x55);
    }
    for name in ["InitializeSListHead", "InterlockedFlushSList"] {
        assert_eq!(
            v.try_call("kernel32.dll", name, &[1]).unwrap_err(),
            "unaligned SLIST_HEADER"
        );
    }
}

#[test]
fn interlocked_contract_baseline_keeps_pe32_output_before_return_frame() {
    let mut v = Vm::new();
    let sp = STACK_BASE + STACK_SIZE - 256;
    assert_eq!(v.k("InterlockedExchange", &[sp, 0x1234]), RET);
    assert_eq!(v.cpu.eip, 0x1234);
    assert_eq!(v.cpu.esp(), sp + 12);
    assert_eq!(v.k("InitializeSListHead", &[sp]), 0);
    assert_eq!(v.cpu.eip, 0);
    assert_eq!(v.cpu.esp(), sp + 8);
    for (name, slots) in [
        ("InterlockedIncrement", 1),
        ("InterlockedDecrement", 1),
        ("InterlockedExchange", 2),
        ("InterlockedCompareExchange", 3),
        ("InitializeSListHead", 1),
        ("InterlockedFlushSList", 1),
    ] {
        assert_eq!(v.call("user32.dll", name, &[DATA; 3][..slots]), 0);
        assert_eq!(v.cpu.esp(), sp + 4);
    }
}
