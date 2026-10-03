//! First-chance x86 SEH dispatch for software-raised exceptions.
//! Full MSVC C++/SEH stack unwinding remains a separate compatibility task.
use crate::api::{ApiLogger, ApiState, ApiTable};
use crate::cpu::{Cpu, ThreadCtx};
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
        (28, ctx.fpu_cw as u32),
        (32, ctx.fpu_sw as u32),
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
    ctx.fpu_cw = mem.read_u16(p + 28).map_err(|e| e.to_string())?;
    ctx.fpu_sw = mem.read_u16(p + 32).map_err(|e| e.to_string())?;
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
    args: &[u32; 12],
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
        let result = cpu.call_guest(
            mem,
            api,
            state,
            log,
            sched,
            handler,
            &[record, frame, context, 0],
            max_instr,
        );
        cpu.load_ctx(&saved);
        match result? {
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
        let result = cpu.call_guest(mem, api, state, log, sched, filter, &[pointers], max_instr);
        cpu.load_ctx(&saved);
        match result? {
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
    Err(format!("unhandled guest exception {code:#010x} at {:#010x} ({} SEH frames; MSVC unwind is not yet implemented)", resumed.eip, seen.len()))
}
