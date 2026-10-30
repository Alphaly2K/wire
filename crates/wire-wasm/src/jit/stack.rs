use super::*;
use crate::memory::{READ, WRITE};
use iced_x86::Code as X86Code;

const SP: u32 = GPR + 4;
const BP: u32 = GPR + 5;

pub(super) fn supported(i: &Instruction) -> bool {
    use X86Code::*;
    !i.has_lock_prefix()
        && matches!(
            i.code(),
            Pushfw
                | Pushfd
                | Pushfq
                | Popfw
                | Popfd
                | Popfq
                | Pushaw
                | Pushad
                | Popaw
                | Popad
                | Enterw_imm16_imm8
                | Enterd_imm16_imm8
                | Enterq_imm16_imm8
        )
}

pub(super) fn memory_access(i: &Instruction) -> u8 {
    use X86Code::*;
    match i.code() {
        Pushfw | Pushfd | Pushfq | Pushaw | Pushad => WRITE,
        Popfw | Popfd | Popfq | Popaw | Popad => READ,
        _ => READ | WRITE,
    }
}

fn size(i: &Instruction) -> u32 {
    use X86Code::*;
    match i.code() {
        Pushfw | Popfw | Pushaw | Popaw | Enterw_imm16_imm8 => 2,
        Pushfd | Popfd | Pushad | Popad | Enterd_imm16_imm8 => 4,
        _ => 8,
    }
}

fn at(c: &mut Code, base: u32, offset: i64, mode: Mode) {
    c.get(base);
    c.i64(offset as u64);
    c.op(0x7c);
    if mode == Mode::X86 {
        scalar::truncate(c, 32);
    }
    c.set(ADDRESS);
}

fn save_flags(c: &mut Code, i: &Instruction, context: access::Context, size: u32) {
    flags::materialize(c, context.flags);
    c.get(FLAGS);
    c.i64(if size == 2 { 0xffff } else { 0xfcffff });
    c.op(0x83);
    c.set(RESULT);
    at(c, SP, -(size as i64), context.mode);
    context.prepare_address(c, i.ip(), size, WRITE);
    context.store(c, i.ip(), size);
    c.get(ADDRESS);
    c.set(SP);
}

fn restore_flags(
    c: &mut Code,
    i: &Instruction,
    known: &mut Option<flags::Recipe>,
    context: access::Context,
    size: u32,
) {
    at(c, SP, 0, context.mode);
    context.prepare_address(c, i.ip(), size, READ);
    context.load(c, i.ip(), size, READ);
    c.set(A);
    flags::materialize(c, context.flags);
    // Windows user code executes at CPL3/IOPL0. IF and IOPL are retained;
    // RF clears, and a 16-bit pop cannot modify AC or ID.
    let writable = if size == 2 { 0x4dd5 } else { 0x244dd5 };
    c.get(FLAGS);
    c.i64(!(writable | 0x10000));
    c.op(0x83);
    c.get(A);
    c.i64(writable);
    c.op(0x83);
    c.op(0x84);
    c.i64(2);
    c.op(0x84);
    c.set(FLAGS);
    flags::discard(c);
    *known = None;
    at(c, SP, size as i64, context.mode);
    c.get(ADDRESS);
    c.set(SP);
}

fn save_all(c: &mut Code, i: &Instruction, context: access::Context, size: u32) {
    for index in 0..8 {
        at(c, SP, -i64::from((index + 1) * size), context.mode);
        context.prepare_address(c, i.ip(), size, WRITE);
        c.get(GPR + index);
        c.set(RESULT);
        context.store(c, i.ip(), size);
    }
    c.get(ADDRESS);
    c.set(SP);
}

fn restore_all(c: &mut Code, i: &Instruction, context: access::Context, size: u32) {
    let registers = [7, 6, 5, 3, 2, 1, 0];
    // Pending values stay on the Wasm operand stack. A fault discards them at
    // the common exit; no architectural register has been committed yet.
    for (slot, index) in [7, 6, 5, 4, 3, 2, 1, 0].into_iter().enumerate() {
        if index != 4 {
            at(c, SP, slot as i64 * size as i64, context.mode);
            context.prepare_address(c, i.ip(), size, READ);
            context.load(c, i.ip(), size, READ);
        }
    }
    for index in registers.into_iter().rev() {
        c.set(RESULT);
        scalar::write_gpr(c, index, size * 8);
    }
    at(c, SP, 8 * size as i64, context.mode);
    c.get(ADDRESS);
    c.set(SP);
}

fn enter(c: &mut Code, i: &Instruction, context: access::Context, size: u32) {
    let level = u32::from(i.immediate8_2nd()) & 31;
    at(c, SP, -(size as i64), context.mode);
    context.prepare_address(c, i.ip(), size, WRITE);
    c.get(BP);
    c.set(RESULT);
    context.store(c, i.ip(), size);
    c.get(ADDRESS);
    c.set(COUNT);
    for depth in 1..level {
        at(c, BP, -i64::from(depth * size), context.mode);
        context.prepare_address(c, i.ip(), size, READ);
        context.load(c, i.ip(), size, READ);
        c.set(RESULT);
        at(c, COUNT, -i64::from(depth * size), context.mode);
        context.prepare_address(c, i.ip(), size, WRITE);
        context.store(c, i.ip(), size);
    }
    if level != 0 {
        at(c, COUNT, -i64::from(level * size), context.mode);
        context.prepare_address(c, i.ip(), size, WRITE);
        c.get(COUNT);
        c.set(RESULT);
        context.store(c, i.ip(), size);
    }
    at(
        c,
        COUNT,
        -i64::from((level * size) + u32::from(i.immediate16())),
        context.mode,
    );
    context.probe(c, i.ip(), size, WRITE);
    c.get(COUNT);
    c.set(RESULT);
    scalar::write_gpr(c, 5, size * 8);
    c.get(ADDRESS);
    c.set(SP);
}

pub(super) fn emit(
    c: &mut Code,
    i: &Instruction,
    known: &mut Option<flags::Recipe>,
    context: access::Context,
) {
    use X86Code::*;
    let size = size(i);
    match i.code() {
        Pushfw | Pushfd | Pushfq => save_flags(c, i, context, size),
        Popfw | Popfd | Popfq => restore_flags(c, i, known, context, size),
        Pushaw | Pushad => save_all(c, i, context, size),
        Popaw | Popad => restore_all(c, i, context, size),
        _ => enter(c, i, context, size),
    }
}
