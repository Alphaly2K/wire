/*
 * BSTR operations adapted from Hangover's Wine oleaut32/oleaut.c,
 * commit 3ea3a14941b6a122b892c84141ce9662c008081e.
 * Copyright 1999, 2000 Marcus Meissner
 * LGPL-2.1-or-later; see third_party/hangover/README.md.
 */
//! PE32 BSTR membership shares GuestHeap with Variant and WMI allocations.
use crate::{heap::GuestHeap, mem::GuestMem};
use std::collections::HashMap;

#[derive(Default)]
pub(crate) struct Bstrings32 {
    blocks: HashMap<u32, u32>,
}

impl Bstrings32 {
    pub(crate) fn owns(&self, pointer: u32) -> bool {
        self.blocks.contains_key(&pointer)
    }

    // Removal precedes heap release, including the historical failed-free path.
    pub(crate) fn release(
        &mut self,
        heap: &mut GuestHeap,
        process_heap: u32,
        pointer: u32,
    ) -> Option<bool> {
        self.blocks
            .remove(&pointer)
            .map(|base| heap.free(process_heap, base))
    }

    pub(crate) fn allocate(
        &mut self,
        mem: &mut GuestMem,
        heap: &mut GuestHeap,
        process_heap: u32,
        text: &[u16],
    ) -> Result<u32, u32> {
        let length = u32::try_from(text.len())
            .map_err(|_| 8u32)?
            .checked_mul(2)
            .ok_or(8u32)?;
        let base = heap
            .alloc(mem, process_heap, length.checked_add(6).ok_or(8u32)?, true)
            .map_err(|_| 8u32)?;
        if base == 0 {
            return Err(8);
        }
        mem.write_u32(base, length).map_err(|_| 87u32)?;
        mem.write_bytes(
            base + 4,
            &text
                .iter()
                .flat_map(|c| c.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .map_err(|_| 87u32)?;
        self.blocks.insert(base + 4, base);
        Ok(base + 4)
    }
}
