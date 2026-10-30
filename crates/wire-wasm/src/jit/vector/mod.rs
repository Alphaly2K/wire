//! Legacy XMM instructions emitted directly as Wasm SIMD. Floating-point
//! arithmetic has a separate exact numerical contract; these operations are bits.
use super::*;
use crate::memory::{READ, WRITE};
use iced_x86::{Mnemonic as M, OpKind, Register};

mod approximate;
pub(super) mod bank;
pub(super) mod floating;
mod masked;
mod mmx;
mod packed;
mod string_compare;
pub(super) mod x87;
pub(super) use bank::Bank;

fn xmm(i: &Instruction, index: u32) -> bool {
    i.op_kind(index) == OpKind::Register
        && (Register::XMM0..=Register::XMM15).contains(&i.op_register(index))
}
fn register(i: &Instruction, index: u32) -> u32 {
    if mmx::register_operand(i, index) {
        16 + i.op_register(index) as u32 - Register::MM0 as u32
    } else {
        i.op_register(index) as u32 - Register::XMM0 as u32
    }
}
fn memory(i: &Instruction, index: u32) -> bool {
    i.op_kind(index) == OpKind::Memory
}
fn vector_source(i: &Instruction, index: u32) -> bool {
    xmm(i, index) || memory(i, index) && i.memory_size().size() == 16
}

fn binary_op(m: M) -> Option<u32> {
    Some(match m {
        M::Pand | M::Andps | M::Andpd => 0x4e,
        M::Pandn | M::Andnps | M::Andnpd => 0x4f,
        M::Por | M::Orps | M::Orpd => 0x50,
        M::Pxor | M::Xorps | M::Xorpd => 0x51,
        M::Paddb => 0x6e,
        M::Paddw => 0x8e,
        M::Paddd => 0xae,
        M::Paddq => 0xce,
        M::Psubb => 0x71,
        M::Psubw => 0x91,
        M::Psubd => 0xb1,
        M::Psubq => 0xd1,
        M::Paddsb => 0x6f,
        M::Paddsw => 0x8f,
        M::Paddusb => 0x70,
        M::Paddusw => 0x90,
        M::Psubsb => 0x72,
        M::Psubsw => 0x92,
        M::Psubusb => 0x73,
        M::Psubusw => 0x93,
        M::Pcmpeqb => 0x23,
        M::Pcmpeqw => 0x2d,
        M::Pcmpeqd => 0x37,
        M::Pcmpeqq => 0xd6,
        M::Pcmpgtb => 0x27,
        M::Pcmpgtw => 0x31,
        M::Pcmpgtd => 0x3b,
        M::Pcmpgtq => 0xd9,
        M::Pminsb => 0x76,
        M::Pminub => 0x77,
        M::Pmaxsb => 0x78,
        M::Pmaxub => 0x79,
        M::Pminsw => 0x96,
        M::Pminuw => 0x97,
        M::Pmaxsw => 0x98,
        M::Pmaxuw => 0x99,
        M::Pminsd => 0xb6,
        M::Pminud => 0xb7,
        M::Pmaxsd => 0xb8,
        M::Pmaxud => 0xb9,
        M::Pavgb => 0x7b,
        M::Pavgw => 0x9b,
        M::Pmullw => 0x95,
        M::Pmulld => 0xb5,
        M::Packsswb => 0x65,
        M::Packuswb => 0x66,
        M::Packssdw => 0x85,
        M::Packusdw => 0x86,
        _ => return None,
    })
}

fn shift(m: M) -> Option<(u32, u32, bool)> {
    Some(match m {
        M::Psllw => (16, 0x8b, false),
        M::Pslld => (32, 0xab, false),
        M::Psllq => (64, 0xcb, false),
        M::Psrlw => (16, 0x8d, false),
        M::Psrld => (32, 0xad, false),
        M::Psrlq => (64, 0xcd, false),
        M::Psraw => (16, 0x8c, true),
        M::Psrad => (32, 0xac, true),
        _ => return None,
    })
}

