//! Existing behavioral tests grouped by exceptions; expectations remain independent of API catalogs.

use super::*;

#[test]
fn software_seh_continues_with_modified_context_and_rejects_noncontinuable() {
    let mut v = Vm::new();
    // mov eax,[esp+12]; mov [eax+176],0x12345678; xor eax,eax; ret
    v.mem
        .write_bytes(
            CODE,
            &[
                0x8b, 0x44, 0x24, 0x0c, 0xc7, 0x80, 0xb0, 0, 0, 0, 0x78, 0x56, 0x34, 0x12, 0x31,
                0xc0, 0xc3,
            ],
        )
        .unwrap();
    v.mem.write_u32(DATA, u32::MAX).unwrap();
    v.mem.write_u32(DATA + 4, CODE).unwrap();
    v.mem.write_u32(TEB_ADDR, DATA).unwrap();
    assert_eq!(v.k("RaiseException", &[0x1234, 0, 0, 0]), 0x12345678);
    assert_eq!(v.cpu.eip, RET);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 236);
    let error = v
        .try_call("kernel32.dll", "RaiseException", &[0x1234, 1, 0, 0])
        .unwrap_err();
    assert!(error.contains("noncontinuable"));
}

#[test]
fn unhandled_filter_is_registered_invoked_and_prior_filter_returned() {
    let mut v = Vm::new();
    // Filter returns EXCEPTION_CONTINUE_EXECUTION (-1), stdcall one argument.
    v.mem
        .write_bytes(CODE, &[0xb8, 0xff, 0xff, 0xff, 0xff, 0xc2, 4, 0])
        .unwrap();
    assert_eq!(v.k("SetUnhandledExceptionFilter", &[CODE]), 0);
    v.k("RaiseException", &[0x1234, 0, 0, 0]);
    assert_eq!(v.cpu.eip, RET);
    assert_eq!(v.k("SetUnhandledExceptionFilter", &[0]), CODE);
    let error = v
        .try_call("kernel32.dll", "RaiseException", &[0xe06d7363, 1, 0, 0])
        .unwrap_err();
    assert!(error.contains("unhandled guest exception"));
}

#[test]
fn seh_unwind_cleans_inner_frames_keeps_target_and_restores_context() {
    let mut v = Vm::new();
    let first = STACK_BASE + STACK_SIZE - 128;
    let target = first + 32;
    // Each handler records its own frame, current FS head, flags and code.
    for (i, frame) in [first, first + 16].into_iter().enumerate() {
        let handler = CODE + i as u32 * 64;
        let out = DATA + i as u32 * 16;
        let mut code = vec![0x8b, 0x54, 0x24, 8, 0x89, 0x15];
        code.extend(out.to_le_bytes());
        code.extend([0x64, 0xa1, 0, 0, 0, 0, 0xa3]);
        code.extend((out + 4).to_le_bytes());
        code.extend([0x8b, 0x54, 0x24, 4, 0x8b, 0x42, 4, 0xa3]);
        code.extend((out + 8).to_le_bytes());
        code.extend([0x8b, 0x02, 0xa3]);
        code.extend((out + 12).to_le_bytes());
        code.extend([0xb8, 1, 0, 0, 0, 0xc3]);
        v.mem.write_bytes(handler, &code).unwrap();
        v.mem.write_u32(frame, frame + 16).unwrap();
        v.mem.write_u32(frame + 4, handler).unwrap();
    }
    v.mem.write_u32(target, u32::MAX).unwrap();
    v.mem.write_u32(target + 4, CODE + 0x500).unwrap();
    v.mem.write_u8(CODE + 0x500, 0xcc).unwrap(); // target must never be invoked
    v.mem.write_u32(TEB_ADDR, first).unwrap();
    v.mem.write_u32(DATA + 64, 0x1234).unwrap();
    v.mem.write_u32(DATA + 68, 1).unwrap();
    v.cpu.regs[3] = 0xfeed_beef;
    assert_eq!(
        v.k("RtlUnwind", &[target, CODE + 0x500, DATA + 64, 0xface]),
        0xface
    );
    assert_eq!(v.cpu.regs[3], 0xfeed_beef);
    assert_eq!(v.cpu.eip, RET); // x86 TargetIp is ignored
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 236);
    assert_eq!(v.mem.read_u32(TEB_ADDR).unwrap(), target);
    for (i, frame) in [first, first + 16].into_iter().enumerate() {
        let out = DATA + i as u32 * 16;
        assert_eq!(v.mem.read_u32(out).unwrap(), frame);
        assert_eq!(v.mem.read_u32(out + 4).unwrap(), frame);
        assert_eq!(v.mem.read_u32(out + 8).unwrap(), 3);
        assert_eq!(v.mem.read_u32(out + 12).unwrap(), 0x1234);
    }
    // Null record/target generates STATUS_UNWIND and marks exit unwind.
    v.mem.write_u32(TEB_ADDR, first).unwrap();
    v.mem.write_u32(first, u32::MAX).unwrap();
    v.k("RtlUnwind", &[0, 0, 0, 7]);
    assert_eq!(v.mem.read_u32(DATA + 8).unwrap(), 6);
    assert_eq!(v.mem.read_u32(DATA + 12).unwrap(), 0xc000_0027);
    assert_eq!(v.mem.read_u32(TEB_ADDR).unwrap(), u32::MAX);
}

