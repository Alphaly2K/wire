//! Portable, fixed-record predecoded IR. This frontend uses the MIT iced-x86
//! decoder; the IR and optimizer are independently implemented for wire.
use crate::cpu::{AF, CF, OF, PF, SF, ZF};
use crate::mem::GuestMem;
use crate::pe::PeFile;
use iced_x86::{Code, Decoder, DecoderOptions, Instruction, Mnemonic, OpKind, Register};
use sha2::{Digest, Sha256};
pub(crate) mod compact;
pub mod wasm;

pub const RECORD_BYTES: usize = 64;
const HEADER_BYTES: usize = 56;
const MAGIC: &[u8; 8] = b"WIREIR01";
const MAX_RECORDS: usize = 2_000_000;
pub(crate) const FLAGS: u32 = CF | PF | AF | ZF | SF | OF;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub(crate) enum Op {
    Fallback,
    Nop,
    Mov,
    Movzx,
    Movsx,
    Lea,
    Alu,
    Push,
    Pop,
    Jmp,
    Jcc,
    Call,
    Ret,
    Leave,
    Setcc,
    Cmov,
    CallNative,
}
impl Op {
    fn parse(n: u32) -> Result<Self, String> {
        Ok(match n {
            0 => Self::Fallback,
            1 => Self::Nop,
            2 => Self::Mov,
            3 => Self::Movzx,
            4 => Self::Movsx,
            5 => Self::Lea,
            6 => Self::Alu,
            7 => Self::Push,
            8 => Self::Pop,
            9 => Self::Jmp,
            10 => Self::Jcc,
            11 => Self::Call,
            12 => Self::Ret,
            13 => Self::Leave,
            14 => Self::Setcc,
            15 => Self::Cmov,
            16 => Self::CallNative,
            _ => return Err("unknown IR operation".into()),
        })
    }
}

/// Symbolic register/immediate/address operand. No host address is serialized.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Operand {
    pub spec: u32,
    pub value: u32,
}
impl Operand {
    pub fn kind(self) -> u32 {
        self.spec & 3
    }
    pub fn width(self) -> u8 {
        ((self.spec >> 16) & 0xff) as u8
    }
    pub fn base(self) -> usize {
        ((self.spec >> 4) & 15) as usize
    }
    pub fn index(self) -> usize {
        ((self.spec >> 8) & 15) as usize
    }
    pub fn scale(self) -> u32 {
        (self.spec >> 12) & 3
    }
    pub fn segment(self) -> u32 {
        (self.spec >> 14) & 3
    }
    fn reg(index: usize, width: u8) -> Self {
        Self {
            spec: 1 | ((index as u32) << 4) | ((width as u32) << 16),
            value: 0,
        }
    }
    fn imm(value: u32) -> Self {
        Self { spec: 2, value }
    }
    fn validate(self) -> Result<(), String> {
        if self.spec & 0xff00_000c != 0 {
            return Err("reserved IR operand bits".into());
        }
        match self.kind() {
            0 if self.spec == 0 && self.value == 0 => Ok(()),
            1 if self.base() < 8
                && matches!(self.width(), 8 | 16 | 32)
                && self.spec & 0xff00_ff00 == 0 =>
            {
                Ok(())
            }
            2 if self.spec == 2 => Ok(()),
            3 if (self.base() < 8 || self.base() == 15)
                && (self.index() < 8 || self.index() == 15)
                && self.segment() < 3
                && matches!(self.width(), 8 | 16 | 32) =>
            {
                Ok(())
            }
            _ => Err("invalid IR operand".into()),
        }
    }
}

