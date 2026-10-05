//! Windows address-space bookkeeping. This is wire's implementation of the
//! Win32 contracts, independent of Wine's host-specific NT/server backend.
use super::{Access, GuestMem, SPACE_SIZE};
use std::collections::BTreeMap;

pub const GUEST_PAGE: u32 = 4096;
pub const ALLOCATION_GRANULARITY: u32 = 65536;
pub const MEM_COMMIT: u32 = 0x1000;
pub const MEM_RESERVE: u32 = 0x2000;
pub const MEM_FREE: u32 = 0x10000;
pub const MEM_PRIVATE: u32 = 0x20000;
pub const MEM_MAPPED: u32 = 0x40000;
pub const MEM_IMAGE: u32 = 0x1000000;
pub const PAGE_NOACCESS: u32 = 1;
pub const PAGE_READONLY: u32 = 2;
pub const PAGE_READWRITE: u32 = 4;
pub const PAGE_EXECUTE: u32 = 16;
pub const PAGE_EXECUTE_READ: u32 = 32;
pub const PAGE_EXECUTE_READWRITE: u32 = 64;
pub const PAGE_GUARD: u32 = 256;
const APP_END: u64 = 0x7fff0000;
const TOP_DOWN: u32 = 0x100000;
const INVALID: u32 = 87;
const INVALID_ADDRESS: u32 = 487;
const NO_MEMORY: u32 = 8;

#[derive(Clone, Copy, PartialEq)]
enum Owner {
    Arena,
    Runtime,
    Image,
    Guest,
    Dib,
}
#[derive(Clone, Copy)]
pub(super) struct Allocation {
    end: u64,
    protect: u32,
    kind: u32,
    owner: Owner,
}

