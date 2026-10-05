//! Experimental direct lowering of one scalar natural loop, not a full backend.
//! The module owns a small linear memory: 64 u64 vregs at offset zero and
//! scratch I/O at 4096. It is not yet the process's 4 GiB GuestMem mapping.
use super::{Image, Op, Operand, Record, FLAGS};
use crate::cpu::{AF, CF, OF, PF, SF, ZF};

pub const VREG_BYTES: usize = 512;
pub const EIP_SLOT: usize = 9;
pub const RETIRED_SLOT: usize = 10;
pub const DONE: u32 = 0;
pub const BUDGET: u32 = 1;
pub const INVALID_ENTRY: u32 = 2;

pub struct LoopModule {
    pub bytes: Vec<u8>,
    pub entry: u32,
    pub exit: u32,
}

// Function parameter 0 is the budget. Locals 1..8 cache GPRs; all state is
// written back at each exit. No guest flag is discarded in this first lowerer.
const EF: u32 = 9;
const PC: u32 = 10;
const COUNT: u32 = 11;
const A: u32 = 12;
const B: u32 = 13;
const R: u32 = 14;
const C: u32 = 15;

#[derive(Default)]
struct Code(Vec<u8>);
impl Code {
    fn op(&mut self, op: u8) {
        self.0.push(op);
    }
    fn uint(&mut self, n: u32) {
        unsigned(&mut self.0, n);
    }
    fn constant(&mut self, n: u32) {
        self.op(0x41);
        signed(&mut self.0, n as i32 as i64);
    }
    fn constant64(&mut self, n: i64) {
        self.op(0x42);
        signed(&mut self.0, n);
    }
    fn get(&mut self, n: u32) {
        self.op(0x20);
        self.uint(n);
    }
    fn set(&mut self, n: u32) {
        self.op(0x21);
        self.uint(n);
    }
    fn assign(&mut self, n: u32, value: u32) {
        self.constant(value);
        self.set(n);
    }
    fn load_slot(&mut self, slot: usize) {
        self.constant(0);
        self.op(0x28); // i32.load
        self.uint(2);
        self.uint(slot as u32 * 8);
    }
    fn save(&mut self) {
        for (slot, local) in (0..8).map(|r| (r, r as u32 + 1)).chain([
            (8, EF),
            (EIP_SLOT, PC),
            (RETIRED_SLOT, COUNT),
        ]) {
            self.constant(0);
            self.get(local);
            self.op(0xad); // i64.extend_i32_u: canonical PE32 vreg high bits
            self.op(0x37); // i64.store
            self.uint(3);
            self.uint(slot as u32 * 8);
        }
    }
    fn stop(&mut self, reason: u32) {
        self.save();
        self.constant(reason);
        self.op(0x0f); // return
    }
    fn boundary(&mut self, r: &Record) {
        self.assign(PC, r.ip());
        self.get(COUNT);
        self.get(0);
        self.op(0x4f); // i32.ge_u
        self.op(0x04); // if
        self.op(0x40);
        self.stop(BUDGET);
        self.op(0x0b);
    }
    fn retire(&mut self, next: u32) {
        self.get(COUNT);
        self.constant(1);
        self.op(0x6a);
        self.set(COUNT);
        self.assign(PC, next);
    }
    fn operand(&mut self, o: Operand) {
        match o.kind() {
            1 => self.get(o.base() as u32 + 1),
            2 => self.constant(o.value),
            _ => unreachable!("preflight restricts operands"),
        }
    }
    fn flag(&mut self, bit: u32) {
        self.get(EF);
        self.constant(bit);
        self.op(0x71);
        self.op(0x45);
        self.op(0x45); // normalize to 0/1
    }
    fn condition(&mut self, cc: u8) {
        match cc / 2 {
            0 => self.flag(OF),
            1 => self.flag(CF),
            2 => self.flag(ZF),
            3 => {
                self.flag(CF);
                self.flag(ZF);
                self.op(0x72);
            }
            4 => self.flag(SF),
            5 => self.flag(PF),
            6 | 7 => {
                self.flag(SF);
                self.flag(OF);
                self.op(0x73);
                if cc / 2 == 7 {
                    self.flag(ZF);
                    self.op(0x72);
                }
            }
            _ => unreachable!(),
        }
        if cc & 1 != 0 {
            self.op(0x45);
        }
    }
    fn arithmetic(&mut self, r: &Record) {
        self.operand(r.operand(0));
        self.set(A);
        if r.sub() >= 9 {
            self.constant(1);
        } else {
            self.operand(r.operand(1));
        }
        self.set(B);
        if matches!(r.sub(), 2 | 3) {
            self.flag(CF);
            self.set(C);
        } else {
            self.assign(C, 0);
        }
        self.get(A);
        self.get(B);
        let (op, carry_op) = match r.sub() {
            0 | 2 | 9 => (0x6a, Some(0x6a)),
            1 => (0x72, None),
            3 | 5 | 7 | 10 => (0x6b, Some(0x6b)),
            4 | 8 => (0x71, None),
            6 => (0x73, None),
            _ => unreachable!(),
        };
        self.op(op);
        if let Some(op) = carry_op {
            self.get(C);
            self.op(op);
        }
        self.set(R);
        self.flags(r.sub());
        if !matches!(r.sub(), 7 | 8) {
            self.get(R);
            self.set(r.operand(0).base() as u32 + 1);
        }
    }
    fn flags(&mut self, sub: u8) {
        let add = matches!(sub, 0 | 2 | 9);
        let subtract = matches!(sub, 3 | 5 | 7 | 10);
        let partial = sub >= 9;
        self.get(EF);
        self.constant(!(if partial { FLAGS & !CF } else { FLAGS }));
        self.op(0x71);
        if (add || subtract) && !partial {
            self.get(A);
            self.op(0xad);
            self.get(B);
            self.op(0xad);
            self.get(C);
            self.op(0xad);
            self.op(0x7c); // B + carry in i64
            if add {
                self.op(0x7c);
                self.constant64(u32::MAX as i64);
                self.op(0x56); // i64.gt_u
            } else {
                self.op(0x54); // i64.lt_u
            }
            self.op(0x72); // CF has bit value 1
        }
        if add || subtract {
            // OF: ((A^R)&(B^R)) for add, ((A^B)&(A^R)) for sub.
            self.get(A);
            self.get(if add { R } else { B });
            self.op(0x73);
            self.get(if add { B } else { A });
            self.get(R);
            self.op(0x73);
            self.op(0x71);
            self.constant(0x8000_0000);
            self.op(0x71);
            self.constant(20);
            self.op(0x76); // sign bit -> OF bit 11
            self.op(0x72);
            self.get(A);
            self.get(B);
            self.op(0x73);
            self.get(R);
            self.op(0x73);
            self.constant(AF);
            self.op(0x71);
            self.op(0x72);
        }
        self.get(R);
        self.constant(0x8000_0000);
        self.op(0x71);
        self.constant(24);
        self.op(0x76); // SF bit 7
        self.op(0x72);
        self.get(R);
        self.op(0x45);
        self.constant(6);
        self.op(0x74); // ZF
        self.op(0x72);
        self.get(R);
        self.constant(255);
        self.op(0x71);
        self.op(0x69); // i32.popcnt
        self.constant(1);
        self.op(0x71);
        self.constant(1);
        self.op(0x73);
        self.constant(2);
        self.op(0x74); // PF
        self.op(0x72);
        self.set(EF);
    }
    fn instruction(&mut self, r: &Record) {
        self.boundary(r);
        match r.op() {
            Op::Nop => {}
            Op::Mov => {
                self.operand(r.operand(1));
                self.set(r.operand(0).base() as u32 + 1);
            }
            Op::Alu => self.arithmetic(r),
            _ => unreachable!("preflight restricts straight-line instructions"),
        }
        self.retire(r.next_ip());
    }
}

