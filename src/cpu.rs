//! x86 interpreter core.
//!
//! Correctness-first decode-dispatch loop covering a compiler-generated-code
//! integer subset (plus string ops, cpuid, rdtsc, SSE and software x87).
//! Unsupported opcodes are fatal errors carrying the opcode
//! bytes; `op_stats` records what actually executed so M1 coverage is driven
//! by data, not guesses. Differential testing against a reference emulator
//! (Unicorn, dev-only) is M1 scope.

use crate::mem::{GuestMem, MemFault};
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::time::Instant;

mod fpu;
mod ir_exec;
mod mmx;
mod process_facade;
pub use crate::x86::process::{GuestCallOutcome, RunOutcome};
pub(crate) mod simd_ir;
pub use fpu::Fpu;
use ir_exec::PendingFlags;

pub const CF: u32 = 0x001;
pub const PF: u32 = 0x004;
pub const AF: u32 = 0x010;
pub const ZF: u32 = 0x040;
pub const SF: u32 = 0x080;
pub const DF: u32 = 0x400;
pub const OF: u32 = 0x800;

pub const EAX: usize = 0;
pub const ECX: usize = 1;
pub const EDX: usize = 2;
pub const EBX: usize = 3;
pub const ESP: usize = 4;
pub const EBP: usize = 5;
pub const ESI: usize = 6;
pub const EDI: usize = 7;

#[derive(Debug)]
pub enum CpuError {
    Mem(MemFault),
    Unsupported { eip: u32, desc: String },
    Breakpoint { eip: u32 },
    DivideError { eip: u32 },
    FloatingPoint { eip: u32, status: u16 },
    SimdFloatingPoint { eip: u32, mxcsr: u32 },
}

impl fmt::Display for CpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CpuError::Mem(e) => write!(f, "{e}"),
            CpuError::Unsupported { eip, desc } => {
                write!(f, "unsupported instruction at {eip:#010x}: {desc}")
            }
            CpuError::Breakpoint { eip } => write!(f, "int3 breakpoint at {eip:#010x}"),
            CpuError::DivideError { eip } => write!(f, "divide error at {eip:#010x}"),
            CpuError::FloatingPoint { eip, status } => write!(
                f,
                "unmasked x87 exception at {eip:#010x}: status={status:#06x}"
            ),
            CpuError::SimdFloatingPoint { eip, mxcsr } => write!(
                f,
                "unmasked SSE exception at {eip:#010x}: MXCSR={mxcsr:#010x}"
            ),
        }
    }
}

