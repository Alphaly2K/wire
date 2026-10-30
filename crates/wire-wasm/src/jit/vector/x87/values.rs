use super::*;
use crate::extended::Output;

macro_rules! helpers { ($($id:ident=>$name:literal),*$(,)?)=>{
    #[derive(Clone,Copy)] enum Helper{$($id),*}
    const FUNCTIONS:&[&str]=&[$($name),*];
}; }
helpers! {
 Load32=>"wire_ext_load32",Load64=>"wire_ext_load64",Integer=>"wire_ext_integer",
 Store32=>"wire_ext_store32",Store64=>"wire_ext_store64",I16=>"wire_ext_i16",I32=>"wire_ext_i32",I64=>"wire_ext_i64",T16=>"wire_ext_t16",T32=>"wire_ext_t32",T64=>"wire_ext_t64",
 One=>"wire_ext_one",L2t=>"wire_ext_l2t",L2e=>"wire_ext_l2e",Pi=>"wire_ext_pi",Lg2=>"wire_ext_lg2",Ln2=>"wire_ext_ln2",Zero=>"wire_ext_zero",
 Add=>"wire_ext_add",Mul=>"wire_ext_mul",Sub=>"wire_ext_sub",Div=>"wire_ext_div",
 Add32=>"wire_ext_add32",Add64=>"wire_ext_add64",AddI16=>"wire_ext_addi16",AddI32=>"wire_ext_addi32",
 Mul32=>"wire_ext_mul32",Mul64=>"wire_ext_mul64",MulI16=>"wire_ext_muli16",MulI32=>"wire_ext_muli32",
 Sub32=>"wire_ext_sub32",Sub64=>"wire_ext_sub64",SubI16=>"wire_ext_subi16",SubI32=>"wire_ext_subi32",
 RSub32=>"wire_ext_rsub32",RSub64=>"wire_ext_rsub64",RSubI16=>"wire_ext_rsubi16",RSubI32=>"wire_ext_rsubi32",
 Div32=>"wire_ext_div32",Div64=>"wire_ext_div64",DivI16=>"wire_ext_divi16",DivI32=>"wire_ext_divi32",
 RDiv32=>"wire_ext_rdiv32",RDiv64=>"wire_ext_rdiv64",RDivI16=>"wire_ext_rdivi16",RDivI32=>"wire_ext_rdivi32",
 Sqrt=>"wire_ext_sqrt",Cmp=>"wire_ext_cmp",UCmp=>"wire_ext_ucmp",Cmp32=>"wire_ext_cmp32",Cmp64=>"wire_ext_cmp64",CmpI16=>"wire_ext_cmpi16",CmpI32=>"wire_ext_cmpi32",
 Cmpi=>"wire_ext_cmpi",UCmpi=>"wire_ext_ucmpi",Round=>"wire_ext_round",
 Rem=>"wire_ext_rem",Rem1=>"wire_ext_rem1",Scale=>"wire_ext_scale",Extract=>"wire_ext_extract",
 BcdLoad=>"wire_ext_bcd_load",BcdStore=>"wire_ext_bcd_store",
 Exp=>"wire_ext_exp",Sin=>"wire_ext_sin",Cos=>"wire_ext_cos",SinCos=>"wire_ext_sincos",Tan=>"wire_ext_tan",Atan=>"wire_ext_atan",Log=>"wire_ext_log",Log1p=>"wire_ext_log1p",
}
fn st(i: &Instruction, n: u32) -> bool {
    n < i.op_count()
        && i.op_kind(n) == OpKind::Register
        && (Register::ST0..=Register::ST7).contains(&i.op_register(n))
}
fn logical(i: &Instruction, n: u32) -> u32 {
    i.op_register(n) as u32 - Register::ST0 as u32
}
fn integer(i: &Instruction) -> bool {
    matches!(
        i.mnemonic(),
        M::Fild
            | M::Fist
            | M::Fistp
            | M::Fisttp
            | M::Fiadd
            | M::Fimul
            | M::Fisub
            | M::Fisubr
            | M::Fidiv
            | M::Fidivr
            | M::Ficom
            | M::Ficomp
    )
}
fn load(i: &Instruction) -> bool {
    matches!(
        i.mnemonic(),
        M::Fld
            | M::Fbld
            | M::Fild
            | M::Fld1
            | M::Fldl2t
            | M::Fldl2e
            | M::Fldpi
            | M::Fldlg2
            | M::Fldln2
            | M::Fldz
    )
}
fn store(i: &Instruction) -> bool {
    matches!(
        i.mnemonic(),
        M::Fst | M::Fstp | M::Fist | M::Fistp | M::Fisttp | M::Fbstp
    )
}
fn compare(i: &Instruction) -> bool {
    matches!(
        i.mnemonic(),
        M::Fcom
            | M::Fcomp
            | M::Fcompp
            | M::Fucom
            | M::Fucomp
            | M::Fucompp
            | M::Ficom
            | M::Ficomp
            | M::Ftst
            | M::Fcomi
            | M::Fcomip
            | M::Fucomi
            | M::Fucomip
    )
}
fn compares_flags(i: &Instruction) -> bool {
    matches!(i.mnemonic(), M::Fcomi | M::Fcomip | M::Fucomi | M::Fucomip)
}
fn movement(i: &Instruction) -> Option<u32> {
    Some(match i.mnemonic() {
        M::Fcmovb => 2,
        M::Fcmovnb => 3,
        M::Fcmove => 4,
        M::Fcmovne => 5,
        M::Fcmovbe => 6,
        M::Fcmovnbe => 7,
        M::Fcmovu => 10,
        M::Fcmovnu => 11,
        _ => return None,
    })
}
fn reverse(i: &Instruction) -> bool {
    matches!(
        i.mnemonic(),
        M::Fsubr | M::Fsubrp | M::Fisubr | M::Fdivr | M::Fdivrp | M::Fidivr
    )
}
fn remainder(i: &Instruction) -> bool {
    matches!(i.mnemonic(), M::Fprem | M::Fprem1)
}
fn two_sources(i: &Instruction) -> bool {
    remainder(i) || i.mnemonic() == M::Fscale || binary_pop(i)
}
fn binary_pop(i: &Instruction) -> bool {
    matches!(i.mnemonic(), M::Fpatan | M::Fyl2x | M::Fyl2xp1)
}
fn paired(i: &Instruction) -> bool {
    matches!(i.mnemonic(), M::Fxtract | M::Fsincos | M::Fptan)
}
fn trigonometric(i: &Instruction) -> bool {
    matches!(i.mnemonic(), M::Fsin | M::Fcos | M::Fsincos | M::Fptan)
}
fn kind(i: &Instruction) -> Option<u32> {
    Some(match i.mnemonic() {
        M::Fadd | M::Faddp | M::Fiadd => 0,
        M::Fmul | M::Fmulp | M::Fimul => 1,
        M::Fsub | M::Fsubp | M::Fsubr | M::Fsubrp | M::Fisub | M::Fisubr => 4,
        M::Fdiv | M::Fdivp | M::Fdivr | M::Fdivrp | M::Fidiv | M::Fidivr => 6,
        _ => return None,
    })
}
fn pops(i: &Instruction) -> u32 {
    match i.mnemonic() {
        M::Fcompp | M::Fucompp => 2,
        M::Fstp
        | M::Fistp
        | M::Fisttp
        | M::Fbstp
        | M::Faddp
        | M::Fmulp
        | M::Fsubp
        | M::Fsubrp
        | M::Fdivp
        | M::Fdivrp
        | M::Fcomp
        | M::Fucomp
        | M::Ficomp
        | M::Fcomip
        | M::Fucomip => 1,
        M::Fpatan | M::Fyl2x | M::Fyl2xp1 => 1,
        _ => 0,
    }
}
pub(super) fn writes(i: &Instruction) -> bool {
    i.mnemonic() != M::Fxam && !compare(i) && !(store(i) && memory(i, 0))
}
pub(super) fn supported(i: &Instruction) -> bool {
    if i.has_lock_prefix() {
        return false;
    }
    let m = i.mnemonic();
    let size = i.memory_size().size();
    if matches!(
        m,
        M::Fld1
            | M::Fldl2t
            | M::Fldl2e
            | M::Fldpi
            | M::Fldlg2
            | M::Fldln2
            | M::Fldz
            | M::Fsqrt
            | M::Frndint
            | M::Fprem
            | M::Fprem1
            | M::Fscale
            | M::Fxtract
            | M::F2xm1
            | M::Fsin
            | M::Fcos
            | M::Fsincos
            | M::Fptan
            | M::Fpatan
            | M::Fyl2x
            | M::Fyl2xp1
            | M::Fxam
            | M::Fabs
            | M::Fchs
            | M::Ftst
            | M::Fcompp
            | M::Fucompp
    ) {
        return true;
    }
    if m == M::Fxch {
        return st(i, 0);
    }
    if movement(i).is_some() {
        return st(i, 0) && st(i, 1);
    }
    if m == M::Fld {
        return st(i, 0) || memory(i, 0) && matches!(size, 4 | 8 | 10);
    }
    if m == M::Fild {
        return memory(i, 0) && matches!(size, 2 | 4 | 8);
    }
    if matches!(m, M::Fbld | M::Fbstp) {
        return memory(i, 0) && size == 10;
    }
    if store(i) {
        return st(i, 0) && matches!(m, M::Fst | M::Fstp)
            || memory(i, 0)
                && if integer(i) {
                    matches!(size, 2 | 4 | 8)
                } else {
                    matches!(size, 4 | 8 | 10)
                };
    }
    if kind(i).is_some() {
        return st(i, 0) && st(i, 1)
            || memory(i, 0)
                && if integer(i) {
                    matches!(size, 2 | 4)
                } else {
                    matches!(size, 4 | 8)
                };
    }
    if compare(i) {
        return if compares_flags(i) {
            st(i, 0) && st(i, 1)
        } else {
            st(i, 0)
                || memory(i, 0)
                    && if integer(i) {
                        matches!(size, 2 | 4)
                    } else {
                        matches!(size, 4 | 8)
                    }
        };
    }
    false
}
pub(super) fn memory_access(i: &Instruction) -> u8 {
    if memory(i, 0) {
        if store(i) {
            WRITE
        } else {
            READ
        }
    } else {
        0
    }
}
fn helper(i: &Instruction) -> Option<Helper> {
    use Helper::*;
    let m = i.mnemonic();
    let size = i.memory_size().size();
    if m == M::Fbld {
        return Some(BcdLoad);
    }
    if m == M::Fbstp {
        return Some(BcdStore);
    }
    if m == M::Fld && memory(i, 0) {
        return match size {
            4 => Some(Load32),
            8 => Some(Load64),
            _ => None,
        };
    }
    if m == M::Fild {
        return Some(Integer);
    }
    if store(i) && memory(i, 0) {
        return Some(if integer(i) {
            match (m == M::Fisttp, size) {
                (false, 2) => I16,
                (false, 4) => I32,
                (false, 8) => I64,
                (true, 2) => T16,
                (true, 4) => T32,
                _ => T64,
            }
        } else {
            match size {
                4 => Store32,
                8 => Store64,
                _ => return None,
            }
        });
    }
    if let Some(k) = kind(i) {
        if !memory(i, 0) {
            return Some(match k {
                0 => Add,
                1 => Mul,
                4 => Sub,
                _ => Div,
            });
        }
        let column = if integer(i) {
            if size == 2 {
                2
            } else {
                3
            }
        } else if size == 4 {
            0
        } else {
            1
        };
        return Some(
            match (k, reverse(i)) {
                (0, _) => [Add32, Add64, AddI16, AddI32],
                (1, _) => [Mul32, Mul64, MulI16, MulI32],
                (4, false) => [Sub32, Sub64, SubI16, SubI32],
                (4, true) => [RSub32, RSub64, RSubI16, RSubI32],
                (6, false) => [Div32, Div64, DivI16, DivI32],
                _ => [RDiv32, RDiv64, RDivI16, RDivI32],
            }[column],
        );
    }
    if compare(i) {
        return Some(if compares_flags(i) {
            if matches!(m, M::Fucomi | M::Fucomip) {
                UCmpi
            } else {
                Cmpi
            }
        } else if memory(i, 0) {
            match (integer(i), size) {
                (true, 2) => CmpI16,
                (true, _) => CmpI32,
                (false, 4) => Cmp32,
                _ => Cmp64,
            }
        } else if matches!(m, M::Fucom | M::Fucomp | M::Fucompp) {
            UCmp
        } else {
            Cmp
        });
    }
    Some(match m {
        M::Fld1 => One,
        M::Fldl2t => L2t,
        M::Fldl2e => L2e,
        M::Fldpi => Pi,
        M::Fldlg2 => Lg2,
        M::Fldln2 => Ln2,
        M::Fldz => Zero,
        M::Fsqrt => Sqrt,
        M::Frndint => Round,
        M::Fprem => Rem,
        M::Fprem1 => Rem1,
        M::Fscale => Scale,
        M::Fxtract => Extract,
        M::F2xm1 => Exp,
        M::Fsin => Sin,
        M::Fcos => Cos,
        M::Fsincos => SinCos,
        M::Fptan => Tan,
        M::Fpatan => Atan,
        M::Fyl2x => Log,
        M::Fyl2xp1 => Log1p,
        _ => return None,
    })
}
pub(super) fn collect(blocks: &[Block]) -> u128 {
    blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .filter(|i| supported(i))
        .filter_map(helper)
        .fold(0, |mask, h| mask | (1u128 << h as u32))
}
pub(super) fn imports(out: &mut Vec<u8>, mask: u128) {
    for (n, &name) in FUNCTIONS.iter().enumerate() {
        if mask & (1u128 << n) != 0 {
            binary::name(out, "core");
            binary::name(out, name);
            out.extend_from_slice(&[0, 9]);
        }
    }
}

