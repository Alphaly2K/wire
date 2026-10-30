//! AMD64 function metadata and virtual stack unwinding; never executes guest code.
use super::*;
use iced_x86::{Decoder, DecoderOptions, Instruction, Mnemonic as M, OpKind, Register};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
pub(super) struct Function {
    pub base: u64,
    pub entry: u64,
}
enum Table {
    Array {
        count: u32,
        base: u64,
    },
    Callback {
        base: u64,
        length: u32,
        target: u64,
        context: u64,
        _name: Option<String>,
    },
}
#[derive(Clone, Copy)]
pub(super) struct Request {
    pub pc: u64,
    pub base: u64,
    pub target: u64,
    pub context: u64,
}
pub(super) enum FunctionLookup {
    Ready(Option<Function>),
    Callback(Request),
}
impl Request {
    pub(super) fn finish(
        &self,
        memory: &Memory,
        entry: u64,
    ) -> Result<Option<Function>, StartupError> {
        if entry == 0 {
            return Ok(None);
        }
        memory.check(entry, 12, READ)?;
        let start = memory.read_int(entry, 4)?;
        let end = memory.read_int(entry + 4, 4)?;
        if start >= end
            || self.base.checked_add(end).is_none_or(|v| v > 1 << 47)
            || self.pc < self.base
            || self.pc - self.base < start
            || self.pc - self.base >= end
        {
            return Err(invalid(
                "AMD64 function-table callback returned an invalid range",
            ));
        }
        Ok(Some(Function {
            base: self.base,
            entry,
        }))
    }
}
#[derive(Default)]
pub(crate) struct Tables {
    entries: BTreeMap<u64, Table>,
}
pub(super) fn invalid(message: &str) -> StartupError {
    StartupError::Invalid(message.into())
}
fn add(address: u64, offset: u64) -> Result<u64, StartupError> {
    address
        .checked_add(offset)
        .filter(|&p| p < 1 << 47)
        .ok_or_else(|| invalid("noncanonical AMD64 unwind address"))
}
fn valid_array(memory: &Memory, table: u64, count: u32, base: u64) -> Result<bool, Fault> {
    if count == 0 || count > 1_000_000 || base >= 1 << 47 {
        return Ok(false);
    }
    memory.check(table, count as usize * 12, READ)?;
    let mut end = 0;
    for n in 0..u64::from(count) {
        let at = table + n * 12;
        let begin = memory.read_int(at, 4)?;
        let next = memory.read_int(at + 4, 4)?;
        if begin < end || begin >= next || base.checked_add(next).is_none_or(|v| v > 1 << 47) {
            return Ok(false);
        }
        end = next;
    }
    Ok(true)
}
fn find(
    memory: &Memory,
    table: u64,
    count: u32,
    base: u64,
    pc: u64,
) -> Result<Option<Function>, StartupError> {
    let Some(rva) = pc.checked_sub(base).filter(|&v| v <= u64::from(u32::MAX)) else {
        return Ok(None);
    };
    let (mut lo, mut hi) = (0, u64::from(count));
    while lo < hi {
        let mid = (lo + hi) / 2;
        if memory.read_int(add(table, mid * 12)?, 4)? <= rva {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    if lo == 0 {
        return Ok(None);
    }
    let entry = add(table, (lo - 1) * 12)?;
    let start = memory.read_int(entry, 4)?;
    let end = memory.read_int(entry + 4, 4)?;
    if start >= end {
        return Err(invalid("invalid AMD64 runtime function range"));
    }
    Ok((rva >= start && rva < end).then_some(Function { base, entry }))
}
impl Tables {
    pub(super) fn lookup(
        &self,
        memory: &Memory,
        modules: &Linked,
        pc: u64,
    ) -> Result<FunctionLookup, StartupError> {
        for module in modules.modules.values().filter(|m| {
            m.image.mode == Mode::X64 && pc >= m.base && pc - m.base < u64::from(m.image.size)
        }) {
            let dir = module.image.dir(3);
            if dir.size != 0 {
                if let Some(f) = find(
                    memory,
                    module.base + u64::from(dir.rva),
                    dir.size / 12,
                    module.base,
                    pc,
                )? {
                    return Ok(FunctionLookup::Ready(Some(f)));
                }
            }
        }
        for (&table, info) in &self.entries {
            match info {
                Table::Array { count, base } => {
                    if let Some(f) = find(memory, table, *count, *base, pc)? {
                        return Ok(FunctionLookup::Ready(Some(f)));
                    }
                }
                Table::Callback {
                    base,
                    length,
                    target,
                    context,
                    ..
                } if pc >= *base && pc - *base < u64::from(*length) => {
                    return Ok(FunctionLookup::Callback(Request {
                        pc,
                        base: *base,
                        target: *target,
                        context: *context,
                    }))
                }
                _ => {}
            }
        }
        Ok(FunctionLookup::Ready(None))
    }
    pub(crate) fn any(&self) -> bool {
        !self.entries.is_empty()
    }
    pub(super) fn call(
        &mut self,
        memory: &mut Memory,
        id: crate::win32::catalog::ExceptionApi,
        args: &[u64; 16],
    ) -> Result<u64, StartupError> {
        use crate::win32::catalog::ExceptionApi as E;
        match id {
            E::AddTable => {
                if self.entries.len() >= 128
                    || self.entries.contains_key(&args[0])
                    || !valid_array(memory, args[0], args[1] as u32, args[2])?
                {
                    return Ok(0);
                }
                self.entries.insert(
                    args[0],
                    Table::Array {
                        count: args[1] as u32,
                        base: args[2],
                    },
                );
                Ok(1)
            }
            E::DeleteTable => Ok(u64::from(self.entries.remove(&args[0]).is_some())),
            E::InstallCallback => {
                if args[0] & 3 != 3
                    || args[2] == 0
                    || args[3] == 0
                    || args[1].checked_add(args[2]).is_none_or(|v| v > 1 << 47)
                    || self.entries.len() >= 128
                    || self.entries.contains_key(&args[0])
                {
                    return Ok(0);
                }
                let name = if args[5] == 0 {
                    None
                } else {
                    let mut units = Vec::new();
                    let mut terminated = false;
                    for n in 0..32768 {
                        let unit = memory.read_int(add(args[5], n * 2)?, 2)? as u16;
                        if unit == 0 {
                            terminated = true;
                            break;
                        }
                        units.push(unit);
                    }
                    if !terminated {
                        return Err(invalid("AMD64 out-of-process table name is unterminated"));
                    }
                    Some(
                        String::from_utf16(&units)
                            .map_err(|_| invalid("invalid AMD64 out-of-process table name"))?,
                    )
                };
                self.entries.insert(
                    args[0],
                    Table::Callback {
                        base: args[1],
                        length: args[2] as u32,
                        target: args[3],
                        context: args[4],
                        _name: name,
                    },
                );
                Ok(1)
            }
            _ => unreachable!(),
        }
    }
}

#[derive(Clone)]
pub(super) struct Handler {
    pub target: u64,
    pub data: u64,
    pub frame: u64,
    pub control: u64,
    pub base: u64,
    pub entry: u64,
    pub flags: u8,
}
pub(super) struct Step {
    pub caller: State,
    pub frame: u64,
    pub handler: Option<Handler>,
    pub saved: Saved,
}
#[derive(Default)]
pub(super) struct Saved {
    pub integer: [Option<u64>; 16],
    pub float: [Option<u64>; 16],
}
#[derive(Clone, Copy)]
pub(super) enum Stack {
    Thread,
    Supplied,
}
#[derive(Clone)]
pub(super) struct Walk {
    state: Box<State>,
    returned: bool,
    seen: BTreeSet<(u64, u64)>,
    resolved: Option<(u64, Option<Function>)>,
}
pub(super) enum WalkStep {
    End,
    Frame(Box<Step>),
    Lookup(Request),
}
impl Walk {
    pub(super) fn new(state: State, returned: bool) -> Self {
        Self {
            state: Box::new(state),
            returned,
            seen: BTreeSet::new(),
            resolved: None,
        }
    }
    pub(super) fn next(
        &mut self,
        memory: &Memory,
        tables: &Tables,
        modules: &Linked,
    ) -> Result<WalkStep, StartupError> {
        if self.state.ip == 0 || gateways::layout(Mode::X64).exit(self.state.ip).is_some() {
            return Ok(WalkStep::End);
        }
        let control = self.state.ip.saturating_sub(u64::from(self.returned));
        let function = match self.resolved.take() {
            Some((pc, function)) if pc == control => function,
            Some(_) => return Err(invalid("AMD64 function lookup resumed at a different PC")),
            None => match tables.lookup(memory, modules, control)? {
                FunctionLookup::Ready(function) => function,
                FunctionLookup::Callback(request) => return Ok(WalkStep::Lookup(request)),
            },
        };
        if self.seen.len() >= 128 || !self.seen.insert((self.state.ip, self.state.regs[4])) {
            return Err(invalid("AMD64 exception stack walk capacity or cycle"));
        }
        let step = step(memory, &self.state, function, control)?;
        if step.caller.regs[4] <= self.state.regs[4] {
            return Err(invalid("AMD64 unwind did not advance the stack"));
        }
        *self.state = step.caller.clone();
        self.returned = true;
        Ok(WalkStep::Frame(Box::new(step)))
    }
    pub(super) fn resolve(
        &mut self,
        pc: u64,
        function: Option<Function>,
    ) -> Result<(), StartupError> {
        if self.resolved.is_some() || pc != self.state.ip.saturating_sub(u64::from(self.returned)) {
            return Err(invalid("AMD64 function lookup continuation mismatch"));
        }
        self.resolved = Some((pc, function));
        Ok(())
    }
}
#[derive(Clone)]
pub(super) struct Invocation {
    pub handler: Handler,
    pub state: Box<State>,
    pub target: bool,
}
#[derive(Clone)]
pub(super) struct Plan {
    pub calls: VecDeque<Invocation>,
    pub target: Box<State>,
}
#[derive(Clone)]
pub(super) struct Planner {
    walk: Walk,
    calls: VecDeque<Invocation>,
    target: [u64; 3],
    bridges: Vec<Bridge>,
}
#[derive(Clone)]
pub(super) struct Bridge {
    pub sp: u64,
    pub caller: Option<(Box<State>, bool)>,
}
pub(super) enum Planning {
    Ready(Plan),
    Lookup(Request),
}
impl Planner {
    pub(super) fn new(
        initial: State,
        returned: bool,
        target: [u64; 3],
        bridges: Vec<Bridge>,
    ) -> Result<Self, StartupError> {
        if target[1] >= 1 << 47 {
            return Err(invalid("noncanonical AMD64 unwind target PC"));
        }
        Ok(Self {
            walk: Walk::new(initial, returned),
            calls: VecDeque::new(),
            target,
            bridges,
        })
    }
    pub(super) fn resolve(
        &mut self,
        pc: u64,
        function: Option<Function>,
    ) -> Result<(), StartupError> {
        self.walk.resolve(pc, function)
    }
    pub(super) fn advance(
        &mut self,
        memory: &Memory,
        tables: &Tables,
        modules: &Linked,
    ) -> Result<Planning, StartupError> {
        let [target_frame, target_ip, value] = self.target;
        loop {
            if self.walk.state.ip == gateways::layout(Mode::X64).callback {
                if let Some(index) = self
                    .bridges
                    .iter()
                    .position(|b| b.sp == self.walk.state.regs[4])
                {
                    let bridge = self.bridges.remove(index);
                    let (caller, returned) = bridge.caller.ok_or_else(|| {
                        invalid("AMD64 collided unwind through a callback is not implemented")
                    })?;
                    if caller.regs[4] <= bridge.sp {
                        return Err(invalid("AMD64 callback unwind did not advance the stack"));
                    }
                    self.walk.state = caller;
                    self.walk.returned = returned;
                    continue;
                }
            }
            let current = self.walk.state.clone();
            let step = match self.walk.next(memory, tables, modules)? {
                WalkStep::Lookup(request) => return Ok(Planning::Lookup(request)),
                WalkStep::End => {
                    return Err(invalid(
                        "AMD64 unwind target frame is not on the current stack",
                    ))
                }
                WalkStep::Frame(step) => step,
            };
            let target = step.frame == target_frame;
            if let Some(handler) = step.handler.filter(|h| h.flags & 2 != 0) {
                self.calls.push_back(Invocation {
                    handler,
                    state: current.clone(),
                    target,
                });
            }
            if target {
                let mut state = *current;
                state.ip = target_ip;
                state.regs[4] = target_frame;
                state.regs[0] = value;
                state.exception = 0;
                state.cancel_repeat();
                return Ok(Planning::Ready(Plan {
                    calls: std::mem::take(&mut self.calls),
                    target: Box::new(state),
                }));
            }
        }
    }
}
fn stack(
    memory: &Memory,
    state: &State,
    bounds: Stack,
    pointer: u64,
    length: usize,
) -> Result<(), StartupError> {
    if matches!(bounds, Stack::Supplied) {
        return Ok(());
    }
    let top = memory.read_int(state.gs_base + 8, 8)?;
    let limit = memory.read_int(state.gs_base + 16, 8)?;
    if pointer < limit
        || pointer
            .checked_add(length as u64)
            .is_none_or(|end| end > top)
        || pointer & 7 != 0
    {
        return Err(invalid("AMD64 unwind stack is outside the current thread"));
    }
    Ok(())
}
fn read_stack(
    memory: &Memory,
    state: &State,
    bounds: Stack,
    pointer: u64,
) -> Result<u64, StartupError> {
    stack(memory, state, bounds, pointer, 8)?;
    Ok(memory.read_int(pointer, 8)?)
}
fn reg(register: Register) -> Option<usize> {
    Some(match register {
        Register::RAX => 0,
        Register::RCX => 1,
        Register::RDX => 2,
        Register::RBX => 3,
        Register::RSP => 4,
        Register::RBP => 5,
        Register::RSI => 6,
        Register::RDI => 7,
        Register::R8 => 8,
        Register::R9 => 9,
        Register::R10 => 10,
        Register::R11 => 11,
        Register::R12 => 12,
        Register::R13 => 13,
        Register::R14 => 14,
        Register::R15 => 15,
        _ => return None,
    })
}
fn instruction(memory: &Memory, pc: u64) -> Option<Instruction> {
    let mut bytes = Vec::new();
    for n in 0..15 {
        let mut byte = [0];
        if memory.observe(pc.checked_add(n)?, &mut byte).is_err() {
            break;
        }
        bytes.push(byte[0]);
    }
    let i = Decoder::with_ip(64, &bytes, pc, DecoderOptions::NONE).decode();
    (!i.is_invalid()).then_some(i)
}
fn indirect_tail(memory: &Memory, i: &Instruction) -> bool {
    if i.op0_kind() != OpKind::Memory {
        return false;
    }
    let mut bytes = vec![0; i.len()];
    if memory.observe(i.ip(), &mut bytes).is_err() {
        return false;
    }
    let mut at = 0;
    while at < bytes.len()
        && matches!(
            bytes[at],
            0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 | 0x66 | 0x67 | 0xf2 | 0xf3 | 0x40..=0x4f
        )
    {
        at += 1;
    }
    bytes.get(at) == Some(&0xff)
        && bytes
            .get(at + 1)
            .is_some_and(|b| b & 0xc0 == 0 && b & 0x38 == 0x20)
}
fn extra_slots(op: u8, info: u8, version: u8) -> Result<usize, StartupError> {
    Ok(match op {
        0 | 2 | 3 | 10 => 0,
        1 if info == 0 => 1,
        1 if info == 1 => 2,
        4 | 8 => 1,
        5 | 9 => 2,
        6 if version == 2 => 0,
        _ => return Err(invalid("unsupported AMD64 unwind operation")),
    })
}
fn epilogue(
    memory: &Memory,
    original: &State,
    bounds: Stack,
    control: u64,
    begin: u64,
    end: u64,
    frame_register: u8,
) -> Result<Option<(State, Saved)>, StartupError> {
    let mut pc = control;
    let mut instructions = Vec::new();
    let mut first = true;
    let mut finished = false;
    for _ in 0..32 {
        let Some(i) = instruction(memory, pc).filter(|i| i.next_ip() <= end) else {
            return Ok(None);
        };
        let adjustment = first
            && ((i.mnemonic() == M::Add
                && i.op0_register() == Register::RSP
                && matches!(
                    i.op1_kind(),
                    OpKind::Immediate8to64 | OpKind::Immediate32to64
                ))
                || (i.mnemonic() == M::Lea
                    && i.op0_register() == Register::RSP
                    && frame_register != 0
                    && reg(i.memory_base()) == Some(frame_register as usize)
                    && i.memory_index() == Register::None));
        if adjustment
            || (i.mnemonic() == M::Pop
                && i.op0_kind() == OpKind::Register
                && reg(i.op0_register()).is_some_and(|n| n != 4))
        {
            instructions.push(i);
        } else if (i.mnemonic() == M::Ret && i.op_count() <= 1)
            || (i.mnemonic() == M::Jmp
                && ((i.op0_kind() == OpKind::NearBranch64
                    && (i.near_branch_target() < begin || i.near_branch_target() >= end))
                    || indirect_tail(memory, &i)))
        {
            instructions.push(i);
            finished = true;
            break;
        } else {
            return Ok(None);
        }
        first = false;
        pc = i.next_ip();
    }
    if !finished {
        return Ok(None);
    }
    let mut state = original.clone();
    let mut saved = Saved::default();
    for i in instructions {
        match i.mnemonic() {
            M::Add => {
                let amount = if i.op1_kind() == OpKind::Immediate8to64 {
                    i.immediate8to64()
                } else {
                    i.immediate32to64()
                };
                if amount <= 0 {
                    return Ok(None);
                }
                state.regs[4] = add(state.regs[4], amount as u64)?;
            }
            M::Lea => {
                state.regs[4] = state.regs[frame_register as usize]
                    .checked_add_signed(i.memory_displacement64() as i64)
                    .ok_or_else(|| invalid("AMD64 epilogue frame address overflow"))?;
            }
            M::Pop => {
                let value = read_stack(memory, &state, bounds, state.regs[4])?;
                let register = reg(i.op0_register()).unwrap();
                saved.integer[register] = Some(state.regs[4]);
                state.regs[register] = value;
                state.regs[4] = add(state.regs[4], 8)?;
            }
            M::Ret | M::Jmp => {
                state.ip = read_stack(memory, &state, bounds, state.regs[4])?;
                state.regs[4] = add(
                    state.regs[4],
                    8 + if i.mnemonic() == M::Ret && i.op_count() != 0 {
                        u64::from(i.immediate16())
                    } else {
                        0
                    },
                )?;
            }
            _ => unreachable!(),
        }
    }
    Ok(Some((state, saved)))
}

pub(super) fn step(
    memory: &Memory,
    original: &State,
    function: Option<Function>,
    control: u64,
) -> Result<Step, StartupError> {
    step_with_stack(memory, original, function, control, Stack::Thread)
}

pub(super) fn step_with_stack(
    memory: &Memory,
    original: &State,
    function: Option<Function>,
    control: u64,
    bounds: Stack,
) -> Result<Step, StartupError> {
    stack(memory, original, bounds, original.regs[4], 0)?;
    let mut saved = Saved::default();
    let Some(function) = function else {
        let mut caller = original.clone();
        caller.ip = read_stack(memory, original, bounds, caller.regs[4])?;
        caller.regs[4] = add(caller.regs[4], 8)?;
        return Ok(Step {
            caller,
            frame: original.regs[4],
            handler: None,
            saved,
        });
    };
    let begin = add(function.base, memory.read_int(function.entry, 4)?)?;
    let end = add(function.base, memory.read_int(add(function.entry, 4)?, 4)?)?;
    if begin >= end || control < begin || control >= end {
        return Err(invalid(
            "AMD64 unwind entry does not contain the control PC",
        ));
    }
    let mut state = original.clone();
    let mut frame = original.regs[4];
    let mut entry = function.entry;
    let mut seen = BTreeSet::new();
    let mut chained = false;
    let mut machine_frame = false;
    let handler = loop {
        if seen.len() >= 32 || !seen.insert(entry) {
            return Err(invalid("cyclic AMD64 chained unwind metadata"));
        }
        let rva = memory.read_int(add(entry, 8)?, 4)?;
        if rva & 1 != 0 {
            entry = add(function.base, rva & !1)?;
            chained = true;
            continue;
        }
        let at = add(function.base, rva)?;
        let mut header = [0; 4];
        memory.read(at, &mut header)?;
        let version = header[0] & 7;
        let flags = header[0] >> 3;
        let fp = header[3] & 15;
        let fp_offset = u64::from(header[3] >> 4) * 16;
        if !matches!(version, 1 | 2)
            || flags & !7 != 0
            || (flags & 4 != 0 && flags & 3 != 0)
            || (fp == 0 && fp_offset != 0)
        {
            return Err(invalid("unsupported AMD64 unwind header"));
        }
        if !chained && header[2] != 0 && control - begin >= u64::from(header[1]) {
            if let Some((caller, saved)) =
                epilogue(memory, original, bounds, control, begin, end, fp)?
            {
                if fp != 0 {
                    frame = caller.regs[4]
                        .checked_sub(8)
                        .ok_or_else(|| invalid("AMD64 epilogue frame underflow"))?;
                }
                return Ok(Step {
                    caller,
                    frame,
                    handler: None,
                    saved,
                });
            }
        }
        let count = usize::from(header[2]);
        let mut codes = vec![0; count * 2];
        memory.read(at + 4, &mut codes)?;
        let offset = if chained { u64::MAX } else { control - begin };
        // Saves use the fixed frame, even after earlier codes have advanced RSP.
        // A partial prologue can still contain the caller's frame register.
        let mut set_fp_executed = offset >= u64::from(header[1]);
        let mut scan = 0;
        while scan < count {
            let token = codes[scan * 2 + 1];
            if token & 15 == 3 && u64::from(codes[scan * 2]) <= offset {
                set_fp_executed = true;
            }
            scan += 1 + extra_slots(token & 15, token >> 4, version)?;
        }
        if fp != 0 && set_fp_executed {
            frame = original.regs[fp as usize]
                .checked_sub(fp_offset)
                .ok_or_else(|| invalid("AMD64 establisher frame underflow"))?;
        }
        let mut slot = 0;
        let mut previous = u8::MAX;
        while slot < count {
            let code_offset = codes[slot * 2];
            let token = codes[slot * 2 + 1];
            let op = token & 15;
            let info = token >> 4;
            let extra = extra_slots(op, info, version)?;
            if slot + extra >= count
                || (op != 6 && (code_offset > header[1] || code_offset > previous))
            {
                return Err(invalid("malformed AMD64 unwind code sequence"));
            }
            if op != 6 {
                previous = code_offset;
            }
            let operand = if extra == 1 {
                u64::from(u16::from_le_bytes(
                    codes[(slot + 1) * 2..(slot + 2) * 2].try_into().unwrap(),
                ))
            } else if extra == 2 {
                u64::from(u32::from_le_bytes(
                    codes[(slot + 1) * 2..(slot + 3) * 2].try_into().unwrap(),
                ))
            } else {
                0
            };
            if u64::from(code_offset) <= offset && op != 6 {
                match op {
                    0 if info != 4 => {
                        state.regs[info as usize] =
                            read_stack(memory, &state, bounds, state.regs[4])?;
                        saved.integer[info as usize] = Some(state.regs[4]);
                        state.regs[4] = add(state.regs[4], 8)?;
                    }
                    1 | 2 => {
                        let amount = if op == 2 {
                            u64::from(info) * 8 + 8
                        } else if info == 0 {
                            operand * 8
                        } else {
                            operand
                        };
                        if amount == 0 || amount & 7 != 0 {
                            return Err(invalid("invalid AMD64 unwind allocation size"));
                        }
                        state.regs[4] = add(state.regs[4], amount)?;
                    }
                    3 if info == 0 && fp != 0 => {
                        state.regs[4] = state.regs[fp as usize]
                            .checked_sub(fp_offset)
                            .ok_or_else(|| invalid("AMD64 unwind frame underflow"))?;
                    }
                    4 | 5 if info != 4 => {
                        let pointer = add(frame, operand * if op == 4 { 8 } else { 1 })?;
                        state.regs[info as usize] = read_stack(memory, &state, bounds, pointer)?;
                        saved.integer[info as usize] = Some(pointer);
                    }
                    8 | 9 => {
                        let pointer = add(frame, operand * if op == 8 { 16 } else { 1 })?;
                        stack(memory, &state, bounds, pointer, 16)?;
                        memory.read(pointer, &mut state.xmm[info as usize])?;
                        saved.float[info as usize] = Some(pointer);
                    }
                    10 if info <= 1 => {
                        let pointer = add(state.regs[4], u64::from(info) * 8)?;
                        stack(memory, &state, bounds, pointer, 40)?;
                        state.ip = memory.read_int(pointer, 8)?;
                        if matches!(bounds, Stack::Thread) {
                            state.flags = memory.read_int(add(pointer, 16)?, 8)?;
                        }
                        state.regs[4] = memory.read_int(add(pointer, 24)?, 8)?;
                        machine_frame = true;
                    }
                    _ => return Err(invalid("invalid AMD64 unwind register or operation")),
                }
            }
            slot += extra + 1;
        }
        if flags & 4 != 0 {
            entry = at + 4 + (count as u64).div_ceil(2) * 4;
            chained = true;
            continue;
        }
        let handler = if flags & 3 != 0 && offset >= u64::from(header[1]) {
            let pointer = at + 4 + (count as u64).div_ceil(2) * 4;
            Some(Handler {
                target: add(function.base, memory.read_int(pointer, 4)?)?,
                data: pointer + 4,
                frame,
                control,
                base: function.base,
                entry: function.entry,
                flags,
            })
        } else {
            None
        };
        break handler;
    };
    stack(memory, original, bounds, frame, 0)?;
    if !machine_frame {
        state.ip = read_stack(memory, &state, bounds, state.regs[4])?;
        state.regs[4] = add(state.regs[4], 8)?;
    }
    if matches!(bounds, Stack::Thread) && state.ip >= 1 << 47 {
        return Err(invalid("noncanonical AMD64 unwind return PC"));
    }
    stack(memory, &state, bounds, state.regs[4], 0)?;
    Ok(Step {
        caller: state,
        frame,
        handler,
        saved,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Memory, State, Function) {
        let mut memory = Memory::new(Mode::X64);
        memory.map(0x10000, 0x10000, 7).unwrap();
        memory.map(0x80000, 4096, 3).unwrap();
        memory.map(0x90000, 65536, 3).unwrap();
        memory.write_int(0x80008, 8, 0xa0000).unwrap();
        memory.write_int(0x80010, 8, 0x90000).unwrap();
        memory.write(0x11000, &[0x90; 256]).unwrap();
        for (at, value) in [(0x10200, 0x1000), (0x10204, 0x1100), (0x10208, 0x300)] {
            memory.write_int(at, 4, value).unwrap();
        }
        let mut state = State::new(Mode::X64, 0x11020, 0x98000);
        state.gs_base = 0x80000;
        (
            memory,
            state,
            Function {
                base: 0x10000,
                entry: 0x10200,
            },
        )
    }
    #[test]
    fn partial_prologue_only_reverses_completed_operations() {
        let (mut memory, mut state, function) = fixture();
        memory
            .write(0x10300, &[1, 5, 2, 0, 5, 0x42, 1, 0x30])
            .unwrap();
        memory.write_int(0x98000, 8, 0x13000).unwrap();
        memory.write_int(0x97ff8, 8, 73).unwrap();
        for (offset, sp, bx) in [(0, 0x98000, 99), (1, 0x97ff8, 73), (5, 0x97fd0, 73)] {
            state.ip = 0x11000 + offset;
            state.regs[4] = sp;
            state.regs[3] = 99;
            let result = step(&memory, &state, Some(function), state.ip).unwrap();
            assert_eq!(
                (
                    result.caller.ip,
                    result.caller.regs[4],
                    result.caller.regs[3]
                ),
                (0x13000, 0x98008, bx)
            );
        }
    }
    #[test]
    fn epilogue_progress_and_return_address_adjustment_preserve_handler_selection() {
        let (mut memory, mut state, function) = fixture();
        memory
            .write(0x10300, &[9, 5, 2, 0, 5, 0x42, 1, 0x30])
            .unwrap();
        memory.write_int(0x10308, 4, 0x1200).unwrap();
        memory
            .write(0x11050, &[0x48, 0x83, 0xc4, 40, 0x5b, 0xc3])
            .unwrap();
        memory.write_int(0x98000, 8, 0x13000).unwrap();
        memory.write_int(0x97ff8, 8, 73).unwrap();
        for (offset, sp, bx) in [
            (0x50, 0x97fd0, 99),
            (0x54, 0x97ff8, 99),
            (0x55, 0x98000, 73),
        ] {
            state.ip = 0x11000 + offset;
            state.regs[4] = sp;
            state.regs[3] = bx;
            let result = step(&memory, &state, Some(function), state.ip).unwrap();
            assert!(result.handler.is_none());
            assert_eq!(
                (
                    result.caller.ip,
                    result.caller.regs[4],
                    result.caller.regs[3]
                ),
                (0x13000, 0x98008, 73)
            );
        }
        state.ip = 0x11050;
        state.regs[4] = 0x97fd0;
        assert!(step(&memory, &state, Some(function), 0x1104f)
            .unwrap()
            .handler
            .is_some());
        memory
            .write(0x10300, &[10, 5, 3, 0, 6, 0x16, 5, 0x42, 1, 0x30])
            .unwrap();
        memory.write_int(0x1030c, 4, 0x1200).unwrap();
        memory
            .write(0x110fa, &[0x48, 0x83, 0xc4, 40, 0x5b, 0xc3])
            .unwrap();
        state.ip = 0x110fa;
        state.regs[4] = 0x97fd0;
        let version_two = step(&memory, &state, Some(function), state.ip).unwrap();
        assert!(version_two.handler.is_none());
        assert_eq!(version_two.caller.regs[4], 0x98008);
    }
    #[test]
    fn chained_unwind_restores_saved_gpr_xmm_and_parent_handler() {
        let (mut memory, mut state, function) = fixture();
        state.regs[4] = 0x97000;
        memory
            .write(0x10300, &[0x21, 5, 4, 0, 5, 0x68, 4, 0, 3, 0xc4, 2, 0])
            .unwrap();
        for (at, value) in [
            (0x1030c, 0x1000),
            (0x10310, 0x1100),
            (0x10314, 0x350),
            (0x1035c, 0x1200),
        ] {
            memory.write_int(at, 4, value).unwrap();
        }
        memory
            .write(0x10350, &[9, 5, 3, 0, 5, 1, 32, 0, 1, 0x30])
            .unwrap();
        memory.write_int(0x97010, 8, 0x12345678).unwrap();
        memory.write(0x97040, &[0xa5; 16]).unwrap();
        memory.write_int(0x97100, 8, 73).unwrap();
        memory.write_int(0x97108, 8, 0x13000).unwrap();
        let result = step(&memory, &state, Some(function), state.ip).unwrap();
        assert_eq!(
            (
                result.caller.ip,
                result.caller.regs[4],
                result.caller.regs[3],
                result.caller.regs[12]
            ),
            (0x13000, 0x97110, 73, 0x12345678)
        );
        assert_eq!(result.caller.xmm[6], [0xa5; 16]);
        let handler = result.handler.unwrap();
        assert_eq!(
            (handler.target, handler.data, handler.frame),
            (0x11200, 0x10360, 0x97000)
        );
        memory.write_int(0x10208, 4, 0x381).unwrap();
        for (at, value) in [(0x10380, 0x900), (0x10384, 0x1100), (0x10388, 0x350)] {
            memory.write_int(at, 4, value).unwrap();
        }
        state.ip = 0x11000;
        let indirect = step(&memory, &state, Some(function), state.ip).unwrap();
        assert_eq!(
            (
                indirect.caller.ip,
                indirect.caller.regs[4],
                indirect.caller.regs[3]
            ),
            (0x13000, 0x97110, 73)
        );
        assert!(indirect.handler.is_some());
    }
    #[test]
    fn far_saves_and_unscaled_large_allocation_use_full_operands() {
        let (mut memory, mut state, function) = fixture();
        state.regs[4] = 0x94000;
        memory
            .write(
                0x10300,
                &[
                    1, 6, 9, 0, 6, 0x79, 0x10, 0x10, 0, 0, 4, 0xd5, 0, 0x10, 0, 0, 2, 0x11, 8,
                    0x20, 0, 0,
                ],
            )
            .unwrap();
        memory.write_int(0x95000, 8, 0x123456789abcdef0).unwrap();
        memory.write(0x95010, &[0x5a; 16]).unwrap();
        memory.write_int(0x96008, 8, 0x13000).unwrap();
        let result = step(&memory, &state, Some(function), state.ip).unwrap();
        assert_eq!(
            (
                result.caller.ip,
                result.caller.regs[4],
                result.caller.regs[13]
            ),
            (0x13000, 0x96010, 0x123456789abcdef0)
        );
        assert_eq!(result.caller.xmm[7], [0x5a; 16]);
    }
    #[test]
    fn machine_frame_restores_its_saved_pc_flags_and_stack_with_optional_error_code() {
        let (mut memory, mut state, function) = fixture();
        state.regs[4] = 0x97000;
        for error in [false, true] {
            memory
                .write(0x10300, &[1, 0, 1, 0, 0, if error { 0x1a } else { 0x0a }])
                .unwrap();
            let frame = 0x97000 + u64::from(error) * 8;
            for (offset, value) in [
                (0, 0x13000),
                (8, 0x33),
                (16, 0x247),
                (24, 0x98000),
                (32, 0x2b),
            ] {
                memory.write_int(frame + offset, 8, value).unwrap();
            }
            let result = step(&memory, &state, Some(function), state.ip).unwrap();
            assert_eq!(
                (result.caller.ip, result.caller.flags, result.caller.regs[4]),
                (0x13000, 0x247, 0x98000)
            );
        }
    }
}
