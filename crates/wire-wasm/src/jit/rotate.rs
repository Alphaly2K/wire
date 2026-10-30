//! Rotate-through-carry uses explicit guest carry bits, including the 65-bit
//! ring at qword width. A zero masked count keeps the incoming FLAGS recipe.
use super::*;
use iced_x86::Mnemonic as M;

pub(super) fn emit(
    c: &mut Code,
    i: &Instruction,
    known: &mut Option<flags::Recipe>,
    context: access::Context,
) {
    let bits = scalar::bits(i, 0);
    let carry = matches!(i.mnemonic(), M::Rcl | M::Rcr);
    let left = matches!(i.mnemonic(), M::Rol | M::Rcl);
    c.get(B);
    c.i64(if bits == 64 { 63 } else { 31 });
    c.op(0x83);
    c.set(COUNT);
    if carry {
        flags::bit(c, 1, *known);
        c.op(0xad);
        c.set(EXTRA);
        c.get(COUNT);
        if bits < 32 {
            c.i64((bits + 1) as u64);
            c.op(0x82);
        }
    } else {
        c.get(COUNT);
        if bits < 32 {
            c.i64((bits - 1) as u64);
            c.op(0x83);
        }
    }
    c.set(B);
    c.get(A);
    c.set(RESULT);
    c.get(B);
    c.i64(0);
    c.op(0x52);
    c.if_void();
    c.get(A);
    c.get(B);
    c.op(if left { 0x86 } else { 0x88 });
    if carry {
        c.get(EXTRA);
        if left {
            c.get(B);
            c.i64(1);
            c.op(0x7d);
        } else {
            c.i64(bits as u64);
            c.get(B);
            c.op(0x7d);
        }
        c.op(0x86);
        c.op(0x84);
        c.set(RESULT);
        // At n=1 this contribution has a 64-bit shift for qword rotates.
        // Omitting it avoids Wasm reducing that shift to zero.
        c.get(B);
        c.i64(1);
        c.op(0x56);
        c.if_void();
        c.get(RESULT);
        c.get(A);
        c.i64((bits + 1) as u64);
        c.get(B);
        c.op(0x7d);
        c.op(if left { 0x88 } else { 0x86 });
        c.op(0x84);
        c.set(RESULT);
        c.end();
    } else {
        c.get(A);
        c.i64(bits as u64);
        c.get(B);
        c.op(0x7d);
        c.op(if left { 0x88 } else { 0x86 });
        c.op(0x84);
        c.set(RESULT);
    }
    c.end();
    c.get(RESULT);
    scalar::truncate(c, bits);
    c.set(RESULT);
    scalar::write(c, i, 0, context);
    c.get(if carry { B } else { COUNT });
    c.i64(0);
    c.op(0x52);
    c.if_void();
    if carry {
        c.get(A);
        if left {
            c.i64(bits as u64);
            c.get(B);
            c.op(0x7d);
        } else {
            c.get(B);
            c.i64(1);
            c.op(0x7d);
        }
        c.op(0x88);
    } else {
        c.get(RESULT);
        if !left {
            c.i64((bits - 1) as u64);
            c.op(0x88);
        }
    }
    c.i64(1);
    c.op(0x83);
    c.op(0xa7);
    c.set(STATUS);
    flags::replace(c, 1, known);
    c.get(COUNT);
    c.i64(1);
    c.op(0x51);
    c.if_void();
    c.get(RESULT);
    c.i64((bits - 1) as u64);
    c.op(0x88);
    if left {
        c.get(STATUS);
        c.op(0xad);
    } else {
        c.get(RESULT);
        c.i64((bits - 2) as u64);
        c.op(0x88);
    }
    c.op(0x85);
    c.i64(1);
    c.op(0x83);
    c.op(0xa7);
    c.set(STATUS);
    flags::replace(c, 2048, known);
    c.end();
    c.end();
    *known = None;
}
