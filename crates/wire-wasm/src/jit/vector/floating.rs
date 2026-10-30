use super::*;
use crate::floating::Output;

macro_rules! helpers {
    ($($id:ident => ($name:literal,$kind:literal)),* $(,)?) => {
        #[derive(Clone,Copy)]
        enum Helper { $($id),* }
        const FUNCTIONS: &[(&str,u8)] = &[$(($name,$kind)),*];
    };
}
helpers! {
    Add32 => ("wire_f32_add",9), Add64 => ("wire_f64_add",9),
    Sub32 => ("wire_f32_sub",9), Sub64 => ("wire_f64_sub",9),
    Mul32 => ("wire_f32_mul",9), Mul64 => ("wire_f64_mul",9),
    Div32 => ("wire_f32_div",9), Div64 => ("wire_f64_div",9),
    Min32 => ("wire_f32_min",9), Min64 => ("wire_f64_min",9),
    Max32 => ("wire_f32_max",9), Max64 => ("wire_f64_max",9),
    Sqrt32 => ("wire_f32_sqrt",9), Sqrt64 => ("wire_f64_sqrt",9),
    Cmp32 => ("wire_f32_predicate",10), Cmp64 => ("wire_f64_predicate",10),
    Comi32 => ("wire_f32_comi",9), Comi64 => ("wire_f64_comi",9),
    Ucomi32 => ("wire_f32_ucomi",9), Ucomi64 => ("wire_f64_ucomi",9),
    Widen => ("wire_f32_f64",9), Narrow => ("wire_f64_f32",9),
    I64F32 => ("wire_i64_f32",9), I64F64 => ("wire_i64_f64",9),
    F32Int => ("wire_f32_integer",9), F64Int => ("wire_f64_integer",9),
    F32Trunc => ("wire_f32_truncate",9), F64Trunc => ("wire_f64_truncate",9),
    F32I32x => ("wire_f32_i32x",9), F64I32x => ("wire_f64_i32x",9),
    F32I32xt => ("wire_f32_i32xt",9), F64I32xt => ("wire_f64_i32xt",9),
    I32xF32 => ("wire_i32x_f32",9), I32xF64 => ("wire_i32x_f64",9),
    HAdd32 => ("wire_f32_hadd",9), HAdd64 => ("wire_f64_hadd",9),
    HSub32 => ("wire_f32_hsub",9), HSub64 => ("wire_f64_hsub",9),
    AddSub32 => ("wire_f32_addsub",9), AddSub64 => ("wire_f64_addsub",9),
    Round32 => ("wire_f32_round",10), Round64 => ("wire_f64_round",10),
    Dot32 => ("wire_f32_dot",10), Dot64 => ("wire_f64_dot",10),
}

#[derive(Clone, Copy)]
pub(in crate::jit) struct Calls {
    pub mask: u64,
    pub base: u32,
}
impl Calls {
    fn function(self, helper: Helper) -> u32 {
        let bit = 1u64 << (helper as u32);
        assert!(self.mask & bit != 0);
        self.base + (self.mask & (bit - 1)).count_ones()
    }
}