/// Exactly sixteen little-endian u32 words on disk, regardless of Rust ABI.
#[derive(Clone, Copy, Debug)]
pub struct Record {
    pub(crate) words: [u32; 16],
}
impl Record {
    pub fn ip(&self) -> u32 {
        self.words[0]
    }
    pub fn next_ip(&self) -> u32 {
        self.words[1]
    }
    pub(crate) fn op(&self) -> Op {
        Op::parse(self.words[2] & 0xff).expect("validated IR")
    }
    pub(crate) fn width(&self) -> u8 {
        ((self.words[2] >> 8) & 0xff) as u8
    }
    pub(crate) fn sub(&self) -> u8 {
        ((self.words[2] >> 16) & 0xff) as u8
    }
    pub(crate) fn defs(&self) -> u32 {
        self.words[3] & 0xffff
    }
    pub(crate) fn reads(&self) -> u32 {
        self.words[3] >> 16
    }
    pub fn eager_flags(&self) -> u32 {
        self.words[4]
    }
    pub(crate) fn operand(&self, n: usize) -> Operand {
        Operand {
            spec: self.words[5 + n * 2],
            value: self.words[6 + n * 2],
        }
    }
    pub(crate) fn opcode(&self) -> u32 {
        self.words[11]
    }
    /// Image-local import identity, encoded as the IAT slot's RVA.
    pub(crate) fn native_import(&self) -> Option<u32> {
        (self.op() == Op::CallNative).then_some(self.words[10])
    }
    pub(crate) fn raw(&self) -> [u8; 16] {
        let mut b = [0; 16];
        for (i, w) in self.words[12..].iter().enumerate() {
            b[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
        b
    }
    fn validate(&self) -> Result<(), String> {
        let op = Op::parse(self.words[2] & 0xff)?;
        if !(1..=15).contains(&self.next_ip().wrapping_sub(self.ip()))
            || self.next_ip() <= self.ip()
        {
            return Err("invalid IR instruction range".into());
        }
        if self.words[2] & 0xff00_0000 != 0
            || !matches!(self.width(), 8 | 16 | 32)
            || self.sub() > 15
            || self.defs() & !FLAGS != 0
            || self.reads() & !FLAGS != 0
            || self.eager_flags() & !self.defs() != 0
            || self.opcode() & 0xff00_ff00 != 0
            || (self.opcode() >> 16 != 0 && self.opcode() & 0xff != 0x0f)
        {
            return Err("invalid IR instruction fields".into());
        }
        for i in 0..2 {
            self.operand(i).validate()?;
        }
        if op == Op::Alu && self.sub() > 10 {
            return Err("invalid IR arithmetic kind".into());
        }
        if op == Op::CallNative {
            let target = self.operand(0);
            if target.kind() != 3
                || target.width() != 32
                || target.base() != 15
                || target.index() != 15
                || target.segment() != 0
                || self.words[9] != 2
            {
                return Err("invalid IR native import call".into());
            }
        }
        if op != Op::CallNative {
            let expected = compact::Inst::lower(self);
            if [self.words[9], self.words[10]] != expected.map_or([0, 0], |i| [i.bits, i.aux]) {
                return Err("invalid compact IR instruction".into());
            }
        }
        Ok(())
    }
}

pub struct Image {
    pub image_base: u32,
    pub exe_hash: [u8; 32],
    pub records: Vec<Record>,
}
impl Image {
    pub fn from_code(base: u32, code: &[u8]) -> Result<Self, String> {
        let mut records = decode(base, code)?;
        optimize_flags(&mut records);
        pack_hot(&mut records);
        Ok(Self {
            image_base: base,
            exe_hash: Sha256::digest(code).into(),
            records,
        })
    }
    pub fn from_pe(exe: &[u8]) -> Result<Self, String> {
        let pe = PeFile::parse(exe)?;
        let mut records = Vec::new();
        for section in pe.sections.iter().filter(|s| s.is_executable()) {
            let start = section.raw_ptr as usize;
            let end = start
                .checked_add(section.raw_size as usize)
                .filter(|&n| n <= exe.len())
                .ok_or("truncated executable section")?;
            let base = pe
                .image_base
                .checked_add(section.virtual_address)
                .ok_or("code address overflow")?;
            records.extend(decode(base, &exe[start..end])?);
        }
        records.sort_by_key(Record::ip);
        records.dedup_by_key(|r| r.ip());
        if records.len() > MAX_RECORDS {
            return Err("IR instruction limit exceeded".into());
        }
        let imports: std::collections::HashSet<u32> = pe
            .imports(exe)?
            .into_iter()
            .flat_map(|dll| dll.functions.into_iter().map(|f| f.iat_rva))
            .collect();
        for r in &mut records {
            let target = r.operand(0);
            if r.op() == Op::Call
                && target.kind() == 3
                && target.width() == 32
                && target.base() == 15
                && target.index() == 15
                && target.segment() == 0
            {
                if let Some(rva) = target.value.checked_sub(pe.image_base) {
                    if imports.contains(&rva) {
                        r.words[2] = Op::CallNative as u32 | (32 << 8);
                        r.words[9] = 2;
                        r.words[10] = rva;
                    }
                }
            }
        }
        optimize_flags(&mut records);
        pack_hot(&mut records);
        Ok(Self {
            image_base: pe.image_base,
            exe_hash: Sha256::digest(exe).into(),
            records,
        })
    }
    pub fn lifted_count(&self) -> usize {
        self.records
            .iter()
            .filter(|r| r.op() != Op::Fallback)
            .count()
    }
    pub fn elided_flag_writers(&self) -> usize {
        self.records
            .iter()
            .filter(|r| r.defs() != 0 && r.eager_flags() != r.defs())
            .count()
    }
    pub fn native_call_count(&self) -> usize {
        self.records
            .iter()
            .filter(|r| r.op() == Op::CallNative)
            .count()
    }
    pub fn compact_count(&self) -> usize {
        self.records
            .iter()
            .filter(|r| r.op() != Op::CallNative && r.words[9] != 0)
            .count()
    }
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_BYTES + self.records.len() * RECORD_BYTES);
        out.extend(MAGIC);
        out.extend(3u32.to_le_bytes());
        out.extend((RECORD_BYTES as u32).to_le_bytes());
        out.extend(self.image_base.to_le_bytes());
        out.extend((self.records.len() as u32).to_le_bytes());
        out.extend(self.exe_hash);
        for r in &self.records {
            for w in r.words {
                out.extend(w.to_le_bytes());
            }
        }
        out
    }
    pub fn parse(bytes: &[u8], exe: &[u8]) -> Result<Self, String> {
        if bytes.len() < HEADER_BYTES || &bytes[..8] != MAGIC {
            return Err("invalid IR header".into());
        }
        let word = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        if word(8) != 3 || word(12) != RECORD_BYTES as u32 {
            return Err("unsupported IR version/record size".into());
        }
        let count = word(20) as usize;
        if count > MAX_RECORDS || bytes.len() != HEADER_BYTES + count * RECORD_BYTES {
            return Err("invalid IR file length".into());
        }
        let hash: [u8; 32] = Sha256::digest(exe).into();
        if bytes[24..56] != hash {
            return Err("IR does not match EXE SHA-256".into());
        }
        let mut records = Vec::with_capacity(count);
        for chunk in bytes[HEADER_BYTES..].as_chunks::<RECORD_BYTES>().0 {
            let mut words = [0; 16];
            for (i, w) in chunk.as_chunks::<4>().0.iter().enumerate() {
                words[i] = u32::from_le_bytes(*w);
            }
            let r = Record { words };
            r.validate()?;
            if records.last().is_some_and(|p: &Record| p.ip() >= r.ip()) {
                return Err("IR addresses are not strictly ordered".into());
            }
            records.push(r);
        }
        Ok(Self {
            image_base: word(16),
            exe_hash: hash,
            records,
        })
    }
}

