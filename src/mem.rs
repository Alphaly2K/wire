//! Guest 32-bit address space.
//!
//! Windows 4 KiB commitment/protection is independent of host backing pages.
//! Host backing is always data (RW, never executable). Every CPU/API access
//! checks logical guest permissions before obtaining a host pointer.

use std::cell::Cell;
use std::fmt;
mod vm;
pub use vm::{
    MemoryInfo, ALLOCATION_GRANULARITY, GUEST_PAGE, MEM_COMMIT, MEM_FREE, MEM_IMAGE, MEM_PRIVATE,
    MEM_RESERVE, PAGE_EXECUTE, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE, PAGE_GUARD,
    PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE,
};

pub const SPACE_SIZE: u64 = 1 << 32;

#[derive(Debug, Clone, Copy)]
pub struct MemFault {
    pub addr: u32,
    pub len: u32,
    pub write: bool,
    pub execute: bool,
    pub guard: bool,
}

impl fmt::Display for MemFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "guest memory {}{} fault at {:#010x} (len {})",
            if self.guard { "guard " } else { "" },
            if self.execute {
                "execute"
            } else if self.write {
                "write"
            } else {
                "read"
            },
            self.addr,
            self.len
        )
    }
}
impl std::error::Error for MemFault {}

pub struct GuestMem {
    pub base: *mut u8,
    backed: Vec<u64>,           // 1 bit per host page; not guest commitment
    protection: Vec<Cell<u16>>, // 0 = uncommitted, otherwise Windows PAGE_*
    allocations: std::collections::BTreeMap<u32, vm::Allocation>,
    host_page: usize, // 16 KiB on Apple Silicon, 4 KiB on most x86 hosts
    page_shift: u32,
    code_versions: Vec<Cell<u64>>,
}

unsafe impl Send for GuestMem {}
unsafe impl Sync for GuestMem {}

impl Drop for GuestMem {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.base as *mut libc::c_void, SPACE_SIZE as usize);
        }
    }
}

