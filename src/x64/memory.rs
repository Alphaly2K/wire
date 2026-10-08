//! Sparse, checked low-canonical AMD64 user address space. Host data is never executable.
use std::{
    cell::{Cell, OnceCell},
    collections::{BTreeMap, BTreeSet},
};
mod pages;
mod vm;
pub const PAGE: u64 = 4096;
pub const READ: u8 = 1;
pub const WRITE: u8 = 2;
pub const EXECUTE: u8 = 4;
const USER_END: u64 = 1 << 47;
const DECODE_SLOTS: usize = 16384;
struct TranslationSlot {
    epoch: Cell<u64>,
    pc: Cell<u64>,
    // Separate cells keep the cold iced record off scalar IR cache hits.
    ir: Cell<Option<super::cpu::ir::ScalarIr>>,
    instruction: Cell<iced_x86::Instruction>,
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
    pages: pages::Pages,
    // Ordered addresses are only needed by VirtualQuery over reserved holes.
    // Checked reads/writes use the hash table; neither index owns host pointers.
    page_addresses: BTreeSet<u64>,
    allocations: BTreeMap<u64, Allocation>,
    decode_epoch: u64,
    decoded: OnceCell<Box<[TranslationSlot]>>,
    checked_execute_page: Cell<Option<u64>>,
    execution_stats: Option<Cell<ExecutionStats>>,
}
impl Memory {
    pub fn enable_execution_stats(&mut self) {
        self.execution_stats = Some(Cell::new(ExecutionStats::default()));
    }
    pub fn execution_stats(&self) -> Option<ExecutionStats> {
        self.execution_stats.as_ref().map(Cell::get)
    }
    pub(crate) fn record_execution(&self, ir: bool) {
        if let Some(stats) = &self.execution_stats {
            let mut value = stats.get();
            if ir {
                value.ir_steps += 1;
            } else {
                value.fallback_steps += 1;
            }
            stats.set(value);
        }
    }
    fn invalidate_decode(&mut self) {
        self.checked_execute_page.set(None);
        if let Some(stats) = &self.execution_stats {
            let mut value = stats.get();
            value.invalidations += 1;
            stats.set(value);
        }
        self.decode_epoch = self.decode_epoch.wrapping_add(1);
        if self.decode_epoch == 0 {
            self.decoded.take();
        }
    }
    fn invalidate_mapping(&mut self, executable_changed: bool) {
        // Mapping/guard changes always discard the cached execute permission
        // proof. IR bytes remain valid when only non-executable data changes.
        // Adding, removing or protecting code still invalidates all translations.
        if executable_changed {
            self.invalidate_decode();
        } else {
            self.checked_execute_page.set(None);
        }
    }
    fn decode_slot(address: u64) -> usize {
        ((address ^ (address >> 16) ^ (address >> 32)) as usize) & (DECODE_SLOTS - 1)
    }
    fn translation(&self, address: u64) -> Option<&TranslationSlot> {
        let entry = &self.decoded.get()?[Self::decode_slot(address)];
        (entry.epoch.get() == self.decode_epoch && entry.pc.get() == address).then_some(entry)
    }
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
    pub(crate) fn cached_ir(
        &self,
        address: u64,
    ) -> Result<Option<super::cpu::ir::ScalarIr>, String> {
        let Some(ir) = self.translation(address).and_then(|entry| entry.ir.get()) else {
            return Ok(None);
        };
        self.check_cached_execute(address, ir.len as usize)?;
        Ok(Some(ir))
    }
    pub(crate) fn cached_instruction(
        &self,
        address: u64,
    ) -> Result<Option<iced_x86::Instruction>, String> {
        let Some(entry) = self.translation(address) else {
            return Ok(None);
        };
        let instruction = entry.instruction.get();
        self.check_cached_execute(address, instruction.len())?;
        Ok(Some(instruction))
    }
    pub(crate) fn cache_instruction(&self, instruction: iced_x86::Instruction) {
        let cache = self.decoded.get_or_init(|| {
            (0..DECODE_SLOTS)
                .map(|_| TranslationSlot {
                    epoch: Cell::new(0),
                    pc: Cell::new(u64::MAX),
                    ir: Cell::new(None),
                    instruction: Cell::new(iced_x86::Instruction::default()),
                })
                .collect::<Vec<_>>()
                .into_boxed_slice()
        });
        let entry = &cache[Self::decode_slot(instruction.ip())];
        entry.pc.set(instruction.ip());
        entry.epoch.set(self.decode_epoch);
        entry.instruction.set(instruction);
        entry.ir.set(super::cpu::ir::ScalarIr::lower(&instruction));
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
        if (self.pages.len() + other.pages.len()) as u64 * PAGE > 512 * 1024 * 1024
            || other
                .allocations
                .iter()
                .any(|(&base, allocation)| !self.free_range(base, allocation.end))
        {
            return Err("AMD64 image mapping overlaps or exceeds capacity".into());
        }
        let executable_changed = other.pages.values().any(|p| p.permissions & EXECUTE != 0);
        for (address, page) in other.pages.into_entries() {
            self.pages.insert(address, page);
        }
        self.page_addresses.append(&mut other.page_addresses);
        self.allocations.append(&mut other.allocations);
        self.invalidate_mapping(executable_changed);
        Ok(())
    }
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
        if len > 256 * 1024 * 1024 || self.pages.len() as u64 * PAGE + len > 512 * 1024 * 1024 {
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
            let code_changed = page.permissions & EXECUTE != 0;
            page.data[start..start + bytes.len()].copy_from_slice(bytes);
            if code_changed {
                self.invalidate_decode();
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
            code_changed |= page.permissions & EXECUTE != 0;
            page.data[start..start + n].copy_from_slice(&bytes[offset..offset + n]);
            offset += n;
        }
        if code_changed {
            self.invalidate_decode();
        }
        Ok(())
    }
    pub fn read_int(&self, addr: u64, size: usize) -> Result<u64, String> {
        if !matches!(size, 1 | 2 | 4 | 8) {
            return Err("invalid AMD64 scalar width".into());
        }
        Self::range(addr, size)?;
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
    pub fn write_int(&mut self, addr: u64, size: usize, value: u64) -> Result<(), String> {
        if !matches!(size, 1 | 2 | 4 | 8) {
            return Err("invalid AMD64 scalar width".into());
        }
        Self::range(addr, size)?;
        let base = addr & !(PAGE - 1);
        let start = (addr - base) as usize;
        if size <= 4096 - start {
            let page = self
                .pages
                .get_mut(&base)
                .ok_or_else(|| format!("unmapped AMD64 address {addr:#x}"))?;
            Self::check_page(page, base, addr, WRITE)?;
            let code_changed = page.permissions & EXECUTE != 0;
            match size {
                1 => page.data[start] = value as u8,
                2 => page.data[start..start + 2].copy_from_slice(&(value as u16).to_le_bytes()),
                4 => page.data[start..start + 4].copy_from_slice(&(value as u32).to_le_bytes()),
                8 => page.data[start..start + 8].copy_from_slice(&value.to_le_bytes()),
                _ => unreachable!(),
            }
            if code_changed {
                self.invalidate_decode();
            }
            return Ok(());
        }
        self.write(addr, &value.to_le_bytes()[..size])
    }
}
