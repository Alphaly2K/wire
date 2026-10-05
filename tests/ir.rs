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
impl Outcome for RunOutcome {
    fn to_string_for_test(&self) -> String {
        match self {
            Self::Exited(n) => format!("exit {n}"),
            Self::InstructionLimit => "limit".into(),
            Self::Crashed(s) => s.clone(),
            Self::CallbackReturned => "callback".into(),
            Self::CallbackUnwound => "callback unwound".into(),
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