fn unpack(m: M) -> Option<(u32, bool)> {
    Some(match m {
        M::Punpcklbw => (1, false),
        M::Punpckhbw => (1, true),
        M::Punpcklwd => (2, false),
        M::Punpckhwd => (2, true),
        M::Punpckldq | M::Unpcklps => (4, false),
        M::Punpckhdq | M::Unpckhps => (4, true),
        M::Punpcklqdq | M::Unpcklpd => (8, false),
        M::Punpckhqdq | M::Unpckhpd => (8, true),
        _ => return None,
    })
}

pub(super) fn supported(i: &Instruction) -> bool {
    if x87::supported(i) {
        return true;
    }
    if floating::supported(i) {
        return true;
    }
    if i.has_lock_prefix() {
        return false;
    }
    if masked_store(i) {
        return true;
    }
    if mmx::supported(i) {
        return true;
    }
    if !(0..i.op_count()).any(|index| xmm(i, index)) {
        return false;
    }
    if packed::supported(i) || approximate::supported(i) || string_compare::supported(i) {
        return true;
    }
    let m = i.mnemonic();
    if binary_op(m).is_some() || unpack(m).is_some() || m == M::Pshufb {
        return xmm(i, 0) && vector_source(i, 1);
    }
    if shift(m).is_some() {
        return xmm(i, 0) && (i.op1_kind() == OpKind::Immediate8 || vector_source(i, 1));
    }
    match m {
        M::Movdqa | M::Movdqu | M::Movaps | M::Movups | M::Movapd | M::Movupd => {
            xmm(i, 0) && vector_source(i, 1)
                || memory(i, 0) && xmm(i, 1) && i.memory_size().size() == 16
        }
        M::Lddqu | M::Movntdqa => xmm(i, 0) && memory(i, 1),
        M::Movntdq | M::Movntps | M::Movntpd => memory(i, 0) && xmm(i, 1),
        M::Movd | M::Movq => {
            let other = |index| {
                xmm(i, index)
                    || scalar::register_operand(i, index)
                    || scalar::memory_operand(i, index)
            };
            other(0) && other(1)
        }
        M::Movss | M::Movsd => {
            (xmm(i, 0) && (xmm(i, 1) || memory(i, 1))) || memory(i, 0) && xmm(i, 1)
        }
        M::Movhlps | M::Movlhps => xmm(i, 0) && xmm(i, 1),
        M::Movlps | M::Movhps | M::Movlpd | M::Movhpd => {
            xmm(i, 0) && memory(i, 1) || memory(i, 0) && xmm(i, 1)
        }
        M::Pshufd | M::Pshufhw | M::Pshuflw | M::Shufps | M::Shufpd => {
            xmm(i, 0) && vector_source(i, 1) && i.op2_kind() == OpKind::Immediate8
        }
        M::Pslldq | M::Psrldq => xmm(i, 0) && i.op1_kind() == OpKind::Immediate8,
        M::Pmovmskb | M::Movmskps | M::Movmskpd => scalar::register_operand(i, 0) && xmm(i, 1),
        _ => false,
    }
}

pub(super) fn memory_access(i: &Instruction) -> u8 {
    if x87::supported(i) {
        return x87::memory_access(i);
    }
    if masked_store(i) {
        return WRITE;
    }
    if floating::supported(i) {
        return floating::memory_access(i);
    }
    if memory(i, 0) {
        WRITE
    } else if memory(i, 1) {
        READ
    } else {
        0
    }
}

fn prepare(c: &mut Code, i: &Instruction, context: access::Context, size: u32) {
    context.address(c, i, true);
    c.set(ADDRESS);
    let unaligned = matches!(i.mnemonic(), M::Movdqu | M::Movups | M::Movupd | M::Lddqu)
        || string_compare::supported(i);
    if size == 16 && !unaligned {
        c.get(ADDRESS);
        c.i64(15);
        c.op(0x83);
        c.i64(0);
        c.op(0x52);
        c.if_void();
        exception(c, context.out, i.ip(), 13);
        c.end();
    }
    context.prepare_address(c, i.ip(), size, memory_access(i));
}

fn lane(c: &mut Code, replace: bool, index: u8) {
    c.simd(if replace { 0x1e } else { 0x1d });
    c.op(index);
}
fn shuffle(c: &mut Code, indices: [u8; 16]) {
    c.simd(13);
    c.bytes.extend(indices);
}

