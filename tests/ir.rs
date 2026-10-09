//! Static flag proofs, cache correctness and observable execution boundaries.
use wire::api::{ApiLogger, ApiState, ApiTable, HandleObj};
use wire::cpu::{Cpu, RunOutcome, AF, CF, OF, PF, SF, ZF};
use wire::ir::Image;
use wire::mem::GuestMem;
use wire::thread::Sched;
const CODE: u32 = 0x10000;
const DATA: u32 = 0x20000;
const STACK: u32 = 0x30000;
const FLAGS: u32 = AF | CF | OF | PF | SF | ZF;
fn machine(code: &[u8], ir: bool) -> (Cpu, GuestMem) {
    let mut mem = GuestMem::new().unwrap();
    for a in [CODE, DATA, STACK] {
        mem.commit(a, 0x10000).unwrap();
    }
    mem.write_bytes(CODE, code).unwrap();
    let mut cpu = Cpu::new(CODE, STACK + 0xff00, 0);
    cpu.profile_opcodes = false;
    if ir {
        cpu.install_ir(Image::from_code(CODE, code).unwrap(), &mem)
            .unwrap();
    }
    (cpu, mem)
}
fn run(cpu: &mut Cpu, mem: &mut GuestMem, budget: u64) -> RunOutcome {
    let mut api = ApiTable::new();
    run_api(cpu, mem, budget, &mut api)
}
fn run_api(cpu: &mut Cpu, mem: &mut GuestMem, budget: u64, api: &mut ApiTable) -> RunOutcome {
    let mut state = ApiState::new(CODE, "C:\\test.exe".into(), 0, 0);
    let path =
        std::env::temp_dir().join(format!("wire-ir-test-{}-{:p}.log", std::process::id(), cpu));
    let mut log = ApiLogger::new(&path).unwrap();
    let mut sched = Sched::new();
    let tid = sched.alloc_tid();
    let h = state.alloc_handle(HandleObj::Thread { tid });
    sched.add_main(tid, h, 0);
    let outcome = cpu.run(mem, api, &mut state, &mut log, &mut sched, budget);
    drop(log);
    std::fs::remove_file(path).unwrap();
    outcome
}

#[test]
fn separate_module_ir_rebases_relative_branches_and_checks_changes_at_every_budget() {
    const MODULE: u32 = 0x50000000;
    const PREFERRED: u32 = 0x10000000;
    let mut code = vec![0xb8, 3, 0, 0, 0, 0xb9, 2, 0, 0, 0, 0xba, 5, 0, 0, 0, 0xe8];
    code.extend_from_slice(&MODULE.wrapping_sub(CODE + 20).to_le_bytes());
    code.extend([0x49, 0x75, 0xf8, 0xcc]);
    let module = [0x83, 0xc0, 7, 0xeb, 2, 0x90, 0x90, 0x83, 0xea, 1, 0xc3];
    for budget in 1..35 {
        for changed in [false, true] {
            let setup = |ir| {
                let (mut cpu, mut mem) = machine(&code, ir);
                mem.commit(MODULE, 0x1000).unwrap();
                mem.virtual_protect(MODULE, 0x1000, wire::mem::PAGE_EXECUTE_READWRITE)
                    .unwrap();
                mem.write_bytes(MODULE, &module).unwrap();
                if ir {
                    let image = Image::from_code(PREFERRED, &module)
                        .unwrap()
                        .at_base(MODULE)
                        .unwrap();
                    cpu.add_module_ir(image, &mem).unwrap();
                }
                (cpu, mem)
            };
            let (mut raw, mut raw_mem) = setup(false);
            let (mut ir, mut ir_mem) = setup(true);
            let a = run(&mut raw, &mut raw_mem, budget);
            let b = run(&mut ir, &mut ir_mem, budget);
            assert_eq!(std::mem::discriminant(&a), std::mem::discriminant(&b));
            if let (RunOutcome::Crashed(a), RunOutcome::Crashed(b)) = (&a, &b) {
                assert_eq!(a, b);
            }
            assert_eq!(raw.regs, ir.regs);
            assert_eq!(raw.eip, ir.eip);
            assert_eq!(raw.effective_eflags(), ir.effective_eflags());
            if changed {
                raw_mem.write_u8(MODULE + 2, 9).unwrap();
                ir_mem.write_u8(MODULE + 2, 9).unwrap();
                let a = run(&mut raw, &mut raw_mem, 50);
                let b = run(&mut ir, &mut ir_mem, 50);
                assert_eq!(std::mem::discriminant(&a), std::mem::discriminant(&b));
                if let (RunOutcome::Crashed(a), RunOutcome::Crashed(b)) = (&a, &b) {
                    assert_eq!(a, b);
                }
                assert_eq!(raw.regs, ir.regs);
                assert_eq!(raw.eip, ir.eip);
                assert_eq!(raw.effective_eflags(), ir.effective_eflags());
            }
        }
    }
    assert!(Image::from_code(PREFERRED, &module)
        .unwrap()
        .at_base(u32::MAX - 3)
        .is_err());
    let (mut cpu, mut mem) = machine(&code, true);
    mem.commit(MODULE, 0x1000).unwrap();
    mem.virtual_protect(MODULE, 0x1000, wire::mem::PAGE_EXECUTE_READWRITE)
        .unwrap();
    mem.write_bytes(MODULE, &module).unwrap();
    cpu.add_module_ir(
        Image::from_code(PREFERRED, &module)
            .unwrap()
            .at_base(MODULE)
            .unwrap(),
        &mem,
    )
    .unwrap();
    let outcome = run(&mut cpu, &mut mem, 50);
    assert!(
        matches!(outcome, RunOutcome::Crashed(ref e) if e.contains("breakpoint")),
        "{} at {:#x}",
        outcome.to_string_for_test(),
        cpu.eip
    );
    assert_eq!((cpu.regs[0], cpu.regs[1], cpu.regs[2]), (17, 0, 3));
    assert!(cpu.ir_steps >= 15, "both caches execute actual IR");
}

