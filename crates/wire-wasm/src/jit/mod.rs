//! Direct x86-to-Wasm compilation. Decoded instructions and CFG data are
//! discarded after emission; Compiled retains only executable bytes and proofs.
use crate::{
    machine::{Mode, State},
    memory::{CodeProof, Fault, Memory, PAGE},
};
use binary::Code;
use iced_x86::{Decoder, DecoderError, DecoderOptions, FlowControl, Instruction};
use std::{
    collections::{BTreeMap, BTreeSet},
    mem::offset_of,
};

mod access;
mod binary;
mod bits;
pub mod cache;
mod control;
mod driver;
mod exchange;
mod flags;
pub mod links;
mod multiply;
mod rotate;
mod scalar;
mod shift;
mod stack;
mod state;
mod strings;
mod system;
mod vector;

const INSTRUCTION_LIMIT: usize = 256;
const PAGE_LIMIT: usize = 8;
const POSITION_BITS: u32 = INSTRUCTION_LIMIT.next_power_of_two().trailing_zeros();
const POSITION_MASK: u32 = (1 << POSITION_BITS) - 1;

const GPR: u32 = 4;
const FLAGS: u32 = 20;
const IP: u32 = 21;
const WORK: u32 = 22;
const RETIRED: u32 = 23;
const LEFT: u32 = 24;
const RIGHT: u32 = 25;
const VALUE: u32 = 26;
const SIGN: u32 = 27;
const TAG: u32 = 28;
const A: u32 = 29;
const B: u32 = 30;
const RESULT: u32 = 31;
const ADDRESS: u32 = 32;
const COUNT: u32 = 33;
const EXTRA: u32 = 34;
const STATUS: u32 = 35;
const ENTRY: u32 = 36;
const TRANSLATION: u32 = 37;
const POINTER: u32 = 38;
const CHANGED: u32 = 39;
const RESUME: u32 = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Exit {
    Dispatch = 0,
    Budget = 1,
    Unsupported = 2,
    Fault = 3,
    SourceChanged = 4,
    Exception = 5,
    HostError = 6,
    Service = 7,
    Returned = 8,
    Callback = 9,
    ThreadReturned = 10,
    Waiting = 11,
    ProcessExited = 12,
}

pub struct Compiled {
    pub entry: u64,
    pub bytes: Vec<u8>,
    proofs: Vec<CodeProof>,
    entries: Vec<u64>,
}

impl Compiled {
    /// Recheck after asynchronous module installation and before every run.
    /// The generated function checks membership in its published entry set.
    pub fn valid(&self, memory: &Memory) -> bool {
        !self.proofs.is_empty() && self.proofs.iter().all(|proof| memory.code_valid(proof))
    }

    pub fn source_pages(&self) -> usize {
        self.proofs.len()
    }

    pub fn entries(&self) -> &[u64] {
        &self.entries
    }
    pub fn can_enter(&self, pc: u64) -> bool {
        self.entries.binary_search(&pc).is_ok()
    }

    pub fn storage_charge(&self) -> usize {
        // Includes conservative allowances for cache and alias tree nodes,
        // in addition to each retained allocation's capacity.
        std::mem::size_of::<Self>()
            + self.bytes.capacity()
            + self.proofs.capacity() * std::mem::size_of::<CodeProof>()
            + self.entries.capacity() * std::mem::size_of::<u64>()
            + 512
            + self.entries.len() * 64
    }
}

#[derive(Debug)]
pub enum CompileError {
    Fetch(Fault),
    InvalidInstruction(u64),
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fetch(fault) => fault.fmt(f),
            Self::InvalidInstruction(pc) => write!(f, "invalid instruction at {pc:#x}"),
        }
    }
}
impl std::error::Error for CompileError {}

struct Source {
    pages: BTreeMap<u64, (Box<[u8; PAGE as usize]>, CodeProof)>,
}