fn source(c: &mut Code, i: &Instruction, context: access::Context, bank: Bank) {
    if xmm(i, 1) {
        c.get(bank.local(register(i, 1)));
        return;
    }
    prepare(c, i, context, 16);
    c.get(POINTER);
    c.open(0x04, 0x7b);
    c.get(POINTER);
    c.vector_memory(0, 0);
    c.otherwise();
    c.vector_const([0; 16]);
    context.load(c, i.ip(), 8, READ);
    lane(c, true, 0);
    context.load_offset(c, i.ip(), 8, READ, 8);
    lane(c, true, 1);
    c.end();
}

fn store(c: &mut Code, i: &Instruction, context: access::Context, local: u32) {
    c.get(POINTER);
    c.if_void();
    c.get(POINTER);
    c.get(local);
    c.vector_memory(11, 0);
    c.otherwise();
    for (index, to) in [(0, COUNT), (1, EXTRA)] {
        c.get(local);
        lane(c, false, index);
        c.set(to);
    }
    context.store128(c, i.ip(), COUNT, EXTRA);
    c.end();
}

fn small_move(c: &mut Code, i: &Instruction, context: access::Context, bank: Bank) {
    let m = i.mnemonic();
    let size = if matches!(m, M::Movd | M::Movss) {
        4
    } else {
        8
    };
    if memory_access(i) != 0 {
        prepare(c, i, context, size);
    }
    if memory(i, 1) {
        context.load(c, i.ip(), size, READ);
    } else if xmm(i, 1) {
        c.get(bank.local(register(i, 1)));
        lane(
            c,
            false,
            u8::from(matches!(m, M::Movhlps | M::Movhps | M::Movhpd)),
        );
    } else {
        scalar::read(c, i, 1, context);
    }
    if size == 4 {
        scalar::truncate(c, 32);
    }
    c.set(RESULT);
    if xmm(i, 0) {
        let merge = matches!(
            m,
            M::Movlps | M::Movhps | M::Movlpd | M::Movhpd | M::Movhlps | M::Movlhps
        ) || matches!(m, M::Movss | M::Movsd) && xmm(i, 1);
        if merge {
            c.get(bank.local(register(i, 0)));
        } else {
            c.vector_const([0; 16]);
        }
        c.get(RESULT);
        if m == M::Movss {
            c.op(0xa7);
            c.simd(0x1c);
            c.op(0);
        } else {
            lane(
                c,
                true,
                u8::from(matches!(m, M::Movhps | M::Movhpd | M::Movlhps)),
            );
        }
        c.set(bank.local(register(i, 0)));
    } else if memory(i, 0) {
        context.store(c, i.ip(), size);
    } else {
        scalar::write(c, i, 0, context);
    }
}

