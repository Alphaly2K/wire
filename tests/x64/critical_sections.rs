//! Independent Win64 critical-section layouts, widths and commit boundaries.
use super::*;

#[test]
fn critical_sections_retry_after_final_release_across_guest_threads() {
    use wire::x64::process::{Outcome, Process64};
    const CODE: u64 = 0x592000000;
    const WORKER: u64 = CODE + 4096;
    const DATA: u64 = CODE + 8192;
    fn mov(code: &mut Vec<u8>, register: u8, value: u64) {
        code.extend([0x48 | (register >> 3), 0xb8 | (register & 7)]);
        code.extend(value.to_le_bytes());
    }
    fn call(p: &mut Process64, code: &mut Vec<u8>, name: &str, args: &[u64]) {
        for (n, &value) in args.iter().enumerate() {
            mov(code, if n < 4 { [1, 2, 8, 9][n] } else { 0 }, value);
            if n >= 4 {
                code.extend([0x48, 0x89, 0x44, 0x24, 32 + (n as u8 - 4) * 8]);
            }
        }
        mov(code, 0, p.runtime.register("kernel32.dll", name));
        code.extend([0xff, 0xd0]);
    }
    let path = std::env::temp_dir().join(format!("wire-critical64-{}.exe", std::process::id()));
    std::fs::write(&path, fixture()).unwrap();
    for slice in [1, 100_000] {
        let mut p = Process64::load(&path, false).unwrap();
        p.memory
            .map(CODE, 3 * 4096, READ | WRITE | EXECUTE)
            .unwrap();
        let mut main = vec![0x48, 0x83, 0xec, 0x38];
        call(&mut p, &mut main, "InitializeCriticalSection", &[DATA]);
        call(&mut p, &mut main, "EnterCriticalSection", &[DATA]);
        call(&mut p, &mut main, "EnterCriticalSection", &[DATA]);
        call(&mut p, &mut main, "CreateThread", &[0, 0, WORKER, 0, 0, 0]);
        main.extend([0x49, 0x89, 0xc4]);
        for _ in 0..2 {
            call(&mut p, &mut main, "SwitchToThread", &[]);
            call(&mut p, &mut main, "LeaveCriticalSection", &[DATA]);
        }
        main.extend([0x4c, 0x89, 0xe1]);
        mov(&mut main, 2, u32::MAX as u64);
        call(&mut p, &mut main, "WaitForSingleObject", &[]);
        mov(&mut main, 0, DATA + 48);
        main.extend([0x48, 0x8b, 0x00, 0x48, 0x83, 0xc4, 0x38, 0xc3]);
        let mut worker = vec![0x48, 0x83, 0xec, 0x28];
        call(&mut p, &mut worker, "TryEnterCriticalSection", &[DATA]);
        mov(&mut worker, 2, DATA + 64);
        worker.extend([0x48, 0x89, 0x02]);
        call(&mut p, &mut worker, "EnterCriticalSection", &[DATA]);
        mov(&mut worker, 0, 42);
        mov(&mut worker, 2, DATA + 48);
        worker.extend([0x48, 0x89, 0x02]);
        call(&mut p, &mut worker, "LeaveCriticalSection", &[DATA]);
        worker.extend([0x48, 0x83, 0xc4, 0x28, 0xc3]);
        p.memory.write(CODE, &main).unwrap();
        p.memory.write(WORKER, &worker).unwrap();
        p.cpu.rip = CODE;
        loop {
            match p.run(p.cpu.work + slice).unwrap() {
                Outcome::Budget => assert!(p.cpu.work < 20000),
                result => {
                    assert_eq!(result, Outcome::Returned(42));
                    break;
                }
            }
        }
        assert_eq!(p.memory.read_int(DATA + 64, 8).unwrap(), 0);
        assert_eq!(p.memory.read_int(DATA + 12, 4).unwrap(), 0);
        assert_eq!(p.memory.read_int(DATA + 16, 8).unwrap(), 0);
        assert_eq!(p.memory.read_int(DATA + 8, 4).unwrap(), u32::MAX as u64);
        assert_eq!(
            p.runtime
                .trace
                .iter()
                .filter(|r| r.contains("!EnterCriticalSection "))
                .count(),
            3
        );
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn critical_sections_keep_win64_widths_ownership_and_readonly_contention() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let section = STACK + 0x100;
    let high = 0xfacefeed_00000000;
    for dll in ["kernel32.dll", "kernelbase.dll"] {
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                dll,
                "InitializeCriticalSectionAndSpinCount",
                &[section, high | 500]
            ),
            1
        );
        assert_eq!(mem.read_int(section + 32, 8).unwrap(), 500);
        // Preserve the historical 64-bit return carrier of the previous slot.
        mem.write_int(section + 32, 8, 0x12345678000001f4).unwrap();
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                dll,
                "SetCriticalSectionSpinCount",
                &[section, high | 900]
            ),
            0x12345678000001f4
        );
        assert_eq!(mem.read_int(section + 32, 8).unwrap(), 900);
        let tid = startup_call(&mut mem, &mut cpu, &mut runtime, "GetCurrentThreadId", &[]);
        mem.write_int(section + 16, 8, tid + 1).unwrap();
        mem.write_int(section + 12, 4, 1).unwrap();
        mem.protect(STACK, 4096, READ).unwrap();
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                dll,
                "TryEnterCriticalSection",
                &[section]
            ),
            0
        );
        cpu.regs[1] = section;
        let before = cpu.clone();
        let count = runtime.api_calls;
        assert!(runtime
            .dispatch(dll, "EnterCriticalSection", &mut cpu, &mut mem)
            .is_err());
        assert_eq!(cpu, before);
        assert_eq!(runtime.api_calls, count);
        mem.protect(STACK, 4096, READ | WRITE).unwrap();
        for name in ["LeaveCriticalSection", "DeleteCriticalSection"] {
            cpu.regs[1] = section;
            let before = cpu.clone();
            assert!(runtime.dispatch(dll, name, &mut cpu, &mut mem).is_err());
            assert_eq!(cpu, before);
            assert!(!wire::x64::runtime::import_supported("user32.dll", name));
        }
    }
}

