//! Win64 POINT is a packed 64-bit argument; preserve provider and frame behavior.
use super::*;

#[test]
fn geometry_contract_win64_keeps_packed_points_dwords_overlap_and_saved_frame() {
    let (mut mem, mut cpu, mut rt) = startup();
    let a = STACK + 0x100;
    mem.write(
        a,
        &[-5i32, -7, 10, 20]
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let point = ((-7i32) as u32 as u64) << 32 | (-5i32) as u32 as u64;
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "user32.dll",
            "PtInRect",
            &[a, point]
        ),
        1
    );
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "user32.dll",
            "PtInRect",
            &[a, (20u64 << 32) | 10]
        ),
        0
    );
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "user32.dll",
            "CopyRect",
            &[a + 4, a]
        ),
        1
    );
    assert_eq!(mem.read_int(a + 4, 4).unwrap() as u32 as i32, -5);
    assert_eq!(mem.read_int(a + 16, 4).unwrap(), 20);
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "user32.dll",
            "OffsetRect",
            &[a + 4, 0xabcdef01_00000001, 0xabcdef01_ffffffff]
        ),
        1
    );
    assert_eq!(mem.read_int(a + 4, 4).unwrap() as u32 as i32, -4);
    assert_eq!(mem.read_int(a + 8, 4).unwrap() as u32 as i32, -8);
    for name in [
        "AdjustWindowRect",
        "AdjustWindowRectEx",
        "AdjustWindowRectExForDpi",
    ] {
        mem.write(
            a,
            &[0i32, 0, 200, 100]
                .into_iter()
                .flat_map(i32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[77]);
        let args = [
            a,
            0xabcdef01_00c40000,
            1u64 << 32,
            1u64 << 32,
            0xabcdef01_00000060,
        ];
        let n = if name == "AdjustWindowRect" {
            3
        } else if name.ends_with("ForDpi") {
            5
        } else {
            4
        };
        assert_eq!(
            startup_dll_call(&mut mem, &mut cpu, &mut rt, "user32.dll", name, &args[..n]),
            1
        );
        let rect: [i32; 4] =
            std::array::from_fn(|i| mem.read_int(a + i as u64 * 4, 4).unwrap() as u32 as i32);
        assert_eq!(rect, [-4, -27, 204, 104]);
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
            77
        );
    }
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "user32.dll",
            "AdjustWindowRectExForDpi",
            &[1, 0, 0, 0, 144]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        50
    );
    mem.write_int(a, 4, 0x11223344).unwrap();
    let slot = STACK + 0x1f88;
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "user32.dll",
            "CopyRect",
            &[slot, a]
        ),
        1
    );
    assert_eq!(mem.read_int(slot, 4).unwrap(), 0x11223344);
    assert_eq!(cpu.rip, BASE);
}

#[test]
fn geometry_contract_win64_keeps_provider_sets_and_absent_rectangle_helpers() {
    let (mut mem, mut cpu, mut rt) = startup();
    let text = STACK + 0x100;
    for dll in ["kernel32.dll", "kernelbase.dll", "user32.dll"] {
        rt.register_provider(dll);
        mem.write(text, format!("{dll}\0").as_bytes()).unwrap();
        let module = startup_call(&mut mem, &mut cpu, &mut rt, "LoadLibraryA", &[text]);
        for name in [
            "PtInRect",
            "CopyRect",
            "OffsetRect",
            "AdjustWindowRect",
            "AdjustWindowRectEx",
            "AdjustWindowRectExForDpi",
        ] {
            assert!(wire::x64::runtime::import_supported(dll, name));
            mem.write(text, format!("{name}\0").as_bytes()).unwrap();
            assert_eq!(
                startup_call(
                    &mut mem,
                    &mut cpu,
                    &mut rt,
                    "GetProcAddress",
                    &[module, text]
                ) != 0,
                dll == "user32.dll",
                "{dll}!{name}"
            );
        }
        for name in [
            "SetRect",
            "SetRectEmpty",
            "EqualRect",
            "IsRectEmpty",
            "InflateRect",
            "IntersectRect",
            "UnionRect",
        ] {
            assert!(!wire::x64::runtime::import_supported(dll, name));
        }
    }
}

#[test]
fn geometry_contract_win64_copy_fault_preserves_output_and_cpu_frame() {
    let (mut mem, mut cpu, mut rt) = startup();
    let output = STACK + 4096 - 8;
    mem.write(STACK, &[1; 16]).unwrap();
    mem.write(output, &[0xa5; 16]).unwrap();
    mem.protect(STACK + 4096, 4096, READ).unwrap();
    cpu.regs[1] = output;
    cpu.regs[2] = STACK;
    let rsp = cpu.regs[4];
    let rip = cpu.rip;
    let calls = rt.api_calls;
    assert!(rt
        .dispatch("user32.dll", "CopyRect", &mut cpu, &mut mem)
        .is_err());
    let mut bytes = [0; 16];
    mem.read(output, &mut bytes).unwrap();
    assert_eq!(bytes, [0xa5; 16]);
    assert_eq!((cpu.rip, cpu.regs[4], rt.api_calls), (rip, rsp, calls));
}
