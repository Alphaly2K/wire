use super::*;
use crate::memory::{READ, WRITE};
use iced_x86::{Code as X86Code, Mnemonic as M, OpKind};

const SP: u32 = GPR + 4;

fn near(instruction: &Instruction) -> bool {
    matches!(
        instruction.op0_kind(),
        OpKind::NearBranch16 | OpKind::NearBranch32 | OpKind::NearBranch64
    )
}

fn value(instruction: &Instruction) -> bool {
    scalar::register_operand(instruction, 0)
        || scalar::memory_operand(instruction, 0)
        || matches!(
            instruction.op0_kind(),
            OpKind::Immediate16
                | OpKind::Immediate32
                | OpKind::Immediate8to16
                | OpKind::Immediate8to32
                | OpKind::Immediate8to64
                | OpKind::Immediate32to64
        )
}

fn count_bits(i: &Instruction) -> Option<u32> {
    use X86Code::*;
    match i.code() {
        Loopne_rel8_16_CX | Loopne_rel8_32_CX | Loope_rel8_16_CX | Loope_rel8_32_CX
        | Loop_rel8_16_CX | Loop_rel8_32_CX | Jcxz_rel8_16 | Jcxz_rel8_32 => Some(16),
        Loopne_rel8_16_ECX | Loopne_rel8_32_ECX | Loopne_rel8_64_ECX | Loope_rel8_16_ECX
        | Loope_rel8_32_ECX | Loope_rel8_64_ECX | Loop_rel8_16_ECX | Loop_rel8_32_ECX
        | Loop_rel8_64_ECX | Jecxz_rel8_16 | Jecxz_rel8_32 | Jecxz_rel8_64 => Some(32),
        Loopne_rel8_16_RCX | Loopne_rel8_64_RCX | Loope_rel8_16_RCX | Loope_rel8_64_RCX
        | Loop_rel8_16_RCX | Loop_rel8_64_RCX | Jrcxz_rel8_16 | Jrcxz_rel8_64 => Some(64),
        _ => None,
    }
}

fn decrements(i: &Instruction) -> bool {
    matches!(i.mnemonic(), M::Loop | M::Loope | M::Loopne)
}

pub(super) fn condition(
    c: &mut Code,
    i: &Instruction,
    recipe: Option<flags::Recipe>,
    prospective: bool,
) {
    if let Some(bits) = count_bits(i) {
        c.get(GPR + 1);
        if prospective && decrements(i) {
            c.i64(1);
            c.op(0x7d);
        }
        scalar::truncate(c, bits);
        c.op(0x50);
        if decrements(i) {
            c.op(0x45);
            if matches!(i.mnemonic(), M::Loope | M::Loopne) {
                flags::bit(c, 64, recipe);
                if i.mnemonic() == M::Loopne {
                    c.op(0x45);
                }
                c.op(0x71);
            }
        }
    } else {
        flags::condition(c, i.condition_code() as u32 - 1, recipe);
    }
}

pub(super) fn supported(instruction: &Instruction) -> bool {
    if instruction.has_lock_prefix() {
        return false;
    }
    match instruction.mnemonic() {
        M::Loop | M::Loope | M::Loopne | M::Jcxz | M::Jecxz | M::Jrcxz => {
            count_bits(instruction).is_some()
        }
        M::Push => value(instruction),
        M::Pop => {
            scalar::register_operand(instruction, 0) || scalar::memory_operand(instruction, 0)
        }
        M::Call => {
            near(instruction)
                || scalar::register_operand(instruction, 0)
                || scalar::memory_operand(instruction, 0)
        }
        M::Jmp => {
            !near(instruction)
                && (scalar::register_operand(instruction, 0)
                    || scalar::memory_operand(instruction, 0))
        }
        M::Ret | M::Leave => true,
        _ => false,
    }
}

pub(super) fn changes_ip(instruction: &Instruction) -> bool {
    supported(instruction)
        && matches!(
            instruction.flow_control(),
            FlowControl::Call
                | FlowControl::IndirectCall
                | FlowControl::IndirectBranch
                | FlowControl::Return
        )
}

pub(super) fn memory_access(instruction: &Instruction) -> u8 {
    match instruction.mnemonic() {
        M::Push | M::Call => {
            WRITE
                | if instruction.op0_kind() == OpKind::Memory {
                    READ
                } else {
                    0
                }
        }
        M::Pop if instruction.op0_kind() == OpKind::Memory => READ | WRITE,
        M::Pop | M::Ret | M::Leave => READ,
        M::Jmp if instruction.op0_kind() == OpKind::Memory => READ,
        _ => 0,
    }
}

fn canonical(address: u64) -> bool {
    ((address << 16) as i64 >> 16) as u64 == address
}

pub(super) fn needs_context(mode: Mode, instruction: &Instruction) -> bool {
    if count_bits(instruction).is_some() {
        return mode == Mode::X64 && !canonical(instruction.near_branch_target());
    }
    supported(instruction)
        || mode == Mode::X64
            && matches!(
                instruction.flow_control(),
                FlowControl::UnconditionalBranch | FlowControl::ConditionalBranch
            )
            && !canonical(instruction.near_branch_target())
}

fn target_fault(c: &mut Code, instruction: &Instruction, context: access::Context) {
    c.get(3);
    c.get(IP);
    c.call(3);
    exit(c, context.out, instruction.ip(), Exit::Fault);
}

