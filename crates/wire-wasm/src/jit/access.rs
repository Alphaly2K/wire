use super::*;
use crate::memory::{
    translation::{Translation, ACCESS_FAULT, CACHE_SLOTS, SOURCE_CHANGED},
    TranslationCache,
};
use iced_x86::Register;
use std::mem::size_of;

#[derive(Clone, Copy)]
pub(super) struct Context {
    pub mode: Mode,
    pub out: u32,
    pub flags: u32,
}

fn address_register(c: &mut Code, register: Register, sp: Option<u32>) {
    if register == Register::None {
        c.i64(0);
    } else {
        c.get(if register.full_register() == Register::RSP {
            sp.unwrap_or(GPR + 4)
        } else {
            GPR + register.full_register() as u32 - Register::RAX as u32
        });
    }
}

fn address_bits(instruction: &Instruction, mode: Mode) -> u32 {
    for register in [instruction.memory_base(), instruction.memory_index()] {
        if register != Register::None {
            return (register.size() * 8) as u32;
        }
    }
    match instruction.memory_displ_size() {
        2 => 16,
        4 if instruction.memory_displacement64() <= u32::MAX as u64 => 32,
        _ => mode as u32,
    }
}

impl Context {
    pub fn address(self, c: &mut Code, instruction: &Instruction, segment: bool) {
        self.address_with_sp(c, instruction, segment, None);
    }

    pub fn address_with_sp(
        self,
        c: &mut Code,
        instruction: &Instruction,
        segment: bool,
        sp: Option<u32>,
    ) {
        self.effective_address(c, instruction, segment, sp, None);
    }

    pub fn address_adjusted(self, c: &mut Code, instruction: &Instruction, offset: u32) {
        self.effective_address(c, instruction, true, None, Some(offset));
    }

    fn effective_address(
        self,
        c: &mut Code,
        instruction: &Instruction,
        segment: bool,
        sp: Option<u32>,
        offset: Option<u32>,
    ) {
        if instruction.is_ip_rel_memory_operand() {
            c.i64(instruction.ip_rel_memory_address());
        } else {
            address_register(c, instruction.memory_base(), sp);
            address_register(c, instruction.memory_index(), sp);
            c.i64(instruction.memory_index_scale() as u64);
            c.op(0x7e);
            c.op(0x7c);
            c.i64(instruction.memory_displacement64());
            c.op(0x7c);
        }
        if let Some(offset) = offset {
            c.get(offset);
            c.op(0x7c);
        }
        scalar::truncate(c, address_bits(instruction, self.mode));
        if segment {
            let offset = match instruction.segment_prefix() {
                Register::FS => Some(offset_of!(State, fs_base)),
                Register::GS => Some(offset_of!(State, gs_base)),
                _ => None,
            };
            if let Some(offset) = offset {
                c.get(0);
                c.load64(offset);
                c.op(0x7c);
                if self.mode == Mode::X86 {
                    scalar::truncate(c, 32);
                }
            }
        }
    }

    fn check_fault(self, c: &mut Code, pc: u64) {
        c.get(2);
        c.memory(0x28, offset_of!(TranslationCache, status));
        c.i32(ACCESS_FAULT);
        c.op(0x46);
        c.if_void();
        exit(c, self.out, pc, Exit::Fault);
        c.end();
    }

    pub fn prepare(self, c: &mut Code, instruction: &Instruction, size: u32, access: u8) {
        self.address(c, instruction, true);
        c.set(ADDRESS);
        self.prepare_address(c, instruction.ip(), size, access);
    }

    /// Validate an access without loading, storing, or revoking source bytes.
    pub fn probe(self, c: &mut Code, pc: u64, size: u32, access: u8) {
        self.prepare_address(c, pc, size, access);
        c.get(POINTER);
        c.op(0x45);
        c.if_void();
        c.get(3);
        c.get(ADDRESS);
        c.i32(size);
        c.i32(access as u32);
        c.call(5);
        c.i32(ACCESS_FAULT);
        c.op(0x46);
        c.if_void();
        exit(c, self.out, pc, Exit::Fault);
        c.end();
        c.end();
    }

    pub fn prepare_address(self, c: &mut Code, pc: u64, size: u32, access: u8) {
        self.lookup(c, pc, size, access, true);
    }

    /// A masked access must not resolve permissions for unselected bytes.
    pub fn cached_address(self, c: &mut Code, pc: u64, size: u32, access: u8) {
        self.lookup(c, pc, size, access, false);
    }

    fn lookup(self, c: &mut Code, pc: u64, size: u32, access: u8, resolve: bool) {
        c.get(2);
        c.get(ADDRESS);
        c.i32(size);
        c.i32(access as u32);
        c.call(6);
        c.set(POINTER);
        if resolve {
            c.get(POINTER);
            c.op(0x45);
            c.get(ADDRESS);
            c.i64(PAGE - 1);
            c.op(0x83);
            c.i64(PAGE - size as u64);
            c.op(0x58);
            c.op(0x71);
            c.if_void();
            c.get(3);
            c.get(ADDRESS);
            c.i32(size);
            c.i32(access as u32);
            c.call(0);
            c.set(POINTER);
            self.check_fault(c, pc);
            c.end();
        }
    }