impl GuestMem {
    pub fn new() -> std::io::Result<GuestMem> {
        let p = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                SPACE_SIZE as usize,
                libc::PROT_NONE,
                libc::MAP_PRIVATE | libc::MAP_ANON,
                -1,
                0,
            )
        };
        if p == libc::MAP_FAILED {
            return Err(std::io::Error::last_os_error());
        }
        let host_page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;
        // Supported hosts use power-of-two pages. Validate before replacing
        // runtime page divisions with shifts in checked guest accesses.
        if !host_page.is_power_of_two()
            || host_page < GUEST_PAGE as usize
            || host_page as u64 > SPACE_SIZE
        {
            unsafe { libc::munmap(p, SPACE_SIZE as usize) };
            return Err(std::io::Error::other("unsupported host page size"));
        }
        Ok(GuestMem {
            base: p as *mut u8,
            backed: vec![0u64; (SPACE_SIZE as usize / host_page) / 64],
            protection: vec![Cell::new(0); SPACE_SIZE as usize / GUEST_PAGE as usize],
            allocations: vm::layout_reservations(),
            host_page,
            page_shift: host_page.trailing_zeros(),
            code_versions: vec![Cell::new(0); SPACE_SIZE as usize / host_page],
        })
    }

    #[inline]
    fn page_ok(&self, page: usize) -> bool {
        self.protection[page].get() != 0
    }

    fn host_backed(&self, page: usize) -> bool {
        self.backed[page / 64] & (1u64 << (page % 64)) != 0
    }

    pub fn is_committed(&self, addr: u32) -> bool {
        self.page_ok(addr as usize >> 12)
    }

    pub(crate) fn host_page_size(&self) -> u32 {
        self.host_page as u32
    }
    pub(crate) fn watch_code(&self, address: u32) {
        let version = &self.code_versions[address as usize >> self.page_shift];
        if version.get() == 0 {
            version.set(1);
        }
    }
    #[inline]
    pub(crate) fn code_version(&self, address: u32) -> u64 {
        self.code_versions[address as usize >> self.page_shift].get()
    }
    #[inline]
    pub(crate) fn code_page_version(&self, page: usize) -> u64 {
        self.code_versions[page].get()
    }
    #[inline]
    fn note_write(&self, address: u32, len: usize) {
        if len == 0 {
            return;
        }
        let first = address as usize >> self.page_shift;
        let last = (address as usize + len - 1) >> self.page_shift;
        for page in first..=last {
            let version = &self.code_versions[page];
            if version.get() != 0 {
                version.set(version.get().saturating_add(1));
            }
        }
    }

    /// Bytes committed below the guest's 2 GiB application-address ceiling.
    pub fn application_committed_bytes(&self) -> u64 {
        self.protection[..0x8000_0000usize / GUEST_PAGE as usize]
            .iter()
            .filter(|p| p.get() != 0)
            .count() as u64
            * GUEST_PAGE as u64
    }

    /// Internal allocation/fixture adapter. Runtime arenas default to RW;
    /// standalone fixtures retain RWX guest permissions. No host executable
    /// memory is produced. Guest VirtualAlloc uses the separate Windows API.
    pub fn commit(&mut self, addr: u32, len: u32) -> Result<(), MemFault> {
        if len == 0 {
            return Ok(());
        }
        let protect = self
            .ensure_internal(addr, len)
            .map_err(|_| Self::fault(addr, len, Access::Write, false))?;
        self.commit_pages(addr, len, protect)
    }

    fn fault(addr: u32, len: u32, access: Access, guard: bool) -> MemFault {
        MemFault {
            addr,
            len,
            write: matches!(access, Access::Write),
            execute: matches!(access, Access::Execute),
            guard,
        }
    }

    fn commit_pages(&mut self, addr: u32, len: u32, protect: u32) -> Result<(), MemFault> {
        if len == 0 || addr as u64 + len as u64 > SPACE_SIZE {
            return Err(Self::fault(addr, len, Access::Write, false));
        }
        let first = (addr as u64 >> self.page_shift) as usize;
        let last = ((addr as u64 + len as u64 - 1) >> self.page_shift) as usize;
        for page in first..=last {
            if !self.host_backed(page) {
                let r = unsafe {
                    libc::mprotect(
                        self.base.add(page * self.host_page) as *mut libc::c_void,
                        self.host_page,
                        libc::PROT_READ | libc::PROT_WRITE,
                    )
                };
                if r != 0 {
                    return Err(Self::fault(addr, len, Access::Write, false));
                }
                self.backed[page / 64] |= 1u64 << (page % 64);
            }
        }
        let first = addr as usize >> 12;
        let last = ((addr as u64 + len as u64 - 1) >> 12) as usize;
        for page in first..=last {
            if !self.page_ok(page) {
                // A decommitted 4 KiB page may share surviving host backing.
                // Zero only this page, never its committed neighbors.
                unsafe {
                    self.base
                        .add(page * GUEST_PAGE as usize)
                        .write_bytes(0, GUEST_PAGE as usize)
                };
                self.protection[page].set(protect as u16);
                self.note_write((page as u32) * GUEST_PAGE, GUEST_PAGE as usize);
            }
        }
        Ok(())
    }

    /// Remove a loader-owned image and restore its address-space reservation.
    pub(crate) fn decommit_image(&mut self, addr: u32, len: u32) -> Result<(), MemFault> {
        self.unmap_image(addr, len)
            .map_err(|_| Self::fault(addr, len, Access::Write, false))
    }

    fn decommit_pages(&mut self, addr: u32, len: u32) {
        let end = addr as usize + len as usize;
        for page in addr as usize / GUEST_PAGE as usize..end / GUEST_PAGE as usize {
            self.protection[page].set(0);
        }
        self.note_write(addr, len as usize);
        for page in (addr as usize >> self.page_shift)..=((end - 1) >> self.page_shift) {
            let first = page * self.host_page / GUEST_PAGE as usize;
            let last = first + self.host_page / GUEST_PAGE as usize;
            if self.host_backed(page) && !self.protection[first..last].iter().any(|p| p.get() != 0)
            {
                let r = unsafe {
                    libc::mprotect(
                        self.base.add(page * self.host_page).cast(),
                        self.host_page,
                        libc::PROT_NONE,
                    )
                };
                if r == 0 {
                    self.backed[page / 64] &= !(1u64 << (page % 64));
                    unsafe {
                        libc::madvise(
                            self.base.add(page * self.host_page).cast(),
                            self.host_page,
                            libc::MADV_DONTNEED,
                        )
                    };
                }
            }
        }
    }

    #[inline]
    pub(crate) fn ptr(&self, addr: u32, len: u32, write: bool) -> Result<*mut u8, MemFault> {
        self.access_ptr(
            addr,
            len,
            if write { Access::Write } else { Access::Read },
            true,
        )
    }

    #[inline]
    fn access_ptr(
        &self,
        addr: u32,
        len: u32,
        access: Access,
        consume_guard: bool,
    ) -> Result<*mut u8, MemFault> {
        let end = addr as u64 + len as u64;
        if end > SPACE_SIZE {
            return Err(Self::fault(addr, len, access, false));
        }
        if len == 0 {
            return Ok(self.base.wrapping_add(addr as usize));
        }
        let first = (addr as u64 >> 12) as usize;
        let last = ((end - 1) >> 12) as usize;
        // Scalar accesses usually check one logical page; crossing a 4 KiB
        // boundary checks every page, even inside a single host page.
        for p in first..=last {
            let protect = self.protection[p].get() as u32;
            if protect & PAGE_GUARD != 0 {
                if consume_guard {
                    self.protection[p].set((protect & !PAGE_GUARD) as u16);
                    self.note_write(p as u32 * GUEST_PAGE, GUEST_PAGE as usize);
                }
                return Err(Self::fault(addr, len, access, true));
            }
            if protect & access.mask() == 0 {
                return Err(Self::fault(addr, len, access, false));
            }
        }
        // Raw host writes (ReadFile, heap copies) use this accessor too.
        // Invalidate conservatively even if the subsequent write fails.
        if matches!(access, Access::Write) {
            self.note_write(addr, len as usize);
        }
        Ok(unsafe { self.base.add(addr as usize) })
    }

    pub(crate) fn fetch_u8(&self, addr: u32) -> Result<u8, MemFault> {
        Ok(unsafe { self.access_ptr(addr, 1, Access::Execute, true)?.read() })
    }
    pub(crate) fn fetch_u16(&self, addr: u32) -> Result<u16, MemFault> {
        Ok(u16::from_le(unsafe {
            self.access_ptr(addr, 2, Access::Execute, true)?
                .cast::<u16>()
                .read_unaligned()
        }))
    }
    pub(crate) fn fetch_u32(&self, addr: u32) -> Result<u32, MemFault> {
        Ok(u32::from_le(unsafe {
            self.access_ptr(addr, 4, Access::Execute, true)?
                .cast::<u32>()
                .read_unaligned()
        }))
    }
    pub(crate) fn inspect_code(&self, addr: u32, bytes: &mut [u8]) -> Result<(), MemFault> {
        let p = self.access_ptr(addr, bytes.len() as u32, Access::Execute, false)?;
        unsafe { std::ptr::copy_nonoverlapping(p, bytes.as_mut_ptr(), bytes.len()) };
        Ok(())
    }

    #[inline]
    pub fn read_u8(&self, addr: u32) -> Result<u8, MemFault> {
        let p = self.ptr(addr, 1, false)?;
        Ok(unsafe { p.read() })
    }
    #[inline]
    pub fn read_u16(&self, addr: u32) -> Result<u16, MemFault> {
        let p = self.ptr(addr, 2, false)?;
        Ok(u16::from_le(unsafe { p.cast::<u16>().read_unaligned() }))
    }
    #[inline]
    pub fn read_u32(&self, addr: u32) -> Result<u32, MemFault> {
        let p = self.ptr(addr, 4, false)?;
        Ok(u32::from_le(unsafe { p.cast::<u32>().read_unaligned() }))
    }
    #[inline]
    pub fn read_u64(&self, addr: u32) -> Result<u64, MemFault> {
        let p = self.ptr(addr, 8, false)?;
        Ok(u64::from_le(unsafe { p.cast::<u64>().read_unaligned() }))
    }

    #[inline]
    pub fn write_u8(&self, addr: u32, v: u8) -> Result<(), MemFault> {
        let p = self.ptr(addr, 1, true)?;
        unsafe { p.write(v) };
        Ok(())
    }
    #[inline]
    pub fn write_u16(&self, addr: u32, v: u16) -> Result<(), MemFault> {
        let p = self.ptr(addr, 2, true)?;
        unsafe { p.cast::<u16>().write_unaligned(v.to_le()) };
        Ok(())
    }
    #[inline]
    pub fn write_u32(&self, addr: u32, v: u32) -> Result<(), MemFault> {
        let p = self.ptr(addr, 4, true)?;
        unsafe { p.cast::<u32>().write_unaligned(v.to_le()) };
        Ok(())
    }
    #[inline]
    pub fn write_u64(&self, addr: u32, v: u64) -> Result<(), MemFault> {
        let p = self.ptr(addr, 8, true)?;
        unsafe { p.cast::<u64>().write_unaligned(v.to_le()) };
        Ok(())
    }

    pub fn read_bytes(&self, addr: u32, buf: &mut [u8]) -> Result<(), MemFault> {
        let len = u32::try_from(buf.len())
            .map_err(|_| Self::fault(addr, u32::MAX, Access::Read, false))?;
        let p = self.ptr(addr, len, false)?;
        unsafe { std::ptr::copy_nonoverlapping(p as *const u8, buf.as_mut_ptr(), buf.len()) };
        Ok(())
    }
    pub fn write_bytes(&self, addr: u32, buf: &[u8]) -> Result<(), MemFault> {
        let len = u32::try_from(buf.len())
            .map_err(|_| Self::fault(addr, u32::MAX, Access::Write, false))?;
        let p = self.ptr(addr, len, true)?;
        unsafe { std::ptr::copy_nonoverlapping(buf.as_ptr(), p, buf.len()) };
        Ok(())
    }

    pub fn read_cstr(&self, addr: u32, max: usize) -> Result<String, MemFault> {
        let mut s = Vec::new();
        for i in 0..max {
            let b = self.read_u8(addr.wrapping_add(i as u32))?;
            if b == 0 {
                break;
            }
            s.push(b);
        }
        Ok(String::from_utf8_lossy(&s).into_owned())
    }

    pub fn read_wstr(&self, addr: u32, max: usize) -> Result<String, MemFault> {
        let mut s = Vec::new();
        for i in 0..max {
            let u = self.read_u16(addr.wrapping_add(2 * i as u32))?;
            if u == 0 {
                break;
            }
            s.push(u);
        }
        Ok(String::from_utf16_lossy(&s))
    }
}

#[derive(Clone, Copy)]
enum Access {
    Read,
    Write,
    Execute,
}
impl Access {
    #[inline]
    fn mask(self) -> u32 {
        match self {
            Self::Read => {
                PAGE_READONLY
                    | PAGE_READWRITE
                    | 8
                    | PAGE_EXECUTE_READ
                    | PAGE_EXECUTE_READWRITE
                    | 128
            }
            Self::Write => PAGE_READWRITE | 8 | PAGE_EXECUTE_READWRITE | 128,
            Self::Execute => PAGE_EXECUTE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | 128,
        }
    }
}