impl Source {
    fn fp_opcode(&self, instruction: &Instruction) -> u32 {
        let words = self.identity(instruction);
        let bytes = u128::from(words[0]) | (u128::from(words[1]) << 64);
        for index in 0..instruction.len().saturating_sub(1) {
            let first = (bytes >> (index * 8)) as u8;
            if (0xd8..=0xdf).contains(&first) {
                return (u32::from(first & 7) << 8) | ((bytes >> ((index + 1) * 8)) as u8 as u32);
            }
        }
        0
    }
    fn identity(&self, instruction: &Instruction) -> [u64; 2] {
        let mut bytes = [0; 16];
        for (offset, byte) in bytes[..instruction.len()].iter_mut().enumerate() {
            let address = instruction.ip().wrapping_add(offset as u64);
            *byte = self.pages[&(address & !(PAGE - 1))].0[(address & (PAGE - 1)) as usize];
        }
        bytes[15] = instruction.len() as u8;
        [
            u64::from_le_bytes(bytes[..8].try_into().unwrap()),
            u64::from_le_bytes(bytes[8..].try_into().unwrap()),
        ]
    }

    fn page(
        &mut self,
        memory: &mut Memory,
        base: u64,
    ) -> Result<&[u8; PAGE as usize], Option<CompileError>> {
        if !self.pages.contains_key(&base) {
            if self.pages.len() == PAGE_LIMIT {
                return Err(None);
            }
            let mut bytes = Box::new([0; PAGE as usize]);
            let proof = memory
                .snapshot_code_page(base, &mut bytes)
                .map_err(|fault| Some(CompileError::Fetch(fault)))?;
            self.pages.insert(base, (bytes, proof));
        }
        Ok(&self.pages[&base].0)
    }

    fn decode(
        &mut self,
        memory: &mut Memory,
        pc: u64,
    ) -> Result<Instruction, Option<CompileError>> {
        let boundary = crate::win32::gateways::layout(memory.mode()).first(pc, 15);
        let limit = boundary.map_or(15, |address| (address - pc) as usize);
        if limit == 0 {
            return Err(Some(CompileError::Fetch(boundary_fault(pc))));
        }
        let base = pc & !(PAGE - 1);
        let offset = (pc - base) as usize;
        let bytes = self.page(memory, base)?;
        let mut decoder = Decoder::with_ip(
            memory.mode() as u32,
            &bytes[offset..(offset + limit).min(PAGE as usize)],
            pc,
            DecoderOptions::NONE,
        );
        let mut instruction = decoder.decode();
        let mut error = decoder.last_error();
        if error == DecoderError::NoMoreBytes && PAGE as usize - offset < limit {
            let prefix = PAGE as usize - offset;
            let mut window = [0; 15];
            window[..prefix].copy_from_slice(&bytes[offset..]);
            window[prefix..limit]
                .copy_from_slice(&self.page(memory, base + PAGE)?[..limit - prefix]);
            let mut decoder = Decoder::with_ip(
                memory.mode() as u32,
                &window[..limit],
                pc,
                DecoderOptions::NONE,
            );
            instruction = decoder.decode();
            error = decoder.last_error();
        }
        if error == DecoderError::NoMoreBytes {
            if let Some(address) = boundary {
                return Err(Some(CompileError::Fetch(boundary_fault(address))));
            }
        }
        if instruction.is_invalid() {
            Err(Some(CompileError::InvalidInstruction(pc)))
        } else {
            Ok(instruction)
        }
    }
}

fn boundary_fault(address: u64) -> Fault {
    Fault {
        address,
        length: 1,
        access: crate::memory::EXECUTE,
        kind: crate::memory::FaultKind::Protection,
    }
}

fn next(mode: Mode, instruction: &Instruction) -> u64 {
    mode.address(instruction.next_ip())
}

fn supported(instruction: &Instruction) -> bool {
    scalar::supported(instruction)
        || control::supported(instruction)
        || multiply::supported(instruction)
        || bits::supported(instruction)
        || shift::double_supported(instruction)
        || exchange::supported(instruction)
        || strings::supported(instruction)
        || stack::supported(instruction)
        || vector::supported(instruction)
        || system::supported(instruction)
}

