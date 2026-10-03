//! Reusable guest heap arena, separate from scratch, images and thread stacks.
use crate::mem::GuestMem;
use std::collections::{BTreeMap, HashMap};

const BASE: u32 = 0x1000_0000;
const END: u32 = 0x3000_0000;

#[derive(Clone, Copy)]
struct Block {
    heap: u32,
    size: u32,
    capacity: u32,
}

pub struct GuestHeap {
    top: u32,
    free: BTreeMap<u32, u32>,
    blocks: HashMap<u32, Block>,
    heaps: HashMap<u32, u32>, // heap -> maximum bytes (0 = unlimited)
    next_heap: u32,
}

impl GuestHeap {
    pub fn new(process_heap: u32) -> Self {
        Self {
            top: BASE,
            free: BTreeMap::new(),
            blocks: HashMap::new(),
            heaps: HashMap::from([(process_heap, 0)]),
            next_heap: 0x7006_0000,
        }
    }
    pub fn create(&mut self, maximum: u32) -> u32 {
        let h = self.next_heap;
        self.next_heap += 4;
        self.heaps.insert(h, maximum);
        h
    }
    pub fn valid(&self, heap: u32, ptr: u32) -> bool {
        self.heaps.contains_key(&heap)
            && (ptr == 0 || self.blocks.get(&ptr).is_some_and(|b| b.heap == heap))
    }
    pub fn size(&self, heap: u32, ptr: u32) -> Option<u32> {
        self.blocks
            .get(&ptr)
            .filter(|b| b.heap == heap)
            .map(|b| b.size)
    }
    fn fits(&self, heap: u32, extra: u32) -> bool {
        self.heaps.get(&heap).is_some_and(|&limit| {
            limit == 0
                || self
                    .blocks
                    .values()
                    .filter(|b| b.heap == heap)
                    .map(|b| b.capacity as u64)
                    .sum::<u64>()
                    + extra as u64
                    <= limit as u64
        })
    }
    fn capacity(size: u32) -> Option<u32> {
        size.max(1).checked_add(15).map(|n| n & !15)
    }
    fn zero(mem: &GuestMem, ptr: u32, len: u32) -> Result<(), String> {
        let zeros = [0; 4096];
        let mut done = 0;
        while done < len {
            let n = (len - done).min(4096);
            mem.write_bytes(ptr + done, &zeros[..n as usize])
                .map_err(|e| e.to_string())?;
            done += n;
        }
        Ok(())
    }
    pub fn alloc(
        &mut self,
        mem: &mut GuestMem,
        heap: u32,
        size: u32,
        zero: bool,
    ) -> Result<u32, String> {
        let Some(cap) = Self::capacity(size) else {
            return Ok(0);
        };
        if !self.fits(heap, cap) {
            return Ok(0);
        }
        let available = self
            .free
            .iter()
            .find(|(_, n)| **n >= cap)
            .map(|(&p, &n)| (p, n));
        let ptr = if let Some((p, n)) = available {
            self.free.remove(&p);
            if n > cap {
                self.free.insert(p + cap, n - cap);
            }
            p
        } else {
            let Some(end) = self.top.checked_add(cap).filter(|&p| p <= END) else {
                return Ok(0);
            };
            mem.commit(self.top, cap).map_err(|e| e.to_string())?;
            let p = self.top;
            self.top = end;
            p
        };
        self.blocks.insert(
            ptr,
            Block {
                heap,
                size,
                capacity: cap,
            },
        );
        if zero {
            Self::zero(mem, ptr, size)?;
        }
        Ok(ptr)
    }
    fn release(&mut self, mut ptr: u32, mut size: u32) {
        if let Some((&p, &n)) = self.free.range(..ptr).next_back() {
            if p + n == ptr {
                self.free.remove(&p);
                ptr = p;
                size += n;
            }
        }
        if let Some(n) = self.free.remove(&(ptr + size)) {
            size += n;
        }
        self.free.insert(ptr, size);
    }
    pub fn free(&mut self, heap: u32, ptr: u32) -> bool {
        if ptr == 0 {
            return self.heaps.contains_key(&heap);
        }
        if !self.valid(heap, ptr) {
            return false;
        }
        let b = self.blocks.remove(&ptr).unwrap();
        self.release(ptr, b.capacity);
        true
    }
    pub fn destroy(&mut self, heap: u32) -> bool {
        if !self.heaps.contains_key(&heap) {
            return false;
        }
        let ptrs: Vec<_> = self
            .blocks
            .iter()
            .filter(|(_, b)| b.heap == heap)
            .map(|(&p, _)| p)
            .collect();
        for p in ptrs {
            self.free(heap, p);
        }
        self.heaps.remove(&heap);
        true
    }
    pub fn realloc(
        &mut self,
        mem: &mut GuestMem,
        heap: u32,
        ptr: u32,
        size: u32,
        flags: u32,
    ) -> Result<u32, String> {
        let Some(old) = self.blocks.get(&ptr).copied().filter(|b| b.heap == heap) else {
            return Ok(0);
        };
        let Some(cap) = Self::capacity(size) else {
            return Ok(0);
        };
        let extra = cap.saturating_sub(old.capacity);
        if !self.fits(heap, extra) {
            return Ok(0);
        }
        let adjacent = self.free.get(&(ptr + old.capacity)).copied().unwrap_or(0);
        if cap <= old.capacity
            || adjacent >= extra
            || (ptr + old.capacity == self.top
                && self.top.checked_add(extra).is_some_and(|p| p <= END))
        {
            if cap < old.capacity {
                self.release(ptr + cap, old.capacity - cap);
            } else if extra > 0 {
                if adjacent >= extra {
                    self.free.remove(&(ptr + old.capacity));
                    if adjacent > extra {
                        self.free.insert(ptr + cap, adjacent - extra);
                    }
                } else {
                    mem.commit(self.top, extra).map_err(|e| e.to_string())?;
                    self.top += extra;
                }
            }
            self.blocks.insert(
                ptr,
                Block {
                    heap,
                    size,
                    capacity: cap,
                },
            );
            if flags & 8 != 0 && size > old.size {
                Self::zero(mem, ptr + old.size, size - old.size)?;
            }
            return Ok(ptr);
        }
        if flags & 0x10 != 0 {
            return Ok(0);
        }
        // Allow the old block's bytes against a fixed heap's limit while moving.
        let max = self.heaps[&heap];
        self.heaps.insert(heap, 0);
        let allocated = self.alloc(mem, heap, size, false);
        self.heaps.insert(heap, max);
        let p = allocated?;
        if p == 0 {
            return Ok(0);
        }
        let n = old.size.min(size);
        // Both regions belong to live blocks; ptr() checks all mapped pages.
        let src = mem.ptr(ptr, n, false).map_err(|e| e.to_string())?;
        let dst = mem.ptr(p, n, true).map_err(|e| e.to_string())?;
        unsafe {
            std::ptr::copy_nonoverlapping(src, dst, n as usize);
        }
        if flags & 8 != 0 && size > n {
            Self::zero(mem, p + n, size - n)?;
        }
        self.free(heap, ptr);
        Ok(p)
    }
}
