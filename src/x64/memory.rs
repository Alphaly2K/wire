//! Sparse, checked low-canonical AMD64 user address space. Host data is never executable.
use std::{cell::Cell, collections::BTreeMap};
mod vm;
pub const PAGE: u64 = 4096;
pub const READ: u8 = 1;
pub const WRITE: u8 = 2;
pub const EXECUTE: u8 = 4;
const USER_END: u64 = 1 << 47;
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
}
#[derive(Default)]
pub struct Memory {
    pages: BTreeMap<u64, Page>,
    allocations: BTreeMap<u64, Allocation>,
}
impl Memory {
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
        self.pages.append(&mut other.pages);
        self.allocations.append(&mut other.allocations);
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
            },
        );
        Ok(())
    }
    pub fn map_image(&mut self, addr: u64, len: u64, permissions: u8) -> Result<(), String> {
        self.map(addr, len, permissions)?;
        self.allocations.get_mut(&addr).unwrap().kind = crate::mem::MEM_IMAGE;
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
        for p in range.step_by(PAGE as usize) {
            let page = self.pages.get_mut(&p).unwrap();
            page.permissions = permissions;
            page.protection.set(Self::windows_protection(permissions));
        }
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
            if page.protection.get() & crate::mem::PAGE_GUARD != 0 {
                page.protection
                    .set(page.protection.get() & !crate::mem::PAGE_GUARD);
                return Err(format!("AMD64 guard-page fault at {p:#x}"));
            }
            if page.permissions & access != access {
                return Err(format!("AMD64 access {access} denied at {addr:#x}"));
            }
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
        for p in range.step_by(PAGE as usize) {
            self.pages.remove(&p);
        }
        self.allocations.remove(&addr);
        Ok(())
    }
    pub fn read(&self, addr: u64, out: &mut [u8]) -> Result<(), String> {
        self.read_access(addr, out, READ)
    }
    pub fn fetch(&self, addr: u64, out: &mut [u8]) -> Result<(), String> {
        self.read_access(addr, out, EXECUTE)
    }
    fn read_access(&self, addr: u64, out: &mut [u8], access: u8) -> Result<(), String> {
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
        self.check(addr, bytes.len(), WRITE)?;
        let mut offset = 0;
        while offset < bytes.len() {
            let at = addr + offset as u64;
            let start = (at & (PAGE - 1)) as usize;
            let n = (4096 - start).min(bytes.len() - offset);
            self.pages.get_mut(&(at & !(PAGE - 1))).unwrap().data[start..start + n]
                .copy_from_slice(&bytes[offset..offset + n]);
            offset += n;
        }
        Ok(())
    }
    pub fn read_int(&self, addr: u64, size: usize) -> Result<u64, String> {
        if !matches!(size, 1 | 2 | 4 | 8) {
            return Err("invalid AMD64 scalar width".into());
        }
        let mut bytes = [0; 8];
        self.read(addr, &mut bytes[..size])?;
        Ok(u64::from_le_bytes(bytes))
    }
    pub fn write_int(&mut self, addr: u64, size: usize, value: u64) -> Result<(), String> {
        if !matches!(size, 1 | 2 | 4 | 8) {
            return Err("invalid AMD64 scalar width".into());
        }
        self.write(addr, &value.to_le_bytes()[..size])
    }
}