fn memory_access(instruction: &Instruction) -> u8 {
    if system::supported(instruction) {
        system::memory_access(instruction)
    } else if vector::supported(instruction) {
        vector::memory_access(instruction)
    } else if stack::supported(instruction) {
        stack::memory_access(instruction)
    } else if strings::supported(instruction) {
        strings::memory_access(instruction)
    } else if exchange::supported(instruction) {
        exchange::memory_access(instruction)
    } else if shift::double_supported(instruction) {
        shift::double_access(instruction)
    } else if bits::supported(instruction) {
        bits::memory_access(instruction)
    } else if multiply::supported(instruction) {
        multiply::memory_access(instruction)
    } else if control::supported(instruction) {
        control::memory_access(instruction)
    } else {
        scalar::memory_access(instruction)
    }
}

fn successors(mode: Mode, instruction: &Instruction) -> Vec<u64> {
    if !supported(instruction) {
        return Vec::new();
    }
    match instruction.flow_control() {
        FlowControl::Next => vec![next(mode, instruction)],
        FlowControl::UnconditionalBranch => vec![mode.address(instruction.near_branch_target())],
        FlowControl::ConditionalBranch => vec![
            next(mode, instruction),
            mode.address(instruction.near_branch_target()),
        ],
        FlowControl::Call => vec![
            next(mode, instruction),
            mode.address(instruction.near_branch_target()),
        ],
        FlowControl::IndirectCall => vec![next(mode, instruction)],
        _ => Vec::new(),
    }
}

struct Block {
    instructions: Vec<Instruction>,
}

fn blocks(mode: Mode, entry: u64, instructions: BTreeMap<u64, Instruction>) -> Vec<Block> {
    let mut leaders = BTreeSet::from([entry]);
    let mut predecessors = BTreeMap::<u64, usize>::new();
    for instruction in instructions.values() {
        for target in successors(mode, instruction) {
            *predecessors.entry(target).or_default() += 1;
            if instruction.flow_control() != FlowControl::Next {
                leaders.insert(target);
            }
        }
    }
    leaders.extend(
        predecessors
            .into_iter()
            .filter_map(|(pc, count)| (count > 1).then_some(pc)),
    );
    let mut blocks: Vec<Block> = Vec::new();
    for instruction in instructions.into_values() {
        if blocks.last().is_none_or(|block| {
            let previous = block.instructions.last().unwrap();
            leaders.contains(&instruction.ip())
                || next(mode, previous) != instruction.ip()
                || previous.flow_control() != FlowControl::Next
                || !supported(previous)
        }) {
            blocks.push(Block {
                instructions: Vec::new(),
            });
        }
        blocks.last_mut().unwrap().instructions.push(instruction);
    }
    blocks
}

fn exit(c: &mut Code, label: u32, pc: u64, reason: Exit) {
    c.i64(pc);
    c.set(IP);
    c.i32(reason as u32);
    c.set(STATUS);
    c.branch(label);
}

fn exit_dynamic(c: &mut Code, label: u32, reason: Exit) {
    c.i32(reason as u32);
    c.set(STATUS);
    c.branch(label);
}

fn exception(c: &mut Code, label: u32, pc: u64, vector: u32) {
    c.get(0);
    c.i32(vector + 1);
    c.memory(0x36, offset_of!(State, exception));
    exit(c, label, pc, Exit::Exception);
}

