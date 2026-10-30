//! Windows virtual-memory protocols over the shared sparse address space.
use super::{
    Allocation, Memory, Page, COMMIT_BYTE_LIMIT, EXECUTE, MAPPING_BYTE_LIMIT, PAGE, READ, WRITE,
};
use crate::mem::{
    MEM_COMMIT, MEM_DECOMMIT, MEM_FREE, MEM_PRIVATE, MEM_RELEASE, MEM_RESERVE, PAGE_GUARD,
};
use std::cell::Cell;
const GRANULARITY: u64 = 65536;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryInfo {
    pub base: u64,
    pub allocation_base: u64,
    pub allocation_protect: u32,
    pub region_size: u64,
    pub state: u32,
    pub protect: u32,
    pub kind: u32,
}

impl MemoryInfo {
    pub fn encode32(self) -> Result<[u8; 28], u32> {
        let mut bytes = [0; 28];
        for (index, value) in [
            u32::try_from(self.base).map_err(|_| 87u32)?,
            u32::try_from(self.allocation_base).map_err(|_| 87u32)?,
            self.allocation_protect,
            u32::try_from(self.region_size).map_err(|_| 87u32)?,
            self.state,
            self.protect,
            self.kind,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        Ok(bytes)
    }

    pub fn encode64(self) -> [u8; 48] {
        let mut bytes = [0; 48];
        bytes[..8].copy_from_slice(&self.base.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.allocation_base.to_le_bytes());
        bytes[16..20].copy_from_slice(&self.allocation_protect.to_le_bytes());
        bytes[24..32].copy_from_slice(&self.region_size.to_le_bytes());
        bytes[32..36].copy_from_slice(&self.state.to_le_bytes());
        bytes[36..40].copy_from_slice(&self.protect.to_le_bytes());
        bytes[40..44].copy_from_slice(&self.kind.to_le_bytes());
        bytes
    }
}

impl Memory {
    pub(crate) fn map_heap(&mut self, span: u64, executable: bool) -> Result<u64, u32> {
        let base = self.find_free(span, false).ok_or(8u32)?;
        self.map(
            base,
            span,
            READ | WRITE | if executable { EXECUTE } else { 0 },
        )
        .map_err(|_| 8u32)?;
        Ok(base)
    }

    pub(crate) fn map_dib(&mut self, bytes: u64) -> Result<u64, u32> {
        let span = bytes.checked_add(PAGE - 1).ok_or(8u32)? & !(PAGE - 1);
        if span == 0 {
            return Err(87);
        }
        let base = self.find_free(span, false).ok_or(8u32)?;
        self.map_kind(base, span, READ | WRITE, crate::mem::MEM_MAPPED)
            .map_err(|_| 8u32)?;
        Ok(base)
    }
    pub(crate) fn file_view_address(&self, address: u64, size: u64) -> Result<u64, u32> {
        let span = size.checked_add(PAGE - 1).ok_or(8u32)? & !(PAGE - 1);
        let base = if address == 0 {
            self.find_free(span, false).ok_or(8u32)?
        } else {
            address
        };
        if !base.is_multiple_of(GRANULARITY) || !self.available(base, span) {
            return Err(487);
        }
        if span > MAPPING_BYTE_LIMIT
            || !Self::commit_capacity(self.pages.len(), (span / PAGE) as usize)
        {
            return Err(8);
        }
        Ok(base)
    }
    pub fn map_file_view(&mut self, address: u64, bytes: &[u8]) -> Result<u64, u32> {
        let span = (bytes.len() as u64).div_ceil(PAGE) * PAGE;
        let base = self.file_view_address(address, bytes.len() as u64)?;
        self.map_kind(base, span, READ | WRITE, crate::mem::MEM_MAPPED)
            .map_err(|_| 8u32)?;
        self.write(base, bytes).expect("owned file view copy");
        self.protect(base, span, READ)
            .expect("owned file view protection");
        let allocation = self.allocations.get_mut(&base).unwrap();
        allocation.protection = 2;
        Ok(base)
    }
    pub fn check_file_view(&self, base: u64, offset: u64, length: usize) -> Result<(), String> {
        let allocation = self
            .allocations
            .get(&base)
            .ok_or("missing file view allocation")?;
        if allocation.kind != crate::mem::MEM_MAPPED
            || allocation.guest
            || offset
                .checked_add(length as u64)
                .is_none_or(|end| end > allocation.end - base)
        {
            return Err("invalid file view update".into());
        }
        if length != 0 {
            let first = (base + offset) & !(PAGE - 1);
            let last = (base + offset + length as u64 - 1) & !(PAGE - 1);
            if (first..=last)
                .step_by(PAGE as usize)
                .any(|p| !self.pages.contains_key(&p))
            {
                return Err("missing file view page".into());
            }
        }
        Ok(())
    }
    pub fn update_file_view(&mut self, base: u64, offset: u64, bytes: &[u8]) -> Result<(), String> {
        self.check_file_view(base, offset, bytes.len())?;
        // The retained section owns these pages. Updating backing data preserves guest
        // protections/guard status and never grants WRITE permission to the guest.
        self.revoke_code(base + offset, base + offset + bytes.len() as u64);
        let mut copied = 0;
        while copied < bytes.len() {
            let at = base + offset + copied as u64;
            let page = self
                .pages
                .get_mut(&(at & !(PAGE - 1)))
                .expect("retained file view page");
            let start = (at & (PAGE - 1)) as usize;
            let n = (4096 - start).min(bytes.len() - copied);
            page.data[start..start + n].copy_from_slice(&bytes[copied..copied + n]);
            copied += n;
        }
        Ok(())
    }
    pub(super) fn committed(&mut self, private: usize) {
        self.private_pages += private;
        self.peak_pages = self.peak_pages.max(self.pages.len());
        self.peak_private_pages = self.peak_private_pages.max(self.private_pages);
    }