    pub fn load(self, c: &mut Code, pc: u64, size: u32, access: u8) {
        self.load_offset(c, pc, size, access, 0);
    }

    /// The original preparation must cover the complete access including offset.
    pub fn load_offset(self, c: &mut Code, pc: u64, size: u32, access: u8, offset: u32) {
        c.get(POINTER);
        c.open(0x04, 0x7e);
        c.get(POINTER);
        c.memory(
            match size {
                1 => 0x31,
                2 => 0x33,
                4 => 0x35,
                8 => 0x29,
                _ => unreachable!(),
            },
            offset as usize,
        );
        c.otherwise();
        c.get(3);
        c.get(ADDRESS);
        if offset != 0 {
            c.i64(offset as u64);
            c.op(0x7c);
        }
        c.i32(size);
        c.i32(access as u32);
        c.call(1);
        self.check_fault(c, pc);
        c.end();
    }

    pub fn store(self, c: &mut Code, pc: u64, size: u32) {
        c.get(POINTER);
        c.if_void();
        c.get(POINTER);
        c.get(RESULT);
        c.memory(
            match size {
                1 => 0x3c,
                2 => 0x3d,
                4 => 0x3e,
                8 => 0x37,
                _ => unreachable!(),
            },
            0,
        );
        c.otherwise();
        c.get(3);
        c.get(ADDRESS);
        c.i32(size);
        c.get(RESULT);
        c.call(2);
        c.set(TRANSLATION);
        self.store_status(c, pc);
        c.end();
    }

    pub fn store128(self, c: &mut Code, pc: u64, low: u32, high: u32) {
        c.get(POINTER);
        c.if_void();
        for (value, offset) in [(low, 0), (high, 8)] {
            c.get(POINTER);
            c.get(value);
            c.store64(offset);
        }
        c.otherwise();
        c.get(3);
        c.get(ADDRESS);
        c.get(low);
        c.get(high);
        c.call(4);
        c.set(TRANSLATION);
        self.store_status(c, pc);
        c.end();
    }

    pub fn store_status(self, c: &mut Code, pc: u64) {
        c.get(TRANSLATION);
        c.i32(ACCESS_FAULT);
        c.op(0x46);
        c.if_void();
        exit(c, self.out, pc, Exit::Fault);
        c.end();
        c.get(TRANSLATION);
        c.i32(SOURCE_CHANGED);
        c.op(0x46);
        c.if_void();
        c.i32(1);
        c.set(CHANGED);
        c.end();
    }
}

/// The hit path reads translation metadata only; misses retain the caller's transaction.
pub(super) fn lookup_function() -> Code {
    let mut c = Code::default();
    c.get(2);
    c.i32(1);
    c.op(0x6b);
    c.i32(PAGE as u32 - 1);
    c.op(0x4b);
    c.if_void();
    c.i32(0);
    c.op(0x0f);
    c.end();
    c.get(1);
    c.i64(PAGE - 1);
    c.op(0x83);
    c.i64(PAGE);
    c.get(2);
    c.op(0xad);
    c.op(0x7d);
    c.op(0x58);
    c.if_i32();
    c.get(0);
    c.i32(offset_of!(TranslationCache, entries) as u32);
    c.op(0x6a);
    for shift in [12, 28, 44] {
        c.get(1);
        c.i64(shift);
        c.op(0x88);
        if shift != 12 {
            c.op(0x85);
        }
    }
    c.op(0xa7);
    c.i32(CACHE_SLOTS as u32 - 1);
    c.op(0x71);
    c.i32(size_of::<Translation>() as u32);
    c.op(0x6c);
    c.op(0x6a);
    c.set(4);
    c.get(4);
    c.load64(offset_of!(Translation, tag));
    c.get(1);
    c.i64(!(PAGE - 1));
    c.op(0x83);
    c.op(0x51);
    c.get(4);
    c.load64(offset_of!(Translation, epoch));
    c.get(0);
    c.load64(offset_of!(TranslationCache, epoch));
    c.op(0x51);
    c.op(0x71);
    c.get(0);
    c.load64(offset_of!(TranslationCache, epoch));
    c.i64(u64::MAX);
    c.op(0x52);
    c.op(0x71);
    c.get(4);
    c.memory(0x28, offset_of!(Translation, permissions));
    c.get(3);
    c.op(0x71);
    c.get(3);
    c.op(0x46);
    c.op(0x71);
    c.if_i32();
    c.get(4);
    c.memory(0x28, offset_of!(Translation, pointer));
    c.get(1);
    c.op(0xa7);
    c.i32(PAGE as u32 - 1);
    c.op(0x71);
    c.op(0x6a);

    c.otherwise();
    c.i32(0);
    c.end();
    c.otherwise();
    c.i32(0);
    c.end();
    c
}