/// These arenas are reserved for existing runtime allocators. Exact images
/// and worker stacks carve their own allocations out of the reservations.
pub(super) fn layout_reservations() -> BTreeMap<u32, Allocation> {
    [
        (0x00f00000, 0x01000000),
        (0x02000000, 0x06000000),
        (0x10000000, 0x30000000),
        (0x50000000, 0x60000000),
        (0x6fff0000, 0x71000000),
        (0x7ff00000, 0x80000000),
    ]
    .into_iter()
    .map(|(base, end)| {
        (
            base,
            Allocation {
                end,
                protect: PAGE_READWRITE,
                kind: MEM_PRIVATE,
                owner: Owner::Arena,
            },
        )
    })
    .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryInfo {
    pub base: u32,
    pub allocation_base: u32,
    pub allocation_protect: u32,
    pub region_size: u32,
    pub state: u32,
    pub protect: u32,
    pub kind: u32,
}
impl MemoryInfo {
    pub fn encode(self) -> [u8; 28] {
        let mut bytes = [0; 28];
        for (i, value) in [
            self.base,
            self.allocation_base,
            self.allocation_protect,
            self.region_size,
            self.state,
            self.protect,
            self.kind,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }
}

fn protection_valid(p: u32) -> bool {
    // WRITECOPY requires mapped sections, which this allocator does not expose.
    matches!(
        p & 0xff,
        PAGE_NOACCESS
            | PAGE_READONLY
            | PAGE_READWRITE
            | PAGE_EXECUTE
            | PAGE_EXECUTE_READ
            | PAGE_EXECUTE_READWRITE
    ) && p & !0x7ff == 0
        && p & 0x600 != 0x600
        && (p & PAGE_GUARD == 0 || (p & 0xff != PAGE_NOACCESS && p & 0x600 == 0))
}
fn round_range(address: u32, len: u32, alignment: u32) -> Result<(u32, u64), u32> {
    if len == 0 {
        return Err(INVALID);
    }
    let start = address & !(alignment - 1);
    let end = (address as u64 + len as u64 + GUEST_PAGE as u64 - 1) & !(GUEST_PAGE as u64 - 1);
    if end > SPACE_SIZE || end - start as u64 > u32::MAX as u64 {
        return Err(INVALID_ADDRESS);
    }
    Ok((start, end))
}

impl GuestMem {
    fn allocation_at(&self, address: u32) -> Option<(u32, Allocation)> {
        self.allocations
            .range(..=address)
            .next_back()
            .filter(|(_, a)| (address as u64) < a.end)
            .map(|(&b, &a)| (b, a))
    }
    fn containing(&self, start: u32, end: u64) -> Result<(u32, Allocation), u32> {
        self.allocation_at(start)
            .filter(|(_, a)| end <= a.end)
            .ok_or(INVALID_ADDRESS)
    }
    /// Claim only genuinely free memory or an unused runtime arena. Never
    /// overwrite a guest allocation or another mapped image.
    fn claim(&mut self, start: u32, end: u64, allocation: Allocation) -> Result<(), u32> {
        if let Some((base, old)) = self.allocation_at(start) {
            if old.owner != Owner::Arena || end > old.end {
                return Err(INVALID_ADDRESS);
            }
            for page in start as usize / GUEST_PAGE as usize..end as usize / GUEST_PAGE as usize {
                if self.page_ok(page) {
                    return Err(INVALID_ADDRESS);
                }
            }
            self.allocations.remove(&base);
            if base < start {
                self.allocations.insert(
                    base,
                    Allocation {
                        end: start as u64,
                        ..old
                    },
                );
            }
            if end < old.end {
                self.allocations.insert(end as u32, old);
            }
        } else if self
            .allocations
            .range(start..)
            .next()
            .is_some_and(|(&b, _)| (b as u64) < end)
        {
            return Err(INVALID_ADDRESS);
        }
        self.allocations.insert(start, allocation);
        Ok(())
    }
    pub(super) fn ensure_internal(&mut self, address: u32, len: u32) -> Result<u32, u32> {
        let (start, end) = round_range(address, len, GUEST_PAGE)?;
        if let Ok((_, allocation)) = self.containing(start, end) {
            return if matches!(allocation.owner, Owner::Guest | Owner::Dib) {
                Err(INVALID_ADDRESS)
            } else {
                Ok(allocation.protect)
            };
        }
        self.claim(
            start,
            end,
            Allocation {
                end,
                protect: PAGE_EXECUTE_READWRITE,
                kind: MEM_PRIVATE,
                owner: Owner::Runtime,
            },
        )?;
        Ok(PAGE_EXECUTE_READWRITE)
    }
    pub(crate) fn map_runtime(&mut self, base: u32, len: u32) -> Result<(), u32> {
        self.map_owned(base, len, MEM_PRIVATE, Owner::Runtime, PAGE_READWRITE)
    }
    pub(crate) fn map_image(&mut self, base: u32, len: u32) -> Result<(), u32> {
        self.map_owned(base, len, MEM_IMAGE, Owner::Image, PAGE_EXECUTE_READWRITE)
    }
    /// Anonymous DIB section storage: a real guest address with a dedicated
    /// lifetime. VirtualFree must not release or recycle a selected bitmap.
    pub(crate) fn alloc_dib(&mut self, len: u32) -> Result<u32, u32> {
        let size = (len as u64).div_ceil(GUEST_PAGE as u64) * GUEST_PAGE as u64;
        if size == 0 {
            return Err(INVALID);
        }
        let base = self.find_free(size, false).ok_or(NO_MEMORY)?;
        self.map_owned(base, size as u32, MEM_MAPPED, Owner::Dib, PAGE_READWRITE)?;
        Ok(base)
    }
    pub(crate) fn free_dib(&mut self, base: u32) -> Result<(), u32> {
        let (start, allocation) = self.allocation_at(base).ok_or(INVALID_ADDRESS)?;
        if start != base || allocation.owner != Owner::Dib {
            return Err(INVALID_ADDRESS);
        }
        self.decommit_pages(base, (allocation.end - base as u64) as u32);
        self.allocations.remove(&base);
        Ok(())
    }
    fn map_owned(
        &mut self,
        base: u32,
        len: u32,
        kind: u32,
        owner: Owner,
        protect: u32,
    ) -> Result<(), u32> {
        let (start, end) = round_range(base, len, GUEST_PAGE)?;
        if start != base {
            return Err(INVALID_ADDRESS);
        }
        // Host commitment can fail, so preserve address-space metadata until
        // backing is ready. Internal mapping happens before guest execution.
        let previous = self.allocations.clone();
        self.claim(
            start,
            end,
            Allocation {
                end,
                protect,
                kind,
                owner,
            },
        )?;
        if self
            .commit_pages(start, (end - start as u64) as u32, protect)
            .is_err()
        {
            self.allocations = previous;
            return Err(NO_MEMORY);
        }
        Ok(())
    }
    pub(super) fn unmap_image(&mut self, base: u32, len: u32) -> Result<(), u32> {
        let (_, end) = round_range(base, len, GUEST_PAGE)?;
        let (allocation_base, allocation) = self.containing(base, end)?;
        if allocation_base != base || allocation.end != end || allocation.owner != Owner::Image {
            return Err(INVALID_ADDRESS);
        }
        self.decommit_pages(base, (end - base as u64) as u32);
        self.allocations.remove(&base);
        if base >= 0x50000000 && end <= 0x60000000 {
            let mut start = base;
            let mut arena_end = end;
            if let Some((&b, &a)) = self.allocations.range(..base).next_back() {
                if a.owner == Owner::Arena && a.end == base as u64 {
                    start = b;
                    self.allocations.remove(&b);
                }
            }
            if let Some(&a) = self.allocations.get(&(end as u32)) {
                if a.owner == Owner::Arena {
                    arena_end = a.end;
                    self.allocations.remove(&(end as u32));
                }
            }
            self.allocations.insert(
                start,
                Allocation {
                    end: arena_end,
                    protect: PAGE_READWRITE,
                    kind: MEM_PRIVATE,
                    owner: Owner::Arena,
                },
            );
        }
        Ok(())
    }
    pub(crate) fn image_snapshot(&self, base: u32, len: u32) -> Result<Vec<u8>, u32> {
        let (_, a) = self.containing(base, base as u64 + len as u64)?;
        if a.owner != Owner::Image {
            return Err(INVALID_ADDRESS);
        }
        let (start, end) = round_range(base, len, GUEST_PAGE)?;
        if !(start as usize / GUEST_PAGE as usize..end as usize / GUEST_PAGE as usize)
            .all(|p| self.page_ok(p))
        {
            return Err(INVALID_ADDRESS);
        }
        // Loader inspection may include NOACCESS padding and execute-only
        // sections; this privileged copy is never used by CPU/guest APIs.
        let mut image = vec![0; len as usize];
        unsafe {
            std::ptr::copy_nonoverlapping(
                self.base.add(base as usize),
                image.as_mut_ptr(),
                image.len(),
            )
        };
        Ok(image)
    }
    pub(crate) fn protect_image(&mut self, base: u32, pe: &crate::pe::PeFile) -> Result<(), u32> {
        if pe.size_of_headers > pe.size_of_image {
            return Err(INVALID_ADDRESS);
        }
        let (_, a) = self.containing(base, base as u64 + pe.size_of_image as u64)?;
        if a.owner != Owner::Image {
            return Err(INVALID_ADDRESS);
        }
        let count = ((a.end - base as u64) / GUEST_PAGE as u64) as usize;
        let mut flags = vec![0u8; count];
        for p in flags
            .iter_mut()
            .take(pe.size_of_headers.div_ceil(GUEST_PAGE) as usize)
        {
            *p |= 1;
        }
        for section in &pe.sections {
            let span = section.virtual_size.max(section.raw_size);
            if span == 0 {
                continue;
            }
            let end = section.virtual_address as u64 + span as u64;
            if end > pe.size_of_image as u64 {
                return Err(INVALID_ADDRESS);
            }
            let bits = u8::from(section.characteristics & 0x40000000 != 0)
                | (u8::from(section.is_writable()) << 1)
                | (u8::from(section.is_executable()) << 2);
            for p in &mut flags[section.virtual_address as usize / GUEST_PAGE as usize
                ..end.div_ceil(GUEST_PAGE as u64) as usize]
            {
                *p |= bits;
            }
        }
        for (i, bits) in flags.into_iter().enumerate() {
            let p = match bits {
                0 => PAGE_NOACCESS,
                1 => PAGE_READONLY,
                2 | 3 => PAGE_READWRITE,
                4 => PAGE_EXECUTE,
                5 => PAGE_EXECUTE_READ,
                _ => PAGE_EXECUTE_READWRITE,
            };
            self.protection[base as usize / GUEST_PAGE as usize + i].set(p as u16);
        }
        self.note_write(base, (a.end - base as u64) as usize);
        Ok(())
    }
    fn find_free(&self, size: u64, top_down: bool) -> Option<u32> {
        let mut cursor = 0x10000u64;
        let mut found = None;
        for (&base, allocation) in &self.allocations {
            let limit = (base as u64).min(APP_END);
            if cursor + size <= limit {
                let candidate = if top_down {
                    (limit - size) & !(ALLOCATION_GRANULARITY as u64 - 1)
                } else {
                    cursor
                };
                if candidate >= cursor {
                    found = Some(candidate as u32);
                    if !top_down {
                        return found;
                    }
                }
            }
            cursor = cursor.max(
                allocation.end.div_ceil(ALLOCATION_GRANULARITY as u64)
                    * ALLOCATION_GRANULARITY as u64,
            );
            if cursor >= APP_END {
                return found;
            }
        }
        if cursor + size <= APP_END {
            found = Some(if top_down {
                ((APP_END - size) & !(ALLOCATION_GRANULARITY as u64 - 1)) as u32
            } else {
                cursor as u32
            });
        }
        found
    }
    pub fn virtual_alloc(
        &mut self,
        address: u32,
        len: u32,
        flags: u32,
        protect: u32,
    ) -> Result<u32, u32> {
        if !protection_valid(protect) || len == 0 {
            return Err(INVALID);
        }
        if flags & !(MEM_RESERVE | MEM_COMMIT | TOP_DOWN) != 0 {
            return Err(50);
        }
        if flags & (MEM_RESERVE | MEM_COMMIT) == 0 {
            return Err(INVALID);
        }
        if address == 0 || flags & MEM_RESERVE != 0 {
            let (start, end) = if address == 0 {
                let size = (len as u64).div_ceil(GUEST_PAGE as u64) * GUEST_PAGE as u64;
                let base = self
                    .find_free(size, flags & TOP_DOWN != 0)
                    .ok_or(NO_MEMORY)?;
                (base, base as u64 + size)
            } else {
                round_range(address, len, ALLOCATION_GRANULARITY)?
            };
            if start < 0x10000 || end > APP_END {
                return Err(INVALID_ADDRESS);
            }
            // Guest allocations cannot carve a runtime arena.
            if self.allocation_at(start).is_some()
                || self
                    .allocations
                    .range(start..)
                    .next()
                    .is_some_and(|(&b, _)| (b as u64) < end)
            {
                return Err(INVALID_ADDRESS);
            }
            self.allocations.insert(
                start,
                Allocation {
                    end,
                    protect,
                    kind: MEM_PRIVATE,
                    owner: Owner::Guest,
                },
            );
            if flags & MEM_COMMIT != 0
                && self
                    .commit_pages(start, (end - start as u64) as u32, protect)
                    .is_err()
            {
                self.allocations.remove(&start);
                return Err(NO_MEMORY);
            }
            return Ok(start);
        }
        let (start, end) = round_range(address, len, GUEST_PAGE)?;
        let (_, allocation) = self.containing(start, end)?;
        if allocation.owner != Owner::Guest {
            return Err(INVALID_ADDRESS);
        }
        self.commit_pages(start, (end - start as u64) as u32, protect)
            .map_err(|_| NO_MEMORY)?;
        Ok(start)
    }
    pub fn virtual_free(&mut self, address: u32, len: u32, flags: u32) -> Result<(), u32> {
        let (base, allocation) = self.allocation_at(address).ok_or(INVALID_ADDRESS)?;
        if allocation.owner != Owner::Guest {
            return Err(INVALID_ADDRESS);
        }
        match flags {
            0x8000 => {
                if address != base || len != 0 {
                    return Err(INVALID);
                }
                self.decommit_pages(base, (allocation.end - base as u64) as u32);
                self.allocations.remove(&base);
            }
            0x4000 => {
                let (start, end) = if len == 0 {
                    if address != base {
                        return Err(INVALID);
                    }
                    (base, allocation.end)
                } else {
                    round_range(address, len, GUEST_PAGE)?
                };
                self.containing(start, end)?;
                self.decommit_pages(start, (end - start as u64) as u32);
            }
            _ => return Err(INVALID),
        }
        Ok(())
    }
    pub fn virtual_protect(&mut self, address: u32, len: u32, protect: u32) -> Result<u32, u32> {
        if !protection_valid(protect) {
            return Err(INVALID);
        }
        let (start, end) = round_range(address, len, GUEST_PAGE)?;
        self.containing(start, end)?;
        let pages = &self.protection
            [start as usize / GUEST_PAGE as usize..end as usize / GUEST_PAGE as usize];
        if pages.iter().any(|p| p.get() == 0) {
            return Err(INVALID_ADDRESS);
        }
        let old = pages[0].get() as u32;
        for page in pages {
            page.set(protect as u16);
        }
        self.note_write(start, (end - start as u64) as usize);
        Ok(old)
    }
    pub fn virtual_query(&self, address: u32) -> Result<MemoryInfo, u32> {
        if address as u64 >= APP_END {
            return Err(INVALID);
        }
        let start = address & !(GUEST_PAGE - 1);
        if let Some((base, a)) = self.allocation_at(start) {
            let protection = self.protection[start as usize / GUEST_PAGE as usize].get();
            let mut end = start as u64 + GUEST_PAGE as u64;
            let limit = a.end.min(APP_END);
            while end < limit
                && self.protection[end as usize / GUEST_PAGE as usize].get() == protection
            {
                end += GUEST_PAGE as u64;
            }
            Ok(MemoryInfo {
                base: start,
                allocation_base: base,
                allocation_protect: a.protect,
                region_size: (end - start as u64) as u32,
                state: if protection == 0 {
                    MEM_RESERVE
                } else {
                    MEM_COMMIT
                },
                protect: protection as u32,
                kind: a.kind,
            })
        } else {
            let end = self
                .allocations
                .range(start..)
                .next()
                .map(|(&b, _)| b as u64)
                .unwrap_or(APP_END)
                .min(APP_END);
            Ok(MemoryInfo {
                base: start,
                allocation_base: 0,
                allocation_protect: 0,
                region_size: (end - start as u64) as u32,
                state: MEM_FREE,
                protect: 0,
                kind: 0,
            })
        }
    }
    pub fn application_reserved_bytes(&self) -> u64 {
        self.allocations
            .iter()
            .map(|(&b, a)| a.end.min(APP_END).saturating_sub(b as u64))
            .sum()
    }
    pub(crate) fn protect_with_output(
        &mut self,
        address: u32,
        len: u32,
        protect: u32,
        output: u32,
    ) -> Result<(), u32> {
        if output == 0 {
            return Err(INVALID);
        }
        // Validate the kernel output at entry. It may alias a page whose new
        // permission is readonly/noaccess; host backing remains RW throughout.
        let out = self
            .access_ptr(output, 4, Access::Write, true)
            .map_err(|_| INVALID)?;
        let old = self.virtual_protect(address, len, protect)?;
        unsafe { out.cast::<u32>().write_unaligned(old.to_le()) };
        Ok(())
    }
}
