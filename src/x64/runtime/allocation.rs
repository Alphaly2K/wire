//! Narrow access to the existing Win64 data-page allocation ledger.
use super::{Memory, Trace, PAGE, READ, WRITE};
use std::collections::BTreeMap;

pub(super) struct Pages<'a> {
    pub next: &'a mut u64,
    pub allocations: &'a mut BTreeMap<u64, (usize, u64)>,
    pub last_error: u64,
    pub trace: &'a mut Trace,
}

impl Pages<'_> {
    pub fn set_error(&self, mem: &mut Memory, error: u32) -> Result<(), String> {
        mem.write_int(self.last_error, 4, error as u64)
    }

    pub fn allocate(&mut self, mem: &mut Memory, size: u64) -> Result<u64, String> {
        if size > 16 * 1024 * 1024 {
            self.set_error(mem, 8)?;
            return Ok(0);
        }
        let span = size.max(1).div_ceil(PAGE) * PAGE;
        let address = *self.next;
        if let Err(error) = mem.map(address, span, READ | WRITE) {
            if self.trace.len() == 4096 {
                self.trace.pop_front();
            }
            self.trace.push_back(format!(
                "Win64 allocation failed: address={address:#x} size={size} span={span}: {error}"
            ));
            self.set_error(mem, 8)?;
            return Ok(0);
        }
        *self.next += span;
        self.allocations.insert(address, (size as usize, span));
        Ok(address)
    }
}