fn entry_resolver(entries: &BTreeMap<u64, usize>) -> Code {
    fn tree(c: &mut Code, entries: &[(u64, usize)]) {
        if entries.is_empty() {
            c.i32(u32::MAX);
            c.op(0x0f);
            return;
        }
        let middle = entries.len() / 2;
        let (pc, index) = entries[middle];
        c.get(0);
        c.i64(pc);
        c.op(0x51);
        c.if_void();
        c.i32(index as u32);
        c.op(0x0f);
        c.end();
        c.get(0);
        c.i64(pc);
        c.op(0x54);
        c.if_void();
        tree(c, &entries[..middle]);
        c.otherwise();
        tree(c, &entries[middle + 1..]);
        c.end();
    }
    let mut c = Code::default();
    tree(
        &mut c,
        &entries
            .iter()
            .map(|(&pc, &index)| (pc, index))
            .collect::<Vec<_>>(),
    );
    c.i32(u32::MAX);
    let first = *entries.first_key_value().expect("compiled entry").0;
    let span = entries.last_key_value().unwrap().0 - first;
    // A table needs at least one encoded label per byte. Do not allocate it
    // when that lower bound already exceeds the complete sparse encoding.
    if span >= c.bytes.len() as u64 {
        return c;
    }
    let mut table = Code::default();
    table.get(0);
    table.i64(first);
    table.op(0x7d);
    table.set(0);
    table.get(0);
    table.i64(span);
    table.op(0x56);
    table.if_void();
    table.i32(u32::MAX);
    table.op(0x0f);
    table.end();
    let missing = table.block();
    let mut labels = vec![0; entries.len()];
    for label in labels.iter_mut().rev() {
        *label = table.block();
    }
    let mut destinations = vec![missing; span as usize + 1];
    for (index, &pc) in entries.keys().enumerate() {
        destinations[(pc - first) as usize] = labels[index];
    }
    table.get(0);
    table.op(0xa7);
    table.table_default(&destinations, missing);
    for &position in entries.values() {
        table.end();
        table.i32(position as u32);
        table.op(0x0f);
    }
    table.end();
    table.i32(u32::MAX);
    if table.bytes.len() < c.bytes.len() {
        table
    } else {
        c
    }
}

struct Flow<'a> {
    mode: Mode,
    source: &'a Source,
    blocks: &'a [Block],
    entries: &'a BTreeMap<u64, usize>,
    labels: Vec<u32>,
    backwards: BTreeMap<usize, u32>,
    dispatch: Option<u32>,
    resolver: u32,
    flags: u32,
    numeric: u32,
    crc: u32,
    crc_mask: u8,
    clock: u32,
    vectors: vector::Bank,
    floating: vector::floating::Calls,
    fp: vector::x87::Calls,
    out: u32,
}