pub(super) fn emit(
    c: &mut Code,
    i: &Instruction,
    known: &mut Option<flags::Recipe>,
    context: access::Context,
    bank: Bank,
    calls: floating::Calls,
    fp: x87::Calls,
) {
    if x87::supported(i) {
        x87::emit(c, i, known, context, bank, fp);
        return;
    }
    if masked_store(i) {
        masked::emit(c, i, context, bank);
        return;
    }
    if floating::supported(i) {
        floating::emit(c, i, known, context, bank, calls);
        return;
    }
    if mmx::supported(i) {
        mmx::emit(c, i, context, bank);
        return;
    }
    if approximate::supported(i) {
        approximate::emit(c, i, context, bank);
        return;
    }
    if string_compare::supported(i) {
        string_compare::emit(c, i, known, context, bank);
        return;
    }
    if packed::supported(i) {
        packed::emit(c, i, known, context, bank);
        return;
    }
    let m = i.mnemonic();
    let a = bank.scratch();
    if matches!(
        m,
        M::Movdqa
            | M::Movdqu
            | M::Movaps
            | M::Movups
            | M::Movapd
            | M::Movupd
            | M::Lddqu
            | M::Movntdqa
            | M::Movntdq
            | M::Movntps
            | M::Movntpd
    ) {
        if memory(i, 0) {
            prepare(c, i, context, 16);
            c.get(bank.local(register(i, 1)));
            c.set(a);
            store(c, i, context, a);
        } else {
            source(c, i, context, bank);
            c.set(bank.local(register(i, 0)));
        }
        return;
    }
    if matches!(
        m,
        M::Movd
            | M::Movq
            | M::Movss
            | M::Movsd
            | M::Movlps
            | M::Movhps
            | M::Movlpd
            | M::Movhpd
            | M::Movhlps
            | M::Movlhps
    ) {
        small_move(c, i, context, bank);
        return;
    }
    if matches!(m, M::Pmovmskb | M::Movmskps | M::Movmskpd) {
        c.get(bank.local(register(i, 1)));
        c.simd(match m {
            M::Pmovmskb => 0x64,
            M::Movmskps => 0xa4,
            _ => 0xc4,
        });
        c.op(0xad);
        c.set(RESULT);
        scalar::write(c, i, 0, context);
        return;
    }
    let dest = bank.local(register(i, 0));
    if matches!(m, M::Pslldq | M::Psrldq) {
        let count = i.immediate8().min(16);
        c.get(dest);
        c.vector_const([0; 16]);
        shuffle(
            c,
            std::array::from_fn(|index| {
                if m == M::Pslldq {
                    if index < count as usize {
                        16
                    } else {
                        index as u8 - count
                    }
                } else {
                    let at = index as u8 + count;
                    if at < 16 {
                        at
                    } else {
                        16
                    }
                }
            }),
        );
    } else if let Some((bits, opcode, arithmetic)) = shift(m) {
        if i.op1_kind() == OpKind::Immediate8 {
            c.i64(i.immediate8() as u64);
        } else {
            source(c, i, context, bank);
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
        source(c, i, context, bank);
        c.set(a);
        if let Some(opcode) = binary_op(m) {
            let invert = matches!(m, M::Pandn | M::Andnps | M::Andnpd);
            c.get(if invert { a } else { dest });
            c.get(if invert { dest } else { a });
            c.simd(opcode);
        } else if m == M::Pshufb {
            c.get(dest);
            c.get(a);
            c.vector_const([0x8f; 16]);
            c.simd(0x4e);
            c.simd(14);
        } else if let Some((size, high)) = unpack(m) {
            c.get(dest);
            c.get(a);
            shuffle(
                c,
                std::array::from_fn(|index| {
                    let unit = index as u32 / size;
                    ((unit / 2) * size
                        + index as u32 % size
                        + if high { 8 } else { 0 }
                        + if unit & 1 != 0 { 16 } else { 0 }) as u8
                }),
            );
        } else {
            let immediate = i.immediate8();
            let unary = matches!(m, M::Pshufd | M::Pshufhw | M::Pshuflw);
            c.get(if unary { a } else { dest });
            c.get(a);
            shuffle(
                c,
                std::array::from_fn(|index| match m {
                    M::Pshufd => ((immediate >> (2 * (index / 4))) & 3) * 4 + (index % 4) as u8,
                    M::Pshufhw | M::Pshuflw => {
                        let selected = if m == M::Pshufhw {
                            index >= 8
                        } else {
                            index < 8
                        };
                        if selected {
                            let base = if m == M::Pshufhw { 8 } else { 0 };
                            base + ((immediate >> (2 * ((index % 8) / 2))) & 3) * 2
                                + (index % 2) as u8
                        } else {
                            index as u8
                        }
                    }
                    M::Shufps => {
                        ((immediate >> (2 * (index / 4))) & 3) * 4
                            + (index % 4) as u8
                            + if index >= 8 { 16 } else { 0 }
                    }
                    M::Shufpd => {
                        ((immediate >> (index / 8)) & 1) * 8
                            + (index % 8) as u8
                            + if index >= 8 { 16 } else { 0 }
                    }
                    _ => unreachable!(),
                }),
            );
        }
    }
    c.set(dest);
}

pub(super) fn masked_store(i: &Instruction) -> bool {
    masked::supported(i)
}

pub(super) fn fp_area(i: &Instruction) -> bool {
    x87::supported(i) && x87::needs_area(i)
}
