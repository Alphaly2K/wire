//! Runtime PE32 SSE/bit descriptors. Persisted .wir v3 records stay unchanged;
//! binding validates original bytes before deriving this process-local metadata.
use super::{Cpu, CpuError, CF};
use crate::mem::GuestMem;
use crate::simd::SimdState;
use iced_x86::{Decoder, DecoderOptions, Instruction, Mnemonic as M, OpKind, Register as R};
use rustc_apfloat::ieee::{Double, Single};
use rustc_apfloat::{Float, Round};
const MEMORY: u8 = 254;
const XMM: u8 = 32;
#[derive(Clone, Copy)]
pub(crate) struct Descriptor {
    m: M,
    dst: u8,
    src: u8,
    width: u8,
    integer_width: u8,
    immediate: u8,
    address: crate::ir::Operand,
    len: u8,
}
impl Descriptor {
    pub(crate) fn lower(bytes: &[u8], pc: u32) -> Option<Self> {
        let i = Decoder::with_ip(32, bytes, pc as u64, DecoderOptions::NONE).decode();
        if i.is_invalid() || i.len() != bytes.len() {
            return None;
        }
        let prefix = bytes.iter().position(|b| {
            !matches!(
                b,
                0x66 | 0x67 | 0x2e | 0x36 | 0x3e | 0x26 | 0x64 | 0x65 | 0xf0 | 0xf2 | 0xf3
            )
        })?;
        if bytes[..prefix].contains(&0x67) {
            return None;
        }
        let m = i.mnemonic();
        let width = match m {
            M::Movss
            | M::Movd
            | M::Addss
            | M::Subss
            | M::Mulss
            | M::Divss
            | M::Minss
            | M::Maxss
            | M::Cvtss2sd
            | M::Cvttss2si
            | M::Cvtss2si
            | M::Cvtsi2ss
            | M::Comiss
            | M::Ucomiss
            | M::Sqrtss
            | M::Cmpss => 4,
            M::Movsd
            | M::Movq
            | M::Addsd
            | M::Subsd
            | M::Mulsd
            | M::Divsd
            | M::Minsd
            | M::Maxsd
            | M::Cvtsd2ss
            | M::Cvttsd2si
            | M::Cvtsd2si
            | M::Cvtsi2sd
            | M::Comisd
            | M::Ucomisd
            | M::Sqrtsd
            | M::Cmpsd => 8,
            M::Paddb | M::Psubb | M::Pcmpeqb | M::Pcmpgtb => 1,
            M::Paddw | M::Psubw | M::Pcmpeqw | M::Pcmpgtw => 2,
            M::Paddd | M::Psubd | M::Pcmpeqd | M::Pcmpgtd => 4,
            M::Paddq | M::Psubq => 8,
            M::Movups
            | M::Movaps
            | M::Movupd
            | M::Movapd
            | M::Movdqu
            | M::Movdqa
            | M::Pand
            | M::Pandn
            | M::Por
            | M::Pxor
            | M::Andps
            | M::Andnps
            | M::Orps
            | M::Xorps
            | M::Andpd
            | M::Andnpd
            | M::Orpd
            | M::Xorpd => 16,
            M::Bt | M::Bts | M::Btr | M::Btc => {
                i.op0_register().size().max(i.memory_size().size()) as u8
            }
            _ => return None,
        };
        let bit = matches!(m, M::Bt | M::Bts | M::Btr | M::Btc);
        if i.has_lock_prefix() && (!bit || m == M::Bt || i.op0_kind() != OpKind::Memory) {
            return None;
        }
        let mut d = Self {
            m,
            dst: 0,
            src: 0,
            width,
            integer_width: 4,
            immediate: 0,
            address: crate::ir::Operand::default(),
            len: i.len() as u8,
        };
        d.dst = d.operand(&i, 0)?;
        d.src = if bit && i.op1_kind() == OpKind::Immediate8 {
            d.immediate = i.immediate8();
            255
        } else {
            d.operand(&i, 1)?
        };
        if bit {
            if !matches!(width, 2 | 4)
                || d.dst != MEMORY && d.dst >= 8
                || d.src != 255 && d.src >= 8
            {
                return None;
            }
        } else if matches!(m, M::Cvtsi2ss | M::Cvtsi2sd) {
            d.integer_width = i.op1_register().size().max(i.memory_size().size()) as u8;
            if d.dst < XMM || d.integer_width != 4 {
                return None;
            }
        } else if matches!(m, M::Cvtss2si | M::Cvttss2si | M::Cvtsd2si | M::Cvttsd2si) {
            if d.dst >= 8 {
                return None;
            }
        } else if matches!(m, M::Movd | M::Movq) {
            if d.dst < XMM && d.src < XMM {
                return None;
            }
        } else if d.dst < XMM || d.src < XMM {
            return None;
        }
        if matches!(m, M::Cmpss | M::Cmpsd) {
            d.immediate = i.immediate8();
        }
        Some(d)
    }
    fn operand(&mut self, i: &Instruction, n: u32) -> Option<u8> {
        if i.op_kind(n) == OpKind::Memory {
            let mut spec = 3;
            for (reg, shift) in [(i.memory_base(), 4), (i.memory_index(), 8)] {
                let r = if reg == R::None {
                    15
                } else {
                    if reg.size() != 4 {
                        return None;
                    }
                    reg as u32 - R::EAX as u32
                };
                if r >= 8 && r != 15 {
                    return None;
                }
                spec |= r << shift;
            }
            spec |= i.memory_index_scale().trailing_zeros() << 12;
            spec |= match i.memory_segment() {
                R::FS => 1 << 14,
                R::GS => 2 << 14,
                _ => 0,
            };
            self.address = crate::ir::Operand {
                spec,
                value: i.memory_displacement64() as u32,
            };
            Some(MEMORY)
        } else if i.op_kind(n) == OpKind::Register {
            let r = i.op_register(n);
            if (R::XMM0..=R::XMM7).contains(&r) {
                Some(XMM + (r as u32 - R::XMM0 as u32) as u8)
            } else if (R::EAX..=R::EDI).contains(&r) {
                Some((r as u32 - R::EAX as u32) as u8)
            } else if (R::AX..=R::DI).contains(&r) {
                Some((r as u32 - R::AX as u32) as u8)
            } else {
                None
            }
        } else {
            None
        }
    }
    fn read<const N: usize>(&self, cpu: &Cpu, mem: &GuestMem) -> Result<[u8; 16], CpuError> {
        let mut b = [0; 16];
        if self.src == MEMORY {
            mem.read_bytes(cpu.ir_address(self.address, true), &mut b[..N])?;
        } else if self.src >= XMM {
            b[..N].copy_from_slice(&cpu.xmm[(self.src - XMM) as usize][..N]);
        } else {
            b[..N].copy_from_slice(&cpu.regs[self.src as usize].to_le_bytes()[..N]);
        }
        Ok(b)
    }
    pub(crate) fn execute(self, cpu: &mut Cpu, mem: &GuestMem, pc: u32) -> Result<(), CpuError> {
        let result = self.execute_inner(cpu, mem, pc);
        if result.is_err() {
            cpu.eip = pc;
        }
        result
    }
    fn execute_inner(self, cpu: &mut Cpu, mem: &GuestMem, pc: u32) -> Result<(), CpuError> {
        let m = self.m;
        if matches!(m, M::Bt | M::Bts | M::Btr | M::Btc) {
            let bits = self.width as u32 * 8;
            let index = if self.src == 255 {
                self.immediate as u32
            } else {
                cpu.regs[self.src as usize]
            };
            let bit = index & (bits - 1);
            let mut address = cpu.ir_address(self.address, true);
            if self.src != 255 {
                let signed = if bits == 16 {
                    index as i16 as i32
                } else {
                    index as i32
                };
                address = address
                    .wrapping_add((signed.div_euclid(bits as i32) * self.width as i32) as u32);
            }
            let a = if self.dst == MEMORY {
                super::read_sized(mem, address, self.width as u32)?
            } else {
                cpu.get_r(self.dst as usize, bits as u8)
            };
            if m != M::Bt {
                let b = match m {
                    M::Bts => a | (1 << bit),
                    M::Btr => a & !(1 << bit),
                    _ => a ^ (1 << bit),
                };
                if self.dst == MEMORY {
                    super::write_sized(mem, address, b, self.width as u32)?;
                } else {
                    cpu.set_r(self.dst as usize, b, bits as u8);
                }
            }
            cpu.flush_flags();
            cpu.eflags = (cpu.eflags & !CF) | ((a >> bit) & 1);
            return Ok(());
        }
        let packed = matches!(
            m,
            M::Paddb
                | M::Paddw
                | M::Paddd
                | M::Paddq
                | M::Psubb
                | M::Psubw
                | M::Psubd
                | M::Psubq
                | M::Pcmpeqb
                | M::Pcmpeqw
                | M::Pcmpeqd
                | M::Pcmpgtb
                | M::Pcmpgtw
                | M::Pcmpgtd
        );
        let size = if packed {
            16
        } else if matches!(m, M::Cvtsi2ss | M::Cvtsi2sd) {
            4
        } else {
            self.width as usize
        };
        let b = match size {
            4 => self.read::<4>(cpu, mem)?,
            8 => self.read::<8>(cpu, mem)?,
            16 => self.read::<16>(cpu, mem)?,
            _ => unreachable!(),
        };
        let a = if self.dst >= XMM && self.dst != MEMORY {
            cpu.xmm[(self.dst - XMM) as usize]
        } else {
            [0; 16]
        };
        let mut value = a;
        match m {
            M::Movups
            | M::Movaps
            | M::Movupd
            | M::Movapd
            | M::Movdqu
            | M::Movdqa
            | M::Movd
            | M::Movq => value = b,
            M::Movss | M::Movsd => {
                if self.src == MEMORY || self.dst == MEMORY {
                    value = b;
                } else {
                    value[..size].copy_from_slice(&b[..size]);
                }
            }
            M::Pand | M::Andps | M::Andpd => value = std::array::from_fn(|n| a[n] & b[n]),
            M::Pandn | M::Andnps | M::Andnpd => value = std::array::from_fn(|n| !a[n] & b[n]),
            M::Por | M::Orps | M::Orpd => value = std::array::from_fn(|n| a[n] | b[n]),
            M::Pxor | M::Xorps | M::Xorpd => value = std::array::from_fn(|n| a[n] ^ b[n]),
            _ if packed => {
                value = match m {
                    M::Paddb | M::Paddw | M::Paddd | M::Paddq => {
                        crate::simd::packed::<0>(a, b, self.width as usize)
                    }
                    M::Psubb | M::Psubw | M::Psubd | M::Psubq => {
                        crate::simd::packed::<1>(a, b, self.width as usize)
                    }
                    M::Pcmpeqb | M::Pcmpeqw | M::Pcmpeqd => {
                        crate::simd::packed::<2>(a, b, self.width as usize)
                    }
                    _ => crate::simd::packed::<3>(a, b, self.width as usize),
                };
            }
            _ => {
                let mut state = SimdState {
                    mxcsr: cpu.mxcsr,
                    rflags: cpu.eflags as u64,
                };
                let double = self.width == 8;
                let read = |x: [u8; 16]| {
                    if double {
                        u64::from_le_bytes(x[..8].try_into().unwrap())
                    } else {
                        u32::from_le_bytes(x[..4].try_into().unwrap()) as u64
                    }
                };
                let (av, bv) = (read(a), read(b));
                let mut exception = false;
                let result = match m {
                    M::Cvtss2sd => state.simd_convert::<Single, Double>(bv, &mut exception),
                    M::Cvtsd2ss => state.simd_convert::<Double, Single>(bv, &mut exception),
                    M::Cvtsi2ss | M::Cvtsi2sd => {
                        let integer = u32::from_le_bytes(b[..4].try_into().unwrap()) as i32 as i128;
                        let round = state.simd_round();
                        if double {
                            state
                                .simd_accept(Double::from_i128_r(integer, round), 0, &mut exception)
                                .map(|v| v.to_bits() as u64)
                        } else {
                            state
                                .simd_accept(Single::from_i128_r(integer, round), 0, &mut exception)
                                .map(|v| v.to_bits() as u64)
                        }
                    }
                    M::Cvtss2si | M::Cvtsd2si | M::Cvttss2si | M::Cvttsd2si => {
                        let round = if matches!(m, M::Cvttss2si | M::Cvttsd2si) {
                            Round::TowardZero
                        } else {
                            state.simd_round()
                        };
                        if double {
                            state.simd_to_integer::<Double>(bv, 32, round, &mut exception)
                        } else {
                            state.simd_to_integer::<Single>(bv, 32, round, &mut exception)
                        }
                    }
                    M::Comiss | M::Comisd | M::Ucomiss | M::Ucomisd => if double {
                        state.simd_compare::<Double>(m, av, bv, &mut exception)
                    } else {
                        state.simd_compare::<Single>(m, av, bv, &mut exception)
                    }
                    .map(|()| 0),
                    M::Cmpss | M::Cmpsd => if double {
                        state.simd_predicate::<Double>(av, bv, self.immediate, &mut exception)
                    } else {
                        state.simd_predicate::<Single>(av, bv, self.immediate, &mut exception)
                    }
                    .map(|p| if p { u64::MAX } else { 0 }),
                    M::Sqrtss | M::Sqrtsd => {
                        if double {
                            state.simd_sqrt::<Double>(bv, 52, 1023, &mut exception)
                        } else {
                            state.simd_sqrt::<Single>(bv, 23, 127, &mut exception)
                        }
                    }
                    _ => {
                        if double {
                            state.simd_arithmetic::<Double>(m, av, bv, &mut exception)
                        } else {
                            state.simd_arithmetic::<Single>(m, av, bv, &mut exception)
                        }
                    }
                };
                cpu.mxcsr = state.mxcsr;
                let output = result.map_err(|_| CpuError::SimdFloatingPoint {
                    eip: pc,
                    mxcsr: cpu.mxcsr,
                })?;
                if matches!(m, M::Comiss | M::Comisd | M::Ucomiss | M::Ucomisd) {
                    // COMI replaces every arithmetic flag, so previous lazy
                    // recipes are dead only after successful arithmetic.
                    cpu.pending_flags = None;
                    cpu.eflags = state.rflags as u32;
                    return Ok(());
                }
                if matches!(m, M::Cvtss2si | M::Cvtsd2si | M::Cvttss2si | M::Cvttsd2si) {
                    cpu.regs[self.dst as usize] = output as u32;
                    return Ok(());
                }
                let output_double = if m == M::Cvtss2sd {
                    true
                } else if m == M::Cvtsd2ss {
                    false
                } else {
                    double
                };
                if output_double {
                    value[..8].copy_from_slice(&output.to_le_bytes());
                } else {
                    value[..4].copy_from_slice(&(output as u32).to_le_bytes());
                }
            }
        }
        if self.dst == MEMORY {
            mem.write_bytes(cpu.ir_address(self.address, true), &value[..size])?;
        } else if self.dst >= XMM {
            cpu.xmm[(self.dst - XMM) as usize] = value;
        } else {
            cpu.regs[self.dst as usize] = u32::from_le_bytes(value[..4].try_into().unwrap());
        }
        Ok(())
    }
}
impl Cpu {
    // Raw execution shares the same descriptor semantics. Inspection never
    // consumes a future guard; actual instruction bytes are fetched in order.
    pub(super) fn try_raw_simd(&mut self, mem: &GuestMem, pc: u32) -> Option<Result<(), CpuError>> {
        if let Some(d) = mem.cached_simd(pc) {
            while self.eip != pc.wrapping_add(d.len as u32) {
                if let Err(e) = self.fetch8(mem) {
                    self.eip = pc;
                    return Some(Err(e));
                }
            }
            return Some(d.execute(self, mem, pc));
        }
        let mut bytes = [0; 15];
        for len in (self.eip.wrapping_sub(pc) as usize + 1)..=15 {
            if mem.inspect_code(pc, &mut bytes[..len]).is_err() {
                return None;
            }
            let i = Decoder::with_ip(32, &bytes[..len], pc as u64, DecoderOptions::NONE).decode();
            if i.is_invalid() {
                continue;
            }
            let d = Descriptor::lower(&bytes[..len], pc)?;
            mem.cache_simd(pc, d);
            while self.eip != pc.wrapping_add(d.len as u32) {
                if let Err(e) = self.fetch8(mem) {
                    self.eip = pc;
                    return Some(Err(e));
                }
            }
            return Some(d.execute(self, mem, pc));
        }
        None
    }
}

