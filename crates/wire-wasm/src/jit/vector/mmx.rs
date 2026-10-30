//! MMX uses physical x87 payloads, with tag/TOP transitions after successful
//! operand access. Integer operations remain in generated Wasm SIMD.
use super::*;

pub(super) fn register_operand(i: &Instruction, index: u32) -> bool {
    i.op_kind(index) == OpKind::Register
        && (Register::MM0..=Register::MM7).contains(&i.op_register(index))
}
fn source_operand(i: &Instruction, index: u32) -> bool {
    register_operand(i, index) || memory(i, index)
}
pub(super) fn supported(i: &Instruction) -> bool {
    if i.has_lock_prefix() {
        return false;
    }
    let m = i.mnemonic();
    if m == M::Emms {
        return true;
    }
    if m == M::Movdq2q {
        return register_operand(i, 0) && xmm(i, 1);
    }
    if m == M::Movq2dq {
        return xmm(i, 0) && register_operand(i, 1);
    }
    if !(0..i.op_count()).any(|n| register_operand(i, n)) {
        return false;
    }
    if matches!(m, M::Movd | M::Movq) {
        let operand = |n| {
            register_operand(i, n) || scalar::register_operand(i, n) || scalar::memory_operand(i, n)
        };
        return operand(0) && operand(1);
    }
    if m == M::Movntq {
        return memory(i, 0) && register_operand(i, 1);
    }
    if m == M::Pinsrw {
        return register_operand(i, 0)
            && (scalar::register_operand(i, 1) || scalar::memory_operand(i, 1))
            && i.op2_kind() == OpKind::Immediate8;
    }
    if m == M::Pextrw {
        return scalar::register_operand(i, 0)
            && register_operand(i, 1)
            && i.op2_kind() == OpKind::Immediate8;
    }
    if m == M::Pmovmskb {
        return scalar::register_operand(i, 0) && register_operand(i, 1);
    }
    if shift(m).is_some() {
        return register_operand(i, 0)
            && (source_operand(i, 1) || i.op1_kind() == OpKind::Immediate8);
    }
    if matches!(m, M::Pshufw | M::Palignr) {
        return register_operand(i, 0)
            && source_operand(i, 1)
            && i.op2_kind() == OpKind::Immediate8;
    }
    (binary_op(m).is_some()
        || unpack(m).is_some()
        || matches!(
            m,
            M::Pmulhw
                | M::Pmulhuw
                | M::Pmuludq
                | M::Pmaddwd
                | M::Pmaddubsw
                | M::Pmulhrsw
                | M::Psadbw
                | M::Phaddw
                | M::Phaddd
                | M::Phaddsw
                | M::Phsubw
                | M::Phsubd
                | M::Phsubsw
                | M::Psignb
                | M::Psignw
                | M::Psignd
                | M::Pabsb
                | M::Pabsw
                | M::Pabsd
                | M::Pshufb
        ))
        && register_operand(i, 0)
        && source_operand(i, 1)
}

fn low(c: &mut Code) {
    c.vector_const([
        255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0,
    ]);
    c.simd(0x4e);
}
fn value(c: &mut Code, i: &Instruction, index: u32, bank: Bank) {
    c.get(bank.local(register(i, index)));
    low(c);
}
fn prepare(c: &mut Code, i: &Instruction, context: access::Context, size: u32) {
    context.prepare(c, i, size, memory_access(i));
}
fn source(
    c: &mut Code,
    i: &Instruction,
    index: u32,
    context: access::Context,
    bank: Bank,
    size: u32,
) {
    if register_operand(i, index) || xmm(i, index) {
        value(c, i, index, bank);
        return;
    }
    if memory(i, index) {
        prepare(c, i, context, size);
    }
    c.vector_const([0; 16]);
    if memory(i, index) {
        context.load(c, i.ip(), size, READ);
    } else {
        scalar::read(c, i, index, context);
    }
    if size == 4 {
        scalar::truncate(c, 32);
    }
    lane(c, true, 0);
}
pub(super) fn commit(c: &mut Code, i: &Instruction, bank: Bank) {
    low(c);
    c.vector_const([0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 0, 0, 0, 0, 0, 0]);
    c.simd(0x50);
    c.set(bank.local(register(i, 0)));
}
pub(super) fn effects(c: &mut Code, i: &Instruction, bank: Bank) {
    c.i32(if i.mnemonic() == M::Emms { 0xffff } else { 0 });
    c.set(bank.ftag());
    if i.mnemonic() != M::Emms {
        c.get(bank.fsw());
        c.i32(!0x3800);
        c.op(0x71);
        c.set(bank.fsw());
    }
}
pub(super) fn wait(c: &mut Code, i: &Instruction, context: access::Context, bank: Bank) {
    c.get(bank.fsw());
    c.get(bank.fcw());
    c.i32(u32::MAX);
    c.op(0x73);
    c.op(0x71);
    c.i32(0x3f);
    c.op(0x71);
    c.if_void();
    exception(c, context.out, i.ip(), 16);
    c.end();
}
fn horizontal(c: &mut Code, m: M, dest: u32, a: u32) {
    let size = if matches!(m, M::Phaddd | M::Phsubd) {
        4
    } else {
        2
    };
    for odd in [false, true] {
        c.get(dest);
        c.get(a);
        shuffle(
            c,
            std::array::from_fn(|n| {
                let n = n % 8;
                (n % 4 / size * size * 2
                    + n % size
                    + usize::from(odd) * size
                    + if n >= 4 { 16 } else { 0 }) as u8
            }),
        );
    }
    c.simd(match m {
        M::Phaddw => 0x8e,
        M::Phaddd => 0xae,
        M::Phaddsw => 0x8f,
        M::Phsubw => 0x91,
        M::Phsubd => 0xb1,
        _ => 0x92,
    });
}

