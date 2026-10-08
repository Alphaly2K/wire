//! Windows virtual memory contracts over checked 64-bit data pages.
use super::{Allocation, Memory, Page, EXECUTE, PAGE, READ, USER_END, WRITE};
use crate::mem::{MEM_COMMIT, MEM_FREE, MEM_PRIVATE, MEM_RESERVE, PAGE_GUARD};
use std::cell::Cell;
const GRANULARITY: u64 = 65536;
const APP_END: u64 = USER_END - GRANULARITY;
impl Memory {
    pub(crate) fn map_file_view(&mut self, address: u64, bytes: &[u8]) -> Result<u64, u32> {
        let span = (bytes.len() as u64).div_ceil(PAGE) * PAGE;
        let base = if address == 0 {
            self.find_free(span, false).ok_or(8u32)?
        } else {
            address
        };
        if !base.is_multiple_of(GRANULARITY) || !self.available(base, span) {
            return Err(487);
        }
        self.map(base, span, READ | WRITE).map_err(|_| 8u32)?;
        self.write(base, bytes).expect("owned file view copy");
        self.protect(base, span, READ)
            .expect("owned file view protection");
        let allocation = self.allocations.get_mut(&base).unwrap();
        allocation.kind = crate::mem::MEM_MAPPED;
        allocation.protection = 2;
        Ok(base)
    }
    pub(crate) fn update_file_view(
        &mut self,
        base: u64,
        offset: u64,
        bytes: &[u8],
    ) -> Result<(), String> {
        let allocation = self
            .allocations
            .get(&base)
            .ok_or("missing file view allocation")?;
        if allocation.kind != crate::mem::MEM_MAPPED
            || allocation.guest
            || offset
                .checked_add(bytes.len() as u64)
                .is_none_or(|end| end > allocation.end - base)
        {
            return Err("invalid file view update".into());
        }
        // The retained section owns these pages. Updating backing data preserves guest
        // protections/guard status and never grants WRITE permission to the guest.
        let mut copied = 0;
        while copied < bytes.len() {
            let at = base + offset + copied as u64;
            let page = self
                .pages
                .get_mut(&(at & !(PAGE - 1)))
                .ok_or("missing file view page")?;
            let start = (at & (PAGE - 1)) as usize;
            let n = (4096 - start).min(bytes.len() - copied);
            page.data[start..start + n].copy_from_slice(&bytes[copied..copied + n]);
            copied += n;
        }
        self.invalidate_decode();
        Ok(())
    }
    pub fn memory_status(&self) -> [u8; 64] {
        let total = 512 * 1024 * 1024u64;
        let used = self.pages.len() as u64 * PAGE;
        let virtual_total = APP_END - GRANULARITY;
        let reserved: u64 = self.allocations.iter().map(|(&base, a)| a.end - base).sum();
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
            || protection & PAGE_GUARD != 0 && protection & 0xff == 1
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
            && !self.allocations.range(start..end).next().is_some()
    }
    pub(crate) fn reserve_runtime(&mut self, address: u64, size: u64) -> Result<(), String> {
        if !address.is_multiple_of(PAGE) || size == 0 || !size.is_multiple_of(PAGE) {
            return Err("invalid AMD64 runtime reservation".into());
        }
        let range = Self::range(
            address,
            size.try_into()
                .map_err(|_| "runtime reservation overflow")?,
        )?;
        if !self.free_range(range.start, range.end) {
            return Err("overlapping AMD64 runtime reservation".into());
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
    fn rounded(address: u64, size: u64, alignment: u64) -> Result<(u64, u64), u32> {
        if size == 0 {
            return Err(87);
        }
        let start = address & !(alignment - 1);
        let end = address
            .checked_add(size)
            .and_then(|v| v.checked_add(PAGE - 1))
            .map(|v| v & !(PAGE - 1))
            .filter(|&v| v <= APP_END)
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
            if base >= APP_END {
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
        if cursor.checked_add(size)? <= APP_END {
            return Some(if top_down {
                (APP_END - size) & !(GRANULARITY - 1)
            } else {
                cursor
            });
        }
        found
    }
    fn commit(&mut self, start: u64, end: u64, protection: u32) -> Result<(), u32> {
        let permissions = Self::protection_permissions(protection)?;
        if end - start > 256 * 1024 * 1024 {
            return Err(8);
        }
        let missing = (start..end)
            .step_by(PAGE as usize)
            .filter(|p| !self.pages.contains_key(p))
            .count();
        if (self.pages.len() + missing) as u64 * PAGE > 512 * 1024 * 1024 {
            return Err(8);
        }
        for p in (start..end).step_by(PAGE as usize) {
            self.pages.entry(p).or_insert_with(|| Page {
                data: Box::new([0; 4096]),
                permissions,
                protection: Cell::new(protection),
            });
        }
        self.invalidate_decode();
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
                Self::rounded(address, size, GRANULARITY)?
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
            let (start, end) = Self::rounded(address, size, PAGE)?;
            if !self.containing(start, end)?.1.guest {
                return Err(487);
            }
            self.commit(start, end, protection)?;
            Ok(start)
        }
    }
    pub fn virtual_free(&mut self, address: u64, size: u64, flags: u32) -> Result<(), u32> {
        if flags == 0x8000 {
            let allocation = self.allocations.get(&address).copied().ok_or(487u32)?;
            if size != 0 {
                return Err(87);
            }
            if !allocation.guest {
                return Err(487);
            }
            self.pages
                .retain(|&p, _| p < address || p >= allocation.end);
            self.allocations.remove(&address);
        } else if flags == 0x4000 {
            let (start, end) = if size == 0 {
                let allocation = self.allocations.get(&address).ok_or(487u32)?;
                (address, allocation.end)
            } else {
                Self::rounded(address, size, PAGE)?
            };
            if !self.containing(start, end)?.1.guest {
                return Err(487);
            }
            self.pages.retain(|&p, _| p < start || p >= end);
        } else {
            return Err(87);
        }
        self.invalidate_decode();
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
        let (start, end) = Self::rounded(address, size, PAGE)?;
        for p in (start..end).step_by(PAGE as usize) {
            let page = self.pages.get_mut(&p).unwrap();
            page.permissions = permissions;
            page.protection.set(protection);
        }
        self.invalidate_decode();
        Ok(old)
    }
    pub(crate) fn validate_protect(
        &self,
        address: u64,
        size: u64,
        protection: u32,
    ) -> Result<u32, u32> {
        Self::protection_permissions(protection)?;
        let (start, end) = Self::rounded(address, size, PAGE)?;
        if self.containing(start, end)?.1.protection_locked && protection != 2 {
            return Err(5);
        }
        if self.containing(start, end)?.1.kind == crate::mem::MEM_MAPPED
            && !matches!(protection & 0xff, 1 | 2)
        {
            return Err(5);
        }
        self.containing(start, end)?;
        if (start..end)
            .step_by(PAGE as usize)
            .any(|p| !self.pages.contains_key(&p))
        {
            return Err(487);
        }
        Ok(self.pages[&start].protection.get())
    }
    /// x64 MEMORY_BASIC_INFORMATION, including required alignment padding.
    pub fn virtual_query(&self, address: u64) -> Result<[u8; 48], u32> {
        if address >= APP_END {
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
                if let Some(page) = self.pages.get(&start) {
                    let protection = page.protection.get();
                    let mut end = start + PAGE;
                    while end < allocation.end
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
                        .pages
                        .range(start..allocation.end)
                        .next()
                        .map(|(&p, _)| p)
                        .unwrap_or(allocation.end);
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
                    .map(|(&b, _)| b.min(APP_END))
                    .unwrap_or(APP_END);
                (0, 0, MEM_FREE, 0, 0, end)
            };
        let mut info = [0; 48];
        info[..8].copy_from_slice(&start.to_le_bytes());
        info[8..16].copy_from_slice(&base.to_le_bytes());
        info[16..20].copy_from_slice(&initial.to_le_bytes());
        info[24..32].copy_from_slice(&(end - start).to_le_bytes());
        info[32..36].copy_from_slice(&state.to_le_bytes());
        info[36..40].copy_from_slice(&protection.to_le_bytes());
        info[40..44].copy_from_slice(&kind.to_le_bytes());
        Ok(info)
    }
}
