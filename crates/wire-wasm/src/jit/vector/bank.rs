//! A VM owns one bank of Wasm globals. Only used XMM/x87 registers enter chunk
//! locals; CPU backing changes once at the outer exit, including fault exits.
use super::*;

const LOADED: u32 = 16;
const DIRTY: u32 = 17;
const MXCSR: u32 = 18;
const CONTROL: u32 = 1 << 16;
const FPU: u32 = 1 << 25;
const FSW: u32 = 27;
const FCW: u32 = 28;
const FTAG: u32 = 29;
const FIP: u32 = 30;
const FDP: u32 = 31;
const FOP: u32 = 32;
const ENV: u32 = 1 << 26;
pub(in crate::jit) const GLOBALS: u8 = 33;
const LOCAL: u32 = RESUME + 1;

#[derive(Clone, Copy, Default)]
pub(in crate::jit) struct Bank {
    pub used: u32,
    written: u32,
    pub floating: bool,
    pub fpu: bool,
    environment: bool,
    temporary: bool,
}

impl Bank {
    pub fn collect(blocks: &[Block], mode: Mode) -> Self {
        let mut bank = Self::default();
        for i in blocks
            .iter()
            .flat_map(|b| &b.instructions)
            .filter(|i| supported(i))
        {
            bank.floating |= floating::supported(i);
            bank.fpu |= mmx::supported(i) || floating::uses_fpu(i) || i.mnemonic() == M::Maskmovq;
            if x87::supported(i) {
                bank.fpu = true;
                bank.environment |= x87::environment(i);
                if x87::uses_values(i) {
                    bank.used |= 0xff0000;
                    if x87::writes_values(i) {
                        bank.written |= 0xff0000;
                    }
                }
                if x87::fx(i) {
                    let registers = 0xff0000 | if mode == Mode::X64 { 0xffff } else { 0xff };
                    bank.used |= registers;
                    bank.floating = true;
                    if x87::restore(i) {
                        bank.written |= registers;
                    }
                }
            }
            bank.temporary |=
                floating::supported(i) && !matches!(i.mnemonic(), M::Ldmxcsr | M::Stmxcsr);
            if packed::mask_width(i.mnemonic()).is_some() {
                bank.used |= 1;
            }
            if string_compare::masked(i) {
                bank.used |= 1;
                bank.written |= 1;
            }
            for index in 0..i.op_count() {
                if xmm(i, index) || mmx::register_operand(i, index) {
                    let bit = 1 << register(i, index);
                    bank.used |= bit;
                    if index == 0
                        && !floating::compares_flags(i)
                        && i.mnemonic() != M::Ptest
                        && !string_compare::supported(i)
                    {
                        bank.written |= bit;
                    }
                }
            }
        }
        bank
    }

    pub fn local(self, register: u32) -> u32 {
        assert!(self.used & (1 << register) != 0);
        LOCAL + (self.used & ((1 << register) - 1)).count_ones()
    }
    pub fn scratch(self) -> u32 {
        LOCAL + self.used.count_ones()
    }
    pub fn active(self) -> bool {
        self.used != 0 || self.floating || self.fpu
    }
    pub fn vector_locals(self) -> u32 {
        if self.used == 0 && !self.temporary {
            0
        } else {
            self.used.count_ones() + 1
        }
    }
    pub fn mxcsr(self) -> u32 {
        LOCAL + self.vector_locals() + self.wide_locals()
    }
    pub fn fip(self) -> u32 {
        LOCAL + self.vector_locals()
    }
    pub fn fdp(self) -> u32 {
        self.fip() + 1
    }
    pub fn fop(self) -> u32 {
        self.ftag() + 1
    }
    pub fn wide_locals(self) -> u32 {
        if self.environment {
            2
        } else {
            0
        }
    }
    pub fn fsw(self) -> u32 {
        self.mxcsr() + u32::from(self.floating)
    }
    pub fn fcw(self) -> u32 {
        self.fsw() + 1
    }
    pub fn ftag(self) -> u32 {
        self.fsw() + 2
    }
    pub fn control_locals(self) -> u32 {
        u32::from(self.floating) + if self.fpu { 3 } else { 0 } + u32::from(self.environment)
    }

