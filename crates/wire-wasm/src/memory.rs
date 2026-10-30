//! Sparse guest pages owned by the core Wasm memory. Code proofs describe
//! source authority; they contain no decoded instructions or execution state.
use crate::machine::Mode;
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    fmt,
    rc::Rc,
};

#[path = "../../../src/memory/constants.rs"]
mod constants;
pub use constants::*;

mod pages;
pub(crate) mod translation;
mod vm;
pub use translation::TranslationCache;
pub use vm::MemoryInfo;

#[cfg(test)]
mod tests;

pub const PAGE: u64 = 4096;
pub const READ: u8 = 1;
pub const WRITE: u8 = 2;
pub const EXECUTE: u8 = 4;
pub const MEM_DECOMMIT: u32 = 0x4000;
pub const MEM_RELEASE: u32 = 0x8000;
const COMMIT_BYTE_LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const MAPPING_BYTE_LIMIT: u64 = 256 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum FaultKind {
    Address = 1,
    Unmapped,
    Protection,
    Guard,
    Width,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fault {
    pub address: u64,
    pub length: usize,
    pub access: u8,
    pub kind: FaultKind,
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "guest memory {:?} at {:#x} (length {}, access {})",
            self.kind, self.address, self.length, self.access
        )
    }
}
impl std::error::Error for Fault {}
impl From<Fault> for String {
    fn from(fault: Fault) -> Self {
        fault.to_string()
    }
}

struct Page {
    data: Box<[u8; PAGE as usize]>,
    permissions: u8,
    protection: Cell<u32>,
    // Zero means no compiler has observed this backing. Once observed, NX
    // writes must still revoke it before execute permission can be restored.
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

/// A source-page proof retains the address-space identity across owner drops.
/// Validate it against Memory before publishing or entering generated code.
#[derive(Clone, Debug)]
pub struct CodeProof {
    owner: Rc<()>,
    page: u64,
    generation: u64,
}

pub struct Memory {
    mode: Mode,
    dep_enabled: bool,
    pages: pages::Pages,
    page_addresses: BTreeSet<u64>,
    allocations: BTreeMap<u64, Allocation>,
    private_pages: usize,
    peak_pages: usize,
    peak_private_pages: usize,
    owner: Rc<()>,
    translations: Box<TranslationCache>,
    last_fault: Cell<Option<Fault>>,
}

impl Default for Memory {
    fn default() -> Self {
        Self::new(Mode::X64)
    }
}

impl Memory {
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            dep_enabled: true,
            pages: pages::Pages::default(),
            page_addresses: BTreeSet::new(),
            allocations: BTreeMap::new(),
            private_pages: 0,
            peak_pages: 0,
            peak_private_pages: 0,
            owner: Rc::new(()),
            translations: Box::default(),
            last_fault: Cell::new(None),
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn dep_enabled(&self) -> bool {
        self.dep_enabled
    }

    /// PE32 opt-in DEP changes effective execution permission while retaining
    /// the Windows protection value reported for each mapped page.
    pub fn set_dep_enabled(&mut self, enabled: bool) {
        let enabled = enabled || self.mode == Mode::X64;
        if self.dep_enabled != enabled {
            self.dep_enabled = enabled;
            let mut observed = false;
            for page in self.pages.values_mut() {
                observed |= page.generation != 0;
                page.generation = 0;
            }
            if observed {
                self.next_source_version();
            }
            self.mapping_changed();
        }
    }

    fn effective_permissions(&self, page: &Page) -> u8 {
        page.permissions
            | if !self.dep_enabled && page.permissions & READ != 0 {
                EXECUTE
            } else {
                0
            }
    }

    fn user_end(&self) -> u64 {
        match self.mode {
            Mode::X86 => 1 << 32,
            Mode::X64 => 1 << 47,
        }
    }

    fn app_end(&self) -> u64 {
        match self.mode {
            Mode::X86 => 0x7fff0000,
            Mode::X64 => self.user_end() - u64::from(ALLOCATION_GRANULARITY),
        }
    }

    fn range(&self, address: u64, length: usize) -> Result<std::ops::Range<u64>, String> {
        let end = address
            .checked_add(length as u64)
            .ok_or("guest address overflow")?;
        if address >= self.user_end() || end > self.user_end() {
            return Err("guest address outside user space".into());
        }
        Ok(address..end)
    }

    fn access_range(
        &self,
        address: u64,
        length: usize,
        access: u8,
    ) -> Result<std::ops::Range<u64>, Fault> {
        self.range(address, length).map_err(|_| Fault {
            address,
            length,
            access,
            kind: FaultKind::Address,
        })
    }