// Owned by the address space, so using one Cpu with a different GuestMem can
// never reuse coincident page version numbers from the previous memory.
pub(crate) struct RawCache {
    entries: Box<[std::cell::Cell<Option<(u32, u64, u64, Descriptor)>>]>,
}
impl Default for RawCache {
    fn default() -> Self {
        Self {
            entries: (0..256)
                .map(|_| std::cell::Cell::new(None))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        }
    }
}
impl GuestMem {
    fn cached_simd(&self, pc: u32) -> Option<Descriptor> {
        let cache = self.raw_simd.get()?;
        let (saved, g, g2, d) = cache.entries[(pc ^ (pc >> 8)) as usize & 255].get()?;
        if saved != pc || g == u64::MAX || g != self.code_version(pc) {
            return None;
        }
        let end = pc.checked_add(d.len as u32 - 1)?;
        if g2 != 0 && (g2 == u64::MAX || g2 != self.code_version(end)) {
            return None;
        }
        Some(d)
    }
    fn cache_simd(&self, pc: u32, d: Descriptor) {
        let Some(end) = pc.checked_add(d.len as u32 - 1) else {
            return;
        };
        self.watch_code(pc);
        self.watch_code(end);
        let g = self.code_version(pc);
        let g2 = if pc / self.host_page_size() != end / self.host_page_size() {
            self.code_version(end)
        } else {
            0
        };
        let cache = self.raw_simd.get_or_init(RawCache::default);
        cache.entries[(pc ^ (pc >> 8)) as usize & 255].set(Some((pc, g, g2, d)));
    }
}