impl Flow<'_> {
    fn dynamic(&self, c: &mut Code) {
        c.get(IP);
        c.call(self.resolver);
        c.set(TRANSLATION);
        c.get(TRANSLATION);
        c.i32(u32::MAX);
        c.op(0x46);
        c.if_void();
        exit_dynamic(c, self.out, Exit::Dispatch);
        c.end();
        resume_position(c);
        // Interior dynamic entries bypass a statically known producer. Keep
        // registers in locals while making their actual flags unambiguous.
        c.get(RESUME);
        c.if_void();
        flags::materialize(c, self.flags);
        flags::discard(c);
        c.end();
        c.branch(self.dispatch.expect("dynamic CFG loop"));
    }
    fn transfer(&self, c: &mut Code, pc: u64, current: usize) {
        if let Some(reason) = crate::win32::gateways::layout(self.mode).exit(pc) {
            c.i64(pc);
            c.set(IP);
            c.get(WORK);
            c.get(1);
            c.op(0x5a);
            c.if_i32();
            c.i32(Exit::Budget as u32);
            c.otherwise();
            c.i32(reason as u32);
            c.end();
            c.set(STATUS);
            c.branch(self.out);
            return;
        }
        match self.entries.get(&pc).copied() {
            Some(index) if index > current => c.branch(self.labels[index]),
            Some(index) => {
                if let Some(back) = self.dispatch {
                    c.i32(index as u32);
                    c.set(ENTRY);
                    c.i32(0);
                    c.set(RESUME);
                    c.branch(back);
                } else {
                    c.branch(self.backwards[&index]);
                }
            }
            None => exit(c, self.out, pc, Exit::Dispatch),
        }
    }

    fn block(&self, c: &mut Code, index: usize) {
        let block = &self.blocks[index];
        let mut labels = vec![0; block.instructions.len()];
        for label in labels.iter_mut().rev() {
            *label = c.block();
        }
        c.get(ENTRY);
        c.i32(u32::MAX);
        c.op(0x47);
        c.if_void();
        c.i32(u32::MAX);
        c.set(ENTRY);
        c.get(RESUME);
        c.table(&labels);
        c.end();
        let mut recipe = None;
        for instruction in &block.instructions {
            c.end();
            c.get(WORK);
            c.get(1);
            c.op(0x5a);
            c.if_void();
            exit(c, self.out, instruction.ip(), Exit::Budget);
            c.end();
            if !supported(instruction) {
                exit(c, self.out, instruction.ip(), Exit::Unsupported);
                continue;
            }
            let context = access::Context {
                mode: self.mode,
                out: self.out,
                flags: self.flags,
            };
            if strings::supported(instruction) {
                strings::emit(c, instruction, &mut recipe, context, self.source);
                continue;
            }
            control::check_direct(c, instruction, context, recipe);
            if system::supported(instruction) {
                system::emit(c, instruction, context, self.clock);
            } else if vector::supported(instruction) {
                vector::emit(
                    c,
                    instruction,
                    &mut recipe,
                    context,
                    self.vectors,
                    self.floating,
                    vector::x87::Calls {
                        opcode: self.source.fp_opcode(instruction),
                        ..self.fp
                    },
                );
            } else if stack::supported(instruction) {
                stack::emit(c, instruction, &mut recipe, context);
            } else if control::supported(instruction) {
                control::emit(c, instruction, context);
            } else if multiply::supported(instruction) {
                multiply::emit(c, instruction, &mut recipe, context, self.numeric);
            } else if bits::supported(instruction) {
                bits::emit(
                    c,
                    instruction,
                    &mut recipe,
                    context,
                    self.crc,
                    self.crc_mask,
                );
            } else if shift::double_supported(instruction) {
                shift::emit_double(c, instruction, &mut recipe, context);
            } else if exchange::supported(instruction) {
                exchange::emit(c, instruction, &mut recipe, context);
            } else if instruction.flow_control() == FlowControl::Next {
                scalar::emit(c, instruction, &mut recipe, context);
            }
            for counter in [WORK, RETIRED] {
                c.get(counter);
                c.i64(1);
                c.op(0x7c);
                c.set(counter);
            }
            if memory_access(instruction) & crate::memory::WRITE != 0 {
                c.get(CHANGED);
                c.if_void();
                if control::changes_ip(instruction) {
                    exit_dynamic(c, self.out, Exit::SourceChanged);
                } else {
                    exit(
                        c,
                        self.out,
                        next(self.mode, instruction),
                        Exit::SourceChanged,
                    );
                }
                c.end();
            }
            match instruction.flow_control() {
                FlowControl::UnconditionalBranch => self.transfer(
                    c,
                    self.mode.address(instruction.near_branch_target()),
                    index,
                ),
                FlowControl::Call => self.transfer(
                    c,
                    self.mode.address(instruction.near_branch_target()),
                    index,
                ),
                FlowControl::IndirectCall | FlowControl::IndirectBranch | FlowControl::Return => {
                    self.dynamic(c)
                }
                FlowControl::ConditionalBranch => {
                    control::condition(c, instruction, recipe, false);
                    c.if_void();
                    self.transfer(
                        c,
                        self.mode.address(instruction.near_branch_target()),
                        index,
                    );
                    c.end();
                    self.transfer(c, next(self.mode, instruction), index);
                }
                _ => {}
            }
        }
        let last = block.instructions.last().unwrap();
        if last.flow_control() == FlowControl::Next {
            self.transfer(c, next(self.mode, last), index);
        }
    }

    fn range(
        &mut self,
        c: &mut Code,
        start: usize,
        end: usize,
        own_header: Option<usize>,
        loops: &BTreeMap<usize, usize>,
    ) {
        let mut units = Vec::new();
        let mut index = start;
        while index < end {
            let tail = loops
                .get(&index)
                .copied()
                .filter(|_| Some(index) != own_header);
            units.push((index, tail));
            index = tail.map_or(index + 1, |tail| tail + 1);
        }
        for &(index, _) in units.iter().rev() {
            self.labels[index] = c.block();
        }
        c.get(ENTRY);
        c.i32(u32::MAX);
        c.op(0x47);
        c.if_void();
        c.get(ENTRY);
        c.i32(start as u32);
        c.op(0x6b);
        let destinations: Vec<_> = units
            .iter()
            .flat_map(|&(index, tail)| {
                std::iter::repeat_n(self.labels[index], tail.map_or(1, |tail| tail - index + 1))
            })
            .collect();
        c.table(&destinations);
        c.end();
        for (index, tail) in units {
            c.end();
            if let Some(tail) = tail {
                self.backwards.insert(index, c.open(0x03, 0x40));
                self.range(c, index, tail + 1, Some(index), loops);
                c.end();
            } else {
                self.block(c, index);
            }
        }
    }
}

