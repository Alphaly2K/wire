//! Software exceptions: real guest handlers and checked AMD64 context records.
use super::{unwind::Table, Cpu64, Memory, Runtime64, READ};
pub(crate) const CONTEXT_SIZE: usize = 1232;
pub(crate) fn capture(cpu: &Cpu64) -> [u8; CONTEXT_SIZE] {
    let mut data = [0; CONTEXT_SIZE];
    data[48..52].copy_from_slice(&0x10000bu32.to_le_bytes());
    data[52..56].copy_from_slice(&cpu.mxcsr.to_le_bytes());
    for (n, segment) in [0x33u16, 0x2b, 0x2b, 0x53, 0x2b, 0x2b]
        .into_iter()
        .enumerate()
    {
        data[56 + n * 2..58 + n * 2].copy_from_slice(&segment.to_le_bytes());
    }
    data[68..72].copy_from_slice(&(cpu.rflags as u32).to_le_bytes());
    for (n, value) in cpu.regs.iter().enumerate() {
        data[120 + n * 8..128 + n * 8].copy_from_slice(&value.to_le_bytes());
    }
    data[248..256].copy_from_slice(&cpu.rip.to_le_bytes());
    data[256..258].copy_from_slice(&cpu.fpu.cw.to_le_bytes());
    data[258..260].copy_from_slice(&cpu.fpu.sw.to_le_bytes());
    for n in 0..8 {
        if (cpu.fpu.tags >> (n * 2)) & 3 != 3 {
            data[260] |= 1 << n;
        }
        data[288 + n * 16..298 + n * 16].copy_from_slice(&cpu.fpu.st_bits(n).to_le_bytes()[..10]);
    }
    data[262..264].copy_from_slice(&cpu.fpu.opcode.to_le_bytes());
    data[264..272].copy_from_slice(&(cpu.fpu.ip as u64).to_le_bytes());
    data[272..280].copy_from_slice(&(cpu.fpu.dp as u64).to_le_bytes());
    data[280..284].copy_from_slice(&cpu.mxcsr.to_le_bytes());
    data[284..288].copy_from_slice(&0xffffu32.to_le_bytes());
    for n in 0..16 {
        data[416 + n * 16..432 + n * 16].copy_from_slice(&cpu.xmm[n]);
    }
    data
}
pub(crate) fn restore(mem: &Memory, pointer: u64, parent: &Cpu64) -> Result<Cpu64, String> {
    let mut data = [0; CONTEXT_SIZE];
    mem.read(pointer, &mut data)?;
    let word = |at| u16::from_le_bytes(data[at..at + 2].try_into().unwrap());
    let dword = |at| u32::from_le_bytes(data[at..at + 4].try_into().unwrap());
    let qword = |at| u64::from_le_bytes(data[at..at + 8].try_into().unwrap());
    let flags = dword(48);
    if flags & !0x10000b != 0 || flags & 0x100000 == 0 {
        return Err("unsupported AMD64 exception context components".into());
    }
    let mut cpu = parent.clone();
    if flags & 1 != 0 {
        if qword(248) >= 1 << 47 || qword(152) >= 1 << 47 {
            return Err("noncanonical AMD64 exception continuation".into());
        }
        const USER_FLAGS: u64 =
            1 | 4 | 16 | 64 | 128 | 256 | 1024 | 2048 | 65536 | 262144 | 2097152;
        if (dword(68) as u64 ^ cpu.rflags) & !USER_FLAGS != 0 {
            return Err("unsupported privileged AMD64 exception FLAGS change".into());
        }
        cpu.rflags = (cpu.rflags & !USER_FLAGS) | (dword(68) as u64 & USER_FLAGS);
        cpu.regs[4] = qword(152);
        cpu.rip = qword(248);
    }
    if flags & 2 != 0 {
        for n in 0..16 {
            if n != 4 {
                cpu.regs[n] = qword(120 + n * 8);
            }
        }
    }
    if flags & 8 != 0 {
        if dword(52) & !0xffff != 0 || dword(280) != dword(52) {
            return Err("invalid AMD64 exception MXCSR state".into());
        }
        if qword(264) > u32::MAX as u64 || qword(272) > u32::MAX as u64 {
            return Err(
                "64-bit x87 exception instruction/data pointers are not implemented".into(),
            );
        }
        cpu.mxcsr = dword(52);
        cpu.fpu.cw = word(256);
        cpu.fpu.sw = word(258);
        cpu.fpu.opcode = word(262);
        cpu.fpu.ip = qword(264) as u32;
        cpu.fpu.dp = qword(272) as u32;
        for n in 0..8 {
            let mut bits = [0; 16];
            bits[..10].copy_from_slice(&data[288 + n * 16..298 + n * 16]);
            cpu.fpu.set_st_bits(n, u128::from_le_bytes(bits));
        }
        for n in 0..8 {
            if data[260] & (1 << n) == 0 {
                cpu.fpu.tags |= 3 << (n * 2);
            }
        }
        for n in 0..16 {
            cpu.xmm[n].copy_from_slice(&data[416 + n * 16..432 + n * 16]);
        }
    }
    Ok(cpu)
}
#[derive(Clone)]
pub(crate) struct Handler {
    pub base: u64,
    pub entry: u64,
    pub start: u64,
    pub end: u64,
    pub frame: u64,
    pub target: u64,
    pub data: u64,
    pub unwind: bool,
}
#[derive(Clone)]
pub(crate) enum Phase {
    Vectored,
    Language,
    Target { pc: u64, value: u64 },
}
#[derive(Clone)]
pub(crate) struct Exception {
    pub code: u32,
    pub flags: u32,
    pub block: u64,
    pub handler: Option<Handler>,
    pub phase: Phase,
    pub dll: String,
    pub args: Vec<u64>,
}
impl Exception {
    pub fn context(&self) -> u64 {
        self.block + 256
    }
    pub fn pointers(&self) -> u64 {
        self.block + 160
    }
    pub fn dispatcher(&self) -> u64 {
        self.block + 1536
    }
}
impl Runtime64 {
    pub(crate) fn vectored_handlers(&self) -> Vec<u64> {
        self.exception_handlers
            .iter()
            .map(|&(_, target)| target)
            .collect()
    }
    pub(crate) fn initial_exception_handler(
        &self,
        mem: &Memory,
        cpu: &Cpu64,
    ) -> Result<Option<Handler>, String> {
        for (&table, info) in &self.function_tables {
            let Table::Array { base, count, .. } = info else {
                continue;
            };
            if cpu.rip < *base || cpu.rip - *base > u32::MAX as u64 {
                continue;
            }
            let pc = cpu.rip - *base;
            let mut low = 0;
            let mut high = *count as u64;
            while low < high {
                let mid = (low + high) / 2;
                if mem.read_int(table + mid * 12, 4)? <= pc {
                    low = mid + 1;
                } else {
                    high = mid;
                }
            }
            if low == 0 {
                continue;
            }
            let entry = table + (low - 1) * 12;
            let start = mem.read_int(entry, 4)?;
            let end = mem.read_int(entry + 4, 4)?;
            if pc >= end {
                continue;
            }
            let unwind = base
                .checked_add(mem.read_int(entry + 8, 4)?)
                .ok_or("unwind address overflow")?;
            let mut header = [0; 4];
            mem.read(unwind, &mut header)?;
            if header[0] & 7 != 1 || header[0] & 0x20 != 0 {
                return Err(
                    "AMD64 exception search needs unsupported chained/versioned unwind".into(),
                );
            }
            if header[0] & 8 == 0 || pc - start < header[1] as u64 {
                return Ok(None);
            }
            mem.check(unwind + 4, header[2] as usize * 2, READ)?;
            let frame_reg = header[3] & 15;
            if frame_reg == 0 && header[3] != 0 {
                return Err("invalid AMD64 unwind frame offset".into());
            }
            let frame = if frame_reg == 0 {
                cpu.regs[4]
            } else {
                cpu.regs[frame_reg as usize]
                    .checked_sub((header[3] >> 4) as u64 * 16)
                    .ok_or("AMD64 exception establisher frame underflow")?
            };
            let at = unwind + (4 + header[2] as u64 * 2).div_ceil(4) * 4;
            let target = base
                .checked_add(mem.read_int(at, 4)?)
                .ok_or("handler address overflow")?;
            return Ok(Some(Handler {
                base: *base,
                entry,
                start: *base + start,
                end: *base + end,
                frame,
                target,
                data: at + 4,
                unwind: header[0] & 16 != 0,
            }));
        }
        Ok(None)
    }
    pub(crate) fn prepare_exception(
        &mut self,
        mem: &mut Memory,
        cpu: &Cpu64,
        dll: String,
        args: Vec<u64>,
    ) -> Result<Exception, String> {
        let mut data = [0; 1664];
        let code = args[0] as u32 & !0x10000000; // RaiseException clears the reserved debugger bit.
        data[..4].copy_from_slice(&code.to_le_bytes());
        data[4..8].copy_from_slice(&(args[1] as u32).to_le_bytes());
        data[16..24].copy_from_slice(&cpu.rip.to_le_bytes());
        data[24..28].copy_from_slice(&(args[2] as u32).to_le_bytes());
        mem.read(args[3], &mut data[32..32 + args[2] as usize * 8])?;
        data[256..256 + CONTEXT_SIZE].copy_from_slice(&capture(cpu));
        let block = self.heap_alloc(mem, data.len() as u64)?;
        if block == 0 {
            return Err("AMD64 exception record allocation failed".into());
        }
        data[160..168].copy_from_slice(&block.to_le_bytes());
        data[168..176].copy_from_slice(&(block + 256).to_le_bytes());
        mem.write(block, &data)?;
        Ok(Exception {
            code,
            flags: args[1] as u32,
            block,
            handler: None,
            phase: Phase::Vectored,
            dll,
            args,
        })
    }
}
