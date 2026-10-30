//! Single-owner memory executes each RMW without guest scheduling. Register
//! and FLAGS commits follow the memory store so faults expose the prior state.
use super::*;
use crate::memory::{READ, WRITE};
use iced_x86::{Mnemonic as M, OpKind};

pub(super) fn supported(i: &Instruction) -> bool {
    wide(i)
        || matches!(i.mnemonic(), M::Xadd | M::Cmpxchg)
            && (scalar::register_operand(i, 0) || scalar::memory_operand(i, 0))
            && scalar::register_operand(i, 1)
            && (!i.has_lock_prefix() || i.op0_kind() == OpKind::Memory)
}

fn wide(i: &Instruction) -> bool {
    matches!(i.mnemonic(), M::Cmpxchg8b | M::Cmpxchg16b) && i.op0_kind() == OpKind::Memory
}

fn pair32(c: &mut Code, low: u32, high: u32) {
    c.get(GPR + high);
    scalar::truncate(c, 32);
    c.i64(32);
    c.op(0x86);
    c.get(GPR + low);
    scalar::truncate(c, 32);
    c.op(0x84);
}

fn emit_wide(
    c: &mut Code,
    i: &Instruction,
    known: &mut Option<flags::Recipe>,
    context: access::Context,
) {
    let wide = i.mnemonic() == M::Cmpxchg16b;
    context.address(c, i, true);
    c.set(ADDRESS);
    if wide {
        c.get(ADDRESS);
        c.i64(15);
        c.op(0x83);
        c.i64(0);
        c.op(0x52);
        c.if_void();
        exception(c, context.out, i.ip(), 13);
        c.end();
    }
    context.prepare_address(c, i.ip(), if wide { 16 } else { 8 }, READ | WRITE);
    context.load(c, i.ip(), 8, READ | WRITE);
    c.set(A);
    if wide {
        context.load_offset(c, i.ip(), 8, READ | WRITE, 8);
        c.set(B);
        c.get(GPR);
        c.get(A);
        c.op(0x51);
        c.get(GPR + 2);
        c.get(B);
        c.op(0x51);
        c.op(0x71);
    } else {
        pair32(c, 0, 2);
        c.get(A);
        c.op(0x51);
    }
    c.set(STATUS);
    if wide {
        for (register, old, result) in [(3, A, COUNT), (1, B, EXTRA)] {
            c.get(GPR + register);
            c.get(old);
            c.get(STATUS);
            c.op(0x1b);
            c.set(result);
        }
        context.store128(c, i.ip(), COUNT, EXTRA);
    } else {
        pair32(c, 3, 1);
        c.get(A);
        c.get(STATUS);
        c.op(0x1b);
        c.set(RESULT);
        context.store(c, i.ip(), 8);
    }
    c.get(STATUS);
    c.op(0x45);
    c.if_void();
    c.get(A);
    c.set(RESULT);
    scalar::write_gpr(c, 0, if wide { 64 } else { 32 });
    c.get(if wide { B } else { A });
    if !wide {
        c.i64(32);
        c.op(0x88);
    }
    c.set(RESULT);
    scalar::write_gpr(c, 2, if wide { 64 } else { 32 });
    c.end();
    flags::replace(c, 64, known);
}

pub(super) fn memory_access(i: &Instruction) -> u8 {
    if i.op0_kind() == OpKind::Memory {
        READ | WRITE
    } else {
        0
    }
}

pub(super) fn emit(
    c: &mut Code,
    i: &Instruction,
    known: &mut Option<flags::Recipe>,
    context: access::Context,
) {
    if wide(i) {
        emit_wide(c, i, known, context);
        return;
    }
    let bits = scalar::bits(i, 0);
    let memory = memory_access(i);
    if memory != 0 {
        context.prepare(c, i, bits / 8, memory);
    }
    scalar::read(c, i, 0, context);
    c.set(A);
    scalar::read(c, i, 1, context);
    c.set(B);
    let kind = if i.mnemonic() == M::Xadd {
        c.get(A);
        c.get(B);
        c.op(0x7c);
        scalar::truncate(c, bits);
        c.set(EXTRA);
        // The sum is the last register write when the operands alias.
        for (index, value) in if memory != 0 {
            [(0, EXTRA), (1, A)]
        } else {
            [(1, A), (0, EXTRA)]
        } {
            c.get(value);
            c.set(RESULT);
            scalar::write(c, i, index, context);
        }
        c.get(EXTRA);
        c.set(RESULT);
        flags::ADD
    } else {
        c.get(GPR);
        scalar::truncate(c, bits);
        c.set(EXTRA);
        c.get(B);
        c.get(A);
        c.get(EXTRA);
        c.get(A);
        c.op(0x51);
        c.op(0x1b);
        c.set(RESULT);
        scalar::write(c, i, 0, context);
        c.get(EXTRA);
        c.get(A);
        c.op(0x52);
        c.if_void();
        c.get(A);
        c.set(RESULT);
        scalar::write_gpr(c, 0, bits);
        c.end();
        if bits == 32 {
            c.get(GPR);
            scalar::truncate(c, 32);
            c.set(GPR);
        }
        c.get(EXTRA);
        c.get(A);
        c.op(0x7d);
        scalar::truncate(c, bits);
        c.set(RESULT);
        c.get(A);
        c.set(B);
        c.get(EXTRA);
        c.set(A);
        flags::SUB
    };
    let recipe = flags::Recipe {
        kind,
        bits,
        preserve_cf: false,
        carry_in: false,
        overridden: 0,
    };
    flags::defer(c, recipe, *known);
    *known = Some(recipe);
}