fn helper(i: &Instruction) -> Option<Helper> {
    use Helper::*;
    Some(match i.mnemonic() {
        M::Addss | M::Addps => Add32,
        M::Addsd | M::Addpd => Add64,
        M::Subss | M::Subps => Sub32,
        M::Subsd | M::Subpd => Sub64,
        M::Mulss | M::Mulps => Mul32,
        M::Mulsd | M::Mulpd => Mul64,
        M::Divss | M::Divps => Div32,
        M::Divsd | M::Divpd => Div64,
        M::Minss | M::Minps => Min32,
        M::Minsd | M::Minpd => Min64,
        M::Maxss | M::Maxps => Max32,
        M::Maxsd | M::Maxpd => Max64,
        M::Sqrtss | M::Sqrtps => Sqrt32,
        M::Sqrtsd | M::Sqrtpd => Sqrt64,
        M::Cmpss | M::Cmpps => Cmp32,
        M::Cmpsd | M::Cmppd => Cmp64,
        M::Comiss => Comi32,
        M::Comisd => Comi64,
        M::Ucomiss => Ucomi32,
        M::Ucomisd => Ucomi64,
        M::Cvtss2sd | M::Cvtps2pd => Widen,
        M::Cvtsd2ss | M::Cvtpd2ps => Narrow,
        M::Cvtsi2ss => I64F32,
        M::Cvtsi2sd => I64F64,
        M::Cvtss2si => F32Int,
        M::Cvtsd2si => F64Int,
        M::Cvttss2si => F32Trunc,
        M::Cvttsd2si => F64Trunc,
        M::Cvtps2dq | M::Cvtps2pi => F32I32x,
        M::Cvtpd2dq | M::Cvtpd2pi => F64I32x,
        M::Cvttps2dq | M::Cvttps2pi => F32I32xt,
        M::Cvttpd2dq | M::Cvttpd2pi => F64I32xt,
        M::Cvtdq2ps | M::Cvtpi2ps => I32xF32,
        M::Cvtdq2pd | M::Cvtpi2pd => I32xF64,
        M::Haddps => HAdd32,
        M::Haddpd => HAdd64,
        M::Hsubps => HSub32,
        M::Hsubpd => HSub64,
        M::Addsubps => AddSub32,
        M::Addsubpd => AddSub64,
        M::Roundss | M::Roundps => Round32,
        M::Roundsd | M::Roundpd => Round64,
        M::Dpps => Dot32,
        M::Dppd => Dot64,
        _ => return None,
    })
}

fn gpr_destination(helper: Helper) -> bool {
    matches!(
        helper,
        Helper::F32Int | Helper::F64Int | Helper::F32Trunc | Helper::F64Trunc
    )
}
fn gpr_source(helper: Helper) -> bool {
    matches!(helper, Helper::I64F32 | Helper::I64F64)
}
pub(super) fn compares_flags(i: &Instruction) -> bool {
    matches!(
        i.mnemonic(),
        M::Comiss | M::Comisd | M::Ucomiss | M::Ucomisd
    )
}
fn scalar(i: &Instruction) -> bool {
    matches!(
        i.mnemonic(),
        M::Addss
            | M::Addsd
            | M::Subss
            | M::Subsd
            | M::Mulss
            | M::Mulsd
            | M::Divss
            | M::Divsd
            | M::Minss
            | M::Minsd
            | M::Maxss
            | M::Maxsd
            | M::Sqrtss
            | M::Sqrtsd
            | M::Cmpss
            | M::Cmpsd
            | M::Roundss
            | M::Roundsd
    )
}

fn mmx_conversion(i: &Instruction) -> bool {
    matches!(
        i.mnemonic(),
        M::Cvtpi2ps | M::Cvtpi2pd | M::Cvtps2pi | M::Cvtpd2pi | M::Cvttps2pi | M::Cvttpd2pi
    )
}

pub(super) fn uses_fpu(i: &Instruction) -> bool {
    // Unlike CVTPI2PS, CVTPI2PD from memory leaves the entire x87 state alone.
    mmx_conversion(i) && !(i.mnemonic() == M::Cvtpi2pd && memory(i, 1))
}

