//! Arithmetic recipes stay in locals until a consumer or architectural exit.
use super::*;

const CF: u64 = 1;
const PF: u64 = 4;
const AF: u64 = 16;
const ZF: u64 = 64;
const SF: u64 = 128;
const OF: u64 = 2048;
const ARITHMETIC: u64 = CF | PF | AF | ZF | SF | OF;
const KIND_MASK: u32 = 7;
const PRESERVE_CF: u32 = 8;
const CARRY_IN: u32 = 16;
const OVERRIDE_SHIFT: u32 = 5;

pub(super) const ADD: u32 = 1;
pub(super) const SUB: u32 = 2;
pub(super) const LOGIC: u32 = 3;
pub(super) const SHL: u32 = 4;
pub(super) const SHR: u32 = 5;
pub(super) const SAR: u32 = 6;
pub(super) const SHRD: u32 = 7;

#[derive(Clone, Copy)]
pub(super) struct Recipe {
    pub kind: u32,
    pub bits: u32,
    pub preserve_cf: bool,
    pub carry_in: bool,
    pub overridden: u64,
}

fn nonzero(c: &mut Code) {
    c.op(0x50);
    c.op(0x45);
}

fn old_bit(c: &mut Code, flag: u64) {
    c.get(FLAGS);
    c.i64(flag);
    c.op(0x83);
    nonzero(c);
}

fn sign(c: &mut Code, known: Option<Recipe>) {
    if let Some(recipe) = known {
        c.i64(1 << (recipe.bits - 1));
    } else {
        c.get(SIGN);
    }
}

fn kind_is(c: &mut Code, value: u32) {
    c.get(TAG);
    c.i32(KIND_MASK);
    c.op(0x71);
    c.i32(value);
    c.op(0x46);
}

fn arithmetic_carry(c: &mut Code, add: bool, carry_in: bool) {
    c.get(if add { VALUE } else { LEFT });
    c.get(if add { LEFT } else { RIGHT });
    c.op(0x54);
    if carry_in {
        c.get(if add { VALUE } else { LEFT });
        c.get(if add { LEFT } else { RIGHT });
        c.op(0x51);
        c.get(TAG);
        c.i32(CARRY_IN);
        c.op(0x71);
        c.i32(0);
        c.op(0x47);
        c.op(0x71);
        c.op(0x72);
    }
}

fn arithmetic_bit(c: &mut Code, flag: u64, known: Option<Recipe>) {
    match flag {
        CF => {
            if let Some(recipe) = known {
                if recipe.preserve_cf {
                    old_bit(c, CF);
                } else if recipe.kind == LOGIC {
                    c.i32(0);
                } else {
                    arithmetic_carry(c, recipe.kind == ADD, recipe.carry_in);
                }
            } else {
                c.get(TAG);
                c.i32(PRESERVE_CF);
                c.op(0x71);
                c.if_i32();
                old_bit(c, CF);
                c.otherwise();
                kind_is(c, LOGIC);
                c.if_i32();
                c.i32(0);
                c.otherwise();
                kind_is(c, ADD);
                c.if_i32();
                arithmetic_carry(c, true, true);
                c.otherwise();
                arithmetic_carry(c, false, true);
                c.end();
                c.end();
                c.end();
            }
        }
        ZF => {
            c.get(VALUE);
            c.op(0x50);
        }
        SF => {
            c.get(VALUE);
            sign(c, known);
            c.op(0x83);
            nonzero(c);
        }
        PF => {
            c.get(VALUE);
            c.i64(255);
            c.op(0x83);
            c.op(0x7b);
            c.i64(1);
            c.op(0x83);
            c.op(0x50);
        }
        AF | OF => {
            if known.is_some_and(|recipe| recipe.kind == LOGIC) {
                c.i32(0);
                return;
            }
            if known.is_none() {
                kind_is(c, LOGIC);
                c.if_i32();
                c.i32(0);
                c.otherwise();
            }
            c.get(LEFT);
            c.get(RIGHT);
            c.op(0x85);
            if flag == AF {
                c.get(VALUE);
                c.op(0x85);
                c.i64(AF);
                c.op(0x83);
            } else {
                if let Some(recipe) = known {
                    if recipe.kind == ADD {
                        c.i64(u64::MAX);
                        c.op(0x85);
                    }
                } else {
                    // ADD complements the operand xor; SUB leaves it intact.
                    c.i64(0);
                    kind_is(c, ADD);
                    c.op(0xad);
                    c.op(0x7d);
                    c.op(0x85);
                }
                c.get(LEFT);
                c.get(VALUE);
                c.op(0x85);
                c.op(0x83);
                sign(c, known);
                c.op(0x83);
            }
            nonzero(c);
            if known.is_none() {
                c.end();
            }
        }
        _ => unreachable!(),
    }
}

