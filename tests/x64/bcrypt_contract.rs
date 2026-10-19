//! BCrypt provider scopes, reusable handles and validation order before migration.
use super::*;
use wire::x64::runtime::{Runtime64, TEB};

fn call(mem: &mut Memory, cpu: &mut Cpu64, rt: &mut Runtime64, name: &str, args: &[u64]) -> u64 {
    startup_dll_call(mem, cpu, rt, "bcrypt.dll", name, args)
}

#[test]
fn bcrypt_preserves_static_dynamic_provider_scopes_and_reuses_closed_full_width_handles() {
    let (mut mem, mut cpu, mut rt) = startup();
    let text = STACK + 0x100;
    let out = STACK + 0x300;
    for dll in ["bcrypt.dll", "kernel32.dll", "kernelbase.dll"] {
        rt.register_provider(dll);
        mem.write(text, format!("{dll}\0").as_bytes()).unwrap();
        let module = startup_call(&mut mem, &mut cpu, &mut rt, "LoadLibraryA", &[text]);
        for name in [
            "BCryptGenRandom",
            "BCryptOpenAlgorithmProvider",
            "BCryptCloseAlgorithmProvider",
        ] {
            assert!(wire::x64::runtime::import_supported(dll, name));
            assert!(!wire::x64::runtime::import_supported("advapi32.dll", name));
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
            let gateway = rt.register(dll, name);
            assert_eq!(
                startup_call(
                    &mut mem,
                    &mut cpu,
                    &mut rt,
                    "GetProcAddress",
                    &[module, text]
                ),
                gateway
            );
        }
    }
    mem.write(text, &[b'R', 0, b'N', 0, b'G', 0, 0, 0]).unwrap();
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "BCryptOpenAlgorithmProvider",
            &[out, text, 0, 0x100000000]
        ),
        0
    );
    assert!(rt.trace.iter().last().unwrap().contains(", 0, 0] -> 0x0"));
    let handle = mem.read_int(out, 8).unwrap();
    assert_eq!(handle, TEB + 0x700000);
    for (name, args, result) in [
        (
            "BCryptCloseAlgorithmProvider",
            vec![handle as u32 as u64, 0],
            0xc0000008,
        ),
        ("BCryptCloseAlgorithmProvider", vec![handle, 1], 0xc000000d),
        ("BCryptGenRandom", vec![handle, 1, 0, 2], 0xc000000d),
        ("BCryptGenRandom", vec![0, 1, 0, 0], 0xc0000008),
        (
            "BCryptGenRandom",
            vec![0, 1, 16 * 1024 * 1024 + 1, 2],
            0xc0000017,
        ),
        ("BCryptCloseAlgorithmProvider", vec![handle, 0x100000000], 0),
    ] {
        assert_eq!(call(&mut mem, &mut cpu, &mut rt, name, &args), result);
    }
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "BCryptOpenAlgorithmProvider",
            &[out, text, 0, 0]
        ),
        0
    );
    assert_eq!(mem.read_int(out, 8).unwrap(), handle);
}

#[test]
fn bcrypt_capacity_and_guest_faults_preserve_output_and_return_commit_order() {
    let (mut mem, mut cpu, mut rt) = startup();
    let text = STACK + 0x100;
    let out = STACK + 0x300;
    mem.write(text, &[b'R', 0, b'N', 0, b'G', 0, 0, 0]).unwrap();
    startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[77]);
    for index in 0..1024 {
        assert_eq!(
            call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "BCryptOpenAlgorithmProvider",
                &[out, text, 0, 0]
            ),
            0
        );
        assert_eq!(mem.read_int(out, 8).unwrap(), TEB + 0x700000 + index * 16);
    }
    mem.write_int(out, 8, 0xfeed).unwrap();
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "BCryptOpenAlgorithmProvider",
            &[out, text, 0, 0]
        ),
        0xc0000017
    );
    assert_eq!(mem.read_int(out, 8).unwrap(), 0xfeed);
    // Even when full, output validation precedes the capacity error.
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    for (index, arg) in [1, text, 0, 0].into_iter().enumerate() {
        cpu.regs[[1, 2, 8, 9][index]] = arg;
    }
    let before = (cpu.rip, cpu.regs, rt.api_calls);
    assert!(rt
        .dispatch(
            "bcrypt.dll",
            "BCryptOpenAlgorithmProvider",
            &mut cpu,
            &mut mem
        )
        .is_err());
    assert_eq!((cpu.rip, cpu.regs, rt.api_calls), before);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        77
    );
}