/// Minimal PE32 with one executable section and one kernel32 import. All
/// bytes are authored here; the tests do not require a cross-compiler/game.
fn import_pe(code: &[u8]) -> Vec<u8> {
    let mut exe = vec![0; 0x600];
    fn w16(b: &mut [u8], o: usize, v: u16) {
        b[o..o + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn w32(b: &mut [u8], o: usize, v: u32) {
        b[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }
    exe[..2].copy_from_slice(b"MZ");
    w32(&mut exe, 0x3c, 0x80);
    exe[0x80..0x84].copy_from_slice(b"PE\0\0");
    w16(&mut exe, 0x84, 0x14c);
    w16(&mut exe, 0x86, 2);
    w16(&mut exe, 0x94, 0xe0);
    let opt = 0x98;
    w16(&mut exe, opt, 0x10b);
    w32(&mut exe, opt + 16, 0x1000);
    w32(&mut exe, opt + 28, CODE - 0x1000);
    w32(&mut exe, opt + 32, 0x1000);
    w32(&mut exe, opt + 36, 0x200);
    w32(&mut exe, opt + 56, 0x20000);
    w32(&mut exe, opt + 60, 0x200);
    w32(&mut exe, opt + 92, 16);
    w32(&mut exe, opt + 104, 0x11000);
    w32(&mut exe, opt + 108, 40);
    for (section, va, raw, flags) in [
        (0x178, 0x1000, 0x200, 0x6000_0020),
        (0x1a0, 0x11000, 0x400, 0x4000_0040),
    ] {
        w32(&mut exe, section + 8, 0x200);
        w32(&mut exe, section + 12, va);
        w32(&mut exe, section + 16, 0x200);
        w32(&mut exe, section + 20, raw);
        w32(&mut exe, section + 36, flags);
    }
    exe[0x200..0x200 + code.len()].copy_from_slice(code);
    w32(&mut exe, 0x400, 0x11050); // ILT
    w32(&mut exe, 0x40c, 0x11040); // DLL name
    w32(&mut exe, 0x410, 0x11080); // IAT
    exe[0x440..0x44d].copy_from_slice(b"kernel32.dll\0");
    w32(&mut exe, 0x450, 0x11060);
    exe[0x462..0x476].copy_from_slice(b"GetCurrentProcessId\0");
    w32(&mut exe, 0x480, 0x11060);
    exe
}

#[test]
fn native_import_calls_dispatch_and_iat_hooks_keep_the_actual_target() {
    let iat = DATA + 0x80;
    let mut code = vec![0xff, 0x15];
    code.extend(iat.to_le_bytes());
    code.extend([0x89, 0xc3, 0xcc]);
    let exe = import_pe(&code);
    let bytes = Image::from_pe(&exe).unwrap().encode();
    assert_eq!(Image::parse(&bytes, &exe).unwrap().native_call_count(), 1);
    for hooked in [false, true] {
        let mut results = Vec::new();
        for use_ir in [false, true] {
            let (mut cpu, mut mem) = machine(&code, false);
            let mut api = ApiTable::new();
            let stub = api.register("kernel32.dll", Some("GetCurrentProcessId"), None);
            // Place a guest hook before binding so a subsequent IAT write
            // leaves the code page unchanged.
            mem.write_bytes(CODE + 0x80, &[0xb8, 0x34, 0x12, 0, 0, 0xc3])
                .unwrap();
            mem.write_u32(iat, stub).unwrap();
            if use_ir {
                cpu.install_ir(Image::parse(&bytes, &exe).unwrap(), &mem)
                    .unwrap();
            }
            if hooked {
                mem.write_u32(iat, CODE + 0x80).unwrap();
            }
            let esp = cpu.esp();
            let outcome = run_api(&mut cpu, &mut mem, 100, &mut api);
            assert!(matches!(outcome, RunOutcome::Crashed(ref s) if s.contains("breakpoint")));
            assert_eq!(cpu.esp(), esp);
            if hooked {
                assert_eq!(cpu.regs[3], 0x1234);
            }
            assert_eq!(cpu.ir_native_calls, u64::from(use_ir && !hooked));
            results.push((cpu.regs, cpu.eflags, cpu.eip, cpu.instr_count));
        }
        assert_eq!(results[0], results[1]);
    }
}
#[test]
fn flags_are_elided_only_before_definite_overwrite_in_a_straight_line() {
    let code = [
        0x83, 0xc0, 1, 0x89, 0xc2, 0x83, 0xc1, 1, 0x39, 0xd9, 0x75, 0xf3,
    ];
    let image = Image::from_code(CODE, &code).unwrap();
    assert_eq!(image.records[0].eager_flags(), 0);
    assert_eq!(image.records[2].eager_flags(), 0);
    assert_eq!(image.records[3].eager_flags(), FLAGS); // branch exit keeps all
                                                       // ADC reads CF from the ADD even when ADC flags are overwritten later.
    let image = Image::from_code(CODE, &[0x83, 0xc0, 1, 0x83, 0xd1, 0, 0x31, 0xd2, 0xcc]).unwrap();
    assert_eq!(image.records[0].eager_flags(), CF);
    // INC preserves CF; conservative analysis keeps it from the earlier ADD.
    let image = Image::from_code(CODE, &[0x83, 0xc0, 1, 0x41, 0x31, 0xd2, 0xcc]).unwrap();
    assert_eq!(image.records[0].eager_flags(), CF);
    for boundary in [
        &[0x8b, 0x13][..],
        &[0x50][..],
        &[0x9f][..],
        &[0x9c][..],
        &[0x75, 0][..],
        &[0xe8, 0, 0, 0, 0][..],
        &[0xff, 0xe1][..],
    ] {
        let mut code = vec![0x83, 0xc0, 1];
        code.extend(boundary);
        code.extend([0x31, 0xc9]);
        let image = Image::from_code(CODE, &code).unwrap();
        assert_eq!(
            image.records[0].eager_flags(),
            FLAGS,
            "boundary {boundary:x?}"
        );
    }
    // A faultable arithmetic store retains its own flags, too.
    let image = Image::from_code(CODE, &[0x83, 0x03, 1, 0x31, 0xc9]).unwrap();
    assert_eq!(image.records[0].eager_flags(), FLAGS);
}

#[test]
fn observer_paths_preserve_budget_fault_state_and_execution_counts() {
    // Compact arithmetic/branches/store, cold byte MOV and memory arithmetic,
    // raw shift and INT3. Profiling counts attempts, including a faulting op.
    let code = [
        0x83, 0xc0, 1, 0x88, 0xc4, 0x03, 0x03, 0xc1, 0xe0, 1, 0x85, 0xc0, 0x74, 2, 0xeb, 0, 0x89,
        0x13, 0xcc,
    ];
    for readonly in [false, true] {
        for budget in 0..=12 {
            let mut states = Vec::new();
            for use_ir in [false, true] {
                for (trace, stats) in [(false, false), (true, false), (false, true), (true, true)] {
                    let (mut cpu, mut mem) = machine(&code, use_ir);
                    cpu.trace_enabled = trace;
                    cpu.profile_opcodes = stats;
                    cpu.regs[0] = u32::MAX;
                    cpu.regs[3] = DATA;
                    cpu.regs[2] = 0x12345678;
                    mem.write_u32(DATA, 0xfedcba98).unwrap();
                    if readonly {
                        mem.virtual_protect(DATA, wire::mem::GUEST_PAGE, wire::mem::PAGE_READONLY)
                            .unwrap();
                    }
                    let outcome = run(&mut cpu, &mut mem, budget);
                    let attempted = cpu
                        .op_stats
                        .iter()
                        .filter(|(op, _)| **op < 0x100)
                        .map(|(_, n)| n)
                        .sum::<u64>();
                    let classified = cpu
                        .raw_op_stats
                        .iter()
                        .filter(|(op, _)| **op < 0x100)
                        .map(|(_, n)| n)
                        .sum::<u64>()
                        + cpu.compact_op_stats.iter().sum::<u64>()
                        + cpu.cold_ir_profile().iter().map(|(_, n)| n).sum::<u64>();
                    assert_eq!(classified, attempted);
                    if use_ir {
                        assert_eq!(
                            cpu.ir_steps + cpu.ir_fallbacks,
                            cpu.instr_count + u64::from(matches!(outcome, RunOutcome::Crashed(_)))
                        );
                    }
                    if stats {
                        assert_eq!(
                            attempted,
                            cpu.instr_count + u64::from(matches!(outcome, RunOutcome::Crashed(_)))
                        );
                    } else {
                        assert_eq!(attempted, 0);
                    }
                    assert_eq!(cpu.trace_ring.is_empty(), !trace || budget == 0);
                    states.push((
                        match outcome {
                            RunOutcome::InstructionLimit => "limit".to_owned(),
                            RunOutcome::Crashed(error) => error,
                            _ => panic!("unexpected execution outcome"),
                        },
                        cpu.regs,
                        cpu.eflags,
                        cpu.eip,
                        cpu.instr_count,
                        mem.read_u32(DATA).unwrap(),
                    ));
                }
            }
            assert!(
                states.iter().all(|s| s == &states[0]),
                "readonly={readonly} budget={budget}: {states:?}"
            );
        }
    }
}

#[test]
fn compact_narrow_arithmetic_keeps_high_bytes_carry_and_every_budget() {
    for width in [8, 16] {
        for sub in 0u8..=10 {
            for immediate in [false, true] {
                if immediate && sub >= 9 {
                    continue;
                }
                // AH or AX, with the source overlapping the same full GPR.
                let dst = if width == 8 { 4 } else { 0 };
                let mut code = vec![0x83, 0xc2, 1]; // prior, potentially lazy ADD
                if width == 16 {
                    code.push(0x66);
                }
                if sub >= 9 {
                    code.extend([
                        if width == 8 { 0xfe } else { 0xff },
                        0xc0 | ((sub - 9) << 3) | dst,
                    ]);
                } else if immediate {
                    code.extend([
                        if sub == 8 {
                            if width == 8 {
                                0xf6
                            } else {
                                0xf7
                            }
                        } else if width == 8 {
                            0x80
                        } else {
                            0x81
                        },
                        0xc0 | (if sub == 8 { 0 } else { sub << 3 }) | dst,
                    ]);
                    code.push(0xff);
                    if width == 16 {
                        code.push(0xff);
                    }
                } else {
                    code.extend([
                        if sub == 8 {
                            if width == 8 {
                                0x84
                            } else {
                                0x85
                            }
                        } else {
                            (sub << 3) | u8::from(width == 16)
                        },
                        0xc0 | dst,
                    ]);
                }
                code.extend([0x0f, 0x95, 0xc1, 0xcc]);
                for value in [0, 0x7f7f, 0x8080, 0xffff, 0x7fff] {
                    for carry in [false, true] {
                        for budget in 0..=4 {
                            let mut states = Vec::new();
                            for use_ir in [false, true] {
                                let (mut cpu, mut mem) = machine(&code, use_ir);
                                cpu.regs[0] = 0xabcd0000 | value;
                                cpu.regs[1] = 0xfeedbeef;
                                cpu.regs[2] = if carry { u32::MAX } else { 0 };
                                let outcome = run(&mut cpu, &mut mem, budget);
                                states.push((
                                    outcome.to_string_for_test(),
                                    cpu.regs,
                                    cpu.eflags,
                                    cpu.eip,
                                    cpu.instr_count,
                                ));
                            }
                            assert_eq!(states[0], states[1], "I{width} sub={sub} immediate={immediate} value={value:x} carry={carry} budget={budget}");
                        }
                    }
                }
                let (mut cpu, mut mem) = machine(&code, true);
                cpu.profile_opcodes = true;
                run(&mut cpu, &mut mem, 2);
                assert_eq!(cpu.compact_op_stats[68..108].iter().sum::<u64>(), 1);
                assert!(cpu.cold_ir_profile().is_empty());
            }
        }
    }
}

#[test]
fn compact_subregister_moves_and_extensions_keep_aliases_and_budgets() {
    let code = [
        0xb4, 0x80, 0x88, 0xe0, // MOV AH,80; MOV AL,AH
        0x88, 0xcc, // MOV AH,CL
        0x66, 0xb9, 0x00, 0x80, 0x66, 0x89, 0xc8, // MOV CX,8000; MOV AX,CX
        0x0f, 0xb6, 0xc4, 0x0f, 0xbe, 0xe8, // MOVZX EAX,AH; MOVSX EBP,AL
        0x0f, 0xb7, 0xd1, 0x0f, 0xbf, 0xc9, 0xcc, // MOVZX EDX,CX; MOVSX ECX,CX
    ];
    for budget in 0..=10 {
        let mut states = Vec::new();
        for use_ir in [false, true] {
            let (mut cpu, mut mem) = machine(&code, use_ir);
            cpu.regs[0] = 0xfedcba98;
            cpu.regs[1] = 0x123456ff;
            cpu.eflags = 0x202 | FLAGS;
            let outcome = run(&mut cpu, &mut mem, budget);
            states.push((
                outcome.to_string_for_test(),
                cpu.regs,
                cpu.eflags,
                cpu.eip,
                cpu.instr_count,
            ));
        }
        assert_eq!(states[0], states[1], "budget={budget}");
    }
    let (mut cpu, mut mem) = machine(&code, true);
    cpu.profile_opcodes = true;
    run(&mut cpu, &mut mem, 9);
    assert_eq!(cpu.compact_op_stats[108..116].iter().sum::<u64>(), 9);
    assert!(cpu.cold_ir_profile().is_empty());
}

#[test]
fn raw_prefix_fetch_keeps_exact_execute_fault_pc_and_one_shot_guard() {
    use wire::mem::{GUEST_PAGE, PAGE_EXECUTE, PAGE_GUARD, PAGE_NOACCESS, PAGE_READONLY};
    for bytes_before_boundary in [1, 3] {
        let start = CODE + GUEST_PAGE - bytes_before_boundary;
        for protection in [PAGE_NOACCESS, PAGE_READONLY, PAGE_EXECUTE | PAGE_GUARD] {
            let mut states = Vec::new();
            for use_ir in [false, true] {
                let (mut cpu, mut mem) = machine(&[], false);
                let code = [0x66, 0xb8, 0x34, 0x12];
                mem.write_bytes(start, &code).unwrap();
                mem.virtual_protect(CODE, GUEST_PAGE, PAGE_EXECUTE).unwrap();
                mem.virtual_protect(CODE + GUEST_PAGE, GUEST_PAGE, protection)
                    .unwrap();
                if use_ir {
                    cpu.install_ir(Image::from_code(start, &code).unwrap(), &mem)
                        .unwrap();
                }
                cpu.eip = start;
                cpu.regs[0] = 0xfeedbeef;
                let outcome = run(&mut cpu, &mut mem, 1);
                assert!(
                    matches!(outcome, RunOutcome::Crashed(ref s) if s.contains("execute fault"))
                );
                assert_eq!(cpu.instr_count, 0);
                assert_eq!(cpu.regs[0], 0xfeedbeef);
                assert_eq!(
                    cpu.eip,
                    start + if bytes_before_boundary == 1 { 1 } else { 2 }
                );
                assert_eq!(
                    mem.virtual_query(CODE + GUEST_PAGE).unwrap().protect,
                    protection & !PAGE_GUARD
                );
                states.push((outcome.to_string_for_test(), cpu.regs, cpu.eflags, cpu.eip));
            }
            assert_eq!(states[0], states[1]);
        }
    }
    // Execute-only fetch still handles multiple prefixes and a successful
    // width-specific immediate; no data-read permission is required.
    let code = [0x2e, 0x66, 0x64, 0xb8, 0x34, 0x12];
    let (mut cpu, mut mem) = machine(&code, false);
    mem.virtual_protect(CODE, GUEST_PAGE, PAGE_EXECUTE).unwrap();
    cpu.regs[0] = 0xfeedbeef;
    assert!(matches!(
        run(&mut cpu, &mut mem, 1),
        RunOutcome::InstructionLimit
    ));
    assert_eq!(cpu.regs[0], 0xfeed1234);
    assert_eq!(cpu.eip, CODE + code.len() as u32);
}

#[test]
fn compact_memory_arithmetic_keeps_fault_flags_addresses_and_code_invalidation() {
    use wire::mem::{GUEST_PAGE, PAGE_GUARD, PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE};
    for sub in 0u8..=8 {
        for memory_destination in [false, true] {
            for address_form in 0..3 {
                let mut code = vec![0x83, 0xc2, 1];
                if address_form != 0 {
                    code.push(if address_form == 1 { 0x64 } else { 0x65 });
                }
                let op = if sub == 8 {
                    0x85
                } else {
                    (sub << 3) | if memory_destination { 1 } else { 3 }
                };
                code.push(op);
                if address_form == 0 {
                    code.push(5); // [absolute], EAX
                    code.extend(DATA.to_le_bytes());
                } else if address_form == 1 {
                    code.extend([0x44, 0x8b, 0x10]); // [FS:EBX+ECX*4+16]
                } else {
                    code.extend([0x45, 0xfc]); // [GS:EBP-4]
                }
                code.extend([0x89, 0xc6, 0xcc]);
                for protection in [
                    PAGE_READWRITE,
                    PAGE_READONLY,
                    PAGE_NOACCESS,
                    PAGE_READWRITE | PAGE_GUARD,
                ] {
                    for budget in 0..=4 {
                        let mut states = Vec::new();
                        for use_ir in [false, true] {
                            let (mut cpu, mut mem) = machine(&code, use_ir);
                            cpu.regs[0] = 0x80000001;
                            cpu.regs[2] = u32::MAX;
                            cpu.regs[3] = u32::MAX - 31;
                            cpu.regs[1] = 4;
                            cpu.fs_base = DATA;
                            cpu.regs[5] = 0x14;
                            cpu.gs_base = DATA - 0x10;
                            mem.write_u32(DATA, 0xffffffff).unwrap();
                            mem.virtual_protect(DATA, GUEST_PAGE, protection).unwrap();
                            let outcome = run(&mut cpu, &mut mem, budget);
                            let remaining_protection = mem.virtual_query(DATA).unwrap().protect;
                            mem.virtual_protect(DATA, GUEST_PAGE, PAGE_READWRITE)
                                .unwrap();
                            states.push((
                                outcome.to_string_for_test(),
                                cpu.regs,
                                cpu.eflags,
                                cpu.eip,
                                cpu.instr_count,
                                remaining_protection,
                                mem.read_u32(DATA).unwrap(),
                            ));
                        }
                        assert_eq!(states[0], states[1], "sub={sub} mem_dst={memory_destination} addr={address_form} protect={protection:x} budget={budget}");
                    }
                }
            }
        }
    }
    for memory_destination in [false, true] {
        for protection in [PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE | PAGE_GUARD] {
            let address = DATA + GUEST_PAGE - 2;
            let mut code = vec![0x83, 0xc2, 1, if memory_destination { 1 } else { 3 }, 5];
            code.extend(address.to_le_bytes());
            code.push(0xcc);
            let mut states = Vec::new();
            for use_ir in [false, true] {
                let (mut cpu, mut mem) = machine(&code, use_ir);
                cpu.regs[0] = 0x80000001;
                cpu.regs[2] = u32::MAX;
                mem.write_u32(address, 0xffffffff).unwrap();
                mem.virtual_protect(DATA + GUEST_PAGE, GUEST_PAGE, protection)
                    .unwrap();
                let outcome = run(&mut cpu, &mut mem, 10);
                let remaining_protection = mem.virtual_query(DATA + GUEST_PAGE).unwrap().protect;
                mem.virtual_protect(DATA + GUEST_PAGE, GUEST_PAGE, PAGE_READWRITE)
                    .unwrap();
                states.push((
                    outcome.to_string_for_test(),
                    cpu.regs,
                    cpu.eflags,
                    cpu.eip,
                    cpu.instr_count,
                    remaining_protection,
                    mem.read_u32(address).unwrap(),
                ));
            }
            assert_eq!(
                states[0], states[1],
                "cross-page mem_dst={memory_destination} protect={protection:x}"
            );
        }
    }
    // RMW on watched code must invalidate an already linked successor.
    let mut code = vec![0x01, 0x05]; // ADD [next instruction],EAX
    code.extend((CODE + 6).to_le_bytes());
    code.extend([0x41, 0x90, 0x90, 0x90, 0xcc]);
    let mut states = Vec::new();
    for use_ir in [false, true] {
        let (mut cpu, mut mem) = machine(&code, use_ir);
        cpu.regs[0] = 8; // INC ECX -> DEC ECX
        cpu.regs[1] = 3;
        let outcome = run(&mut cpu, &mut mem, 2);
        states.push((
            outcome.to_string_for_test(),
            cpu.regs,
            cpu.eflags,
            cpu.eip,
            cpu.instr_count,
        ));
        assert_eq!(cpu.regs[1], 2);
        if use_ir {
            assert_eq!((cpu.ir_steps, cpu.ir_fallbacks), (1, 1));
        }
    }
    assert_eq!(states[0], states[1]);
}

#[test]
fn runtime_protection_changes_invalidate_ir_and_execute_only_code_still_runs() {
    use wire::mem::{GUEST_PAGE, PAGE_EXECUTE, PAGE_EXECUTE_READWRITE, PAGE_READWRITE};
    let code = [0xb8, 42, 0, 0, 0, 0xcc];
    for use_ir in [false, true] {
        let (mut cpu, mut mem) = machine(&code, use_ir);
        assert_eq!(
            mem.virtual_protect(CODE, GUEST_PAGE, PAGE_READWRITE),
            Ok(PAGE_EXECUTE_READWRITE)
        );
        let outcome = run(&mut cpu, &mut mem, 1);
        assert!(matches!(outcome, RunOutcome::Crashed(ref e) if e.contains("execute fault")));
        assert_eq!(cpu.instr_count, 0);
        assert_eq!(cpu.eip, CODE);
        assert_eq!(
            mem.virtual_protect(CODE, GUEST_PAGE, PAGE_EXECUTE),
            Ok(PAGE_READWRITE)
        );
        assert!(mem.read_u8(CODE).is_err());
        // Binding inspects execute permissions, rather than treating code as a data read.
        if use_ir {
            assert!(
                cpu.install_ir(Image::from_code(CODE, &code).unwrap(), &mem)
                    .unwrap()
                    > 0
            );
        }
        assert!(matches!(
            run(&mut cpu, &mut mem, 1),
            RunOutcome::InstructionLimit
        ));
        assert_eq!(cpu.regs[0], 42);
        assert_eq!(cpu.instr_count, 1);
    }
}

#[test]
fn readonly_data_fault_and_one_shot_guard_keep_raw_ir_state_and_retirement_equal() {
    use wire::mem::{GUEST_PAGE, PAGE_EXECUTE_READWRITE, PAGE_GUARD, PAGE_READONLY};
    let code = [0x83, 0xc0, 1, 0x89, 0x03, 0xcc];
    let mut results = Vec::new();
    for use_ir in [false, true] {
        let (mut cpu, mut mem) = machine(&code, use_ir);
        cpu.regs[0] = u32::MAX;
        cpu.regs[3] = DATA;
        mem.write_u32(DATA, 0xdeadbeef).unwrap();
        mem.virtual_protect(DATA, GUEST_PAGE, PAGE_READONLY)
            .unwrap();
        let outcome = run(&mut cpu, &mut mem, 100);
        assert!(matches!(outcome, RunOutcome::Crashed(ref e) if e.contains("write fault")));
        assert_eq!(cpu.instr_count, 1);
        assert_eq!(mem.read_u32(DATA).unwrap(), 0xdeadbeef);
        results.push((cpu.regs, cpu.eflags, cpu.eip));
    }
    assert_eq!(results[0], results[1]);
    for use_ir in [false, true] {
        let (mut cpu, mut mem) = machine(&code, false);
        let protection = PAGE_EXECUTE_READWRITE | PAGE_GUARD;
        mem.virtual_protect(CODE, GUEST_PAGE, protection).unwrap();
        if use_ir {
            cpu.install_ir(Image::from_code(CODE, &code).unwrap(), &mem)
                .unwrap();
        }
        assert_eq!(mem.virtual_query(CODE).unwrap().protect, protection); // IR binding must not consume a guard
        assert!(
            matches!(run(&mut cpu, &mut mem, 1), RunOutcome::Crashed(ref e) if e.contains("guard execute fault"))
        );
        assert_eq!(cpu.instr_count, 0);
        assert_eq!(cpu.eip, CODE);
        assert_eq!(
            mem.virtual_query(CODE).unwrap().protect,
            PAGE_EXECUTE_READWRITE
        );
        assert!(matches!(
            run(&mut cpu, &mut mem, 1),
            RunOutcome::InstructionLimit
        ));
        assert_eq!(cpu.instr_count, 1);
        assert_eq!(cpu.regs[0], 1);
    }
}

#[test]
fn address16_absolute_mov_zero_extends_offset_before_adding_fs_base() {
    let code = [
        0x64, 0x67, 0xa1, 0x10, 0, 0x66, 0x64, 0x67, 0xa3, 0x20, 0, 0x64, 0x67, 0x89, 0x1e, 0x30,
        0, 0xcc,
    ];
    let mut results = Vec::new();
    for use_ir in [false, true] {
        let (mut cpu, mut mem) = machine(&code, use_ir);
        cpu.fs_base = DATA;
        cpu.regs[3] = 0x13579bdf;
        mem.write_u32(DATA + 0x10, 0xfedc9876).unwrap();
        mem.write_u32(DATA + 0x20, 0x12345678).unwrap();
        assert!(matches!(
            run(&mut cpu, &mut mem, 3),
            RunOutcome::InstructionLimit
        ));
        assert_eq!(cpu.regs[0], 0xfedc9876);
        assert_eq!(mem.read_u32(DATA + 0x20).unwrap(), 0x12349876);
        assert_eq!(mem.read_u32(DATA + 0x30).unwrap(), 0x13579bdf);
        results.push((cpu.regs, cpu.eip, cpu.eflags));
    }
    assert_eq!(results[0], results[1]);
}
#[test]
fn lazy_flags_are_restored_at_instruction_limits_and_before_faults() {
    let code = [0x83, 0xc0, 1, 0x31, 0xc9, 0xcc];
    for limit in [1, 2, 3] {
        let (mut cpu, mut mem) = machine(&code, false);
        let (mut ir, mut imem) = machine(&code, true);
        cpu.regs[0] = u32::MAX;
        ir.regs[0] = u32::MAX;
        let a = run(&mut cpu, &mut mem, limit);
        let b = run(&mut ir, &mut imem, limit);
        assert_eq!(a.to_string_for_test(), b.to_string_for_test());
        assert_eq!(cpu.regs, ir.regs);
        assert_eq!(cpu.eflags, ir.eflags);
        assert_eq!(cpu.save_ctx().eflags, ir.save_ctx().eflags);
        assert!(ir.ir_steps > 0);
    }
    for code in [
        &[0x83, 0xc0, 1, 0x8b, 0x13, 0x31, 0xc9][..],
        &[0x83, 0xc0, 1, 0x83, 0x03, 1, 0x31, 0xc9][..],
    ] {
        let (mut cpu, mut mem) = machine(code, false);
        let (mut ir, mut imem) = machine(code, true);
        cpu.regs[0] = 0x7fffffff;
        ir.regs[0] = 0x7fffffff;
        cpu.regs[3] = 0x700000;
        ir.regs[3] = 0x700000;
        let a = run(&mut cpu, &mut mem, 10);
        let b = run(&mut ir, &mut imem, 10);
        assert_eq!(a.to_string_for_test(), b.to_string_for_test());
        assert_eq!(cpu.eflags, ir.eflags);
        assert_eq!(cpu.eip, ir.eip);
    }
}
trait Outcome {
    fn to_string_for_test(&self) -> String;
}

#[test]
fn scalar_runs_keep_all_flags_branches_and_every_interior_entry() {
    for compare in [0x39, 0x85] {
        // CMP / TEST EAX,EDX
        for cc in 0..16 {
            let code = [
                0x89,
                0xc2, // MOV EDX,EAX
                0x83,
                0xc0,
                1,    // ADD EAX,1
                0x41, // INC ECX (preserve CF)
                0x88,
                0xc7, // MOV BH,AL
                0x0f,
                0xb6,
                0xf7, // MOVZX ESI,BH
                0x8d,
                0x7c,
                0x4e,
                3, // LEA EDI,[ESI+ECX*2+3]
                compare,
                0xd0,
                0x70 + cc,
                5,
                0xbb,
                7,
                0,
                0,
                0, // MOV EBX,7
                0xcc,
            ];
            for initial in [0, 0x7fff_ffff, u32::MAX] {
                for entry in [0, 2, 5, 6, 8, 11, 15] {
                    for budget in 0..=11 {
                        let mut states = Vec::new();
                        for use_ir in [false, true] {
                            let (mut cpu, mut mem) = machine(&code, use_ir);
                            cpu.eip += entry;
                            cpu.regs[0] = initial;
                            cpu.regs[1] = initial;
                            cpu.regs[2] = 0x8000_0000;
                            cpu.eflags = 2 | FLAGS;
                            let outcome = run(&mut cpu, &mut mem, budget);
                            states.push((
                                outcome.to_string_for_test(),
                                cpu.regs,
                                cpu.eflags,
                                cpu.eip,
                                cpu.instr_count,
                            ));
                            if use_ir && entry == 0 && budget >= 8 {
                                assert!(cpu.ir_scalar_steps >= 8);
                                assert!(cpu.ir_scalar_runs > 0);
                            }
                        }
                        assert_eq!(states[0], states[1],
                            "compare {compare:x} cc {cc} initial {initial:x} entry {entry} budget {budget}");
                    }
                }
            }
            for (trace, stats) in [(true, false), (false, true)] {
                let (mut cpu, mut mem) = machine(&code, true);
                cpu.trace_enabled = trace;
                cpu.profile_opcodes = stats;
                run(&mut cpu, &mut mem, 8);
                assert_eq!(cpu.ir_scalar_runs, 0);
                assert_eq!(cpu.ir_steps, 8);
                if trace {
                    assert_eq!(cpu.trace_ring.len(), 8);
                }
                if stats {
                    assert_eq!(cpu.compact_op_stats.iter().sum::<u64>(), 8);
                }
            }
        }
    }
}

#[test]
fn scalar_runs_stop_before_guarded_code_and_a_following_memory_fault() {
    use wire::mem::{GUEST_PAGE, PAGE_EXECUTE_READWRITE, PAGE_GUARD};
    for guard in [false, true] {
        let entry = if guard { GUEST_PAGE - 4 } else { 0 };
        let mut code = vec![0; entry as usize];
        code.extend([0x40, 0x41, 0x42, 0x43, 0x8b, 0x06, 0xcc]);
        let mut states = Vec::new();
        for use_ir in [false, true] {
            let (mut cpu, mut mem) = machine(&code, false);
            if guard {
                mem.virtual_protect(
                    CODE + GUEST_PAGE,
                    GUEST_PAGE,
                    PAGE_EXECUTE_READWRITE | PAGE_GUARD,
                )
                .unwrap();
            }
            if use_ir {
                cpu.install_ir(Image::from_code(CODE, &code).unwrap(), &mem)
                    .unwrap();
            }
            cpu.eip += entry;
            cpu.regs[0] = u32::MAX;
            cpu.regs[6] = 0x700000; // uncommitted MOV source
            let outcome = run(&mut cpu, &mut mem, 10);
            assert!(matches!(outcome, RunOutcome::Crashed(_)));
            assert_eq!(cpu.instr_count, 4);
            if use_ir {
                assert_eq!(cpu.ir_scalar_steps, 4);
            }
            states.push((
                outcome.to_string_for_test(),
                cpu.regs,
                cpu.eflags,
                cpu.eip,
                cpu.instr_count,
                mem.virtual_query(CODE + GUEST_PAGE).unwrap().protect,
            ));
        }
        assert_eq!(states[0], states[1], "guard {guard}");
    }
}

#[test]
fn scalar_run_interior_patches_invalidate_the_entire_run() {
    let code = [0x40, 0x41, 0x42, 0x43, 0x45, 0x46, 0x47, 0xcc];
    for before_bind in [false, true] {
        let mut states = Vec::new();
        for use_ir in [false, true] {
            let (mut cpu, mut mem) = machine(&code, false);
            if before_bind {
                mem.write_u8(CODE + 4, 0x4d).unwrap();
            }
            if use_ir {
                cpu.install_ir(Image::from_code(CODE, &code).unwrap(), &mem)
                    .unwrap();
            }
            if !before_bind {
                mem.write_u8(CODE + 4, 0x4d).unwrap();
            }
            let outcome = run(&mut cpu, &mut mem, 7);
            assert_eq!(cpu.regs[5], u32::MAX); // patched DEC EBP
            states.push((
                outcome.to_string_for_test(),
                cpu.regs,
                cpu.eflags,
                cpu.eip,
                cpu.instr_count,
            ));
            if use_ir && !before_bind {
                assert_eq!(cpu.ir_scalar_steps, 0);
            }
        }
        assert_eq!(states[0], states[1], "before bind {before_bind}");
    }
}

#[test]
fn typed_segments_preserve_every_budget_and_fault_retirement_boundary() {
    let code = [
        0xb9, 3, 0, 0, 0, // MOV ECX,3
        0x89, 0x03, 0x83, 0xc0, 1, 0x0f, 0xb6, 0x13, 0x49, 0x75, 0xf5, 0xd1,
        0xc0, // ROL EAX,1: raw fallback boundary
        0x8b, 0x06, // MOV EAX,[ESI]: fault after successful instructions
        0xcc,
    ];
    for budget in 0..=20 {
        let mut states = Vec::new();
        for use_ir in [false, true] {
            let (mut cpu, mut mem) = machine(&code, use_ir);
            cpu.regs[0] = 0xffff_fffe;
            cpu.regs[3] = DATA;
            cpu.regs[6] = 0x700000; // uncommitted
            let outcome = run(&mut cpu, &mut mem, budget);
            states.push((
                outcome.to_string_for_test(),
                cpu.regs,
                cpu.eflags,
                cpu.eip,
                cpu.instr_count,
                mem.read_u32(DATA).unwrap(),
            ));
        }
        assert_eq!(states[0], states[1], "budget {budget}");
    }
}

#[test]
fn typed_segment_stops_at_the_exact_cooperative_timeslice() {
    let mut code = vec![0x83, 0xc0, 1, 0x83, 0xc0, 1, 0xb8, 0xad, 0xde, 0, 0];
    code.resize(0x80, 0x90);
    code.extend([0xb8, 9, 0, 0, 0, 0x83, 0xc0, 1, 0xcc]);
    let mut states = Vec::new();
    for use_ir in [false, true] {
        let (mut cpu, mut mem) = machine(&code, use_ir);
        cpu.regs[0] = 0xffff_fffe;
        cpu.instr_count = 0x10_0000 - 2;
        let mut api = ApiTable::new();
        let mut state = ApiState::new(CODE, "C:\\slice.exe".into(), 0, 0);
        let path =
            std::env::temp_dir().join(format!("wire-ir-slice-{}-{use_ir}.log", std::process::id()));
        let mut log = ApiLogger::new(&path).unwrap();
        let mut sched = Sched::new();
        let tid = sched.alloc_tid();
        let handle = state.alloc_handle(HandleObj::Thread { tid });
        sched.add_main(tid, handle, 0);
        let worker = sched.alloc_tid();
        let handle = state.alloc_handle(HandleObj::Thread { tid: worker });
        sched
            .create_thread(&mut mem, CODE + 0x80, 0, false, worker, handle, 0)
            .unwrap();
        let outcome = cpu.run(
            &mut mem,
            &mut api,
            &mut state,
            &mut log,
            &mut sched,
            0x10_0000 + 2,
        );
        assert!(matches!(outcome, RunOutcome::InstructionLimit));
        assert_eq!(cpu.regs[0], 10); // two instructions in the worker
        assert_eq!(sched.threads[0].ctx.regs[0], 0); // exactly two in main
        assert_eq!(sched.threads[0].ctx.eip, CODE + 6);
        states.push((
            cpu.regs,
            cpu.eflags,
            cpu.eip,
            cpu.instr_count,
            sched.threads[0].ctx.eflags,
        ));
        drop(log);
        std::fs::remove_file(path).unwrap();
    }
    assert_eq!(states[0], states[1]);
}

#[test]
fn callback_chains_keep_each_budget_and_nonlocal_stack_boundary() {
    for transfer in [false, true] {
        let mut code = vec![0xb9, 3, 0, 0, 0, 0x83, 0xc0, 1, 0x49, 0x75, 0xfa];
        if transfer {
            // Crossing the saved callback ceiling must stop before the next
            // instruction, even if that instruction would lower ESP again.
            code.push(0xbc);
            code.extend((STACK + 0xff00u32).to_le_bytes());
            code.extend([0x83, 0xec, 16, 0xb8, 99, 0, 0, 0, 0xcc]);
        } else {
            code.extend([0xe8, 3, 0, 0, 0, 0xc2, 4, 0]);
            code.extend([0x50, 0x83, 0xc0, 1, 0x58, 0xc3]); // nested frame
        }
        for budget in 0..=22 {
            let mut states = Vec::new();
            for use_ir in [false, true] {
                let (mut cpu, mut mem) = machine(&code, use_ir);
                cpu.regs[0] = u32::MAX;
                let mut api = ApiTable::new();
                let mut state = ApiState::new(CODE, "C:\\callback.exe".into(), 0, 0);
                let path = std::env::temp_dir().join(format!(
                    "wire-ir-callback-{}-{use_ir}.log",
                    std::process::id()
                ));
                let mut log = ApiLogger::new(&path).unwrap();
                let mut sched = Sched::new();
                let tid = sched.alloc_tid();
                let h = state.alloc_handle(HandleObj::Thread { tid });
                sched.add_main(tid, h, 0);
                let outcome = cpu.call_guest(
                    &mut mem,
                    &mut api,
                    &mut state,
                    &mut log,
                    &mut sched,
                    CODE,
                    &[7],
                    budget,
                );
                let outcome = match outcome {
                    Ok(wire::cpu::GuestCallOutcome::Returned(n)) => format!("returned {n}"),
                    Ok(wire::cpu::GuestCallOutcome::Transferred) => "transferred".into(),
                    Ok(wire::cpu::GuestCallOutcome::Exited(n)) => format!("exited {n}"),
                    Err(e) => e,
                };
                states.push((outcome, cpu.regs, cpu.eflags, cpu.eip, cpu.instr_count));
                drop(log);
                std::fs::remove_file(path).unwrap();
            }
            assert_eq!(states[0], states[1], "transfer {transfer}, budget {budget}");
        }
    }
}

#[test]
fn compact_stack_and_calls_keep_fault_state_and_esp_aliases() {
    let codes: &[&[u8]] = &[
        &[0x54, 0x5b, 0x50, 0x5c, 0xcc], // PUSH ESP, POP EBX, PUSH EAX, POP ESP
        &[0x83, 0xc0, 1, 0x50, 0xcc],    // faulting PUSH must decrement ESP
        &[0x83, 0xc0, 1, 0x58, 0xcc],    // faulting POP must preserve ESP
        &[0x83, 0xc0, 1, 0xe8, 0, 0, 0, 0, 0xcc],
        &[0x83, 0xc0, 1, 0xff, 0xd4, 0xcc], // CALL ESP reads target before PUSH
        &[0x83, 0xc0, 1, 0xc2, 8, 0],       // RET stack-read fault
        &[0x83, 0xc0, 1, 0xc9, 0xcc],       // LEAVE sets ESP before read fault
    ];
    for (case, code) in codes.iter().enumerate() {
        for budget in 0..=8 {
            let mut results = Vec::new();
            for use_ir in [false, true] {
                let (mut cpu, mut mem) = machine(code, use_ir);
                cpu.regs[0] = 0x7fff_ffff;
                cpu.regs[5] = 0x700000;
                if case != 0 {
                    cpu.set_esp(0x700004);
                }
                let outcome = run(&mut cpu, &mut mem, budget);
                results.push((
                    outcome.to_string_for_test(),
                    cpu.regs,
                    cpu.eflags,
                    cpu.eip,
                    cpu.instr_count,
                ));
            }
            assert_eq!(results[0], results[1], "case {case}, budget {budget}");
        }
    }
}

#[test]
fn presentation_limit_waits_for_callback_return_and_restores_flags() {
    let code = [0x83, 0xc0, 1, 0xc3];
    let (mut cpu, mut mem) = machine(&code, true);
    let mut api = ApiTable::new();
    let mut state = ApiState::new(CODE, "C:\\present.exe".into(), 0, 0);
    state.stop_after_presents = Some(8);
    state.d3d9.frames = 9;
    let path = std::env::temp_dir().join(format!("wire-ir-present-{}.log", std::process::id()));
    let mut log = ApiLogger::new(&path).unwrap();
    let mut sched = Sched::new();
    let tid = sched.alloc_tid();
    let h = state.alloc_handle(HandleObj::Thread { tid });
    sched.add_main(tid, h, 0);
    cpu.regs[0] = u32::MAX;
    assert!(matches!(
        cpu.call_guest(
            &mut mem,
            &mut api,
            &mut state,
            &mut log,
            &mut sched,
            CODE,
            &[],
            100
        ),
        Ok(wire::cpu::GuestCallOutcome::Returned(0))
    ));
    assert_eq!(cpu.instr_count, 2);
    assert_eq!(cpu.eflags & (CF | ZF), CF | ZF);
    assert!(matches!(
        cpu.run(&mut mem, &mut api, &mut state, &mut log, &mut sched, 100),
        RunOutcome::PresentationLimit(9)
    ));
    assert_eq!(cpu.instr_count, 2);
    cpu.halted = true;
    cpu.exit_code = Some(42);
    assert!(matches!(
        cpu.run(&mut mem, &mut api, &mut state, &mut log, &mut sched, 100),
        RunOutcome::Exited(42)
    ));
    drop(log);
    std::fs::remove_file(path).unwrap();
}
impl Outcome for RunOutcome {
    fn to_string_for_test(&self) -> String {
        match self {
            Self::Exited(n) => format!("exit {n}"),
            Self::InstructionLimit => "limit".into(),
            Self::Crashed(s) => s.clone(),
            Self::CallbackReturned => "callback".into(),
            Self::CallbackUnwound => "callback unwound".into(),
            Self::PresentationLimit(n) => format!("present {n}"),
        }
    }
}
#[test]
fn changed_code_and_unrecognized_entry_points_fall_back() {
    let code = [0xb8, 1, 0, 0, 0, 0x83, 0xc0, 1];
    let (mut cpu, mut mem) = machine(&code, true);
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.ir_steps, 1);
    assert_eq!(cpu.regs[0], 1);
    mem.write_u8(CODE + 7, 5).unwrap();
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.regs[0], 6);
    assert_eq!(cpu.ir_fallbacks, 1);
    // An overlapping entry was not statically decoded, so retain x86 semantics.
    let code = [0xb8, 0x90, 0x90, 0x90, 0x90];
    let (mut cpu, mut mem) = machine(&code, true);
    cpu.eip = CODE + 1;
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.eip, CODE + 2);
    assert_eq!(cpu.ir_fallbacks, 1);
    // Binding after relocation/patching rejects a record with changed bytes.
    let (mut cpu, mem) = machine(&code, false);
    mem.write_u8(CODE + 1, 2).unwrap();
    assert_eq!(
        cpu.install_ir(Image::from_code(CODE, &code).unwrap(), &mem)
            .unwrap(),
        0
    );
}

