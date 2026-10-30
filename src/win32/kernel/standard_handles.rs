//! Pure standard endpoint lifetimes, aliases and process redirection slots.
pub(crate) const ALIAS_START: u64 = 0x7fff3000;
pub(crate) const ALIAS_END: u64 = 0x7fff4000;

use crate::win32::catalog::HandleApi;
use std::collections::BTreeMap;
pub(crate) fn slot(selector: u32) -> Result<usize, u32> {
    match selector {
        0xfffffff6 => Ok(0),
        0xfffffff5 => Ok(1),
        0xfffffff4 => Ok(2),
        _ => Err(87),
    }
}

pub(crate) struct StandardHandles {
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
            next: ALIAS_START,
        }
    }
}
impl StandardHandles {
    #[allow(dead_code)]
    pub(crate) fn fork_process(&self, select: impl Fn(u64, u32) -> bool) -> Self {
        Self {
            redirects: [0; 3],
            open: std::array::from_fn(|n| self.open[n] && select(0x100 + n as u64, self.flags[n])),
            flags: self.flags,
            aliases: self
                .aliases
                .iter()
                .filter(|(h, v)| select(**h, v.1))
                .map(|(&h, &v)| (h, v))
                .collect(),
            next: self.next,
        }
    }
    #[allow(dead_code)]
    pub(crate) fn close_all(&mut self) {
        self.open = [false; 3];
        self.aliases.clear();
    }
    pub(crate) fn redirect_request(&mut self, id: HandleApi, args: &[u64]) -> Result<u64, u32> {
        let slot = slot(args[0] as u32)?;
        Ok(if id == HandleApi::GetStdHandle {
            self.redirect(slot)
        } else {
            self.set_redirect(slot, args[1]);
            1
        })
    }

    pub(crate) fn redirect(&self, slot: usize) -> u64 {
        self.redirects[slot]
    }
    pub(crate) fn set_redirect(&mut self, slot: usize, handle: u64) {
        self.redirects[slot] = handle;
    }
    #[inline]
    pub(crate) fn index(&self, handle: u64) -> Option<usize> {
        self.aliases.get(&handle).map(|h| h.0).or_else(|| {
            handle
                .checked_sub(0x100)
                .filter(|&n| n < 3 && self.open[n as usize])
                .map(|n| n as usize)
        })
    }
    #[inline]
    pub(crate) fn flags(&self, handle: u64) -> Option<u32> {
        self.aliases
            .get(&handle)
            .map(|h| h.1)
            .or_else(|| self.index(handle).map(|n| self.flags[n]))
    }
    pub(crate) fn close(&mut self, handle: u64) -> Result<(), u32> {
        if self.flags(handle).ok_or(6u32)? & 2 != 0 {
            return Err(6);
        }
        if self.aliases.remove(&handle).is_none() {
            self.open[(handle - 0x100) as usize] = false;
        }
        Ok(())
    }
    pub(crate) fn set_flags(
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
    pub(crate) fn duplicate(
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
            if self.aliases.len() >= 128 || self.next >= ALIAS_END {
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
