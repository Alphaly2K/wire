//! Operand values stay in locals; wide division returns only its numeric pair
//! through the owning core, without exporting the resident architectural state.
use super::*;
use crate::memory::READ;
use iced_x86::{Mnemonic as M, OpKind};

pub(super) fn supported(i: &Instruction) -> bool {
    if i.has_lock_prefix() || i.flow_control() != FlowControl::Next {
        return false;
    }
    let value = |index| scalar::register_operand(i, index) || scalar::memory_operand(i, index);
    match (i.mnemonic(), i.op_count()) {
        (M::Mul | M::Imul | M::Div | M::Idiv, 1) => value(0),
        (M::Imul, 2) => scalar::register_operand(i, 0) && value(1),
        (M::Imul, 3) => {
            scalar::register_operand(i, 0)
                && value(1)
                && matches!(
                    i.op2_kind(),
                    OpKind::Immediate8to16
                        | OpKind::Immediate8to32
                        | OpKind::Immediate8to64
                        | OpKind::Immediate16
                        | OpKind::Immediate32
                        | OpKind::Immediate32to64
                )
        }
        _ => false,
    }
}

pub(super) fn memory_access(i: &Instruction) -> u8 {
    if (0..i.op_count()).any(|index| i.op_kind(index) == OpKind::Memory) {
        READ
    } else {
        0
    }
}

pub(super) fn needs_numeric(i: &Instruction) -> bool {
    supported(i) && scalar::bits(i, 0) == 64
}

fn fail_if(c: &mut Code, i: &Instruction, context: access::Context) {
    c.if_void();
    exception(c, context.out, i.ip(), 0);
    c.end();
}

fn divide(c: &mut Code, i: &Instruction, context: access::Context, numeric: u32) {
    let bits = scalar::bits(i, 0);
    let signed = i.mnemonic() == M::Idiv;
    scalar::read(c, i, 0, context);
    c.set(B);
    if bits == 64 {
        c.get(3);
        c.get(GPR);
        c.get(GPR + 2);
        c.get(B);
        c.call(numeric + if signed { 3 } else { 2 });
        c.set(TRANSLATION);
        c.get(TRANSLATION);
        c.op(0x45);
        fail_if(c, i, context);
        c.get(TRANSLATION);
        c.load64(0);
        c.set(RESULT);
        c.get(TRANSLATION);
        c.load64(8);
        c.set(B);
    } else {
        c.get(GPR);
        scalar::truncate(c, if bits == 8 { 16 } else { bits });
        if bits != 8 {
            c.get(GPR + 2);
            scalar::truncate(c, bits);
            c.i64(bits as u64);
            c.op(0x86);
            c.op(0x84);
        }
        if signed {
            scalar::sign_extend(c, bits * 2);
        }
        c.set(A);
        c.get(B);
        if signed {
            scalar::sign_extend(c, bits);
        }
        c.set(B);
        c.get(B);
        c.op(0x50);
        fail_if(c, i, context);
        if signed && bits == 32 {
            c.get(A);
            c.i64(1 << 63);
            c.op(0x51);
            c.get(B);
            c.i64(u64::MAX);
            c.op(0x51);
            c.op(0x71);
            fail_if(c, i, context);
        }
        c.get(A);
        c.get(B);
        c.op(if signed { 0x7f } else { 0x80 });
        c.set(RESULT);
        c.get(RESULT);
        scalar::truncate(c, bits);
        if signed {
            scalar::sign_extend(c, bits);
        }
        c.get(RESULT);
        c.op(0x52);
        fail_if(c, i, context);
        c.get(A);
        c.get(B);
        c.op(if signed { 0x81 } else { 0x82 });
        c.set(B);
    }
    if bits == 8 {
        c.get(RESULT);
        scalar::truncate(c, 8);
        c.get(B);
        scalar::truncate(c, 8);
        c.i64(8);
        c.op(0x86);
        c.op(0x84);
        c.set(RESULT);
        scalar::write_gpr(c, 0, 16);
    } else {
        scalar::write_gpr(c, 0, bits);
        c.get(B);
        c.set(RESULT);
        scalar::write_gpr(c, 2, bits);
    }
}

pub(super) fn emit(
    c: &mut Code,
    i: &Instruction,
    known: &mut Option<flags::Recipe>,
    context: access::Context,
    numeric: u32,
) {
    let bits = scalar::bits(i, 0);
    if memory_access(i) != 0 {
        context.prepare(c, i, bits / 8, READ);
    }
    if matches!(i.mnemonic(), M::Div | M::Idiv) {
        divide(c, i, context, numeric);
        return;
    }
    let implicit = i.op_count() == 1;
    let signed = i.mnemonic() == M::Imul;
    if implicit {
        c.get(GPR);
        scalar::truncate(c, bits);
    } else {
        scalar::read(c, i, if i.op_count() == 3 { 1 } else { 0 }, context);
    }
    c.set(A);
    scalar::read(c, i, if implicit { 0 } else { i.op_count() - 1 }, context);
    scalar::truncate(c, bits);
    c.set(B);
    for local in [A, B] {
        c.get(local);
        if signed {
            scalar::sign_extend(c, bits);
        }
    }
    c.op(0x7e);
    c.set(RESULT);
    if bits == 64 {
        c.get(A);
        c.get(B);
        c.call(numeric + u32::from(signed));
        c.set(ADDRESS);
        c.get(ADDRESS);
        if signed {
            c.get(RESULT);
            c.i64(63);
            c.op(0x87);
        } else {
            c.i64(0);
        }
    } else {
        c.get(RESULT);
        c.i64(bits as u64);
        c.op(0x88);
        c.set(ADDRESS);
        c.get(RESULT);
        scalar::truncate(c, bits);
        if signed {
            scalar::sign_extend(c, bits);
        }
        c.get(RESULT);
    }
    c.op(0x52);
    c.set(STATUS);
    flags::replace(c, 0x801, known);
    if implicit {
        scalar::write_gpr(c, 0, if bits == 8 { 16 } else { bits });
        if bits != 8 {
            c.get(ADDRESS);
            c.set(RESULT);
            scalar::write_gpr(c, 2, bits);
        }
    } else {
        scalar::write(c, i, 0, context);
    }
}
