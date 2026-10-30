use super::*;
use crate::memory::{READ, WRITE};
use iced_x86::{Mnemonic as M, OpKind, Register};

#[derive(Clone, Copy)]
struct RegisterView {
    local: u32,
    bits: u32,
    shift: u32,
}

fn register(register: Register) -> Option<RegisterView> {
    let full = register.full_register();
    if !(Register::RAX..=Register::R15).contains(&full) {
        return None;
    }
    Some(RegisterView {
        local: GPR + full as u32 - Register::RAX as u32,
        bits: (register.size() * 8) as u32,
        shift: if (Register::AH..=Register::BH).contains(&register) {
            8
        } else {
            0
        },
    })
}

fn immediate(instruction: &Instruction, index: u32) -> Option<u64> {
    Some(match instruction.op_kind(index) {
        OpKind::Immediate8 => instruction.immediate8() as u64,
        OpKind::Immediate16 => instruction.immediate16() as u64,
        OpKind::Immediate32 => instruction.immediate32() as u64,
        OpKind::Immediate64 => instruction.immediate64(),
        OpKind::Immediate8to16 => instruction.immediate8to16() as u64,
        OpKind::Immediate8to32 => instruction.immediate8to32() as u64,
        OpKind::Immediate8to64 => instruction.immediate8to64() as u64,
        OpKind::Immediate32to64 => instruction.immediate32to64() as u64,
        _ => return None,
    })
}

pub(super) fn register_operand(instruction: &Instruction, index: u32) -> bool {
    instruction.op_kind(index) == OpKind::Register
        && register(instruction.op_register(index)).is_some()
}

pub(super) fn memory_operand(instruction: &Instruction, index: u32) -> bool {
    instruction.op_kind(index) == OpKind::Memory
        && matches!(instruction.memory_size().size(), 1 | 2 | 4 | 8)
}

pub(super) fn bits(instruction: &Instruction, index: u32) -> u32 {
    if instruction.op_kind(index) == OpKind::Memory {
        (instruction.memory_size().size() * 8) as u32
    } else {
        register(instruction.op_register(index)).unwrap().bits
    }
}

pub(super) fn memory_access(instruction: &Instruction) -> u8 {
    if matches!(instruction.mnemonic(), M::Nop | M::Pause | M::Lea) {
        return 0;
    }
    if instruction.op0_kind() == OpKind::Memory {
        if matches!(instruction.mnemonic(), M::Cmp | M::Test) {
            READ
        } else if instruction.mnemonic() == M::Mov || is_set(instruction.mnemonic()) {
            WRITE
        } else {
            READ | WRITE
        }
    } else if instruction.op_count() > 1 && instruction.op1_kind() == OpKind::Memory {
        if instruction.mnemonic() == M::Xchg {
            READ | WRITE
        } else {
            READ
        }
    } else {
        0
    }
}

pub(super) fn supported(instruction: &Instruction) -> bool {
    let mnemonic = instruction.mnemonic();
    if instruction.has_lock_prefix()
        && !(instruction.op0_kind() == OpKind::Memory
            && matches!(
                mnemonic,
                M::Add
                    | M::Adc
                    | M::Sub
                    | M::Sbb
                    | M::And
                    | M::Or
                    | M::Xor
                    | M::Inc
                    | M::Dec
                    | M::Neg
                    | M::Not
                    | M::Xchg
            )
            || mnemonic == M::Xchg && instruction.op1_kind() == OpKind::Memory)
    {
        return false;
    }
    if instruction.flow_control() == FlowControl::UnconditionalBranch {
        return mnemonic == M::Jmp
            && matches!(
                instruction.op0_kind(),
                OpKind::NearBranch16 | OpKind::NearBranch32 | OpKind::NearBranch64
            );
    }
    if instruction.flow_control() == FlowControl::ConditionalBranch {
        return instruction.condition_code() != iced_x86::ConditionCode::None;
    }
    if instruction.flow_control() != FlowControl::Next {
        return false;
    }
    if matches!(
        mnemonic,
        M::Nop
            | M::Pause
            | M::Endbr32
            | M::Endbr64
            | M::Cbw
            | M::Cwde
            | M::Cdqe
            | M::Cwd
            | M::Cdq
            | M::Cqo
            | M::Clc
            | M::Stc
            | M::Cmc
            | M::Cld
            | M::Std
            | M::Lahf
            | M::Sahf
    ) {
        return true;
    }
    if mnemonic == M::Lea {
        return register_operand(instruction, 0) && instruction.op1_kind() == OpKind::Memory;
    }
    if !register_operand(instruction, 0) && !memory_operand(instruction, 0) {
        return false;
    }
    if matches!(mnemonic, M::Inc | M::Dec | M::Neg | M::Not) {
        return true;
    }
    if is_set(mnemonic) {
        return true;
    }
    if instruction.op_count() != 2 {
        return false;
    }
    if !register_operand(instruction, 1)
        && !memory_operand(instruction, 1)
        && immediate(instruction, 1).is_none()
    {
        return false;
    }
    matches!(
        mnemonic,
        M::Mov
            | M::Movzx
            | M::Movsx
            | M::Movsxd
            | M::Xchg
            | M::Add
            | M::Adc
            | M::Sub
            | M::Sbb
            | M::Cmp
            | M::And
            | M::Or
            | M::Xor
            | M::Test
            | M::Shl
            | M::Sal
            | M::Shr
            | M::Sar
            | M::Rol
            | M::Ror
            | M::Rcl
            | M::Rcr
    ) || is_cmov(mnemonic)
}

