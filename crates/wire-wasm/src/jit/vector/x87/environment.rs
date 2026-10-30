use super::*;

pub(super) fn supported(i: &Instruction) -> bool {
    matches!(i.mnemonic(), M::Fldenv | M::Fnstenv | M::Frstor | M::Fnsave)
}

fn save(c: &mut Code, bank: Bank, header: usize, full: bool) {
    let stride = if header == 14 { 2 } else { 4 };
    for (n, local) in [bank.fcw(), bank.fsw(), bank.ftag()]
        .into_iter()
        .enumerate()
    {
        c.get(POINTER);
        c.get(local);
        c.memory(0x3b, n * stride);
    }
    for (offset, local) in [(3 * stride, bank.fip()), (5 * stride, bank.fdp())] {
        c.get(POINTER);
        c.get(local);
        c.op(0xa7);
        c.memory(if stride == 2 { 0x3b } else { 0x36 }, offset);
    }
    if header == 28 {
        c.get(POINTER);
        c.get(bank.fop());
        c.i32(0x7ff);
        c.op(0x71);
        c.memory(0x3b, 18);
        for offset in [2, 6, 10, 26] {
            c.get(POINTER);
            c.i32(0xffff);
            c.memory(0x3b, offset);
        }
    }
    if full {
        for slot in 0..8 {
            values::read(c, bank, slot);
            c.set(bank.scratch());
            for (part, offset) in [(0, 0), (1, 8)] {
                c.get(POINTER);
                c.get(bank.scratch());
                lane(c, false, part);
                if part == 1 {
                    c.op(0xa7);
                }
                c.memory(
                    if part == 0 { 0x37 } else { 0x3b },
                    header + slot as usize * 10 + offset,
                );
            }
        }
    }
}

fn load(c: &mut Code, bank: Bank, header: usize, full: bool) {
    let stride = if header == 14 { 2 } else { 4 };
    c.get(POINTER);
    c.memory(0x2f, 0);
    control(c, bank);
    c.get(POINTER);
    c.memory(0x2f, stride);
    c.set(bank.fsw());
    summary(c, bank);
    c.get(POINTER);
    c.memory(0x2f, 2 * stride);
    c.set(STATUS);
    for (offset, local) in [(3 * stride, bank.fip()), (5 * stride, bank.fdp())] {
        c.get(POINTER);
        c.memory(if stride == 2 { 0x33 } else { 0x35 }, offset);
        c.set(local);
    }
    if header == 28 {
        c.get(POINTER);
        c.memory(0x2f, 18);
        c.i32(0x7ff);
        c.op(0x71);
    } else {
        // The protected-mode 16-bit image has no opcode field.
        c.i32(0);
    }
    c.set(bank.fop());
    if full {
        for physical in 0..8 {
            c.get(POINTER);
            c.i32(physical + 8);
            c.get(bank.fsw());
            c.i32(11);
            c.op(0x76);
            c.op(0x6b);
            c.i32(7);
            c.op(0x71);
            c.i32(10);
            c.op(0x6c);
            c.op(0x6a);
            c.set(TRANSLATION);
            c.vector_const([0; 16]);
            for (part, offset) in [(0, 0), (1, 8)] {
                c.get(TRANSLATION);
                c.memory(if part == 0 { 0x29 } else { 0x33 }, header + offset);
                lane(c, true, part);
            }
            c.set(bank.local(16 + physical));
        }
    }
    c.i32(0);
    c.set(bank.ftag());
    for physical in 0..8 {
        c.get(bank.ftag());
        c.get(STATUS);
        c.i32(physical * 2);
        c.op(0x76);
        c.i32(3);
        c.op(0x71);
        c.i32(3);
        c.op(0x46);
        c.if_i32();
        c.i32(3);
        c.otherwise();
        classify(c, bank, physical);
        c.end();
        c.i32(physical * 2);
        c.op(0x74);
        c.op(0x72);
        c.set(bank.ftag());
    }
}

pub(super) fn emit(
    c: &mut Code,
    i: &Instruction,
    context: access::Context,
    bank: Bank,
    calls: u32,
) {
    let restoring = restore(i);
    if restoring {
        mmx::wait(c, i, context, bank);
    }
    let length = i.memory_size().size();
    let header = if matches!(length, 14 | 94) { 14 } else { 28 };
    let full = length > 28;
    context.address(c, i, true);
    c.set(ADDRESS);
    c.get(3);
    c.get(ADDRESS);
    c.i32(length as u32);
    c.i32(memory_access(i) as u32);
    c.call(calls);
    c.set(POINTER);
    c.get(POINTER);
    c.op(0x45);
    c.if_void();
    exit(c, context.out, i.ip(), Exit::Fault);
    c.end();
    if restoring {
        load(c, bank, header, full);
    } else {
        save(c, bank, header, full);
        c.get(3);
        c.get(ADDRESS);
        c.i32(length as u32);
        c.call(calls + 1);
        c.set(TRANSLATION);
        context.store_status(c, i.ip());
        if full {
            initialize(c, bank);
        } else {
            c.get(bank.fcw());
            c.i32(0x3f);
            c.op(0x72);
            c.set(bank.fcw());
            summary(c, bank);
        }
    }
}
