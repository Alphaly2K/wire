//! x87 instructions and explicit FP context transfers use the resident bank. The
//! core's bounded image buffer transfers bytes without transacting CPU backing.
use super::*;
mod environment;
mod values;

#[derive(Clone, Copy)]
pub(in crate::jit) struct Calls {
    pub area: u32,
    pub base: u32,
    pub mask: u128,
    pub opcode: u32,
}
pub(in crate::jit) fn collect(blocks: &[Block]) -> u128 {
    values::collect(blocks)
}
pub(in crate::jit) fn imports(out: &mut Vec<u8>, mask: u128) {
    values::imports(out, mask);
}
pub(super) fn needs_area(i: &Instruction) -> bool {
    area(i) || values::supported(i) && i.memory_size().size() == 10
}
pub(super) fn uses_values(i: &Instruction) -> bool {
    values::supported(i) || matches!(i.mnemonic(), M::Fldenv | M::Frstor | M::Fnsave)
}
pub(super) fn writes_values(i: &Instruction) -> bool {
    values::supported(i) && values::writes(i) || i.mnemonic() == M::Frstor
}

pub(super) fn area(i: &Instruction) -> bool {
    fx(i) || environment::supported(i)
}
pub(super) fn fx(i: &Instruction) -> bool {
    matches!(
        i.mnemonic(),
        M::Fxsave | M::Fxsave64 | M::Fxrstor | M::Fxrstor64
    )
}
pub(super) fn restore(i: &Instruction) -> bool {
    matches!(
        i.mnemonic(),
        M::Fxrstor | M::Fxrstor64 | M::Fldenv | M::Frstor
    )
}
pub(super) fn environment(i: &Instruction) -> bool {
    area(i)
        || values::supported(i)
        || matches!(
            i.mnemonic(),
            M::Fninit | M::Fnop | M::Fincstp | M::Fdecstp | M::Ffree | M::Ffreep
        )
}
pub(super) fn supported(i: &Instruction) -> bool {
    if values::supported(i) {
        return true;
    }
    if i.has_lock_prefix() {
        return false;
    }
    if area(i) {
        return memory(i, 0);
    }
    match i.mnemonic() {
        M::Wait | M::Fnclex | M::Fninit | M::Fnop | M::Fincstp | M::Fdecstp => true,
        M::Ffree | M::Ffreep => {
            i.op0_kind() == OpKind::Register
                && (Register::ST0..=Register::ST7).contains(&i.op0_register())
        }
        M::Fldcw | M::Fnstcw => memory(i, 0),
        M::Fnstsw => memory(i, 0) || i.op0_register() == Register::AX,
        _ => false,
    }
}
pub(super) fn memory_access(i: &Instruction) -> u8 {
    if values::supported(i) {
        return values::memory_access(i);
    }
    if memory(i, 0) {
        if restore(i) || i.mnemonic() == M::Fldcw {
            READ
        } else {
            WRITE
        }
    } else {
        0
    }
}

fn initialize(c: &mut Code, bank: Bank) {
    for (local, value) in [
        (bank.fcw(), 0x37f),
        (bank.fsw(), 0),
        (bank.ftag(), 0xffff),
        (bank.fop(), 0),
    ] {
        c.i32(value);
        c.set(local);
    }
    for local in [bank.fip(), bank.fdp()] {
        c.i64(0);
        c.set(local);
    }
}

fn control(c: &mut Code, bank: Bank) {
    // CW reserved bits read as zero, except bit 6 which reads as one.
    c.i32(0x1f3f);
    c.op(0x71);
    c.i32(0x40);
    c.op(0x72);
    c.set(bank.fcw());
}

fn summary(c: &mut Code, bank: Bank) {
    c.get(bank.fsw());
    c.i32(!0x8080);
    c.op(0x71);
    c.get(bank.fcw());
    c.i32(!0);
    c.op(0x73);
    c.get(bank.fsw());
    c.op(0x71);
    c.i32(0x3f);
    c.op(0x71);
    c.if_i32();
    c.i32(0x8080);
    c.otherwise();
    c.i32(0);
    c.end();
    c.op(0x72);
    c.set(bank.fsw());
}

fn top(c: &mut Code, bank: Bank, delta: u32) {
    c.get(bank.fsw());
    c.i32(delta << 11);
    c.op(0x6a);
    c.i32(0x3800);
    c.op(0x71);
    c.get(bank.fsw());
    c.i32(!0x3a00);
    c.op(0x71);
    c.op(0x72);
    c.set(bank.fsw());
}