#[test]
fn compact_store_invalidates_a_prelinked_successor_and_rejected_records_are_not_linked() {
    let mut code = vec![0xb8, 0x49, 0, 0, 0, 0xa2]; // MOV AL -> code byte
    code.extend((CODE + 10).to_le_bytes());
    code.extend([0x41, 0xcc]); // INC ECX becomes DEC ECX
    let (mut cpu, mut mem) = machine(&code, true);
    cpu.regs[1] = 3;
    cpu.step(&mut mem).unwrap();
    cpu.step(&mut mem).unwrap();
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.regs[1], 2);
    assert_eq!((cpu.ir_steps, cpu.ir_fallbacks), (2, 1));

    // The first record binds but the subsequent patched one must not be
    // reachable through the precomputed sequential index.
    let code = [0xb8, 1, 0, 0, 0, 0x41, 0xcc];
    let (mut cpu, mut mem) = machine(&code, false);
    mem.write_u8(CODE + 5, 0x49).unwrap();
    cpu.install_ir(Image::from_code(CODE, &code).unwrap(), &mem)
        .unwrap();
    cpu.regs[1] = 3;
    cpu.step(&mut mem).unwrap();
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.regs[1], 2);
    assert_eq!((cpu.ir_steps, cpu.ir_fallbacks), (1, 1));
}