    fn commit_capacity(used: usize, additional: usize) -> bool {
        let limit = (COMMIT_BYTE_LIMIT / PAGE) as usize;
        additional <= limit && used <= limit - additional
    }

    /// Epochs saturate instead of reusing an old capability tag. At MAX a
    /// generated memory cache must remain disabled until its owner is reset.
    pub fn mapping_epoch(&self) -> u64 {
        self.translations.epoch.get()
    }

    pub fn source_version(&self) -> u64 {
        self.translations.source.get()
    }

    fn mapping_changed(&self) {
        self.translations
            .epoch
            .set(self.mapping_epoch().saturating_add(1));
    }

    fn next_source_version(&mut self) -> u64 {
        if self.source_version() == u64::MAX {
            self.owner = Rc::new(());
            for page in self.pages.values_mut() {
                page.generation = 0;
            }
            self.translations.source.set(0);
            self.mapping_changed();
        }
        self.translations.source.set(self.source_version() + 1);
        self.source_version()
    }

    fn revoke_code(&mut self, start: u64, end: u64) {
        if start == end {
            return;
        }
        let first = start & !(PAGE - 1);
        let last = (end - 1) & !(PAGE - 1);
        if !(first..=last)
            .step_by(PAGE as usize)
            .any(|p| self.pages.get(&p).is_some_and(|p| p.generation != 0))
        {
            return;
        }
        let generation = self.next_source_version();
        for base in (first..=last).step_by(PAGE as usize) {
            if let Some(page) = self.pages.get_mut(&base).filter(|p| p.generation != 0) {
                page.generation = generation;
            }
        }
    }

    /// Non-consuming compiler observation. Executable guards are rejected;
    /// only an actual checked fetch may consume their fault.
    pub fn snapshot_code_page(
        &mut self,
        base: u64,
        out: &mut [u8; PAGE as usize],
    ) -> Result<CodeProof, Fault> {
        if !base.is_multiple_of(PAGE) {
            return Err(Fault {
                address: base,
                length: PAGE as usize,
                access: EXECUTE,
                kind: FaultKind::Address,
            });
        }
        self.access_range(base, PAGE as usize, EXECUTE)?;
        let page = self.pages.get(&base).ok_or(Fault {
            address: base,
            length: PAGE as usize,
            access: EXECUTE,
            kind: FaultKind::Unmapped,
        })?;
        self.check_page(page, base, PAGE as usize, EXECUTE, false)?;
        out.copy_from_slice(&page.data[..]);
        if page.generation == 0 {
            let generation = self.next_source_version();
            self.pages.get_mut(&base).unwrap().generation = generation;
            // Existing write capabilities must be revoked before a formerly
            // ordinary data page becomes observed executable source.
            self.mapping_changed();
        }
        Ok(CodeProof {
            owner: self.owner.clone(),
            page: base,
            generation: self.pages[&base].generation,
        })
    }

    pub fn code_valid(&self, proof: &CodeProof) -> bool {
        Rc::ptr_eq(&self.owner, &proof.owner)
            && self.pages.get(&proof.page).is_some_and(|page| {
                page.generation == proof.generation
                    && self.effective_permissions(page) & EXECUTE != 0
                    && page.protection.get() & PAGE_GUARD == 0
            })
    }

    pub fn available(&self, address: u64, length: u64) -> bool {
        address.is_multiple_of(PAGE)
            && length != 0
            && length.is_multiple_of(PAGE)
            && usize::try_from(length)
                .ok()
                .and_then(|length| self.range(address, length).ok())
                .is_some_and(|range| self.free_range(range.start, range.end))
    }

    pub fn map(&mut self, address: u64, length: u64, permissions: u8) -> Result<(), String> {
        self.map_kind(address, length, permissions, MEM_PRIVATE)
    }

