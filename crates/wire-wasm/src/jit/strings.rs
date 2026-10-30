use super::*;
use crate::{
    machine::Repeat,
    memory::{READ, WRITE},
};
use iced_x86::{Mnemonic as M, OpKind, Register};

#[derive(Clone, Copy)]
enum Operation {
    Move,
    Store,
    Load,
    Compare,
    Scan,
}

impl Operation {
    fn decode(i: &Instruction) -> Option<Self> {
        if !i.is_string_instruction() || i.has_lock_prefix() {
            return None;
        }
        Some(match i.mnemonic() {
            M::Movsb | M::Movsw | M::Movsd | M::Movsq => Self::Move,
            M::Stosb | M::Stosw | M::Stosd | M::Stosq => Self::Store,
            M::Lodsb | M::Lodsw | M::Lodsd | M::Lodsq => Self::Load,
            M::Cmpsb | M::Cmpsw | M::Cmpsd | M::Cmpsq => Self::Compare,
            M::Scasb | M::Scasw | M::Scasd | M::Scasq => Self::Scan,
            _ => return None,
        })
    }

    fn source(self) -> bool {
        matches!(self, Self::Move | Self::Load | Self::Compare)
    }

    fn destination(self) -> bool {
        !matches!(self, Self::Load)
    }

    fn compares(self) -> bool {
        matches!(self, Self::Compare | Self::Scan)
    }

    fn writes(self) -> bool {
        matches!(self, Self::Move | Self::Store)
    }
}

pub(super) fn supported(i: &Instruction) -> bool {
    Operation::decode(i).is_some()
}

pub(super) fn memory_access(i: &Instruction) -> u8 {
    match Operation::decode(i).unwrap() {
        Operation::Move => READ | WRITE,
        Operation::Store => WRITE,
        _ => READ,
    }
}

fn address_bits(i: &Instruction) -> u32 {
    i.op_kinds()
        .find_map(|kind| match kind {
            OpKind::MemorySegSI | OpKind::MemoryESDI => Some(16),
            OpKind::MemorySegESI | OpKind::MemoryESEDI => Some(32),
            OpKind::MemorySegRSI | OpKind::MemoryESRDI => Some(64),
            _ => None,
        })
        .expect("string address width")
}

const RESTART: usize = offset_of!(State, repeat);

fn cancel(c: &mut Code) {
    c.get(0);
    c.i32(0);
    c.memory(0x36, RESTART + offset_of!(Repeat, active));
}

fn snapshot(c: &mut Code, i: &Instruction, source: &Source, materialize: u32) {
    c.get(0);
    c.memory(0x28, RESTART + offset_of!(Repeat, active));
    c.i32(1);
    c.op(0x46);
    let identity = source.identity(i);
    for (offset, value) in [
        (offset_of!(Repeat, pc), i.ip()),
        (offset_of!(Repeat, code), identity[0]),
        (offset_of!(Repeat, code) + 8, identity[1]),
    ] {
        c.get(0);
        c.load64(RESTART + offset);
        c.i64(value);
        c.op(0x51);
        c.op(0x71);
    }
    c.op(0x45);
    c.if_void();
    flags::materialize(c, materialize);
    for (offset, value) in [
        (offset_of!(Repeat, pc), i.ip()),
        (offset_of!(Repeat, code), identity[0]),
        (offset_of!(Repeat, code) + 8, identity[1]),
    ] {
        c.get(0);
        c.i64(value);
        c.store64(RESTART + offset);
    }
    c.get(0);
    c.get(FLAGS);
    c.store64(RESTART + offset_of!(Repeat, flags));
    c.get(0);
    c.i32(1);
    c.memory(0x36, RESTART + offset_of!(Repeat, active));
    c.end();
}

fn increment(c: &mut Code, local: u32) {
    c.get(local);
    c.i64(1);
    c.op(0x7c);
    c.set(local);
}

fn pointer(c: &mut Code, index: u32, bits: u32) {
    c.get(GPR + index);
    c.get(COUNT);
    c.op(0x7c);
    c.set(RESULT);
    scalar::write_gpr(c, index, bits);
}

fn address(c: &mut Code, i: &Instruction, context: access::Context, source: bool, bits: u32) {
    c.get(GPR + if source { 6 } else { 7 });
    scalar::truncate(c, bits);
    if source {
        let segment = match i.segment_prefix() {
            Register::FS => Some(offset_of!(State, fs_base)),
            Register::GS => Some(offset_of!(State, gs_base)),
            _ => None,
        };
        if let Some(offset) = segment {
            c.get(0);
            c.load64(offset);
            c.op(0x7c);
            if context.mode == Mode::X86 {
                scalar::truncate(c, 32);
            }
        }
    }
    c.set(ADDRESS);
}