fn resume_position(c: &mut Code) {
    c.get(TRANSLATION);
    c.i32(POSITION_BITS);
    c.op(0x76);
    c.set(ENTRY);
    c.get(TRANSLATION);
    c.i32(POSITION_MASK);
    c.op(0x71);
    c.set(RESUME);
}

// Address order admits lexical loops when their intervals nest and all
// incoming edges use the header. Other CFGs keep locals in a shared Wasm
// loop with a table only on backwards edges; no guest dispatch helper runs.
fn lexical_loops(
    mode: Mode,
    blocks: &[Block],
    entries: &BTreeMap<u64, usize>,
    initial: usize,
) -> Option<BTreeMap<usize, usize>> {
    if initial != 0 {
        return None;
    }
    let mut loops = BTreeMap::<usize, usize>::new();
    for (index, block) in blocks.iter().enumerate() {
        for pc in successors(mode, block.instructions.last().unwrap()) {
            if let Some(&header) = entries.get(&pc).filter(|&&target| target <= index) {
                loops
                    .entry(header)
                    .and_modify(|tail| *tail = (*tail).max(index))
                    .or_insert(index);
            }
        }
    }
    for (&header, &tail) in &loops {
        if header < tail
            && loops
                .range(header + 1..=tail)
                .any(|(_, &inner_tail)| inner_tail > tail)
        {
            return None;
        }
        for (_, block) in blocks
            .iter()
            .enumerate()
            .filter(|(index, _)| *index < header || *index > tail)
        {
            if successors(mode, block.instructions.last().unwrap())
                .iter()
                .any(|pc| {
                    entries
                        .get(pc)
                        .is_some_and(|&target| target > header && target <= tail)
                })
            {
                return None;
            }
        }
    }
    Some(loops)
}

pub fn compile(memory: &mut Memory, entry: u64) -> Result<Compiled, CompileError> {
    compile_until(memory, entry, |_, _| false)
}

