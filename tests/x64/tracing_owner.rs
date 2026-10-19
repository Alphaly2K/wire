//! Independent ETW lifetime and fault-order baselines before extracting its owner.
use super::*;
use wire::x64::runtime::Runtime64;

fn call(mem: &mut Memory, cpu: &mut Cpu64, rt: &mut Runtime64, name: &str, args: &[u64]) -> u64 {
    startup_dll_call(mem, cpu, rt, "advapi32.dll", name, args)
}

fn fault(mem: &mut Memory, cpu: &mut Cpu64, rt: &mut Runtime64, name: &str, args: &[u64]) {
    assert!(args.len() <= 4);
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    for (index, value) in args.iter().enumerate() {
        cpu.regs[[1, 2, 8, 9][index]] = *value;
    }
    let before = (cpu.rip, cpu.regs[4], cpu.regs[0], rt.api_calls);
    assert!(rt.dispatch("advapi32.dll", name, cpu, mem).is_err());
    assert_eq!((cpu.rip, cpu.regs[4], cpu.regs[0], rt.api_calls), before);
}

#[test]
fn etw_registration_faults_do_not_consume_tokens_and_output_can_alias_saved_frame() {
    let (mut mem, mut cpu, mut rt) = startup();
    let guid = 0x550000000;
    let out = guid + 4096;
    mem.map(guid, 8192, READ | WRITE).unwrap();
    mem.write(guid, &[0x42; 16]).unwrap();
    mem.write_int(out, 8, 0xfeed).unwrap();
    startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[123]);
    mem.virtual_protect(guid, 4096, 4 | wire::mem::PAGE_GUARD)
        .unwrap();
    mem.virtual_protect(out, 4096, 4 | wire::mem::PAGE_GUARD)
        .unwrap();
    fault(
        &mut mem,
        &mut cpu,
        &mut rt,
        "EventRegister",
        &[guid, 1, 2, out],
    );
    let mut bytes = [0; 16];
    mem.read(guid, &mut bytes).unwrap(); // Source guard is consumed before output preflight.
    assert_eq!(bytes, [0x42; 16]);
    assert!(mem.read_int(out, 8).unwrap_err().contains("guard-page"));
    assert_eq!(mem.read_int(out, 8).unwrap(), 0xfeed);
    fault(
        &mut mem,
        &mut cpu,
        &mut rt,
        "EventRegister",
        &[guid, 1, 2, 1],
    );
    let return_slot = STACK + 0x1f88;
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "EventRegister",
            &[guid, u64::MAX, 1, return_slot]
        ),
        0
    );
    assert_eq!(mem.read_int(return_slot, 8).unwrap(), 0x7fff6000);
    assert_eq!(cpu.rip, BASE);
    assert_eq!(cpu.regs[4], return_slot + 8);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        123
    );
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "EventUnregister",
            &[0x7fff6000]
        ),
        0
    );
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "EventUnregister",
            &[0x7fff6000]
        ),
        6
    );
}

#[test]
fn etw_disabled_events_check_headers_but_do_not_follow_payload_or_callback_pointers() {
    let (mut mem, mut cpu, mut rt) = startup();
    let guid = STACK + 0x100;
    let out = STACK + 0x200;
    let data = STACK + 0x300;
    mem.write(guid, &[0x42; 16]).unwrap();
    mem.write_int(data, 8, 1).unwrap(); // Unmapped payload, valid descriptor array.
    mem.write_int(data + 8, 8, u32::MAX as u64).unwrap();
    startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[123]);
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "EventRegister",
            &[guid, 1, u64::MAX, out]
        ),
        0
    );
    let handle = mem.read_int(out, 8).unwrap();
    for name in ["EventEnabled", "EventProviderEnabled"] {
        assert_eq!(
            call(
                &mut mem,
                &mut cpu,
                &mut rt,
                name,
                &[handle | 0x100000000, 1, 1]
            ),
            0
        );
    }
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "EventWrite",
            &[handle | 0x100000000, 1, 129, 1]
        ),
        6
    );
    fault(
        &mut mem,
        &mut cpu,
        &mut rt,
        "EventWrite",
        &[handle, 1, 129, 1],
    );
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "EventWrite",
            &[handle, guid, 129, 1]
        ),
        87
    );
    fault(
        &mut mem,
        &mut cpu,
        &mut rt,
        "EventWrite",
        &[handle, guid, 1, 1],
    );
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "EventWrite",
            &[handle, guid, 0x100000001, data]
        ),
        0
    );
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "EventWriteTransfer",
            &[handle, guid, 1, 1, 0x100000001, data]
        ),
        0
    );
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "EventWriteString",
            &[handle, u64::MAX, u64::MAX, 1]
        ),
        0
    );
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "EventSetInformation",
            &[handle, u64::MAX, 1, u64::MAX]
        ),
        50
    );
    assert_eq!(mem.read_int(data, 8).unwrap(), 1);
    assert_eq!(mem.read_int(data + 8, 8).unwrap(), u32::MAX as u64);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        123
    );
    assert_eq!(
        call(&mut mem, &mut cpu, &mut rt, "EventUnregister", &[handle]),
        0
    );
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "EventWrite",
            &[handle, 1, 1, 1]
        ),
        6
    );
}

#[test]
fn etw_registration_limits_keep_output_preflight_and_monotonic_token_exhaustion() {
    let (mut mem, mut cpu, mut rt) = startup();
    let guid = STACK + 0x100;
    let out = STACK + 0x200;
    mem.write(guid, &[0x42; 16]).unwrap();
    let mut handles = Vec::new();
    for index in 0..64 {
        assert_eq!(
            call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "EventRegister",
                &[guid, 1, 2, out]
            ),
            0
        );
        let handle = mem.read_int(out, 8).unwrap();
        assert_eq!(handle, 0x7fff6000 + index);
        handles.push(handle);
    }
    mem.write_int(out, 8, 0xfeed).unwrap();
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "EventRegister",
            &[guid, 1, 2, out]
        ),
        8
    );
    assert_eq!(mem.read_int(out, 8).unwrap(), 0xfeed);
    fault(
        &mut mem,
        &mut cpu,
        &mut rt,
        "EventRegister",
        &[1, 1, 2, out],
    );
    fault(
        &mut mem,
        &mut cpu,
        &mut rt,
        "EventRegister",
        &[guid, 1, 2, 1],
    );
    for handle in handles {
        assert_eq!(
            call(&mut mem, &mut cpu, &mut rt, "EventUnregister", &[handle]),
            0
        );
    }
    for index in 64..4096 {
        assert_eq!(
            call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "EventRegister",
                &[guid, 1, 2, out]
            ),
            0
        );
        let handle = mem.read_int(out, 8).unwrap();
        assert_eq!(handle, 0x7fff6000 + index);
        assert_eq!(
            call(&mut mem, &mut cpu, &mut rt, "EventUnregister", &[handle]),
            0
        );
    }
    mem.write_int(out, 8, 0xfeed).unwrap();
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "EventRegister",
            &[guid, 1, 2, out]
        ),
        8
    );
    assert_eq!(mem.read_int(out, 8).unwrap(), 0xfeed);
}
