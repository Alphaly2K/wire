//! MMX integer operations alias the physical x87 register file, not ST(i).
use super::{Cpu, CpuError};
use crate::x86::memory::GuestMem;

pub(super) fn is_opcode(op: u8) -> bool {
    matches!(op, 0x60..=0x6b | 0x6e..=0x77 | 0x7e | 0x7f | 0xc4 | 0xc5 |
        0xd1..=0xd5 | 0xd7..=0xdf | 0xe0..=0xe5 | 0xe7..=0xef |
        0xf1..=0xf3 | 0xf5 | 0xf6 | 0xf8..=0xfe)
}
fn lane(value: u64, i: usize, bits: u32) -> u64 {
    (value >> (i as u32 * bits)) & (u64::MAX >> (64 - bits))
}
fn signed(value: u64, bits: u32) -> i64 {
    ((value << (64 - bits)) as i64) >> (64 - bits)
}
fn shift(value: u64, bits: u32, count: u64, group: u8) -> u64 {
    let mut out = 0;
    for i in 0..64 / bits {
        let v = lane(value, i as usize, bits);
        let result = if group == 4 {
            (signed(v, bits) >> count.min((bits - 1) as u64)) as u64
        } else if count >= bits as u64 {
            0
        } else if group == 6 {
            v << count
        } else {
            v >> count
        };
        out |= (result & (u64::MAX >> (64 - bits))) << (i * bits);
    }
    out
}
impl Cpu {
    pub(super) fn mmx(
        &mut self,
        mem: &mut GuestMem,
        seg: u32,
        op: u8,
        eip: u32,
    ) -> Result<(), CpuError> {
        self.fpu_wait(eip)?;
        if op == 0x77 {
            self.fpu.tags = 0xffff; // EMMS preserves payloads and TOP.
            return Ok(());
        }
        let rm = self.modrm(mem)?;
        let a = self.fpu.regs[rm.reg] as u64;
        let mut destination = rm.reg;
        let result = match op {
            0x6e => Some(self.rm_read(mem, &rm, 32, seg)? as u64),
            0x7e => {
                self.rm_write(mem, &rm, a as u32, 32, seg)?;
                None
            }
            0x7f | 0xe7 => {
                if let Some(address) = rm.addr {
                    mem.write_u64(address.wrapping_add(seg), a)?;
                    None
                } else if op == 0x7f {
                    destination = rm.idx;
                    Some(a)
                } else {
                    return Err(self.unsupported(mem, eip, "movntq requires memory"));
                }
            }
            0xc4 => {
                let value = self.rm_read(mem, &rm, 16, seg)? as u64;
                let at = (self.fetch8(mem)? & 3) as u32 * 16;
                Some((a & !(0xffff << at)) | (value << at))
            }
            0xc5 | 0xd7 => {
                if rm.addr.is_some() {
                    return Err(self.unsupported(mem, eip, "MMX extraction requires register"));
                }
                let value = self.fpu.regs[rm.idx] as u64;
                let out = if op == 0xc5 {
                    lane(value, (self.fetch8(mem)? & 3) as usize, 16) as u32
                } else {
                    (0..8).map(|i| ((lane(value, i, 8) >> 7) as u32) << i).sum()
                };
                self.set_r(rm.reg, out, 32);
                None
            }
            0x71..=0x73 => {
                if rm.addr.is_some() || !matches!(rm.reg, 2 | 4 | 6) || (op == 0x73 && rm.reg == 4)
                {
                    return Err(self.unsupported(mem, eip, "invalid MMX shift group"));
                }
                let count = self.fetch8(mem)? as u64;
                destination = rm.idx;
                Some(shift(
                    self.fpu.regs[rm.idx] as u64,
                    16 << (op - 0x71),
                    count,
                    rm.reg as u8,
                ))
            }
            _ => {
                let b = match rm.addr {
                    // Low unpack consumes only the low dword of a memory operand.
                    Some(address) if matches!(op, 0x60..=0x62) => {
                        mem.read_u32(address.wrapping_add(seg))? as u64
                    }
                    Some(address) => mem.read_u64(address.wrapping_add(seg))?,
                    None => self.fpu.regs[rm.idx] as u64,
                };
                Some(match op {
                    0x6f => b,
                    0x70 => {
                        let imm = self.fetch8(mem)?;
                        (0..4)
                            .map(|i| lane(b, ((imm >> (i * 2)) & 3) as usize, 16) << (i * 16))
                            .fold(0, |x, y| x | y)
                    }
                    0xdb => a & b,
                    0xdf => !a & b,
                    0xeb => a | b,
                    0xef => a ^ b,
                    0xd1..=0xd3 | 0xe1 | 0xe2 | 0xf1..=0xf3 => shift(
                        a,
                        16 << ((op & 15) - 1),
                        b,
                        if op >= 0xf1 {
                            6
                        } else if op >= 0xe1 {
                            4
                        } else {
                            2
                        },
                    ),
                    0x60..=0x62 | 0x68..=0x6a => {
                        let bits = 8 << (op & 3);
                        let off = if op >= 0x68 { 32 / bits } else { 0 };
                        let mut out = 0;
                        for i in 0..32 / bits {
                            out |= lane(a, (off + i) as usize, bits) << (2 * i * bits);
                            out |= lane(b, (off + i) as usize, bits) << ((2 * i + 1) * bits);
                        }
                        out
                    }
                    0x63 | 0x67 | 0x6b => {
                        let bits = if op == 0x6b { 16 } else { 8 };
                        let mut out = 0;
                        for (half, source) in [a, b].into_iter().enumerate() {
                            for i in 0..32 / bits {
                                let value = signed(lane(source, i as usize, bits * 2), bits * 2);
                                let narrowed = if op == 0x67 {
                                    value.clamp(0, 255)
                                } else {
                                    value.clamp(-(1 << (bits - 1)), (1 << (bits - 1)) - 1)
                                };
                                out |= (narrowed as u64 & ((1 << bits) - 1))
                                    << (half as u32 * 32 + i * bits);
                            }
                        }
                        out
                    }
                    0xf5 => {
                        // PMADDWD wraps its singular -32768* -32768 pair overflow.
                        let mut out = 0;
                        for i in 0..2 {
                            let product =
                                |j| signed(lane(a, j, 16), 16) * signed(lane(b, j, 16), 16);
                            out |=
                                ((product(i * 2) + product(i * 2 + 1)) as u32 as u64) << (i * 32);
                        }
                        out
                    }
                    0xf6 => (0..8).map(|i| lane(a, i, 8).abs_diff(lane(b, i, 8))).sum(),
                    _ => {
                        let bits = match op {
                            0x64 | 0x74 | 0xd8 | 0xdc | 0xe0 | 0xe8 | 0xec | 0xf8 | 0xfc | 0xda
                            | 0xde => 8,
                            0x66 | 0x76 | 0xfa | 0xfe => 32,
                            0xd4 | 0xfb => 64,
                            _ => 16,
                        };
                        let mask = u64::MAX >> (64 - bits);
                        let mut out = 0;
                        for i in 0..64 / bits {
                            let x = lane(a, i as usize, bits);
                            let y = lane(b, i as usize, bits);
                            let sx = signed(x, bits);
                            let sy = signed(y, bits);
                            let v = match op {
                                0x64..=0x66 => {
                                    if sx > sy {
                                        mask
                                    } else {
                                        0
                                    }
                                }
                                0x74..=0x76 => {
                                    if x == y {
                                        mask
                                    } else {
                                        0
                                    }
                                }
                                0xd4 | 0xfc..=0xfe => x.wrapping_add(y),
                                0xf8..=0xfb => x.wrapping_sub(y),
                                0xd8 | 0xd9 => x.saturating_sub(y),
                                0xdc | 0xdd => (x + y).min(mask),
                                0xe8 | 0xe9 | 0xec | 0xed => {
                                    let z = if op < 0xec { sx - sy } else { sx + sy };
                                    z.clamp(-(1 << (bits - 1)), (1 << (bits - 1)) - 1) as u64
                                }
                                0xe0 | 0xe3 => (x + y).div_ceil(2),
                                0xda => x.min(y),
                                0xde => x.max(y),
                                0xea => sx.min(sy) as u64,
                                0xee => sx.max(sy) as u64,
                                0xd5 => x * y,
                                0xe4 => (x * y) >> 16,
                                0xe5 => ((sx * sy) >> 16) as u64,
                                _ => return Err(self.unsupported(mem, eip, "MMX integer opcode")),
                            };
                            out |= (v & mask) << (i * bits);
                        }
                        out
                    }
                })
            }
        };
        if let Some(value) = result {
            self.fpu.regs[destination] = (0xffffu128 << 64) | value as u128;
        }
        // Commit the MMX state only after all checked reads/writes succeeded.
        self.fpu.tags = 0;
        self.fpu.sw &= !0x3800;
        Ok(())
    }
}
