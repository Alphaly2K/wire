use super::*;

pub(super) fn supported(i: &Instruction) -> bool {
    if i.has_lock_prefix() {
        return false;
    }
    let operands = match i.mnemonic() {
        M::Maskmovq => mmx::register_operand(i, 1) && mmx::register_operand(i, 2),
        M::Maskmovdqu => xmm(i, 1) && xmm(i, 2),
        _ => return false,
    };
    operands
        && matches!(
            i.op0_kind(),
            OpKind::MemorySegDI | OpKind::MemorySegEDI | OpKind::MemorySegRDI
        )
}

pub(super) fn emit(c: &mut Code, i: &Instruction, context: access::Context, bank: Bank) {
    let size = if i.mnemonic() == M::Maskmovq {
        mmx::wait(c, i, context, bank);
        8
    } else {
        16
    };
    let bits = match i.op0_kind() {
        OpKind::MemorySegDI => 16,
        OpKind::MemorySegEDI => 32,
        _ => 64,
    };
    c.get(GPR + 7);
    scalar::truncate(c, bits);
    if let Some(offset) = match i.segment_prefix() {
        Register::FS => Some(offset_of!(State, fs_base)),
        Register::GS => Some(offset_of!(State, gs_base)),
        _ => None,
    } {
        c.get(0);
        c.load64(offset);
        c.op(0x7c);
        if context.mode == Mode::X86 {
            scalar::truncate(c, 32);
        }
    }
    c.set(ADDRESS);
    c.get(bank.local(register(i, 2)));
    c.simd(0x64); // i8x16.bitmask
    c.i32(if size == 8 { 0xff } else { 0xffff });
    c.op(0x71);
    c.op(0xad);
    c.set(COUNT);
    c.get(COUNT);
    c.i64(0);
    c.op(0x52);
    c.if_void();
    context.cached_address(c, i.ip(), size, WRITE);
    c.get(POINTER);
    c.if_void();
    for index in 0..size {
        c.get(COUNT);
        c.i64(1 << index);
        c.op(0x83);
        c.i64(0);
        c.op(0x52);
        c.if_void();
        c.get(POINTER);
        c.get(bank.local(register(i, 1)));
        c.simd(0x16); // i8x16.extract_lane_u
        c.op(index as u8);
        c.memory(0x3a, index as usize); // i32.store8
        c.end();
    }
    c.otherwise();
    c.get(3);
    c.get(ADDRESS);
    c.get(bank.local(register(i, 1)));
    lane(c, false, 0);
    if size == 8 {
        c.i64(0);
    } else {
        c.get(bank.local(register(i, 1)));
        lane(c, false, 1);
    }
    c.get(COUNT);
    c.op(0xa7);
    c.call(7);
    c.set(TRANSLATION);
    context.store_status(c, i.ip());
    c.end();
    c.end();
    if size == 8 {
        mmx::effects(c, i, bank);
    }
}
