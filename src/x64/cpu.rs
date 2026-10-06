//! Integer-only AMD64 bring-up interpreter. Unsupported instructions fail at RIP.
//! All flags are computed eagerly; the PE32 FLAGS optimizer is not reused here.
use super::memory::Memory;
use iced_x86::{
    Decoder, DecoderError, DecoderOptions, Instruction, Mnemonic as M, OpKind, Register as R,
};
const CF: u64 = 1;
const PF: u64 = 4;
const AF: u64 = 16;
const ZF: u64 = 64;
const SF: u64 = 128;
const OF: u64 = 2048;
const ARITH: u64 = CF | PF | AF | ZF | SF | OF;
#[derive(Clone, Debug)]
pub struct Cpu64 {
    pub regs: [u64; 16], // RAX,RCX,RDX,RBX,RSP,RBP,RSI,RDI,R8..R15
    pub rip: u64,
    pub rflags: u64,
    pub gs_base: u64,
    pub retired: u64,
}
fn reg(r: R) -> Result<(usize, u32, u32), String> {
    let full = r.full_register();
    let index = match full {
        R::RAX => 0,
        R::RCX => 1,
        R::RDX => 2,
        R::RBX => 3,
        R::RSP => 4,
        R::RBP => 5,
        R::RSI => 6,
        R::RDI => 7,
        R::R8 => 8,
        R::R9 => 9,
        R::R10 => 10,
        R::R11 => 11,
        R::R12 => 12,
        R::R13 => 13,
        R::R14 => 14,
        R::R15 => 15,
        _ => return Err(format!("unsupported AMD64 register {r:?}")),
    };
    Ok((
        index,
        (r.size() * 8) as u32,
        if matches!(r, R::AH | R::BH | R::CH | R::DH) {
            8
        } else {
            0
        },
    ))
}
fn mask(bits: u32) -> u64 {
    if bits == 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    }
}
impl Cpu64 {
    pub fn new(rip: u64, rsp: u64) -> Self {
        let mut regs = [0; 16];
        regs[4] = rsp;
        Self {
            regs,
            rip,
            rflags: 0x202,
            gs_base: 0,
            retired: 0,
        }
    }
    fn get(&self, r: R) -> Result<u64, String> {
        if r == R::None {
            return Ok(0);
        }
        let (i, b, s) = reg(r)?;
        Ok((self.regs[i] >> s) & mask(b))
    }
    fn set(&mut self, r: R, value: u64) -> Result<(), String> {
        let (i, b, s) = reg(r)?;
        let m = mask(b);
        if b >= 32 {
            self.regs[i] = value & m;
        } else {
            self.regs[i] = (self.regs[i] & !(m << s)) | ((value & m) << s);
        }
        Ok(())
    }
    fn address(&self, i: &Instruction) -> Result<u64, String> {
        let mut at = if i.is_ip_rel_memory_operand() {
            i.ip_rel_memory_address()
        } else {
            self.get(i.memory_base())?
                .wrapping_add(
                    self.get(i.memory_index())?
                        .wrapping_mul(i.memory_index_scale() as u64),
                )
                .wrapping_add(i.memory_displacement64())
        };
        if matches!(
            i.memory_base(),
            R::EAX
                | R::ECX
                | R::EDX
                | R::EBX
                | R::ESP
                | R::EBP
                | R::ESI
                | R::EDI
                | R::R8D
                | R::R9D
                | R::R10D
                | R::R11D
                | R::R12D
                | R::R13D
                | R::R14D
                | R::R15D
        ) {
            at = at as u32 as u64;
        }
        match if i.mnemonic() == M::Lea {
            R::None
        } else {
            i.segment_prefix()
        } {
            R::GS => at = at.wrapping_add(self.gs_base),
            R::FS => return Err("AMD64 FS base not implemented".into()),
            _ => {}
        }
        Ok(at)
    }
    fn width(i: &Instruction, op: u32) -> Result<u32, String> {
        match i.op_kind(op) {
            OpKind::Register => Ok((i.op_register(op).size() * 8) as u32),
            OpKind::Memory => Ok((i.memory_size().size() * 8) as u32),
            _ => Err("operand has no scalar width".into()),
        }
    }
    fn read(&self, i: &Instruction, op: u32, mem: &Memory) -> Result<u64, String> {
        Ok(match i.op_kind(op) {
            OpKind::Register => self.get(i.op_register(op))?,
            OpKind::Memory => mem.read_int(self.address(i)?, i.memory_size().size())?,
            OpKind::Immediate8 => i.immediate8() as u64,
            OpKind::Immediate16 => i.immediate16() as u64,
            OpKind::Immediate32 => i.immediate32() as u64,
            OpKind::Immediate64 => i.immediate64(),
            OpKind::Immediate8to16 => i.immediate8to16() as u16 as u64,
            OpKind::Immediate8to32 => i.immediate8to32() as u32 as u64,
            OpKind::Immediate8to64 => i.immediate8to64() as u64,
            OpKind::Immediate32to64 => i.immediate32to64() as u64,
            OpKind::NearBranch64 => i.near_branch64(),
            _ => return Err(format!("unsupported AMD64 operand {:?}", i.op_kind(op))),
        })
    }
    fn write(
        &mut self,
        i: &Instruction,
        op: u32,
        value: u64,
        mem: &mut Memory,
    ) -> Result<(), String> {
        match i.op_kind(op) {
            OpKind::Register => self.set(i.op_register(op), value),
            OpKind::Memory => mem.write_int(self.address(i)?, i.memory_size().size(), value),
            _ => Err("invalid AMD64 destination".into()),
        }
    }
    fn flags(&mut self, a: u64, b: u64, result: u64, bits: u32, sub: bool, carry: bool) {
        let m = mask(bits);
        let sign = 1u64 << (bits - 1);
        let a = a & m;
        let b = b & m;
        let r = result & m;
        let c = if sub {
            (a as u128) < b as u128 + carry as u128
        } else {
            a as u128 + b as u128 + carry as u128 > m as u128
        };
        let o = if sub {
            ((a ^ b) & (a ^ r) & sign) != 0
        } else {
            (!(a ^ b) & (a ^ r) & sign) != 0
        };
        self.rflags = (self.rflags & !ARITH)
            | c as u64
            | if (r as u8).count_ones().is_multiple_of(2) {
                PF
            } else {
                0
            }
            | if (a ^ b ^ r) & 16 != 0 { AF } else { 0 }
            | if r == 0 { ZF } else { 0 }
            | if r & sign != 0 { SF } else { 0 }
            | if o { OF } else { 0 };
    }
    fn condition(&self, m: M) -> Option<bool> {
        let c = self.rflags & CF != 0;
        let z = self.rflags & ZF != 0;
        let s = self.rflags & SF != 0;
        let o = self.rflags & OF != 0;
        let p = self.rflags & PF != 0;
        Some(match m {
            M::Je => z,
            M::Jne => !z,
            M::Ja => !c && !z,
            M::Jae => !c,
            M::Jb => c,
            M::Jbe => c || z,
            M::Jg => !z && s == o,
            M::Jge => s == o,
            M::Jl => s != o,
            M::Jle => z || s != o,
            M::Jo => o,
            M::Jno => !o,
            M::Js => s,
            M::Jns => !s,
            M::Jp => p,
            M::Jnp => !p,
            _ => return None,
        })
    }
    pub fn decode(&self, mem: &Memory) -> Result<Instruction, String> {
        let mut bytes = [0; 15];
        for n in 1..=15 {
            mem.fetch(
                self.rip.checked_add(n as u64 - 1).ok_or("RIP overflow")?,
                &mut bytes[n - 1..n],
            )?;
            let mut decoder = Decoder::with_ip(64, &bytes[..n], self.rip, DecoderOptions::NONE);
            let i = decoder.decode();
            if !i.is_invalid() {
                return Ok(i);
            }
            if decoder.last_error() != DecoderError::NoMoreBytes {
                return Err(format!("invalid AMD64 instruction at {:#x}", self.rip));
            }
        }
        Err("AMD64 instruction exceeds 15 bytes".into())
    }
    pub fn step(&mut self, mem: &mut Memory) -> Result<(), String> {
        let i = self.decode(mem)?;
        if i.has_lock_prefix() || i.has_rep_prefix() || i.has_repne_prefix() {
            return Err(format!("unsupported AMD64 prefix at {:#x}", self.rip));
        }
        let mut next = self.clone();
        next.rip = i.next_ip();
        next.execute(&i, mem)
            .map_err(|e| format!("{:#x} {:?}: {e}", self.rip, i.mnemonic()))?;
        next.retired += 1;
        *self = next;
        Ok(())
    }
    fn execute(&mut self, i: &Instruction, mem: &mut Memory) -> Result<(), String> {
        if let Some(taken) = self.condition(i.mnemonic()) {
            if taken {
                self.rip = i.near_branch64();
            }
            return Ok(());
        }
        match i.mnemonic() {
            M::Nop | M::Endbr64 => {}
            M::Mov => {
                let value = self.read(i, 1, mem)?;
                self.write(i, 0, value, mem)?;
            }
            M::Movzx | M::Movsx | M::Movsxd => {
                let b = Self::width(i, 1)?;
                let mut value = self.read(i, 1, mem)?;
                if i.mnemonic() != M::Movzx && value & (1u64 << (b - 1)) != 0 {
                    value |= !mask(b);
                }
                self.write(i, 0, value, mem)?;
            }
            M::Lea => {
                let address = self.address(i)?;
                self.write(i, 0, address, mem)?;
            }
            M::Add | M::Adc | M::Sub | M::Sbb | M::Cmp | M::And | M::Or | M::Xor | M::Test => {
                let bits = Self::width(i, 0)?;
                let a = self.read(i, 0, mem)?;
                let b = self.read(i, 1, mem)?;
                let carry = matches!(i.mnemonic(), M::Adc | M::Sbb) && self.rflags & CF != 0;
                let sub = matches!(i.mnemonic(), M::Sub | M::Sbb | M::Cmp);
                let result = match i.mnemonic() {
                    M::And | M::Test => a & b,
                    M::Or => a | b,
                    M::Xor => a ^ b,
                    _ => {
                        if sub {
                            a.wrapping_sub(b).wrapping_sub(carry as u64)
                        } else {
                            a.wrapping_add(b).wrapping_add(carry as u64)
                        }
                    }
                };
                if matches!(i.mnemonic(), M::And | M::Or | M::Xor | M::Test) {
                    self.flags(result, 0, result, bits, false, false);
                    self.rflags &= !(CF | OF | AF);
                } else {
                    self.flags(a, b, result, bits, sub, carry);
                }
                if !matches!(i.mnemonic(), M::Cmp | M::Test) {
                    self.write(i, 0, result, mem)?;
                }
            }
            M::Inc | M::Dec | M::Neg | M::Not => {
                let a = self.read(i, 0, mem)?;
                let bits = Self::width(i, 0)?;
                let result = match i.mnemonic() {
                    M::Inc => a.wrapping_add(1),
                    M::Dec => a.wrapping_sub(1),
                    M::Neg => 0u64.wrapping_sub(a),
                    _ => !a,
                };
                if i.mnemonic() != M::Not {
                    let old_cf = self.rflags & CF;
                    if i.mnemonic() == M::Neg {
                        self.flags(0, a, result, bits, true, false);
                    } else {
                        self.flags(a, 1, result, bits, i.mnemonic() == M::Dec, false);
                        self.rflags = (self.rflags & !CF) | old_cf;
                    }
                }
                self.write(i, 0, result, mem)?;
            }
            M::Push | M::Call => {
                let value = if i.mnemonic() == M::Call {
                    i.next_ip()
                } else {
                    self.read(i, 0, mem)?
                };
                let target = if i.mnemonic() == M::Call {
                    Some(self.read(i, 0, mem)?)
                } else {
                    None
                };
                let width = if i.stack_pointer_increment() == -2 {
                    2
                } else {
                    8
                };
                let rsp = self.regs[4]
                    .checked_sub(width as u64)
                    .ok_or("AMD64 stack underflow")?;
                mem.write_int(rsp, width, value)?;
                self.regs[4] = rsp;
                if let Some(target) = target {
                    self.rip = target;
                }
            }
            M::Pop => {
                let width = if Self::width(i, 0)? == 16 { 2 } else { 8 };
                let value = mem.read_int(self.regs[4], width)?;
                self.regs[4] = self.regs[4]
                    .checked_add(width as u64)
                    .ok_or("AMD64 stack overflow")?;
                self.write(i, 0, value, mem)?;
            }
            M::Ret => {
                let target = mem.read_int(self.regs[4], 8)?;
                self.regs[4] = self.regs[4]
                    .checked_add(
                        8 + if i.op_count() == 0 {
                            0
                        } else {
                            i.immediate16() as u64
                        },
                    )
                    .ok_or("AMD64 stack overflow")?;
                self.rip = target;
            }
            M::Jmp => self.rip = self.read(i, 0, mem)?,
            M::Clc => self.rflags &= !CF,
            M::Stc => self.rflags |= CF,
            M::Cmc => self.rflags ^= CF,
            M::Cld => self.rflags &= !1024,
            M::Std => self.rflags |= 1024,
            _ => return Err("not implemented in AMD64 bring-up interpreter".into()),
        }
        Ok(())
    }
    /// Windows x64 integer/pointer arguments: RCX,RDX,R8,R9, then RSP+40.
    pub fn arguments(&self, mem: &Memory, count: usize) -> Result<Vec<u64>, String> {
        if count > 16 {
            return Err("AMD64 gateway argument limit".into());
        }
        (0..count)
            .map(|i| {
                if i < 4 {
                    Ok(self.regs[[1, 2, 8, 9][i]])
                } else {
                    mem.read_int(
                        self.regs[4]
                            .checked_add(40 + (i as u64 - 4) * 8)
                            .ok_or("AMD64 argument overflow")?,
                        8,
                    )
                }
            })
            .collect()
    }
    /// Callee pops only return address; caller owns shadow space/stack arguments.
    pub fn return_integer(&mut self, mem: &Memory, value: u64) -> Result<(), String> {
        let target = mem.read_int(self.regs[4], 8)?;
        let rsp = self.regs[4].checked_add(8).ok_or("AMD64 return overflow")?;
        self.regs[0] = value;
        self.regs[4] = rsp;
        self.rip = target;
        Ok(())
    }
}
