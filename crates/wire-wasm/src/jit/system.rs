use super::*;
use iced_x86::{Mnemonic as M, OpKind};

pub(super) fn timestamp(i: &Instruction) -> bool {
    matches!(i.mnemonic(), M::Rdtsc | M::Rdtscp)
}

pub(super) fn supported(i: &Instruction) -> bool {
    if i.has_lock_prefix() {
        return false;
    }
    match i.mnemonic() {
        M::Cpuid | M::Rdtsc | M::Rdtscp | M::Xgetbv | M::Sfence | M::Lfence | M::Mfence => true,
        M::Prefetchw | M::Prefetchnta | M::Prefetcht0 | M::Prefetcht1 | M::Prefetcht2 => {
            i.op0_kind() == OpKind::Memory
        }
        M::Movnti => {
            i.op0_kind() == OpKind::Memory
                && matches!(i.memory_size().size(), 4 | 8)
                && scalar::register_operand(i, 1)
        }
        _ => false,
    }
}

pub(super) fn memory_access(i: &Instruction) -> u8 {
    if i.mnemonic() == M::Movnti {
        crate::memory::WRITE
    } else {
        0
    }
}

fn cpuid(c: &mut Code) {
    for (register, local) in [(0, A), (1, B)] {
        c.get(GPR + register);
        scalar::truncate(c, 32);
        c.set(local);
    }
    for register in [0, 3, 1, 2] {
        c.i64(0);
        c.set(GPR + register);
    }
    let done = c.block();
    for &(leaf, subleaf, value) in &crate::cpu::LEAVES {
        c.get(A);
        c.i64(leaf as u64);
        c.op(0x51);
        if let Some(subleaf) = subleaf {
            c.get(B);
            c.i64(subleaf as u64);
            c.op(0x51);
            c.op(0x71);
        }
        c.if_void();
        for (register, value) in [0, 3, 1, 2].into_iter().zip(value) {
            c.i64(value as u64);
            c.set(GPR + register);
        }
        c.branch(done);
        c.end();
    }
    c.end();
}

pub(super) fn emit(c: &mut Code, i: &Instruction, context: access::Context, clock: u32) {
    match i.mnemonic() {
        M::Cpuid => cpuid(c),
        M::Rdtsc | M::Rdtscp => {
            c.call(clock);
            c.set(STATUS);
            c.set(COUNT);
            c.get(STATUS);
            c.if_void();
            exit(c, context.out, i.ip(), Exit::HostError);
            c.end();
            c.get(COUNT);
            scalar::truncate(c, 32);
            c.set(GPR);
            c.get(COUNT);
            c.i64(32);
            c.op(0x88);
            c.set(GPR + 2);
            if i.mnemonic() == M::Rdtscp {
                c.i64(0);
                c.set(GPR + 1);
            }
        }
        M::Xgetbv => exception(c, context.out, i.ip(), 6),
        M::Movnti => {
            let size = i.memory_size().size() as u32;
            context.prepare(c, i, size, crate::memory::WRITE);
            scalar::read(c, i, 1, context);
            c.set(RESULT);
            context.store(c, i.ip(), size);
        }
        // A cooperative VM owns all guest accesses; hints add no observable
        // memory operation, and fences preserve the existing program order.
        M::Sfence
        | M::Lfence
        | M::Mfence
        | M::Prefetchw
        | M::Prefetchnta
        | M::Prefetcht0
        | M::Prefetcht1
        | M::Prefetcht2 => {}
        _ => unreachable!(),
    }
}
