//! Existing token allocation, query and handle behavior before owner extraction.
use super::*;
use wire::x64::runtime::Runtime64;

fn call(mem: &mut Memory, cpu: &mut Cpu64, rt: &mut Runtime64, name: &str, args: &[u64]) -> u64 {
    startup_dll_call(mem, cpu, rt, "advapi32.dll", name, args)
}

fn fault(mem: &mut Memory, cpu: &mut Cpu64, rt: &mut Runtime64, name: &str, args: &[u64]) {
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    for (index, value) in args.iter().enumerate() {
        if index < 4 {
            cpu.regs[[1, 2, 8, 9][index]] = *value;
        } else {
            mem.write_int(cpu.regs[4] + 40 + (index as u64 - 4) * 8, 8, *value)
                .unwrap();
        }
    }
    let before = (cpu.rip, cpu.regs, rt.api_calls);
    assert!(rt.dispatch("advapi32.dll", name, cpu, mem).is_err());
    assert_eq!((cpu.rip, cpu.regs, rt.api_calls), before);
}

#[test]
fn token_open_keeps_validation_order_dword_access_and_output_alias_saved_frame() {
    let (mut mem, mut cpu, mut rt) = startup();
    for (process, access, error) in [(0, 0x20, 6), (u64::MAX, 0x20, 50)] {
        assert_eq!(
            call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "OpenProcessToken",
                &[process, access, 1]
            ),
            0
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
            error
        );
    }
    fault(
        &mut mem,
        &mut cpu,
        &mut rt,
        "OpenProcessToken",
        &[u64::MAX, 8, 1],
    );
    let slot = STACK + 0x1f88;
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "OpenProcessToken",
            &[u64::MAX, 0x100000008, slot]
        ),
        1
    );
    assert_eq!(mem.read_int(slot, 8).unwrap(), 0x7fff4000);
    assert_eq!(cpu.rip, BASE);
    assert_eq!(cpu.regs[4], slot + 8);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "CloseHandle", &[0x7fff4000]),
        1
    );
}

#[test]
fn token_flags_and_query_faults_keep_protection_errors_aliases_and_copyout_order() {
    let (mut mem, mut cpu, mut rt) = startup();
    let out = STACK + 0x100;
    let used = STACK + 0x200;
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "OpenProcessToken",
            &[u64::MAX, 0, out]
        ),
        1
    );
    let token = mem.read_int(out, 8).unwrap();
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "GetTokenInformation",
            &[token, u64::MAX, 1, 1, 1]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        5
    );
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "GetTokenInformation",
            &[token | 0x100000000, 20, 1, 1, 1]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        6
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "SetHandleInformation",
            &[token, 0x100000003, 0x100000003]
        ),
        1
    );
    let slot = STACK + 0x1f88;
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "GetHandleInformation",
            &[token, slot]
        ),
        1
    );
    assert_eq!(mem.read_int(slot, 4).unwrap(), 3);
    assert_eq!(cpu.rip, BASE);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "CloseHandle", &[token]),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        6
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "SetHandleInformation",
            &[token, 4, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        87
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "SetHandleInformation",
            &[token, 2, 0]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "GetHandleInformation",
            &[token, out]
        ),
        1
    );
    assert_eq!(mem.read_int(out, 4).unwrap(), 1);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "CloseHandle", &[token]),
        1
    );
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "OpenProcessToken",
            &[u64::MAX, 8, out]
        ),
        1
    );
    let token = mem.read_int(out, 8).unwrap();
    assert_eq!(token, 0x7fff4001);
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "GetTokenInformation",
            &[token, 20, used, 4, used]
        ),
        1
    );
    assert_eq!(mem.read_int(used, 4).unwrap(), 4); // Required-size copy-out follows the data store.
    mem.write_int(used, 4, 0xfeed).unwrap();
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "GetTokenInformation",
            &[token, 1, u64::MAX, 44, used]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        87
    );
    assert_eq!(mem.read_int(used, 4).unwrap(), 0xfeed);
    let data = 0x560000000;
    let required = data + 4096;
    mem.map(data, 8192, READ | WRITE).unwrap();
    mem.write(data, &[0xa5; 28]).unwrap();
    mem.write_int(required, 4, 0xfeed).unwrap();
    for address in [data, required] {
        mem.virtual_protect(address, 4096, 4 | wire::mem::PAGE_GUARD)
            .unwrap();
    }
    fault(
        &mut mem,
        &mut cpu,
        &mut rt,
        "GetTokenInformation",
        &[token, 25, data, 28, required],
    );
    assert_eq!(mem.read_int(required, 4).unwrap(), 0xfeed);
    assert!(mem.read_int(data, 4).unwrap_err().contains("guard-page"));
    let mut bytes = [0; 28];
    mem.read(data, &mut bytes).unwrap();
    assert_eq!(bytes, [0xa5; 28]);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        87
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "CloseHandle", &[token]),
        1
    );
}

#[test]
fn token_capacity_precedes_output_and_close_does_not_reuse_retired_tokens() {
    let (mut mem, mut cpu, mut rt) = startup();
    let out = STACK + 0x100;
    for index in 0..128 {
        assert_eq!(
            call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "OpenProcessToken",
                &[u64::MAX, 0, out]
            ),
            1
        );
        assert_eq!(mem.read_int(out, 8).unwrap(), 0x7fff4000 + index);
    }
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "OpenProcessToken",
            &[u64::MAX, 0, 1]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        8
    );
    for index in 0..128 {
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "CloseHandle",
                &[0x7fff4000 + index]
            ),
            1
        );
    }
    for index in 128..4096 {
        assert_eq!(
            call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "OpenProcessToken",
                &[u64::MAX, 0, out]
            ),
            1
        );
        let token = mem.read_int(out, 8).unwrap();
        assert_eq!(token, 0x7fff4000 + index);
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, "CloseHandle", &[token]),
            1
        );
    }
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "OpenProcessToken",
            &[u64::MAX, 0, 1]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        8
    );
}
