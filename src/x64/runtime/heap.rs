//! Guest process heap packs small allocations in owned data slabs.
//! Native callback/COM allocations retain their separate page ownership.
use super::{allocation::Pages, Memory, PROCESS_HEAP};
use crate::win32::catalog::HeapApi;
use std::collections::BTreeMap;
const SLAB: u64 = 65536;
#[derive(Clone, Copy)]
struct Block {
    size: usize,
    capacity: usize,
    slab: Option<u64>,
}
struct Slab {
    bucket: usize,
    free: Vec<u64>,
    live: usize,
}
#[derive(Default)]
pub(super) struct Heap {
    blocks: BTreeMap<u64, Block>,
    slabs: BTreeMap<u64, Slab>,
}
impl Heap {
    fn allocate(
        &mut self,
        mem: &mut Memory,
        size: usize,
        zero: bool,
        pages: &mut Pages<'_>,
    ) -> Result<u64, String> {
        if size > 16 * 1024 * 1024 {
            pages.set_error(mem, 8)?;
            return Ok(0);
        }
        if size > 2048 {
            let p = pages.allocate(mem, size as u64)?;
            if p != 0 {
                self.blocks.insert(
                    p,
                    Block {
                        size,
                        capacity: pages.allocations[&p].1 as usize,
                        slab: None,
                    },
                );
            }
            return Ok(p);
        }
        let bucket = size.max(16).next_power_of_two();
        let existing = self
            .slabs
            .iter()
            .find_map(|(&p, s)| (s.bucket == bucket && !s.free.is_empty()).then_some(p));
        let base = if let Some(p) = existing {
            p
        } else {
            let p = pages.allocate(mem, SLAB)?;
            if p == 0 {
                return Ok(0);
            }
            let free = (0..SLAB / bucket as u64)
                .rev()
                .map(|n| p + n * bucket as u64)
                .collect();
            self.slabs.insert(
                p,
                Slab {
                    bucket,
                    free,
                    live: 0,
                },
            );
            p
        };
        let p = *self.slabs[&base].free.last().unwrap();
        // Validate a reused block before publishing allocator metadata.
        if zero && size != 0 {
            mem.write(p, &vec![0; size])?;
        }
        let slab = self.slabs.get_mut(&base).unwrap();
        slab.free.pop();
        slab.live += 1;
        self.blocks.insert(
            p,
            Block {
                size,
                capacity: bucket,
                slab: Some(base),
            },
        );
        Ok(p)
    }
    fn free(&mut self, mem: &mut Memory, p: u64, pages: &mut Pages<'_>) -> Result<(), String> {
        let block = *self.blocks.get(&p).ok_or("invalid Win64 heap allocation")?;
        if let Some(base) = block.slab {
            let bucket = self.slabs[&base].bucket;
            let reclaim = self.slabs[&base].live == 1
                && self.slabs.values().filter(|s| s.bucket == bucket).count() > 1;
            if reclaim {
                mem.unmap(base, SLAB)?;
                pages.allocations.remove(&base);
                self.slabs.remove(&base);
            } else {
                let slab = self.slabs.get_mut(&base).unwrap();
                slab.live -= 1;
                slab.free.push(p);
            }
        } else {
            mem.unmap(p, block.capacity as u64)?;
            pages.allocations.remove(&p);
        }
        self.blocks.remove(&p);
        Ok(())
    }
    pub(super) fn call(
        &mut self,
        mem: &mut Memory,
        id: HeapApi,
        a: &[u64],
        pages: &mut Pages<'_>,
    ) -> Result<u64, String> {
        if id == HeapApi::Process {
            return Ok(PROCESS_HEAP);
        }
        let flags = if id == HeapApi::ReAlloc { 0x19 } else { 9 };
        if a[0] != PROCESS_HEAP || a[1] & !flags != 0 {
            return Err("unsupported Win64 heap/flags".into());
        }
        if id == HeapApi::Alloc {
            return self.allocate(
                mem,
                a[2].min(usize::MAX as u64) as usize,
                a[1] & 8 != 0,
                pages,
            );
        }
        if id == HeapApi::Free && a[2] == 0 {
            return Ok(1);
        }
        let block = *self
            .blocks
            .get(&a[2])
            .ok_or("invalid Win64 heap allocation")?;
        if id == HeapApi::Size {
            return Ok(block.size as u64);
        }
        if id == HeapApi::Free {
            self.free(mem, a[2], pages)?;
            return Ok(1);
        }
        debug_assert_eq!(id, HeapApi::ReAlloc);
        let size = a[3];
        if size <= block.capacity as u64 {
            if a[1] & 8 != 0 && size > block.size as u64 {
                mem.write(
                    a[2] + block.size as u64,
                    &vec![0; size as usize - block.size],
                )?;
            }
            self.blocks.get_mut(&a[2]).unwrap().size = size as usize;
            return Ok(a[2]);
        }
        if a[1] & 0x10 != 0 {
            return Ok(0);
        }
        if size > 16 * 1024 * 1024 {
            pages.set_error(mem, 8)?;
            return Ok(0);
        }
        let mut old = vec![0; block.size.min(size as usize)];
        mem.read(a[2], &mut old)?;
        let p = self.allocate(mem, size as usize, a[1] & 8 != 0, pages)?;
        if p != 0 {
            mem.write(p, &old)?;
            self.free(mem, a[2], pages)?;
        }
        Ok(p)
    }
}