fn check_target(c: &mut Code, instruction: &Instruction, context: access::Context) {
    if context.mode == Mode::X64 {
        c.get(IP);
        c.i64(16);
        c.op(0x86);
        c.i64(16);
        c.op(0x87);
        c.get(IP);
        c.op(0x52);
        c.if_void();
        target_fault(c, instruction, context);
        c.end();
    }
}

pub(super) fn check_direct(
    c: &mut Code,
    instruction: &Instruction,
    context: access::Context,
    recipe: Option<flags::Recipe>,
) {
    if context.mode != Mode::X64
        || !matches!(
            instruction.flow_control(),
            FlowControl::UnconditionalBranch | FlowControl::ConditionalBranch
        )
        || canonical(instruction.near_branch_target())
    {
        return;
    }
    let conditional = instruction.flow_control() == FlowControl::ConditionalBranch;
    if conditional {
        condition(c, instruction, recipe, true);
        c.if_void();
    }
    c.i64(instruction.near_branch_target());
    c.set(IP);
    target_fault(c, instruction, context);
    if conditional {
        c.end();
    }
}

fn read_value(c: &mut Code, instruction: &Instruction, context: access::Context) {
    if instruction.op0_kind() == OpKind::Memory {
        let size = scalar::bits(instruction, 0) / 8;
        context.prepare(c, instruction, size, READ);
        context.load(c, instruction.ip(), size, READ);
    } else {
        scalar::read(c, instruction, 0, context);
    }
}

fn wrap_sp(c: &mut Code, mode: Mode) {
    if mode == Mode::X86 {
        scalar::truncate(c, 32);
    }
}

pub(super) fn emit(c: &mut Code, instruction: &Instruction, context: access::Context) {
    match instruction.mnemonic() {
        M::Loop | M::Loope | M::Loopne => {
            let bits = count_bits(instruction).unwrap();
            c.get(GPR + 1);
            c.i64(1);
            c.op(0x7d);
            scalar::truncate(c, bits);
            c.set(RESULT);
            scalar::write_gpr(c, 1, bits);
        }
        M::Jcxz | M::Jecxz | M::Jrcxz => {}
        M::Push | M::Call => {
            let call = instruction.mnemonic() == M::Call;
            if call && near(instruction) {
                c.i64(context.mode.address(instruction.near_branch_target()));
            } else {
                read_value(c, instruction, context);
            }
            if call {
                c.set(IP);
                check_target(c, instruction, context);
                c.i64(next(context.mode, instruction));
            }
            c.set(RESULT);
            let size = instruction.stack_pointer_increment().unsigned_abs();
            c.get(SP);
            c.i64(size as u64);
            c.op(0x7d);
            wrap_sp(c, context.mode);
            c.set(ADDRESS);
            context.prepare_address(c, instruction.ip(), size, WRITE);
            context.store(c, instruction.ip(), size);
            c.get(ADDRESS);
            c.set(SP);
        }
        M::Pop | M::Ret => {
            let ret = instruction.mnemonic() == M::Ret;
            let extra = if ret && instruction.op_count() != 0 {
                instruction.immediate16() as u32
            } else {
                0
            };
            let size = instruction.stack_pointer_increment() as u32 - extra;
            c.get(SP);
            wrap_sp(c, context.mode);
            c.set(ADDRESS);
            context.prepare_address(c, instruction.ip(), size, READ);
            context.load(c, instruction.ip(), size, READ);
            if ret {
                c.set(IP);
                check_target(c, instruction, context);
                c.get(SP);
                c.i64((size + extra) as u64);
                c.op(0x7c);
                wrap_sp(c, context.mode);
                c.set(SP);
            } else {
                c.set(A);
                c.get(SP);
                c.i64(size as u64);
                c.op(0x7c);
                wrap_sp(c, context.mode);
                c.set(B);
                if instruction.op0_kind() == OpKind::Memory {
                    // POP computes an ESP/RSP-based destination after the
                    // increment, but commits SP only after the store succeeds.
                    context.address_with_sp(c, instruction, true, Some(B));
                    c.set(ADDRESS);
                    context.prepare_address(c, instruction.ip(), size, WRITE);
                    c.get(A);
                    c.set(RESULT);
                    context.store(c, instruction.ip(), size);
                    c.get(B);
                    c.set(SP);
                } else {
                    c.get(B);
                    c.set(SP);
                    c.get(A);
                    c.set(RESULT);
                    scalar::write(c, instruction, 0, context);
                }
            }
        }
        M::Leave => {
            let size = match instruction.code() {
                X86Code::Leavew => 2,
                X86Code::Leaved => 4,
                X86Code::Leaveq => 8,
                _ => unreachable!(),
            };
            c.get(GPR + 5);
            wrap_sp(c, context.mode);
            c.set(ADDRESS);
            context.prepare_address(c, instruction.ip(), size, READ);
            context.load(c, instruction.ip(), size, READ);
            c.set(RESULT);
            c.get(ADDRESS);
            c.i64(size as u64);
            c.op(0x7c);
            wrap_sp(c, context.mode);
            c.set(B);
            scalar::write_gpr(c, 5, size * 8);
            c.get(B);
            c.set(SP);
        }
        M::Jmp => {
            read_value(c, instruction, context);
            c.set(IP);
            check_target(c, instruction, context);
        }
        _ => unreachable!(),
    }
}