    pub(crate) fn process_memory(&self, extended: bool) -> Vec<u8> {
        let width = self.mode() as usize / 8;
        let mut bytes = vec![0; 8 + width * if extended { 9 } else { 8 }];
        let length = bytes.len() as u32;
        bytes[..4].copy_from_slice(&length.to_le_bytes());
        // Pages are eagerly backed; there is no paging or guest kernel pool.
        for (slot, pages) in [
            (0, self.peak_pages),
            (1, self.pages.len()),
            (6, self.private_pages),
            (7, self.peak_private_pages),
        ]
        .into_iter()
        .chain(extended.then_some((8, self.private_pages)))
        {
            let at = 8 + slot * width;
            bytes[at..at + width].copy_from_slice(&(pages as u64 * PAGE).to_le_bytes()[..width]);
        }
        bytes
    }

    pub fn memory_status(&self) -> [u8; 64] {
        let total = COMMIT_BYTE_LIMIT;
        let used = self.pages.len() as u64 * PAGE;
        let virtual_total = self.app_end() - GRANULARITY;
        let reserved: u64 = self
            .allocations
            .iter()
            .map(|(&base, a)| {
                a.end
                    .min(self.app_end())
                    .saturating_sub(base.max(GRANULARITY))
            })
            .sum();
        let mut status = [0; 64];
        status[..4].copy_from_slice(&64u32.to_le_bytes());
        status[4..8].copy_from_slice(&((used * 100 / total) as u32).to_le_bytes());
        for (offset, value) in [
            (8, total),
            (16, total - used),
            (24, total),
            (32, total - used),
            (40, virtual_total),
            (48, virtual_total.saturating_sub(reserved)),
        ] {
            status[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        status
    }
    pub(super) fn windows_protection(permissions: u8) -> u32 {
        match permissions {
            0 => 1,
            READ => 2,
            EXECUTE => 16,
            p if p == READ | EXECUTE => 32,
            p if p & EXECUTE != 0 => 64,
            _ => 4,
        }
    }
    fn protection_permissions(protection: u32) -> Result<u8, u32> {
        if protection & !0x7ff != 0
            || protection & 0x600 == 0x600
            || protection & PAGE_GUARD != 0 && (protection & 0xff == 1 || protection & 0x600 != 0)
        {
            return Err(87);
        }
        Ok(match protection & 0xff {
            1 => 0,
            2 => READ,
            4 => READ | WRITE,
            16 => EXECUTE,
            32 => READ | EXECUTE,
            64 => READ | WRITE | EXECUTE,
            _ => return Err(87), // Private WRITECOPY requires mapped-section semantics.
        })
    }
    pub(super) fn free_range(&self, start: u64, end: u64) -> bool {
        !self
            .allocations
            .range(..=start)
            .next_back()
            .is_some_and(|(_, a)| a.end > start)
            && self.allocations.range(start..end).next().is_none()
    }
    pub fn reserve_runtime(&mut self, address: u64, size: u64) -> Result<(), String> {
        if !address.is_multiple_of(PAGE) || size == 0 || !size.is_multiple_of(PAGE) {
            return Err("invalid guest runtime reservation".into());
        }
        let range = self.range(
            address,
            size.try_into()
                .map_err(|_| "runtime reservation overflow")?,
        )?;
        if !self.free_range(range.start, range.end) {
            return Err("overlapping guest runtime reservation".into());
        }
        self.allocations.insert(
            address,
            Allocation {
                end: range.end,
                protection: 1,
                kind: MEM_PRIVATE,
                guest: false,
                protection_locked: false,
            },
        );
        Ok(())
    }
    fn containing(&self, start: u64, end: u64) -> Result<(u64, Allocation), u32> {
        let (&base, allocation) = self.allocations.range(..=start).next_back().ok_or(487u32)?;
        if end > allocation.end {
            return Err(487);
        }
        Ok((base, *allocation))
    }
    fn rounded(&self, address: u64, size: u64, alignment: u64) -> Result<(u64, u64), u32> {
        if size == 0 {
            return Err(87);
        }
        let start = address & !(alignment - 1);
        let end = address
            .checked_add(size)
            .and_then(|v| v.checked_add(PAGE - 1))
            .map(|v| v & !(PAGE - 1))
            .filter(|&v| v <= self.app_end())
            .ok_or(487u32)?;
        if start < GRANULARITY || end <= start {
            return Err(487);
        }
        Ok((start, end))
    }
    fn find_free(&self, size: u64, top_down: bool) -> Option<u64> {
        let mut cursor = GRANULARITY;
        let mut found = None;
        for (&base, allocation) in &self.allocations {
            if base >= self.app_end() {
                break;
            }
            if cursor.checked_add(size)? <= base {
                if !top_down {
                    return Some(cursor);
                }
                found = Some((base - size) & !(GRANULARITY - 1));
            }
            cursor = cursor.max(allocation.end.div_ceil(GRANULARITY) * GRANULARITY);
        }
        if cursor.checked_add(size)? <= self.app_end() {
            return Some(if top_down {
                (self.app_end() - size) & !(GRANULARITY - 1)
            } else {
                cursor
            });
        }
        found
    }
    fn commit(&mut self, start: u64, end: u64, protection: u32) -> Result<(), u32> {
        let permissions = Self::protection_permissions(protection)?;
        if end - start > MAPPING_BYTE_LIMIT {
            return Err(8);
        }
        let missing = (start..end)
            .step_by(PAGE as usize)
            .filter(|p| !self.pages.contains_key(p))
            .count();
        if !Self::commit_capacity(self.pages.len(), missing) {
            return Err(8);
        }
        for p in (start..end).step_by(PAGE as usize) {
            self.page_addresses.insert(p);
            if !self.pages.contains_key(&p) {
                self.pages.insert(
                    p,
                    Page {
                        data: Box::new([0; 4096]),
                        permissions,
                        protection: Cell::new(protection),
                        generation: 0,
                    },
                );
            }
        }
        if missing != 0 {
            self.committed(missing);
            self.mapping_changed();
        }
        Ok(())
    }
    pub fn virtual_alloc(
        &mut self,
        address: u64,
        size: u64,
        flags: u32,
        protection: u32,
    ) -> Result<u64, u32> {
        Self::protection_permissions(protection)?;
        if size == 0 || flags & 0x3000 == 0 {
            return Err(87);
        }
        if flags & !0x103000 != 0 {
            return Err(50);
        }
        if address == 0 || flags & MEM_RESERVE != 0 {
            let (start, end) = if address == 0 {
                let size = size
                    .checked_add(PAGE - 1)
                    .map(|v| v & !(PAGE - 1))
                    .ok_or(8u32)?;
                let start = self.find_free(size, flags & 0x100000 != 0).ok_or(8u32)?;
                (start, start + size)
            } else {
                self.rounded(address, size, GRANULARITY)?
            };
            if !self.free_range(start, end) {
                return Err(487);
            }
            if flags & MEM_COMMIT != 0 {
                self.commit(start, end, protection)?;
            }
            self.allocations.insert(
                start,
                Allocation {
                    end,
                    protection,
                    kind: MEM_PRIVATE,
                    guest: true,
                    protection_locked: false,
                },
            );
            Ok(start)
        } else {
            let (start, end) = self.rounded(address, size, PAGE)?;
            if !self.containing(start, end)?.1.guest {
                return Err(487);
            }
            self.commit(start, end, protection)?;
            Ok(start)
        }
    }
    pub fn virtual_free(&mut self, address: u64, size: u64, flags: u32) -> Result<(), u32> {
        let before = self.pages.len();
        if flags == MEM_RELEASE {
            let allocation = self.allocations.get(&address).copied().ok_or(487u32)?;
            if size != 0 {
                return Err(87);
            }
            if !allocation.guest {
                return Err(487);
            }
            self.pages
                .retain(|&p, _| p < address || p >= allocation.end);
            self.page_addresses
                .retain(|&p| p < address || p >= allocation.end);
            self.allocations.remove(&address);
        } else if flags == MEM_DECOMMIT {
            let (start, end) = if size == 0 {
                let allocation = self.allocations.get(&address).ok_or(487u32)?;
                (address, allocation.end)
            } else {
                self.rounded(address, size, PAGE)?
            };
            if !self.containing(start, end)?.1.guest {
                return Err(487);
            }
            self.pages.retain(|&p, _| p < start || p >= end);
            self.page_addresses.retain(|&p| p < start || p >= end);
        } else {
            return Err(87);
        }
        self.private_pages -= before - self.pages.len();
        self.mapping_changed();
        Ok(())
    }
    pub fn virtual_protect(
        &mut self,
        address: u64,
        size: u64,
        protection: u32,
    ) -> Result<u32, u32> {
        let old = self.validate_protect(address, size, protection)?;
        let permissions = Self::protection_permissions(protection)?;
        let (start, end) = self.rounded(address, size, PAGE)?;
        for p in (start..end).step_by(PAGE as usize) {
            let page = self.pages.get_mut(&p).unwrap();
            page.permissions = permissions;
            page.protection.set(protection);
        }
        self.revoke_code(start, end);
        self.mapping_changed();
        Ok(old)
    }
    pub fn validate_protect(&self, address: u64, size: u64, protection: u32) -> Result<u32, u32> {
        Self::protection_permissions(protection)?;
        let (start, end) = self.rounded(address, size, PAGE)?;
        let allocation = self.containing(start, end)?.1;
        if allocation.protection_locked && protection != 2 {
            return Err(5);
        }
        if allocation.kind == crate::mem::MEM_MAPPED
            && Self::protection_permissions(protection)?
                & !Self::protection_permissions(allocation.protection)?
                != 0
        {
            return Err(5);
        }
        if (start..end)
            .step_by(PAGE as usize)
            .any(|p| !self.pages.contains_key(&p))
        {
            return Err(487);
        }
        Ok(self.pages[&start].protection.get())
    }
    pub fn virtual_query(&self, address: u64) -> Result<MemoryInfo, u32> {
        if address >= self.app_end() {
            return Err(87);
        }
        let start = address & !(PAGE - 1);
        let allocation = self
            .allocations
            .range(..=start)
            .next_back()
            .filter(|(_, a)| a.end > start);
        let (base, initial, state, protection, kind, end) =
            if let Some((&base, allocation)) = allocation {
                let limit = allocation.end.min(self.app_end());
                if let Some(page) = self.pages.get(&start) {
                    let protection = page.protection.get();
                    let mut end = start + PAGE;
                    while end < limit
                        && self
                            .pages
                            .get(&end)
                            .is_some_and(|p| p.protection.get() == protection)
                    {
                        end += PAGE;
                    }
                    (
                        base,
                        allocation.protection,
                        MEM_COMMIT,
                        protection,
                        allocation.kind,
                        end,
                    )
                } else {
                    let end = self
                        .page_addresses
                        .range(start..limit)
                        .next()
                        .copied()
                        .unwrap_or(limit);
                    (
                        base,
                        allocation.protection,
                        MEM_RESERVE,
                        0,
                        allocation.kind,
                        end,
                    )
                }
            } else {
                let end = self
                    .allocations
                    .range(start..)
                    .next()
                    .map(|(&b, _)| b.min(self.app_end()))
                    .unwrap_or(self.app_end());
                (0, 0, MEM_FREE, 0, 0, end)
            };
        Ok(MemoryInfo {
            base: start,
            allocation_base: base,
            allocation_protect: initial,
            region_size: end - start,
            state,
            protect: protection,
            kind,
        })
    }
}