impl Image {
    /// Lower a contiguous register-only region with exactly one final Jcc
    /// backedge. An optional final INT3 is an unexecuted fallback boundary.
    /// Entry must be the first guest PC; mid-region resume is not supported.
    pub fn lower_wasm_loop(&self) -> Result<LoopModule, String> {
        let mut records = self.records.as_slice();
        if records.last().is_some_and(|r| {
            r.op() == Op::Fallback && r.next_ip() - r.ip() == 1 && r.raw()[0] == 0xcc
        }) {
            records = &records[..records.len() - 1];
        }
        let first = records.first().ok_or("empty Wasm region")?;
        for (i, r) in records.iter().enumerate() {
            r.validate()?;
            if i != 0 && records[i - 1].next_ip() != r.ip() {
                return Err("Wasm region must have contiguous guest instructions".into());
            }
        }
        let tail = records.last().unwrap();
        if tail.op() != Op::Jcc || tail.operand(0).kind() != 2 {
            return Err("Wasm region needs a final conditional backedge".into());
        }
        let header = records[..records.len() - 1]
            .iter()
            .position(|r| r.ip() == tail.operand(0).value)
            .ok_or("Wasm backedge must target an instruction in the region")?;
        let reg32 = |o: Operand| o.kind() == 1 && o.width() == 32;
        for r in &records[..records.len() - 1] {
            let supported = match r.op() {
                Op::Nop => true,
                Op::Mov => reg32(r.operand(0)) && (reg32(r.operand(1)) || r.operand(1).kind() == 2),
                Op::Alu => {
                    r.width() == 32
                        && reg32(r.operand(0))
                        && (r.sub() >= 9 || reg32(r.operand(1)) || r.operand(1).kind() == 2)
                }
                _ => false,
            };
            if !supported {
                return Err(format!(
                    "unsupported Wasm loop instruction at {:#x}",
                    r.ip()
                ));
            }
        }
        let mut run = Code::default();
        run.load_slot(EIP_SLOT);
        run.constant(first.ip());
        run.op(0x47); // wrong entry: leave context untouched
        run.op(0x04);
        run.op(0x40);
        run.constant(INVALID_ENTRY);
        run.op(0x0f);
        run.op(0x0b);
        for slot in 0..9 {
            run.load_slot(slot);
            run.set(slot as u32 + 1);
        }
        // EF local is 9, matching EFLAGS vreg 8 above. COUNT starts at zero.
        for r in &records[..header] {
            run.instruction(r);
        }
        run.op(0x03); // one actual structured loop, no IR/PC dispatcher
        run.op(0x40);
        for r in &records[header..records.len() - 1] {
            run.instruction(r);
        }
        run.boundary(tail);
        run.retire(tail.next_ip());
        run.condition(tail.sub());
        run.op(0x04);
        run.op(0x40);
        run.assign(PC, tail.operand(0).value);
        run.op(0x0c);
        run.uint(1); // branch through the if to the loop header
        run.op(0x0b);
        run.op(0x0b);
        run.stop(DONE);
        run.op(0x0b);

        let mut native = gateway_loop(0);
        let mut callback = gateway_loop(2);
        let mut read = Code::default();
        read.constant(1);
        read.get(0);
        read.op(0x10);
        read.uint(0); // imported native gateway
        read.op(0x0b);
        let mut load = Code::default();
        load.get(0);
        load.op(0x28);
        load.uint(2);
        load.uint(0);
        load.op(0x0b);
        let mut grow = Code::default();
        grow.get(0);
        grow.op(0x40);
        grow.uint(0); // memory.grow
        grow.op(0x0b);
        let mut add = Code::default();
        add.get(0);
        add.get(1);
        add.op(0x6a);
        add.op(0x0b);
        let functions = [
            body(&[(15, 0x7f)], &run.0),
            body(&[(2, 0x7f)], &std::mem::take(&mut native.0)),
            body(&[], &read.0),
            body(&[], &load.0),
            body(&[], &grow.0),
            body(&[], &add.0),
            body(&[(2, 0x7f)], &std::mem::take(&mut callback.0)),
        ];
        let mut bytes = b"\0asm\x01\0\0\0".to_vec();
        section(
            &mut bytes,
            1,
            &[2, 0x60, 2, 0x7f, 0x7f, 1, 0x7f, 0x60, 1, 0x7f, 1, 0x7f],
        );
        let mut imports = vec![1];
        name(&mut imports, "wire");
        name(&mut imports, "gateway");
        imports.extend([0, 0]); // function, type0
        section(&mut bytes, 2, &imports);
        section(&mut bytes, 3, &[7, 1, 1, 1, 1, 1, 0, 1]);
        section(&mut bytes, 5, &[1, 1, 1, 16]); // memory min1/max16 pages
        let mut exports = vec![8];
        for (function, label) in [
            "run",
            "native_loop",
            "read_file",
            "load",
            "grow",
            "add",
            "callback_loop",
        ]
        .iter()
        .enumerate()
        {
            name(&mut exports, label);
            exports.push(0);
            unsigned(&mut exports, function as u32 + 1);
        }
        name(&mut exports, "memory");
        exports.extend([2, 0]);
        section(&mut bytes, 7, &exports);
        let mut code = vec![functions.len() as u8];
        for f in functions {
            unsigned(&mut code, f.len() as u32);
            code.extend(f);
        }
        section(&mut bytes, 10, &code);
        let mut metadata = Vec::new();
        name(&mut metadata, "wire.region");
        metadata.extend(first.ip().to_le_bytes());
        metadata.extend(tail.next_ip().to_le_bytes());
        metadata.extend(self.exe_hash);
        section(&mut bytes, 0, &metadata);
        Ok(LoopModule {
            bytes,
            entry: first.ip(),
            exit: tail.next_ip(),
        })
    }
}

