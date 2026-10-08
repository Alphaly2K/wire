//! AMD64 operand access around the shared software FP80 arithmetic core.
//! New unmasked arithmetic exceptions explicitly stop until deferred x87
//! exception delivery is implemented; no destination or store is committed.
use super::{Cpu64, Instruction, Memory, AF, CF, OF, PF, SF, ZF};
use crate::x64::memory::WRITE;
use rustc_apfloat::ieee::X87DoubleExtended as Ext;
use rustc_apfloat::{Float, Round};

impl Cpu64 {
    pub(super) fn execute_x87(
        &mut self,
        instruction: &Instruction,
        mem: &mut Memory,
    ) -> Result<(), String> {
        if self.fpu.sw & !self.fpu.cw & 0x3f != 0 {
            return Err(format!(
                "unmasked x87 floating-point exception (#MF); status={:#x}",
                self.fpu.sw
            ));
        }
        let mut code = [0; 15];
        mem.fetch(instruction.ip(), &mut code[..instruction.len()])?;
        let offset = code[..instruction.len()]
            .iter()
            .position(|b| (0xd8..=0xdf).contains(b))
            .ok_or("missing x87 opcode")?;
        let op = code[offset];
        let b = *code.get(offset + 1).ok_or("missing x87 ModRM")?;
        let group = ((b >> 3) & 7) as usize;
        self.fpu.ip = instruction.ip();
        self.fpu.opcode = ((op as u16 & 7) << 8) | b as u16;
        // Delay the only possible memory write until the entire operation and
        // exception mask have been checked. Cpu64::step owns CPU rollback.
        let mut store = None;
        if b >= 0xc0 {
            let index = (b & 7) as usize;
            match (op, b) {
                (0xd8, _) | (0xdc, _) | (0xde, 0xc0..=0xcf | 0xe0..=0xff) => {
                    let a = self.fpu.get(0);
                    let v = self.fpu.get(index);
                    if group == 2 || group == 3 {
                        self.fpu.compare_sw(a, v, false);
                        if group == 3 {
                            self.fpu.pop();
                        }
                    } else {
                        let (left, right, kind, dest) = if op == 0xd8 {
                            (a, v, group, 0)
                        } else {
                            (v, a, if group >= 4 { group ^ 1 } else { group }, index)
                        };
                        let result = self.fpu.binary(left, right, kind);
                        self.fpu.put(dest, result);
                        if op == 0xde {
                            self.fpu.pop();
                        }
                    }
                }
                (0xd9, 0xc0..=0xc7) => {
                    let value = self.fpu.get(index);
                    self.fpu.push(value);
                }
                (0xd9, 0xc8..=0xcf) => {
                    let a = self.fpu.get(0);
                    let v = self.fpu.get(index);
                    self.fpu.put(0, v);
                    self.fpu.put(index, a);
                    self.fpu.sw &= !0x200;
                }
                (0xd9, 0xd0) => {}
                (0xd9, 0xe0 | 0xe1) => {
                    let value = self.fpu.get(0);
                    self.fpu
                        .put(0, if b == 0xe0 { -value } else { value.abs() });
                    self.fpu.sw &= !0x200;
                }
                (0xd9, 0xe4) => {
                    let value = self.fpu.get(0);
                    self.fpu.compare_sw(value, Ext::ZERO, false);
                }
                (0xd9, 0xe5) => {
                    let physical = self.fpu.index(0);
                    let value = Ext::from_bits(self.fpu.regs[physical]);
                    let classification = if (self.fpu.tags >> (physical * 2)) & 3 == 3 {
                        0x4100
                    } else if value.is_nan() {
                        0x100
                    } else if value.is_infinite() {
                        0x500
                    } else if value.is_zero() {
                        0x4000
                    } else if value.is_denormal() {
                        0x4400
                    } else {
                        0x400
                    };
                    self.fpu.sw = (self.fpu.sw & !0x4700)
                        | classification
                        | if value.is_negative() { 0x200 } else { 0 };
                }
                (0xd9, 0xe8..=0xee) => self.fpu.load_constant((b - 0xe8) as usize),
                (0xd9, 0xf0) => self.fpu.exp2_minus_one(),
                (0xd9, 0xf1 | 0xf9) => self.fpu.log_multiply(b == 0xf9),
                (0xd9, 0xf6 | 0xf7) => {
                    let top = self.fpu.top();
                    self.fpu.set_top(if b == 0xf6 { top + 7 } else { top + 1 });
                    self.fpu.sw &= !0x200;
                }
                (0xd9, 0xfa) => {
                    let value = self.fpu.get(0);
                    let result = self.fpu.sqrt(value);
                    self.fpu.put(0, result);
                }
                (0xd9, 0xfb | 0xfe | 0xff) => self.fpu.trig(b),
                (0xd9, 0xfc) => {
                    let value = self.fpu.get(0);
                    self.fpu.operands(&[value]);
                    let rounded = value.round_to_integral(self.fpu.round());
                    let truncated = value.round_to_integral(Round::TowardZero).value;
                    let result = self.fpu.accept(rounded, truncated);
                    self.fpu.put(0, result);
                }
                (0xd9, 0xfd) => {
                    let value = self.fpu.get(0);
                    let exponent = self.fpu.get(1);
                    self.fpu.operands(&[value, exponent]);
                    let exponent = exponent
                        .to_i128_r(32, Round::TowardZero, &mut false)
                        .value
                        .clamp(-32768, 32768) as i32;
                    let result = value.scalbn_r(exponent, self.fpu.round());
                    if result.is_infinite() && value.is_finite() {
                        self.fpu.exception(0x28);
                    }
                    if (result.is_denormal() || result.is_zero()) && !value.is_zero() {
                        self.fpu.exception(0x30);
                    }
                    self.fpu.put(0, result);
                }
                (0xda, 0xe9) | (0xde, 0xd9) => {
                    let a = self.fpu.get(0);
                    let v = self.fpu.get(1);
                    self.fpu.compare_sw(a, v, op == 0xda);
                    self.fpu.pop();
                    self.fpu.pop();
                }
                (0xda | 0xdb, 0xc0..=0xdf) => {
                    let condition = match group {
                        0 => self.rflags & CF != 0,
                        1 => self.rflags & ZF != 0,
                        2 => self.rflags & (CF | ZF) != 0,
                        _ => self.rflags & PF != 0,
                    };
                    let a = self.fpu.get(0);
                    let v = self.fpu.get(index);
                    self.fpu
                        .put(0, if condition ^ (op == 0xdb) { v } else { a });
                }
                (0xdb | 0xdf, 0xe8..=0xf7) => {
                    let a = self.fpu.get(0);
                    let v = self.fpu.get(index);
                    let flags = self.fpu.compare(a, v, b < 0xf0);
                    self.rflags &= !(CF | PF | ZF | OF | SF | AF);
                    if flags & 0x100 != 0 {
                        self.rflags |= CF;
                    }
                    if flags & 0x400 != 0 {
                        self.rflags |= PF;
                    }
                    if flags & 0x4000 != 0 {
                        self.rflags |= ZF;
                    }
                    if op == 0xdf {
                        self.fpu.pop();
                    }
                }
                (0xdd, 0xc0..=0xc7) | (0xdf, 0xc0..=0xc7) => {
                    self.fpu.set_tag(self.fpu.index(index), 3);
                    if op == 0xdf {
                        self.fpu.pop();
                    }
                }
                (0xdd, 0xd0..=0xdf) => {
                    let value = self.fpu.get(0);
                    self.fpu.put(index, value);
                    self.fpu.sw &= !0x200;
                    if group == 3 {
                        self.fpu.pop();
                    }
                }
                (0xdd, 0xe0..=0xef) => {
                    let a = self.fpu.get(0);
                    let v = self.fpu.get(index);
                    self.fpu.compare_sw(a, v, true);
                    if group == 5 {
                        self.fpu.pop();
                    }
                }
                _ => {
                    return Err(format!(
                        "unsupported AMD64 x87 register operation {op:02x} {b:02x}"
                    ))
                }
            }
        } else {
            let address = self.address(instruction)?;
            self.fpu.dp = address;
            match (op, group) {
                (0xd8 | 0xdc | 0xda | 0xde, _) => {
                    let v = match op {
                        0xd8 => self.x87_load_real(mem, address, 4)?,
                        0xdc => self.x87_load_real(mem, address, 8)?,
                        0xda => Ext::from_i128(mem.read_int(address, 4)? as i32 as i128).value,
                        _ => Ext::from_i128(mem.read_int(address, 2)? as i16 as i128).value,
                    };
                    let a = self.fpu.get(0);
                    if group == 2 || group == 3 {
                        self.fpu.compare_sw(a, v, false);
                        if group == 3 {
                            self.fpu.pop();
                        }
                    } else {
                        let result = self.fpu.binary(a, v, group);
                        self.fpu.put(0, result);
                    }
                }
                (0xd9 | 0xdd, 0) | (0xdb, 5) => {
                    let size = match op {
                        0xd9 => 4,
                        0xdd => 8,
                        _ => 10,
                    };
                    let value = self.x87_load_real(mem, address, size)?;
                    self.fpu.push(value);
                }
                (0xd9 | 0xdd, 2 | 3) | (0xdb, 7) => {
                    let size = match op {
                        0xd9 => 4,
                        0xdd => 8,
                        _ => 10,
                    };
                    mem.check(address, size, WRITE)?;
                    store = Some((address, self.fpu.store_real_bits(size), size));
                    if group == 3 || group == 7 {
                        self.fpu.pop();
                    }
                }
                (0xdb | 0xdf, 0) | (0xdf, 5) => {
                    let value = match (op, group) {
                        (0xdb, _) => mem.read_int(address, 4)? as i32 as i128,
                        (0xdf, 5) => mem.read_int(address, 8)? as i64 as i128,
                        _ => mem.read_int(address, 2)? as i16 as i128,
                    };
                    self.fpu.push(Ext::from_i128(value).value);
                }
                (0xdb | 0xdf, 1..=3) | (0xdd, 1) | (0xdf, 7) => {
                    let size = match (op, group) {
                        (0xdd, _) | (0xdf, 7) => 8,
                        (0xdb, _) => 4,
                        _ => 2,
                    };
                    mem.check(address, size, WRITE)?;
                    store = Some((address, self.fpu.store_int_bits(size, group == 1), size));
                    if group != 2 {
                        self.fpu.pop();
                    }
                }
                _ => {
                    return Err(format!(
                        "unsupported AMD64 x87 memory operation {op:02x} /{group}"
                    ))
                }
            }
        }
        if self.fpu.sw & !self.fpu.cw & 0x3f != 0 {
            return Err(
                "new unmasked x87 arithmetic exception: deferred delivery is not implemented"
                    .into(),
            );
        }
        if let Some((address, value, size)) = store {
            mem.write(address, &value.to_le_bytes()[..size])?;
        }
        Ok(())
    }
    fn x87_load_real(&mut self, mem: &Memory, address: u64, size: usize) -> Result<Ext, String> {
        let mut bytes = [0; 16];
        mem.read(address, &mut bytes[..size])?;
        Ok(self.fpu.load_real_bits(u128::from_le_bytes(bytes), size))
    }
}