fn stamp(c: &mut Code, i: &Instruction, bank: Bank, opcode: u32) {
    c.i64(i.ip());
    c.set(bank.fip());
    c.i32(opcode);
    c.set(bank.fop());
}

fn payload(c: &mut Code) {
    c.vector_const([
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0,
    ]);
    c.simd(0x4e);
}

fn save(c: &mut Code, i: &Instruction, context: access::Context, bank: Bank, calls: u32) {
    for (local, offset) in [(bank.fcw(), 0), (bank.fsw(), 2), (bank.fop(), 6)] {
        c.get(POINTER);
        c.get(local);
        if offset == 6 {
            c.i32(0x7ff);
            c.op(0x71);
        }
        c.memory(0x3b, offset);
    }
    c.get(POINTER);
    c.i32(0);
    for physical in 0..8 {
        c.get(bank.ftag());
        c.i32(physical * 2);
        c.op(0x76);
        c.i32(3);
        c.op(0x71);
        c.i32(3);
        c.op(0x47);
        c.i32(physical);
        c.op(0x74);
        c.op(0x72);
    }
    c.memory(0x3a, 4);
    for (local, offset) in [(bank.fip(), 8), (bank.fdp(), 16)] {
        c.get(POINTER);
        c.get(local);
        if i.mnemonic() != M::Fxsave64 {
            scalar::truncate(c, 32);
        }
        c.store64(offset);
    }
    c.get(POINTER);
    c.get(bank.mxcsr());
    c.memory(0x36, 24);
    c.get(POINTER);
    c.i32(0xffff);
    c.memory(0x36, 28);
    for slot in 0..8 {
        c.get(POINTER);
        values::read(c, bank, slot);
        payload(c);
        c.vector_memory(11, 32 + slot as usize * 16);
    }
    for register in 0..if context.mode == Mode::X64 { 16 } else { 8 } {
        c.get(POINTER);
        c.get(bank.local(register));
        c.vector_memory(11, 160 + register as usize * 16);
    }
    c.get(3);
    c.get(ADDRESS);
    c.i32(464);
    c.call(calls + 1);
    c.set(TRANSLATION);
    context.store_status(c, i.ip());
}

fn classify(c: &mut Code, bank: Bank, physical: u32) {
    c.get(bank.local(16 + physical));
    lane(c, false, 0);
    c.set(EXTRA);
    c.get(bank.local(16 + physical));
    lane(c, false, 1);
    c.op(0xa7);
    c.i32(0x7fff);
    c.op(0x71);
    c.set(TRANSLATION);
    c.get(TRANSLATION);
    c.op(0x45);
    c.get(EXTRA);
    c.op(0x50);
    c.op(0x71);
    c.if_i32();
    c.i32(1);
    c.otherwise();
    c.get(TRANSLATION);
    c.i32(0);
    c.op(0x47);
    c.get(TRANSLATION);
    c.i32(0x7fff);
    c.op(0x47);
    c.op(0x71);
    c.get(EXTRA);
    c.i64(0);
    c.op(0x53); // signed < 0: the explicit integer bit is set
    c.op(0x71);
    c.if_i32();
    c.i32(0);
    c.otherwise();
    c.i32(2);
    c.end();
    c.end();
}

fn load(c: &mut Code, i: &Instruction, context: access::Context, bank: Bank) {
    c.get(POINTER);
    c.memory(0x28, 24);
    c.set(TRANSLATION);
    c.get(TRANSLATION);
    c.i32(!0xffff);
    c.op(0x71);
    c.if_void();
    exception(c, context.out, i.ip(), 13);
    c.end();
    c.get(TRANSLATION);
    c.set(bank.mxcsr());
    for (local, offset) in [(bank.fcw(), 0), (bank.fsw(), 2), (bank.fop(), 6)] {
        c.get(POINTER);
        c.memory(0x2f, offset);
        if offset == 6 {
            c.i32(0x7ff);
            c.op(0x71);
        }
        if offset == 0 {
            control(c, bank);
        } else {
            c.set(local);
        }
    }
    summary(c, bank);
    for (local, offset) in [(bank.fip(), 8), (bank.fdp(), 16)] {
        c.get(POINTER);
        c.memory(
            if i.mnemonic() == M::Fxrstor64 {
                0x29
            } else {
                0x35
            },
            offset,
        );
        c.set(local);
    }
    c.i32(0);
    c.set(bank.ftag());
    for physical in 0..8 {
        c.get(POINTER);
        c.i32(physical + 8);
        c.get(bank.fsw());
        c.i32(11);
        c.op(0x76);
        c.op(0x6b);
        c.i32(7);
        c.op(0x71);
        c.i32(4);
        c.op(0x74);
        c.op(0x6a);
        c.vector_memory(0, 32);
        payload(c);
        c.set(bank.local(16 + physical));
        c.get(bank.ftag());
        c.get(POINTER);
        c.memory(0x2d, 4);
        c.i32(1 << physical);
        c.op(0x71);
        c.if_i32();
        classify(c, bank, physical);
        c.otherwise();
        c.i32(3);
        c.end();
        c.i32(physical * 2);
        c.op(0x74);
        c.op(0x72);
        c.set(bank.ftag());
    }
    for register in 0..if context.mode == Mode::X64 { 16 } else { 8 } {
        c.get(POINTER);
        c.vector_memory(0, 160 + register as usize * 16);
        c.set(bank.local(register));
    }
}