fn reg(r: Register) -> Option<(usize, u8)> {
    use Register::*;
    Some(match r {
        AL => (0, 8),
        CL => (1, 8),
        DL => (2, 8),
        BL => (3, 8),
        AH => (4, 8),
        CH => (5, 8),
        DH => (6, 8),
        BH => (7, 8),
        AX => (0, 16),
        CX => (1, 16),
        DX => (2, 16),
        BX => (3, 16),
        SP => (4, 16),
        BP => (5, 16),
        SI => (6, 16),
        DI => (7, 16),
        EAX => (0, 32),
        ECX => (1, 32),
        EDX => (2, 32),
        EBX => (3, 32),
        ESP => (4, 32),
        EBP => (5, 32),
        ESI => (6, 32),
        EDI => (7, 32),
        _ => return Option::None,
    })
}
fn operand(ins: &Instruction, n: u32) -> Option<Operand> {
    Some(match ins.op_kind(n) {
        OpKind::Register => {
            let (i, w) = reg(ins.op_register(n))?;
            Operand::reg(i, w)
        }
        OpKind::Immediate8 => Operand::imm(ins.immediate8() as u32),
        OpKind::Immediate16 => Operand::imm(ins.immediate16() as u32),
        OpKind::Immediate32 => Operand::imm(ins.immediate32()),
        OpKind::Immediate8to16 => Operand::imm(ins.immediate8to16() as i32 as u32),
        OpKind::Immediate8to32 => Operand::imm(ins.immediate8to32() as u32),
        OpKind::NearBranch32 => Operand::imm(ins.near_branch32()),
        OpKind::Memory => {
            let base = if ins.memory_base() == Register::None {
                15
            } else {
                let (i, w) = reg(ins.memory_base())?;
                if w != 32 {
                    return None;
                }
                i
            };
            let index = if ins.memory_index() == Register::None {
                15
            } else {
                let (i, w) = reg(ins.memory_index())?;
                if w != 32 {
                    return None;
                }
                i
            };
            let width = if ins.mnemonic() == Mnemonic::Lea {
                32
            } else {
                (ins.memory_size().size() * 8) as u8
            };
            if !matches!(width, 8 | 16 | 32) {
                return None;
            }
            let segment = match ins.memory_segment() {
                Register::FS => 1,
                Register::GS => 2,
                _ => 0,
            };
            Operand {
                spec: 3
                    | ((base as u32) << 4)
                    | ((index as u32) << 8)
                    | (ins.memory_index_scale().trailing_zeros() << 12)
                    | (segment << 14)
                    | ((width as u32) << 16),
                value: ins.memory_displacement64() as u32,
            }
        }
        _ => return None,
    })
}
pub(crate) fn cc_flags(cc: u8) -> u32 {
    match cc {
        0 | 1 => OF,
        2 | 3 => CF,
        4 | 5 => ZF,
        6 | 7 => CF | ZF,
        8 | 9 => SF,
        10 | 11 => PF,
        12 | 13 => SF | OF,
        _ => ZF | SF | OF,
    }
}
fn decode(base: u32, bytes: &[u8]) -> Result<Vec<Record>, String> {
    base.checked_add(bytes.len().try_into().map_err(|_| "code too large")?)
        .ok_or("code range overflow")?;
    let mut decoder = Decoder::with_ip(32, bytes, base as u64, DecoderOptions::NONE);
    let mut out = Vec::new();
    while decoder.can_decode() {
        let ins = decoder.decode();
        let start = (ins.ip() as u32 - base) as usize;
        let len = ins.len();
        let mut r = Record { words: [0; 16] };
        r.words[0] = ins.ip() as u32;
        r.words[1] = ins.next_ip() as u32;
        r.words[2] = 32 << 8;
        let mut raw = [0; 16];
        raw[..len].copy_from_slice(&bytes[start..start + len]);
        for (i, b) in raw.as_chunks::<4>().0.iter().enumerate() {
            r.words[12 + i] = u32::from_le_bytes(*b);
        }
        let prefix = raw[..len]
            .iter()
            .position(|b| {
                !matches!(
                    b,
                    0x66 | 0x67 | 0x2e | 0x36 | 0x3e | 0x26 | 0x64 | 0x65 | 0xf0 | 0xf2 | 0xf3
                )
            })
            .unwrap_or(0);
        r.words[11] = raw[prefix] as u32;
        if raw[prefix] == 0x0f && prefix + 1 < len {
            r.words[11] |= (raw[prefix + 1] as u32) << 16;
        }
        // Statistics encode the primary and the optional secondary byte separately.
        let no_address16 = !raw[..prefix].contains(&0x67);
        if !ins.is_invalid() && no_address16 && !ins.has_rep_prefix() && !ins.has_repne_prefix() {
            if let Some((op, width, sub, a, b, defs, reads)) = lift(&ins, &raw[..prefix]) {
                r.words[2] = op as u32 | ((width as u32) << 8) | ((sub as u32) << 16);
                r.words[3] = defs | (reads << 16);
                r.words[4] = defs;
                r.words[5] = a.spec;
                r.words[6] = a.value;
                r.words[7] = b.spec;
                r.words[8] = b.value;
            }
        }
        if r.op() == Op::Fallback {
            r.words[3] = FLAGS | (FLAGS << 16);
            r.words[4] = FLAGS;
        }
        out.push(r);
    }
    Ok(out)
}
type Lifted = (Op, u8, u8, Operand, Operand, u32, u32);
fn lift(i: &Instruction, prefix: &[u8]) -> Option<Lifted> {
    use Mnemonic::*;
    let a = if i.op_count() > 0 {
        operand(i, 0)?
    } else {
        Operand::default()
    };
    let b = if i.op_count() > 1 {
        operand(i, 1)?
    } else {
        Operand::default()
    };
    let width = if matches!(a.kind(), 1 | 3) {
        a.width()
    } else if prefix.contains(&0x66) {
        16
    } else {
        32
    };
    let cc = (i.condition_code() as u8).wrapping_sub(1);
    let (op, sub, defs, reads) = match i.mnemonic() {
        Nop => (Op::Nop, 0, 0, 0),
        Mov => (Op::Mov, 0, 0, 0),
        Movzx => (Op::Movzx, 0, 0, 0),
        Movsx => (Op::Movsx, 0, 0, 0),
        Lea => (Op::Lea, 0, 0, 0),
        Add => (Op::Alu, 0, FLAGS, 0),
        Or => (Op::Alu, 1, FLAGS, 0),
        Adc => (Op::Alu, 2, FLAGS, CF),
        Sbb => (Op::Alu, 3, FLAGS, CF),
        And => (Op::Alu, 4, FLAGS, 0),
        Sub => (Op::Alu, 5, FLAGS, 0),
        Xor => (Op::Alu, 6, FLAGS, 0),
        Cmp => (Op::Alu, 7, FLAGS, 0),
        Test => (Op::Alu, 8, FLAGS, 0),
        Inc => (Op::Alu, 9, FLAGS & !CF, CF),
        Dec => (Op::Alu, 10, FLAGS & !CF, CF),
        Push => (Op::Push, 0, 0, 0),
        Pop if a.kind() == 1 => (Op::Pop, 0, 0, 0),
        Jmp if width == 32 => (Op::Jmp, 0, 0, 0),
        Call if width == 32 => (Op::Call, 0, 0, FLAGS),
        Ret if !prefix.contains(&0x66) && matches!(i.code(), Code::Retnd | Code::Retnd_imm16) => {
            (Op::Ret, 0, 0, FLAGS)
        }
        Leave if width == 32 => (Op::Leave, 0, 0, 0),
        _ if i.is_jcc_short_or_near() && i.op0_kind() == OpKind::NearBranch32 => {
            (Op::Jcc, cc, 0, cc_flags(cc))
        }
        Seto | Setno | Setb | Setae | Sete | Setne | Setbe | Seta | Sets | Setns | Setp | Setnp
        | Setl | Setge | Setle | Setg => (Op::Setcc, cc, 0, cc_flags(cc)),
        Cmovo | Cmovno | Cmovb | Cmovae | Cmove | Cmovne | Cmovbe | Cmova | Cmovs | Cmovns
        | Cmovp | Cmovnp | Cmovl | Cmovge | Cmovle | Cmovg => (Op::Cmov, cc, 0, cc_flags(cc)),
        _ => return None,
    };
    if matches!(
        op,
        Op::Mov | Op::Movzx | Op::Movsx | Op::Lea | Op::Setcc | Op::Cmov
    ) && !matches!(a.kind(), 1 | 3)
    {
        return None;
    }
    if op == Op::Lea && b.kind() != 3 {
        return None;
    }
    if op == Op::Alu && !matches!(a.kind(), 1 | 3) {
        return None;
    }
    Some((op, width, sub, a, b, defs, reads))
}

