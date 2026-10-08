//! Guest process heap packs small allocations in owned data slabs.
//! Native callback/COM allocations retain their separate page ownership.
use super::{Memory, Runtime64};
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
impl Runtime64 {
    fn guest_heap_allocate(
        &mut self,
        mem: &mut Memory,
        size: usize,
        zero: bool,
    ) -> Result<u64, String> {
        if size > 16 * 1024 * 1024 {
            self.set_error(mem, 8)?;
            return Ok(0);
        }
        if size > 2048 {
            let p = self.heap_alloc(mem, size as u64)?;
            if p != 0 {
                self.guest_heap.blocks.insert(
                    p,
                    Block {
                        size,
                        capacity: self.allocations[&p].1 as usize,
                        slab: None,
                    },
                );
            }
            return Ok(p);
        }
        let bucket = size.max(16).next_power_of_two();
        let existing = self
            .guest_heap
            .slabs
            .iter()
            .find_map(|(&p, s)| (s.bucket == bucket && !s.free.is_empty()).then_some(p));
        let base = if let Some(p) = existing {
            p
        } else {
            let p = self.heap_alloc(mem, SLAB)?;
            if p == 0 {
                return Ok(0);
            }
            let free = (0..SLAB / bucket as u64)
                .rev()
                .map(|n| p + n * bucket as u64)
                .collect();
            self.guest_heap.slabs.insert(
                p,
                Slab {
                    bucket,
                    free,
                    live: 0,
                },
            );
            p
        };
        let p = *self.guest_heap.slabs[&base].free.last().unwrap();
        // Validate a reused block before publishing allocator metadata.
        if zero && size != 0 {
            mem.write(p, &vec![0; size])?;
        }
        let slab = self.guest_heap.slabs.get_mut(&base).unwrap();
        slab.free.pop();
        slab.live += 1;
        self.guest_heap.blocks.insert(
            p,
            Block {
                size,
                capacity: bucket,
                slab: Some(base),
            },
        );
        Ok(p)
    }
    fn guest_heap_free(&mut self, mem: &mut Memory, p: u64) -> Result<(), String> {
        let block = *self
            .guest_heap
            .blocks
            .get(&p)
            .ok_or("invalid Win64 heap allocation")?;
        if let Some(base) = block.slab {
            let bucket = self.guest_heap.slabs[&base].bucket;
            let reclaim = self.guest_heap.slabs[&base].live == 1
                && self
                    .guest_heap
                    .slabs
                    .values()
                    .filter(|s| s.bucket == bucket)
                    .count()
                    > 1;
            if reclaim {
                mem.unmap(base, SLAB)?;
                self.allocations.remove(&base);
                self.guest_heap.slabs.remove(&base);
            } else {
                let slab = self.guest_heap.slabs.get_mut(&base).unwrap();
                slab.live -= 1;
                slab.free.push(p);
            }
        } else {
            mem.unmap(p, block.capacity as u64)?;
            self.allocations.remove(&p);
        }
        self.guest_heap.blocks.remove(&p);
        Ok(())
    }
    pub(super) fn guest_heap_call(
        &mut self,
        mem: &mut Memory,
        name: &str,
        a: &[u64],
    ) -> Result<u64, String> {
        if name == "HeapAlloc" {
            return self.guest_heap_allocate(
                mem,
                a[2].min(usize::MAX as u64) as usize,
                a[1] & 8 != 0,
            );
        }
        if name == "HeapFree" && a[2] == 0 {
            return Ok(1);
        }
        let block = *self
            .guest_heap
            .blocks
            .get(&a[2])
            .ok_or("invalid Win64 heap allocation")?;
        if name == "HeapSize" {
            return Ok(block.size as u64);
        }
        if name == "HeapFree" {
            self.guest_heap_free(mem, a[2])?;
            return Ok(1);
        }
        let size = a[3];
        if size <= block.capacity as u64 {
            if a[1] & 8 != 0 && size > block.size as u64 {
                mem.write(
                    a[2] + block.size as u64,
                    &vec![0; size as usize - block.size],
                )?;
            }
            self.guest_heap.blocks.get_mut(&a[2]).unwrap().size = size as usize;
            return Ok(a[2]);
        }
        if size > 16 * 1024 * 1024 {
            self.set_error(mem, 8)?;
            return Ok(0);
        }
        let mut old = vec![0; block.size.min(size as usize)];
        mem.read(a[2], &mut old)?;
        let p = self.guest_heap_allocate(mem, size as usize, a[1] & 8 != 0)?;
        if p != 0 {
            mem.write(p, &old)?;
            self.guest_heap_free(mem, a[2])?;
        }
        Ok(p)
    }
}