impl From<MemFault> for CpuError {
    fn from(e: MemFault) -> CpuError {
        CpuError::Mem(e)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Alu {
    Add,
    Or,
    Adc,
    Sbb,
    And,
    Sub,
    Xor,
    Cmp,
}

fn alu_from_index(i: usize) -> Alu {
    match i & 7 {
        0 => Alu::Add,
        1 => Alu::Or,
        2 => Alu::Adc,
        3 => Alu::Sbb,
        4 => Alu::And,
        5 => Alu::Sub,
        6 => Alu::Xor,
        _ => Alu::Cmp,
    }
}

#[inline]
fn mask(bits: u8) -> u32 {
    match bits {
        8 => 0xff,
        16 => 0xffff,
        _ => 0xffff_ffff,
    }
}

#[inline]
fn sbit(bits: u8) -> u32 {
    1u32 << (bits - 1)
}

struct Rm {
    reg: usize,
    addr: Option<u32>, // None => register operand in `idx`
    idx: usize,
}

/// Per-thread execution context: the subset of Cpu state that belongs to one
/// guest thread. Everything else on Cpu (instruction counters, opcode stats,
/// tracing) is machine-global and survives context switches.
#[derive(Debug, Clone, Copy, Default)]
pub struct ThreadCtx {
    pub regs: [u32; 8],
    pub eip: u32,
    pub eflags: u32,
    pub fs_base: u32,
    pub gs_base: u32,
    pub fpu: Fpu,
    pub xmm: [[u8; 16]; 8],
    pub mxcsr: u32,
}

pub struct Cpu {
    pub regs: [u32; 8], // EAX ECX EDX EBX ESP EBP ESI EDI
    pub eip: u32,
    pub eflags: u32,
    pub fs_base: u32,
    pub gs_base: u32,
    pub fpu: Fpu,
    pub xmm: [[u8; 16]; 8],
    pub mxcsr: u32,
    pub halted: bool,
    pub exit_code: Option<u32>,
    pub instr_count: u64,
    pub op_stats: HashMap<u32, u64>,
    pub raw_op_stats: HashMap<u32, u64>,
    pub compact_op_stats: [u64; 256],
    pub ir_scalar_runs: u64,
    pub ir_scalar_steps: u64,
    ir_cold_stats: HashMap<(crate::ir::Op, u8, u8, u32, u32), u64>,
    pub trace_enabled: bool,
    pub profile_opcodes: bool,
    pub ir_steps: u64,
    pub ir_fallbacks: u64,
    pub ir_native_calls: u64,
    ir_cache: Option<crate::ir::Cache>,
    ir_modules: Vec<crate::ir::Cache>,
    ir_hint: (u32, u32),
    pending_flags: Option<PendingFlags>,
    decode_address16: bool, // transient raw-decoder state, never a thread register
    pub trace_ring: VecDeque<(u32, Vec<u8>)>,
    t0: Instant,
}

impl Cpu {
    pub fn new(entry: u32, esp: u32, teb: u32) -> Cpu {
        let mut c = Cpu {
            regs: [0; 8],
            eip: entry,
            eflags: 0x202,
            fs_base: teb,
            gs_base: 0,
            fpu: Fpu::default(),
            xmm: [[0; 16]; 8],
            mxcsr: 0x1f80,
            halted: false,
            exit_code: None,
            instr_count: 0,
            op_stats: HashMap::new(),
            raw_op_stats: HashMap::new(),
            compact_op_stats: [0; 256],
            ir_scalar_runs: 0,
            ir_scalar_steps: 0,
            ir_cold_stats: HashMap::new(),
            trace_enabled: false,
            profile_opcodes: true,
            ir_steps: 0,
            ir_fallbacks: 0,
            ir_native_calls: 0,
            ir_cache: None,
            ir_modules: Vec::new(),
            ir_hint: (0, 0),
            pending_flags: None,
            decode_address16: false,
            trace_ring: VecDeque::new(),
            t0: Instant::now(),
        };
        c.regs[ESP] = esp;
        c
    }

    #[inline]
    pub fn esp(&self) -> u32 {
        self.regs[ESP]
    }
    #[inline]
    pub fn set_esp(&mut self, v: u32) {
        self.regs[ESP] = v;
    }
    #[inline]
    pub fn flag(&self, f: u32) -> bool {
        self.flags_for(f) & f != 0
    }
    #[inline]
    pub fn set_flag(&mut self, f: u32, v: bool) {
        self.flush_flags();
        if v {
            self.eflags |= f;
        } else {
            self.eflags &= !f;
        }
    }

    pub fn save_ctx(&self) -> ThreadCtx {
        ThreadCtx {
            regs: self.regs,
            eip: self.eip,
            eflags: self.effective_eflags(),
            fs_base: self.fs_base,
            gs_base: self.gs_base,
            fpu: self.fpu,
            xmm: self.xmm,
            mxcsr: self.mxcsr,
        }
    }

    pub fn load_ctx(&mut self, c: &ThreadCtx) {
        self.pending_flags = None;
        self.ir_hint = (0, 0);
        self.regs = c.regs;
        self.eip = c.eip;
        self.eflags = c.eflags;
        self.fs_base = c.fs_base;
        self.gs_base = c.gs_base;
        self.fpu = c.fpu;
        self.xmm = c.xmm;
        self.mxcsr = c.mxcsr;
    }

    // ---- flag arithmetic ----

    fn set_szp(&mut self, res: u32, bits: u8) {
        let r = res & mask(bits);
        self.set_flag(SF, r & sbit(bits) != 0);
        self.set_flag(ZF, r == 0);
        self.set_flag(PF, (r as u8).count_ones() & 1 == 0);
    }

    fn arith_add(&mut self, a: u32, b: u32, carry: u32, bits: u8) -> u32 {
        let m = mask(bits);
        let (a, b) = (a & m, b & m);
        let total = a as u64 + b as u64 + carry as u64;
        let r = (total as u32) & m;
        self.set_flag(CF, total > m as u64);
        self.set_flag(OF, ((a ^ r) & (b ^ r) & sbit(bits)) != 0);
        self.set_flag(AF, ((a ^ b ^ r) & 0x10) != 0);
        self.set_szp(r, bits);
        r
    }

    fn arith_sub(&mut self, a: u32, b: u32, borrow: u32, bits: u8) -> u32 {
        let m = mask(bits);
        let (a, b) = (a & m, b & m);
        let r = a.wrapping_sub(b).wrapping_sub(borrow) & m;
        self.set_flag(CF, (a as u64) < (b as u64 + borrow as u64));
        self.set_flag(OF, ((a ^ b) & (a ^ r) & sbit(bits)) != 0);
        self.set_flag(AF, ((a ^ b ^ r) & 0x10) != 0);
        self.set_szp(r, bits);
        r
    }

    fn arith_logic(&mut self, r: u32, bits: u8) -> u32 {
        let r = r & mask(bits);
        self.set_flag(CF, false);
        self.set_flag(OF, false);
        self.set_flag(AF, false); // undefined on hardware; deterministic here
        self.set_szp(r, bits);
        r
    }

    fn alu(&mut self, op: Alu, a: u32, b: u32, bits: u8) -> u32 {
        match op {
            Alu::Add => self.arith_add(a, b, 0, bits),
            Alu::Adc => self.arith_add(a, b, self.flag(CF) as u32, bits),
            Alu::Sub | Alu::Cmp => self.arith_sub(a, b, 0, bits),
            Alu::Sbb => self.arith_sub(a, b, self.flag(CF) as u32, bits),
            Alu::Or => self.arith_logic(a | b, bits),
            Alu::And => self.arith_logic(a & b, bits),
            Alu::Xor => self.arith_logic(a ^ b, bits),
        }
    }

    // ---- register access ----

    fn r8(&self, i: usize) -> u8 {
        if i < 4 {
            self.regs[i] as u8
        } else {
            (self.regs[i - 4] >> 8) as u8
        }
    }
    fn set_r8(&mut self, i: usize, v: u8) {
        if i < 4 {
            self.regs[i] = (self.regs[i] & !0xff) | v as u32;
        } else {
            let r = i - 4;
            self.regs[r] = (self.regs[r] & !0xff00) | ((v as u32) << 8);
        }
    }
    fn get_r(&self, i: usize, bits: u8) -> u32 {
        match bits {
            8 => self.r8(i) as u32,
            16 => self.regs[i] & 0xffff,
            _ => self.regs[i],
        }
    }
    fn set_r(&mut self, i: usize, v: u32, bits: u8) {
        match bits {
            8 => self.set_r8(i, v as u8),
            16 => self.regs[i] = (self.regs[i] & !0xffff) | (v & 0xffff),
            _ => self.regs[i] = v,
        }
    }

    // ---- fetch ----

    fn fetch8(&mut self, mem: &GuestMem) -> Result<u8, CpuError> {
        let v = mem.fetch_u8(self.eip)?;
        self.eip = self.eip.wrapping_add(1);
        Ok(v)
    }
    fn fetch16(&mut self, mem: &GuestMem) -> Result<u16, CpuError> {
        let v = mem.fetch_u16(self.eip)?;
        self.eip = self.eip.wrapping_add(2);
        Ok(v)
    }
    fn fetch32(&mut self, mem: &GuestMem) -> Result<u32, CpuError> {
        let v = mem.fetch_u32(self.eip)?;
        self.eip = self.eip.wrapping_add(4);
        Ok(v)
    }
    fn fetch_sized(&mut self, mem: &GuestMem, bits: u8) -> Result<u32, CpuError> {
        match bits {
            8 => Ok(self.fetch8(mem)? as u32),
            16 => Ok(self.fetch16(mem)? as u32),
            _ => self.fetch32(mem),
        }
    }

    // ---- stack ----

    pub(crate) fn push32(&mut self, mem: &GuestMem, v: u32) -> Result<(), CpuError> {
        let esp = self.esp().wrapping_sub(4);
        self.set_esp(esp);
        mem.write_u32(esp, v)?;
        Ok(())
    }
    fn pop32(&mut self, mem: &GuestMem) -> Result<u32, CpuError> {
        let esp = self.esp();
        let v = mem.read_u32(esp)?;
        self.set_esp(esp.wrapping_add(4));
        Ok(v)
    }
    fn push16(&mut self, mem: &GuestMem, v: u16) -> Result<(), CpuError> {
        let esp = self.esp().wrapping_sub(2);
        self.set_esp(esp);
        mem.write_u16(esp, v)?;
        Ok(())
    }
    fn pop16(&mut self, mem: &GuestMem) -> Result<u16, CpuError> {
        let esp = self.esp();
        let v = mem.read_u16(esp)?;
        self.set_esp(esp.wrapping_add(2));
        Ok(v)
    }
    fn push_sized(&mut self, mem: &GuestMem, v: u32, bits: u8) -> Result<(), CpuError> {
        if bits == 16 {
            self.push16(mem, v as u16)
        } else {
            self.push32(mem, v)
        }
    }
    fn pop_sized(&mut self, mem: &GuestMem, bits: u8) -> Result<u32, CpuError> {
        if bits == 16 {
            Ok(self.pop16(mem)? as u32)
        } else {
            self.pop32(mem)
        }
    }

    // ---- modrm ----

    /// Decodes modrm + SIB + displacement. The returned address is the raw
    /// effective address (segment base NOT applied; callers add it, except
    /// LEA which must not).
    fn modrm(&mut self, mem: &GuestMem) -> Result<Rm, CpuError> {
        let b = self.fetch8(mem)? as usize;
        let md = b >> 6;
        let reg = (b >> 3) & 7;
        let rm = b & 7;
        if md == 3 {
            return Ok(Rm {
                reg,
                addr: None,
                idx: rm,
            });
        }
        if self.decode_address16 {
            let mut ea = match rm {
                0 => self.regs[EBX].wrapping_add(self.regs[ESI]),
                1 => self.regs[EBX].wrapping_add(self.regs[EDI]),
                2 => self.regs[EBP].wrapping_add(self.regs[ESI]),
                3 => self.regs[EBP].wrapping_add(self.regs[EDI]),
                4 => self.regs[ESI],
                5 => self.regs[EDI],
                6 if md == 0 => self.fetch16(mem)? as u32,
                6 => self.regs[EBP],
                _ => self.regs[EBX],
            };
            ea = match md {
                1 => ea.wrapping_add(self.fetch8(mem)? as i8 as i32 as u32),
                2 => ea.wrapping_add(self.fetch16(mem)? as u32),
                _ => ea,
            } & 0xffff;
            return Ok(Rm {
                reg,
                addr: Some(ea),
                idx: 0,
            });
        }
        let mut ea: u32;
        if rm == 4 {
            let s = self.fetch8(mem)? as usize;
            let ss = (s >> 6) as u32;
            let index = (s >> 3) & 7;
            let base = s & 7;
            let mut v = if base == 5 && md == 0 {
                self.fetch32(mem)?
            } else {
                self.regs[base]
            };
            if index != 4 {
                v = v.wrapping_add(self.regs[index].wrapping_shl(ss));
            }
            ea = v;
        } else if rm == 5 && md == 0 {
            ea = self.fetch32(mem)?;
        } else {
            ea = self.regs[rm];
        }
        match md {
            1 => ea = ea.wrapping_add(self.fetch8(mem)? as i8 as i32 as u32),
            2 => ea = ea.wrapping_add(self.fetch32(mem)?),
            _ => {}
        }
        Ok(Rm {
            reg,
            addr: Some(ea),
            idx: 0,
        })
    }

    fn rm_read(&self, mem: &GuestMem, rm: &Rm, bits: u8, seg: u32) -> Result<u32, CpuError> {
        match rm.addr {
            Some(a) => {
                let a = a.wrapping_add(seg);
                match bits {
                    8 => Ok(mem.read_u8(a)? as u32),
                    16 => Ok(mem.read_u16(a)? as u32),
                    _ => Ok(mem.read_u32(a)?),
                }
            }
            None => Ok(self.get_r(rm.idx, bits)),
        }
    }

    fn rm_write(
        &mut self,
        mem: &GuestMem,
        rm: &Rm,
        v: u32,
        bits: u8,
        seg: u32,
    ) -> Result<(), CpuError> {
        match rm.addr {
            Some(a) => {
                let a = a.wrapping_add(seg);
                match bits {
                    8 => mem.write_u8(a, v as u8)?,
                    16 => mem.write_u16(a, v as u16)?,
                    _ => mem.write_u32(a, v)?,
                }
            }
            None => self.set_r(rm.idx, v, bits),
        }
        Ok(())
    }

    fn cc(&self, cond: u8) -> bool {
        match cond & 15 {
            0x0 => self.flag(OF),
            0x1 => !self.flag(OF),
            0x2 => self.flag(CF),
            0x3 => !self.flag(CF),
            0x4 => self.flag(ZF),
            0x5 => !self.flag(ZF),
            0x6 => self.flag(CF) || self.flag(ZF),
            0x7 => !self.flag(CF) && !self.flag(ZF),
            0x8 => self.flag(SF),
            0x9 => !self.flag(SF),
            0xA => self.flag(PF),
            0xB => !self.flag(PF),
            0xC => self.flag(SF) != self.flag(OF),
            0xD => self.flag(SF) == self.flag(OF),
            0xE => self.flag(ZF) || (self.flag(SF) != self.flag(OF)),
            _ => !self.flag(ZF) && (self.flag(SF) == self.flag(OF)),
        }
    }

    fn unsupported(&self, mem: &GuestMem, eip0: u32, desc: &str) -> CpuError {
        let mut bytes = Vec::new();
        for i in 0..10 {
            match mem.read_u8(eip0.wrapping_add(i)) {
                Ok(b) => bytes.push(b),
                Err(_) => break,
            }
        }
        let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
        CpuError::Unsupported {
            eip: eip0,
            desc: format!("{desc} [bytes: {}]", hex.join(" ")),
        }
    }

    // ---- instruction helpers ----

    fn alu_forms(
        &mut self,
        mem: &mut GuestMem,
        seg: u32,
        bits: u8,
        form: u8,
        op: Alu,
    ) -> Result<(), CpuError> {
        match form & 7 {
            0 => {
                let rm = self.modrm(mem)?;
                let d = self.rm_read(mem, &rm, 8, seg)?;
                let s = self.get_r(rm.reg, 8);
                let r = self.alu(op, d, s, 8);
                if op != Alu::Cmp {
                    self.rm_write(mem, &rm, r, 8, seg)?;
                }
            }
            1 => {
                let rm = self.modrm(mem)?;
                let d = self.rm_read(mem, &rm, bits, seg)?;
                let s = self.get_r(rm.reg, bits);
                let r = self.alu(op, d, s, bits);
                if op != Alu::Cmp {
                    self.rm_write(mem, &rm, r, bits, seg)?;
                }
            }
            2 => {
                let rm = self.modrm(mem)?;
                let s = self.rm_read(mem, &rm, 8, seg)?;
                let d = self.get_r(rm.reg, 8);
                let r = self.alu(op, d, s, 8);
                if op != Alu::Cmp {
                    self.set_r(rm.reg, r, 8);
                }
            }
            3 => {
                let rm = self.modrm(mem)?;
                let s = self.rm_read(mem, &rm, bits, seg)?;
                let d = self.get_r(rm.reg, bits);
                let r = self.alu(op, d, s, bits);
                if op != Alu::Cmp {
                    self.set_r(rm.reg, r, bits);
                }
            }
            4 => {
                let imm = self.fetch8(mem)? as u32;
                let r = self.alu(op, self.get_r(EAX, 8), imm, 8);
                if op != Alu::Cmp {
                    self.set_r(EAX, r, 8);
                }
            }
            _ => {
                let imm = self.fetch_sized(mem, bits)?;
                let r = self.alu(op, self.get_r(EAX, bits), imm, bits);
                if op != Alu::Cmp {
                    self.set_r(EAX, r, bits);
                }
            }
        }
        Ok(())
    }

    fn group1(
        &mut self,
        mem: &mut GuestMem,
        seg: u32,
        bits: u8,
        imm8_form: bool,
        byte_form: bool,
    ) -> Result<(), CpuError> {
        let rm = self.modrm(mem)?;
        let b = if byte_form { 8 } else { bits };
        let imm = if byte_form {
            self.fetch8(mem)? as u32
        } else if imm8_form {
            self.fetch8(mem)? as i8 as i32 as u32
        } else {
            self.fetch_sized(mem, bits)?
        };
        let op = alu_from_index(rm.reg);
        let d = self.rm_read(mem, &rm, b, seg)?;
        let r = self.alu(op, d, imm, b);
        if op != Alu::Cmp {
            self.rm_write(mem, &rm, r, b, seg)?;
        }
        Ok(())
    }

    fn shift(&mut self, kind: usize, val: u32, count: u32, bits: u8) -> u32 {
        let m = mask(bits);
        let v = val & m;
        let c = count & 31; // 80386+ masks the count to 5 bits
        if c == 0 {
            return val & m;
        }
        let c = c as u8;
        let width = bits as u32;
        match kind {
            0 | 1 => {
                // rol / ror
                let c2 = (c as u32) % width;
                let r = if kind == 0 {
                    v.rotate_left(c2) & m
                } else {
                    v.rotate_right(c2) & m
                };
                let r = if bits == 32 {
                    r
                } else if kind == 0 {
                    // rotate within the narrower width
                    let c2 = c2 % width;
                    ((v << c2) | (v >> (width - c2))) & m
                } else {
                    let c2 = c2 % width;
                    ((v >> c2) | (v << (width - c2))) & m
                };
                self.set_flag(
                    CF,
                    if kind == 0 {
                        r & 1 != 0
                    } else {
                        r & sbit(bits) != 0
                    },
                );
                if c == 1 {
                    let msb = (r >> (bits - 1)) & 1;
                    let of = if kind == 0 {
                        msb ^ (r & 1)
                    } else {
                        msb ^ ((r >> (bits - 2)) & 1)
                    };
                    self.set_flag(OF, of != 0);
                }
                r
            }
            2 | 3 => {
                // rcl / rcr through CF, in a (bits+1)-bit field
                let total = width + 1;
                let c2 = (c as u32) % total;
                let full = (v as u64) | ((self.flag(CF) as u64) << width);
                let mt = (1u64 << total) - 1;
                let r = if c2 == 0 {
                    full
                } else if kind == 2 {
                    ((full << c2) | (full >> (total - c2))) & mt
                } else {
                    ((full >> c2) | (full << (total - c2))) & mt
                };
                self.set_flag(CF, (r >> width) & 1 != 0);
                if c == 1 {
                    // OF only defined for count 1; rcl: MSB(result) ^ CF,
                    // rcr: MSB ^ next-MSB of result
                    let res = (r as u32) & m;
                    let of = if kind == 2 {
                        ((res >> (bits - 1)) & 1) ^ ((r >> width) as u32 & 1)
                    } else {
                        ((res >> (bits - 1)) & 1) ^ ((res >> (bits - 2)) & 1)
                    };
                    self.set_flag(OF, of != 0);
                }
                (r as u32) & m
            }
            4 | 6 => {
                // shl / sal (intel: /6 is an alias of /4)
                if (c as u32) >= width {
                    self.set_flag(CF, false);
                    self.set_szp(0, bits);
                    return 0;
                }
                let r = (v << c) & m;
                self.set_flag(CF, (v >> (bits - c)) & 1 != 0);
                if c == 1 {
                    self.set_flag(OF, ((r >> (bits - 1)) & 1) != (self.flag(CF) as u32));
                }
                self.set_szp(r, bits);
                r
            }
            5 => {
                // shr
                if (c as u32) >= width {
                    self.set_flag(CF, false);
                    self.set_szp(0, bits);
                    return 0;
                }
                let r = (v >> c) & m;
                self.set_flag(CF, (v >> (c - 1)) & 1 != 0);
                if c == 1 {
                    self.set_flag(OF, v & sbit(bits) != 0);
                }
                self.set_szp(r, bits);
                r
            }
            _ => {
                // sar (kind == 7)
                let sv = if bits == 32 {
                    v as i32
                } else {
                    let shift = 32 - bits as u32;
                    ((v << shift) as i32) >> shift
                };
                if (c as u32) >= width {
                    let r = if sv < 0 { m } else { 0 };
                    self.set_flag(CF, sv < 0);
                    self.set_szp(r, bits);
                    return r;
                }
                let r = ((sv >> c) as u32) & m;
                self.set_flag(CF, (sv >> (c - 1)) & 1 != 0);
                if c == 1 {
                    self.set_flag(OF, false);
                }
                self.set_szp(r, bits);
                r
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn string_op(
        &mut self,
        mem: &mut GuestMem,
        op: u8,
        rep: Option<u8>,
        bits: u8,
    ) -> Result<(), CpuError> {
        let size: u32 = match op {
            0xA4 | 0xA6 | 0xAA | 0xAC | 0xAE => 1,
            _ => (bits / 8) as u32,
        };
        let step: u32 = if self.flag(DF) {
            0u32.wrapping_sub(size)
        } else {
            size
        };
        let is_cmp = matches!(op, 0xA6 | 0xA7 | 0xAE | 0xAF);
        loop {
            if rep.is_some() {
                if self.regs[ECX] == 0 {
                    break;
                }
                self.regs[ECX] = self.regs[ECX].wrapping_sub(1);
            }
            match op {
                0xA4 | 0xA5 => {
                    let v = read_sized(mem, self.regs[ESI], size)?;
                    write_sized(mem, self.regs[EDI], v, size)?;
                    self.regs[ESI] = self.regs[ESI].wrapping_add(step);
                    self.regs[EDI] = self.regs[EDI].wrapping_add(step);
                }
                0xAA | 0xAB => {
                    let v = self.get_r(EAX, size_to_bits(size));
                    write_sized(mem, self.regs[EDI], v, size)?;
                    self.regs[EDI] = self.regs[EDI].wrapping_add(step);
                }
                0xAC | 0xAD => {
                    let v = read_sized(mem, self.regs[ESI], size)?;
                    self.set_r(EAX, v, size_to_bits(size));
                    self.regs[ESI] = self.regs[ESI].wrapping_add(step);
                }
                0xA6 | 0xA7 => {
                    let a = read_sized(mem, self.regs[ESI], size)?;
                    let b = read_sized(mem, self.regs[EDI], size)?;
                    self.arith_sub(a, b, 0, size_to_bits(size));
                    self.regs[ESI] = self.regs[ESI].wrapping_add(step);
                    self.regs[EDI] = self.regs[EDI].wrapping_add(step);
                }
                _ => {
                    // 0xAE / 0xAF scas
                    let a = self.get_r(EAX, size_to_bits(size));
                    let b = read_sized(mem, self.regs[EDI], size)?;
                    self.arith_sub(a, b, 0, size_to_bits(size));
                    self.regs[EDI] = self.regs[EDI].wrapping_add(step);
                }
            }
            match rep {
                None => break,
                Some(0xF3) => {
                    if is_cmp && !self.flag(ZF) {
                        break;
                    }
                }
                Some(_) => {
                    if is_cmp && self.flag(ZF) {
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    fn group3(
        &mut self,
        mem: &mut GuestMem,
        seg: u32,
        bits: u8,
        eip0: u32,
    ) -> Result<(), CpuError> {
        let rm = self.modrm(mem)?;
        match rm.reg {
            0 => {
                let imm = self.fetch_sized(mem, bits)?;
                let a = self.rm_read(mem, &rm, bits, seg)?;
                self.arith_logic(a & imm, bits);
            }
            2 => {
                let a = self.rm_read(mem, &rm, bits, seg)?;
                self.rm_write(mem, &rm, !a, bits, seg)?;
            }
            3 => {
                let a = self.rm_read(mem, &rm, bits, seg)?;
                let r = self.arith_sub(0, a, 0, bits);
                self.rm_write(mem, &rm, r, bits, seg)?;
            }
            4 | 5 => {
                let signed = rm.reg == 5;
                let a = self.rm_read(mem, &rm, bits, seg)?;
                match bits {
                    8 => {
                        let al = self.get_r(EAX, 8);
                        let (r, fits) = if signed {
                            let p = (al as u8 as i8 as i32) * (a as u8 as i8 as i32);
                            let r = (p as u16) as u32;
                            (r, p == (r as u8 as i8 as i32))
                        } else {
                            let p = al * a;
                            (p & 0xffff, p & 0xff00 == 0)
                        };
                        self.regs[EAX] = (self.regs[EAX] & !0xffff) | r;
                        self.set_flag(CF, !fits);
                        self.set_flag(OF, !fits);
                    }
                    16 => {
                        let ax = self.get_r(EAX, 16);
                        let (hi, lo, fits) = if signed {
                            let p = (ax as u16 as i16 as i64) * (a as u16 as i16 as i64);
                            let r = p as u32;
                            (
                                (r >> 16) & 0xffff,
                                r & 0xffff,
                                p == ((r & 0xffff) as u16 as i16 as i64),
                            )
                        } else {
                            let p = (ax as u64) * (a as u64);
                            (
                                (p >> 16) as u32 & 0xffff,
                                p as u32 & 0xffff,
                                p & 0xffff_0000 == 0,
                            )
                        };
                        self.regs[EAX] = (self.regs[EAX] & !0xffff) | lo;
                        self.regs[EDX] = (self.regs[EDX] & !0xffff) | hi;
                        self.set_flag(CF, !fits);
                        self.set_flag(OF, !fits);
                    }
                    _ => {
                        let eax = self.regs[EAX];
                        let (hi, lo, fits) = if signed {
                            let p = (eax as i32 as i64) * (a as i32 as i64);
                            let r = p as u64;
                            ((r >> 32) as u32, r as u32, p == ((r as u32) as i32 as i64))
                        } else {
                            let p = (eax as u64) * (a as u64);
                            ((p >> 32) as u32, p as u32, p >> 32 == 0)
                        };
                        self.regs[EAX] = lo;
                        self.regs[EDX] = hi;
                        self.set_flag(CF, !fits);
                        self.set_flag(OF, !fits);
                    }
                }
            }
            6 | 7 => {
                let signed = rm.reg == 7;
                let d = self.rm_read(mem, &rm, bits, seg)?;
                match bits {
                    8 => {
                        if d == 0 {
                            return Err(CpuError::DivideError { eip: eip0 });
                        }
                        let ax = self.get_r(EAX, 16);
                        let (q, r, ok) = if signed {
                            let n = ax as u16 as i16 as i32;
                            let dd = d as u8 as i8 as i32;
                            if dd == 0 {
                                return Err(CpuError::DivideError { eip: eip0 });
                            }
                            let q = n.wrapping_div(dd);
                            let r = n.wrapping_rem(dd);
                            (q as u32, r as u32, (-128..=127).contains(&q))
                        } else {
                            (ax / d, ax % d, ax / d <= 0xff)
                        };
                        if !ok {
                            return Err(CpuError::DivideError { eip: eip0 });
                        }
                        self.regs[EAX] =
                            (self.regs[EAX] & !0xffff) | ((r & 0xff) << 8) | (q & 0xff);
                    }
                    16 => {
                        if d == 0 {
                            return Err(CpuError::DivideError { eip: eip0 });
                        }
                        let n = (self.get_r(EDX, 16) << 16) | self.get_r(EAX, 16);
                        let (q, r, ok) = if signed {
                            let n = n as i32;
                            let dd = d as u16 as i16 as i32;
                            if dd == 0 {
                                return Err(CpuError::DivideError { eip: eip0 });
                            }
                            if n == i32::MIN && dd == -1 {
                                return Err(CpuError::DivideError { eip: eip0 });
                            }
                            let q = n / dd;
                            let r = n % dd;
                            (q as u32, r as u32, (-32768..=32767).contains(&q))
                        } else {
                            (n / d, n % d, n / d <= 0xffff)
                        };
                        if !ok {
                            return Err(CpuError::DivideError { eip: eip0 });
                        }
                        self.regs[EAX] = (self.regs[EAX] & !0xffff) | (q & 0xffff);
                        self.regs[EDX] = (self.regs[EDX] & !0xffff) | (r & 0xffff);
                    }
                    _ => {
                        if d == 0 {
                            return Err(CpuError::DivideError { eip: eip0 });
                        }
                        let n = ((self.regs[EDX] as u64) << 32) | (self.regs[EAX] as u64);
                        let (q, r, ok) = if signed {
                            let n = n as i64;
                            let dd = d as i32 as i64;
                            if dd == 0 {
                                return Err(CpuError::DivideError { eip: eip0 });
                            }
                            if n == i64::MIN && dd == -1 {
                                return Err(CpuError::DivideError { eip: eip0 });
                            }
                            let q = n / dd;
                            let r = n % dd;
                            (
                                q as u64,
                                r as u64,
                                q >= i32::MIN as i64 && q <= i32::MAX as i64,
                            )
                        } else {
                            (n / d as u64, n % d as u64, n / d as u64 <= 0xffff_ffff)
                        };
                        if !ok {
                            return Err(CpuError::DivideError { eip: eip0 });
                        }
                        self.regs[EAX] = q as u32;
                        self.regs[EDX] = r as u32;
                    }
                }
            }
            _ => return Err(self.unsupported(mem, eip0, "group3 /1")),
        }
        Ok(())
    }

    fn bt_op(
        &mut self,
        mem: &mut GuestMem,
        seg: u32,
        bits: u8,
        kind: usize, // 4 bt, 5 bts, 6 btr, 7 btc
        bit_from_reg: bool,
    ) -> Result<(), CpuError> {
        let rm = self.modrm(mem)?;
        let width = bits as i32;
        let raw_bit = if bit_from_reg {
            self.get_r(rm.reg, bits) as i32
        } else {
            self.fetch8(mem)? as i32
        };
        match rm.addr {
            Some(a) => {
                let (byte_off, bit) = if bit_from_reg {
                    (
                        raw_bit.div_euclid(width) * (width / 8),
                        raw_bit.rem_euclid(width) as u32,
                    )
                } else {
                    (0, (raw_bit as u32) & (width as u32 - 1))
                };
                let addr = (a as i64 + byte_off as i64) as u32;
                let addr = addr.wrapping_add(seg);
                let v = read_sized(mem, addr, (bits / 8) as u32)?;
                self.set_flag(CF, (v >> bit) & 1 != 0);
                let nv = match kind {
                    5 => v | (1 << bit),
                    6 => v & !(1 << bit),
                    7 => v ^ (1 << bit),
                    _ => v,
                };
                if nv != v {
                    write_sized(mem, addr, nv, (bits / 8) as u32)?;
                }
            }
            None => {
                let bit = (raw_bit as u32) & (width as u32 - 1);
                let v = self.get_r(rm.idx, bits);
                self.set_flag(CF, (v >> bit) & 1 != 0);
                let nv = match kind {
                    5 => v | (1 << bit),
                    6 => v & !(1 << bit),
                    7 => v ^ (1 << bit),
                    _ => v,
                };
                self.set_r(rm.idx, nv, bits);
            }
        }
        Ok(())
    }

    fn cpuid(&mut self) {
        let leaf = self.regs[EAX];
        match leaf {
            0 => {
                self.regs[EAX] = 1;
                self.regs[EBX] = 0x756e_6547; // "Genu"
                self.regs[EDX] = 0x4965_6e69; // "ineI"
                self.regs[ECX] = 0x6c65_746e; // "ntel"
            }
            1 => {
                self.regs[EAX] = 0x0000_0673;
                self.regs[EBX] = 0;
                self.regs[ECX] = 0; // deliberately minimal; grows with interpreter coverage
                self.regs[EDX] = (1 << 0) | (1 << 15); // FPU | CMOV
            }
            0x8000_0000 => {
                self.regs[EAX] = 0x8000_0001;
                self.regs[EBX] = 0;
                self.regs[ECX] = 0;
                self.regs[EDX] = 0;
            }
            _ => {
                self.regs[EAX] = 0;
                self.regs[EBX] = 0;
                self.regs[ECX] = 0;
                self.regs[EDX] = 0;
            }
        }
    }

    // ---- main dispatch ----

    /// Single-step exposes canonical EFLAGS, including when using IR.
    pub fn step(&mut self, mem: &mut GuestMem) -> Result<(), CpuError> {
        let result = self.step_cached(mem);
        self.flush_flags();
        result
    }

    fn step_x86(&mut self, mem: &mut GuestMem) -> Result<(), CpuError> {
        self.decode_address16 = false;
        let eip0 = self.eip;
        if self.trace_enabled {
            let mut bytes = Vec::with_capacity(8);
            for i in 0..8 {
                match mem.read_u8(eip0.wrapping_add(i)) {
                    Ok(b) => bytes.push(b),
                    Err(_) => break,
                }
            }
            self.trace_ring.push_back((eip0, bytes));
            if self.trace_ring.len() > 256 {
                self.trace_ring.pop_front();
            }
        }
        let mut opsize16 = false;
        let mut address16 = false;
        let mut seg: u32 = 0;
        let mut rep: Option<u8> = None;
        let op = loop {
            let b = mem.fetch_u8(self.eip)?;
            match b {
                0x66 => {
                    opsize16 = !opsize16;
                    self.eip = self.eip.wrapping_add(1);
                }
                0x67 => {
                    address16 = true;
                    self.eip = self.eip.wrapping_add(1);
                }
                0x2E | 0x36 | 0x3E | 0x26 => self.eip = self.eip.wrapping_add(1),
                0x64 => {
                    seg = self.fs_base;
                    self.eip = self.eip.wrapping_add(1);
                }
                0x65 => {
                    seg = self.gs_base;
                    self.eip = self.eip.wrapping_add(1);
                }
                0xF0 => self.eip = self.eip.wrapping_add(1), // lock: no-op (single CPU)
                0xF2 | 0xF3 => {
                    rep = Some(b);
                    self.eip = self.eip.wrapping_add(1);
                }
                _ => break b,
            }
        };
        let bits: u8 = if opsize16 { 16 } else { 32 };
        // Prefix scanning already fetched this opcode with execute checks.
        // No guest/host action can change the page between that fetch and
        // dispatch; consume the byte once, without prefetching any operands.
        self.eip = self.eip.wrapping_add(1);
        if address16 && !matches!(op, 0x88..=0x8b | 0x8d | 0xa0..=0xa3 | 0xc6 | 0xc7) {
            return Err(self.unsupported(mem, eip0, "16-bit addressing outside MOV/LEA"));
        }
        self.decode_address16 = address16;
        if self.profile_opcodes {
            *self.op_stats.entry(op as u32).or_insert(0) += 1;
            *self.raw_op_stats.entry(op as u32).or_insert(0) += 1;
        }

        match op {
            0x00..=0x05 => self.alu_forms(mem, seg, bits, op, Alu::Add)?,
            0x08..=0x0D => self.alu_forms(mem, seg, bits, op, Alu::Or)?,
            0x10..=0x15 => self.alu_forms(mem, seg, bits, op, Alu::Adc)?,
            0x18..=0x1D => self.alu_forms(mem, seg, bits, op, Alu::Sbb)?,
            0x20..=0x25 => self.alu_forms(mem, seg, bits, op, Alu::And)?,
            0x28..=0x2D => self.alu_forms(mem, seg, bits, op, Alu::Sub)?,
            0x30..=0x35 => self.alu_forms(mem, seg, bits, op, Alu::Xor)?,
            0x38..=0x3D => self.alu_forms(mem, seg, bits, op, Alu::Cmp)?,

            0x40..=0x47 => {
                let i = (op - 0x40) as usize;
                let cf = self.flag(CF);
                let r = self.arith_add(self.get_r(i, bits), 1, 0, bits);
                self.set_flag(CF, cf);
                self.set_r(i, r, bits);
            }
            0x48..=0x4F => {
                let i = (op - 0x48) as usize;
                let cf = self.flag(CF);
                let r = self.arith_sub(self.get_r(i, bits), 1, 0, bits);
                self.set_flag(CF, cf);
                self.set_r(i, r, bits);
            }
            0x50..=0x57 => {
                let v = self.get_r((op - 0x50) as usize, bits);
                self.push_sized(mem, v, bits)?;
            }
            0x58..=0x5F => {
                let v = self.pop_sized(mem, bits)?;
                self.set_r((op - 0x58) as usize, v, bits);
            }
            0x60 => {
                if opsize16 {
                    return Err(self.unsupported(mem, eip0, "pusha (16-bit)"));
                }
                let esp = self.esp();
                self.push32(mem, self.regs[EAX])?;
                self.push32(mem, self.regs[ECX])?;
                self.push32(mem, self.regs[EDX])?;
                self.push32(mem, self.regs[EBX])?;
                self.push32(mem, esp)?;
                self.push32(mem, self.regs[EBP])?;
                self.push32(mem, self.regs[ESI])?;
                self.push32(mem, self.regs[EDI])?;
            }
            0x61 => {
                if opsize16 {
                    return Err(self.unsupported(mem, eip0, "popa (16-bit)"));
                }
                self.regs[EDI] = self.pop32(mem)?;
                self.regs[ESI] = self.pop32(mem)?;
                self.regs[EBP] = self.pop32(mem)?;
                self.set_esp(self.esp().wrapping_add(4));
                self.regs[EBX] = self.pop32(mem)?;
                self.regs[EDX] = self.pop32(mem)?;
                self.regs[ECX] = self.pop32(mem)?;
                self.regs[EAX] = self.pop32(mem)?;
            }
            0x68 => {
                let v = self.fetch_sized(mem, bits)?;
                self.push_sized(mem, v, bits)?;
            }
            0x69 | 0x6B => {
                let rm = self.modrm(mem)?;
                let a = self.rm_read(mem, &rm, bits, seg)?;
                let imm = if op == 0x69 {
                    self.fetch_sized(mem, bits)?
                } else {
                    self.fetch8(mem)? as i8 as i32 as u32
                };
                let sa = sign_extend(a, bits) as i64;
                let sb = sign_extend(imm, bits) as i64;
                let p = sa * sb;
                let r = (p as u32) & mask(bits);
                self.set_flag(CF, p != sign_extend(r, bits) as i64);
                self.set_flag(OF, p != sign_extend(r, bits) as i64);
                self.set_r(rm.reg, r, bits);
            }
            0x6A => {
                let v = self.fetch8(mem)? as i8 as i32 as u32;
                self.push_sized(mem, v, bits)?;
            }
            0x70..=0x7F => {
                let d = self.fetch8(mem)? as i8 as i32 as u32;
                if self.cc(op & 15) {
                    self.eip = self.eip.wrapping_add(d);
                }
            }
            0x80 => self.group1(mem, seg, bits, false, true)?,
            0x81 => self.group1(mem, seg, bits, false, false)?,
            0x83 => self.group1(mem, seg, bits, true, false)?,
            0x84 => {
                let rm = self.modrm(mem)?;
                let a = self.rm_read(mem, &rm, 8, seg)?;
                let b = self.get_r(rm.reg, 8);
                self.arith_logic(a & b, 8);
            }
            0x85 => {
                let rm = self.modrm(mem)?;
                let a = self.rm_read(mem, &rm, bits, seg)?;
                let b = self.get_r(rm.reg, bits);
                self.arith_logic(a & b, bits);
            }
            0x86 | 0x87 => {
                let b8 = op == 0x86;
                let rm = self.modrm(mem)?;
                let bb = if b8 { 8 } else { bits };
                let d = self.rm_read(mem, &rm, bb, seg)?;
                let s = self.get_r(rm.reg, bb);
                self.rm_write(mem, &rm, s, bb, seg)?;
                self.set_r(rm.reg, d, bb);
            }
            0x88 | 0x89 => {
                let b8 = op == 0x88;
                let rm = self.modrm(mem)?;
                let bb = if b8 { 8 } else { bits };
                let v = self.get_r(rm.reg, bb);
                self.rm_write(mem, &rm, v, bb, seg)?;
            }
            0x8A | 0x8B => {
                let b8 = op == 0x8A;
                let rm = self.modrm(mem)?;
                let bb = if b8 { 8 } else { bits };
                let v = self.rm_read(mem, &rm, bb, seg)?;
                self.set_r(rm.reg, v, bb);
            }
            0x8C => {
                // mov r/m16, sreg — store plausible flat-model selectors
                let rm = self.modrm(mem)?;
                let v: u32 = match rm.reg {
                    1 => 0x23, // cs
                    4 => 0x53, // fs
                    _ => 0x2b, // ds/es/ss/gs
                };
                self.rm_write(mem, &rm, v, 16, seg)?;
            }
            0x8D => {
                let rm = self.modrm(mem)?;
                match rm.addr {
                    Some(a) => self.set_r(rm.reg, a, bits),
                    None => return Err(self.unsupported(mem, eip0, "lea with register operand")),
                }
            }
            0x8E => return Err(self.unsupported(mem, eip0, "mov sreg, r/m16")),
            0x8F => {
                let rm = self.modrm(mem)?;
                if rm.reg != 0 {
                    return Err(self.unsupported(mem, eip0, "0x8f group /n"));
                }
                let v = self.pop_sized(mem, bits)?;
                self.rm_write(mem, &rm, v, bits, seg)?;
            }
            0x90 => {} // nop
            0x91..=0x97 => {
                let i = (op - 0x90) as usize;
                let a = self.get_r(EAX, bits);
                let b = self.get_r(i, bits);
                self.set_r(EAX, b, bits);
                self.set_r(i, a, bits);
            }
            0x98 => {
                if opsize16 {
                    // cbw
                    let v = self.regs[EAX] as u8 as i8 as i16 as u16 as u32;
                    self.regs[EAX] = (self.regs[EAX] & !0xffff) | v;
                } else {
                    // cwde
                    self.regs[EAX] = self.regs[EAX] as u16 as i16 as i32 as u32;
                }
            }
            0x99 => {
                if opsize16 {
                    let v: u32 = if self.regs[EAX] & 0x8000 != 0 {
                        0xffff
                    } else {
                        0
                    };
                    self.regs[EDX] = (self.regs[EDX] & !0xffff) | v;
                } else {
                    self.regs[EDX] = if self.regs[EAX] as i32 & 0x8000_0000u32 as i32 != 0 {
                        0xffff_ffff
                    } else {
                        0
                    };
                }
            }
            0x9B => self.fpu_wait(eip0)?, // fwait
            0x9C => {
                let v = (self.eflags | 2) & mask(bits);
                self.push_sized(mem, v, bits)?;
            }
            0x9D => {
                let v = self.pop_sized(mem, bits)?;
                // TF is masked off: single-step #DB is not emulated (a guest
                // that sets TF would otherwise diverge silently). IF forced
                // to the usual user-mode state.
                self.eflags = (v & 0x0cd5) | 0x202;
            }
            0x9E => {
                // sahf
                let ah = (self.regs[EAX] >> 8) & 0xff;
                self.eflags = (self.eflags & !0x0d5) | (ah & 0xd5) | 2;
            }
            0x9F => {
                // lahf
                let v = (self.eflags & 0xd5) | 2;
                self.regs[EAX] = (self.regs[EAX] & !0xff00) | (v << 8);
            }
            0xA0 => {
                let a = self
                    .fetch_sized(mem, if address16 { 16 } else { 32 })?
                    .wrapping_add(seg);
                let v = mem.read_u8(a)?;
                self.set_r(EAX, v as u32, 8);
            }
            0xA1 => {
                let a = self
                    .fetch_sized(mem, if address16 { 16 } else { 32 })?
                    .wrapping_add(seg);
                let v = read_sized(mem, a, (bits / 8) as u32)?;
                self.set_r(EAX, v, bits);
            }
            0xA2 => {
                let a = self
                    .fetch_sized(mem, if address16 { 16 } else { 32 })?
                    .wrapping_add(seg);
                let v = self.get_r(EAX, 8);
                mem.write_u8(a, v as u8)?;
            }
            0xA3 => {
                let a = self
                    .fetch_sized(mem, if address16 { 16 } else { 32 })?
                    .wrapping_add(seg);
                let v = self.get_r(EAX, bits);
                write_sized(mem, a, v, (bits / 8) as u32)?;
            }
            0xA4..=0xA7 | 0xAA..=0xAF => self.string_op(mem, op, rep, bits)?,
            0xA8 => {
                let imm = self.fetch8(mem)? as u32;
                self.arith_logic(self.get_r(EAX, 8) & imm, 8);
            }
            0xA9 => {
                let imm = self.fetch_sized(mem, bits)?;
                let a = self.get_r(EAX, bits);
                self.arith_logic(a & imm, bits);
            }
            0xB0..=0xB7 => {
                let v = self.fetch8(mem)?;
                self.set_r8((op - 0xB0) as usize, v);
            }
            0xB8..=0xBF => {
                let v = self.fetch_sized(mem, bits)?;
                self.set_r((op - 0xB8) as usize, v, bits);
            }
            0xC0 | 0xC1 => {
                let b8 = op == 0xC0;
                let rm = self.modrm(mem)?;
                let c = self.fetch8(mem)? as u32;
                let bb = if b8 { 8 } else { bits };
                let v = self.rm_read(mem, &rm, bb, seg)?;
                let r = self.shift(rm.reg, v, c, bb);
                self.rm_write(mem, &rm, r, bb, seg)?;
            }
            0xC2 => {
                let n = self.fetch16(mem)? as u32;
                let r = self.pop32(mem)?;
                self.set_esp(self.esp().wrapping_add(n));
                self.eip = r;
            }
            0xC3 => {
                self.eip = self.pop32(mem)?;
            }
            0xC6 => {
                let rm = self.modrm(mem)?;
                if rm.reg != 0 {
                    return Err(self.unsupported(mem, eip0, "0xc6 group /n"));
                }
                let v = self.fetch8(mem)? as u32;
                self.rm_write(mem, &rm, v, 8, seg)?;
            }
            0xC7 => {
                let rm = self.modrm(mem)?;
                if rm.reg != 0 {
                    return Err(self.unsupported(mem, eip0, "0xc7 group /n"));
                }
                let v = self.fetch_sized(mem, bits)?;
                self.rm_write(mem, &rm, v, bits, seg)?;
            }
            0xC8 => {
                let size = self.fetch16(mem)? as u32;
                let nesting = self.fetch8(mem)? & 31;
                if nesting != 0 {
                    return Err(self.unsupported(mem, eip0, "enter with nesting"));
                }
                self.push32(mem, self.regs[EBP])?;
                self.regs[EBP] = self.esp();
                self.set_esp(self.esp().wrapping_sub(size));
            }
            0xC9 => {
                self.set_esp(self.regs[EBP]);
                self.regs[EBP] = self.pop32(mem)?;
            }
            0xCC => return Err(CpuError::Breakpoint { eip: eip0 }),
            0xCD => return Err(self.unsupported(mem, eip0, "int imm8")),
            0xD0..=0xD3 => {
                let b8 = op == 0xD0 || op == 0xD2;
                let rm = self.modrm(mem)?;
                let c = if op == 0xD0 || op == 0xD1 {
                    1
                } else {
                    self.regs[ECX] as u8 as u32
                };
                let bb = if b8 { 8 } else { bits };
                let v = self.rm_read(mem, &rm, bb, seg)?;
                let r = self.shift(rm.reg, v, c, bb);
                self.rm_write(mem, &rm, r, bb, seg)?;
            }
            0xD8..=0xDF => self.fpu_op(mem, seg, bits, op, eip0)?,
            0xE0..=0xE2 => {
                let d = self.fetch8(mem)? as i8 as i32 as u32;
                self.regs[ECX] = self.regs[ECX].wrapping_sub(1);
                let zf = self.flag(ZF);
                let take = match op {
                    0xE0 => self.regs[ECX] != 0 && !zf,
                    0xE1 => self.regs[ECX] != 0 && zf,
                    _ => self.regs[ECX] != 0,
                };
                if take {
                    self.eip = self.eip.wrapping_add(d);
                }
            }
            0xE3 => {
                let d = self.fetch8(mem)? as i8 as i32 as u32;
                if self.regs[ECX] == 0 {
                    self.eip = self.eip.wrapping_add(d);
                }
            }
            0xE8 => {
                let d = self.fetch32(mem)?;
                self.push32(mem, self.eip)?;
                self.eip = self.eip.wrapping_add(d);
            }
            0xE9 => {
                let d = self.fetch32(mem)?;
                self.eip = self.eip.wrapping_add(d);
            }
            0xEB => {
                let d = self.fetch8(mem)? as i8 as i32 as u32;
                self.eip = self.eip.wrapping_add(d);
            }
            0xF5 => {
                let f = !self.flag(CF);
                self.set_flag(CF, f);
            }
            0xF6 => self.group3(mem, seg, 8, eip0)?,
            0xF7 => self.group3(mem, seg, bits, eip0)?,
            0xF8 => self.set_flag(CF, false),
            0xF9 => self.set_flag(CF, true),
            0xFA | 0xFB => {} // cli/sti: irrelevant in user-mode emulation
            0xFC => self.set_flag(DF, false),
            0xFD => self.set_flag(DF, true),
            0xFE => {
                let rm = self.modrm(mem)?;
                let cf = self.flag(CF);
                match rm.reg {
                    0 => {
                        let a = self.rm_read(mem, &rm, 8, seg)?;
                        let r = self.arith_add(a, 1, 0, 8);
                        self.set_flag(CF, cf);
                        self.rm_write(mem, &rm, r, 8, seg)?;
                    }
                    1 => {
                        let a = self.rm_read(mem, &rm, 8, seg)?;
                        let r = self.arith_sub(a, 1, 0, 8);
                        self.set_flag(CF, cf);
                        self.rm_write(mem, &rm, r, 8, seg)?;
                    }
                    _ => return Err(self.unsupported(mem, eip0, "0xfe group /n")),
                }
            }
            0xFF => {
                let rm = self.modrm(mem)?;
                match rm.reg {
                    0 => {
                        let cf = self.flag(CF);
                        let a = self.rm_read(mem, &rm, bits, seg)?;
                        let r = self.arith_add(a, 1, 0, bits);
                        self.set_flag(CF, cf);
                        self.rm_write(mem, &rm, r, bits, seg)?;
                    }
                    1 => {
                        let cf = self.flag(CF);
                        let a = self.rm_read(mem, &rm, bits, seg)?;
                        let r = self.arith_sub(a, 1, 0, bits);
                        self.set_flag(CF, cf);
                        self.rm_write(mem, &rm, r, bits, seg)?;
                    }
                    2 => {
                        let t = self.rm_read(mem, &rm, 32, seg)?;
                        self.push32(mem, self.eip)?;
                        self.eip = t;
                    }
                    4 => {
                        self.eip = self.rm_read(mem, &rm, 32, seg)?;
                    }
                    6 => {
                        let v = self.rm_read(mem, &rm, bits, seg)?;
                        self.push_sized(mem, v, bits)?;
                    }
                    _ => return Err(self.unsupported(mem, eip0, "0xff group far form")),
                }
            }
            0x0F => self.step_0f(mem, seg, bits, eip0, opsize16, rep)?,
            _ => return Err(self.unsupported(mem, eip0, &format!("opcode {op:02x}"))),
        }
        Ok(())
    }

    fn step_0f(
        &mut self,
        mem: &mut GuestMem,
        seg: u32,
        bits: u8,
        eip0: u32,
        op66: bool,
        rep: Option<u8>,
    ) -> Result<(), CpuError> {
        let op = self.fetch8(mem)?;
        if self.profile_opcodes {
            *self.op_stats.entry(0x100 | op as u32).or_insert(0) += 1;
            *self.raw_op_stats.entry(0x100 | op as u32).or_insert(0) += 1;
        }
        let descriptor_candidate = matches!(
            op,
            0x10 | 0x11
                | 0x28
                | 0x29
                | 0x6e
                | 0x7e
                | 0x6f
                | 0x7f
                | 0xd6
                | 0xa3
                | 0xab
                | 0xb3
                | 0xbb
                | 0xba
        ) || op66
            && matches!(op,0x64..=0x66|0x74..=0x76|0xd4|0xdb|0xdf|0xeb|0xef|0xf8..=0xfe)
            || !op66
                && rep.is_some()
                && matches!(op, 0x2a | 0x2c | 0x2d | 0x51 | 0x58..=0x5f | 0xc2)
            || rep.is_none() && matches!(op, 0x2e | 0x2f);
        if descriptor_candidate {
            if let Some(result) = self.try_raw_simd(mem, eip0) {
                return result;
            }
        }
        if !op66 && rep.is_none() && mmx::is_opcode(op) {
            return self.mmx(mem, seg, op, eip0);
        }
        match op {
            0x0D | 0x18..=0x1F => {
                // prefetch / multi-byte nop: consume modrm, do nothing
                self.modrm(mem)?;
            }
            0x0B => return Err(self.unsupported(mem, eip0, "ud2")),
            0x31 => {
                let t = self.t0.elapsed().as_nanos() as u64;
                self.regs[EAX] = t as u32;
                self.regs[EDX] = (t >> 32) as u32;
            }
            0x40..=0x4F => {
                let rm = self.modrm(mem)?;
                // cmov with a memory source performs the read even when the
                // condition is false (it can fault); only the register write
                // is conditional.
                let v = self.rm_read(mem, &rm, bits, seg)?;
                if self.cc(op & 15) {
                    self.set_r(rm.reg, v, bits);
                }
            }
            0x80..=0x8F => {
                let d = self.fetch32(mem)?;
                if self.cc(op & 15) {
                    self.eip = self.eip.wrapping_add(d);
                }
            }
            0x90..=0x9F => {
                let rm = self.modrm(mem)?;
                let v = self.cc(op & 15) as u32;
                self.rm_write(mem, &rm, v, 8, seg)?;
            }
            0xA2 => self.cpuid(),
            0xA3 => self.bt_op(mem, seg, bits, 4, true)?,
            0xAB => self.bt_op(mem, seg, bits, 5, true)?,
            0xB3 => self.bt_op(mem, seg, bits, 6, true)?,
            0xBB => self.bt_op(mem, seg, bits, 7, true)?,
            0xBA => {
                let rm = self.modrm(mem)?;
                // reg field selects the operation; re-dispatch manually
                let kind = rm.reg;
                if !(4..=7).contains(&kind) {
                    return Err(self.unsupported(mem, eip0, "0f ba group /n"));
                }
                // bt_op decodes its own modrm; we already consumed it, so
                // handle inline instead.
                let width = bits as u32;
                let bit = (self.fetch8(mem)? as u32) & (width - 1);
                match rm.addr {
                    Some(a) => {
                        let a = a.wrapping_add(seg);
                        let v = read_sized(mem, a, (bits / 8) as u32)?;
                        self.set_flag(CF, (v >> bit) & 1 != 0);
                        let nv = match kind {
                            5 => v | (1 << bit),
                            6 => v & !(1 << bit),
                            7 => v ^ (1 << bit),
                            _ => v,
                        };
                        if nv != v {
                            write_sized(mem, a, nv, (bits / 8) as u32)?;
                        }
                    }
                    None => {
                        let v = self.get_r(rm.idx, bits);
                        self.set_flag(CF, (v >> bit) & 1 != 0);
                        let nv = match kind {
                            5 => v | (1 << bit),
                            6 => v & !(1 << bit),
                            7 => v ^ (1 << bit),
                            _ => v,
                        };
                        self.set_r(rm.idx, nv, bits);
                    }
                }
            }
            0xA4 | 0xA5 | 0xAC | 0xAD => {
                // SHLD/SHRD r/m16/32, r16/32, imm8/CL.
                let rm = self.modrm(mem)?;
                let count = if op & 1 == 0 {
                    self.fetch8(mem)? as u32
                } else {
                    self.regs[ECX] & 0xff
                } & 31;
                let dest = self.rm_read(mem, &rm, bits, seg)?;
                let src = self.get_r(rm.reg, bits);
                if count != 0 {
                    let width = bits as u32;
                    let left = op < 0xAC;
                    let pair = if left {
                        ((dest as u64) << width) | src as u64
                    } else {
                        ((src as u64) << width) | dest as u64
                    };
                    let result = if left {
                        (pair << count) >> width
                    } else {
                        pair >> count
                    } as u32
                        & mask(bits);
                    let carry = if left {
                        (pair >> (2 * width - count)) & 1 != 0
                    } else {
                        (pair >> (count - 1)) & 1 != 0
                    };
                    self.set_flag(CF, carry);
                    if count == 1 {
                        let sign = 1u32 << (bits - 1);
                        self.set_flag(
                            OF,
                            if left {
                                (result & sign != 0) != carry
                            } else {
                                (dest ^ result) & sign != 0
                            },
                        );
                    }
                    self.set_szp(result, bits);
                    self.rm_write(mem, &rm, result, bits, seg)?;
                }
            }
            0xAF => {
                let rm = self.modrm(mem)?;
                let a = self.get_r(rm.reg, bits);
                let b = self.rm_read(mem, &rm, bits, seg)?;
                let p = sign_extend(a, bits) as i64 * sign_extend(b, bits) as i64;
                let r = (p as u32) & mask(bits);
                let fits = p == sign_extend(r, bits) as i64;
                self.set_flag(CF, !fits);
                self.set_flag(OF, !fits);
                self.set_r(rm.reg, r, bits);
            }
            0xB0 | 0xB1 => {
                let b8 = op == 0xB0;
                let bb = if b8 { 8 } else { bits };
                let rm = self.modrm(mem)?;
                let dest = self.rm_read(mem, &rm, bb, seg)?;
                let acc = self.get_r(EAX, bb);
                self.arith_sub(acc, dest, 0, bb);
                if acc == dest {
                    let src = self.get_r(rm.reg, bb);
                    self.rm_write(mem, &rm, src, bb, seg)?;
                } else {
                    self.set_r(EAX, dest, bb);
                }
            }
            0xB6 | 0xB7 => {
                let src_bits: u8 = if op == 0xB6 { 8 } else { 16 };
                let rm = self.modrm(mem)?;
                let v = self.rm_read(mem, &rm, src_bits, seg)?;
                self.set_r(rm.reg, v, bits);
            }
            0xBE | 0xBF => {
                let src_bits: u8 = if op == 0xBE { 8 } else { 16 };
                let rm = self.modrm(mem)?;
                let v = self.rm_read(mem, &rm, src_bits, seg)?;
                let sv = if src_bits == 8 {
                    v as u8 as i8 as i32 as u32
                } else {
                    v as u16 as i16 as i32 as u32
                };
                self.set_r(rm.reg, sv, bits);
            }
            0xBC | 0xBD => {
                let rm = self.modrm(mem)?;
                let v = self.rm_read(mem, &rm, bits, seg)? & mask(bits);
                if v == 0 {
                    self.set_flag(ZF, true);
                } else {
                    self.set_flag(ZF, false);
                    let idx = if op == 0xBC {
                        v.trailing_zeros()
                    } else {
                        31 - v.leading_zeros()
                    };
                    self.set_r(rm.reg, idx, bits);
                }
            }
            0xC0 | 0xC1 => {
                let b8 = op == 0xC0;
                let bb = if b8 { 8 } else { bits };
                let rm = self.modrm(mem)?;
                let dest = self.rm_read(mem, &rm, bb, seg)?;
                let src = self.get_r(rm.reg, bb);
                let r = self.arith_add(dest, src, 0, bb);
                self.set_r(rm.reg, dest, bb);
                self.rm_write(mem, &rm, r, bb, seg)?;
            }
            0xC8..=0xCF => {
                let i = (op - 0xC8) as usize;
                self.regs[i] = self.regs[i].swap_bytes();
            }
            // ---- SSE/SSE2 subset (game-driven; MOVAPS alignment faults and
            // MXCSR rounding modes are not modeled — documented) ----
            0x10 | 0x11 => {
                let rm = self.modrm(mem)?;
                let load = op == 0x10;
                match (op66, rep) {
                    (false, Some(0xF3)) => {
                        // movss
                        if load {
                            match rm.addr {
                                Some(a) => {
                                    let mut v = [0u8; 16];
                                    v[..4].copy_from_slice(
                                        &mem.read_u32(a.wrapping_add(seg))?.to_le_bytes(),
                                    );
                                    self.xmm[rm.reg] = v;
                                }
                                None => {
                                    let s = self.xmm[rm.idx];
                                    self.xmm[rm.reg][..4].copy_from_slice(&s[..4]);
                                }
                            }
                        } else {
                            match rm.addr {
                                Some(a) => {
                                    let v = u32::from_le_bytes(
                                        self.xmm[rm.reg][..4].try_into().unwrap(),
                                    );
                                    mem.write_u32(a.wrapping_add(seg), v)?;
                                }
                                None => {
                                    let s = self.xmm[rm.reg];
                                    self.xmm[rm.idx][..4].copy_from_slice(&s[..4]);
                                }
                            }
                        }
                    }
                    (false, Some(0xF2)) => {
                        // movsd
                        if load {
                            match rm.addr {
                                Some(a) => {
                                    let mut v = [0u8; 16];
                                    v[..8].copy_from_slice(
                                        &mem.read_u64(a.wrapping_add(seg))?.to_le_bytes(),
                                    );
                                    self.xmm[rm.reg] = v;
                                }
                                None => {
                                    let s = self.xmm[rm.idx];
                                    self.xmm[rm.reg][..8].copy_from_slice(&s[..8]);
                                }
                            }
                        } else {
                            match rm.addr {
                                Some(a) => {
                                    let v = u64::from_le_bytes(
                                        self.xmm[rm.reg][..8].try_into().unwrap(),
                                    );
                                    mem.write_u64(a.wrapping_add(seg), v)?;
                                }
                                None => {
                                    let s = self.xmm[rm.reg];
                                    self.xmm[rm.idx][..8].copy_from_slice(&s[..8]);
                                }
                            }
                        }
                    }
                    _ => {
                        // movups / movupd
                        if load {
                            let v = self.xmm_rm(mem, &rm, seg)?;
                            self.xmm[rm.reg] = v;
                        } else {
                            let v = self.xmm[rm.reg];
                            self.xmm_rm_write(mem, &rm, seg, v)?;
                        }
                    }
                }
            }
            0x12 => {
                let rm = self.modrm(mem)?;
                if !op66 && rep == Some(0xF2) {
                    // movddup
                    let v = self.xmm_rm_sized(mem, &rm, seg, 8)?;
                    let mut out = [0u8; 16];
                    out[..8].copy_from_slice(&v[..8]);
                    out[8..].copy_from_slice(&v[..8]);
                    self.xmm[rm.reg] = out;
                } else {
                    // movlps/movlpd load; reg-reg form is movhlps
                    match rm.addr {
                        Some(a) => {
                            let v = mem.read_u64(a.wrapping_add(seg))?;
                            self.xmm[rm.reg][..8].copy_from_slice(&v.to_le_bytes());
                        }
                        None => {
                            let s = self.xmm[rm.idx];
                            self.xmm[rm.reg][..8].copy_from_slice(&s[8..]);
                        }
                    }
                }
            }
            0x13 => {
                // movlps/movlpd store
                let rm = self.modrm(mem)?;
                let v = u64::from_le_bytes(self.xmm[rm.reg][..8].try_into().unwrap());
                match rm.addr {
                    Some(a) => mem.write_u64(a.wrapping_add(seg), v)?,
                    None => self.xmm[rm.idx][..8].copy_from_slice(&v.to_le_bytes()),
                }
            }
            0x14 | 0x15 => {
                // unpcklps/unpcklpd, unpckhps/unpckhpd (bitwise == punpckldq/dq)
                let rm = self.modrm(mem)?;
                let a = self.xmm[rm.reg];
                let b = self.xmm_rm(mem, &rm, seg)?;
                let lo = op == 0x14;
                let off = if lo { 0 } else { 8 };
                let mut out = [0u8; 16];
                if op66 {
                    out[..8].copy_from_slice(&a[off..off + 8]);
                    out[8..].copy_from_slice(&b[off..off + 8]);
                } else {
                    out[0..4].copy_from_slice(&a[off..off + 4]);
                    out[4..8].copy_from_slice(&b[off..off + 4]);
                    out[8..12].copy_from_slice(&a[off + 4..off + 8]);
                    out[12..16].copy_from_slice(&b[off + 4..off + 8]);
                }
                self.xmm[rm.reg] = out;
            }
            0x16 | 0x17 => {
                // movhps/movhpd; reg-reg load form is movlhps
                let rm = self.modrm(mem)?;
                if op == 0x16 {
                    match rm.addr {
                        Some(a) => {
                            let v = mem.read_u64(a.wrapping_add(seg))?;
                            self.xmm[rm.reg][8..].copy_from_slice(&v.to_le_bytes());
                        }
                        None => {
                            let s = self.xmm[rm.idx];
                            self.xmm[rm.reg][8..].copy_from_slice(&s[..8]);
                        }
                    }
                } else {
                    let v = u64::from_le_bytes(self.xmm[rm.reg][8..].try_into().unwrap());
                    match rm.addr {
                        Some(a) => mem.write_u64(a.wrapping_add(seg), v)?,
                        None => self.xmm[rm.idx][8..].copy_from_slice(&v.to_le_bytes()),
                    }
                }
            }
            0x28 | 0x29 => {
                // movaps/movapd (alignment faults not modeled)
                let rm = self.modrm(mem)?;
                if op == 0x28 {
                    let v = self.xmm_rm(mem, &rm, seg)?;
                    self.xmm[rm.reg] = v;
                } else {
                    let v = self.xmm[rm.reg];
                    self.xmm_rm_write(mem, &rm, seg, v)?;
                }
            }
            0x2B => {
                // movntps/movntpd: plain store here
                let rm = self.modrm(mem)?;
                let v = self.xmm[rm.reg];
                self.xmm_rm_write(mem, &rm, seg, v)?;
            }
            0x2A => {
                // cvtsi2ss / cvtsi2sd
                let rm = self.modrm(mem)?;
                let i = self.rm_read(mem, &rm, 32, seg)? as i32;
                match rep {
                    Some(0xF3) => {
                        let f = i as f32;
                        self.xmm[rm.reg][..4].copy_from_slice(&f.to_le_bytes());
                    }
                    Some(0xF2) => {
                        let f = i as f64;
                        self.xmm[rm.reg][..8].copy_from_slice(&f.to_le_bytes());
                    }
                    _ => return Err(self.unsupported(mem, eip0, "cvtsi2 with bad prefix")),
                }
            }
            0x2C | 0x2D => {
                // cvttss2si/cvttsd2si (truncate), cvtss2si/cvtsd2si (round-nearest)
                let rm = self.modrm(mem)?;
                let x64 = rep == Some(0xF2);
                let (v, valid) = if x64 {
                    let x = f64::from_le_bytes(
                        self.xmm_rm_sized(mem, &rm, seg, 8)?[..8]
                            .try_into()
                            .unwrap(),
                    );
                    let t = if op == 0x2C {
                        x.trunc()
                    } else {
                        x.round_ties_even()
                    };
                    (
                        t as i64 as i32 as u32,
                        t.is_finite() && (-2147483648.0..2147483648.0).contains(&t),
                    )
                } else {
                    let x = f32::from_le_bytes(
                        self.xmm_rm_sized(mem, &rm, seg, 4)?[..4]
                            .try_into()
                            .unwrap(),
                    );
                    let t = if op == 0x2C {
                        x.trunc()
                    } else {
                        x.round_ties_even()
                    };
                    (
                        t as i64 as i32 as u32,
                        t.is_finite() && (-2147483648.0..2147483648.0).contains(&t),
                    )
                };
                self.set_r(rm.reg, if valid { v } else { 0x8000_0000 }, 32);
            }
            0x2E | 0x2F => {
                // comiss/ucomiss, comisd/ucomisd
                let rm = self.modrm(mem)?;
                let s = self.xmm_rm_sized(mem, &rm, seg, if op66 { 8 } else { 4 })?;
                let (unordered, greater, less) = if op66 {
                    let a = f64::from_le_bytes(self.xmm[rm.reg][..8].try_into().unwrap());
                    let b = f64::from_le_bytes(s[..8].try_into().unwrap());
                    (a.is_nan() || b.is_nan(), a > b, a < b)
                } else {
                    let a = f32::from_le_bytes(self.xmm[rm.reg][..4].try_into().unwrap());
                    let b = f32::from_le_bytes(s[..4].try_into().unwrap());
                    (a.is_nan() || b.is_nan(), a > b, a < b)
                };
                self.set_flag(ZF, unordered || (!greater && !less));
                self.set_flag(PF, unordered);
                self.set_flag(CF, unordered || less);
                self.set_flag(OF, false);
                self.set_flag(SF, false);
                self.set_flag(AF, false);
            }
            0xC2 => {
                // Legacy CMPPS/CMPPD/CMPSS/CMPSD: exact bit comparisons,
                // including signed zero, NaN predicates, DAZ and MXCSR faults.
                if op66 && rep.is_some() {
                    return Err(self.unsupported(mem, eip0, "invalid CMP prefix"));
                }
                let rm = self.modrm(mem)?;
                let double = op66 || rep == Some(0xf2);
                let scalar = rep.is_some();
                let width = if double { 8 } else { 4 };
                let source = self.xmm_rm_sized(mem, &rm, seg, if scalar { width } else { 16 })?;
                let predicate = self.fetch8(mem)? & 7;
                let mut out = self.xmm[rm.reg];
                let mut exceptions = 0u32;
                let mantissa = if double { 52 } else { 23 };
                let exponent = if double { 0x7ffu64 } else { 0xff };
                let sign = 1u64 << (mantissa + if double { 11 } else { 8 });
                let fraction = (1u64 << mantissa) - 1;
                let classify = |raw: u64| {
                    let denormal = (raw >> mantissa) & exponent == 0 && raw & fraction != 0;
                    let raw = if denormal && self.mxcsr & 0x40 != 0 {
                        raw & sign
                    } else {
                        raw
                    };
                    let nan = (raw >> mantissa) & exponent == exponent && raw & fraction != 0;
                    let signaling = nan && raw & (1u64 << (mantissa - 1)) == 0;
                    (
                        raw,
                        nan,
                        signaling,
                        if denormal && self.mxcsr & 0x40 == 0 {
                            2
                        } else {
                            0
                        },
                    )
                };
                for i in 0..if scalar { 1 } else { 16 / width } {
                    let read = |bytes: &[u8]| {
                        if double {
                            u64::from_le_bytes(bytes.try_into().unwrap())
                        } else {
                            u32::from_le_bytes(bytes.try_into().unwrap()) as u64
                        }
                    };
                    let at = i * width;
                    let (a, an, asn, ad) = classify(read(&out[at..at + width]));
                    let (b, bn, bsn, bd) = classify(read(&source[at..at + width]));
                    exceptions |= ad | bd;
                    let unordered = an || bn;
                    if asn || bsn || (unordered && matches!(predicate, 1 | 2 | 5 | 6)) {
                        exceptions |= 1;
                    }
                    let equal = a == b || (a & !sign == 0 && b & !sign == 0);
                    let less = !equal
                        && if (a ^ b) & sign != 0 {
                            a & sign != 0
                        } else if a & sign != 0 {
                            a > b
                        } else {
                            a < b
                        };
                    let result = match predicate {
                        0 => !unordered && equal,
                        1 => !unordered && less,
                        2 => !unordered && (less || equal),
                        3 => unordered,
                        4 => unordered || !equal,
                        5 => unordered || !less,
                        6 => unordered || !(less || equal),
                        _ => !unordered,
                    };
                    out[at..at + width].fill(if result { 0xff } else { 0 });
                }
                self.mxcsr |= exceptions;
                if exceptions & !(self.mxcsr >> 7) & 0x3f != 0 {
                    return Err(CpuError::SimdFloatingPoint {
                        eip: eip0,
                        mxcsr: self.mxcsr,
                    });
                }
                self.xmm[rm.reg] = out;
            }
            0x50 => {
                // movmskps (np) / movmskpd (66)
                let rm = self.modrm(mem)?;
                let x = self.xmm[rm.idx];
                let m: u32 = if op66 {
                    ((x[15] >> 7) as u32) << 1 | (x[7] >> 7) as u32
                } else {
                    [3, 7, 11, 15]
                        .iter()
                        .enumerate()
                        .map(|(i, &o)| ((x[o] >> 7) as u32) << i)
                        .sum()
                };
                self.set_r(rm.reg, m, 32);
            }
            0x51 => {
                // sqrtps/sqrtpd/sqrtss/sqrtsd
                let rm = self.modrm(mem)?;
                let width = match rep {
                    Some(0xF3) => 4,
                    Some(0xF2) => 8,
                    _ => 16,
                };
                let s = self.xmm_rm_sized(mem, &rm, seg, width)?;
                match (op66, rep) {
                    (false, Some(0xF3)) => {
                        let x = f32::from_le_bytes(s[..4].try_into().unwrap());
                        self.xmm[rm.reg][..4].copy_from_slice(&x.sqrt().to_le_bytes());
                    }
                    (false, Some(0xF2)) => {
                        let x = f64::from_le_bytes(s[..8].try_into().unwrap());
                        self.xmm[rm.reg][..8].copy_from_slice(&x.sqrt().to_le_bytes());
                    }
                    (false, _) => {
                        self.xmm[rm.reg] = pack_f32x4(f32x4(&s).map(f32::sqrt));
                    }
                    (true, _) => {
                        self.xmm[rm.reg] = pack_f64x2(f64x2(&s).map(f64::sqrt));
                    }
                }
            }
            0x52 | 0x53 => {
                // rsqrtps/rsqrtss, rcpps/rcpss (exact 1/x, 1/sqrt(x): HW is
                // approximate, difference irrelevant for game logic)
                let rm = self.modrm(mem)?;
                let scalar = rep == Some(0xF3);
                let s = self.xmm_rm_sized(mem, &rm, seg, if scalar { 4 } else { 16 })?;
                let f = |x: f32| -> f32 {
                    if op == 0x52 {
                        1.0 / x.sqrt()
                    } else {
                        1.0 / x
                    }
                };
                if scalar {
                    let x = f32::from_le_bytes(s[..4].try_into().unwrap());
                    self.xmm[rm.reg][..4].copy_from_slice(&f(x).to_le_bytes());
                } else {
                    self.xmm[rm.reg] = pack_f32x4(f32x4(&s).map(f));
                }
            }
            0x54..=0x57 => {
                // andps/andpd, andnps/andnpd, orps/orpd, xorps/xorpd
                let rm = self.modrm(mem)?;
                let a = self.xmm[rm.reg];
                let b = self.xmm_rm(mem, &rm, seg)?;
                let mut out = [0u8; 16];
                for i in 0..16 {
                    out[i] = match op {
                        0x54 => a[i] & b[i],
                        0x55 => !a[i] & b[i],
                        0x56 => a[i] | b[i],
                        _ => a[i] ^ b[i],
                    };
                }
                self.xmm[rm.reg] = out;
            }
            0x58..=0x5F => {
                self.sse_arith(mem, seg, op, op66, rep, eip0)?;
            }
            0x60..=0x62 | 0x68..=0x6A | 0x6C | 0x6D if op66 => {
                // punpcklbw/lwd/ldq, punpckhbw/hwd/hdq, punpcklqdq/hqdq
                let rm = self.modrm(mem)?;
                let a = self.xmm[rm.reg];
                let b = self.xmm_rm(mem, &rm, seg)?;
                let hi = matches!(op, 0x68..=0x6A | 0x6D);
                let off = if hi { 8 } else { 0 };
                let mut out = [0u8; 16];
                match op {
                    0x60 | 0x68 => {
                        for i in 0..8 {
                            out[2 * i] = a[off + i];
                            out[2 * i + 1] = b[off + i];
                        }
                    }
                    0x61 | 0x69 => {
                        for i in 0..4 {
                            out[4 * i..4 * i + 2].copy_from_slice(&a[off + 2 * i..off + 2 * i + 2]);
                            out[4 * i + 2..4 * i + 4]
                                .copy_from_slice(&b[off + 2 * i..off + 2 * i + 2]);
                        }
                    }
                    0x62 | 0x6A => {
                        for i in 0..2 {
                            out[8 * i..8 * i + 4].copy_from_slice(&a[off + 4 * i..off + 4 * i + 4]);
                            out[8 * i + 4..8 * i + 8]
                                .copy_from_slice(&b[off + 4 * i..off + 4 * i + 4]);
                        }
                    }
                    _ => {
                        out[..8].copy_from_slice(&a[off..off + 8]);
                        out[8..].copy_from_slice(&b[off..off + 8]);
                    }
                }
                self.xmm[rm.reg] = out;
            }
            0x6E if op66 => {
                // movd xmm, r/m32
                let rm = self.modrm(mem)?;
                let v = self.rm_read(mem, &rm, 32, seg)?;
                let mut out = [0u8; 16];
                out[..4].copy_from_slice(&v.to_le_bytes());
                self.xmm[rm.reg] = out;
            }
            0x6F => {
                let rm = self.modrm(mem)?;
                if op66 || rep == Some(0xF3) {
                    // movdqa / movdqu (identical without alignment faults)
                    let v = self.xmm_rm(mem, &rm, seg)?;
                    self.xmm[rm.reg] = v;
                } else {
                    return Err(self.unsupported(mem, eip0, "movq mm (MMX)"));
                }
            }
            0x70 => {
                let rm = self.modrm(mem)?;
                let s = self.xmm_rm(mem, &rm, seg)?;
                let imm = self.fetch8(mem)?;
                let mut out = s;
                match (op66, rep) {
                    (true, _) => {
                        // pshufd
                        let l = lanes32(&s);
                        out = pack32([
                            l[(imm & 3) as usize],
                            l[((imm >> 2) & 3) as usize],
                            l[((imm >> 4) & 3) as usize],
                            l[((imm >> 6) & 3) as usize],
                        ]);
                    }
                    (false, Some(0xF3)) => {
                        // pshufhw: word lanes 4..8 shuffled
                        for i in 0..4 {
                            let sel = ((imm >> (2 * i)) & 3) as usize;
                            out[8 + 2 * i..8 + 2 * i + 2]
                                .copy_from_slice(&s[8 + 2 * sel..8 + 2 * sel + 2]);
                        }
                    }
                    (false, Some(0xF2)) => {
                        // pshuflw: word lanes 0..4 shuffled
                        for i in 0..4 {
                            let sel = ((imm >> (2 * i)) & 3) as usize;
                            out[2 * i..2 * i + 2].copy_from_slice(&s[2 * sel..2 * sel + 2]);
                        }
                    }
                    _ => return Err(self.unsupported(mem, eip0, "pshufw mm (MMX)")),
                }
                self.xmm[rm.reg] = out;
            }
            0x71..=0x73 if op66 => {
                // shift-by-imm groups: 71 w, 72 d, 73 q/dq
                let rm = self.modrm(mem)?;
                if rm.addr.is_some() {
                    return Err(self.unsupported(mem, eip0, "shift group with memory operand"));
                }
                let imm = self.fetch8(mem)? as u32;
                let s = self.xmm[rm.idx];
                let mut out = [0u8; 16];
                match (op, rm.reg) {
                    (0x71, 2) | (0x71, 6) | (0x71, 4) => {
                        let mut l = lanes16(&s);
                        for w in l.iter_mut() {
                            *w = match rm.reg {
                                2 => {
                                    if imm > 15 {
                                        0
                                    } else {
                                        *w >> imm
                                    }
                                }
                                6 => {
                                    if imm > 15 {
                                        0
                                    } else {
                                        w.wrapping_shl(imm)
                                    }
                                }
                                _ => ((*w as i16) >> imm.min(15)) as u16,
                            };
                        }
                        out = pack16(l);
                    }
                    (0x72, 2) | (0x72, 6) | (0x72, 4) => {
                        let mut l = lanes32(&s);
                        for d in l.iter_mut() {
                            *d = match rm.reg {
                                2 => {
                                    if imm > 31 {
                                        0
                                    } else {
                                        *d >> imm
                                    }
                                }
                                6 => {
                                    if imm > 31 {
                                        0
                                    } else {
                                        d.wrapping_shl(imm)
                                    }
                                }
                                _ => ((*d as i32) >> imm.min(31)) as u32,
                            };
                        }
                        out = pack32(l);
                    }
                    (0x73, 2) | (0x73, 6) => {
                        for half in 0..2 {
                            let q =
                                u64::from_le_bytes(s[8 * half..8 * half + 8].try_into().unwrap());
                            let r = if rm.reg == 2 {
                                if imm > 63 {
                                    0
                                } else {
                                    q >> imm
                                }
                            } else if imm > 63 {
                                0
                            } else {
                                q.wrapping_shl(imm)
                            };
                            out[8 * half..8 * half + 8].copy_from_slice(&r.to_le_bytes());
                        }
                    }
                    (0x73, 3) => {
                        // psrldq
                        let n = (imm as usize).min(16);
                        for i in 0..16 {
                            out[i] = if i + n < 16 { s[i + n] } else { 0 };
                        }
                    }
                    (0x73, 7) => {
                        // pslldq
                        let n = (imm as usize).min(16);
                        for i in 0..16 {
                            out[i] = if i >= n { s[i - n] } else { 0 };
                        }
                    }
                    _ => return Err(self.unsupported(mem, eip0, "shift group /n")),
                }
                self.xmm[rm.idx] = out;
            }
            0x74..=0x76 if op66 => {
                // pcmpeqb/w/d
                let rm = self.modrm(mem)?;
                let a = self.xmm[rm.reg];
                let b = self.xmm_rm(mem, &rm, seg)?;
                let mut out = [0u8; 16];
                match op {
                    0x74 => {
                        for i in 0..16 {
                            out[i] = if a[i] == b[i] { 0xFF } else { 0 };
                        }
                    }
                    0x75 => {
                        for i in 0..8 {
                            let eq = a[2 * i..2 * i + 2] == b[2 * i..2 * i + 2];
                            out[2 * i] = if eq { 0xFF } else { 0 };
                            out[2 * i + 1] = if eq { 0xFF } else { 0 };
                        }
                    }
                    _ => {
                        for i in 0..4 {
                            let eq = a[4 * i..4 * i + 4] == b[4 * i..4 * i + 4];
                            for j in 0..4 {
                                out[4 * i + j] = if eq { 0xFF } else { 0 };
                            }
                        }
                    }
                }
                self.xmm[rm.reg] = out;
            }
            0x7E => {
                let rm = self.modrm(mem)?;
                match (op66, rep) {
                    (false, Some(0xF3)) => {
                        // movq xmm, xmm/m64: load 8, zero upper
                        match rm.addr {
                            Some(a) => {
                                let v = mem.read_u64(a.wrapping_add(seg))?;
                                let mut out = [0u8; 16];
                                out[..8].copy_from_slice(&v.to_le_bytes());
                                self.xmm[rm.reg] = out;
                            }
                            None => {
                                let s = self.xmm[rm.idx];
                                let mut out = [0u8; 16];
                                out[..8].copy_from_slice(&s[..8]);
                                self.xmm[rm.reg] = out;
                            }
                        }
                    }
                    (true, _) => {
                        // movd r/m32, xmm
                        let v = u32::from_le_bytes(self.xmm[rm.reg][..4].try_into().unwrap());
                        match rm.addr {
                            Some(a) => mem.write_u32(a.wrapping_add(seg), v)?,
                            None => self.set_r(rm.idx, v, 32),
                        }
                    }
                    _ => return Err(self.unsupported(mem, eip0, "movd/movq with bad prefix")),
                }
            }
            0x7F => {
                let rm = self.modrm(mem)?;
                if op66 || rep == Some(0xF3) {
                    let v = self.xmm[rm.reg];
                    self.xmm_rm_write(mem, &rm, seg, v)?;
                } else {
                    return Err(self.unsupported(mem, eip0, "movq mm (MMX)"));
                }
            }
            0xC6 if rep.is_none() => {
                // SHUFPS / SHUFPD selects low lanes from the original
                // destination and high lanes from the source, including aliases.
                let rm = self.modrm(mem)?;
                let a = self.xmm[rm.reg];
                let b = self.xmm_rm(mem, &rm, seg)?;
                let imm = self.fetch8(mem)?;
                let mut out = [0; 16];
                if op66 {
                    let lo = (imm & 1) as usize * 8;
                    let hi = ((imm >> 1) & 1) as usize * 8;
                    out[..8].copy_from_slice(&a[lo..lo + 8]);
                    out[8..].copy_from_slice(&b[hi..hi + 8]);
                } else {
                    for i in 0..4 {
                        let source = if i < 2 { a } else { b };
                        let at = ((imm >> (2 * i)) & 3) as usize * 4;
                        out[i * 4..i * 4 + 4].copy_from_slice(&source[at..at + 4]);
                    }
                }
                self.xmm[rm.reg] = out;
            }
            0xC4 if op66 => {
                // pinsrw xmm, r32/m16, imm8
                let rm = self.modrm(mem)?;
                let v = self.rm_read(mem, &rm, 16, seg)? as u16;
                let imm = (self.fetch8(mem)? & 7) as usize;
                self.xmm[rm.reg][2 * imm..2 * imm + 2].copy_from_slice(&v.to_le_bytes());
            }
            0xC5 if op66 => {
                // pextrw r32, xmm, imm8
                let rm = self.modrm(mem)?;
                if rm.addr.is_some() {
                    return Err(self.unsupported(mem, eip0, "pextrw from memory"));
                }
                let imm = (self.fetch8(mem)? & 7) as usize;
                let v =
                    u16::from_le_bytes(self.xmm[rm.idx][2 * imm..2 * imm + 2].try_into().unwrap());
                self.set_r(rm.reg, v as u32, 32);
            }
            0xD4 | 0xD7 | 0xDB | 0xDF | 0xE7 | 0xEB | 0xEF | 0xF8..=0xFE if op66 => {
                let rm = self.modrm(mem)?;
                match op {
                    0xD7 => {
                        // pmovmskb r32, xmm
                        if rm.addr.is_some() {
                            return Err(self.unsupported(mem, eip0, "pmovmskb from memory"));
                        }
                        let x = self.xmm[rm.idx];
                        let m: u32 = x
                            .iter()
                            .enumerate()
                            .map(|(i, b)| ((b >> 7) as u32) << i)
                            .sum();
                        self.set_r(rm.reg, m, 32);
                    }
                    0xE7 => {
                        // movntdq: plain store here
                        let v = self.xmm[rm.reg];
                        self.xmm_rm_write(mem, &rm, seg, v)?;
                    }
                    _ => {
                        let a = self.xmm[rm.reg];
                        let b = self.xmm_rm(mem, &rm, seg)?;
                        let mut out = [0u8; 16];
                        match op {
                            0xDB | 0xDF | 0xEB | 0xEF => {
                                for i in 0..16 {
                                    out[i] = match op {
                                        0xDB => a[i] & b[i],
                                        0xDF => !a[i] & b[i],
                                        0xEB => a[i] | b[i],
                                        _ => a[i] ^ b[i],
                                    };
                                }
                            }
                            0xD4 | 0xFB => {
                                for half in 0..2 {
                                    let qa = u64::from_le_bytes(
                                        a[8 * half..8 * half + 8].try_into().unwrap(),
                                    );
                                    let qb = u64::from_le_bytes(
                                        b[8 * half..8 * half + 8].try_into().unwrap(),
                                    );
                                    let r = if op == 0xD4 {
                                        qa.wrapping_add(qb)
                                    } else {
                                        qa.wrapping_sub(qb)
                                    };
                                    out[8 * half..8 * half + 8].copy_from_slice(&r.to_le_bytes());
                                }
                            }
                            0xFC | 0xF8 => {
                                for i in 0..16 {
                                    out[i] = if op == 0xFC {
                                        a[i].wrapping_add(b[i])
                                    } else {
                                        a[i].wrapping_sub(b[i])
                                    };
                                }
                            }
                            0xFD | 0xF9 => {
                                let la = lanes16(&a);
                                let lb = lanes16(&b);
                                let mut l = [0u16; 8];
                                for i in 0..8 {
                                    l[i] = if op == 0xFD {
                                        la[i].wrapping_add(lb[i])
                                    } else {
                                        la[i].wrapping_sub(lb[i])
                                    };
                                }
                                out = pack16(l);
                            }
                            _ => {
                                // 0xFE paddd / 0xFA psubd
                                let la = lanes32(&a);
                                let lb = lanes32(&b);
                                let mut l = [0u32; 4];
                                for i in 0..4 {
                                    l[i] = if op == 0xFE {
                                        la[i].wrapping_add(lb[i])
                                    } else {
                                        la[i].wrapping_sub(lb[i])
                                    };
                                }
                                out = pack32(l);
                            }
                        }
                        self.xmm[rm.reg] = out;
                    }
                }
            }
            0xD6 if op66 => {
                // movq xmm/m64, xmm: store low 8
                let rm = self.modrm(mem)?;
                let v = u64::from_le_bytes(self.xmm[rm.reg][..8].try_into().unwrap());
                match rm.addr {
                    Some(a) => mem.write_u64(a.wrapping_add(seg), v)?,
                    None => {
                        self.xmm[rm.idx] = [0; 16];
                        self.xmm[rm.idx][..8].copy_from_slice(&v.to_le_bytes());
                    }
                }
            }
            0xAE => {
                let rm = self.modrm(mem)?;
                match rm.reg {
                    2 => {
                        // ldmxcsr m32
                        if let Some(a) = rm.addr {
                            self.mxcsr = mem.read_u32(a.wrapping_add(seg))?;
                        }
                    }
                    3 => {
                        // stmxcsr m32
                        if let Some(a) = rm.addr {
                            mem.write_u32(a.wrapping_add(seg), self.mxcsr)?;
                        }
                    }
                    5..=7 => {} // lfence/mfence/sfence: nop
                    _ => return Err(self.unsupported(mem, eip0, "0f ae group /n")),
                }
            }
            0xD1..=0xD3 | 0xE1 | 0xE2 | 0xF1..=0xF3 if op66 && rep.is_none() => {
                // PSRLW/D/Q, PSRAW/D, PSLLW/D/Q. The count is the full
                // unsigned low qword, never x86's scalar masked shift count.
                let rm = self.modrm(mem)?;
                let count_source = self.xmm_rm(mem, &rm, seg)?;
                let count = u64::from_le_bytes(count_source[..8].try_into().unwrap());
                let width = match op & 0xf {
                    1 => 2,
                    2 => 4,
                    _ => 8,
                };
                let bits = (width * 8) as u64;
                let source = self.xmm[rm.reg];
                let mut out = [0; 16];
                for (src, dst) in source.chunks_exact(width).zip(out.chunks_exact_mut(width)) {
                    let mut bytes = [0; 8];
                    bytes[..width].copy_from_slice(src);
                    let value = u64::from_le_bytes(bytes);
                    let shifted = if matches!(op, 0xE1 | 0xE2) {
                        let signed = if width == 2 {
                            value as i16 as i64
                        } else {
                            value as i32 as i64
                        };
                        (signed >> count.min(bits - 1)) as u64
                    } else if count >= bits {
                        0
                    } else if op >= 0xF1 {
                        value << count
                    } else {
                        value >> count
                    };
                    dst.copy_from_slice(&shifted.to_le_bytes()[..width]);
                }
                self.xmm[rm.reg] = out;
            }
            0xF0 if rep == Some(0xF2) => {
                // lddqu: same as movdqu without alignment faults
                let rm = self.modrm(mem)?;
                let v = self.xmm_rm(mem, &rm, seg)?;
                self.xmm[rm.reg] = v;
            }
            0xE6 => {
                // cvttpd2dq (66) / cvtpd2dq (f2) / cvtdq2pd (f3)
                let rm = self.modrm(mem)?;
                let s = self.xmm_rm_sized(mem, &rm, seg, if rep == Some(0xF3) { 8 } else { 16 })?;
                match rep {
                    Some(0xF3) => {
                        let l = lanes32(&s);
                        let mut out = [0u8; 16];
                        out[..8].copy_from_slice(&(l[0] as i32 as f64).to_le_bytes());
                        out[8..].copy_from_slice(&(l[1] as i32 as f64).to_le_bytes());
                        self.xmm[rm.reg] = out;
                    }
                    p => {
                        let x = f64x2(&s);
                        let mut l = [0u32; 4];
                        for i in 0..2 {
                            let t = if p == Some(0xF2) {
                                x[i].round_ties_even()
                            } else {
                                x[i].trunc()
                            };
                            l[i] = if t.is_finite() && (-2147483648.0..2147483648.0).contains(&t) {
                                t as i64 as i32 as u32
                            } else {
                                0x8000_0000
                            };
                        }
                        self.xmm[rm.reg] = pack32(l);
                    }
                }
            }
            _ => {
                return Err(self.unsupported(mem, eip0, &format!("opcode 0f {op:02x}")));
            }
        }
        Ok(())
    }

    // ---- SSE helpers ----

    fn xmm_rm(&self, mem: &GuestMem, rm: &Rm, seg: u32) -> Result<[u8; 16], CpuError> {
        self.xmm_rm_sized(mem, rm, seg, 16)
    }

    fn xmm_rm_sized(
        &self,
        mem: &GuestMem,
        rm: &Rm,
        seg: u32,
        width: usize,
    ) -> Result<[u8; 16], CpuError> {
        match rm.addr {
            Some(a) => {
                let mut b = [0u8; 16];
                mem.read_bytes(a.wrapping_add(seg), &mut b[..width])?;
                Ok(b)
            }
            None => Ok(self.xmm[rm.idx]),
        }
    }

    fn xmm_rm_write(
        &mut self,
        mem: &mut GuestMem,
        rm: &Rm,
        seg: u32,
        v: [u8; 16],
    ) -> Result<(), CpuError> {
        match rm.addr {
            Some(a) => mem.write_bytes(a.wrapping_add(seg), &v)?,
            None => self.xmm[rm.idx] = v,
        }
        Ok(())
    }

    /// Packed/scalar floating-point arithmetic: 0f 58..=5f with NP/66/f3/f2.
    #[allow(clippy::too_many_arguments)]
    fn sse_arith(
        &mut self,
        mem: &mut GuestMem,
        seg: u32,
        op: u8,
        op66: bool,
        rep: Option<u8>,
        eip0: u32,
    ) -> Result<(), CpuError> {
        if op == 0x5A || op == 0x5B {
            return self.sse_cvt(mem, seg, op, op66, rep, eip0);
        }
        let rm = self.modrm(mem)?;
        let a = self.xmm[rm.reg];
        let width = match rep {
            Some(0xF3) => 4,
            Some(0xF2) => 8,
            _ => 16,
        };
        let b = self.xmm_rm_sized(mem, &rm, seg, width)?;
        fn af32(op: u8, x: f32, y: f32) -> f32 {
            match op {
                0x58 => x + y,
                0x59 => x * y,
                0x5C => x - y,
                0x5E => x / y,
                // min/max: SRC1 < SRC2 ? SRC1 : SRC2 (x = old dest, y = src)
                0x5D => {
                    if x < y {
                        x
                    } else {
                        y
                    }
                }
                _ => {
                    if x > y {
                        x
                    } else {
                        y
                    }
                }
            }
        }
        fn af64(op: u8, x: f64, y: f64) -> f64 {
            match op {
                0x58 => x + y,
                0x59 => x * y,
                0x5C => x - y,
                0x5E => x / y,
                0x5D => {
                    if x < y {
                        x
                    } else {
                        y
                    }
                }
                _ => {
                    if x > y {
                        x
                    } else {
                        y
                    }
                }
            }
        }
        match (op66, rep) {
            (false, Some(0xF3)) => {
                let x = f32::from_le_bytes(a[..4].try_into().unwrap());
                let y = f32::from_le_bytes(b[..4].try_into().unwrap());
                self.xmm[rm.reg][..4].copy_from_slice(&af32(op, x, y).to_le_bytes());
            }
            (false, Some(0xF2)) => {
                let x = f64::from_le_bytes(a[..8].try_into().unwrap());
                let y = f64::from_le_bytes(b[..8].try_into().unwrap());
                self.xmm[rm.reg][..8].copy_from_slice(&af64(op, x, y).to_le_bytes());
            }
            (false, None) | (false, Some(_)) => {
                let x = f32x4(&a);
                let y = f32x4(&b);
                let mut out = [0f32; 4];
                for i in 0..4 {
                    out[i] = af32(op, x[i], y[i]);
                }
                self.xmm[rm.reg] = pack_f32x4(out);
            }
            (true, _) => {
                let x = f64x2(&a);
                let y = f64x2(&b);
                self.xmm[rm.reg] = pack_f64x2([af64(op, x[0], y[0]), af64(op, x[1], y[1])]);
            }
        }
        Ok(())
    }

    /// Conversion group: 0f 5a / 0f 5b.
    #[allow(clippy::too_many_arguments)]
    fn sse_cvt(
        &mut self,
        mem: &mut GuestMem,
        seg: u32,
        op: u8,
        op66: bool,
        rep: Option<u8>,
        eip0: u32,
    ) -> Result<(), CpuError> {
        let rm = self.modrm(mem)?;
        let width = if op == 0x5A {
            match (op66, rep) {
                (false, Some(0xF3)) => 4,
                (false, _) => 8,
                _ => 16,
            }
        } else {
            16
        };
        let s = self.xmm_rm_sized(mem, &rm, seg, width)?;
        if op == 0x5A {
            match (op66, rep) {
                (true, _) => {
                    // cvtpd2ps
                    let x = f64x2(&s);
                    let mut out = [0u8; 16];
                    out[..4].copy_from_slice(&(x[0] as f32).to_le_bytes());
                    out[4..8].copy_from_slice(&(x[1] as f32).to_le_bytes());
                    self.xmm[rm.reg] = out;
                }
                (false, Some(0xF3)) => {
                    // cvtss2sd
                    let x = f32::from_le_bytes(s[..4].try_into().unwrap());
                    self.xmm[rm.reg][..8].copy_from_slice(&(x as f64).to_le_bytes());
                }
                (false, Some(0xF2)) => {
                    // cvtsd2ss
                    let x = f64::from_le_bytes(s[..8].try_into().unwrap());
                    self.xmm[rm.reg][..4].copy_from_slice(&(x as f32).to_le_bytes());
                }
                (false, _) => {
                    // cvtps2pd
                    let x = f32x4(&s);
                    self.xmm[rm.reg] = pack_f64x2([x[0] as f64, x[1] as f64]);
                }
            }
        } else {
            match (op66, rep) {
                (true, _) | (false, Some(0xF3)) => {
                    // cvtps2dq (round-nearest) / cvttps2dq (truncate)
                    let x = f32x4(&s);
                    let mut l = [0u32; 4];
                    for i in 0..4 {
                        let t = if op66 {
                            x[i].round_ties_even()
                        } else {
                            x[i].trunc()
                        };
                        l[i] = if t.is_finite() && (-2147483648.0..2147483648.0).contains(&t) {
                            t as i64 as i32 as u32
                        } else {
                            0x8000_0000
                        };
                    }
                    self.xmm[rm.reg] = pack32(l);
                }
                (false, Some(0xF2)) => {
                    return Err(self.unsupported(mem, eip0, "0f 5b with f2 prefix"));
                }
                (false, _) => {
                    // cvtdq2ps
                    let l = lanes32(&s);
                    self.xmm[rm.reg] = pack_f32x4(l.map(|v| v as i32 as f32));
                }
            }
        }
        Ok(())
    }

    /// ISA execution capability used by the process scheduler; no host/API dispatch.
    #[inline]
    pub(crate) fn execution_batch_limit(&self, remaining: u64, slice_remaining: u64) -> u64 {
        if self.ir_cache.is_some() && !self.trace_enabled && !self.profile_opcodes {
            remaining.min(slice_remaining)
        } else {
            1
        }
    }
}

fn read_sized(mem: &GuestMem, addr: u32, size: u32) -> Result<u32, CpuError> {
    match size {
        1 => Ok(mem.read_u8(addr)? as u32),
        2 => Ok(mem.read_u16(addr)? as u32),
        _ => Ok(mem.read_u32(addr)?),
    }
}

// ---- SSE lane helpers ----

fn lanes32(v: &[u8; 16]) -> [u32; 4] {
    [
        u32::from_le_bytes(v[0..4].try_into().unwrap()),
        u32::from_le_bytes(v[4..8].try_into().unwrap()),
        u32::from_le_bytes(v[8..12].try_into().unwrap()),
        u32::from_le_bytes(v[12..16].try_into().unwrap()),
    ]
}

fn pack32(l: [u32; 4]) -> [u8; 16] {
    let mut out = [0u8; 16];
    for (i, v) in l.iter().enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&v.to_le_bytes());
    }
    out
}

fn lanes16(v: &[u8; 16]) -> [u16; 8] {
    let mut l = [0u16; 8];
    for i in 0..8 {
        l[i] = u16::from_le_bytes(v[2 * i..2 * i + 2].try_into().unwrap());
    }
    l
}

fn pack16(l: [u16; 8]) -> [u8; 16] {
    let mut out = [0u8; 16];
    for (i, v) in l.iter().enumerate() {
        out[2 * i..2 * i + 2].copy_from_slice(&v.to_le_bytes());
    }
    out
}

fn f32x4(v: &[u8; 16]) -> [f32; 4] {
    lanes32(v).map(f32::from_bits)
}

fn pack_f32x4(a: [f32; 4]) -> [u8; 16] {
    pack32(a.map(f32::to_bits))
}

fn f64x2(v: &[u8; 16]) -> [f64; 2] {
    [
        f64::from_bits(u64::from_le_bytes(v[0..8].try_into().unwrap())),
        f64::from_bits(u64::from_le_bytes(v[8..16].try_into().unwrap())),
    ]
}

fn pack_f64x2(a: [f64; 2]) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[0..8].copy_from_slice(&a[0].to_bits().to_le_bytes());
    out[8..16].copy_from_slice(&a[1].to_bits().to_le_bytes());
    out
}

fn write_sized(mem: &GuestMem, addr: u32, v: u32, size: u32) -> Result<(), CpuError> {
    match size {
        1 => mem.write_u8(addr, v as u8)?,
        2 => mem.write_u16(addr, v as u16)?,
        _ => mem.write_u32(addr, v)?,
    }
    Ok(())
}

fn size_to_bits(size: u32) -> u8 {
    match size {
        1 => 8,
        2 => 16,
        _ => 32,
    }
}

fn sign_extend(v: u32, bits: u8) -> i32 {
    match bits {
        8 => v as u8 as i8 as i32,
        16 => v as u16 as i16 as i32,
        _ => v as i32,
    }
}