/// Deliberately local analysis. Unknown control flow, calls, returns, fallback
/// instructions and faultable accesses make all architectural flags live.
/// A flag is omitted only when a later writer in this straight-line region
/// definitely replaces it before any reader or observation boundary.
fn optimize_flags(records: &mut [Record]) {
    let mut live = FLAGS;
    let mut following = None;
    for r in records.iter_mut().rev() {
        let control = matches!(
            r.op(),
            Op::Fallback | Op::Jmp | Op::Jcc | Op::Call | Op::CallNative | Op::Ret
        );
        let stack = matches!(r.op(), Op::Push | Op::Pop | Op::Leave);
        let memory = r.op() != Op::Lea && (0..2).any(|i| r.operand(i).kind() == 3);
        let faultable = stack || memory;
        if control || faultable || following != Some(r.next_ip()) {
            live = FLAGS;
        }
        r.words[4] = if faultable { r.defs() } else { live & r.defs() };
        live = if faultable {
            FLAGS
        } else {
            (live & !r.defs()) | r.reads()
        };
        following = Some(r.ip());
    }
}
fn pack_hot(records: &mut [Record]) {
    for r in records {
        if let Some(inst) = compact::Inst::lower(r) {
            r.words[9] = inst.bits;
            r.words[10] = inst.aux;
        }
    }
}