pub(super) fn supported(i: &Instruction) -> bool {
    if i.has_lock_prefix() {
        return false;
    }
    if matches!(i.mnemonic(), M::Ldmxcsr | M::Stmxcsr) {
        return memory(i, 0);
    }
    if mmx_conversion(i) {
        return if matches!(i.mnemonic(), M::Cvtpi2ps | M::Cvtpi2pd) {
            xmm(i, 0) && (mmx::register_operand(i, 1) || memory(i, 1))
        } else {
            mmx::register_operand(i, 0) && (xmm(i, 1) || memory(i, 1))
        };
    }
    let Some(h) = helper(i) else {
        return false;
    };
    let dest = if gpr_destination(h) {
        scalar::register_operand(i, 0)
    } else {
        xmm(i, 0)
    };
    let source = if gpr_source(h) {
        scalar::register_operand(i, 1) || scalar::memory_operand(i, 1)
    } else {
        xmm(i, 1) || memory(i, 1)
    };
    dest && source && (!immediate(h) || i.op2_kind() == OpKind::Immediate8)
}
fn immediate(h: Helper) -> bool {
    matches!(
        h,
        Helper::Cmp32
            | Helper::Cmp64
            | Helper::Round32
            | Helper::Round64
            | Helper::Dot32
            | Helper::Dot64
    )
}
pub(super) fn memory_access(i: &Instruction) -> u8 {
    if i.mnemonic() == M::Stmxcsr {
        WRITE
    } else if i.mnemonic() == M::Ldmxcsr || memory(i, 1) {
        READ
    } else {
        0
    }
}
pub(in crate::jit) fn collect(blocks: &[Block]) -> u64 {
    blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .filter(|i| supported(i))
        .filter_map(helper)
        .fold(0, |mask, h| mask | (1u64 << h as u32))
}
pub(in crate::jit) fn imports(out: &mut Vec<u8>, mask: u64) {
    for (index, &(name, kind)) in FUNCTIONS.iter().enumerate() {
        if mask & (1u64 << index) != 0 {
            binary::name(out, "core");
            binary::name(out, name);
            out.extend_from_slice(&[0, kind]);
        }
    }
}

fn source_size(i: &Instruction, h: Helper) -> u32 {
    use Helper::*;
    if mmx_conversion(i) {
        return if matches!(i.mnemonic(), M::Cvtpd2pi | M::Cvttpd2pi) {
            16
        } else {
            8
        };
    }
    match h {
        Add32 | Sub32 | Mul32 | Div32 | Min32 | Max32 | Sqrt32 | Cmp32 | Round32 => {
            if scalar(i) {
                4
            } else {
                16
            }
        }
        Add64 | Sub64 | Mul64 | Div64 | Min64 | Max64 | Sqrt64 | Cmp64 | Round64 => {
            if scalar(i) {
                8
            } else {
                16
            }
        }
        Comi32 | Ucomi32 | F32Int | F32Trunc => 4,
        Comi64 | Ucomi64 | F64Int | F64Trunc => 8,
        Widen => {
            if i.mnemonic() == M::Cvtss2sd {
                4
            } else {
                8
            }
        }
        Narrow => {
            if i.mnemonic() == M::Cvtsd2ss {
                8
            } else {
                16
            }
        }
        I64F32 | I64F64 => scalar::bits(i, 1) / 8,
        I32xF64 => 8,
        _ => 16,
    }
}
fn shape(i: &Instruction, h: Helper) -> u32 {
    use Helper::*;
    if mmx_conversion(i) {
        return 2;
    }
    match h {
        F32Int | F64Int | F32Trunc | F64Trunc => scalar::bits(i, 0),
        I64F32 | I64F64 | Comi32 | Comi64 | Ucomi32 | Ucomi64 => 1,
        Widen => {
            if i.mnemonic() == M::Cvtss2sd {
                1
            } else {
                2
            }
        }
        Narrow => {
            if i.mnemonic() == M::Cvtsd2ss {
                1
            } else {
                2
            }
        }
        I32xF64 | F64I32x | F64I32xt => 2,
        I32xF32 | F32I32x | F32I32xt => 4,
        _ if scalar(i) => 1,
        Add64 | Sub64 | Mul64 | Div64 | Min64 | Max64 | Sqrt64 | Cmp64 | HAdd64 | HSub64
        | AddSub64 | Round64 | Dot64 => 2,
        _ => 4,
    }
}

