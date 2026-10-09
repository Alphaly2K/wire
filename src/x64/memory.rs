//! Sparse, checked low-canonical AMD64 user address space. Host data is never executable.
use std::{
    cell::{Cell, OnceCell, RefCell},
    collections::{BTreeMap, BTreeSet},
};
mod diagnostics;
mod pages;
#[cfg(feature = "native-mono-memory")]
pub mod shared;
mod vm;
pub use diagnostics::TranslationDiagnosticsSummary;
pub const PAGE: u64 = 4096;
pub const READ: u8 = 1;
pub const WRITE: u8 = 2;
pub const EXECUTE: u8 = 4;
const USER_END: u64 = 1 << 47;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TranslationCacheLayout {
    Direct16K,
    Direct32K,
    Direct64K,
    Direct128K,
    FourWay16K,
}

// Experiments use separate frozen builds. The chosen direct layout uses a
// constant mask without associative probing or runtime layout branches.
const BUILD_FOUR_WAY: bool = option_env!("WIRE_X64_CACHE_FOUR_WAY").is_some();
const BUILD_16K: bool = option_env!("WIRE_X64_CACHE_16K").is_some() || BUILD_FOUR_WAY;
const BUILD_32K: bool = option_env!("WIRE_X64_CACHE_32K").is_some();
const BUILD_64K: bool = option_env!("WIRE_X64_CACHE_64K").is_some();
const BUILD_128K: bool =
    option_env!("WIRE_X64_CACHE_128K").is_some() || !(BUILD_16K || BUILD_32K || BUILD_64K);
