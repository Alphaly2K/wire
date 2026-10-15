//! Win64 SLIST width/alignment and return-frame baseline.
use super::*;

#[test]
fn interlocked_contract_baseline_keeps_win64_slist_alias_width_and_absence() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let sp = STACK + 0x1000;
    let ret = 0x140000100;
    mem.write_int(wire::x64::runtime::TEB + 0x68, 4, 0x55)
        .unwrap();
    cpu.regs[4] = sp;
    mem.write_int(sp, 8, ret).unwrap();
    cpu.regs[1] = sp;
    runtime
        .dispatch("kernelbase.dll", "InitializeSListHead", &mut cpu, &mut mem)
        .unwrap();
    assert_eq!(mem.read_int(sp, 8).unwrap(), 0);
    assert_eq!(mem.read_int(sp + 8, 8).unwrap(), 0);
    assert_eq!(cpu.rip, ret);
    assert_eq!(cpu.regs[4], sp + 8);
    for address in [1, 0xface0000_00020000] {
        cpu.regs[4] = sp;
        mem.write_int(sp, 8, ret).unwrap();
        cpu.regs[1] = address;
        let before = cpu.clone();
        let calls = runtime.api_calls;
        assert!(runtime
            .dispatch("kernel32.dll", "InitializeSListHead", &mut cpu, &mut mem)
            .is_err());
        assert_eq!(cpu, before);
        assert_eq!(runtime.api_calls, calls);
        assert_eq!(
            mem.read_int(wire::x64::runtime::TEB + 0x68, 4).unwrap(),
            0x55
        );
    }
    assert!(!wire::x64::runtime::import_supported(
        "user32.dll",
        "InitializeSListHead"
    ));
    for name in [
        "InterlockedIncrement",
        "InterlockedDecrement",
        "InterlockedExchange",
        "InterlockedCompareExchange",
        "InterlockedFlushSList",
    ] {
        for dll in ["kernel32.dll", "kernelbase.dll"] {
            assert!(!wire::x64::runtime::import_supported(dll, name));
            let before = cpu.clone();
            let calls = runtime.api_calls;
            assert!(runtime.dispatch(dll, name, &mut cpu, &mut mem).is_err());
            assert_eq!(cpu, before);
            assert_eq!(runtime.api_calls, calls);
        }
    }
}