pub(super) fn emit(
    c: &mut Code,
    i: &Instruction,
    known: &mut Option<flags::Recipe>,
    context: access::Context,
    source: &Source,
) {
    let operation = Operation::decode(i).unwrap();
    let size = i.memory_size().size() as u32;
    let bits = address_bits(i);
    let repeated = i.has_rep_prefix() || i.has_repne_prefix();
    let restore = repeated && operation.compares();
    let done = c.block();
    if repeated {
        c.get(GPR + 1);
        scalar::truncate(c, bits);
        c.op(0x50);
        c.if_void();
        increment(c, WORK);
        increment(c, RETIRED);
        if restore {
            cancel(c);
        }
        c.branch(done);
        c.end();
    }
    if restore {
        snapshot(c, i, source, context.flags);
    }
    // DF is unaffected by arithmetic recipes. The signed step is constant
    // throughout this instruction, including all of its completed elements.
    c.i64(size as u64);
    c.i64((0u64).wrapping_sub(size as u64));
    c.get(FLAGS);
    c.i64(1024);
    c.op(0x83);
    c.op(0x50);
    c.op(0x1b);
    c.set(COUNT);

    let failure = restore.then(|| c.block());
    let access = access::Context {
        out: failure.unwrap_or(context.out),
        ..context
    };
    let again = repeated.then(|| c.open(0x03, 0x40));
    if operation.source() {
        address(c, i, access, true, bits);
        access.prepare_address(c, i.ip(), size, READ);
        access.load(c, i.ip(), size, READ);
    } else {
        c.get(GPR);
        scalar::truncate(c, size * 8);
    }
    c.set(A);
    if operation.destination() {
        address(c, i, access, false, bits);
        access.prepare_address(
            c,
            i.ip(),
            size,
            if operation.writes() { WRITE } else { READ },
        );
        if operation.writes() {
            c.get(A);
            c.set(RESULT);
            access.store(c, i.ip(), size);
        } else {
            access.load(c, i.ip(), size, READ);
            c.set(B);
        }
    } else {
        c.get(A);
        c.set(RESULT);
        scalar::write_gpr(c, 0, size * 8);
    }
    if operation.compares() {
        c.get(A);
        c.get(B);
        c.op(0x7d);
        scalar::truncate(c, size * 8);
        c.set(RESULT);
        flags::defer(
            c,
            flags::Recipe {
                kind: flags::SUB,
                bits: size * 8,
                preserve_cf: false,
                carry_in: false,
                overridden: 0,
            },
            None,
        );
        // A zero-count REP retains the incoming recipe.
        *known = None;
    }
    if operation.source() {
        pointer(c, 6, bits);
    }
    if operation.destination() {
        pointer(c, 7, bits);
    }
    increment(c, WORK);
    if let Some(again) = again {
        c.get(GPR + 1);
        c.i64(1);
        c.op(0x7d);
        c.set(RESULT);
        scalar::write_gpr(c, 1, bits);
        c.get(GPR + 1);
        scalar::truncate(c, bits);
        c.op(0x50);
        c.op(0x45);
        if operation.compares() {
            c.get(VALUE);
            c.op(0x50);
            if i.has_repne_prefix() {
                c.op(0x45);
            }
            c.op(0x71);
        }
        c.if_void();
        if operation.writes() {
            c.get(CHANGED);
            c.if_void();
            exit(c, context.out, i.ip(), Exit::SourceChanged);
            c.end();
        }
        c.get(WORK);
        c.get(1);
        c.op(0x5a);
        c.if_void();
        exit(c, context.out, i.ip(), Exit::Budget);
        c.end();
        c.branch(again);
        c.end();
        c.end();
    }
    increment(c, RETIRED);
    if restore {
        cancel(c);
    }
    if operation.writes() {
        c.get(CHANGED);
        c.if_void();
        exit(c, context.out, next(context.mode, i), Exit::SourceChanged);
        c.end();
    }
    if failure.is_some() {
        c.branch(done);
        c.end();
        // A comparison fault restores the instruction's initial EFLAGS, even
        // after earlier elements crossed artificial budget or module yields.
        c.get(0);
        c.load64(RESTART + offset_of!(Repeat, flags));
        c.set(FLAGS);
        flags::discard(c);
        cancel(c);
        c.branch(context.out);
    }
    c.end();
}
