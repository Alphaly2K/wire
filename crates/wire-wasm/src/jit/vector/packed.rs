//! Integer lanes and bit rearrangement with no numerical or CPU-state calls.
use super::*;

fn extension(m: M) -> Option<(u32, u32, bool)> {
    Some(match m {
        M::Pmovsxbw => (8, 16, true),
        M::Pmovsxbd => (8, 32, true),
        M::Pmovsxbq => (8, 64, true),
        M::Pmovsxwd => (16, 32, true),
        M::Pmovsxwq => (16, 64, true),
        M::Pmovsxdq => (32, 64, true),
        M::Pmovzxbw => (8, 16, false),
        M::Pmovzxbd => (8, 32, false),
        M::Pmovzxbq => (8, 64, false),
        M::Pmovzxwd => (16, 32, false),
        M::Pmovzxwq => (16, 64, false),
        M::Pmovzxdq => (32, 64, false),
        _ => return None,
    })
}

pub(super) fn mask_width(m: M) -> Option<u32> {
    match m {
        M::Pblendvb => Some(8),
        M::Blendvps => Some(32),
        M::Blendvpd => Some(64),
        _ => None,
    }
}
fn insert_size(m: M) -> Option<u32> {
    match m {
        M::Pinsrb => Some(1),
        M::Pinsrw => Some(2),
        M::Pinsrd => Some(4),
        M::Pinsrq => Some(8),
        _ => None,
    }
}
fn extract_size(m: M) -> Option<u32> {
    match m {
        M::Pextrb => Some(1),
        M::Pextrw => Some(2),
        M::Pextrd | M::Extractps => Some(4),
        M::Pextrq => Some(8),
        _ => None,
    }
}

pub(super) fn supported(i: &Instruction) -> bool {
    let m = i.mnemonic();
    if extension(m).is_some() || m == M::Movddup {
        return xmm(i, 0) && (xmm(i, 1) || memory(i, 1));
    }
    if insert_size(m).is_some() {
        return xmm(i, 0)
            && (scalar::register_operand(i, 1) || scalar::memory_operand(i, 1))
            && i.op2_kind() == OpKind::Immediate8;
    }
    if extract_size(m).is_some() {
        return (scalar::register_operand(i, 0) || scalar::memory_operand(i, 0))
            && xmm(i, 1)
            && i.op2_kind() == OpKind::Immediate8;
    }
    if m == M::Insertps {
        return xmm(i, 0) && (xmm(i, 1) || memory(i, 1)) && i.op2_kind() == OpKind::Immediate8;
    }
    if matches!(
        m,
        M::Palignr | M::Pblendw | M::Blendps | M::Blendpd | M::Mpsadbw
    ) {
        return xmm(i, 0) && vector_source(i, 1) && i.op2_kind() == OpKind::Immediate8;
    }
    if mask_width(m).is_some() {
        return xmm(i, 0) && vector_source(i, 1);
    }
    matches!(
        m,
        M::Pmulhw
            | M::Pmulhuw
            | M::Pmuludq
            | M::Pmuldq
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
            | M::Phminposuw
            | M::Ptest
            | M::Movsldup
            | M::Movshdup
    ) && xmm(i, 0)
        && vector_source(i, 1)
}

fn source(c: &mut Code, i: &Instruction, context: access::Context, bank: Bank) {
    let size = if let Some((from, to, _)) = extension(i.mnemonic()) {
        16 * from / to
    } else if i.mnemonic() == M::Movddup {
        8
    } else if i.mnemonic() == M::Insertps {
        4
    } else {
        16
    };
    if xmm(i, 1) || size == 16 {
        super::source(c, i, context, bank);
    } else {
        prepare(c, i, context, size);
        c.vector_const([0; 16]);
        context.load(c, i.ip(), size, READ);
        lane(c, true, 0);
    }
}

fn insert(c: &mut Code, i: &Instruction, context: access::Context, bank: Bank, size: u32) {
    if memory(i, 1) {
        prepare(c, i, context, size);
        context.load(c, i.ip(), size, READ);
    } else {
        scalar::read(c, i, 1, context);
    }
    c.set(RESULT);
    c.get(bank.local(register(i, 0)));
    c.get(RESULT);
    if size != 8 {
        c.op(0xa7);
    }
    c.simd(match size {
        1 => 0x17,
        2 => 0x1a,
        4 => 0x1c,
        _ => 0x1e,
    });
    c.op(i.immediate8() & (16 / size - 1) as u8);
    c.set(bank.local(register(i, 0)));
}