#[test]
fn chained_branches_validate_taken_targets_and_keep_every_budget_boundary() {
    // Both JNZ outcomes and a direct JMP stay within the interpreter chain.
    // The JMP target is deliberately an overlapping, unrecognized entry.
    let code = [
        0xb9, 2, 0, 0, 0, // MOV ECX,2
        0x83, 0xc0, 1, 0x49, 0x75, 0xfa, // ADD; DEC; JNZ ADD
        0xeb, 1, // JMP into the immediate below
        0xb8, 0x90, 0x90, 0x90, 0x90, 0xcc,
    ];
    for budget in 0..=15 {
        let mut results = Vec::new();
        for use_ir in [false, true] {
            let (mut cpu, mut mem) = machine(&code, use_ir);
            cpu.regs[0] = u32::MAX;
            let outcome = run(&mut cpu, &mut mem, budget);
            results.push((
                outcome.to_string_for_test(),
                cpu.regs,
                cpu.eflags,
                cpu.eip,
                cpu.instr_count,
            ));
        }
        assert_eq!(results[0], results[1], "overlapping entry budget {budget}");
    }

    // A compact store patches a different code page. The valid JMP source
    // must not permit its linked, now stale target to execute from IR.
    let target = CODE + 0x10000;
    let mut code = vec![0xb8, 0x49, 0, 0, 0, 0xa2];
    code.extend(target.to_le_bytes());
    code.push(0xe9);
    code.extend((target - (CODE + 15)).to_le_bytes());
    code.resize(0x10000, 0x90);
    code.extend([0x41, 0xcc]); // INC ECX is patched to DEC ECX
    for patched_before_bind in [false, true] {
        for budget in 0..=6 {
            let mut results = Vec::new();
            for use_ir in [false, true] {
                let (mut cpu, mut mem) = machine(&code, false);
                if patched_before_bind {
                    mem.write_u8(target, 0x49).unwrap();
                }
                if use_ir {
                    cpu.install_ir(Image::from_code(CODE, &code).unwrap(), &mem)
                        .unwrap();
                }
                cpu.regs[1] = 3;
                let outcome = run(&mut cpu, &mut mem, budget);
                results.push((
                    outcome.to_string_for_test(),
                    cpu.regs,
                    cpu.eflags,
                    cpu.eip,
                    cpu.instr_count,
                ));
                if budget >= 4 {
                    assert_eq!(cpu.regs[1], 2);
                    if use_ir {
                        assert_eq!((cpu.ir_steps, cpu.ir_fallbacks), (3, budget.min(5) - 3));
                    }
                }
            }
            assert_eq!(results[0], results[1], "patched target budget {budget}");
        }
    }
}