fn context(c: &mut Code, i: &Instruction, context: access::Context, bank: Bank, calls: u32) {
    context.address(c, i, true);
    c.set(ADDRESS);
    c.get(ADDRESS);
    c.i64(15);
    c.op(0x83);
    c.i64(0);
    c.op(0x52);
    c.if_void();
    exception(c, context.out, i.ip(), 13);
    c.end();
    c.get(3);
    c.get(ADDRESS);
    c.i32(if restore(i) {
        if context.mode == Mode::X64 {
            416
        } else {
            288
        }
    } else {
        464
    });
    c.i32(memory_access(i) as u32);
    c.call(calls);
    c.set(POINTER);
    c.get(POINTER);
    c.op(0x45);
    c.if_void();
    exit(c, context.out, i.ip(), Exit::Fault);
    c.end();
    if restore(i) {
        load(c, i, context, bank);
    } else {
        save(c, i, context, bank, calls);
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
    if values::supported(i) {
        values::emit(c, i, known, context, bank, calls);
        return;
    }
    let m = i.mnemonic();
    if area(i) {
        if environment::supported(i) {
            environment::emit(c, i, context, bank, calls.area);
        } else {
            self::context(c, i, context, bank, calls.area);
        }
        return;
    }
    if matches!(
        m,
        M::Wait | M::Fldcw | M::Fnop | M::Fincstp | M::Fdecstp | M::Ffree | M::Ffreep
    ) {
        mmx::wait(c, i, context, bank);
    }
    if memory(i, 0) {
        context.prepare(c, i, 2, memory_access(i));
    }
    match m {
        M::Wait => {}
        M::Fninit => initialize(c, bank),
        M::Fnclex => {
            c.get(bank.fsw());
            c.i32(0x7f00);
            c.op(0x71);
            c.set(bank.fsw());
        }
        M::Fldcw => {
            context.load(c, i.ip(), 2, READ);
            c.op(0xa7);
            control(c, bank);
            summary(c, bank);
        }
        M::Fnstcw | M::Fnstsw => {
            c.get(if m == M::Fnstcw {
                bank.fcw()
            } else {
                bank.fsw()
            });
            c.op(0xad);
            c.set(RESULT);
            if memory(i, 0) {
                context.store(c, i.ip(), 2);
            } else {
                scalar::write(c, i, 0, context);
            }
        }
        M::Fnop => stamp(c, i, bank, 0x1d0),
        M::Fincstp | M::Fdecstp => {
            top(c, bank, if m == M::Fincstp { 1 } else { 7 });
            stamp(c, i, bank, if m == M::Fincstp { 0x1f7 } else { 0x1f6 });
        }
        M::Ffree | M::Ffreep => {
            let index = i.op0_register() as u32 - Register::ST0 as u32;
            c.get(bank.ftag());
            c.i32(3);
            c.get(bank.fsw());
            c.i32(11);
            c.op(0x76);
            c.i32(index);
            c.op(0x6a);
            c.i32(7);
            c.op(0x71);
            c.i32(1);
            c.op(0x74);
            c.op(0x74);
            c.op(0x72);
            c.set(bank.ftag());
            if m == M::Ffreep {
                c.get(bank.ftag());
                c.i32(3);
                c.get(bank.fsw());
                c.i32(10);
                c.op(0x76);
                c.i32(14);
                c.op(0x71);
                c.op(0x74);
                c.op(0x72);
                c.set(bank.ftag());
                top(c, bank, 1);
            }
            stamp(
                c,
                i,
                bank,
                if m == M::Ffree { 0x5c0 } else { 0x7c0 } | index,
            );
        }
        _ => unreachable!(),
    }
}
