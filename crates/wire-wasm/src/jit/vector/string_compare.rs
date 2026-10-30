//! SSE4.2 string aggregation over SIMD comparisons. The control byte selects
//! emitted operations; no guest instruction selector remains at execution time.
use super::*;

pub(super) fn supported(i: &Instruction) -> bool {
    matches!(
        i.mnemonic(),
        M::Pcmpestri | M::Pcmpestri64 | M::Pcmpestrm | M::Pcmpestrm64 | M::Pcmpistri | M::Pcmpistrm
    ) && xmm(i, 0)
        && vector_source(i, 1)
        && i.op2_kind() == OpKind::Immediate8
}
pub(super) fn masked(i: &Instruction) -> bool {
    matches!(i.mnemonic(), M::Pcmpestrm | M::Pcmpestrm64 | M::Pcmpistrm)
}

fn length(c: &mut Code, vector: u32, local: u32, capacity: u32, explicit: Option<(u32, bool)>) {
    if let Some((register, wide)) = explicit {
        c.get(GPR + register);
        if !wide {
            c.op(0xa7);
            c.op(0xac);
        }
        c.set(COUNT);
        c.i64(0);
        c.get(COUNT);
        c.op(0x7d);
        c.get(COUNT);
        c.get(COUNT);
        c.i64(0);
        c.op(0x53);
        c.op(0x1b);
        c.set(COUNT);
        // Unsigned saturation also handles the absolute value of INT64_MIN.
        c.i64(capacity as u64);
        c.get(COUNT);
        c.get(COUNT);
        c.i64(capacity as u64);
        c.op(0x56);
        c.op(0x1b);
    } else {
        c.get(vector);
        c.vector_const([0; 16]);
        c.simd(if capacity == 16 { 0x23 } else { 0x2d });
        c.simd(if capacity == 16 { 0x64 } else { 0x84 });
        c.i32(1 << capacity);
        c.op(0x72);
        c.op(0x68);
        c.op(0xad);
    }
    c.set(local);
}

fn valid(c: &mut Code, local: u32) {
    c.i32(1);
    c.get(local);
    c.op(0xa7);
    c.op(0x74);
    c.i32(1);
    c.op(0x6b);
}

fn broadcast(c: &mut Code, vector: u32, index: u32, capacity: u32) {
    c.get(vector);
    c.simd(if capacity == 16 { 0x16 } else { 0x19 });
    c.op(index as u8);
    c.simd(if capacity == 16 { 0x0f } else { 0x10 });
}

fn compare(c: &mut Code, left: u32, right: u32, capacity: u32, kind: u8, signed: bool) {
    let all = (1 << capacity) - 1;
    valid(c, B);
    c.set(TRANSLATION);
    let eq = if capacity == 16 { 0x23 } else { 0x2d };
    let bitmask = if capacity == 16 { 0x64 } else { 0x84 };
    if kind == 2 {
        valid(c, A);
        c.set(POINTER);
        c.get(left);
        c.get(right);
        c.simd(eq);
        c.simd(bitmask);
        c.get(POINTER);
        c.op(0x71);
        c.get(TRANSLATION);
        c.op(0x71);
        c.get(POINTER);
        c.get(TRANSLATION);
        c.op(0x72);
        c.i32(all);
        c.op(0x73);
        c.op(0x72);
        c.set(STATUS);
        return;
    }
    c.i32(if kind == 3 { all } else { 0 });
    c.set(STATUS);
    for index in (0..capacity).step_by(if kind == 1 { 2 } else { 1 }) {
        c.get(A);
        c.i64((index + u32::from(kind == 1)) as u64);
        c.op(0x56);
        c.if_void();
        c.get(STATUS);
        broadcast(c, left, index, capacity);
        c.get(right);
        if kind == 1 {
            let le = if capacity == 16 { 0x29 } else { 0x33 } + u32::from(!signed);
            let ge = le + 2;
            c.simd(le);
            broadcast(c, left, index + 1, capacity);
            c.get(right);
            c.simd(ge);
            c.simd(0x4e);
        } else {
            c.simd(eq);
        }
        c.simd(bitmask);
        if kind == 3 {
            c.get(TRANSLATION);
            c.op(0x71);
            if index != 0 {
                c.i32(index);
                c.op(0x76);
                // The ordered diagonal stops at the physical vector edge.
                c.i32(all ^ ((1 << (capacity - index)) - 1));
                c.op(0x72);
            }
            c.op(0x71);
        } else {
            c.op(0x72);
        }
        c.set(STATUS);
        c.end();
    }
    if kind != 3 {
        c.get(STATUS);
        c.get(TRANSLATION);
        c.op(0x71);
        c.set(STATUS);
    }
}