#[test]
fn typed_memory_access_keeps_exact_width_fault_state_and_flags() {
    let address = STACK + 0xffff;
    for second in [0xb6, 0xb7] {
        // MOVZX byte succeeds; word crosses the boundary
        let mut code = vec![0x83, 0xc0, 1, 0x0f, second, 0x1d];
        code.extend(address.to_le_bytes());
        code.push(0xcc);
        let mut results = Vec::new();
        for ir in [false, true] {
            let (mut cpu, mut mem) = machine(&code, ir);
            cpu.regs[0] = u32::MAX;
            mem.write_u8(address, 0x80).unwrap();
            let outcome = run(&mut cpu, &mut mem, 10);
            results.push((
                cpu.regs,
                cpu.eflags,
                cpu.eip,
                cpu.instr_count,
                outcome.to_string_for_test(),
            ));
            if ir {
                assert!(cpu.ir_steps >= 2);
            }
        }
        assert_eq!(results[0], results[1]);
        assert_eq!(results[0].0[3], if second == 0xb6 { 0x80 } else { 0 });
    }
}
#[test]
fn fixed_ir_roundtrips_and_rejects_invalid_files_or_wrong_exe() {
    let code = [0x83, 0xc0, 1, 0x0f, 0x95, 0xc1, 0xcc];
    let image = Image::from_code(CODE, &code).unwrap();
    let bytes = image.encode();
    assert_eq!(bytes.len(), 56 + 64 * image.records.len());
    let parsed = Image::parse(&bytes, &code).unwrap();
    assert_eq!(parsed.records.len(), image.records.len());
    assert_eq!(parsed.encode(), bytes);
    assert!(Image::parse(&bytes, &[0x90]).is_err());
    assert!(Image::parse(&bytes[..bytes.len() - 1], &code).is_err());
    for (offset, value) in [
        (8, 99),
        (12, 32),
        (56 + 8, 255),
        (56 + 20, 0xffff),
        (56 + 36, 0xffff),
    ] {
        let mut bad = bytes.clone();
        bad[offset..offset + 4].copy_from_slice(&(value as u32).to_le_bytes());
        assert!(Image::parse(&bad, &code).is_err(), "offset {offset}");
    }
    // GPR accessors use the proven 0..7 range; malformed six-bit fields
    // must be rejected before that representation reaches execution.
    for bit in [11, 17, 23] {
        let offset = 56 + 36;
        let mut bad = bytes.clone();
        let word = u32::from_le_bytes(bad[offset..offset + 4].try_into().unwrap()) | (1 << bit);
        bad[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
        assert!(Image::parse(&bad, &code).is_err(), "GPR field bit {bit}");
    }
}
#[test]
fn sib_fs_high_bytes_extensions_and_control_flow_match_direct_interpretation() {
    let code = [
        0xbb, 0, 0, 2, 0, 0xb9, 3, 0, 0, 0, 0xb8, 0x80, 0xff, 0, 0, 0x88, 0x64, 0x8b, 8, 0x0f,
        0xbe, 0x54, 0x8b, 8, 0x64, 0x8b, 0x35, 0, 0, 0, 0, 0x83, 0xfa, 0xff, 0x0f, 0x94, 0xc4,
        0x0f, 0x45, 0xd8, 0x50, 0x5f, 0xcc,
    ];
    let (mut cpu, mut mem) = machine(&code, false);
    let (mut ir, mut imem) = machine(&code, true);
    for (c, m) in [(&mut cpu, &mut mem), (&mut ir, &mut imem)] {
        c.fs_base = DATA;
        m.write_u32(DATA, 0x12345678).unwrap();
    }
    run(&mut cpu, &mut mem, 100);
    run(&mut ir, &mut imem, 100);
    assert_eq!(cpu.regs, ir.regs);
    assert_eq!(cpu.eflags, ir.eflags);
    assert!(ir.ir_steps >= 10);
}

#[test]
fn pe32_runtime_scalar_sse_matches_amd64_exact_state_and_preserves_persisted_ir() {
    use wire::x64::{
        cpu::Cpu64,
        memory::{Memory, EXECUTE, READ, WRITE},
    };
    let mut codes = Vec::new();
    for prefix in [0xf2, 0xf3] {
        for op in [
            0x58, 0x59, 0x5c, 0x5e, 0x5d, 0x5f, 0x5a, 0x51, 0x2a, 0x2c, 0x2d,
        ] {
            for operand in [0xc1, 0x03] {
                codes.push(vec![prefix, 0x0f, op, operand]);
            }
        }
    }
    for prefix in [None, Some(0x66)] {
        for op in [0x2e, 0x2f] {
            let mut c = prefix.into_iter().collect::<Vec<_>>();
            c.extend([0x0f, op, 0xc1]);
            codes.push(c);
        }
    }
    let values = [
        0u64,
        1,
        0x80000001,
        0x3f800001,
        0x7f812345,
        0x7fc12345,
        0x7f7fffff,
        0x3ff0000000000001,
        0x7ff0123456789abc,
        0x7ff8123456789abc,
        0x7fefffffffffffff,
    ];
    for code in codes {
        let mut m64 = Memory::default();
        m64.map(CODE as u64, 4096, READ | WRITE | EXECUTE).unwrap();
        m64.map(DATA as u64, 4096, READ | WRITE).unwrap();
        m64.write(CODE as u64, &code).unwrap();
        for installed in [false, true] {
            let (mut cpu, mut mem) = machine(&code, installed);
            for round in 0..4 {
                for controls in [0, 0x40, 0x8000, 0x8040] {
                    for masks in [0, 0x1f80] {
                        for seed in 0..values.len() {
                            cpu.eip = CODE;
                            cpu.eflags = 0xad7;
                            cpu.mxcsr = masks | controls | (round << 13) | 0x20;
                            cpu.regs[1] = values[(seed + 3) % values.len()] as u32;
                            cpu.regs[3] = DATA;
                            cpu.xmm = [[0xa5; 16]; 8];
                            cpu.xmm[0][..8].copy_from_slice(&values[seed].to_le_bytes());
                            cpu.xmm[1][..8]
                                .copy_from_slice(&values[(seed + 3) % values.len()].to_le_bytes());
                            mem.write_u64(DATA, values[(seed + 3) % values.len()])
                                .unwrap();
                            m64.write_int(DATA as u64, 8, values[(seed + 3) % values.len()])
                                .unwrap();
                            let mut reference = Cpu64::new(CODE as u64, 0);
                            reference.regs[..8].copy_from_slice(&cpu.regs.map(|x| x as u64));
                            reference.xmm[..8].copy_from_slice(&cpu.xmm);
                            reference.mxcsr = cpu.mxcsr;
                            reference.rflags = cpu.eflags as u64;
                            let a = cpu.step(&mut mem);
                            let b = reference.step(&mut m64);
                            assert_eq!(
                                a.is_ok(),
                                b.is_ok(),
                                "{code:x?} installed={installed} seed={seed}"
                            );
                            assert_eq!(
                                cpu.regs,
                                reference.regs[..8]
                                    .iter()
                                    .map(|&x| x as u32)
                                    .collect::<Vec<_>>()
                                    .as_slice()
                            );
                            assert_eq!(cpu.xmm, reference.xmm[..8]);
                            assert_eq!(cpu.mxcsr, reference.mxcsr);
                            assert_eq!(cpu.effective_eflags() as u64, reference.rflags);
                            assert_eq!(cpu.eip as u64, reference.rip);
                        }
                    }
                }
            }
            if installed {
                assert!(cpu.ir_steps > 0);
                assert_eq!(cpu.ir_fallbacks, 0);
            }
        }
    }
}

#[test]
fn pe32_runtime_sse_bit_tests_keep_raw_and_cached_memory_faults_and_invalidation() {
    for code in [
        vec![0x66, 0x0f, 0xd4, 0xc1],
        vec![0x66, 0x0f, 0x64, 0xc1],
        vec![0x66, 0x0f, 0x76, 0xc1],
        vec![0x0f, 0xab, 0x0b],
        vec![0xf0, 0x0f, 0xbb, 0x0b],
        vec![0x0f, 0xba, 0x23, 255],
    ] {
        let mut results = Vec::new();
        for installed in [false, true] {
            let (mut cpu, mut mem) = machine(&code, installed);
            cpu.xmm[0] = [0x80; 16];
            cpu.xmm[1] = [0x7f; 16];
            cpu.regs[1] = u32::MAX;
            cpu.regs[3] = DATA + 32;
            cpu.eflags = 0xad7;
            mem.write_u32(DATA + 28, 0x12345678).unwrap();
            mem.write_u32(DATA + 32, 0x87654321).unwrap();
            cpu.step(&mut mem).unwrap();
            results.push((
                cpu.regs,
                cpu.xmm,
                cpu.eip,
                cpu.effective_eflags(),
                mem.read_u32(DATA + 28).unwrap(),
                mem.read_u32(DATA + 32).unwrap(),
            ));
            if installed {
                assert_eq!(cpu.ir_fallbacks, 0);
            }
            mem.write_u8(CODE, 0x90).unwrap();
            cpu.eip = CODE;
            cpu.step(&mut mem).unwrap();
            assert_eq!(cpu.eip, CODE + 1);
        }
        assert_eq!(results[0], results[1], "{code:x?}");
    }
    for installed in [false, true] {
        for code in [vec![0x0f, 0xab, 0x0b], vec![0xf3, 0x0f, 0x58, 0x03]] {
            let (mut cpu, mut mem) = machine(&code, installed);
            cpu.regs[3] = DATA;
            cpu.eflags = 0xad7;
            cpu.mxcsr = 0;
            cpu.xmm[0] = [0xa5; 16];
            mem.virtual_protect(DATA, 0x10000, wire::mem::PAGE_NOACCESS)
                .unwrap();
            let before = cpu.save_ctx();
            assert!(cpu.step(&mut mem).is_err());
            let after = cpu.save_ctx();
            assert_eq!(after.regs, before.regs);
            assert_eq!(after.xmm, before.xmm);
            assert_eq!(after.eflags, before.eflags);
            assert_eq!(after.mxcsr, before.mxcsr);
            assert_eq!(after.eip, before.eip);
        }
    }
}

#[test]
fn pe32_raw_descriptor_cache_is_owned_by_memory_and_rechecks_guards_and_bytes() {
    let add = [0x66, 0x0f, 0xd4, 0xc1];
    let sub = [0x66, 0x0f, 0xfb, 0xc1];
    let (mut cpu, mut first) = machine(&add, false);
    let (_, mut second) = machine(&sub, false);
    let reset = |cpu: &mut Cpu| {
        cpu.eip = CODE;
        cpu.xmm[0] = [0; 16];
        cpu.xmm[1] = [0; 16];
        cpu.xmm[0][..8].copy_from_slice(&1u64.to_le_bytes());
        cpu.xmm[1][..8].copy_from_slice(&3u64.to_le_bytes());
    };
    reset(&mut cpu);
    cpu.step(&mut first).unwrap();
    assert_eq!(u64::from_le_bytes(cpu.xmm[0][..8].try_into().unwrap()), 4);
    reset(&mut cpu);
    cpu.step(&mut second).unwrap();
    assert_eq!(
        u64::from_le_bytes(cpu.xmm[0][..8].try_into().unwrap()),
        u64::MAX - 1
    );
    first.write_u8(CODE + 2, 0xfb).unwrap();
    reset(&mut cpu);
    cpu.step(&mut first).unwrap();
    assert_eq!(
        u64::from_le_bytes(cpu.xmm[0][..8].try_into().unwrap()),
        u64::MAX - 1
    );
    first
        .virtual_protect(
            CODE,
            0x10000,
            wire::mem::PAGE_EXECUTE_READWRITE | wire::mem::PAGE_GUARD,
        )
        .unwrap();
    reset(&mut cpu);
    let before = cpu.save_ctx();
    assert!(cpu.step(&mut first).is_err());
    assert_eq!(cpu.eip, before.eip);
    assert_eq!(cpu.xmm, before.xmm);
    cpu.step(&mut first).unwrap();
    first.set_dep_enabled(true);
    first
        .virtual_protect(CODE, 0x10000, wire::mem::PAGE_READWRITE)
        .unwrap();
    reset(&mut cpu);
    assert!(cpu.step(&mut first).is_err());
    assert_eq!(cpu.eip, CODE);
}

#[test]
fn pe32_scalar_fp_preserves_lazy_flags_and_comi_discards_them_only_after_success() {
    let code = [
        0xb8, 0xff, 0xff, 0xff, 0xff, 0xb9, 1, 0, 0, 0, 0x01, 0xc8, 0xf3, 0x0f, 0x58, 0xc1, 0x0f,
        0x2f, 0xc2, 0x0f, 0x92, 0xc2, 0x40, 0xcc,
    ];
    for budget in 0..=10 {
        for mxcsr in [0, 0x1f80] {
            for nan in [false, true] {
                let mut states = Vec::new();
                for installed in [false, true] {
                    let (mut cpu, mut mem) = machine(&code, installed);
                    cpu.mxcsr = mxcsr;
                    cpu.xmm[0][..4].copy_from_slice(&1.0f32.to_le_bytes());
                    cpu.xmm[1][..4].copy_from_slice(&0.0f32.to_le_bytes());
                    cpu.xmm[2][..4].copy_from_slice(
                        &(if nan { 0x7fc12345u32 } else { 2.0f32.to_bits() }).to_le_bytes(),
                    );
                    let outcome = run(&mut cpu, &mut mem, budget);
                    states.push((
                        std::mem::discriminant(&outcome),
                        cpu.regs,
                        cpu.xmm,
                        cpu.eip,
                        cpu.effective_eflags(),
                        cpu.mxcsr,
                        cpu.instr_count,
                    ));
                }
                assert_eq!(
                    states[0], states[1],
                    "budget={budget} mxcsr={mxcsr:x} nan={nan}"
                );
            }
        }
    }
}

#[test]
fn pe32_mixed_runs_preserve_all_entries_budgets_and_flags() {
    use iced_x86::{Decoder, DecoderOptions};
    let code = [
        0x01, 0xd8, 0x66, 0x0f, 0xd4, 0xf9, 0x41, 0x66, 0x0f, 0x64, 0xd7, 0x66, 0x0f, 0x7e, 0xfa,
        0x0f, 0x92, 0xc0, 0x66, 0x0f, 0xef, 0xf6, 0x0f, 0xba, 0xeb, 3, 0x11, 0xd9, 0xcc,
    ];
    let mut decoder = Decoder::with_ip(32, &code, CODE as u64, DecoderOptions::NONE);
    let mut entries = Vec::new();
    while decoder.can_decode() {
        entries.push(decoder.decode().ip() as u32);
    }
    for entry in entries {
        for budget in 0..=12 {
            for seed in [0u32, 0x7fffffff, u32::MAX] {
                let mut states = Vec::new();
                for installed in [false, true] {
                    let (mut cpu, mut mem) = machine(&code, installed);
                    cpu.eip = entry;
                    cpu.regs[0] = seed;
                    cpu.regs[1] = seed;
                    cpu.regs[3] = seed;
                    cpu.eflags = 2 | FLAGS;
                    for (n, x) in cpu.xmm.iter_mut().enumerate() {
                        *x = seed
                            .rotate_left(n as u32)
                            .to_le_bytes()
                            .repeat(4)
                            .try_into()
                            .unwrap();
                    }
                    let outcome = run(&mut cpu, &mut mem, budget);
                    states.push((
                        outcome.to_string_for_test(),
                        cpu.regs,
                        cpu.xmm,
                        cpu.eip,
                        cpu.effective_eflags(),
                        cpu.mxcsr,
                        cpu.instr_count,
                    ));
                    if installed && entry == CODE && budget >= 8 {
                        assert!(cpu.ir_scalar_runs > 0);
                    }
                }
                assert_eq!(
                    states[0], states[1],
                    "entry={entry:x} budget={budget} seed={seed:x}"
                );
            }
        }
    }
}