fn index(c: &mut Code, bank: Bank, logical: u32) {
    c.get(bank.fsw());
    c.i32(11);
    c.op(0x76);
    c.i32(logical);
    c.op(0x6a);
    c.i32(7);
    c.op(0x71);
}
fn empty(c: &mut Code, bank: Bank, logical: u32) {
    c.get(bank.ftag());
    index(c, bank, logical);
    c.i32(1);
    c.op(0x74);
    c.op(0x76);
    c.i32(3);
    c.op(0x71);
    c.i32(3);
    c.op(0x46);
}
pub(super) fn read(c: &mut Code, bank: Bank, logical: u32) {
    index(c, bank, logical);
    c.set(TRANSLATION);
    c.get(bank.local(16));
    for physical in 1..8 {
        c.get(bank.local(16 + physical));
        c.get(TRANSLATION);
        c.i32(physical);
        c.op(0x47);
        c.op(0x1b);
    }
}
fn write(c: &mut Code, bank: Bank, logical: u32) {
    for physical in 0..8 {
        index(c, bank, logical);
        c.i32(physical);
        c.op(0x46);
        c.if_void();
        c.get(bank.scratch());
        payload(c);
        c.set(bank.local(16 + physical));
        c.get(bank.ftag());
        c.i32(!(3 << (physical * 2)));
        c.op(0x71);
        classify(c, bank, physical);
        c.i32(physical * 2);
        c.op(0x74);
        c.op(0x72);
        c.set(bank.ftag());
        c.end();
    }
}
fn shift(c: &mut Code, bank: Bank, delta: u32) {
    c.get(bank.fsw());
    c.i32(delta << 11);
    c.op(0x6a);
    c.i32(0x3800);
    c.op(0x71);
    c.get(bank.fsw());
    c.i32(!0x3800);
    c.op(0x71);
    c.op(0x72);
    c.set(bank.fsw());
}
fn pop(c: &mut Code, bank: Bank) {
    c.get(bank.ftag());
    c.i32(3);
    index(c, bank, 0);
    c.i32(1);
    c.op(0x74);
    c.op(0x74);
    c.op(0x72);
    c.set(bank.ftag());
    shift(c, bank, 1);
}
fn prepare(c: &mut Code, i: &Instruction, context: access::Context, calls: Calls) {
    let size = i.memory_size().size() as u32;
    let access = memory_access(i);
    if size == 10 {
        context.address(c, i, true);
        c.set(ADDRESS);
        c.get(3);
        c.get(ADDRESS);
        c.i32(10);
        c.i32(access as u32);
        c.call(calls.area);
        c.set(POINTER);
        c.get(POINTER);
        c.op(0x45);
        c.if_void();
        exit(c, context.out, i.ip(), Exit::Fault);
        c.end();
    } else {
        context.address(c, i, true);
        c.set(ADDRESS);
        if access == WRITE {
            context.probe(c, i.ip(), size, access);
        } else {
            context.prepare_address(c, i.ip(), size, access);
        }
    }
    if access == READ {
        if size == 10 {
            c.get(POINTER);
            c.load64(0);
            c.set(A);
            c.get(POINTER);
            c.memory(0x33, 8);
            c.set(B);
        } else {
            context.load(c, i.ip(), size, READ);
            if integer(i) {
                match size {
                    2 => {
                        c.i64(48);
                        c.op(0x86);
                        c.i64(48);
                        c.op(0x87);
                    }
                    4 => {
                        c.op(0xa7);
                        c.op(0xac);
                    }
                    _ => {}
                }
            }
            c.set(A);
            c.i64(0);
            c.set(B);
        }
    }
}
fn args(c: &mut Code, bank: Bank, logical: Option<u32>) {
    if let Some(n) = logical {
        read(c, bank, n);
        c.set(bank.scratch());
        for part in 0..2 {
            c.get(bank.scratch());
            lane(c, false, part);
        }
    } else {
        c.i64(0);
        c.i64(0);
    }
}
fn right(i: &Instruction) -> u32 {
    if st(i, 1) {
        logical(i, 1)
    } else if st(i, 0) {
        logical(i, 0)
    } else {
        1
    }
}
fn invoke(c: &mut Code, i: &Instruction, bank: Bank, calls: Calls, h: Helper) {
    c.get(3);
    c.get(bank.fcw());
    let binary = kind(i).is_some() || two_sources(i);
    let first = if binary && st(i, 0) { logical(i, 0) } else { 0 };
    args(
        c,
        bank,
        if load(i) {
            None
        } else if binary && reverse(i) && !memory(i, 0) {
            Some(right(i))
        } else {
            Some(first)
        },
    );
    if memory(i, 0) && !store(i) {
        c.get(A);
        c.get(B);
    } else if binary || compare(i) {
        if i.mnemonic() == M::Ftst {
            c.i64(0);
            c.i64(0);
        } else {
            args(
                c,
                bank,
                Some(if binary && reverse(i) {
                    first
                } else {
                    right(i)
                }),
            );
        }
    } else if i.mnemonic() == M::Fld && st(i, 0) {
        args(c, bank, Some(logical(i, 0)));
    } else {
        c.i64(0);
        c.i64(0);
    }
    c.get(bank.fsw());
    let bit = 1u128 << h as u32;
    assert!(calls.mask & bit != 0);
    c.call(calls.base + (calls.mask & (bit - 1)).count_ones());
    c.set(TRANSLATION);
    c.get(TRANSLATION);
    c.memory(0x28, offset_of!(Output, sw));
    c.op(0xad);
    c.set(EXTRA);
    c.get(TRANSLATION);
    c.memory(0x28, offset_of!(Output, commit));
    c.op(0xad);
    c.set(COUNT);
    c.get(TRANSLATION);
    c.vector_memory(0, offset_of!(Output, value));
    c.set(bank.scratch());
    if paired(i) {
        c.get(TRANSLATION);
        c.load64(offset_of!(crate::extended::Pair, pushed));
        c.set(A);
        c.get(TRANSLATION);
        c.load64(offset_of!(crate::extended::Pair, pushed) + 8);
        c.set(B);
    }
}
fn direct(c: &mut Code, i: &Instruction, bank: Bank) {
    if memory(i, 0) && load(i) {
        c.vector_const([0; 16]);
        c.get(A);
        lane(c, true, 0);
        c.get(B);
        lane(c, true, 1);
    } else {
        read(c, bank, if load(i) { logical(i, 0) } else { 0 });
    }
    if i.mnemonic() == M::Fabs {
        c.vector_const([
            255, 255, 255, 255, 255, 255, 255, 255, 255, 127, 0, 0, 0, 0, 0, 0,
        ]);
        c.simd(0x4e);
    }
    if i.mnemonic() == M::Fchs {
        c.vector_const([0, 0, 0, 0, 0, 0, 0, 0, 0, 128, 0, 0, 0, 0, 0, 0]);
        c.simd(0x51);
    }
    c.set(bank.scratch());
    c.get(bank.fsw());
    c.i32(!0x200);
    c.op(0x71);
    c.op(0xad);
    c.set(EXTRA);
    c.i64(1);
    c.set(COUNT);
}
fn fault(c: &mut Code, i: &Instruction, bank: Bank) {
    c.get(bank.fsw());
    c.i32(!0x200);
    c.op(0x71);
    c.i32(0x41);
    c.op(0x72);
    if load(i) || paired(i) {
        empty(c, bank, 7);
        c.op(0x45);
        if paired(i) {
            empty(c, bank, 0);
            c.op(0x45);
            c.op(0x71);
        }
        c.if_i32();
        c.i32(0x200);
        c.otherwise();
        c.i32(0);
        c.end();
        c.op(0x72);
    }
    c.get(bank.fcw());
    c.i32(1);
    c.op(0x71);
    c.op(0x45);
    c.if_i32();
    c.i32(0x8080);
    c.otherwise();
    c.i32(0);
    c.end();
    c.op(0x72);
    if compare(i) && !compares_flags(i) {
        c.get(bank.fcw());
        c.i32(1);
        c.op(0x71);
        c.if_i32();
        c.i32(0x4500);
        c.otherwise();
        c.i32(0);
        c.end();
        c.op(0x72);
    }
    c.op(0xad);
    c.set(EXTRA);
    c.get(bank.fcw());
    c.i32(1);
    c.op(0x71);
    c.op(0xad);
    c.set(COUNT);
    let value = if compares_flags(i) {
        0x45
    } else if store(i) && memory(i, 0) {
        let size = i.memory_size().size();
        if integer(i) {
            1u128 << (size * 8 - 1)
        } else {
            match size {
                4 => 0xffc0_0000,
                8 => 0xfff8_0000_0000_0000,
                _ => crate::extended::INDEFINITE,
            }
        }
    } else {
        crate::extended::INDEFINITE
    };
    c.vector_const(value.to_le_bytes());
    c.set(bank.scratch());
    if paired(i) {
        c.i64(value as u64);
        c.set(A);
        c.i64((value >> 64) as u64);
        c.set(B);
    }
}
fn exchange(c: &mut Code, i: &Instruction, bank: Bank) {
    let other = right(i);
    for (logical, save) in [(0, true), (other, false)] {
        read(c, bank, logical);
        c.set(bank.scratch());
        empty(c, bank, logical);
        c.open(0x04, 0x7b);
        c.vector_const(crate::extended::INDEFINITE.to_le_bytes());
        c.otherwise();
        c.get(bank.scratch());
        c.end();
        c.set(bank.scratch());
        if save {
            c.get(bank.scratch());
            lane(c, false, 0);
            c.set(A);
            c.get(bank.scratch());
            lane(c, false, 1);
            c.set(B);
        }
    }
    write(c, bank, 0);
    c.vector_const([0; 16]);
    c.get(A);
    lane(c, true, 0);
    c.get(B);
    lane(c, true, 1);
    c.set(bank.scratch());
    write(c, bank, other);
}
fn examine(c: &mut Code, bank: Bank) {
    read(c, bank, 0);
    c.set(bank.scratch());
    c.get(bank.scratch());
    lane(c, false, 0);
    c.set(A);
    c.get(bank.scratch());
    lane(c, false, 1);
    c.op(0xa7);
    c.set(STATUS);
    c.get(STATUS);
    c.i32(0x7fff);
    c.op(0x71);
    c.set(TRANSLATION);
    empty(c, bank, 0);
    c.if_i32();
    c.i32(0x4100);
    c.otherwise();
    c.get(TRANSLATION);
    c.op(0x45);
    c.if_i32();
    c.get(A);
    c.op(0x50);
    c.if_i32();
    c.i32(0x4000);
    c.otherwise();
    c.i32(0x4400);
    c.end();
    c.otherwise();
    c.get(A);
    c.i64(1 << 63);
    c.op(0x83);
    c.op(0x50);
    c.if_i32();
    c.i32(0);
    c.otherwise();
    c.get(TRANSLATION);
    c.i32(0x7fff);
    c.op(0x46);
    c.if_i32();
    c.get(A);
    c.i64(0x7fff_ffff_ffff_ffff);
    c.op(0x83);
    c.op(0x50);
    c.if_i32();
    c.i32(0x500);
    c.otherwise();
    c.i32(0x100);
    c.end();
    c.otherwise();
    c.i32(0x400);
    c.end();
    c.end();
    c.end();
    c.end();
    c.get(STATUS);
    c.i32(6);
    c.op(0x76);
    c.i32(0x200);
    c.op(0x71);
    c.op(0x72);
    c.get(bank.fsw());
    c.i32(!0x4700);
    c.op(0x71);
    c.op(0x72);
    c.set(bank.fsw());
}
pub(super) fn emit(
    c: &mut Code,
    i: &Instruction,
    known: &mut Option<flags::Recipe>,
    context: access::Context,
    bank: Bank,
    calls: Calls,
) {
    mmx::wait(c, i, context, bank);
    if remainder(i) || trigonometric(i) {
        c.get(bank.fsw());
        c.i32(!0x400);
        c.op(0x71);
        c.set(bank.fsw());
    }
    if i.mnemonic() == M::Fxam {
        examine(c, bank);
        stamp(c, i, bank, calls.opcode);
        return;
    }
    if memory(i, 0) {
        prepare(c, i, context, calls);
    }
    if paired(i) {
        empty(c, bank, 0);
        empty(c, bank, 7);
        c.op(0x45);
        c.op(0x72);
    } else if load(i) {
        empty(c, bank, 7);
        c.op(0x45);
        if st(i, 0) {
            empty(c, bank, logical(i, 0));
            c.op(0x72);
        }
    } else {
        empty(c, bank, 0);
        if kind(i).is_some() && st(i, 0) {
            empty(c, bank, logical(i, 0));
            c.op(0x72);
        }
        if (kind(i).is_some()
            || two_sources(i)
            || compare(i)
            || i.mnemonic() == M::Fxch
            || movement(i).is_some())
            && !memory(i, 0)
            && i.mnemonic() != M::Ftst
        {
            empty(c, bank, right(i));
            c.op(0x72);
        }
    }
    c.if_void();
    fault(c, i, bank);
    c.otherwise();
    if let Some(h) = helper(i) {
        invoke(c, i, bank, calls, h);
    } else if let Some(cc) = movement(i) {
        c.get(bank.fsw());
        c.op(0xad);
        c.set(EXTRA);
        flags::condition(c, cc, *known);
        c.if_void();
        read(c, bank, right(i));
        c.set(bank.scratch());
        c.i64(1);
        c.set(COUNT);
        c.otherwise();
        c.i64(0);
        c.set(COUNT);
        c.end();
    } else if i.mnemonic() == M::Fxch {
        c.get(bank.fsw());
        c.i32(!0x200);
        c.op(0x71);
        c.op(0xad);
        c.set(EXTRA);
        c.i64(1);
        c.set(COUNT);
    } else {
        direct(c, i, bank);
    }
    c.end();
    if compares_flags(i) {
        c.i32(0);
        c.set(STATUS);
        flags::replace(c, 0x890, known);
    }
    c.get(COUNT);
    c.i64(0);
    c.op(0x52);
    c.if_void();
    if memory(i, 0) && store(i) {
        c.get(bank.scratch());
        lane(c, false, 0);
        c.set(RESULT);
        let size = i.memory_size().size() as u32;
        if size == 10 {
            c.get(POINTER);
            c.get(RESULT);
            c.store64(0);
            c.get(POINTER);
            c.get(bank.scratch());
            lane(c, false, 1);
            c.op(0xa7);
            c.memory(0x3b, 8);
            c.get(3);
            c.get(ADDRESS);
            c.i32(10);
            c.call(calls.area + 1);
            c.set(TRANSLATION);
            context.store_status(c, i.ip());
        } else {
            context.store(c, i.ip(), size);
        }
    }
    c.get(EXTRA);
    c.op(0xa7);
    c.set(bank.fsw());
    if compares_flags(i) {
        c.get(FLAGS);
        c.i64(!0x8d5);
        c.op(0x83);
        c.get(bank.scratch());
        lane(c, false, 0);
        c.op(0x84);
        c.set(FLAGS);
        flags::discard(c);
    }
    if paired(i) {
        write(c, bank, 0);
        shift(c, bank, 7);
        c.vector_const([0; 16]);
        c.get(A);
        lane(c, true, 0);
        c.get(B);
        lane(c, true, 1);
        c.set(bank.scratch());
        write(c, bank, 0);
    } else if i.mnemonic() == M::Fxch {
        exchange(c, i, bank);
    } else if !compare(i) && !(store(i) && memory(i, 0)) {
        if load(i) {
            shift(c, bank, 7);
        }
        write(
            c,
            bank,
            if binary_pop(i) {
                1
            } else if store(i) || kind(i).is_some() {
                if st(i, 0) {
                    logical(i, 0)
                } else {
                    0
                }
            } else {
                0
            },
        );
    }
    for _ in 0..pops(i) {
        pop(c, bank);
    }
    c.otherwise();
    c.get(EXTRA);
    c.op(0xa7);
    c.set(bank.fsw());
    c.end();
    stamp(c, i, bank, calls.opcode);
    if memory(i, 0) {
        c.get(ADDRESS);
        c.set(bank.fdp());
    }
    if compares_flags(i) {
        *known = None;
    }
}
