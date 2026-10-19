//! Original PE32 scratch arena; guest commits precede cursor advancement.
use crate::mem::GuestMem;

pub const SCRATCH_BASE: u32 = 0x00f0_0000;
pub(crate) struct Scratch32 {
    top: u32,
}
impl Default for Scratch32 {
    fn default() -> Self {
        Self { top: SCRATCH_BASE }
    }
}
impl Scratch32 {
    pub(crate) fn position(&self) -> u32 {
        self.top
    }
    pub(crate) fn alloc(
        &mut self,
        mem: &mut GuestMem,
        size: u32,
        align: u32,
        last_error: &mut u32,
    ) -> u32 {
        let allocation = (|| {
            if !align.is_power_of_two() {
                return None;
            }
            let a = self.top.checked_add(align - 1)? & !(align - 1);
            let end = a.checked_add(size.max(1))?;
            if end > 0x01000000 || mem.commit(a, size.max(1)).is_err() {
                return None;
            }
            Some((a, end))
        })();
        let Some((a, end)) = allocation else {
            *last_error = 8;
            return 0;
        };
        self.top = end;
        a
    }
}