fn gateway_loop(tag: u32) -> Code {
    let mut c = Code::default();
    c.op(0x02);
    c.op(0x40);
    c.op(0x03);
    c.op(0x40);
    c.get(1);
    c.get(0);
    c.op(0x4f);
    c.op(0x0d);
    c.uint(1);
    c.get(2);
    c.constant(tag);
    c.get(1);
    c.op(0x10);
    c.uint(0);
    c.op(0x73);
    c.set(2);
    c.get(1);
    c.constant(1);
    c.op(0x6a);
    c.set(1);
    c.op(0x0c);
    c.uint(0);
    c.op(0x0b);
    c.op(0x0b);
    c.get(2);
    c.op(0x0b);
    c
}
fn body(locals: &[(u32, u8)], code: &[u8]) -> Vec<u8> {
    let mut b = Vec::new();
    unsigned(&mut b, locals.len() as u32);
    for &(count, ty) in locals {
        unsigned(&mut b, count);
        b.push(ty);
    }
    b.extend(code);
    b
}
fn unsigned(out: &mut Vec<u8>, mut n: u32) {
    loop {
        let byte = (n & 127) as u8;
        n >>= 7;
        out.push(byte | if n != 0 { 128 } else { 0 });
        if n == 0 {
            break;
        }
    }
}
fn signed(out: &mut Vec<u8>, mut n: i64) {
    loop {
        let byte = (n & 127) as u8;
        n >>= 7;
        let end = (n == 0 && byte & 64 == 0) || (n == -1 && byte & 64 != 0);
        out.push(byte | if end { 0 } else { 128 });
        if end {
            break;
        }
    }
}
fn name(out: &mut Vec<u8>, n: &str) {
    unsigned(out, n.len() as u32);
    out.extend(n.as_bytes());
}
fn section(out: &mut Vec<u8>, id: u8, data: &[u8]) {
    out.push(id);
    unsigned(out, data.len() as u32);
    out.extend(data);
}
