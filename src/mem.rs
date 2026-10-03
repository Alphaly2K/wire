//! Guest 32-bit address space.
//!
//! 4 GiB reserved as PROT_NONE up front; pages are committed (RW) on demand.
//! Every access is bounds- and commit-checked, producing a MemFault the CPU
//! core can report as a guest access violation.

use std::fmt;

pub const SPACE_SIZE: u64 = 1 << 32;

#[derive(Debug, Clone, Copy)]
pub struct MemFault {
    pub addr: u32,
    pub len: u32,
    pub write: bool,
}

impl fmt::Display for MemFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "guest memory {} fault at {:#010x} (len {})",
            if self.write { "write" } else { "read" },
            self.addr,
            self.len
        )
    }
}
impl std::error::Error for MemFault {}

pub struct GuestMem {
    pub base: *mut u8,
    committed: Vec<u64>, // 1 bit per host page
    host_page: usize,    // 16 KiB on Apple Silicon, 4 KiB on most x86 hosts
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
        Ok(GuestMem {
            base: p as *mut u8,
            committed: vec![0u64; (SPACE_SIZE as usize / host_page) / 64],
            host_page,
        })
    }

    fn page_ok(&self, page: usize) -> bool {
        self.committed[page / 64] & (1u64 << (page % 64)) != 0
    }

    fn page_mark(&mut self, page: usize) {
        self.committed[page / 64] |= 1u64 << (page % 64);
    }

    pub fn is_committed(&self, addr: u32) -> bool {
        self.page_ok(addr as usize / self.host_page)
    }

    /// Commit the host pages covering [addr, addr+len). The range is rounded
    /// out to host-page boundaries because mprotect(2) requires host-page
    /// alignment (16 KiB on Apple Silicon — guest-visible allocation
    /// granularity stays finer in the bitmap).
    pub fn commit(&mut self, addr: u32, len: u32) -> Result<(), MemFault> {
        if addr as u64 + len as u64 > SPACE_SIZE {
            return Err(MemFault {
                addr,
                len,
                write: true,
            });
        }
        if len == 0 {
            return Ok(());
        }
        let hp = self.host_page as u64;
        let first = (addr as u64 / hp) as usize;
        let last = ((addr as u64 + len as u64 - 1) / hp) as usize;
        for page in first..=last {
            if !self.page_ok(page) {
                let r = unsafe {
                    libc::mprotect(
                        self.base.add(page * self.host_page) as *mut libc::c_void,
                        self.host_page,
                        libc::PROT_READ | libc::PROT_WRITE,
                    )
                };
                if r != 0 {
                    return Err(MemFault {
                        addr,
                        len,
                        write: true,
                    });
                }
                self.page_mark(page);
            }
        }
        Ok(())
    }

    pub(crate) fn ptr(&self, addr: u32, len: u32, write: bool) -> Result<*mut u8, MemFault> {
        let end = addr as u64 + len as u64;
        if end > SPACE_SIZE {
            return Err(MemFault { addr, len, write });
        }
        if len == 0 {
            return Ok(self.base.wrapping_add(addr as usize));
        }
        let hp = self.host_page as u64;
        let first = (addr as u64 / hp) as usize;
        let last = ((end - 1) / hp) as usize;
        for p in first..=last {
            if !self.page_ok(p) {
                return Err(MemFault { addr, len, write });
            }
        }
        Ok(unsafe { self.base.add(addr as usize) })
    }

    pub fn read_u8(&self, addr: u32) -> Result<u8, MemFault> {
        let p = self.ptr(addr, 1, false)?;
        Ok(unsafe { p.read() })
    }
    pub fn read_u16(&self, addr: u32) -> Result<u16, MemFault> {
        let p = self.ptr(addr, 2, false)?;
        Ok(u16::from_le(unsafe { p.cast::<u16>().read_unaligned() }))
    }
    pub fn read_u32(&self, addr: u32) -> Result<u32, MemFault> {
        let p = self.ptr(addr, 4, false)?;
        Ok(u32::from_le(unsafe { p.cast::<u32>().read_unaligned() }))
    }
    pub fn read_u64(&self, addr: u32) -> Result<u64, MemFault> {
        let p = self.ptr(addr, 8, false)?;
        Ok(u64::from_le(unsafe { p.cast::<u64>().read_unaligned() }))
    }

    pub fn write_u8(&self, addr: u32, v: u8) -> Result<(), MemFault> {
        let p = self.ptr(addr, 1, true)?;
        unsafe { p.write(v) };
        Ok(())
    }
    pub fn write_u16(&self, addr: u32, v: u16) -> Result<(), MemFault> {
        let p = self.ptr(addr, 2, true)?;
        unsafe { p.cast::<u16>().write_unaligned(v.to_le()) };
        Ok(())
    }
    pub fn write_u32(&self, addr: u32, v: u32) -> Result<(), MemFault> {
        let p = self.ptr(addr, 4, true)?;
        unsafe { p.cast::<u32>().write_unaligned(v.to_le()) };
        Ok(())
    }
    pub fn write_u64(&self, addr: u32, v: u64) -> Result<(), MemFault> {
        let p = self.ptr(addr, 8, true)?;
        unsafe { p.cast::<u64>().write_unaligned(v.to_le()) };
        Ok(())
    }

    pub fn read_bytes(&self, addr: u32, buf: &mut [u8]) -> Result<(), MemFault> {
        let p = self.ptr(addr, buf.len() as u32, false)?;
        unsafe { std::ptr::copy_nonoverlapping(p as *const u8, buf.as_mut_ptr(), buf.len()) };
        Ok(())
    }
    pub fn write_bytes(&self, addr: u32, buf: &[u8]) -> Result<(), MemFault> {
        let p = self.ptr(addr, buf.len() as u32, true)?;
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