    fn map_kind(
        &mut self,
        address: u64,
        length: u64,
        permissions: u8,
        kind: u32,
    ) -> Result<(), String> {
        if !address.is_multiple_of(PAGE)
            || length == 0
            || !length.is_multiple_of(PAGE)
            || permissions & !7 != 0
        {
            return Err("invalid guest mapping".into());
        }
        let range = self.range(
            address,
            usize::try_from(length).map_err(|_| "mapping too large")?,
        )?;
        if length > MAPPING_BYTE_LIMIT
            || !Self::commit_capacity(self.pages.len(), (length / PAGE) as usize)
        {
            return Err("guest mapping capacity exceeded".into());
        }
        if !self.free_range(range.start, range.end) {
            return Err("overlapping guest mapping".into());
        }
        for base in range.step_by(PAGE as usize) {
            self.page_addresses.insert(base);
            self.pages.insert(
                base,
                Page {
                    data: Box::new([0; PAGE as usize]),
                    permissions,
                    protection: Cell::new(Self::windows_protection(permissions)),
                    generation: 0,
                },
            );
        }
        self.allocations.insert(
            address,
            Allocation {
                end: address + length,
                protection: Self::windows_protection(permissions),
                kind,
                guest: false,
                protection_locked: false,
            },
        );
        self.committed(if kind == MEM_PRIVATE {
            (length / PAGE) as usize
        } else {
            0
        });
        self.mapping_changed();
        Ok(())
    }

    pub fn map_image(&mut self, address: u64, length: u64, permissions: u8) -> Result<(), String> {
        self.map_kind(address, length, permissions, MEM_IMAGE)
    }

    pub fn merge(&mut self, other: Memory) -> Result<(), String> {
        if self.mode != other.mode
            || !Self::commit_capacity(self.pages.len(), other.pages.len())
            || other
                .allocations
                .iter()
                .any(|(&base, a)| !self.free_range(base, a.end))
        {
            return Err("image mapping overlaps or exceeds address-space capacity".into());
        }
        for (address, mut page) in other.pages.into_entries() {
            page.generation = 0;
            self.pages.insert(address, page);
        }
        self.page_addresses.extend(other.page_addresses);
        self.allocations.extend(other.allocations);
        self.committed(other.private_pages);
        self.mapping_changed();
        Ok(())
    }

    pub fn protect(&mut self, address: u64, length: u64, permissions: u8) -> Result<(), String> {
        if !address.is_multiple_of(PAGE)
            || length == 0
            || !length.is_multiple_of(PAGE)
            || permissions & !7 != 0
        {
            return Err("invalid guest protection".into());
        }
        let range = self.range(
            address,
            usize::try_from(length).map_err(|_| "protection too large")?,
        )?;
        if range
            .clone()
            .step_by(PAGE as usize)
            .any(|p| !self.pages.contains_key(&p))
        {
            return Err("unmapped guest protection".into());
        }
        self.revoke_code(range.start, range.end);
        for base in range.step_by(PAGE as usize) {
            let page = self.pages.get_mut(&base).unwrap();
            page.permissions = permissions;
            page.protection.set(Self::windows_protection(permissions));
        }
        self.mapping_changed();
        Ok(())
    }

    pub fn seal_runtime_readonly(&mut self, address: u64) -> Result<(), String> {
        let allocation = self
            .allocations
            .get(&address)
            .ok_or("missing runtime allocation")?;
        if allocation.guest {
            return Err("cannot seal a guest allocation".into());
        }
        self.protect(address, allocation.end - address, READ)?;
        self.allocations
            .get_mut(&address)
            .unwrap()
            .protection_locked = true;
        Ok(())
    }

    pub fn check_unmap(&self, address: u64, length: u64) -> Result<(), String> {
        if !address.is_multiple_of(PAGE) || length == 0 || !length.is_multiple_of(PAGE) {
            return Err("invalid guest unmap".into());
        }
        let range = self.range(
            address,
            usize::try_from(length).map_err(|_| "unmap too large")?,
        )?;
        if self
            .allocations
            .get(&address)
            .is_none_or(|a| a.end != range.end || a.guest)
            || range
                .step_by(PAGE as usize)
                .any(|p| !self.pages.contains_key(&p))
        {
            return Err("runtime unmap must match its owned committed allocation".into());
        }
        Ok(())
    }

    pub fn unmap(&mut self, address: u64, length: u64) -> Result<(), String> {
        self.check_unmap(address, length)?;
        if self.allocations[&address].kind == MEM_PRIVATE {
            self.private_pages -= (length / PAGE) as usize;
        }
        for base in (address..address + length).step_by(PAGE as usize) {
            self.pages.remove(&base);
            self.page_addresses.remove(&base);
        }
        self.allocations.remove(&address);
        self.mapping_changed();
        Ok(())
    }

    fn check_page(
        &self,
        page: &Page,
        address: u64,
        length: usize,
        access: u8,
        consume_guard: bool,
    ) -> Result<(), Fault> {
        let kind = if page.protection.get() & PAGE_GUARD != 0 {
            if consume_guard {
                page.protection.set(page.protection.get() & !PAGE_GUARD);
                self.mapping_changed();
            }
            Some(FaultKind::Guard)
        } else if self.effective_permissions(page) & access != access {
            Some(FaultKind::Protection)
        } else {
            None
        };
        match kind {
            Some(kind) => Err(Fault {
                address,
                length,
                access,
                kind,
            }),
            None => Ok(()),
        }
    }

