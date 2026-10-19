//! First-chance software x86 SEH and registration-chain unwind.
//! Guest MSVC handlers perform their own local cleanup and catch transfer.
//! Nested/collided unwind recovery and automatic CPU-fault dispatch remain open.
use crate::api::{ApiLogger, ApiState, ApiTable};
use crate::cpu::{Cpu, GuestCallOutcome, ThreadCtx};
use crate::mem::GuestMem;
use crate::thread::Sched;

const REG_OFFSETS: [u32; 8] = [176, 172, 168, 164, 196, 180, 160, 156];

fn write_context(mem: &GuestMem, p: u32, ctx: &ThreadCtx) -> Result<(), String> {
    mem.write_bytes(p, &[0; 716]).map_err(|e| e.to_string())?;
    mem.write_u32(p, 0x1002f).map_err(|e| e.to_string())?;
    for (i, offset) in REG_OFFSETS.iter().enumerate() {
        mem.write_u32(p + offset, ctx.regs[i])
            .map_err(|e| e.to_string())?;
    }
    for (offset, value) in [
        (28, ctx.fpu.cw as u32),
        (32, ctx.fpu.sw as u32),
        (36, ctx.fpu.tags as u32),
        (40, ctx.fpu.ip as u32),
        (44, (ctx.fpu.opcode as u32) << 16),
        (48, ctx.fpu.dp as u32),
        (144, 0x53),
        (148, 0x23),
        (152, 0x23),
        (184, ctx.eip),
        (188, 0x1b),
        (192, ctx.eflags),
        (200, 0x23),
        (228, ctx.mxcsr),
    ] {
        mem.write_u32(p + offset, value)
            .map_err(|e| e.to_string())?;
    }
    mem.write_u16(p + 204, ctx.fpu.cw)
        .map_err(|e| e.to_string())?;
    mem.write_u16(p + 206, ctx.fpu.sw)
        .map_err(|e| e.to_string())?;
    let mut abridged = 0u8;
    for i in 0..8 {
        if (ctx.fpu.tags >> (i * 2)) & 3 != 3 {
            abridged |= 1 << i;
        }
        let bytes = ctx.fpu.st_bits(i).to_le_bytes();
        mem.write_bytes(p + 56 + i as u32 * 10, &bytes[..10])
            .map_err(|e| e.to_string())?;
        mem.write_bytes(p + 236 + i as u32 * 16, &bytes[..10])
            .map_err(|e| e.to_string())?;
    }
    mem.write_u8(p + 208, abridged).map_err(|e| e.to_string())?;
    for i in 0..8 {
        mem.write_bytes(p + 364 + i as u32 * 16, &ctx.xmm[i])
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn read_context(mem: &GuestMem, p: u32, mut ctx: ThreadCtx) -> Result<ThreadCtx, String> {
    for (i, offset) in REG_OFFSETS.iter().enumerate() {
        ctx.regs[i] = mem.read_u32(p + offset).map_err(|e| e.to_string())?;
    }
    ctx.eip = mem.read_u32(p + 184).map_err(|e| e.to_string())?;
    ctx.eflags = mem.read_u32(p + 192).map_err(|e| e.to_string())?;
    ctx.fpu.cw = mem.read_u16(p + 28).map_err(|e| e.to_string())?;
    ctx.fpu.sw = mem.read_u16(p + 32).map_err(|e| e.to_string())?;
    ctx.fpu.tags = mem.read_u16(p + 36).map_err(|e| e.to_string())?;
    ctx.fpu.ip = mem.read_u32(p + 40).map_err(|e| e.to_string())? as u64;
    ctx.fpu.opcode = mem.read_u16(p + 46).map_err(|e| e.to_string())?;
    ctx.fpu.dp = mem.read_u32(p + 48).map_err(|e| e.to_string())? as u64;
    for i in 0..8 {
        let mut bytes = [0; 16];
        mem.read_bytes(p + 56 + i as u32 * 10, &mut bytes[..10])
            .map_err(|e| e.to_string())?;
        let physical = (ctx.fpu.top() + i) & 7;
        ctx.fpu.regs[physical] = u128::from_le_bytes(bytes);
    }
    ctx.mxcsr = mem.read_u32(p + 228).map_err(|e| e.to_string())?;
    for i in 0..8 {
        mem.read_bytes(p + 364 + i as u32 * 16, &mut ctx.xmm[i])
            .map_err(|e| e.to_string())?;
    }
    Ok(ctx)
}

#[allow(clippy::too_many_arguments)]
pub fn raise(
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    args: &crate::api::Args,
    max_instr: u64,
) -> Result<(), String> {
    let saved = cpu.save_ctx();
    let mut resumed = saved;
    resumed.eip = mem.read_u32(cpu.esp()).map_err(|e| e.to_string())?;
    resumed.regs[4] = cpu
        .esp()
        .checked_add(20)
        .ok_or("exception stack overflow")?;
    let p = state.scratch_alloc(mem, 832, 16);
    let record = p;
    let context = p + 80;
    let pointers = p + 800;
    mem.write_bytes(record, &[0; 80])
        .map_err(|e| e.to_string())?;
    let code = args[0] & !0x1000_0000;
    mem.write_u32(record, code).map_err(|e| e.to_string())?;
    mem.write_u32(record + 4, args[1] & 1)
        .map_err(|e| e.to_string())?;
    mem.write_u32(record + 12, resumed.eip)
        .map_err(|e| e.to_string())?;
    let count = args[2].min(15);
    mem.write_u32(record + 16, count)
        .map_err(|e| e.to_string())?;
    for i in 0..count {
        let value = mem
            .read_u32(args[3].wrapping_add(i * 4))
            .map_err(|e| e.to_string())?;
        mem.write_u32(record + 20 + i * 4, value)
            .map_err(|e| e.to_string())?;
    }
    write_context(mem, context, &resumed)?;
    mem.write_u32(pointers, record).map_err(|e| e.to_string())?;
    mem.write_u32(pointers + 4, context)
        .map_err(|e| e.to_string())?;
    let mut frame = mem.read_u32(cpu.fs_base).map_err(|e| e.to_string())?;
    let mut seen = std::collections::HashSet::new();
    while frame != u32::MAX && frame != 0 {
        if seen.len() >= 128 || !seen.insert(frame) {
            return Err("invalid/cyclic SEH chain".into());
        }
        let next = mem.read_u32(frame).map_err(|e| e.to_string())?;
        let handler = mem.read_u32(frame + 4).map_err(|e| e.to_string())?;
        log.note(&format!(
            "SEH {code:#010x}: frame={frame:#x} handler={handler:#x}"
        ));
        let result = crate::x86::process::call_guest(
            cpu,
            mem,
            api,
            state,
            log,
            sched,
            handler,
            &[record, frame, context, 0],
            max_instr,
        );
        let result = match result {
            Ok(GuestCallOutcome::Returned(value)) => {
                cpu.load_ctx(&saved);
                value
            }
            Ok(_) => return Ok(()), // catch/exit discarded the exception callback
            Err(error) => {
                cpu.load_ctx(&saved);
                return Err(error);
            }
        };
        match result {
            0 => {
                if args[1] & 1 != 0 {
                    return Err(format!(
                        "cannot continue noncontinuable exception {code:#010x}"
                    ));
                }
                cpu.load_ctx(&read_context(mem, context, resumed)?);
                return Ok(());
            }
            1 => frame = next,
            value => {
                return Err(format!(
                    "unsupported SEH disposition {value} for exception {code:#010x}"
                ))
            }
        }
    }
    if state.unhandled_filter != 0 {
        let filter = state.unhandled_filter;
        let result = crate::x86::process::call_guest(
            cpu,
            mem,
            api,
            state,
            log,
            sched,
            filter,
            &[pointers],
            max_instr,
        );
        let result = match result {
            Ok(GuestCallOutcome::Returned(value)) => {
                cpu.load_ctx(&saved);
                value
            }
            Ok(_) => return Ok(()),
            Err(error) => {
                cpu.load_ctx(&saved);
                return Err(error);
            }
        };
        match result {
            u32::MAX if args[1] & 1 == 0 => {
                cpu.load_ctx(&read_context(mem, context, resumed)?);
                return Ok(());
            }
            1 => {
                cpu.halted = true;
                cpu.exit_code = Some(code);
                return Ok(());
            }
            _ => {}
        }
    }
    Err(format!(
        "unhandled guest exception {code:#010x} at {:#010x} ({} SEH frames)",
        resumed.eip,
        seen.len()
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn unwind(
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    args: &crate::api::Args,
    max_instr: u64,
) -> Result<(), String> {
    let saved = cpu.save_ctx();
    let mut resumed = saved;
    // The 32-bit ABI resumes RtlUnwind's caller (TargetIp is unused). The
    // caller's guest CRT then restores its target frame and enters the catch.
    resumed.eip = mem.read_u32(cpu.esp()).map_err(|e| e.to_string())?;
    resumed.regs[4] = cpu.esp().checked_add(20).ok_or("unwind stack overflow")?;
    resumed.regs[0] = args[3];
    let target = args[0];
    let stack_top = mem.read_u32(cpu.fs_base + 4).map_err(|e| e.to_string())?;
    let stack_limit = mem.read_u32(cpu.fs_base + 8).map_err(|e| e.to_string())?;
    let mut frame = mem.read_u32(cpu.fs_base).map_err(|e| e.to_string())?;
    let mut frames = Vec::new();
    // Validate the chain/target before starting cleanup. Do not silently skip
    // a destructor when a target is absent or a registration is corrupt.
    while frame != u32::MAX {
        if frames.len() >= 128
            || frame & 3 != 0
            || frame < stack_limit
            || frame.checked_add(8).is_none_or(|end| end > stack_top)
        {
            return Err(format!("invalid SEH unwind frame {frame:#x}"));
        }
        if frame == target {
            break; // target stays installed and is not called during global unwind
        }
        if target != 0 && frame > target {
            return Err(format!("invalid SEH unwind target {target:#x}"));
        }
        let next = mem.read_u32(frame).map_err(|e| e.to_string())?;
        let handler = mem.read_u32(frame + 4).map_err(|e| e.to_string())?;
        if next != u32::MAX && next <= frame {
            return Err("invalid/cyclic SEH unwind chain".into());
        }
        frames.push((frame, next, handler));
        frame = next;
    }
    if target != 0 && frame != target {
        return Err(format!("SEH unwind target {target:#x} is not in the chain"));
    }
    let storage = state.scratch_alloc(mem, 832, 16);
    let record = if args[2] != 0 { args[2] } else { storage };
    let context = storage + 80;
    let dispatcher = storage + 800;
    if args[2] == 0 {
        mem.write_bytes(record, &[0; 80])
            .map_err(|e| e.to_string())?;
        mem.write_u32(record, 0xc000_0027)
            .map_err(|e| e.to_string())?; // STATUS_UNWIND
        mem.write_u32(record + 12, resumed.eip)
            .map_err(|e| e.to_string())?;
    }
    let flags = mem.read_u32(record + 4).map_err(|e| e.to_string())?;
    mem.write_u32(record + 4, flags | 2 | if target == 0 { 4 } else { 0 })
        .map_err(|e| e.to_string())?;
    write_context(mem, context, &resumed)?;
    for (frame, next, handler) in frames {
        mem.write_u32(dispatcher, 0).map_err(|e| e.to_string())?;
        log.note(&format!(
            "SEH unwind: frame={frame:#x} handler={handler:#x} target={target:#x}"
        ));
        let result = crate::x86::process::call_guest(
            cpu,
            mem,
            api,
            state,
            log,
            sched,
            handler,
            &[record, frame, context, dispatcher],
            max_instr,
        );
        let disposition = match result {
            Ok(GuestCallOutcome::Returned(value)) => {
                cpu.load_ctx(&saved);
                value
            }
            Ok(GuestCallOutcome::Exited(_)) => return Ok(()),
            Ok(GuestCallOutcome::Transferred) => {
                return Err("nested/collided SEH unwind transfer is unsupported".into());
            }
            Err(error) => {
                cpu.load_ctx(&saved);
                return Err(error);
            }
        };
        if disposition != 1 {
            return Err(format!("unsupported SEH unwind disposition {disposition}"));
        }
        if mem.read_u32(cpu.fs_base).map_err(|e| e.to_string())? != frame
            || mem.read_u32(frame).map_err(|e| e.to_string())? != next
        {
            return Err(
                "SEH registration changed during unwind (collided unwind unsupported)".into(),
            );
        }
        mem.write_u32(cpu.fs_base, next)
            .map_err(|e| e.to_string())?;
    }
    cpu.load_ctx(&read_context(mem, context, resumed)?);
    Ok(())
}