fn is_set(mnemonic: M) -> bool {
    matches!(
        mnemonic,
        M::Seto
            | M::Setno
            | M::Setb
            | M::Setae
            | M::Sete
            | M::Setne
            | M::Setbe
            | M::Seta
            | M::Sets
            | M::Setns
            | M::Setp
            | M::Setnp
            | M::Setl
            | M::Setge
            | M::Setle
            | M::Setg
    )
}
fn is_cmov(mnemonic: M) -> bool {
    matches!(
        mnemonic,
        M::Cmovo
            | M::Cmovno
            | M::Cmovb
            | M::Cmovae
            | M::Cmove
            | M::Cmovne
            | M::Cmovbe
            | M::Cmova
            | M::Cmovs
            | M::Cmovns
            | M::Cmovp
            | M::Cmovnp
            | M::Cmovl
            | M::Cmovge
            | M::Cmovle
            | M::Cmovg
    )
}

pub(super) fn truncate(c: &mut Code, bits: u32) {
    if bits < 64 {
        c.i64((1u64 << bits) - 1);
        c.op(0x83);
    }
}
pub(super) fn sign_extend(c: &mut Code, bits: u32) {
    if bits < 64 {
        c.i64((64 - bits) as u64);
        c.op(0x86);
        c.i64((64 - bits) as u64);
        c.op(0x87);
    }
}

fn read_register(c: &mut Code, register: RegisterView) {
    c.get(register.local);
    if register.shift != 0 {
        c.i64(register.shift as u64);
        c.op(0x88);
    }
    truncate(c, register.bits);
}
pub(super) fn read(c: &mut Code, instruction: &Instruction, index: u32, context: access::Context) {
    if let Some(value) = immediate(instruction, index) {
        c.i64(value);
    } else if instruction.op_kind(index) == OpKind::Memory {
        context.load(
            c,
            instruction.ip(),
            bits(instruction, index) / 8,
            super::memory_access(instruction),
        );
    } else {
        read_register(c, register(instruction.op_register(index)).unwrap());
    }
}
fn write_register(c: &mut Code, register: RegisterView) {
    if register.bits >= 32 {
        c.get(RESULT);
        truncate(c, register.bits);
        c.set(register.local);
    } else {
        c.get(register.local);
        c.i64(!(((1u64 << register.bits) - 1) << register.shift));
        c.op(0x83);
        c.get(RESULT);
        truncate(c, register.bits);
        if register.shift != 0 {
            c.i64(register.shift as u64);
            c.op(0x86);
        }
        c.op(0x84);
        c.set(register.local);
    }
}

pub(super) fn write_gpr(c: &mut Code, index: u32, bits: u32) {
    write_register(
        c,
        RegisterView {
            local: GPR + index,
            bits,
            shift: 0,
        },
    );
}

pub(super) fn write(c: &mut Code, instruction: &Instruction, index: u32, context: access::Context) {
    if instruction.op_kind(index) == OpKind::Memory {
        context.store(c, instruction.ip(), bits(instruction, index) / 8);
    } else {
        write_register(c, register(instruction.op_register(index)).unwrap());
    }
}

