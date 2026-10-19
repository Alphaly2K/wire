//! Local/Global operations and ownership, borrowing only data-page resources.
use super::{allocation::Pages, Memory};
use crate::win32::catalog::LocalMemoryApi;
use std::collections::BTreeSet;

#[derive(Default)]
pub(super) struct LocalMemory {
    blocks: BTreeSet<u64>,
}

impl LocalMemory {
    pub(super) fn track(&mut self, pointer: u64) {
        self.blocks.insert(pointer);
    }

    pub(super) fn format_message(
        &mut self,
        mem: &mut Memory,
        id: crate::win32::catalog::MessageFormatApi,
        args: &[u64],
        pages: &mut Pages<'_>,
    ) -> Result<u64, String> {
        use crate::win32::x64::message_format::{prepare, Prepared};
        let error_slot = pages.last_error;
        match prepare(mem, id, args, error_slot)? {
            Prepared::Done(value) => Ok(value),
            Prepared::Allocate { bytes, count, size } => {
                let pointer = pages.allocate(mem, size)?;
                if pointer == 0 {
                    return Ok(0);
                }
                mem.write(pointer, &bytes)?;
                mem.write_int(args[4], 8, pointer)?;
                self.blocks.insert(pointer);
                Ok(count)
            }
        }
    }
    pub(super) fn call(
        &mut self,
        mem: &mut Memory,
        id: LocalMemoryApi,
        a: &[u64],
        pages: &mut Pages<'_>,
    ) -> Result<u64, String> {
        Ok(match id {
            LocalMemoryApi::Alloc => {
                if a[0] & !0x40 != 0 {
                    pages.set_error(mem, 50)?;
                    0
                } else {
                    let pointer = pages.allocate(mem, a[1])?;
                    if pointer != 0 {
                        self.blocks.insert(pointer);
                    }
                    pointer
                }
            }
            LocalMemoryApi::Size
            | LocalMemoryApi::Lock
            | LocalMemoryApi::Unlock
            | LocalMemoryApi::Flags => {
                if let Some(&(size, _)) = pages
                    .allocations
                    .get(&a[0])
                    .filter(|_| self.blocks.contains(&a[0]))
                {
                    match id {
                        LocalMemoryApi::Size => size as u64,
                        LocalMemoryApi::Unlock => {
                            pages.set_error(mem, 158)?;
                            0
                        }
                        LocalMemoryApi::Lock => a[0],
                        _ => 0,
                    }
                } else {
                    pages.set_error(mem, 6)?;
                    if id == LocalMemoryApi::Flags {
                        0x8000
                    } else {
                        0
                    }
                }
            }
            LocalMemoryApi::ReAlloc => {
                if a[2] & !0x42 != 0 {
                    pages.set_error(mem, 50)?;
                    0
                } else if let Some(&(old_size, span)) = pages
                    .allocations
                    .get(&a[0])
                    .filter(|_| self.blocks.contains(&a[0]))
                {
                    if a[1] > 16 * 1024 * 1024 {
                        pages.set_error(mem, 8)?;
                        0
                    } else if a[1] <= span {
                        if a[2] & 0x40 != 0 && a[1] > old_size as u64 {
                            mem.write(a[0] + old_size as u64, &vec![0; a[1] as usize - old_size])?;
                        }
                        pages.allocations.insert(a[0], (a[1] as usize, span));
                        a[0]
                    } else if a[2] & 2 == 0 {
                        pages.set_error(mem, 8)?;
                        0
                    } else {
                        let mut data = vec![0; old_size.min(a[1] as usize)];
                        mem.read(a[0], &mut data)?;
                        let pointer = pages.allocate(mem, a[1])?;
                        if pointer != 0 {
                            mem.write(pointer, &data)?;
                            mem.unmap(a[0], span)?;
                            pages.allocations.remove(&a[0]);
                            self.blocks.remove(&a[0]);
                            self.blocks.insert(pointer);
                        }
                        pointer
                    }
                } else {
                    pages.set_error(mem, 6)?;
                    0
                }
            }
            LocalMemoryApi::Free => {
                if a[0] == 0 {
                    0
                } else if self.blocks.contains(&a[0]) && pages.allocations.contains_key(&a[0]) {
                    let span = pages.allocations[&a[0]].1;
                    mem.unmap(a[0], span)?;
                    pages.allocations.remove(&a[0]);
                    self.blocks.remove(&a[0]);
                    0
                } else {
                    pages.set_error(mem, 6)?;
                    a[0]
                }
            }
        })
    }
}