#[test]
fn seh_unwind_rejects_corrupt_or_absent_targets_before_cleanup() {
    let mut v = Vm::new();
    let first = STACK_BASE + STACK_SIZE - 128;
    v.mem.write_u32(first, u32::MAX).unwrap();
    v.mem.write_u32(first + 4, CODE).unwrap();
    v.mem.write_u8(CODE, 0xcc).unwrap(); // validation must not run this handler
    v.mem.write_u32(TEB_ADDR, first).unwrap();
    assert!(v
        .try_call("kernel32.dll", "RtlUnwind", &[first + 16, 0, 0, 0])
        .unwrap_err()
        .contains("not in the chain"));
    assert_eq!(v.mem.read_u32(TEB_ADDR).unwrap(), first);
    v.mem.write_u32(first, first).unwrap();
    assert!(v
        .try_call("ntdll.dll", "RtlUnwind", &[0, 0, 0, 0])
        .unwrap_err()
        .contains("cyclic"));
    v.mem.write_u32(TEB_ADDR, DATA).unwrap();
    assert!(v
        .try_call("kernel32.dll", "RtlUnwind", &[0, 0, 0, 0])
        .unwrap_err()
        .contains("invalid SEH unwind frame"));
}

#[test]
fn seh_catch_transfer_escapes_raise_and_initializer_host_callbacks() {
    for initializer in [false, true] {
        let mut v = Vm::new();
        let first = STACK_BASE + STACK_SIZE - 128;
        let target = first + 32;
        let unwind = v.api.register("kernel32.dll", Some("RtlUnwind"), None);
        let raise = v.api.register("kernel32.dll", Some("RaiseException"), None);
        v.mem.write_bytes(CODE, &[0xb8, 1, 0, 0, 0, 0xc3]).unwrap();
        v.mem.write_u32(first, target).unwrap();
        v.mem.write_u32(first + 4, CODE).unwrap();
        v.mem.write_u32(target, u32::MAX).unwrap();
        v.mem.write_u32(target + 4, CODE + 64).unwrap();
        v.mem.write_u32(TEB_ADDR, first).unwrap();
        // Outer search handler: global unwind, then discard the callback stack
        // and jump to a catch outside both RaiseException and _initterm.
        let mut handler = vec![0x8b, 0x44, 0x24, 4, 0x6a, 0, 0x50, 0x6a, 0, 0x68];
        handler.extend(target.to_le_bytes());
        handler.push(0xb8);
        handler.extend(unwind.to_le_bytes());
        handler.extend([0xff, 0xd0, 0xbc]);
        handler.extend((target - 16).to_le_bytes());
        handler.push(0xb8);
        handler.extend((CODE + 256).to_le_bytes());
        handler.extend([0xff, 0xe0]);
        v.mem.write_bytes(CODE + 64, &handler).unwrap();
        let mut catch = vec![0xc7, 0x05];
        catch.extend(DATA.to_le_bytes());
        catch.extend(0xcafe_beefu32.to_le_bytes());
        catch.push(0xcc);
        v.mem.write_bytes(CODE + 256, &catch).unwrap();
        if initializer {
            let mut code = vec![0x6a, 0, 0x6a, 0, 0x6a, 1, 0x68];
            code.extend(0xe06d_7363u32.to_le_bytes());
            code.push(0xb8);
            code.extend(raise.to_le_bytes());
            code.extend([0xff, 0xd0, 0xcc]); // cannot return here
            v.mem.write_bytes(CODE + 512, &code).unwrap();
            v.mem.write_u32(DATA + 64, CODE + 512).unwrap();
            v.call("msvcrt.dll", "_initterm", &[DATA + 64, DATA + 68]);
        } else {
            v.k("RaiseException", &[0xe06d_7363, 1, 0, 0]);
        }
        assert_eq!(v.cpu.esp(), target - 16);
        assert_ne!(v.cpu.eip, RET); // no stub epilogue/saved callback restoration
        let outcome = v.cpu.run(
            &mut v.mem,
            &mut v.api,
            &mut v.state,
            &mut v.log,
            &mut v.sched,
            1_000_000,
        );
        assert!(
            matches!(outcome, wire::cpu::RunOutcome::Crashed(ref e) if e.contains("breakpoint"))
        );
        assert_eq!(v.mem.read_u32(DATA).unwrap(), 0xcafe_beef);
        assert_eq!(v.mem.read_u32(TEB_ADDR).unwrap(), target);
    }
}

#[test]
fn seh_context_preserves_and_allows_editing_x87_stack() {
    let mut v = Vm::new();
    v.mem.write_bytes(CODE + 0x400, &[0xd9, 0xe8]).unwrap();
    v.cpu.eip = CODE + 0x400;
    v.cpu.step(&mut v.mem).unwrap();
    v.cpu.fpu.cw = 0xb7f;
    // FNINIT; ctx=[esp+12]; FLDZ; FSTP m80[ctx+56]; tag[ctx]=zero;
    // return ExceptionContinueExecution. Callback clobbers the live FP state.
    v.mem
        .write_bytes(
            CODE,
            &[
                0xdb, 0xe3, 0x8b, 0x44, 0x24, 0x0c, 0xd9, 0xee, 0xdb, 0x78, 0x38, 0xc7, 0x40, 0x24,
                0xff, 0x7f, 0, 0, 0x31, 0xc0, 0xc3,
            ],
        )
        .unwrap();
    v.mem.write_u32(DATA, u32::MAX).unwrap();
    v.mem.write_u32(DATA + 4, CODE).unwrap();
    v.mem.write_u32(TEB_ADDR, DATA).unwrap();
    v.k("RaiseException", &[0x1234, 0, 0, 0]);
    assert_eq!(v.cpu.fpu.cw, 0xb7f);
    assert_eq!(v.cpu.fpu.top(), 7);
    assert_eq!(v.cpu.fpu.tags, 0x7fff);
    assert_eq!(v.cpu.fpu.st_bits(0), 0);
}