pub(super) fn emit(
    c: &mut Code,
    instruction: &Instruction,
    known: &mut Option<flags::Recipe>,
    context: access::Context,
) {
    let mnemonic = instruction.mnemonic();
    if flags::instruction(c, instruction, known) {
        return;
    }
    if matches!(mnemonic, M::Nop | M::Pause | M::Endbr32 | M::Endbr64) {
        return;
    }
    if matches!(
        mnemonic,
        M::Cbw | M::Cwde | M::Cdqe | M::Cwd | M::Cdq | M::Cqo
    ) {
        let (bits, output, high) = match mnemonic {
            M::Cbw => (8, 16, false),
            M::Cwde => (16, 32, false),
            M::Cdqe => (32, 64, false),
            M::Cwd => (16, 16, true),
            M::Cdq => (32, 32, true),
            M::Cqo => (64, 64, true),
            _ => unreachable!(),
        };
        read_register(
            c,
            RegisterView {
                local: GPR,
                bits,
                shift: 0,
            },
        );
        sign_extend(c, bits);
        if high {
            c.i64(63);
            c.op(0x87);
        }
        c.set(RESULT);
        write_register(
            c,
            RegisterView {
                local: if high { GPR + 2 } else { GPR },
                bits: output,
                shift: 0,
            },
        );
        return;
    }
    let bits = bits(instruction, 0);
    if mnemonic == M::Lea {
        context.address(c, instruction, false);
        c.set(RESULT);
        write(c, instruction, 0, context);
        return;
    }
    let memory = memory_access(instruction);
    if memory != 0 {
        context.prepare(
            c,
            instruction,
            (instruction.memory_size().size()) as u32,
            memory,
        );
    }
    if is_set(mnemonic) || is_cmov(mnemonic) {
        if is_set(mnemonic) {
            flags::condition(c, instruction.condition_code() as u32 - 1, *known);
            c.op(0xad);
            c.set(RESULT);
            write(c, instruction, 0, context);
        } else {
            read(c, instruction, 1, context);
            read(c, instruction, 0, context);
            flags::condition(c, instruction.condition_code() as u32 - 1, *known);
            c.op(0x1b);
            c.set(RESULT);
            write(c, instruction, 0, context);
        }
        return;
    }
    if matches!(mnemonic, M::Mov | M::Movzx | M::Movsx | M::Movsxd) {
        read(c, instruction, 1, context);
        if matches!(mnemonic, M::Movsx | M::Movsxd) {
            sign_extend(c, self::bits(instruction, 1));
        }
        c.set(RESULT);
        write(c, instruction, 0, context);
        return;
    }
    read(c, instruction, 0, context);
    c.set(A);
    if instruction.op_count() > 1 {
        read(c, instruction, 1, context);
        truncate(c, bits);
    } else if matches!(mnemonic, M::Inc | M::Dec) {
        c.i64(1);
    } else {
        c.i64(0);
    }
    c.set(B);
    if matches!(mnemonic, M::Rol | M::Ror | M::Rcl | M::Rcr) {
        rotate::emit(c, instruction, known, context);
        return;
    }
    if matches!(mnemonic, M::Shl | M::Sal | M::Shr | M::Sar) {
        shift::emit(c, instruction, known, context);
        return;
    }
    if mnemonic == M::Xchg {
        let memory_source = instruction.op1_kind() == OpKind::Memory;
        for (index, value) in if memory_source {
            [(1, A), (0, B)]
        } else {
            [(0, B), (1, A)]
        } {
            c.get(value);
            c.set(RESULT);
            write(c, instruction, index, context);
        }
        return;
    }
    if mnemonic == M::Neg {
        c.get(A);
        c.set(B);
        c.i64(0);
        c.set(A);
    }
    let carry_in = matches!(mnemonic, M::Adc | M::Sbb);
    if carry_in {
        flags::bit(c, 1, *known);
        c.set(STATUS);
    }
    c.get(A);
    if mnemonic == M::Not {
        c.i64(u64::MAX);
        c.op(0x85);
    } else {
        c.get(B);
        c.op(match mnemonic {
            M::Add | M::Adc | M::Inc => 0x7c,
            M::Sub | M::Sbb | M::Cmp | M::Dec | M::Neg => 0x7d,
            M::And | M::Test => 0x83,
            M::Or => 0x84,
            M::Xor => 0x85,
            _ => unreachable!(),
        });
        if carry_in {
            c.get(STATUS);
            c.op(0xad);
            c.op(if mnemonic == M::Adc { 0x7c } else { 0x7d });
        }
    }
    truncate(c, bits);
    c.set(RESULT);
    if !matches!(mnemonic, M::Cmp | M::Test) {
        write(c, instruction, 0, context);
    }
    if mnemonic != M::Not {
        let recipe = flags::Recipe {
            kind: match mnemonic {
                M::Add | M::Adc | M::Inc => flags::ADD,
                M::Sub | M::Sbb | M::Cmp | M::Dec | M::Neg => flags::SUB,
                _ => flags::LOGIC,
            },
            bits,
            preserve_cf: matches!(mnemonic, M::Inc | M::Dec),
            carry_in,
            overridden: 0,
        };
        flags::defer(c, recipe, *known);
        *known = Some(recipe);
    }
}