pub(crate) struct Hit {
    pub hot: compact::Hot,
    pub opcode: u32,
    pub index: usize,
}

/// Address lookup is dense within the image span, avoiding hashing per step.
/// Writes to any watched code page conservatively disable its IR records.
pub struct Cache {
    base: u32,
    slots: Vec<u32>,
    records: Vec<Record>,
    hot: Vec<compact::Hot>,
    versions: Vec<u64>,
    native_targets: Vec<Option<u32>>,
    page_shift: u32,
    pub accepted: usize,
}
impl Cache {
    pub fn bind(image: Image, mem: &GuestMem) -> Result<Self, String> {
        let Some(first) = image.records.first() else {
            return Ok(Self {
                base: 0,
                slots: Vec::new(),
                records: Vec::new(),
                hot: Vec::new(),
                versions: Vec::new(),
                native_targets: Vec::new(),
                page_shift: mem.host_page_size().trailing_zeros(),
                accepted: 0,
            });
        };
        let page_size = mem.host_page_size();
        let base = first.ip() & !(page_size - 1);
        let end = image.records.last().unwrap().next_ip();
        let span = end.checked_sub(base).ok_or("invalid IR range")?;
        if span > 64 * 1024 * 1024 {
            return Err("IR span exceeds 64 MiB cache limit".into());
        }
        let mut cache = Self {
            base,
            slots: vec![0; span as usize],
            native_targets: vec![None; image.records.len()],
            hot: image
                .records
                .iter()
                .map(|r| compact::Hot {
                    inst: compact::Inst::lower_runtime(r).unwrap_or(compact::Inst {
                        bits: if r.op() == Op::CallNative {
                            0
                        } else {
                            r.words[9]
                        },
                        aux: r.words[10],
                    }),
                    next_ip: r.next_ip(),
                    next_slot: 0,
                })
                .collect(),
            records: image.records,
            versions: vec![0; span.div_ceil(page_size) as usize],
            page_shift: page_size.trailing_zeros(),
            accepted: 0,
        };
        for (index, r) in cache.records.iter().enumerate() {
            r.validate()?;
            if r.op() == Op::Fallback {
                continue;
            }
            let len = (r.next_ip() - r.ip()) as usize;
            let mut raw = [0; 16];
            if mem.inspect_code(r.ip(), &mut raw[..len]).is_err() || raw[..len] != r.raw()[..len] {
                continue;
            }
            if let Some(rva) = r.native_import() {
                let slot = image.image_base.checked_add(rva).ok_or("invalid IAT RVA")?;
                if slot != r.operand(0).value {
                    return Err("native import identity does not match IAT operand".into());
                }
                // Guest stub addresses are process-local bindings, never host
                // function pointers. The run loop still checks ApiTable bounds.
                cache.native_targets[index] = mem.read_u32(slot).ok().filter(|&target| {
                    target >= crate::api::STUB_BASE
                        && target
                            < crate::api::STUB_BASE
                                + crate::api::STUB_CAPACITY as u32 * crate::api::STUB_STRIDE
                        && (target - crate::api::STUB_BASE).is_multiple_of(crate::api::STUB_STRIDE)
                });
            }
            cache.slots[(r.ip() - base) as usize] = index as u32 + 1;
            cache.accepted += 1;
            for address in [r.ip(), r.next_ip() - 1] {
                mem.watch_code(address);
                let page = ((address - base) / page_size) as usize;
                cache.versions[page] = mem.code_version(address);
            }
        }
        for hot in &mut cache.hot {
            hot.next_slot = hot
                .next_ip
                .checked_sub(base)
                .and_then(|offset| cache.slots.get(offset as usize))
                .copied()
                .unwrap_or(0);
            let target_slot = hot
                .inst
                .aux
                .checked_sub(base)
                .and_then(|offset| cache.slots.get(offset as usize))
                .copied()
                .unwrap_or(0);
            hot.inst.link_branch(target_slot);
        }
        // Static block planning after byte/permission validation. Every guest
        // entry remains independently addressable. Bound runs contain at most
        // eight accepted, contiguous, nonfaulting scalar operations within one
        // guest page; only the last may branch. No instruction in a run can
        // write code, change protection, switch threads or change callback ESP.
        for index in (0..cache.hot.len()).rev() {
            let r = &cache.records[index];
            let hot = cache.hot[index];
            if !compact::scalar_member(hot.inst)
                || matches!(hot.inst.opcode(), compact::JCC_BASE..=compact::JMP_I32)
                || cache.slots[(r.ip() - base) as usize] != index as u32 + 1
                || hot.next_slot() != index as u32 + 2
            {
                continue;
            }
            let next = cache.hot[index + 1];
            if !compact::scalar_member(next.inst) {
                continue;
            }
            let len = (1 + next.scalar_len()).min(8);
            let end = cache.hot[index + len - 1].next_ip - 1;
            if r.ip() / crate::mem::GUEST_PAGE == end / crate::mem::GUEST_PAGE {
                cache.hot[index].set_scalar_len(len);
            }
        }
        Ok(cache)
    }
    /// Only used for the contiguous range proven by bind and validated at its
    /// entry. The single cooperative host CPU cannot invalidate code inside a
    /// nonfaulting register-only run. Checked indexing remains in place.
    #[inline]
    pub(crate) fn scalar_hot(&self, index: usize) -> compact::Hot {
        self.hot[index]
    }
    #[inline]
    pub(crate) fn get(
        &self,
        ip: u32,
        mem: &GuestMem,
        profile: bool,
        hint: (u32, u32),
    ) -> Option<Hit> {
        let offset = ip.checked_sub(self.base)? as usize;
        let slot = if hint.0 == ip && hint.1 != 0 {
            hint.1
        } else {
            *self.slots.get(offset)?
        };
        if slot == 0 {
            return None;
        }
        let index = slot as usize - 1;
        let hot = self.hot[index];
        let first = offset >> self.page_shift;
        let last = ((hot.next_ip - 1 - self.base) >> self.page_shift) as usize;
        if self.versions[first] != mem.code_page_version(ip as usize >> self.page_shift)
            || (last != first
                && self.versions[last]
                    != mem.code_page_version((hot.next_ip - 1) as usize >> self.page_shift))
        {
            return None;
        }
        Some(Hit {
            hot,
            index,
            opcode: if profile {
                self.records[index].opcode()
            } else {
                0
            },
        })
    }
    pub(crate) fn predecoded(&self, index: usize, mem: &GuestMem) -> Record {
        let mut r = self.records[index];
        if r.op() == Op::CallNative
            && (self.native_targets[index].is_none()
                || mem.read_u32(r.operand(0).value).ok() != self.native_targets[index])
        {
            // IAT hooks must call their actual guest target. Keep predecoded
            // CALL semantics rather than bypassing the patched import slot.
            r.words[2] = Op::Call as u32 | (32 << 8);
        }
        r
    }
}