    pub fn enter(self, c: &mut Code) {
        for index in 0..24 {
            if self.used & (1 << index) == 0 {
                continue;
            }
            c.global_get(LOADED);
            c.i32(register_bit(index));
            c.op(0x71);
            c.op(0x45);
            c.if_void();
            c.get(0);
            c.vector_memory(0, register_offset(index));
            c.global_set(register_global(index));
            c.end();
            c.global_get(register_global(index));
            c.set(self.local(index));
        }
        if self.floating {
            c.global_get(LOADED);
            c.i32(CONTROL);
            c.op(0x71);
            c.op(0x45);
            c.if_void();
            c.get(0);
            c.memory(0x28, offset_of!(State, mxcsr));
            c.global_set(MXCSR);
            c.end();
            c.global_get(MXCSR);
            c.set(self.mxcsr());
            c.global_get(LOADED);
            c.i32(CONTROL);
            c.op(0x72);
            c.global_set(LOADED);
        }
        if self.used != 0 {
            c.global_get(LOADED);
            c.i32(register_mask(self.used));
            c.op(0x72);
            c.global_set(LOADED);
        }
        if self.fpu {
            c.global_get(LOADED);
            c.i32(FPU);
            c.op(0x71);
            c.op(0x45);
            c.if_void();
            for (global, offset) in fpu_fields() {
                c.get(0);
                c.memory(0x2f, offset);
                c.global_set(global);
            }
            c.end();
            for (global, local) in [(FSW, self.fsw()), (FCW, self.fcw()), (FTAG, self.ftag())] {
                c.global_get(global);
                c.set(local);
            }
            c.global_get(LOADED);
            c.i32(FPU);
            c.op(0x72);
            c.global_set(LOADED);
        }
        if self.environment {
            c.global_get(LOADED);
            c.i32(ENV);
            c.op(0x71);
            c.op(0x45);
            c.if_void();
            for (global, offset, wide) in environment_fields() {
                c.get(0);
                c.memory(if wide { 0x29 } else { 0x2f }, offset);
                c.global_set(global);
            }
            c.end();
            for (global, local) in [(FIP, self.fip()), (FDP, self.fdp()), (FOP, self.fop())] {
                c.global_get(global);
                c.set(local);
            }
            c.global_get(LOADED);
            c.i32(ENV);
            c.op(0x72);
            c.global_set(LOADED);
        }
    }

    pub fn leave(self, c: &mut Code) {
        for index in 0..24 {
            if self.written & (1 << index) != 0 {
                c.get(self.local(index));
                c.global_set(register_global(index));
            }
        }
        if self.floating {
            c.get(self.mxcsr());
            c.global_set(MXCSR);
            c.global_get(DIRTY);
            c.i32(CONTROL);
            c.op(0x72);
            c.global_set(DIRTY);
        }
        if self.written != 0 {
            c.global_get(DIRTY);
            c.i32(register_mask(self.written));
            c.op(0x72);
            c.global_set(DIRTY);
        }
        if self.fpu {
            for (global, local) in [(FSW, self.fsw()), (FCW, self.fcw()), (FTAG, self.ftag())] {
                c.get(local);
                c.global_set(global);
            }
            c.global_get(DIRTY);
            c.i32(FPU);
            c.op(0x72);
            c.global_set(DIRTY);
        }
        if self.environment {
            for (global, local) in [(FIP, self.fip()), (FDP, self.fdp()), (FOP, self.fop())] {
                c.get(local);
                c.global_set(global);
            }
            c.global_get(DIRTY);
            c.i32(ENV);
            c.op(0x72);
            c.global_set(DIRTY);
        }
    }
}

pub(in crate::jit) fn reset(c: &mut Code) {
    for index in [LOADED, DIRTY] {
        c.i32(0);
        c.global_set(index);
    }
}

