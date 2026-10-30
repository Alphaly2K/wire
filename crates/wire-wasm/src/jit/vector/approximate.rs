//! The legacy reciprocal instructions do not observe or change MXCSR. Native
//! Wasm division/sqrt exceed their required accuracy; class handling is explicit.
use super::*;

pub(super) fn supported(i: &Instruction) -> bool {
    matches!(i.mnemonic(), M::Rcpps | M::Rcpss | M::Rsqrtps | M::Rsqrtss)
        && xmm(i, 0)
        && (xmm(i, 1) || memory(i, 1))
}

fn sign(c: &mut Code) {
    c.get(TRANSLATION);
    c.i32(0x8000_0000);
    c.op(0x71);
}
fn magnitude(c: &mut Code) {
    c.get(TRANSLATION);
    c.i32(0x7fff_ffff);
    c.op(0x71);
}

fn ordinary(c: &mut Code, square_root: bool) {
    magnitude(c);
    c.i32(0x7f80_0000);
    c.op(0x46);
    c.if_i32();
    sign(c);
    c.otherwise();
    c.i32(0x3f80_0000);
    c.op(0xbe);
    c.get(TRANSLATION);
    c.op(0xbe);
    if square_root {
        c.op(0x91);
    }
    c.op(0x95);
    c.op(0xbc);
    c.set(POINTER);
    c.get(POINTER);
    c.i32(0x7f80_0000);
    c.op(0x71);
    c.op(0x45);
    c.if_i32();
    sign(c);
    c.otherwise();
    c.get(POINTER);
    c.end();
    c.end();
}

fn reciprocal(c: &mut Code, square_root: bool) {
    magnitude(c);
    c.i32(0x7f80_0000);
    c.op(0x4b);
    c.if_i32();
    c.get(TRANSLATION);
    c.i32(0x0040_0000);
    c.op(0x72);
    c.otherwise();
    c.get(TRANSLATION);
    c.i32(0x7f80_0000);
    c.op(0x71);
    c.op(0x45);
    c.if_i32();
    sign(c);
    c.i32(0x7f80_0000);
    c.op(0x72);
    c.otherwise();
    if square_root {
        sign(c);
        c.if_i32();
        c.i32(0xffc0_0000);
        c.otherwise();
        ordinary(c, true);
        c.end();
    } else {
        ordinary(c, false);
    }
    c.end();
    c.end();
}

pub(super) fn emit(c: &mut Code, i: &Instruction, context: access::Context, bank: Bank) {
    let scalar = matches!(i.mnemonic(), M::Rcpss | M::Rsqrtss);
    if scalar && memory(i, 1) {
        prepare(c, i, context, 4);
        c.vector_const([0; 16]);
        context.load(c, i.ip(), 4, READ);
        lane(c, true, 0);
    } else {
        source(c, i, context, bank);
    }
    c.set(bank.scratch());
    if scalar {
        c.get(bank.local(register(i, 0)));
    } else {
        c.vector_const([0; 16]);
    }
    for index in 0..if scalar { 1 } else { 4 } {
        c.get(bank.scratch());
        c.simd(0x1b);
        c.op(index);
        c.set(TRANSLATION);
        reciprocal(c, matches!(i.mnemonic(), M::Rsqrtps | M::Rsqrtss));
        c.simd(0x1c);
        c.op(index);
    }
    c.set(bank.local(register(i, 0)));
}
