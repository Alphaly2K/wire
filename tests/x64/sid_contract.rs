//! Independent SID ABI, guard order and existing registered-gateway lookup.
use super::*;
use wire::x64::runtime::Runtime64;

fn call(mem: &mut Memory, cpu: &mut Cpu64, rt: &mut Runtime64, name: &str, args: &[u64]) -> u64 {
    startup_dll_call(mem, cpu, rt, "advapi32.dll", name, args)
}

fn fault(mem: &mut Memory, cpu: &mut Cpu64, rt: &mut Runtime64, name: &str, args: &[u64]) {
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    for (i, value) in args.iter().enumerate() {
        cpu.regs[[1, 2, 8, 9][i]] = *value;
    }
    let before = (cpu.rip, cpu.regs, rt.api_calls);
    assert!(rt.dispatch("advapi32.dll", name, cpu, mem).is_err());
    assert_eq!((cpu.rip, cpu.regs, rt.api_calls), before);
}

#[test]
fn sid_win64_keeps_static_providers_and_registered_gateway_dynamic_lookup() {
    let (mut mem, mut cpu, mut rt) = startup();
    let text = STACK + 0x100;
    for dll in ["advapi32.dll", "kernel32.dll", "kernelbase.dll"] {
        rt.register_provider(dll);
        mem.write(text, format!("{dll}\0").as_bytes()).unwrap();
        let module = startup_call(&mut mem, &mut cpu, &mut rt, "LoadLibraryA", &[text]);
        for name in [
            "GetSidSubAuthority",
            "GetSidSubAuthorityCount",
            "GetSidIdentifierAuthority",
            "IsValidSid",
            "GetLengthSid",
            "CopySid",
            "EqualSid",
            "GetSidLengthRequired",
            "InitializeSid",
        ] {
            assert!(wire::x64::runtime::import_supported(dll, name));
            assert!(!wire::x64::runtime::import_supported("user32.dll", name));
            mem.write(text, format!("{name}\0").as_bytes()).unwrap();
            assert_eq!(
                startup_call(
                    &mut mem,
                    &mut cpu,
                    &mut rt,
                    "GetProcAddress",
                    &[module, text]
                ),
                0
            );
            let address = rt.register(dll, name);
            assert_eq!(
                startup_call(
                    &mut mem,
                    &mut cpu,
                    &mut rt,
                    "GetProcAddress",
                    &[module, text]
                ),
                address
            );
        }
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                dll,
                "GetSidLengthRequired",
                &[0x1000000ff]
            ),
            1028
        );
        assert!(rt.trace.iter().last().unwrap().contains("args=[1000000ff]"));
    }
}

#[test]
fn sid_byte_counts_dword_index_full_pointer_and_header_only_initialization_are_preserved() {
    let (mut mem, mut cpu, mut rt) = startup();
    let sid = 0x550000000;
    let authority = sid + 4096;
    mem.map(sid, 8192, READ | WRITE).unwrap();
    mem.write(sid, &[0xa5; 12]).unwrap();
    mem.write(authority, &[0, 0, 0, 0, 0, 5]).unwrap();
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "InitializeSid",
            &[sid, authority, 0x100000001]
        ),
        1
    );
    assert!(rt.trace.iter().last().unwrap().contains("100000001]"));
    assert_eq!(mem.read_int(sid, 8).unwrap(), 0x0500000000000101);
    assert_eq!(mem.read_int(sid + 8, 4).unwrap(), 0xa5a5a5a5);
    for (name, args, expected) in [
        ("GetSidIdentifierAuthority", vec![sid], sid + 2),
        ("GetSidSubAuthorityCount", vec![sid], sid + 1),
        ("GetSidSubAuthority", vec![sid, 0x100000000], sid + 8),
        ("GetLengthSid", vec![sid], 12),
        ("IsValidSid", vec![sid], 1),
        ("IsValidSid", vec![sid as u32 as u64], 0),
    ] {
        assert_eq!(call(&mut mem, &mut cpu, &mut rt, name, &args), expected);
    }
    assert_eq!(
        call(&mut mem, &mut cpu, &mut rt, "GetSidSubAuthority", &[sid, 1]),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        87
    );
}

#[test]
fn sid_initialize_and_copy_keep_distinct_guard_fault_and_error_commit_order() {
    let (mut mem, mut cpu, mut rt) = startup();
    let source = 0x550000000;
    let output = source + 4096;
    let authority = source + 8192;
    mem.map(source, 12288, READ | WRITE).unwrap();
    mem.write(source, &[1, 1, 0, 0, 0, 0, 0, 5, 42, 0, 0, 0])
        .unwrap();
    mem.write(authority, &[0, 0, 0, 0, 0, 5]).unwrap();
    mem.write(output, &[0xa5; 12]).unwrap();
    let guard = 4 | wire::mem::PAGE_GUARD;
    mem.virtual_protect(output, 4096, guard).unwrap();
    mem.virtual_protect(authority, 4096, guard).unwrap();
    // A count over fifteen rejects before touching either guard.
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "InitializeSid",
            &[output, authority, 16]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        87
    );
    fault(
        &mut mem,
        &mut cpu,
        &mut rt,
        "InitializeSid",
        &[output, authority, 1],
    );
    assert_eq!(mem.read_int(output, 1).unwrap(), 0xa5);
    // The first fault consumed only the destination guard; the source still faults.
    fault(
        &mut mem,
        &mut cpu,
        &mut rt,
        "InitializeSid",
        &[output, authority, 1],
    );
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "InitializeSid",
            &[output, authority, 1]
        ),
        1
    );
    assert_eq!(mem.read_int(output + 8, 4).unwrap(), 0xa5a5a5a5);
    mem.virtual_protect(output, 4096, guard).unwrap();
    fault(
        &mut mem,
        &mut cpu,
        &mut rt,
        "CopySid",
        &[12, output, source],
    );
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "CopySid",
            &[12, output, source]
        ),
        1
    );
    assert_eq!(mem.read_int(output + 8, 4).unwrap(), 42);
    mem.write(output, &[0xa5; 12]).unwrap();
    mem.virtual_protect(source, 4096, guard).unwrap();
    mem.virtual_protect(output, 4096, guard).unwrap();
    // The optional preflight consumes the source guard; the repeated SID read
    // then succeeds and a write fault becomes error 1337 with a committed return.
    let calls = rt.api_calls;
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "CopySid",
            &[12, output, source]
        ),
        0
    );
    assert_eq!(rt.api_calls, calls + 1);
    assert_eq!(cpu.rip, BASE);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        1337
    );
    assert_eq!(mem.read_int(output, 1).unwrap(), 0xa5);
    assert_eq!(mem.read_int(source + 8, 4).unwrap(), 42);
}