pub(super) fn emit(c: &mut Code, i: &Instruction, context: access::Context, bank: Bank) {
    wait(c, i, context, bank);
    let m = i.mnemonic();
    if m == M::Emms {
        effects(c, i, bank);
        return;
    }
    if m == M::Movq2dq {
        value(c, i, 1, bank);
        c.set(bank.local(register(i, 0)));
        effects(c, i, bank);
        return;
    }
    if m == M::Movdq2q {
        value(c, i, 1, bank);
        commit(c, i, bank);
        effects(c, i, bank);
        return;
    }
    if matches!(m, M::Movd | M::Movq | M::Movntq) {
        let size = if m == M::Movd { 4 } else { 8 };
        if register_operand(i, 0) {
            source(c, i, 1, context, bank, size);
            commit(c, i, bank);
        } else {
            if memory(i, 0) {
                prepare(c, i, context, size);
            }
            value(c, i, 1, bank);
            lane(c, false, 0);
            c.set(RESULT);
            if memory(i, 0) {
                context.store(c, i.ip(), size);
            } else {
                scalar::write(c, i, 0, context);
            }
        }
        effects(c, i, bank);
        return;
    }
    if m == M::Pinsrw {
        source(c, i, 1, context, bank, 2);
        lane(c, false, 0);
        c.set(RESULT);
        value(c, i, 0, bank);
        c.get(RESULT);
        c.op(0xa7);
        c.simd(0x1a);
        c.op(i.immediate8() & 3);
        commit(c, i, bank);
        effects(c, i, bank);
        return;
    }
    if matches!(m, M::Pextrw | M::Pmovmskb) {
        value(c, i, 1, bank);
        if m == M::Pextrw {
            c.simd(0x19);
            c.op(i.immediate8() & 3);
        } else {
            c.simd(0x64);
        }
        c.op(0xad);
        c.set(RESULT);
        scalar::write(c, i, 0, context);
        effects(c, i, bank);
        return;
    }
    let dest = bank.local(register(i, 0));
    let a = bank.scratch();
    if let Some((bits, opcode, arithmetic)) = shift(m) {
        if i.op1_kind() == OpKind::Immediate8 {
            c.i64(i.immediate8() as u64);
        } else {
            source(c, i, 1, context, bank, 8);
            lane(c, false, 0);
        }
        c.set(COUNT);
        c.get(COUNT);
        c.i64(bits as u64);
        c.op(0x5a);
        c.open(0x04, 0x7b);
        if arithmetic {
            c.get(dest);
            c.i32(bits - 1);
            c.simd(opcode);
        } else {
            c.vector_const([0; 16]);
        }
        c.otherwise();
        c.get(dest);
        c.get(COUNT);
        c.op(0xa7);
        c.simd(opcode);
        c.end();
    } else {
        // Low MMX unpack consumes m32; its register source remains 64 bits.
        let size = if matches!(m, M::Punpcklbw | M::Punpcklwd | M::Punpckldq) {
            4
        } else {
            8
        };
        source(c, i, 1, context, bank, size);
        c.set(a);
        if let Some(opcode) = binary_op(m) {
            let invert = m == M::Pandn;
            c.get(if invert { a } else { dest });
            c.get(if invert { dest } else { a });
            c.simd(opcode);
            if matches!(m, M::Packsswb | M::Packuswb | M::Packssdw) {
                c.vector_const([0; 16]);
                shuffle(
                    c,
                    std::array::from_fn(|n| {
                        if n >= 8 {
                            16
                        } else {
                            (n % 4 + n / 4 * 8) as u8
                        }
                    }),
                );
            }
        } else if let Some((size, high)) = unpack(m) {
            c.get(dest);
            c.get(a);
            shuffle(
                c,
                std::array::from_fn(|n| {
                    let n = n as u32 % 8;
                    let unit = n / size;
                    (unit / 2 * size
                        + n % size
                        + if high { 4 } else { 0 }
                        + if unit & 1 != 0 { 16 } else { 0 }) as u8
                }),
            );
        } else {
            match m {
                M::Pshufw => {
                    c.get(a);
                    c.get(a);
                    shuffle(
                        c,
                        std::array::from_fn(|n| {
                            ((i.immediate8() >> (2 * (n / 2 % 4))) & 3) * 2 + (n % 2) as u8
                        }),
                    );
                }
                M::Pshufb => {
                    c.get(dest);
                    low(c);
                    c.get(a);
                    c.vector_const([0x87; 16]);
                    c.simd(0x4e);
                    c.simd(14);
                }
                M::Palignr => {
                    let count = i.immediate8() as usize;
                    c.get(a);
                    c.get(dest);
                    shuffle(
                        c,
                        std::array::from_fn(|n| if n < 8 { n as u8 } else { 16 + (n - 8) as u8 }),
                    );
                    // Build the 16-byte concatenation before its byte shift.
                    c.set(a);
                    c.get(a);
                    c.vector_const([0; 16]);
                    shuffle(
                        c,
                        std::array::from_fn(|n| if n < 8 { (n + count).min(16) as u8 } else { 16 }),
                    );
                }
                M::Pabsb | M::Pabsw | M::Pabsd => {
                    c.get(a);
                    c.simd(match m {
                        M::Pabsb => 0x60,
                        M::Pabsw => 0x80,
                        _ => 0xa0,
                    });
                }
                M::Pmulhw | M::Pmulhuw | M::Pmulhrsw => {
                    c.get(dest);
                    c.get(a);
                    c.simd(if m == M::Pmulhuw { 0xbe } else { 0xbc });
                    if m == M::Pmulhrsw {
                        c.vector_const(std::array::from_fn(|n| if n % 4 == 1 { 0x40 } else { 0 }));
                        c.simd(0xae);
                        c.i32(15);
                        c.simd(0xac);
                    }
                    c.vector_const([0; 16]);
                    let start = if m == M::Pmulhrsw { 0 } else { 2 };
                    shuffle(
                        c,
                        std::array::from_fn(|n| {
                            if n < 8 {
                                (n / 2 * 4 + n % 2 + start) as u8
                            } else {
                                16
                            }
                        }),
                    );
                }
                M::Pmuludq => {
                    c.get(dest);
                    c.get(a);
                    c.simd(0xde);
                }
                M::Pmaddwd => {
                    c.get(dest);
                    c.get(a);
                    c.simd(0xba);
                }
                M::Pmaddubsw => {
                    c.get(dest);
                    c.simd(0x89);
                    c.get(a);
                    c.simd(0x87);
                    c.simd(0x95);
                    c.simd(0x7e);
                    c.vector_const([0; 16]);
                    c.simd(0x85);
                }
                M::Psadbw => {
                    value(c, i, 0, bank);
                    c.get(a);
                    c.simd(0x73);
                    c.get(a);
                    value(c, i, 0, bank);
                    c.simd(0x73);
                    c.simd(0x50);
                    c.simd(0x7d);
                    c.simd(0x7f);
                    c.set(a);
                    c.vector_const([0; 16]);
                    c.get(a);
                    c.simd(0x1b);
                    c.op(0);
                    c.get(a);
                    c.simd(0x1b);
                    c.op(1);
                    c.op(0x6a);
                    c.op(0xad);
                    lane(c, true, 0);
                }
                M::Phaddw | M::Phaddd | M::Phaddsw | M::Phsubw | M::Phsubd | M::Phsubsw => {
                    horizontal(c, m, dest, a)
                }
                M::Psignb | M::Psignw | M::Psignd => packed::sign(c, m, dest, a),
                _ => unreachable!(),
            }
        }
    }
    commit(c, i, bank);
    effects(c, i, bank);
}