#[test]
fn critical_sections_keep_win64_saved_return_and_whole_structure_write_checks() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let section = STACK + 4096 - 16;
    mem.write(section, &[0xaa; 40]).unwrap();
    mem.protect(STACK + 4096, 4096, READ).unwrap();
    cpu.regs[1] = section;
    let before = cpu.clone();
    assert!(runtime
        .dispatch(
            "kernel32.dll",
            "InitializeCriticalSection",
            &mut cpu,
            &mut mem
        )
        .is_err());
    assert_eq!(cpu, before);
    assert_eq!(runtime.api_calls, 0);
    let mut bytes = [0; 40];
    mem.read(section, &mut bytes).unwrap();
    assert_eq!(bytes, [0xaa; 40]);
    mem.protect(STACK + 4096, 4096, READ | WRITE).unwrap();
    let pointer = STACK + 0x100;
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "InitializeCriticalSection",
        &[pointer],
    );
    cpu.regs[4] = 0xdeadbeef;
    cpu.regs[1] = pointer;
    let before = cpu.clone();
    let count = runtime.api_calls;
    assert!(runtime
        .dispatch(
            "kernel32.dll",
            "TryEnterCriticalSection",
            &mut cpu,
            &mut mem
        )
        .is_err());
    assert_eq!(cpu, before);
    assert_eq!(runtime.api_calls, count);
    assert_eq!(mem.read_int(pointer + 12, 4).unwrap(), 0);
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    cpu.regs[1] = cpu.regs[4];
    assert_eq!(
        runtime
            .dispatch(
                "kernel32.dll",
                "InitializeCriticalSection",
                &mut cpu,
                &mut mem
            )
            .unwrap(),
        wire::x64::runtime::Control::Returned
    );
    assert_eq!(cpu.rip, BASE);
}