pub(crate) fn compile_until(
    memory: &mut Memory,
    entry: u64,
    mut covered: impl FnMut(&Memory, u64) -> bool,
) -> Result<Compiled, CompileError> {
    let mode = memory.mode();
    let mut source = Source {
        pages: BTreeMap::new(),
    };
    let first = source
        .decode(memory, entry)
        .map_err(|error| error.expect("first page has capacity"))?;
    let mut instructions = BTreeMap::from([(entry, first)]);
    let mut pending: BTreeSet<u64> = successors(mode, &first).into_iter().collect();
    while instructions.len() < INSTRUCTION_LIMIT {
        let Some(pc) = pending.pop_first() else {
            break;
        };
        if instructions.contains_key(&pc) || covered(memory, pc) {
            continue;
        }
        if let Ok(instruction) = source.decode(memory, pc) {
            pending.extend(successors(mode, &instruction));
            instructions.insert(pc, instruction);
        }
    }
    let blocks = blocks(mode, entry, instructions);
    let entries: BTreeMap<_, _> = blocks
        .iter()
        .enumerate()
        .map(|(n, block)| (block.instructions[0].ip(), n))
        .collect();
    let initial = entries[&entry];
    let positions: BTreeMap<_, _> = blocks
        .iter()
        .enumerate()
        .flat_map(|(block, contents)| {
            contents
                .instructions
                .iter()
                .enumerate()
                .map(move |(position, instruction)| {
                    (instruction.ip(), (block << POSITION_BITS) | position)
                })
        })
        .collect();
    let loops = if blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .any(control::changes_ip)
    {
        None
    } else {
        lexical_loops(mode, &blocks, &entries, initial)
    };
    let uses_memory = blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .any(|i| supported(i) && (memory_access(i) != 0 || control::needs_context(mode, i)));
    let imports = binary::Imports {
        memory: uses_memory,
        masked: blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .any(vector::masked_store),
        fp_area: blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .any(vector::fp_area),
        numeric: blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .any(multiply::needs_numeric),
        vectors: vector::Bank::collect(&blocks, mode),
        floating: vector::floating::collect(&blocks),
        extended: vector::x87::collect(&blocks),
        crc: bits::crc_mask(&blocks),
        clock: blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .any(system::timestamp),
    };
    let resolver = entry_resolver(&positions);
    let resolver_function = imports.function_count() + 1;
    let mut c = Code::default();
    c.get(IP);
    c.i64(entry);
    c.op(0x51);
    c.if_void();
    c.i32(initial as u32);
    c.set(ENTRY);
    c.otherwise();
    c.get(IP);
    c.call(resolver_function);
    c.set(TRANSLATION);
    c.get(TRANSLATION);
    c.i32(u32::MAX);
    c.op(0x46);
    c.if_void();
    c.i32(Exit::Dispatch as u32);
    c.set(STATUS);
    state::results(&mut c);
    c.op(0x0f);
    c.end();
    resume_position(&mut c);
    c.end();
    imports.vectors.enter(&mut c);
    c.get(RESUME);
    c.if_void();
    flags::materialize(&mut c, imports.flags_base());
    flags::discard(&mut c);
    c.end();
    if uses_memory {
        c.get(2);
        c.i32(0);
        c.memory(0x36, offset_of!(crate::memory::TranslationCache, status));
    }
    let out = c.block();
    let mut flow = Flow {
        mode,
        source: &source,
        blocks: &blocks,
        entries: &entries,
        labels: vec![0; blocks.len()],
        backwards: BTreeMap::new(),
        dispatch: None,
        resolver: resolver_function,
        flags: imports.flags_base(),
        numeric: imports.numeric_base(),
        crc: imports.crc_base(),
        crc_mask: imports.crc,
        clock: imports.clock_base(),
        vectors: imports.vectors,
        floating: vector::floating::Calls {
            mask: imports.floating,
            base: imports.floating_base(),
        },
        fp: vector::x87::Calls {
            area: imports.fp_area_base(),
            base: imports.extended_base(),
            mask: imports.extended,
            opcode: 0,
        },
        out,
    };
    let published_entries = positions.keys().copied().collect();
    if let Some(loops) = loops {
        flow.range(&mut c, 0, blocks.len(), None, &loops);
    } else {
        flow.dispatch = Some(c.open(0x03, 0x40));
        for label in flow.labels.iter_mut().rev() {
            *label = c.block();
        }
        c.get(ENTRY);
        c.table(&flow.labels);
        for index in 0..blocks.len() {
            c.end();
            flow.block(&mut c, index);
        }
        c.end();
    }
    c.end(); // common exit
    imports.vectors.leave(&mut c);
    state::results(&mut c);
    let bytes = c.module(imports, resolver);
    Ok(Compiled {
        entry,
        bytes,
        proofs: source.pages.into_values().map(|(_, proof)| proof).collect(),
        entries: published_entries,
    })
}

pub fn resident_driver() -> Vec<u8> {
    driver::module()
}
