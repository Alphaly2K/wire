//! Shift counts and zero-count FLAGS follow the guest operand width rather
//! than Wasm's fixed 64-bit shift masking.
use super::*;
use crate::memory::{READ, WRITE};
use iced_x86::{Mnemonic as M, OpKind};

pub(super) fn double_supported(i: &Instruction) -> bool {
    matches!(i.mnemonic(), M::Shld | M::Shrd)
        && !i.has_lock_prefix()
        && (scalar::register_operand(i, 0) || scalar::memory_operand(i, 0))
        && scalar::register_operand(i, 1)
        && (scalar::register_operand(i, 2) || i.op2_kind() == OpKind::Immediate8)
}

pub(super) fn double_access(i: &Instruction) -> u8 {
    if i.op0_kind() == OpKind::Memory {
        READ | WRITE
    } else {
        0
    }
}

pub(super) fn emit_double(
    c: &mut Code,
    i: &Instruction,
    known: &mut Option<flags::Recipe>,
    context: access::Context,
) {
    let bits = scalar::bits(i, 0);
    let left = i.mnemonic() == M::Shld;
    let access = double_access(i);
    if access != 0 {
        context.prepare(c, i, bits / 8, access);
    }
    scalar::read(c, i, 0, context);
    c.set(A);
    scalar::read(c, i, 1, context);
    c.set(B);
    scalar::read(c, i, 2, context);
    c.i64(if bits == 64 { 63 } else { 31 });
    c.op(0x83);
    c.set(COUNT);
    c.get(A);
    c.set(RESULT);
    c.get(COUNT);
    c.i64(0);
    c.op(0x52);
    c.if_void();
    if bits == 16 {
        c.get(if left { A } else { B });
        c.i64(16);
        c.op(0x86);
        c.get(if left { B } else { A });
        c.op(0x84);
        c.get(COUNT);
        c.op(if left { 0x86 } else { 0x88 });
        if left {
            c.i64(16);
            c.op(0x88);
        }
    } else {
        c.get(A);
        c.get(COUNT);
        c.op(if left { 0x86 } else { 0x88 });
        c.get(B);
        c.i64(bits as u64);
        c.get(COUNT);
        c.op(0x7d);
        c.op(if left { 0x88 } else { 0x86 });
        c.op(0x84);
    }
    scalar::truncate(c, bits);
    c.set(RESULT);
    c.end();
    scalar::write(c, i, 0, context);
    c.get(COUNT);
    c.i64(0);
    c.op(0x52);
    c.if_void();
    c.get(COUNT);
    c.set(B);
    flags::defer(
        c,
        flags::Recipe {
            kind: if left { flags::SHL } else { flags::SHRD },
            bits,
            preserve_cf: false,
            carry_in: false,
            overridden: 0,
        },
        *known,
    );
    c.end();
    *known = None;
}

pub(super) fn emit(
    c: &mut Code,
    instruction: &Instruction,
    known: &mut Option<flags::Recipe>,
    context: access::Context,
) {
    let bits = scalar::bits(instruction, 0);
    let kind = match instruction.mnemonic() {
        M::Shl | M::Sal => flags::SHL,
        M::Shr => flags::SHR,
        M::Sar => flags::SAR,
        _ => unreachable!(),
    };
    c.get(B);
    c.i64(if bits == 64 { 63 } else { 31 });
    c.op(0x83);
    c.set(B);
    c.get(A);
    if kind == flags::SAR {
        scalar::sign_extend(c, bits);
    }
    c.get(B);
    c.op(match kind {
        flags::SHL => 0x86,
        flags::SHR => 0x88,
        _ => 0x87,
    });
    scalar::truncate(c, bits);
    c.set(RESULT);
    scalar::write(c, instruction, 0, context);
    c.get(B);
    c.i64(0);
    c.op(0x52);
    c.if_void();
    if kind == flags::SAR {
        c.get(A);
        scalar::sign_extend(c, bits);
        c.set(A);
    }
    flags::defer(
        c,
        flags::Recipe {
            kind,
            bits,
            preserve_cf: false,
            carry_in: false,
            overridden: 0,
        },
        *known,
    );
    c.end();
    *known = None;
}
