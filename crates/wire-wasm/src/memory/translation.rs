//! Stable, single-owner translation data read by generated Wasm. Epoch changes
//! revoke pointers before permissions or page ownership can be reused.
use super::*;

pub(crate) const CACHE_SLOTS: usize = 64;
pub(crate) const ACCESS_FAULT: u32 = 1;
pub(crate) const SOURCE_CHANGED: u32 = 2;

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub(crate) struct Translation {
    pub tag: u64,
    pub epoch: u64,
    pub pointer: usize,
    pub permissions: u32,
}

#[repr(C)]
pub struct TranslationCache {
    pub(crate) epoch: Cell<u64>,
    pub(crate) status: Cell<u32>,
    pub(crate) source: Cell<u64>,
    pub(crate) entries: [Cell<Translation>; CACHE_SLOTS],
}

impl Default for TranslationCache {
    fn default() -> Self {
        Self {
            epoch: Cell::new(1),
            status: Cell::new(0),
            source: Cell::new(0),
            entries: std::array::from_fn(|_| Cell::new(Translation::default())),
        }
    }
}

fn slot(address: u64) -> usize {
    ((address >> 12) ^ (address >> 28) ^ (address >> 44)) as usize & (CACHE_SLOTS - 1)
}

impl Memory {
    pub fn translations(&self) -> &TranslationCache {
        &self.translations
    }

    pub fn fault(&self) -> Option<Fault> {
        if self.translations.status.get() == ACCESS_FAULT {
            self.last_fault.get()
        } else {
            None
        }
    }

    pub(crate) fn clear_fault(&self) {
        self.translations.status.set(0);
        self.last_fault.set(None);
    }

    pub(crate) fn record_fault(&self, fault: Fault) {
        self.last_fault.set(Some(fault));
        self.translations.status.set(ACCESS_FAULT);
    }

    /// Returns zero for accesses requiring a concrete read/write helper. A
    /// cross-page access is not preflighted here: its helper owns guard faults.
    pub fn resolve(&self, address: u64, size: usize, access: u8) -> Result<usize, Fault> {
        if !matches!(size, 1 | 2 | 4 | 8 | 16) || access == 0 || access & !(READ | WRITE) != 0 {
            return Err(Fault {
                address,
                length: size,
                access,
                kind: FaultKind::Width,
            });
        }
        if address & (PAGE - 1) > PAGE - size as u64 {
            return Ok(0);
        }
        self.check(address, size, access)?;
        let base = address & !(PAGE - 1);
        let page = &self.pages[&base];
        let permissions = page.permissions
            & if page.generation == 0 {
                READ | WRITE
            } else {
                READ
            };
        let pointer = page.data.as_ptr() as usize;
        self.translations.entries[slot(address)].set(Translation {
            tag: base,
            epoch: self.mapping_epoch(),
            pointer,
            permissions: permissions as u32,
        });
        Ok(if permissions & access == access {
            pointer + (address - base) as usize
        } else {
            0
        })
    }

    /// Validate permissions without reading, writing, or revoking source bytes.
    /// Returns 1 and records the fault on failure.
    pub fn probe_checked(&self, address: u64, size: usize, access: u8) -> u32 {
        self.clear_fault();
        match self.check(address, size, access) {
            Ok(()) => 0,
            Err(fault) => {
                self.record_fault(fault);
                ACCESS_FAULT
            }
        }
    }

    /// A failed concrete load returns zero and records its fault for the caller.
    pub fn load_checked(&self, address: u64, size: usize, access: u8) -> u64 {
        self.clear_fault();
        match self
            .check(address, size, access)
            .and_then(|()| self.read_int(address, size))
        {
            Ok(value) => value,
            Err(fault) => {
                self.record_fault(fault);
                0
            }
        }
    }

    /// Returns 1 for a fault and 2 when a watched source page changed.
    pub fn store_checked(&mut self, address: u64, size: usize, value: u64) -> u32 {
        self.clear_fault();
        let before = self.source_version();
        match self.write_int(address, size, value) {
            Ok(()) if before != self.source_version() => SOURCE_CHANGED,
            Ok(()) => 0,
            Err(fault) => {
                self.record_fault(fault);
                ACCESS_FAULT
            }
        }
    }

    /// Both halves share permission preflight and source revocation.
    pub fn store128_checked(&mut self, address: u64, low: u64, high: u64) -> u32 {
        let bytes = (u128::from(low) | u128::from(high) << 64).to_le_bytes();
        self.store_bytes_checked(address, &bytes)
    }

    pub(crate) fn store_bytes_checked(&mut self, address: u64, bytes: &[u8]) -> u32 {
        self.clear_fault();
        let before = self.source_version();
        match self.write(address, bytes) {
            Ok(()) if before != self.source_version() => SOURCE_CHANGED,
            Ok(()) => 0,
            Err(fault) => {
                self.record_fault(fault);
                ACCESS_FAULT
            }
        }
    }

    /// Validate every selected byte before writing; untouched bytes require no
    /// read permission and cannot consume guards or revoke observed source.
    pub fn store_masked_checked(&mut self, address: u64, low: u64, high: u64, mask: u16) -> u32 {
        self.clear_fault();
        if mask == 0 {
            return 0;
        }
        for index in 0..16 {
            if mask & (1 << index) == 0 {
                continue;
            }
            let result = address
                .checked_add(index)
                .ok_or(Fault {
                    address,
                    length: 1,
                    access: WRITE,
                    kind: FaultKind::Address,
                })
                .and_then(|at| self.check(at, 1, WRITE));
            if let Err(fault) = result {
                self.record_fault(fault);
                return ACCESS_FAULT;
            }
        }
        let before = self.source_version();
        let first = address + u64::from(mask.trailing_zeros());
        let last = address + u64::from(15 - mask.leading_zeros());
        self.revoke_code(first, last + 1);
        let bytes = (u128::from(low) | u128::from(high) << 64).to_le_bytes();
        for (index, byte) in bytes.into_iter().enumerate() {
            if mask & (1 << index) != 0 {
                let at = address + index as u64;
                self.pages.get_mut(&(at & !(PAGE - 1))).unwrap().data[(at & (PAGE - 1)) as usize] =
                    byte;
            }
        }
        // Populate ordinary write capabilities for subsequent inline stores.
        // Observed source remains excluded by resolve's generation check.
        let _ = self.resolve(first, 1, WRITE);
        if first & !(PAGE - 1) != last & !(PAGE - 1) {
            let _ = self.resolve(last, 1, WRITE);
        }
        if before != self.source_version() {
            SOURCE_CHANGED
        } else {
            0
        }
    }
}