fn extract(c: &mut Code, i: &Instruction, context: access::Context, bank: Bank, size: u32) {
    if memory(i, 0) {
        prepare(c, i, context, size);
    }
    c.get(bank.local(register(i, 1)));
    c.simd(match size {
        1 => 0x16,
        2 => 0x19,
        4 => 0x1b,
        _ => 0x1d,
    });
    c.op(i.immediate8() & (16 / size - 1) as u8);
    if size != 8 {
        c.op(0xad);
    }
    c.set(RESULT);
    if memory(i, 0) {
        context.store(c, i.ip(), size);
    } else {
        scalar::write(c, i, 0, context);
    }
}

fn product(c: &mut Code, dest: u32, source: u32, unsigned: bool, high: bool) {
    c.get(dest);
    c.get(source);
    c.simd(0xbc + u32::from(unsigned) * 2 + u32::from(high));
}
fn even_dwords(c: &mut Code, value: u32) {
    c.get(value);
    c.get(value);
    shuffle(c, std::array::from_fn(|n| ((n / 4 % 2) * 8 + n % 4) as u8));
}

fn horizontal(c: &mut Code, m: M, dest: u32, source: u32) {
    let size = if matches!(m, M::Phaddd | M::Phsubd) {
        4
    } else {
        2
    };
    for odd in [false, true] {
        c.get(dest);
        c.get(source);
        shuffle(
            c,
            std::array::from_fn(|n| {
                ((n / size) * size * 2 + n % size + usize::from(odd) * size) as u8
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

pub(super) fn sign(c: &mut Code, m: M, dest: u32, source: u32) {
    let (negative, greater, negate) = match m {
        M::Psignb => (0x25, 0x27, 0x61),
        M::Psignw => (0x2f, 0x31, 0x81),
        _ => (0x39, 0x3b, 0xa1),
    };
    c.get(dest);
    c.get(source);
    c.vector_const([0; 16]);
    c.simd(greater);
    c.simd(0x4e);
    c.get(dest);
    c.simd(negate);
    c.get(source);
    c.vector_const([0; 16]);
    c.simd(negative);
    c.simd(0x4e);
    c.simd(0x50);
}

fn test(c: &mut Code, dest: u32, source: u32) {
    for inverted in [false, true] {
        c.get(if inverted { source } else { dest });
        c.get(if inverted { dest } else { source });
        c.simd(if inverted { 0x4f } else { 0x4e });
        c.simd(0x53);
        c.op(0x45);
        c.op(0xad);
        if !inverted {
            c.i64(6);
            c.op(0x86);
        }
    }
    c.op(0x84);
    c.set(RESULT);
    c.get(FLAGS);
    c.i64(!0x8d5);
    c.op(0x83);
    c.get(RESULT);
    c.op(0x84);
    c.set(FLAGS);
    flags::discard(c);
}

pub(super) fn emit(
    c: &mut Code,
    i: &Instruction,
    known: &mut Option<flags::Recipe>,
    context: access::Context,
    bank: Bank,
) {
    let m = i.mnemonic();
    if let Some(size) = insert_size(m) {
        insert(c, i, context, bank, size);
        return;
    }
    if let Some(size) = extract_size(m) {
        extract(c, i, context, bank, size);
        return;
    }
    source(c, i, context, bank);
    let dest = bank.local(register(i, 0));
    let a = bank.scratch();
    c.set(a);
    if let Some((mut from, to, signed)) = extension(m) {
        c.get(a);
        while from < to {
            c.simd(
                match from {
                    8 => 0x87,
                    16 => 0xa7,
                    _ => 0xc7,
                } + if signed { 0 } else { 2 },
            );
            from *= 2;
        }
    } else if let Some(width) = mask_width(m) {
        c.get(a);
        c.get(dest);
        c.get(bank.local(0));
        if width == 64 {
            c.i32(63);
            c.simd(0xcc);
        } else {
            c.vector_const([0; 16]);
            c.simd(if width == 8 { 0x25 } else { 0x39 });
        }
        c.simd(0x52);
    } else {
        match m {
            M::Pabsb | M::Pabsw | M::Pabsd => {
                c.get(a);
                c.simd(match m {
                    M::Pabsb => 0x60,
                    M::Pabsw => 0x80,
                    _ => 0xa0,
                });
            }
            M::Pmulhw | M::Pmulhuw | M::Pmulhrsw => {
                for high in [false, true] {
                    product(c, dest, a, m == M::Pmulhuw, high);
                    if m == M::Pmulhrsw {
                        c.vector_const(std::array::from_fn(|n| if n % 4 == 1 { 0x40 } else { 0 }));
                        c.simd(0xae);
                        c.i32(15);
                        c.simd(0xac);
                    }
                }
                // PMULHRSW wraps -32768 * -32768 to 0x8000; Wasm's
                // saturating Q15 multiply instead produces 0x7fff.
                let start = if m == M::Pmulhrsw { 0 } else { 2 };
                shuffle(
                    c,
                    std::array::from_fn(|n| (n / 2 * 4 + n % 2 + start) as u8),
                );
            }
            M::Pmuldq | M::Pmuludq => {
                even_dwords(c, dest);
                even_dwords(c, a);
                c.simd(if m == M::Pmuldq { 0xdc } else { 0xde });
            }
            M::Pmaddwd => {
                c.get(dest);
                c.get(a);
                c.simd(0xba);
            }
            M::Pmaddubsw => {
                for high in [false, true] {
                    c.get(dest);
                    c.simd(0x89 + u32::from(high));
                    c.get(a);
                    c.simd(0x87 + u32::from(high));
                    c.simd(0x95);
                    c.simd(0x7e);
                }
                c.simd(0x85);
            }
            M::Psadbw => {
                c.get(dest);
                c.get(a);
                c.simd(0x73);
                c.get(a);
                c.get(dest);
                c.simd(0x73);
                c.simd(0x50);
                c.simd(0x7d);
                c.simd(0x7f);
                c.set(a);
                for odd in [false, true] {
                    c.get(a);
                    c.vector_const([0; 16]);
                    shuffle(
                        c,
                        std::array::from_fn(|n| {
                            if n % 8 >= 4 {
                                16
                            } else {
                                (n / 8 * 8 + n % 8 + usize::from(odd) * 4) as u8
                            }
                        }),
                    );
                }
                c.simd(0xce);
            }
            M::Mpsadbw => {
                c.vector_const([0; 16]);
                for offset in 0..4 {
                    c.get(dest);
                    c.get(dest);
                    let start = (i.immediate8() & 4) as usize + offset;
                    shuffle(c, std::array::from_fn(|n| (n % 8 + start) as u8));
                    c.simd(0x89);
                    c.get(a);
                    c.simd(0x16);
                    c.op((i.immediate8() & 3) * 4 + offset as u8);
                    c.simd(0x10);
                    c.simd(0x91);
                    c.simd(0x80);
                    c.simd(0x8e);
                }
            }
            M::Phaddw | M::Phaddd | M::Phaddsw | M::Phsubw | M::Phsubd | M::Phsubsw => {
                horizontal(c, m, dest, a)
            }
            M::Psignb | M::Psignw | M::Psignd => sign(c, m, dest, a),
            M::Ptest => {
                test(c, dest, a);
                *known = None;
                return;
            }
            M::Phminposuw => {
                c.get(a);
                c.set(dest);
                for distance in [4, 2, 1] {
                    c.get(dest);
                    c.get(dest);
                    c.get(dest);
                    shuffle(
                        c,
                        std::array::from_fn(|n| (((n / 2 + distance) % 8) * 2 + n % 2) as u8),
                    );
                    c.simd(0x97);
                    c.set(dest);
                }
                c.vector_const([0; 16]);
                c.get(dest);
                c.simd(0x19);
                c.op(0);
                c.simd(0x1a);
                c.op(0);
                c.get(a);
                c.get(dest);
                c.simd(0x2d);
                c.simd(0x84);
                c.op(0x68);
                c.simd(0x1a);
                c.op(1);
            }
            M::Palignr => {
                let count = i.immediate8() as usize;
                if count >= 32 {
                    c.vector_const([0; 16]);
                } else if count < 16 {
                    c.get(a);
                    c.get(dest);
                    shuffle(c, std::array::from_fn(|n| (n + count) as u8));
                } else {
                    c.get(dest);
                    c.vector_const([0; 16]);
                    shuffle(c, std::array::from_fn(|n| (n + count - 16).min(16) as u8));
                }
            }
            M::Pblendw | M::Blendps | M::Blendpd => {
                let size = match m {
                    M::Pblendw => 2,
                    M::Blendps => 4,
                    _ => 8,
                };
                c.get(dest);
                c.get(a);
                shuffle(
                    c,
                    std::array::from_fn(|n| {
                        (n + if i.immediate8() & (1 << (n / size)) != 0 {
                            16
                        } else {
                            0
                        }) as u8
                    }),
                );
            }
            M::Insertps => {
                c.get(dest);
                c.get(a);
                c.simd(0x1b);
                c.op(if memory(i, 1) { 0 } else { i.immediate8() >> 6 });
                c.simd(0x1c);
                c.op((i.immediate8() >> 4) & 3);
                c.vector_const(std::array::from_fn(|n| {
                    if i.immediate8() & (1 << (n / 4)) == 0 {
                        255
                    } else {
                        0
                    }
                }));
                c.simd(0x4e);
            }
            M::Movddup | M::Movsldup | M::Movshdup => {
                c.get(a);
                c.get(a);
                shuffle(
                    c,
                    std::array::from_fn(|n| match m {
                        M::Movddup => (n % 8) as u8,
                        M::Movsldup => (n / 8 * 8 + n % 4) as u8,
                        _ => (n / 8 * 8 + 4 + n % 4) as u8,
                    }),
                );
            }
            _ => unreachable!(),
        }
    }
    c.set(dest);
}