const BUILD_SLOTS: usize = if BUILD_16K {
    16384
} else if BUILD_128K {
    131072
} else if BUILD_64K {
    65536
} else {
    32768
};
impl Default for TranslationCacheLayout {
    fn default() -> Self {
        if BUILD_FOUR_WAY {
            Self::FourWay16K
        } else if BUILD_16K {
            Self::Direct16K
        } else if BUILD_128K {
            Self::Direct128K
        } else if BUILD_64K {
            Self::Direct64K
        } else {
            Self::Direct32K
        }
    }
}
struct TranslationSlot {
    validation_epoch: Cell<u64>,
    pc: Cell<u64>,
    ir: Cell<Option<super::cpu::ir::ScalarIr>>,
}
struct TranslationCache {
    // Scalar hits only touch densely packed PC/epoch/IR records. The larger
    // iced records are retained in a separate, equally bounded cold array.
    slots: Box<[TranslationSlot]>,
    instructions: Box<[Cell<iced_x86::Instruction>]>,
    generations: Box<[Cell<(u64, u64)>]>, // Second 0 means a single-page instruction.
    segments: Box<[RefCell<SegmentState>]>,
    links: Box<[Cell<(u64, usize)>]>,
    // Four-way set order, MRU in the low two bits; empty for direct layouts.
    order: Box<[Cell<u8>]>,
}
enum SegmentState {
    Unbuilt,
    Single,
    Ready(Box<super::cpu::ir::RegisterSegment>),
}
impl TranslationCache {
    fn hash(address: u64) -> usize {
        (address ^ (address >> 16) ^ (address >> 32)) as usize
    }
    fn touch(&self, slot: usize) {
        let order = &self.order[slot / 4];
        let way = (slot & 3) as u8;
        let old = order.get();
        if old & 3 == way {
            return;
        }
        let rank = (1..4).find(|n| (old >> (n * 2)) & 3 == way).unwrap();
        let before = (1u16 << (rank * 2)) - 1;
        let through = (1u16 << ((rank + 1) * 2)) - 1;
        order.set(((old as u16 & !through) | ((old as u16 & before) << 2) | way as u16) as u8);
    }
    #[inline(always)]
    fn find(&self, address: u64) -> Option<usize> {
        let hash = Self::hash(address);
        if !BUILD_FOUR_WAY {
            let slot = hash & (BUILD_SLOTS - 1);
            let entry = &self.slots[slot];
            return (entry.pc.get() == address).then_some(slot);
        }
        self.find_four_way(address)
    }
    #[inline(never)]
    fn find_four_way(&self, address: u64) -> Option<usize> {
        let start = (Self::hash(address) & (self.order.len() - 1)) * 4;
        let order = self.order[start / 4].get();
        for rank in 0..4 {
            let slot = start + ((order >> (rank * 2)) & 3) as usize;
            let entry = &self.slots[slot];
            if entry.pc.get() == address {
                self.touch(slot);
                return Some(slot);
            }
        }
        None
    }
    fn replacement(&self, address: u64) -> usize {
        let hash = Self::hash(address);
        if !BUILD_FOUR_WAY {
            return hash & (BUILD_SLOTS - 1);
        }
        let start = (hash & (self.order.len() - 1)) * 4;
        let slot = (start..start + 4)
            .find(|&s| self.slots[s].pc.get() == address)
            .or_else(|| (start..start + 4).find(|&s| self.slots[s].pc.get() == u64::MAX))
            .unwrap_or_else(|| start + (self.order[start / 4].get() >> 6) as usize);
        self.touch(slot);
        slot
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct ExecutionStats {
    pub ir_steps: u64,
    pub fallback_steps: u64,
    pub translations: u64,
    pub invalidations: u64,
}
struct Page {
    data: Box<[u8; 4096]>,
    permissions: u8,
    protection: Cell<u32>,
    // Nonzero also remembers formerly executable pages. Writes while NX must
    // invalidate their old translations before EXECUTE can be restored.
    generation: u64,
}
#[derive(Clone, Copy)]
struct Allocation {
    end: u64,
    protection: u32,
    kind: u32,
    guest: bool,
    protection_locked: bool,
}
#[derive(Default)]
pub struct Memory {
    #[cfg(feature = "native-mono-memory")]
    shared: BTreeMap<u64, shared::SharedMapping>,
    #[cfg(feature = "native-mono-memory")]
    shared_bytes: u64,
    pages: pages::Pages,
    // Ordered addresses are only needed by VirtualQuery over reserved holes.
    // Checked reads/writes use the hash table; neither index owns host pointers.
    page_addresses: BTreeSet<u64>,
    allocations: BTreeMap<u64, Allocation>,
    decode_epoch: u64,
    decoded: OnceCell<TranslationCache>,
    checked_execute_page: Cell<Option<u64>>,
    execution_stats: Option<Cell<ExecutionStats>>,
    diagnostics: Option<RefCell<diagnostics::Diagnostics>>,
    code_images: BTreeMap<u64, String>,
}
impl Memory {
    #[cfg(feature = "native-mono-memory")]
    fn shared_bytes(&self) -> u64 {
        self.shared_bytes
    }
    #[cfg(feature = "native-mono-memory")]
    fn committed_bytes(&self) -> u64 {
        self.pages.len() as u64 * PAGE + self.shared_bytes()
    }

    /// Select an experimental layout before execution; never reinterpret live slots.
    pub fn set_translation_cache_layout(
        &mut self,
        layout: TranslationCacheLayout,
    ) -> Result<(), String> {
        if self.decoded.get().is_some() {
            return Err("AMD64 translation cache layout is already initialized".into());
        }
        let built = TranslationCacheLayout::default();
        if layout != built {
            return Err(format!("AMD64 cache layout {layout:?} requires a separate build; this binary uses {built:?}"));
        }
        Ok(())
    }
    /// Expensive history/opcode counters. Enable only for diagnostic replays.
    pub fn enable_execution_diagnostics(&mut self) {
        self.enable_execution_stats();
        self.diagnostics = Some(RefCell::new(diagnostics::Diagnostics::default()));
    }
    pub fn translation_diagnostics(&self) -> Option<TranslationDiagnosticsSummary> {
        self.diagnostics.as_ref().map(|d| d.borrow().summary)
    }
    pub fn execution_diagnostics_report(&self) -> Option<String> {
        self.diagnostics
            .as_ref()
            .map(|d| d.borrow().report(&self.code_images))
    }
    pub fn register_code_image(&mut self, base: u64, name: String) {
        self.code_images.insert(base, name);
    }
    fn code_origin(&self, address: u64) -> (u64, u32) {
        self.allocations
            .range(..=address)
            .next_back()
            .filter(|(_, allocation)| address < allocation.end)
            .map_or((address & !(PAGE - 1), 0), |(&base, a)| (base, a.kind))
    }
    pub(crate) fn record_fallback(&self, instruction: &iced_x86::Instruction) {
        if let Some(d) = &self.diagnostics {
            d.borrow_mut()
                .fallback(instruction, self.code_origin(instruction.ip()));
        }
    }
    pub fn enable_execution_stats(&mut self) {
        self.execution_stats = Some(Cell::new(ExecutionStats::default()));
    }
    pub fn execution_stats(&self) -> Option<ExecutionStats> {
        self.execution_stats.as_ref().map(Cell::get)
    }
    pub(crate) fn record_execution(&self, ir: bool) {
        self.record_executions(ir, 1);
    }
    pub(crate) fn record_executions(&self, ir: bool, work: u64) {
        if let Some(stats) = &self.execution_stats {
            let mut value = stats.get();
            if ir {
                value.ir_steps += work;
            } else {
                value.fallback_steps += work;
            }
            stats.set(value);
        }
    }
    fn invalidate_decode(&mut self) -> u64 {
        self.checked_execute_page.set(None);
        if let Some(stats) = &self.execution_stats {
            let mut value = stats.get();
            value.invalidations += 1;
            stats.set(value);
        }
        self.decode_epoch = self.decode_epoch.wrapping_add(1);
        if self.decode_epoch == 0 {
            self.decoded.take();
            // Clear all cached records and restart every live page at one.
            // Zero remains reserved for pages with no executable history.
            for page in self.pages.values_mut() {
                page.generation = 1;
            }
            self.decode_epoch = 1;
        }
        self.decode_epoch
    }
    fn invalidate_mapping(&mut self, executable_changed: bool) {
        // Every mapping/guard change discards the execution proof. Assign a
        // fresh identity only to new executable pages: existing bytes remain
        // reusable, but live permission/guard checks still precede execution.
        if executable_changed {
            if let Some(d) = &self.diagnostics {
                d.borrow_mut().summary.mapping_invalidations += 1;
            }
            let generation = self.invalidate_decode();
            for page in self.pages.values_mut() {
                if page.permissions & EXECUTE != 0 && page.generation == 0 {
                    page.generation = generation;
                }
            }
        } else {
            self.checked_execute_page.set(None);
        }
    }
    #[inline(always)]
    fn translation(&self, address: u64) -> Option<(usize, &TranslationSlot)> {
        let cache = self.decoded.get()?;
        let slot = cache.find(address)?;
        let entry = &cache.slots[slot];
        if entry.validation_epoch.get() == self.decode_epoch
            || self.refresh_translation(address, slot, entry)
        {
            return Some((slot, entry));
        }
        None
    }
    #[inline(never)]
    fn refresh_translation(&self, address: u64, slot: usize, entry: &TranslationSlot) -> bool {
        let base = address & !(PAGE - 1);
        let Some(page) = self.pages.get(&base) else {
            return false;
        };
        let (generation, second) = self.decoded.get().unwrap().generations[slot].get();
        if generation != page.generation {
            return false;
        }
        if second != 0
            && !self
                .pages
                .get(&(base + PAGE))
                .is_some_and(|p| p.generation == second)
        {
            return false;
        }
        entry.validation_epoch.set(self.decode_epoch);
        true
    }
    #[inline(always)]
    fn check_cached_execute(&self, address: u64, len: usize) -> Result<(), String> {
        let base = address & !(PAGE - 1);
        let single_page = len != 0 && len as u64 <= PAGE - (address - base);
        if single_page && self.checked_execute_page.get() == Some(base) {
            return Ok(());
        }
        self.check(address, len, EXECUTE)?;
        // This proof is only reused by cached translations in this Memory.
        // There is one cooperative host CPU; every mapping/protection/code
        // mutation clears it. A guarded page is never cached before success,
        // and a crossing instruction always checks both pages again.
        if single_page {
            self.checked_execute_page.set(Some(base));
        }
        Ok(())
    }
    #[inline(always)]
    pub(crate) fn cached_ir(
        &self,
        address: u64,
    ) -> Result<Option<super::cpu::ir::ScalarIr>, String> {
        let Some(ir) = self
            .translation(address)
            .and_then(|(_, entry)| entry.ir.get())
        else {
            return Ok(None);
        };
        self.check_cached_execute(address, ir.len as usize)?;
        Ok(Some(ir))
    }
    pub(crate) fn cached_instruction(
        &self,
        address: u64,
    ) -> Result<Option<iced_x86::Instruction>, String> {
        let Some((slot, _)) = self.translation(address) else {
            return Ok(None);
        };
        let instruction = self.decoded.get().unwrap().instructions[slot].get();
        self.check_cached_execute(address, instruction.len())?;
        Ok(Some(instruction))
    }
    /// Called only after cached_ir has checked execution permissions. Its
    /// immutable borrow guarantees no mapping/code mutation during the segment.
    pub(crate) fn try_register_segment(
        &self,
        cpu: &mut super::cpu::Cpu64,
        remaining: u64,
        mut allowed: impl FnMut(u64, u64) -> bool,
    ) -> bool {
        if option_env!("WIRE_X64_NO_SEGMENTS").is_some() || remaining < 2 {
            return false;
        }
        let pc = cpu.rip;
        let Some((mut slot, _)) = self.translation(pc) else {
            return false;
        };
        let cache = self.decoded.get().unwrap();
        let base = pc & !(PAGE - 1);
        let before = cpu.work;
        let mut left = remaining;
        loop {
            let pc = cpu.rip;
            let mut state = cache.segments[slot].borrow_mut();
            if matches!(*state, SegmentState::Unbuilt) {
                let base = pc & !(PAGE - 1);
                let page = &self.pages[&base];
                *state =
                    super::cpu::ir::RegisterSegment::lower(&page.data[(pc - base) as usize..], pc)
                        .map_or(SegmentState::Single, |s| SegmentState::Ready(Box::new(s)));
                if matches!(*state, SegmentState::Single) {
                    // One-time negative caching retains the ordinary scalar hit
                    // cost for an isolated register operation in a memory loop.
                    let entry = &cache.slots[slot];
                    entry.ir.set(
                        entry
                            .ir
                            .get()
                            .map(super::cpu::ir::ScalarIr::without_segment),
                    );
                }
            }
            if let SegmentState::Ready(segment) = &*state {
                if segment.work() <= left && allowed(pc, pc + segment.bytes) {
                    segment.execute(cpu);
                    left -= segment.work();
                } else {
                    break;
                }
            } else {
                break;
            }
            drop(state);
            let next = cpu.rip;
            if option_env!("WIRE_X64_NO_LINKS").is_some() || left < 2 || next & !(PAGE - 1) != base
            {
                break;
            }
            let (linked_pc, linked_slot) = cache.links[slot].get();
            if linked_pc == next && linked_slot == usize::MAX {
                break;
            }
            let target = if linked_pc == next && cache.slots[linked_slot].pc.get() == next {
                let entry = &cache.slots[linked_slot];
                (entry.validation_epoch.get() == self.decode_epoch
                    || self.refresh_translation(next, linked_slot, entry))
                .then_some((linked_slot, entry))
            } else {
                self.translation(next)
            };
            let Some((target, entry)) = target else {
                break;
            };
            if !entry.ir.get().is_some_and(|ir| ir.register_only()) {
                cache.links[slot].set((next, usize::MAX));
                break;
            }
            cache.links[slot].set((next, target));
            slot = target;
        }
        let work = cpu.work - before;
        self.record_executions(true, work);
        work != 0
    }
    pub(crate) fn cache_instruction(&self, instruction: iced_x86::Instruction, bytes: &[u8]) {
        let base = instruction.ip() & !(PAGE - 1);
        let generation = self.pages[&base].generation;
        let second_generation = if instruction.len() as u64 > PAGE - (instruction.ip() - base) {
            self.pages[&(base + PAGE)].generation
        } else {
            0
        };
        if let Some(d) = &self.diagnostics {
            d.borrow_mut().translated(
                instruction.ip(),
                (generation, second_generation),
                bytes,
                self.code_origin(instruction.ip()),
            );
        }
        let cache = self.decoded.get_or_init(|| TranslationCache {
            slots: (0..BUILD_SLOTS)
                .map(|_| TranslationSlot {
                    validation_epoch: Cell::new(0),
                    pc: Cell::new(u64::MAX),
                    ir: Cell::new(None),
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            instructions: (0..BUILD_SLOTS)
                .map(|_| Cell::new(iced_x86::Instruction::default()))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            generations: (0..BUILD_SLOTS)
                .map(|_| Cell::new((0, 0)))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            segments: (0..BUILD_SLOTS)
                .map(|_| RefCell::new(SegmentState::Unbuilt))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            links: (0..BUILD_SLOTS)
                .map(|_| Cell::new((u64::MAX, usize::MAX)))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            order: if BUILD_FOUR_WAY {
                (0..4096)
                    .map(|_| Cell::new(0b11100100))
                    .collect::<Vec<_>>()
                    .into_boxed_slice()
            } else {
                Vec::new().into_boxed_slice()
            },
        });
        let slot = cache.replacement(instruction.ip());
        let entry = &cache.slots[slot];
        entry.pc.set(instruction.ip());
        entry.validation_epoch.set(self.decode_epoch);
        cache.generations[slot].set((generation, second_generation));
        *cache.segments[slot].borrow_mut() = SegmentState::Unbuilt;
        cache.links[slot].set((u64::MAX, usize::MAX));
        cache.instructions[slot].set(instruction);
        let ir = super::cpu::ir::ScalarIr::lower(&instruction);
        entry.ir.set(ir);
        if let Some(stats) = &self.execution_stats {
            let mut value = stats.get();
            value.translations += 1;
            stats.set(value);
        }
    }
    pub fn available(&self, addr: u64, len: u64) -> bool {
        if !addr.is_multiple_of(PAGE) || len == 0 || !len.is_multiple_of(PAGE) {
            return false;
        }
        Self::range(addr, len as usize).is_ok_and(|range| self.free_range(range.start, range.end))
    }
    pub fn merge(&mut self, mut other: Memory) -> Result<(), String> {
        #[cfg(feature = "native-mono-memory")]
        let shared_overlap = other
            .shared
            .values()
            .any(|mapping| !self.free_range(mapping.base(), mapping.end()));
        #[cfg(feature = "native-mono-memory")]
        let exceeds_capacity =
            self.committed_bytes() + other.committed_bytes() > 512 * 1024 * 1024 || shared_overlap;
        #[cfg(not(feature = "native-mono-memory"))]
        let exceeds_capacity =
            (self.pages.len() + other.pages.len()) as u64 * PAGE > 512 * 1024 * 1024;
        if exceeds_capacity
            || other
                .allocations
                .iter()
                .any(|(&base, allocation)| !self.free_range(base, allocation.end))
        {
            return Err("AMD64 image mapping overlaps or exceeds capacity".into());
        }
        let executable_changed = other.pages.values().any(|p| p.permissions & EXECUTE != 0);
        for (address, mut page) in other.pages.into_entries() {
            page.generation = 0;
            self.pages.insert(address, page);
        }
        self.page_addresses.append(&mut other.page_addresses);
        self.allocations.append(&mut other.allocations);
        #[cfg(feature = "native-mono-memory")]
        self.shared_merge(&mut other.shared);
        self.invalidate_mapping(executable_changed);
        Ok(())
    }
    #[inline(always)]
    fn range(addr: u64, len: usize) -> Result<std::ops::Range<u64>, String> {
        let end = addr
            .checked_add(len as u64)
            .ok_or("AMD64 address overflow")?;
        if addr >= USER_END || end > USER_END {
            return Err(format!("noncanonical/user address {addr:#x}"));
        }
        Ok(addr..end)
    }
    pub fn map(&mut self, addr: u64, len: u64, permissions: u8) -> Result<(), String> {
        if !addr.is_multiple_of(PAGE)
            || len == 0
            || !len.is_multiple_of(PAGE)
            || permissions & !7 != 0
        {
            return Err("invalid AMD64 mapping".into());
        }
        let range = Self::range(addr, usize::try_from(len).map_err(|_| "mapping too large")?)?;
        #[cfg(feature = "native-mono-memory")]
        let used = self.committed_bytes();
        #[cfg(not(feature = "native-mono-memory"))]
        let used = self.pages.len() as u64 * PAGE;
        if len > 256 * 1024 * 1024 || used + len > 512 * 1024 * 1024 {
            return Err("AMD64 mapping capacity exceeded".into());
        }
        if !self.free_range(range.start, range.end) {
            return Err("overlapping AMD64 mapping".into());
        }
        for p in range.step_by(PAGE as usize) {
            self.page_addresses.insert(p);
            self.pages.insert(
                p,
                Page {
                    data: Box::new([0; 4096]),
                    permissions,
                    protection: Cell::new(Self::windows_protection(permissions)),
                    generation: 0,
                },
            );
        }
        self.allocations.insert(
            addr,
            Allocation {
                end: addr + len,
                protection: Self::windows_protection(permissions),
                kind: crate::mem::MEM_PRIVATE,
                guest: false,
                protection_locked: false,
            },
        );
        self.invalidate_mapping(permissions & EXECUTE != 0);
        Ok(())
    }
    pub fn map_image(&mut self, addr: u64, len: u64, permissions: u8) -> Result<(), String> {
        self.map(addr, len, permissions)?;
        self.allocations.get_mut(&addr).unwrap().kind = crate::mem::MEM_IMAGE;
        Ok(())
    }
    /// Immutable runtime COM tables/objects. Guest VirtualProtect must not
    /// turn a service-owned interface page into writable or executable data.
    pub(crate) fn seal_runtime_readonly(&mut self, addr: u64) -> Result<(), String> {
        let allocation = self
            .allocations
            .get(&addr)
            .ok_or("missing runtime allocation")?;
        if allocation.guest {
            return Err("cannot seal guest allocation".into());
        }
        self.protect(addr, allocation.end - addr, READ)?;
        self.allocations.get_mut(&addr).unwrap().protection_locked = true;
        Ok(())
    }
    pub fn protect(&mut self, addr: u64, len: u64, permissions: u8) -> Result<(), String> {
        if !addr.is_multiple_of(PAGE)
            || !len.is_multiple_of(PAGE)
            || len == 0
            || permissions & !7 != 0
        {
            return Err("invalid AMD64 protection".into());
        }
        let range = Self::range(
            addr,
            usize::try_from(len).map_err(|_| "protection too large")?,
        )?;
        if range
            .clone()
            .step_by(PAGE as usize)
            .any(|p| !self.pages.contains_key(&p))
        {
            return Err("unmapped AMD64 protection".into());
        }
        let mut executable_changed = permissions & EXECUTE != 0;
        for p in range.step_by(PAGE as usize) {
            let page = self.pages.get_mut(&p).unwrap();
            executable_changed |= page.permissions & EXECUTE != 0;
            page.permissions = permissions;
            page.protection.set(Self::windows_protection(permissions));
        }
        self.invalidate_mapping(executable_changed);
        Ok(())
    }
    pub fn check(&self, addr: u64, len: usize, access: u8) -> Result<(), String> {
        let range = Self::range(addr, len)?;
        #[cfg(feature = "native-mono-memory")]
        if !self.shared.is_empty() && self.shared_check(addr, len, access)? {
            return Ok(());
        }
        if range.is_empty() {
            return Ok(());
        }
        for p in (addr & !(PAGE - 1)..=(range.end - 1) & !(PAGE - 1)).step_by(PAGE as usize) {
            let page = self
                .pages
                .get(&p)
                .ok_or_else(|| format!("unmapped AMD64 address {addr:#x}"))?;
            Self::check_page(page, p, addr, access)?;
        }
        Ok(())
    }
    #[inline(always)]
    fn check_page(page: &Page, base: u64, address: u64, access: u8) -> Result<(), String> {
        if page.protection.get() & crate::mem::PAGE_GUARD != 0 {
            page.protection
                .set(page.protection.get() & !crate::mem::PAGE_GUARD);
            return Err(format!("AMD64 guard-page fault at {base:#x}"));
        }
        if page.permissions & access != access {
            return Err(format!("AMD64 access {access} denied at {address:#x}"));
        }
        Ok(())
    }
    pub fn unmap(&mut self, addr: u64, len: u64) -> Result<(), String> {
        if !addr.is_multiple_of(PAGE) || len == 0 || !len.is_multiple_of(PAGE) {
            return Err("invalid AMD64 unmap".into());
        }
        let range = Self::range(addr, usize::try_from(len).map_err(|_| "unmap too large")?)?;
        if self
            .allocations
            .get(&addr)
            .is_none_or(|allocation| allocation.end != range.end || allocation.guest)
        {
            return Err("AMD64 runtime unmap must match its owned allocation".into());
        }
        if range
            .clone()
            .step_by(PAGE as usize)
            .any(|p| !self.pages.contains_key(&p))
        {
            return Err("unmapped AMD64 unmap".into());
        }
        let mut executable_changed = false;
        for p in range.step_by(PAGE as usize) {
            executable_changed |= self.pages.remove(&p).unwrap().permissions & EXECUTE != 0;
            self.page_addresses.remove(&p);
        }
        self.allocations.remove(&addr);
        self.invalidate_mapping(executable_changed);
        Ok(())
    }
    pub fn read(&self, addr: u64, out: &mut [u8]) -> Result<(), String> {
        self.read_access(addr, out, READ)
    }
    pub fn fetch(&self, addr: u64, out: &mut [u8]) -> Result<(), String> {
        self.read_access(addr, out, EXECUTE)
    }
    fn read_access(&self, addr: u64, out: &mut [u8], access: u8) -> Result<(), String> {
        Self::range(addr, out.len())?;
        #[cfg(feature = "native-mono-memory")]
        if !self.shared.is_empty() && self.shared_read(addr, out, access)? {
            return Ok(());
        }
        if out.is_empty() {
            return Ok(());
        }
        let base = addr & !(PAGE - 1);
        let start = (addr - base) as usize;
        if out.len() <= 4096 - start {
            let page = self
                .pages
                .get(&base)
                .ok_or_else(|| format!("unmapped AMD64 address {addr:#x}"))?;
            Self::check_page(page, base, addr, access)?;
            out.copy_from_slice(&page.data[start..start + out.len()]);
            return Ok(());
        }
        self.check(addr, out.len(), access)?;
        let mut offset = 0;
        while offset < out.len() {
            let at = addr + offset as u64;
            let start = (at & (PAGE - 1)) as usize;
            let n = (4096 - start).min(out.len() - offset);
            out[offset..offset + n]
                .copy_from_slice(&self.pages[&(at & !(PAGE - 1))].data[start..start + n]);
            offset += n;
        }
        Ok(())
    }
    pub fn write(&mut self, addr: u64, bytes: &[u8]) -> Result<(), String> {
        Self::range(addr, bytes.len())?;
        #[cfg(feature = "native-mono-memory")]
        if !self.shared.is_empty() && self.shared_write(addr, bytes)? {
            return Ok(());
        }
        if bytes.is_empty() {
            return Ok(());
        }
        let base = addr & !(PAGE - 1);
        let start = (addr - base) as usize;
        if bytes.len() <= 4096 - start {
            let page = self
                .pages
                .get_mut(&base)
                .ok_or_else(|| format!("unmapped AMD64 address {addr:#x}"))?;
            Self::check_page(page, base, addr, WRITE)?;
            let code_changed = page.generation != 0;
            if code_changed {
                if let Some(d) = &self.diagnostics {
                    d.borrow_mut()
                        .write(base, page.data[start..start + bytes.len()] != *bytes);
                }
            }
            page.data[start..start + bytes.len()].copy_from_slice(bytes);
            if code_changed {
                let generation = self.invalidate_decode();
                self.pages.get_mut(&base).unwrap().generation = generation;
            }
            return Ok(());
        }
        self.check(addr, bytes.len(), WRITE)?;
        let mut offset = 0;
        let mut code_changed = false;
        while offset < bytes.len() {
            let at = addr + offset as u64;
            let start = (at & (PAGE - 1)) as usize;
            let n = (4096 - start).min(bytes.len() - offset);
            let page = self.pages.get_mut(&(at & !(PAGE - 1))).unwrap();
            code_changed |= page.generation != 0;
            if page.generation != 0 {
                if let Some(d) = &self.diagnostics {
                    d.borrow_mut().write(
                        at & !(PAGE - 1),
                        page.data[start..start + n] != bytes[offset..offset + n],
                    );
                }
            }
            page.data[start..start + n].copy_from_slice(&bytes[offset..offset + n]);
            offset += n;
        }
        if code_changed {
            let generation = self.invalidate_decode();
            let first = addr & !(PAGE - 1);
            let last = (addr + bytes.len() as u64 - 1) & !(PAGE - 1);
            for base in (first..=last).step_by(PAGE as usize) {
                let page = self.pages.get_mut(&base).unwrap();
                if page.generation != 0 {
                    page.generation = generation;
                }
            }
        }
        Ok(())
    }
    pub fn read_int(&self, addr: u64, size: usize) -> Result<u64, String> {
        if !matches!(size, 1 | 2 | 4 | 8) {
            return Err("invalid AMD64 scalar width".into());
        }
        Self::range(addr, size)?;
        #[cfg(feature = "native-mono-memory")]
        if !self.shared.is_empty() {
            let mut bytes = [0; 8];
            if self.shared_read(addr, &mut bytes[..size], READ)? {
                return Ok(u64::from_le_bytes(bytes));
            }
        }
        let base = addr & !(PAGE - 1);
        let start = (addr - base) as usize;
        if size <= 4096 - start {
            let page = self
                .pages
                .get(&base)
                .ok_or_else(|| format!("unmapped AMD64 address {addr:#x}"))?;
            Self::check_page(page, base, addr, READ)?;
            // Fixed-width copies lower to loads instead of a variable memmove.
            // Slice bounds and live guest permissions are still checked.
            return Ok(match size {
                1 => page.data[start] as u64,
                2 => u16::from_le_bytes(page.data[start..start + 2].try_into().unwrap()) as u64,
                4 => u32::from_le_bytes(page.data[start..start + 4].try_into().unwrap()) as u64,
                8 => u64::from_le_bytes(page.data[start..start + 8].try_into().unwrap()),
                _ => unreachable!(),
            });
        }
        let mut bytes = [0; 8];
        self.read(addr, &mut bytes[..size])?;
        Ok(u64::from_le_bytes(bytes))
    }
    /// Reuse one owned page lookup while preserving read-before-write checks.
    /// A crossing access retains the original checked all-or-nothing write.
    pub(crate) fn modify_int(
        &mut self,
        addr: u64,
        size: usize,
        update: impl FnOnce(u64) -> u64,
    ) -> Result<(), String> {
        if !matches!(size, 1 | 2 | 4 | 8) {
            return Err("invalid AMD64 scalar width".into());
        }
        Self::range(addr, size)?;
        #[cfg(feature = "native-mono-memory")]
        if !self.shared.is_empty() && self.shared_contains_range(addr, size)? {
            return self.shared_modify_int(addr, size, update);
        }

        let base = addr & !(PAGE - 1);
        let start = (addr - base) as usize;
        if option_env!("WIRE_X64_NO_RMW").is_some() || size > 4096 - start {
            let value = self.read_int(addr, size)?;
            return self.write_int(addr, size, update(value));
        }
        let page = self
            .pages
            .get_mut(&base)
            .ok_or_else(|| format!("unmapped AMD64 address {addr:#x}"))?;
        Self::check_page(page, base, addr, READ)?;
        let value = match size {
            1 => page.data[start] as u64,
            2 => u16::from_le_bytes(page.data[start..start + 2].try_into().unwrap()) as u64,
            4 => u32::from_le_bytes(page.data[start..start + 4].try_into().unwrap()) as u64,
            _ => u64::from_le_bytes(page.data[start..start + 8].try_into().unwrap()),
        };
        let result = update(value);
        Self::check_page(page, base, addr, WRITE)?;
        let code_changed = page.generation != 0;
        if code_changed {
            if let Some(d) = &self.diagnostics {
                d.borrow_mut().write(
                    base,
                    page.data[start..start + size] != result.to_le_bytes()[..size],
                );
            }
        }
        match size {
            1 => page.data[start] = result as u8,
            2 => page.data[start..start + 2].copy_from_slice(&(result as u16).to_le_bytes()),
            4 => page.data[start..start + 4].copy_from_slice(&(result as u32).to_le_bytes()),
            8 => page.data[start..start + 8].copy_from_slice(&result.to_le_bytes()),
            _ => unreachable!(),
        }
        if code_changed {
            let generation = self.invalidate_decode();
            self.pages.get_mut(&base).unwrap().generation = generation;
        }
        Ok(())
    }
    pub fn write_int(&mut self, addr: u64, size: usize, value: u64) -> Result<(), String> {
        if !matches!(size, 1 | 2 | 4 | 8) {
            return Err("invalid AMD64 scalar width".into());
        }
        Self::range(addr, size)?;
        #[cfg(feature = "native-mono-memory")]
        if !self.shared.is_empty() && self.shared_write(addr, &value.to_le_bytes()[..size])? {
            return Ok(());
        }
        let base = addr & !(PAGE - 1);
        let start = (addr - base) as usize;
        if size <= 4096 - start {
            let page = self
                .pages
                .get_mut(&base)
                .ok_or_else(|| format!("unmapped AMD64 address {addr:#x}"))?;
            Self::check_page(page, base, addr, WRITE)?;
            let code_changed = page.generation != 0;
            if code_changed {
                if let Some(d) = &self.diagnostics {
                    d.borrow_mut().write(
                        base,
                        page.data[start..start + size] != value.to_le_bytes()[..size],
                    );
                }
            }
            match size {
                1 => page.data[start] = value as u8,
                2 => page.data[start..start + 2].copy_from_slice(&(value as u16).to_le_bytes()),
                4 => page.data[start..start + 4].copy_from_slice(&(value as u32).to_le_bytes()),
                8 => page.data[start..start + 8].copy_from_slice(&value.to_le_bytes()),
                _ => unreachable!(),
            }
            if code_changed {
                let generation = self.invalidate_decode();
                self.pages.get_mut(&base).unwrap().generation = generation;
            }
            return Ok(());
        }
        self.write(addr, &value.to_le_bytes()[..size])
    }
    /// Only forward, non-overlapping, single-page strings on ordinary data.
    /// Probes consume no guard and report no speculative fault. The caller
    /// falls back to one checked element whenever a proof is unavailable.
    pub(crate) fn try_string_chunk(
        &mut self,
        dest: u64,
        source: Option<u64>,
        size: usize,
        count: u64,
        value: u64,
    ) -> Result<Option<u64>, String> {
        let boundary = |at: u64| (PAGE - (at & (PAGE - 1))) / size as u64;
        let mut count = count.min(64).min(boundary(dest));
        if let Some(at) = source {
            count = count.min(boundary(at));
        }
        if count < 2 {
            return Ok(None);
        }
        let len = count as usize * size;
        if Self::range(dest, len).is_err() {
            return Ok(None);
        }
        let Some(page) = self.pages.get(&(dest & !(PAGE - 1))) else {
            return Ok(None);
        };
        if page.permissions & WRITE == 0
            || page.generation != 0
            || page.protection.get() & crate::mem::PAGE_GUARD != 0
        {
            return Ok(None);
        }
        if let Some(at) = source {
            if Self::range(at, len).is_err() || (dest < at + len as u64 && at < dest + len as u64) {
                return Ok(None);
            }
            let Some(page) = self.pages.get(&(at & !(PAGE - 1))) else {
                return Ok(None);
            };
            if page.permissions & READ == 0 || page.protection.get() & crate::mem::PAGE_GUARD != 0 {
                return Ok(None);
            }
        }
        let mut bytes = [0; 512];
        if let Some(at) = source {
            self.read(at, &mut bytes[..len])?;
        } else {
            for element in bytes[..len].chunks_exact_mut(size) {
                element.copy_from_slice(&value.to_le_bytes()[..size]);
            }
        }
        self.write(dest, &bytes[..len])?;
        Ok(Some(count))
    }
}

#[cfg(test)]
mod generation_tests {
    use super::*;
    use crate::x64::cpu::Cpu64;
    #[test]
    fn generation_wrap_discards_every_translation_and_segment() {
        let pc = 0x140000000;
        let mut mem = Memory::default();
        mem.map(pc, PAGE, READ | WRITE | EXECUTE).unwrap();
        mem.write(pc, &[0xb8, 1, 0, 0, 0, 0x90]).unwrap();
        let mut cpu = Cpu64::new(pc, 0);
        cpu.step_with_budget(&mut mem, 2).unwrap();
        cpu.rip = pc;
        cpu.step_with_budget(&mut mem, 2).unwrap();
        mem.decode_epoch = u64::MAX;
        mem.write(pc + 1, &[2]).unwrap();
        assert_eq!(mem.decode_epoch, 1);
        assert!(mem.decoded.get().is_none());
        assert!(mem.pages.values().all(|p| p.generation != 0));
        cpu.rip = pc;
        cpu.step(&mut mem).unwrap();
        assert_eq!(cpu.regs[0], 2);
    }
    #[test]
    fn reused_rmw_page_lookup_keeps_read_write_guard_and_cross_page_fault_order() {
        let base = 0x200000000;
        for first in [1, 2, 4, 0x104, 0x20, 0x40] {
            for second in [1, 2, 4, 0x104, 0x20, 0x40] {
                for size in [1, 2, 4, 8] {
                    for offset in [0, 4096 - size, 4095] {
                        let setup = || {
                            let mut m = Memory::default();
                            m.map(base, 8192, READ | WRITE).unwrap();
                            m.write(base, &[0x55; 8192]).unwrap();
                            m.virtual_protect(base, 4096, first).unwrap();
                            m.virtual_protect(base + 4096, 4096, second).unwrap();
                            m
                        };
                        let mut fast = setup();
                        let mut slow = setup();
                        let called = Cell::new(false);
                        let result = fast.modify_int(base + offset as u64, size, |v| {
                            called.set(true);
                            v.wrapping_add(0x123456789)
                        });
                        let read = slow.read_int(base + offset as u64, size);
                        let read_ok = read.is_ok();
                        let expected = read.and_then(|v| {
                            slow.write_int(base + offset as u64, size, v.wrapping_add(0x123456789))
                        });
                        assert_eq!(
                            result, expected,
                            "offset={offset} size={size} protect={first:x}/{second:x}"
                        );
                        assert_eq!(called.get(), read_ok);
                        for at in [base, base + PAGE] {
                            let a = &fast.pages[&at];
                            let b = &slow.pages[&at];
                            assert_eq!(a.data, b.data);
                            assert_eq!(a.protection.get(), b.protection.get());
                            assert_eq!(a.generation, b.generation);
                        }
                    }
                }
            }
        }
    }
}