pub(in crate::jit) fn commit(c: &mut Code) {
    c.global_get(DIRTY);
    c.if_void();
    for index in 0..24 {
        c.global_get(DIRTY);
        c.i32(register_bit(index));
        c.op(0x71);
        c.if_void();
        c.get(0);
        c.global_get(register_global(index));
        c.vector_memory(11, register_offset(index));
        c.end();
    }
    c.global_get(DIRTY);
    c.i32(CONTROL);
    c.op(0x71);
    c.if_void();
    c.get(0);
    c.global_get(MXCSR);
    c.memory(0x36, offset_of!(State, mxcsr));
    c.end();
    c.global_get(DIRTY);
    c.i32(FPU);
    c.op(0x71);
    c.if_void();
    for (global, offset) in fpu_fields() {
        c.get(0);
        c.global_get(global);
        c.memory(0x3b, offset);
    }
    c.end();
    c.global_get(DIRTY);
    c.i32(ENV);
    c.op(0x71);
    c.if_void();
    for (global, offset, wide) in environment_fields() {
        c.get(0);
        c.global_get(global);
        c.memory(if wide { 0x37 } else { 0x3b }, offset);
    }
    c.end();
    c.end();
}

fn register_global(index: u32) -> u32 {
    if index < 16 {
        index
    } else {
        index + 3
    }
}
fn register_bit(index: u32) -> u32 {
    1 << if index < 16 { index } else { index + 1 }
}
fn register_mask(mask: u32) -> u32 {
    (mask & 0xffff) | ((mask & 0xff0000) << 1)
}
fn register_offset(index: u32) -> usize {
    if index < 16 {
        offset_of!(State, xmm) + index as usize * 16
    } else {
        offset_of!(State, fpu.regs) + (index - 16) as usize * 16
    }
}
fn fpu_fields() -> [(u32, usize); 3] {
    [
        (FSW, offset_of!(State, fpu.sw)),
        (FCW, offset_of!(State, fpu.cw)),
        (FTAG, offset_of!(State, fpu.tags)),
    ]
}
fn environment_fields() -> [(u32, usize, bool); 3] {
    [
        (FIP, offset_of!(State, fpu.ip), true),
        (FDP, offset_of!(State, fpu.dp), true),
        (FOP, offset_of!(State, fpu.opcode), false),
    ]
}
fn global_type(index: usize) -> u8 {
    if index < 16 || (19..27).contains(&index) {
        0x7b
    } else if (FIP as usize..=FDP as usize).contains(&index) {
        0x7e
    } else {
        0x7f
    }
}

fn names() -> impl Iterator<Item = String> {
    (0..16)
        .map(|index| format!("xmm{index}"))
        .chain(["xmm_loaded".into(), "xmm_dirty".into(), "mxcsr".into()])
        .chain((0..8).map(|index| format!("fp{index}")))
        .chain(["fsw".into(), "fcw".into(), "ftag".into()])
        .chain(["fip".into(), "fdp".into(), "fop".into()])
}

pub(in crate::jit) fn imports(out: &mut Vec<u8>) {
    for (index, name) in names().enumerate() {
        binary::name(out, "jit");
        binary::name(out, &name);
        out.extend_from_slice(&[3, global_type(index), 1]);
    }
}

pub(in crate::jit) fn exports(out: &mut Vec<u8>) {
    for (index, name) in names().enumerate() {
        binary::name(out, &name);
        out.extend_from_slice(&[3, index as u8]);
    }
}

pub(in crate::jit) fn declare(module: &mut Vec<u8>) {
    let mut globals = vec![GLOBALS];
    for index in 0..GLOBALS as usize {
        if global_type(index) == 0x7b {
            globals.extend_from_slice(&[0x7b, 1, 0xfd, 12]);
            globals.extend_from_slice(&[0; 16]);
            globals.push(11);
        } else {
            globals.extend_from_slice(&[
                global_type(index),
                1,
                if global_type(index) == 0x7e {
                    0x42
                } else {
                    0x41
                },
                0,
                11,
            ]);
        }
    }
    binary::section(module, 6, &globals);
}
