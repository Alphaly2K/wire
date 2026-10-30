//! Bit-string register indices adjust the unsegmented effective address before
//! guest address-size wrapping; immediate indices stay within one operand.
use super::*;
use crate::memory::{READ, WRITE};
use iced_x86::{Mnemonic as M, OpKind};

pub(super) fn supported(i: &Instruction) -> bool {
    if i.flow_control() != FlowControl::Next
        || i.has_lock_prefix()
            && !(i.op0_kind() == OpKind::Memory && matches!(i.mnemonic(), M::Bts | M::Btr | M::Btc))
    {
        return false;
    }
    match i.mnemonic() {
        M::Bswap => scalar::register_operand(i, 0) && matches!(scalar::bits(i, 0), 32 | 64),
        M::Bsf | M::Bsr | M::Tzcnt | M::Lzcnt | M::Popcnt => {
            scalar::register_operand(i, 0)
                && (scalar::register_operand(i, 1) || scalar::memory_operand(i, 1))
        }
        M::Crc32 => {
            scalar::register_operand(i, 0)
                && matches!(scalar::bits(i, 0), 32 | 64)
                && (scalar::register_operand(i, 1) || scalar::memory_operand(i, 1))
        }
        M::Bt | M::Bts | M::Btr | M::Btc => {
            (scalar::register_operand(i, 0) || scalar::memory_operand(i, 0))
                && (scalar::register_operand(i, 1) || i.op1_kind() == OpKind::Immediate8)
        }
        _ => false,
    }
}

pub(super) fn memory_access(i: &Instruction) -> u8 {
    if (0..i.op_count()).all(|index| i.op_kind(index) != OpKind::Memory) {
        0
    } else if matches!(
        i.mnemonic(),
        M::Bt | M::Bsf | M::Bsr | M::Tzcnt | M::Lzcnt | M::Popcnt | M::Crc32
    ) {
        READ
    } else {
        READ | WRITE
    }
}

fn crc_helper(i: &Instruction) -> u32 {
    scalar::bits(i, 1).trailing_zeros() - 3
}
pub(super) fn crc_mask(blocks: &[Block]) -> u8 {
    blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter(|i| i.mnemonic() == M::Crc32 && supported(i))
        .fold(0, |mask, i| mask | (1 << crc_helper(i)))
}

pub(super) fn emit(
    c: &mut Code,
    i: &Instruction,
    known: &mut Option<flags::Recipe>,
    context: access::Context,
    crc_base: u32,
    crc_mask: u8,
) {
    let bits = scalar::bits(i, 0);
    if i.mnemonic() == M::Popcnt {
        if memory_access(i) != 0 {
            context.prepare(c, i, bits / 8, READ);
        }
        scalar::read(c, i, 1, context);
        c.set(A);
        c.get(A);
        c.op(0x7b);
        c.set(RESULT);
        scalar::write(c, i, 0, context);
        c.get(FLAGS);
        c.i64(!0x8d5);
        c.op(0x83);
        c.get(A);
        c.op(0x50);
        c.op(0xad);
        c.i64(6);
        c.op(0x86);
        c.op(0x84);
        c.set(FLAGS);
        flags::discard(c);
        *known = None;
        return;
    }
    if i.mnemonic() == M::Crc32 {
        let bytes = scalar::bits(i, 1) / 8;
        if memory_access(i) != 0 {
            context.prepare(c, i, bytes, READ);
        }
        scalar::read(c, i, 0, context);
        c.op(0xa7);
        scalar::read(c, i, 1, context);
        let helper = 1u8 << crc_helper(i);
        assert!(crc_mask & helper != 0);
        c.call(crc_base + (crc_mask & (helper - 1)).count_ones());
        c.op(0xad);
        c.set(RESULT);
        scalar::write(c, i, 0, context);
        return;
    }
    if i.mnemonic() == M::Bswap {
        scalar::read(c, i, 0, context);
        c.set(A);
        for (shift, mask) in [
            (8, 0x00ff00ff00ff00ff),
            (16, 0x0000ffff0000ffff),
            (32, 0xffffffff),
        ] {
            if shift >= bits {
                break;
            }
            c.get(A);
            c.i64(mask);
            c.op(0x83);
            c.i64(shift as u64);
            c.op(0x86);
            c.get(A);
            c.i64(shift as u64);
            c.op(0x88);
            c.i64(mask);
            c.op(0x83);
            c.op(0x84);
            c.set(A);
        }
        c.get(A);
        c.set(RESULT);
        scalar::write(c, i, 0, context);
        return;
    }
    let access = memory_access(i);
    if matches!(i.mnemonic(), M::Bsf | M::Bsr | M::Tzcnt | M::Lzcnt) {
        // BMI1/LZCNT are not published: their F3 encodings retain BSF/BSR.
        if access != 0 {
            context.prepare(c, i, bits / 8, access);
        }
        scalar::read(c, i, 1, context);
        c.set(A);
        c.get(A);
        c.i64(0);
        c.op(0x52);
        c.if_void();
        if matches!(i.mnemonic(), M::Bsf | M::Tzcnt) {
            c.get(A);
            c.op(0x7a);
        } else {
            c.i64(63);
            c.get(A);
            c.op(0x79);
            c.op(0x7d);
        }
        c.set(RESULT);
        scalar::write(c, i, 0, context);
        c.end();
        c.get(A);
        c.op(0x50);
        c.set(STATUS);
        flags::replace(c, 64, known);
    } else {
        scalar::read(c, i, 1, context);
        c.set(B);
        if access != 0 {
            if i.op1_kind() == OpKind::Register {
                c.get(B);
                scalar::sign_extend(c, bits);
                c.i64(bits.trailing_zeros() as u64);
                c.op(0x87);
                c.i64((bits / 8).trailing_zeros() as u64);
                c.op(0x86);
                c.set(A);
                context.address_adjusted(c, i, A);
            } else {
                context.address(c, i, true);
            }
            c.set(ADDRESS);
            context.prepare_address(c, i.ip(), bits / 8, access);
        }
        c.get(B);
        c.i64((bits - 1) as u64);
        c.op(0x83);
        c.set(B);
        scalar::read(c, i, 0, context);
        c.set(A);
        c.get(A);
        c.get(B);
        c.op(0x88);
        c.i64(1);
        c.op(0x83);
        c.op(0xa7);
        c.set(STATUS);
        if i.mnemonic() != M::Bt {
            c.get(A);
            c.i64(1);
            c.get(B);
            c.op(0x86);
            if i.mnemonic() == M::Btr {
                c.i64(u64::MAX);
                c.op(0x85);
            }
            c.op(match i.mnemonic() {
                M::Bts => 0x84,
                M::Btr => 0x83,
                _ => 0x85,
            });
            c.set(RESULT);
            scalar::write(c, i, 0, context);
        }
        flags::replace(c, 1, known);
    }
}