    fn validate_access(
        &self,
        address: u64,
        length: usize,
        access: u8,
        consume_guard: bool,
    ) -> Result<(), Fault> {
        let range = self.access_range(address, length, access)?;
        if length == 0 {
            return Ok(());
        }
        for base in (address & !(PAGE - 1)..=(range.end - 1) & !(PAGE - 1)).step_by(PAGE as usize) {
            let page = self.pages.get(&base).ok_or(Fault {
                address,
                length,
                access,
                kind: FaultKind::Unmapped,
            })?;
            self.check_page(page, address, length, access, consume_guard)?;
        }
        Ok(())
    }

    pub fn check(&self, address: u64, length: usize, access: u8) -> Result<(), Fault> {
        self.validate_access(address, length, access, true)
    }

    pub(crate) fn probe(&self, address: u64, length: usize, access: u8) -> bool {
        self.validate_access(address, length, access, false).is_ok()
    }

    fn read_access(
        &self,
        address: u64,
        out: &mut [u8],
        access: u8,
        consume_guard: bool,
    ) -> Result<(), Fault> {
        self.validate_access(address, out.len(), access, consume_guard)?;
        let mut copied = 0;
        while copied < out.len() {
            let at = address + copied as u64;
            let offset = (at & (PAGE - 1)) as usize;
            let length = (PAGE as usize - offset).min(out.len() - copied);
            out[copied..copied + length]
                .copy_from_slice(&self.pages[&(at & !(PAGE - 1))].data[offset..offset + length]);
            copied += length;
        }
        Ok(())
    }

    pub fn read(&self, address: u64, out: &mut [u8]) -> Result<(), Fault> {
        self.read_access(address, out, READ, true)
    }

    pub fn fetch(&self, address: u64, out: &mut [u8]) -> Result<(), Fault> {
        self.read_access(address, out, EXECUTE, true)
    }

    pub fn observe(&self, address: u64, out: &mut [u8]) -> Result<(), Fault> {
        self.read_access(address, out, READ, false)
    }

    pub fn write(&mut self, address: u64, bytes: &[u8]) -> Result<(), Fault> {
        self.check(address, bytes.len(), WRITE)?;
        self.revoke_code(address, address + bytes.len() as u64);
        let mut copied = 0;
        while copied < bytes.len() {
            let at = address + copied as u64;
            let offset = (at & (PAGE - 1)) as usize;
            let length = (PAGE as usize - offset).min(bytes.len() - copied);
            self.pages.get_mut(&(at & !(PAGE - 1))).unwrap().data[offset..offset + length]
                .copy_from_slice(&bytes[copied..copied + length]);
            copied += length;
        }
        Ok(())
    }

    pub(crate) fn copy(
        &mut self,
        source: u64,
        destination: u64,
        length: usize,
    ) -> Result<(), Fault> {
        self.check(source, length, READ)?;
        self.check(destination, length, WRITE)?;
        let mut buffer = [0; PAGE as usize];
        let backward = destination > source && destination - source < length as u64;
        let mut remaining = length;
        while remaining != 0 {
            let n = remaining.min(buffer.len());
            let offset = if backward {
                remaining - n
            } else {
                length - remaining
            } as u64;
            self.read(source + offset, &mut buffer[..n])
                .expect("preflighted copy source");
            self.write(destination + offset, &buffer[..n])
                .expect("preflighted copy destination");
            remaining -= n;
        }
        Ok(())
    }

    pub fn read_int(&self, address: u64, size: usize) -> Result<u64, Fault> {
        if !matches!(size, 1 | 2 | 4 | 8) {
            return Err(Fault {
                address,
                length: size,
                access: READ,
                kind: FaultKind::Width,
            });
        }
        let mut bytes = [0; 8];
        self.read(address, &mut bytes[..size])?;
        Ok(u64::from_le_bytes(bytes))
    }

    pub fn write_int(&mut self, address: u64, size: usize, value: u64) -> Result<(), Fault> {
        if !matches!(size, 1 | 2 | 4 | 8) {
            return Err(Fault {
                address,
                length: size,
                access: WRITE,
                kind: FaultKind::Width,
            });
        }
        self.write(address, &value.to_le_bytes()[..size])
    }
}