fn shift_carry(c: &mut Code, known: Option<Recipe>) {
    let left = |c: &mut Code| {
        c.get(LEFT);
        c.get(RIGHT);
        c.i64(1);
        c.op(0x7d);
        c.op(0x86);
        sign(c, known);
        c.op(0x83);
        nonzero(c);
    };
    let right = |c: &mut Code| {
        c.get(LEFT);
        c.get(RIGHT);
        c.i64(1);
        c.op(0x7d);
        c.op(0x88);
        c.i64(1);
        c.op(0x83);
        c.op(0xa7);
    };
    if let Some(recipe) = known {
        if recipe.kind == SHL {
            left(c)
        } else {
            right(c)
        }
    } else {
        kind_is(c, SHL);
        c.if_i32();
        left(c);
        c.otherwise();
        right(c);
        c.end();
    }
}

fn shift_bit(c: &mut Code, flag: u64, known: Option<Recipe>) {
    match flag {
        CF => shift_carry(c, known),
        AF => old_bit(c, AF),
        OF => {
            c.get(RIGHT);
            c.i64(1);
            c.op(0x51);
            c.if_i32();
            let overflow = |c: &mut Code, kind| match kind {
                SHL => {
                    arithmetic_bit(c, SF, known);
                    shift_carry(c, known);
                    c.op(0x73);
                }
                SHR => {
                    c.get(LEFT);
                    sign(c, known);
                    c.op(0x83);
                    nonzero(c);
                }
                SHRD => {
                    c.get(LEFT);
                    c.get(VALUE);
                    c.op(0x85);
                    sign(c, known);
                    c.op(0x83);
                    nonzero(c);
                }
                _ => c.i32(0),
            };
            if let Some(recipe) = known {
                overflow(c, recipe.kind);
            } else {
                kind_is(c, SHL);
                c.if_i32();
                overflow(c, SHL);
                c.otherwise();
                kind_is(c, SHR);
                c.if_i32();
                overflow(c, SHR);
                c.otherwise();
                kind_is(c, SHRD);
                c.if_i32();
                overflow(c, SHRD);
                c.otherwise();
                overflow(c, SAR);
                c.end();
                c.end();
                c.end();
            }
            c.otherwise();
            old_bit(c, OF);
            c.end();
        }
        _ => unreachable!(),
    }
}

fn computed_bit(c: &mut Code, flag: u64, known: Option<Recipe>) {
    if !matches!(flag, CF | AF | OF) || known.is_some_and(|recipe| recipe.kind < SHL) {
        arithmetic_bit(c, flag, known);
    } else if known.is_some() {
        shift_bit(c, flag, known);
    } else {
        c.get(TAG);
        c.i32(KIND_MASK);
        c.op(0x71);
        c.i32(SHL);
        c.op(0x4f);
        c.if_i32();
        shift_bit(c, flag, None);
        c.otherwise();
        arithmetic_bit(c, flag, None);
        c.end();
    }
}

fn recipe_bit(c: &mut Code, flag: u64, known: Option<Recipe>) {
    if let Some(recipe) = known {
        if recipe.overridden & flag != 0 {
            old_bit(c, flag);
        } else {
            computed_bit(c, flag, known);
        }
    } else {
        c.get(TAG);
        c.i32((flag as u32) << OVERRIDE_SHIFT);
        c.op(0x71);
        c.if_i32();
        old_bit(c, flag);
        c.otherwise();
        computed_bit(c, flag, None);
        c.end();
    }
}

pub(super) fn mark_overridden(c: &mut Code, mask: u64, known: &mut Option<Recipe>) {
    c.get(TAG);
    c.if_void();
    c.get(TAG);
    c.i32((mask as u32) << OVERRIDE_SHIFT);
    c.op(0x72);
    c.set(TAG);
    c.end();
    if let Some(recipe) = known {
        recipe.overridden |= mask;
    }
}

/// STATUS contains one Boolean value for all selected flag bits.
pub(super) fn replace(c: &mut Code, mask: u64, known: &mut Option<Recipe>) {
    c.get(FLAGS);
    c.i64(!mask);
    c.op(0x83);
    c.get(STATUS);
    c.op(0xad);
    c.i64(mask);
    c.op(0x7e);
    c.op(0x84);
    c.set(FLAGS);
    mark_overridden(c, mask, known);
}

pub(super) fn bit(c: &mut Code, flag: u64, known: Option<Recipe>) {
    c.get(TAG);
    c.if_i32();
    recipe_bit(c, flag, known);
    c.otherwise();
    old_bit(c, flag);
    c.end();
}

pub(super) fn defer(c: &mut Code, recipe: Recipe, known: Option<Recipe>) {
    let preserved: &[u64] = if recipe.kind >= SHL {
        &[AF, OF]
    } else if recipe.preserve_cf {
        &[CF]
    } else {
        &[]
    };
    for &flag in preserved {
        c.get(FLAGS);
        c.i64(!flag);
        c.op(0x83);
        bit(c, flag, known);
        c.op(0xad);
        c.i64(flag);
        c.op(0x7e);
        c.op(0x84);
        c.set(FLAGS);
    }
    for (from, to) in [(A, LEFT), (B, RIGHT), (RESULT, VALUE)] {
        c.get(from);
        c.set(to);
    }
    c.i64(1 << (recipe.bits - 1));
    c.set(SIGN);
    c.i32(recipe.kind | if recipe.preserve_cf { PRESERVE_CF } else { 0 });
    if recipe.carry_in {
        c.get(STATUS);
        c.i32(CARRY_IN.trailing_zeros());
        c.op(0x74);
        c.op(0x72);
    }
    c.set(TAG);
}