fn control(c: &mut Code, i: &Instruction, context: access::Context, bank: Bank) {
    super::prepare(c, i, context, 4);
    if i.mnemonic() == M::Ldmxcsr {
        context.load(c, i.ip(), 4, READ);
        c.set(RESULT);
        c.get(RESULT);
        c.i64(!0xffff);
        c.op(0x83);
        c.i64(0);
        c.op(0x52);
        c.if_void();
        exception(c, context.out, i.ip(), 13);
        c.end();
        c.get(RESULT);
        c.op(0xa7);
        c.set(bank.mxcsr());
    } else {
        c.get(bank.mxcsr());
        c.op(0xad);
        c.set(RESULT);
        context.store(c, i.ip(), 4);
    }
}

pub(super) fn emit(
    c: &mut Code,
    i: &Instruction,
    known: &mut Option<flags::Recipe>,
    context: access::Context,
    bank: Bank,
    calls: Calls,
) {
    if matches!(i.mnemonic(), M::Ldmxcsr | M::Stmxcsr) {
        control(c, i, context, bank);
        return;
    }
    let h = helper(i).unwrap();
    if uses_fpu(i) {
        mmx::wait(c, i, context, bank);
    }
    let size = source_size(i, h);
    if mmx::register_operand(i, 1) {
        c.get(bank.local(register(i, 1)));
    } else if gpr_source(h) {
        if memory(i, 1) {
            super::prepare(c, i, context, size);
        }
        c.vector_const([0; 16]);
        scalar::read(c, i, 1, context);
        if size == 4 {
            c.op(0xa7);
            c.op(0xac);
        }
        super::lane(c, true, 0);
    } else if xmm(i, 1) || size == 16 {
        super::source(c, i, context, bank);
    } else {
        super::prepare(c, i, context, size);
        c.vector_const([0; 16]);
        context.load(c, i.ip(), size, READ);
        super::lane(c, true, 0);
    }
    c.set(bank.scratch());
    c.get(3);
    c.get(bank.mxcsr());
    for lane in 0..2 {
        if xmm(i, 0) {
            c.get(bank.local(register(i, 0)));
            super::lane(c, false, lane);
        } else {
            c.i64(0);
        }
    }
    for lane in 0..2 {
        c.get(bank.scratch());
        super::lane(c, false, lane);
    }
    c.i32(shape(i, h));
    if immediate(h) {
        c.i32(i.immediate8() as u32);
    }
    c.call(calls.function(h));
    c.set(TRANSLATION);
    c.get(TRANSLATION);
    c.memory(0x28, offset_of!(Output, mxcsr));
    c.set(bank.mxcsr());
    c.get(TRANSLATION);
    c.memory(0x28, offset_of!(Output, exception));
    c.if_void();
    // SDM Vol. 3A, Event 19: #XM commits MXCSR, not the MMX tag/TOP transition.
    exception(c, context.out, i.ip(), 19);
    c.end();
    if compares_flags(i) {
        c.get(FLAGS);
        c.i64(!0x8d5);
        c.op(0x83);
        c.get(TRANSLATION);
        c.load64(offset_of!(Output, flags));
        c.op(0x84);
        c.set(FLAGS);
        flags::discard(c);
        *known = None;
    } else if gpr_destination(h) {
        c.get(TRANSLATION);
        c.load64(offset_of!(Output, value));
        c.set(RESULT);
        scalar::write(c, i, 0, context);
    } else if mmx::register_operand(i, 0) {
        c.get(TRANSLATION);
        c.vector_memory(0, offset_of!(Output, value));
        mmx::commit(c, i, bank);
    } else if i.mnemonic() == M::Cvtpi2ps {
        c.get(bank.local(register(i, 0)));
        c.get(TRANSLATION);
        c.load64(offset_of!(Output, value));
        super::lane(c, true, 0);
        c.set(bank.local(register(i, 0)));
    } else {
        c.get(TRANSLATION);
        c.vector_memory(0, offset_of!(Output, value));
        c.set(bank.local(register(i, 0)));
    }
    if uses_fpu(i) {
        mmx::effects(c, i, bank);
    }
}
