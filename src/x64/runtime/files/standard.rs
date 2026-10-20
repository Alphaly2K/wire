//! Process redirect slots and standard endpoint lifetimes are distinct state.
use crate::{
    win32::catalog::{FileApi, HandleApi},
    x64::runtime::{threads::Threads, Memory, WRITE},
};
use std::collections::BTreeMap;

pub(in crate::x64::runtime) struct StandardHandles {
    redirects: [u64; 3],
    open: [bool; 3],
    flags: [u32; 3],
    aliases: BTreeMap<u64, (usize, u32)>,
    next: u64,
}
impl Default for StandardHandles {
    fn default() -> Self {
        Self {
            redirects: [0x100, 0x101, 0x102],
            open: [true; 3],
            flags: [0; 3],
            aliases: BTreeMap::new(),
            next: 0x7fff3000,
        }
    }
}
impl StandardHandles {
    pub(in crate::x64::runtime) fn redirect_call(
        &mut self,
        mem: &mut Memory,
        id: HandleApi,
        args: &[u64],
        threads: &Threads,
    ) -> Result<u64, String> {
        Ok({
            let index = match args[0] as u32 {
                0xfffffff6 => Some(0),
                0xfffffff5 => Some(1),
                0xfffffff4 => Some(2),
                _ => None,
            };
            if let Some(index) = index {
                if id == HandleApi::GetStdHandle {
                    self.redirect(index)
                } else {
                    self.set_redirect(index, args[1]);
                    1
                }
            } else {
                threads.set_error(mem, 87)?;
                if id == HandleApi::GetStdHandle {
                    u64::MAX
                } else {
                    0
                }
            }
        })
    }

    pub(in crate::x64::runtime) fn write_output(
        &self,
        mem: &mut Memory,
        id: FileApi,
        args: &[u64],
        threads: &Threads,
    ) -> Result<u64, String> {
        Ok({
            let index = self.index(args[0]).filter(|&i| i != 0);
            if id == FileApi::WriteConsoleW || index.is_none() {
                threads.set_error(mem, 6)?;
                0
            } else if args[4] != 0 {
                threads.set_error(mem, 50)?;
                0
            } else {
                use std::io::Write;
                let count = args[2] as u32 as usize;
                if count > 16 * 1024 * 1024 {
                    return Err("Win64 stdio transfer exceeds capacity".into());
                }
                if args[3] != 0 {
                    mem.check(args[3], 4, WRITE)?;
                }
                let mut bytes = vec![0; count];
                mem.read(args[1], &mut bytes)?;
                let result = if index == Some(1) {
                    std::io::stdout().lock().write_all(&bytes)
                } else {
                    std::io::stderr().lock().write_all(&bytes)
                };
                match result {
                    Ok(()) => {
                        if args[3] != 0 {
                            mem.write_int(args[3], 4, count as u64)?;
                        }
                        1
                    }
                    Err(error) => {
                        threads.set_error(mem, crate::fs::win_error(&error))?;
                        0
                    }
                }
            }
        })
    }

    pub(in crate::x64::runtime) fn redirect(&self, slot: usize) -> u64 {
        self.redirects[slot]
    }
    pub(in crate::x64::runtime) fn set_redirect(&mut self, slot: usize, handle: u64) {
        self.redirects[slot] = handle;
    }
    #[inline]
    pub(in crate::x64::runtime) fn index(&self, handle: u64) -> Option<usize> {
        self.aliases.get(&handle).map(|h| h.0).or_else(|| {
            handle
                .checked_sub(0x100)
                .filter(|&n| n < 3 && self.open[n as usize])
                .map(|n| n as usize)
        })
    }
    #[inline]
    pub(in crate::x64::runtime) fn flags(&self, handle: u64) -> Option<u32> {
        self.aliases
            .get(&handle)
            .map(|h| h.1)
            .or_else(|| self.index(handle).map(|n| self.flags[n]))
    }
    pub(in crate::x64::runtime) fn close(&mut self, handle: u64) -> Result<(), u32> {
        if self.flags(handle).ok_or(6u32)? & 2 != 0 {
            return Err(6);
        }
        if self.aliases.remove(&handle).is_none() {
            self.open[(handle - 0x100) as usize] = false;
        }
        Ok(())
    }
    pub(in crate::x64::runtime) fn set_flags(
        &mut self,
        handle: u64,
        mask: u64,
        flags: u64,
    ) -> Option<Result<(), u32>> {
        let old = self.flags(handle)?;
        if mask & !3 != 0 {
            return Some(Err(87));
        }
        let new = old & !(mask as u32) | flags as u32 & mask as u32;
        if let Some(alias) = self.aliases.get_mut(&handle) {
            alias.1 = new;
        } else {
            self.flags[(handle - 0x100) as usize] = new;
        }
        Some(Ok(()))
    }
    pub(in crate::x64::runtime) fn duplicate(
        &mut self,
        handle: u64,
        desired: Option<u32>,
        inherit: bool,
        close_source: bool,
    ) -> Option<Result<u64, u32>> {
        let index = self.index(handle)?;
        Some((|| {
            if close_source && self.flags(handle).unwrap() & 2 != 0 {
                return Err(6);
            }
            if let Some(access) = desired {
                let allowed = if index == 0 { 0x80120089 } else { 0x40120116 };
                if access & !allowed != 0 {
                    return Err(5);
                }
                // Narrowed standard handles still need separate data access rights.
                return Err(50);
            }
            if self.aliases.len() >= 128 || self.next >= 0x7fff4000 {
                return Err(8);
            }
            let alias = self.next;
            self.next += 1;
            self.aliases.insert(alias, (index, inherit as u32));
            // The adapter closes the source after writing the guest output.
            Ok(alias)
        })())
    }
}