fn mask(c: &mut Code, bank: Bank, capacity: u32, expanded: bool) {
    if expanded {
        c.get(STATUS);
        c.simd(0x10);
        if capacity == 16 {
            c.set(bank.scratch());
            c.get(bank.scratch());
            c.get(bank.scratch());
            shuffle(c, std::array::from_fn(|n| u8::from(n >= 8)));
        }
        c.vector_const(std::array::from_fn(|n| {
            if capacity == 16 {
                1 << (n % 8)
            } else {
                ((1u16 << (n / 2)) >> (n % 2 * 8)) as u8
            }
        }));
        c.simd(0x4e);
        c.vector_const([0; 16]);
        c.simd(if capacity == 16 { 0x23 } else { 0x2d });
        c.simd(0x4d);
    } else {
        c.vector_const([0; 16]);
        c.get(STATUS);
        c.op(0xad);
        lane(c, true, 0);
    }
    c.set(bank.local(0));
}

pub(super) fn emit(
    c: &mut Code,
    i: &Instruction,
    known: &mut Option<flags::Recipe>,
    context: access::Context,
    bank: Bank,
) {
    source(c, i, context, bank);
    c.set(bank.scratch());
    let capacity = if i.immediate8() & 1 == 0 { 16 } else { 8 };
    let explicit = matches!(
        i.mnemonic(),
        M::Pcmpestri | M::Pcmpestri64 | M::Pcmpestrm | M::Pcmpestrm64
    );
    let wide = matches!(
        i.code(),
        iced_x86::Code::Pcmpestri64_xmm_xmmm128_imm8 | iced_x86::Code::Pcmpestrm64_xmm_xmmm128_imm8
    );
    let left = bank.local(register(i, 0));
    let right = bank.scratch();
    length(c, left, A, capacity, explicit.then_some((0, wide)));
    length(c, right, B, capacity, explicit.then_some((2, wide)));
    compare(
        c,
        left,
        right,
        capacity,
        (i.immediate8() >> 2) & 3,
        i.immediate8() & 2 != 0,
    );
    match (i.immediate8() >> 4) & 3 {
        1 | 3 => {
            c.get(STATUS);
            if i.immediate8() & 0x20 == 0 {
                c.i32((1 << capacity) - 1);
            } else {
                c.get(TRANSLATION);
            }
            c.op(0x73);
            c.set(STATUS);
        }
        _ => {}
    }
    if masked(i) {
        mask(c, bank, capacity, i.immediate8() & 0x40 != 0);
    } else {
        if i.immediate8() & 0x40 == 0 {
            c.get(STATUS);
            c.i32(1 << capacity);
            c.op(0x72);
            c.op(0x68);
        } else {
            c.get(STATUS);
            c.if_i32();
            c.i32(31);
            c.get(STATUS);
            c.op(0x67);
            c.op(0x6b);
            c.otherwise();
            c.i32(capacity);
            c.end();
        }
        c.op(0xad);
        c.set(GPR + 1);
    }
    c.get(STATUS);
    c.op(0x45);
    c.op(0x45);
    c.op(0xad);
    for (local, shift) in [(B, 6), (A, 7)] {
        c.get(local);
        c.i64(capacity as u64);
        c.op(0x54);
        c.op(0xad);
        c.i64(shift);
        c.op(0x86);
        c.op(0x84);
    }
    c.get(STATUS);
    c.i32(1);
    c.op(0x71);
    c.op(0xad);
    c.i64(11);
    c.op(0x86);
    c.op(0x84);
    c.set(RESULT);
    c.get(FLAGS);
    c.i64(!0x8d5);
    c.op(0x83);
    c.get(RESULT);
    c.op(0x84);
    c.set(FLAGS);
    flags::discard(c);
    *known = None;
}
