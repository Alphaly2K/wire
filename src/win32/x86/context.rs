//! Checked PE32 Windows CONTEXT codec; no execution or process state.
use crate::{cpu::ThreadCtx, mem::GuestMem};

const REG_OFFSETS: [u32; 8] = [176, 172, 168, 164, 196, 180, 160, 156];

pub(crate) fn write_context(mem: &GuestMem, p: u32, ctx: &ThreadCtx) -> Result<(), String> {
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

pub(crate) fn read_context(
    mem: &GuestMem,
    p: u32,
    mut ctx: ThreadCtx,
) -> Result<ThreadCtx, String> {
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