fn condition_bits(c: &mut Code, cc: u32, known: Option<Recipe>, architectural: bool) {
    let bit = |c: &mut Code, flag| {
        if architectural {
            old_bit(c, flag);
        } else {
            recipe_bit(c, flag, known);
        }
    };
    match cc / 2 {
        0 => bit(c, OF),
        1 => bit(c, CF),
        2 => bit(c, ZF),
        3 => {
            bit(c, CF);
            bit(c, ZF);
            c.op(0x72);
        }
        4 => bit(c, SF),
        5 => bit(c, PF),
        6 | 7 => {
            bit(c, SF);
            bit(c, OF);
            c.op(0x73);
            if cc / 2 == 7 {
                bit(c, ZF);
                c.op(0x72);
            }
        }
        _ => unreachable!("x86 condition code"),
    }
    if cc & 1 != 0 {
        c.op(0x45);
    }
}

pub(super) fn condition(c: &mut Code, cc: u32, known: Option<Recipe>) {
    c.get(TAG);
    c.if_i32();
    condition_bits(c, cc, known, false);
    c.otherwise();
    condition_bits(c, cc, known, true);
    c.end();
}

pub(super) fn discard(c: &mut Code) {
    c.i32(0);
    c.set(TAG);
}

pub(super) fn materialize(c: &mut Code, function: u32) {
    c.get(TAG);
    c.if_void();
    for local in [FLAGS, LEFT, RIGHT, VALUE, SIGN, TAG] {
        c.get(local);
    }
    c.call(function);
    c.set(FLAGS);
    c.end();
}

pub(super) fn signature(out: &mut Vec<u8>) {
    binary::signature(out, &[0x7e, 0x7e, 0x7e, 0x7e, 0x7e, 0x7f], &[0x7e]);
}

pub(super) fn function() -> Code {
    let mut c = Code::default();
    // Retain the recipe emitter's local layout without passing CPU state.
    for (parameter, local) in [FLAGS, LEFT, RIGHT, VALUE, SIGN, TAG]
        .into_iter()
        .enumerate()
    {
        c.get(parameter as u32);
        c.set(local);
    }
    materialize_inline(&mut c);
    c.get(FLAGS);
    c
}

fn materialize_inline(c: &mut Code) {
    c.get(TAG);
    c.if_void();
    c.i64(0);
    c.set(RESULT);
    for flag in [CF, PF, AF, ZF, SF, OF] {
        c.get(RESULT);
        recipe_bit(c, flag, None);
        c.op(0xad);
        c.i64(flag);
        c.op(0x7e);
        c.op(0x84);
        c.set(RESULT);
    }
    c.get(FLAGS);
    c.i64(!ARITHMETIC);
    c.op(0x83);
    c.get(RESULT);
    c.op(0x84);
    c.set(FLAGS);
    c.end();
}

pub(super) fn instruction(c: &mut Code, i: &Instruction, known: &mut Option<Recipe>) -> bool {
    use iced_x86::Mnemonic as M;
    let mnemonic = i.mnemonic();
    if matches!(mnemonic, M::Cld | M::Std) {
        c.get(FLAGS);
        c.i64(if mnemonic == M::Cld { !1024 } else { 1024 });
        c.op(if mnemonic == M::Cld { 0x83 } else { 0x84 });
        c.set(FLAGS);
        return true;
    }
    if !matches!(mnemonic, M::Clc | M::Stc | M::Cmc | M::Lahf | M::Sahf) {
        return false;
    }
    match mnemonic {
        M::Clc | M::Stc | M::Cmc => {
            if mnemonic == M::Cmc {
                bit(c, CF, *known);
                c.op(0x45);
            } else {
                c.i32(u32::from(mnemonic == M::Stc));
            }
            c.set(STATUS);
            replace(c, CF, known);
        }
        M::Lahf => {
            c.i64(2);
            c.set(RESULT);
            for flag in [CF, PF, AF, ZF, SF] {
                c.get(RESULT);
                bit(c, flag, *known);
                c.op(0xad);
                c.i64(flag);
                c.op(0x7e);
                c.op(0x84);
                c.set(RESULT);
            }
            c.get(GPR);
            c.i64(!0xff00);
            c.op(0x83);
            c.get(RESULT);
            c.i64(8);
            c.op(0x86);
            c.op(0x84);
            c.set(GPR);
        }
        M::Sahf => {
            c.get(FLAGS);
            c.i64(!0xd5);
            c.op(0x83);
            c.get(GPR);
            c.i64(8);
            c.op(0x88);
            c.i64(0xd5);
            c.op(0x83);
            c.op(0x84);
            c.set(FLAGS);
            mark_overridden(c, 0xd5, known);
        }
        _ => unreachable!(),
    }
    true
}
