//! Fixed-record interpreter, sharing wire's guest-memory/register semantics.
use super::{mask, read_sized, sign_extend, write_sized, Cpu, CpuError, AF, CF, OF, PF, SF, ZF};
use crate::x86::ir::{compact, Image, Op, Operand, Record, FLAGS};
use crate::x86::memory::GuestMem;

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
    /// Diagnostic counts are collected only with opcode profiling enabled.
    pub fn cold_ir_profile(&self) -> Vec<(String, u64)> {
        let operand_name = |kind| match kind {
            1 => "reg",
            2 => "imm",
            3 => "mem",
            _ => "none",
        };
        self.ir_cold_stats
            .iter()
            .map(|(&(op, sub, width, a, b), &count)| {
                (
                    format!(
                        "{op:?}/{sub} I{width} {},{}",
                        operand_name(a),
                        operand_name(b)
                    ),
                    count,
                )
            })
            .collect()
    }
    pub fn install_ir(&mut self, image: Image, mem: &GuestMem) -> Result<usize, String> {
        self.flush_flags();
        let cache = crate::x86::ir::Cache::bind(image, mem)?;
        let accepted = cache.accepted;
        self.ir_cache = Some(cache);
        self.ir_modules.clear();
        self.ir_hint = (0, 0);
        Ok(accepted)
    }
    /// Add an independently guarded module cache without spanning the gaps
    /// between PE images. No code is decoded or generated during binding.
    pub fn add_module_ir(&mut self, image: Image, mem: &GuestMem) -> Result<usize, String> {
        self.flush_flags();
        let cache = crate::x86::ir::Cache::bind(image, mem)?;
        let accepted = cache.accepted;
        if accepted == 0 {
            return Ok(0);
        }
        let base = cache.base();
        self.ir_modules.retain(|c| c.base() != base);
        if self.ir_cache.as_ref().is_none_or(|c| c.base() == base) {
            self.ir_cache = Some(cache);
            self.ir_hint = (0, 0);
        } else {
            if self.ir_modules.len() >= 63 {
                return Err("module IR cache limit reached".into());
            }
            self.ir_modules.push(cache);
        }
        Ok(accepted)
    }
    pub(crate) fn uses_ir(&self) -> bool {
        self.ir_cache.is_some()
    }
    pub(crate) fn remove_module_ir(&mut self, base: u32, size: u32) {
        let matches = |c: &crate::x86::ir::Cache| {
            c.base() >= base && (c.base() as u64) < base as u64 + size as u64
        };
        self.ir_modules.retain(|c| !matches(c));
        if self.ir_cache.as_ref().is_some_and(matches) {
            self.flush_flags();
            self.ir_cache = self.ir_modules.pop();
            self.ir_hint = (0, 0);
        }
    }
    fn select_module_ir(&mut self) -> bool {
        if self.ir_cache.as_ref().is_some_and(|c| c.covers(self.eip)) {
            return false;
        }
        if let Some(i) = self.ir_modules.iter().position(|c| c.covers(self.eip)) {
            let next = self.ir_modules.swap_remove(i);
            if let Some(previous) = self.ir_cache.replace(next) {
                self.ir_modules.push(previous);
            }
            self.ir_hint = (0, 0);
            return true;
        }
        false
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
    pub(crate) fn flush_flags(&mut self) {
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
            if keep != 0 {
                self.eflags = (self.eflags & !keep) | previous.compute(keep);
            }
        }
        if eager != 0 {
            self.eflags = (self.eflags & !eager) | flags.compute(eager);
        }
        flags.deferred = defs & !eager;
        self.pending_flags = if flags.deferred != 0 {
            Some(flags)
        } else {
            None
        };
    }
    pub(super) fn step_cached(&mut self, mem: &mut GuestMem) -> Result<(), CpuError> {
        self.steps_cached(mem, 1, None).1
    }
    /// Chain typed operations across scalar branches, retaining a guest
    /// boundary after every operation. Budgets and cooperative timeslices
    /// bound the chain; observers use a one-instruction limit. A callback's
    /// stack ceiling is checked after each operation, before the next fetch.
    /// Cold/fallback operations and API targets return to the outer dispatcher.
    #[cfg_attr(
        target_os = "macos",
        link_section = "__TEXT,__wire_ir32,regular,pure_instructions"
    )]
    pub(crate) fn steps_cached(
        &mut self,
        mem: &mut GuestMem,
        limit: u64,
        stack_ceiling: Option<u32>,
    ) -> (u64, Result<(), CpuError>) {
        self.select_module_ir();
        let mut retired = 0;
        // No API/callback runs within a chain. Keep lookup state local and
        // publish the hint/count at its exit, including a faulting attempt.
        let mut hint = self.ir_hint;
        loop {
            let record = self
                .ir_cache
                .as_ref()
                .and_then(|c| c.get(self.eip, mem, self.profile_opcodes, hint));
            if let Some(mut r) = record {
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
                    let opcode = r.opcode;
                    let primary = opcode & 0xff;
                    *self.op_stats.entry(primary).or_insert(0) += 1;
                    if primary == 0x0f {
                        *self.op_stats.entry(0x100 | (opcode >> 16)).or_insert(0) += 1;
                    }
                }
                let mut op = r.hot.inst.opcode();
                let mut executed = 1;
                let scalar_len = r.hot.scalar_len();
                let result = if scalar_len > 1
                    && scalar_len as u64 <= limit - retired
                    && !self.trace_enabled
                    && !self.profile_opcodes
                {
                    // Entry get() validated the page shared by the whole run.
                    // No memory, host action, ESP write or intermediate branch
                    // can change code or expose an interior boundary. If the
                    // budget ends inside, use normal per-guest dispatch instead.
                    if r.hot.mixed_run() {
                        r.hot = self.execute_mixed_run(mem, r.index, scalar_len);
                    } else {
                        for index in r.index..r.index + scalar_len {
                            r.hot = self.ir_cache.as_ref().unwrap().scalar_hot(index);
                            self.execute_scalar(r.hot, true);
                        }
                    }
                    executed = scalar_len as u64;
                    self.ir_scalar_runs += 1;
                    self.ir_scalar_steps += executed;
                    op = r.hot.inst.opcode();
                    Ok(())
                } else if op == 134 {
                    let descriptor = self
                        .ir_cache
                        .as_ref()
                        .unwrap()
                        .simd(r.hot.inst.aux as usize);
                    let pc = self.eip;
                    self.eip = r.hot.next_ip;
                    descriptor.execute(self, mem, pc)
                } else if op != 0 {
                    if self.profile_opcodes {
                        self.compact_op_stats[op as usize] += 1;
                    }
                    self.execute_compact(r.hot, mem)
                } else {
                    let record = self.ir_cache.as_ref().unwrap().predecoded(r.index, mem);
                    if self.profile_opcodes {
                        let key = (
                            record.op(),
                            record.sub(),
                            record.width(),
                            record.operand(0).kind(),
                            record.operand(1).kind(),
                        );
                        *self.ir_cold_stats.entry(key).or_insert(0) += 1;
                    }
                    self.execute_ir(record, mem)
                };
                if let Err(error) = result {
                    self.ir_steps += retired + 1;
                    self.ir_hint = (0, 0);
                    return (retired, Err(error));
                }
                hint = if self.eip == r.hot.next_ip {
                    (r.hot.next_ip, r.hot.next_slot())
                } else if matches!(op, compact::JCC_BASE..=compact::JMP_I32)
                    && self.eip == r.hot.inst.aux
                {
                    (self.eip, r.hot.inst.branch_slot())
                } else {
                    (0, 0)
                };
                retired += executed;
                // API stubs and callback sentinels stay in the outer run loop.
                // A branch itself has no host action. The next get() still
                // validates changed code and rejects unrecognized entries.
                if retired >= limit
                    || op == 0
                    || self.eip >= crate::x86::address_space::STUB_BASE
                    || stack_ceiling.is_some_and(|ceiling| self.esp() >= ceiling)
                {
                    self.ir_steps += retired;
                    self.ir_hint = hint;
                    return (retired, Ok(()));
                }
            } else {
                if !self.ir_modules.is_empty() && self.select_module_ir() {
                    hint = (0, 0);
                    continue;
                }
                self.ir_steps += retired;
                if self.ir_cache.is_some() {
                    self.ir_fallbacks += 1;
                }
                self.flush_flags();
                self.ir_hint = (0, 0);
                let result = self.step_x86(mem);
                return (retired + u64::from(result.is_ok()), result);
            }
        }
    }
    #[inline(always)]
    fn execute_compact(&mut self, hot: compact::Hot, mem: &GuestMem) -> Result<(), CpuError> {
        use compact::*;
        let i = hot.inst;
        self.eip = hot.next_ip;
        let op = i.opcode();
        match op {
            NOP..=DEC_I32 | JCC_BASE..=JMP_I32 | LEA_I32 | ALU_GPR8_BASE..=MOV_S16_I32 => {
                self.execute_scalar(hot, false);
            }
            LOAD_I32
            | STORE_I32
            | LOAD_GPR8..=LOAD_S16_I32
            | ALU_REG_MEM_I32_BASE..=ALU_MEM_REG_I32_END => {
                let base = if i.has_base() {
                    self.regs[i.gpr_a()]
                } else {
                    0
                };
                let index = if i.has_index() {
                    self.regs[i.gpr_b()].wrapping_shl(i.address_scale())
                } else {
                    0
                };
                let segment = match i.address_segment() {
                    1 => self.fs_base,
                    2 => self.gs_base,
                    _ => 0,
                };
                let address = base
                    .wrapping_add(index)
                    .wrapping_add(i.aux)
                    .wrapping_add(segment);
                // All guest range/commit checks and code-write tracking stay in
                // GuestMem. Only x86 operand decoding/record copies disappear.
                match op {
                    LOAD_I32 => self.regs[i.gpr_dst()] = mem.read_u32(address)?,
                    STORE_I32 => mem.write_u32(address, self.regs[i.gpr_dst()])?,
                    LOAD_GPR8 => self.set_r8(i.gpr_dst(), mem.read_u8(address)?),
                    LOAD_GPR16 => self.set_r(i.gpr_dst(), mem.read_u16(address)? as u32, 16),
                    STORE_GPR8 => mem.write_u8(address, self.r8(i.gpr_dst()))?,
                    STORE_GPR16 => mem.write_u16(address, self.regs[i.gpr_dst()] as u16)?,
                    LOAD_U8_I32 => self.regs[i.gpr_dst()] = mem.read_u8(address)? as u32,
                    LOAD_U16_I32 => self.regs[i.gpr_dst()] = mem.read_u16(address)? as u32,
                    LOAD_S8_I32 => {
                        self.regs[i.gpr_dst()] = mem.read_u8(address)? as i8 as i32 as u32
                    }
                    LOAD_S16_I32 => {
                        self.regs[i.gpr_dst()] = mem.read_u16(address)? as i16 as i32 as u32
                    }
                    ALU_REG_MEM_I32_BASE..=ALU_MEM_REG_I32_END => {
                        let memory = mem.read_u32(address)?;
                        let register = self.regs[i.gpr_dst()];
                        let (a, b, sub) = if op < ALU_MEM_REG_I32_BASE {
                            (register, memory, op - ALU_REG_MEM_I32_BASE)
                        } else {
                            (memory, register, op - ALU_MEM_REG_I32_BASE)
                        };
                        let result = self.compact_alu(a, b, sub, 32, FLAGS);
                        if !matches!(sub, 7 | 8) {
                            if op < ALU_MEM_REG_I32_BASE {
                                self.regs[i.gpr_dst()] = result;
                            } else {
                                // Match the raw/cold ordering: eager FLAGS are
                                // visible even if the checked store faults.
                                mem.write_u32(address, result)?;
                            }
                        }
                    }
                    _ => unreachable!(),
                }
            }
            PUSH_I32 => self.push32(mem, self.regs[i.gpr_a()])?,
            PUSH_CONST_I32 => self.push32(mem, i.aux)?,
            POP_I32 => self.regs[i.gpr_dst()] = self.pop32(mem)?,
            CALL_CONST_I32 | CALL_I32 => {
                let target = if op == CALL_CONST_I32 {
                    i.aux
                } else {
                    self.regs[i.gpr_a()]
                };
                self.flush_flags();
                self.push32(mem, hot.next_ip)?;
                self.eip = target;
            }
            JMP_REG_I32 => self.eip = self.regs[i.gpr_a()],
            RET_I32 => {
                self.eip = self.pop32(mem)?;
                self.set_esp(self.esp().wrapping_add(i.aux));
            }
            LEAVE_I32 => {
                self.set_esp(self.regs[super::EBP]);
                self.regs[super::EBP] = self.pop32(mem)?;
            }
            _ => unreachable!("compact instructions are validated before binding"),
        }
        Ok(())
    }
    /// Register-only execution shared by ordinary dispatch and static runs.
    /// All FLAGS recipes remain complete, including at a branch or split.
    // Preserve the original tight integer loop. The verified mixed path has
    // a separate cold call boundary and reuses the same semantic operations.
    #[inline(never)]
    #[cfg_attr(
        target_os = "macos",
        link_section = "__TEXT,__wire_mix32,regular,pure_instructions"
    )]
    fn execute_mixed_run(&mut self, mem: &GuestMem, index: usize, len: usize) -> compact::Hot {
        let mut last = self.ir_cache.as_ref().unwrap().scalar_hot(index);
        for index in index..index + len {
            last = self.ir_cache.as_ref().unwrap().scalar_hot(index);
            if last.inst.opcode() == 134 {
                let descriptor = self.ir_cache.as_ref().unwrap().simd(last.inst.aux as usize);
                let pc = self.eip;
                self.eip = last.next_ip;
                descriptor
                    .execute(self, mem, pc)
                    .expect("verified register SSE cannot fault");
            } else {
                self.execute_scalar(last, true);
            }
        }
        last
    }
    #[inline(always)]
    fn execute_scalar(&mut self, hot: compact::Hot, lazy: bool) {
        use compact::*;
        let i = hot.inst;
        let op = i.opcode();
        self.eip = hot.next_ip;
        match op {
            NOP => {}
            MOV_I32 => self.regs[i.gpr_dst()] = self.regs[i.gpr_a()],
            CONST_I32 => self.regs[i.gpr_dst()] = i.aux,
            MOV_GPR8 => self.set_r8(i.gpr_dst(), self.r8(i.gpr_a())),
            MOV_GPR16 => self.set_r(i.gpr_dst(), self.regs[i.gpr_a()], 16),
            CONST_GPR8 => self.set_r8(i.gpr_dst(), i.aux as u8),
            CONST_GPR16 => self.set_r(i.gpr_dst(), i.aux, 16),
            MOV_U8_I32 => self.regs[i.gpr_dst()] = self.r8(i.gpr_a()) as u32,
            MOV_U16_I32 => self.regs[i.gpr_dst()] = self.regs[i.gpr_a()] & 0xffff,
            MOV_S8_I32 => self.regs[i.gpr_dst()] = self.r8(i.gpr_a()) as i8 as i32 as u32,
            MOV_S16_I32 => self.regs[i.gpr_dst()] = self.regs[i.gpr_a()] as i16 as i32 as u32,
            LEA_I32 => {
                let base = if i.has_base() {
                    self.regs[i.gpr_a()]
                } else {
                    0
                };
                let index = if i.has_index() {
                    self.regs[i.gpr_b()].wrapping_shl(i.address_scale())
                } else {
                    0
                };
                self.regs[i.gpr_dst()] = base.wrapping_add(index).wrapping_add(i.aux);
            }
            JMP_I32 => self.eip = i.aux,
            JCC_BASE..=47 => {
                if self.cc(op - JCC_BASE) {
                    self.eip = i.aux;
                }
            }
            ADD_I32..=DEC_I32 => {
                let a = self.regs[i.gpr_a()];
                let b = if op >= INC_I32 || op & 1 != 0 {
                    i.aux
                } else {
                    self.regs[i.gpr_b()]
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
                    if lazy { 0 } else { i.eager_flags() },
                );
                if !matches!(op, CMP_I32 | CMP_IMM_I32 | TEST_I32 | TEST_IMM_I32) {
                    self.regs[i.gpr_dst()] = result;
                }
            }
            ALU_GPR8_BASE..=ALU_GPR8_END | ALU_GPR16_BASE..=ALU_GPR16_END => {
                let (op, width) = if op >= ALU_GPR16_BASE {
                    (op - ALU_GPR16_BASE + ADD_I32, 16)
                } else {
                    (op - ALU_GPR8_BASE + ADD_I32, 8)
                };
                let a = self.get_r(i.gpr_a(), width);
                let b = if op >= INC_I32 || op & 1 != 0 {
                    i.aux
                } else {
                    self.get_r(i.gpr_b(), width)
                };
                let sub = if op >= INC_I32 {
                    9 + op - INC_I32
                } else {
                    (op - ADD_I32) / 2
                };
                let result =
                    self.compact_alu(a, b, sub, width, if lazy { 0 } else { i.eager_flags() });
                if !matches!(sub, 7 | 8) {
                    self.set_r(i.gpr_dst(), result, width);
                }
            }
            _ => unreachable!("validated scalar run"),
        }
    }
    #[inline(always)]
    fn compact_alu(&mut self, a: u32, b: u32, sub: u8, width: u8, eager: u32) -> u32 {
        let carry = if matches!(sub, 2 | 3) {
            self.flag(CF) as u32
        } else {
            0
        };
        let result = match sub {
            0 | 2 | 9 => a.wrapping_add(b).wrapping_add(carry),
            1 => a | b,
            3 | 5 | 7 | 10 => a.wrapping_sub(b).wrapping_sub(carry),
            4 | 8 => a & b,
            6 => a ^ b,
            _ => unreachable!(),
        } & mask(width);
        let defs = if sub >= 9 { FLAGS & !CF } else { FLAGS };
        self.defer_flags(
            PendingFlags {
                a,
                b,
                result,
                carry,
                width,
                sub,
                deferred: 0,
            },
            defs,
            eager,
        );
        result
    }
    pub(super) fn ir_address(&self, o: Operand, segment: bool) -> u32 {
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
    // Keep the large compatibility dispatcher out of the compact loop's
    // instruction cache and register allocation. It ends a chain anyway.
    #[inline(never)]
    fn execute_ir(&mut self, r: Record, mem: &mut GuestMem) -> Result<(), CpuError> {
        self.eip = r.next_ip();
        let a = r.operand(0);
        let b = r.operand(1);
        let width = r.width();
        let op = r.op();
        match op {
            Op::Nop => {}
            Op::Mov | Op::Movzx | Op::Movsx => {
                let mut value = self.ir_read(b, mem)?;
                if op == Op::Movsx {
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
                if op == Op::CallNative {
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
