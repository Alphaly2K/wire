//! Process redirect slots and standard endpoint lifetimes are distinct state.
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
