//! Fixed-record interpreter, sharing wire's guest-memory/register semantics.
use super::{mask, read_sized, sign_extend, write_sized, Cpu, CpuError, AF, CF, OF, PF, SF, ZF};
use crate::ir::{compact, Cached, Image, Op, Operand, Record, FLAGS};
use crate::mem::GuestMem;

#[derive(Clone, Copy)]
pub(super) struct PendingFlags {
    a: u32,
    b: u32,
    result: u32,
    carry: u32,
    width: u8,
    sub: u8,
    deferred: u32,
}
impl PendingFlags {
    #[inline]
    fn compute(self, want: u32) -> u32 {
        let m = mask(self.width);
        let sign = 1u32 << (self.width - 1);
        let a = self.a & m;
        let b = self.b & m;
        let r = self.result & m;
        let add = matches!(self.sub, 0 | 2 | 9);
        let subtract = matches!(self.sub, 3 | 5 | 7 | 10);
        let mut flags = 0;
        if want & CF != 0 {
            let carry = if add {
                a as u64 + b as u64 + self.carry as u64 > m as u64
            } else if subtract {
                (a as u64) < b as u64 + self.carry as u64
            } else {
                false
            };
            if carry {
                flags |= CF;
            }
        }
        if want & OF != 0
            && ((add && ((a ^ r) & (b ^ r) & sign) != 0)
                || (subtract && ((a ^ b) & (a ^ r) & sign) != 0))
        {
            flags |= OF;
        }
        if want & AF != 0 && (add || subtract) && (a ^ b ^ r) & 0x10 != 0 {
            flags |= AF;
        }
        if want & ZF != 0 && r == 0 {
            flags |= ZF;
        }
        if want & SF != 0 && r & sign != 0 {
            flags |= SF;
        }
        if want & PF != 0 && (r as u8).count_ones() & 1 == 0 {
            flags |= PF;
        }
        flags
    }
}
impl Cpu {
    pub fn install_ir(&mut self, image: Image, mem: &GuestMem) -> Result<usize, String> {
        self.flush_flags();
        let cache = crate::ir::Cache::bind(image, mem)?;
        let accepted = cache.accepted;
        self.ir_cache = Some(cache);
        Ok(accepted)
    }
    pub fn effective_eflags(&self) -> u32 {
        self.flags_for(FLAGS)
    }
    pub(super) fn flags_for(&self, want: u32) -> u32 {
        if let Some(p) = self.pending_flags {
            let deferred = p.deferred & want;
            (self.eflags & !deferred) | p.compute(deferred)
        } else {
            self.eflags
        }
    }
    pub(super) fn flush_flags(&mut self) {
        if let Some(p) = self.pending_flags.take() {
            self.eflags = (self.eflags & !p.deferred) | p.compute(p.deferred);
        }
    }
    #[inline]
    fn defer_flags(&mut self, mut flags: PendingFlags, defs: u32, eager: u32) {
        // Partial writers (INC/DEC) preserve earlier CF without forcing the
        // dead ZF/SF/PF/AF/OF formula. Full writers discard that formula.
        if let Some(previous) = self.pending_flags.take() {
            let keep = previous.deferred & !defs;
            self.eflags = (self.eflags & !keep) | previous.compute(keep);
        }
        self.eflags = (self.eflags & !eager) | flags.compute(eager);
        flags.deferred = defs & !eager;
        self.pending_flags = if flags.deferred != 0 {
            Some(flags)
        } else {
            None
        };
    }
    pub(super) fn step_cached(&mut self, mem: &mut GuestMem) -> Result<(), CpuError> {
        let record = self
            .ir_cache
            .as_ref()
            .and_then(|c| c.get(self.eip, mem, self.profile_opcodes));
        if let Some(r) = record {
            self.ir_steps += 1;
            if self.trace_enabled {
                let mut bytes = Vec::with_capacity(8);
                for i in 0..8 {
                    match mem.read_u8(self.eip.wrapping_add(i)) {
                        Ok(b) => bytes.push(b),
                        Err(_) => break,
                    }
                }
                self.trace_ring.push_back((self.eip, bytes));
                if self.trace_ring.len() > 256 {
                    self.trace_ring.pop_front();
                }
            }
            if self.profile_opcodes {
                let opcode = match &r {
                    Cached::Compact(_, opcode) => *opcode,
                    Cached::Predecoded(r) => r.opcode(),
                };
                let primary = opcode & 0xff;
                *self.op_stats.entry(primary).or_insert(0) += 1;
                if primary == 0x0f {
                    *self.op_stats.entry(0x100 | (opcode >> 16)).or_insert(0) += 1;
                }
            }
            match r {
                Cached::Compact(hot, _) => {
                    self.execute_compact(hot);
                    Ok(())
                }
                Cached::Predecoded(r) => self.execute_ir(r, mem),
            }
        } else {
            if self.ir_cache.is_some() {
                self.ir_fallbacks += 1;
            }
            self.flush_flags();
            self.step_x86(mem)
        }
    }
    fn execute_compact(&mut self, hot: compact::Hot) {
        use compact::*;
        let i = hot.inst;
        self.eip = hot.next_ip;
        let op = i.opcode();
        match op {
            NOP => {}
            MOV_I32 => self.regs[i.dst()] = self.regs[i.a()],
            CONST_I32 => self.regs[i.dst()] = i.aux,
            JMP_I32 => self.eip = i.aux,
            JCC_BASE..=47 => {
                if self.cc(op - JCC_BASE) {
                    self.eip = i.aux;
                }
            }
            ADD_I32..=DEC_I32 => {
                let a = self.regs[i.a()];
                let b = if op >= INC_I32 || op & 1 != 0 {
                    i.aux
                } else {
                    self.regs[i.b()]
                };
                let carry = if matches!(op, ADC_I32 | ADC_IMM_I32 | SBB_I32 | SBB_IMM_I32) {
                    self.flag(CF) as u32
                } else {
                    0
                };
                let result = match op {
                    ADD_I32 | ADD_IMM_I32 | ADC_I32 | ADC_IMM_I32 | INC_I32 => {
                        a.wrapping_add(b).wrapping_add(carry)
                    }
                    OR_I32 | OR_IMM_I32 => a | b,
                    SBB_I32 | SBB_IMM_I32 | SUB_I32 | SUB_IMM_I32 | CMP_I32 | CMP_IMM_I32
                    | DEC_I32 => a.wrapping_sub(b).wrapping_sub(carry),
                    AND_I32 | AND_IMM_I32 | TEST_I32 | TEST_IMM_I32 => a & b,
                    XOR_I32 | XOR_IMM_I32 => a ^ b,
                    _ => unreachable!(),
                };
                let sub = if op >= INC_I32 {
                    9 + op - INC_I32
                } else {
                    (op - ADD_I32) / 2
                };
                let defs = if op >= INC_I32 { FLAGS & !CF } else { FLAGS };
                self.defer_flags(
                    PendingFlags {
                        a,
                        b,
                        result,
                        carry,
                        width: 32,
                        sub,
                        deferred: 0,
                    },
                    defs,
                    i.eager_flags(),
                );
                if !matches!(op, CMP_I32 | CMP_IMM_I32 | TEST_I32 | TEST_IMM_I32) {
                    self.regs[i.dst()] = result;
                }
            }
            _ => unreachable!("compact instructions are validated before binding"),
        }
    }
    fn ir_address(&self, o: Operand, segment: bool) -> u32 {
        let base = if o.base() == 15 {
            0
        } else {
            self.regs[o.base()]
        };
        let index = if o.index() == 15 {
            0
        } else {
            self.regs[o.index()].wrapping_shl(o.scale())
        };
        let seg = if segment {
            match o.segment() {
                1 => self.fs_base,
                2 => self.gs_base,
                _ => 0,
            }
        } else {
            0
        };
        base.wrapping_add(index)
            .wrapping_add(o.value)
            .wrapping_add(seg)
    }
    fn ir_read(&self, o: Operand, mem: &GuestMem) -> Result<u32, CpuError> {
        Ok(match o.kind() {
            1 => self.get_r(o.base(), o.width()),
            2 => o.value,
            3 => read_sized(mem, self.ir_address(o, true), o.width() as u32 / 8)?,
            _ => 0,
        })
    }
    fn ir_write(&mut self, o: Operand, value: u32, mem: &GuestMem) -> Result<(), CpuError> {
        match o.kind() {
            1 => self.set_r(o.base(), value, o.width()),
            3 => write_sized(mem, self.ir_address(o, true), value, o.width() as u32 / 8)?,
            _ => {
                return Err(CpuError::Unsupported {
                    eip: self.eip,
                    desc: "invalid IR destination".into(),
                })
            }
        };
        Ok(())
    }
    fn execute_ir(&mut self, r: Record, mem: &mut GuestMem) -> Result<(), CpuError> {
        self.eip = r.next_ip();
        let a = r.operand(0);
        let b = r.operand(1);
        let width = r.width();
        match r.op() {
            Op::Nop => {}
            Op::Mov | Op::Movzx | Op::Movsx => {
                let mut value = self.ir_read(b, mem)?;
                if r.op() == Op::Movsx {
                    value = sign_extend(value, b.width()) as u32;
                }
                self.ir_write(a, value, mem)?;
            }
            Op::Lea => {
                let value = self.ir_address(b, false);
                self.ir_write(a, value, mem)?;
            }
            Op::Alu => {
                let left = self.ir_read(a, mem)?;
                let right = if r.sub() >= 9 {
                    1
                } else {
                    self.ir_read(b, mem)?
                };
                let carry = if matches!(r.sub(), 2 | 3) {
                    self.flag(CF) as u32
                } else {
                    0
                };
                let result = match r.sub() {
                    0 | 2 | 9 => left.wrapping_add(right).wrapping_add(carry),
                    1 => left | right,
                    3 | 5 | 7 | 10 => left.wrapping_sub(right).wrapping_sub(carry),
                    4 | 8 => left & right,
                    6 => left ^ right,
                    _ => unreachable!(),
                } & mask(width);
                self.defer_flags(
                    PendingFlags {
                        a: left,
                        b: right,
                        result,
                        carry,
                        width,
                        sub: r.sub(),
                        deferred: 0,
                    },
                    r.defs(),
                    r.eager_flags(),
                );
                if !matches!(r.sub(), 7 | 8) {
                    self.ir_write(a, result, mem)?;
                }
            }
            Op::Push => {
                let value = self.ir_read(a, mem)?;
                self.push_sized(mem, value, width)?;
            }
            Op::Pop => {
                let value = self.pop_sized(mem, width)?;
                self.ir_write(a, value, mem)?;
            }
            Op::Jmp => self.eip = self.ir_read(a, mem)?,
            Op::Jcc => {
                if self.cc(r.sub()) {
                    self.eip = a.value;
                }
            }
            Op::Call | Op::CallNative => {
                let target = self.ir_read(a, mem)?;
                self.flush_flags();
                self.push32(mem, r.next_ip())?;
                self.eip = target;
                if r.op() == Op::CallNative {
                    self.ir_native_calls += 1;
                }
            }
            Op::Ret => {
                self.eip = self.pop32(mem)?;
                self.set_esp(self.esp().wrapping_add(a.value));
            }
            Op::Leave => {
                self.set_esp(self.regs[super::EBP]);
                self.regs[super::EBP] = self.pop32(mem)?;
            }
            Op::Setcc => {
                let value = self.cc(r.sub()) as u32;
                self.ir_write(a, value, mem)?;
            }
            Op::Cmov => {
                let value = self.ir_read(b, mem)?;
                if self.cc(r.sub()) {
                    self.ir_write(a, value, mem)?;
                }
            }
            Op::Fallback => unreachable!("fallback records are not cached"),
        }
        Ok(())
    }
}
