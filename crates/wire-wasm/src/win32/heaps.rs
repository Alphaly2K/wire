//! Guest heaps share VMM pages; free spans are indexed by size and address.
use super::{
    call::Call,
    catalog::{ApiId, Contract, HeapApi as H, LocalMemoryApi as L},
    context, ServiceError,
};
use crate::{
    machine::State,
    memory::{Fault, Memory, PAGE, READ, WRITE},
};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

const CHUNK: u64 = 65536;
const MAX_BLOCK: u64 = 256 * 1024 * 1024;
const METADATA_BUDGET: usize = 64 * 1024 * 1024;
// Include the allocation record and its share of chunk/free-tree storage.
const BLOCK_METADATA: usize = 256;

struct Metadata {
    used: Cell<usize>,
    limit: usize,
}

#[derive(Clone, Copy)]
struct Block {
    size: u64,
    capacity: u64,
    chunk: u64,
}
struct Chunk {
    span: u64,
    live: usize,
}
struct Heap {
    metadata: Rc<Metadata>,
    maximum: u64,
    used: u64,
    executable: bool,
    blocks: BTreeMap<u64, Block>,
    chunks: BTreeMap<u64, Chunk>,
    by_address: BTreeMap<u64, u64>,
    by_size: BTreeSet<(u64, u64)>,
    spare: Option<u64>,
}

#[derive(Debug)]
pub(super) enum Error {
    Windows(u32),
    Fault(Fault),
    Backing,
}
impl From<Fault> for Error {
    fn from(f: Fault) -> Self {
        Self::Fault(f)
    }
}
type Result<T> = std::result::Result<T, Error>;

impl Heap {
    fn new(maximum: u64, executable: bool, metadata: Rc<Metadata>) -> Self {
        Self {
            metadata,
            maximum,
            executable,
            used: 0,
            blocks: BTreeMap::new(),
            chunks: BTreeMap::new(),
            by_address: BTreeMap::new(),
            by_size: BTreeSet::new(),
            spare: None,
        }
    }
    fn insert_free(&mut self, p: u64, size: u64) {
        debug_assert_ne!(size, 0);
        self.by_address.insert(p, size);
        self.by_size.insert((size, p));
    }
    fn remove_free(&mut self, p: u64) -> u64 {
        let size = self.by_address.remove(&p).expect("indexed heap free span");
        self.by_size.remove(&(size, p));
        size
    }
    fn occupy(&mut self, p: u64, size: u64) {
        let available = self.remove_free(p);
        debug_assert!(available >= size);
        if available > size {
            self.insert_free(p + size, available - size);
        }
    }
    fn release(&mut self, mut p: u64, mut size: u64, chunk: u64) {
        let end = chunk + self.chunks[&chunk].span;
        if let Some((&prev, &n)) = self.by_address.range(chunk..p).next_back() {
            if prev + n == p {
                self.remove_free(prev);
                p = prev;
                size += n;
            }
        }
        if p + size < end {
            if let Some(n) = self.by_address.get(&(p + size)).copied() {
                self.remove_free(p + size);
                size += n;
            }
        }
        self.insert_free(p, size);
    }
    fn capacity(size: u64) -> Result<u64> {
        if size > MAX_BLOCK {
            return Err(Error::Windows(8));
        }
        Ok(size.max(1).div_ceil(16) * 16)
    }
    fn fits(&self, extra: u64, credit: u64) -> bool {
        self.maximum == 0 || self.used - credit + extra <= self.maximum
    }
    fn add_chunk(&mut self, base: u64, span: u64) {
        self.chunks.insert(base, Chunk { span, live: 0 });
        self.insert_free(base, span);
    }
    fn allocate(
        &mut self,
        memory: &mut Memory,
        size: u64,
        zero: bool,
        source: Option<u64>,
    ) -> Result<u64> {
        let cap = Self::capacity(size)?;
        let old = source.map(|p| self.blocks[&p]);
        if (source.is_none()
            && !self
                .metadata
                .used
                .get()
                .checked_add(BLOCK_METADATA)
                .is_some_and(|n| n <= self.metadata.limit))
            || !self.fits(cap, old.map_or(0, |b| b.capacity))
        {
            return Err(Error::Windows(8));
        }
        let copied = old.map_or(0, |b| b.size.min(size));
        if let Some(p) = source {
            memory.check(p, copied as usize, READ)?;
            self.check_free(memory, p)?;
        }
        let candidate = self.by_size.range((cap, 0)..).next().copied();
        let (p, chunk, fresh) = if let Some((_, p)) = candidate {
            let (&chunk, _) = self.chunks.range(..=p).next_back().unwrap();
            (p, chunk, None)
        } else {
            let mut span = cap.max(CHUNK).div_ceil(PAGE) * PAGE;
            if self.maximum != 0 {
                span = span.min(self.maximum.max(cap).div_ceil(PAGE) * PAGE);
            }
            let p = memory
                .map_heap(span, self.executable)
                .map_err(Error::Windows)?;
            (p, p, Some(span))
        };
        let written = if zero { size } else { copied };
        // Existing free pages may have guest protection or source authority.
        // No free-list/block metadata is consumed until the writes succeed.
        memory.check(p, written as usize, WRITE)?;
        if zero && size > copied {
            fill(memory, p + copied, size - copied);
        }
        if let Some(src) = source {
            let mut buffer = [0; PAGE as usize];
            let mut done = 0;
            while done < copied {
                let n = (copied - done).min(PAGE) as usize;
                memory
                    .read(src + done, &mut buffer[..n])
                    .expect("preflighted heap source");
                memory
                    .write(p + done, &buffer[..n])
                    .expect("preflighted heap destination");
                done += n as u64;
            }
        }
        if let Some(span) = fresh {
            self.add_chunk(p, span);
        }
        self.occupy(p, cap);
        self.chunks.get_mut(&chunk).unwrap().live += 1;
        if self.spare == Some(chunk) {
            self.spare = None;
        }
        self.blocks.insert(
            p,
            Block {
                size,
                capacity: cap,
                chunk,
            },
        );
        self.used += cap;
        // A moving realloc borrows one record until its old block is released.
        self.metadata
            .used
            .set(self.metadata.used.get() + BLOCK_METADATA);
        Ok(p)
    }
    fn check_free(&self, memory: &Memory, p: u64) -> Result<()> {
        let block = self.blocks.get(&p).ok_or(Error::Windows(87))?;
        let chunk = &self.chunks[&block.chunk];
        if chunk.live == 1 {
            memory
                .check_unmap(block.chunk, chunk.span)
                .map_err(|_| Error::Backing)?;
        }
        Ok(())
    }
    fn free(&mut self, memory: &mut Memory, p: u64) -> Result<()> {
        self.check_free(memory, p)?;
        let block = self.blocks[&p];
        let chunk = &self.chunks[&block.chunk];
        if chunk.live == 1 && (chunk.span != CHUNK || self.spare.is_some()) {
            memory
                .unmap(block.chunk, chunk.span)
                .map_err(|_| Error::Backing)?;
            self.release(p, block.capacity, block.chunk);
            self.remove_free(block.chunk);
            self.chunks.remove(&block.chunk);
        } else {
            if chunk.live == 1 {
                self.spare = Some(block.chunk);
            }
            self.chunks.get_mut(&block.chunk).unwrap().live -= 1;
            self.release(p, block.capacity, block.chunk);
        }
        self.used -= block.capacity;
        self.blocks.remove(&p);
        self.metadata
            .used
            .set(self.metadata.used.get() - BLOCK_METADATA);
        Ok(())
    }
    fn realloc(&mut self, memory: &mut Memory, p: u64, size: u64, flags: u32) -> Result<u64> {
        let old = *self.blocks.get(&p).ok_or(Error::Windows(87))?;
        let cap = Self::capacity(size)?;
        let extra = cap.saturating_sub(old.capacity);
        if !self.fits(extra, 0) {
            return Err(Error::Windows(8));
        }
        let adjacent = self
            .by_address
            .get(&(p + old.capacity))
            .copied()
            .unwrap_or(0);
        let chunk_end = old.chunk + self.chunks[&old.chunk].span;
        if cap <= old.capacity || (adjacent >= extra && p + cap <= chunk_end) {
            if flags & 8 != 0 && size > old.size {
                memory.check(p + old.size, (size - old.size) as usize, WRITE)?;
                fill(memory, p + old.size, size - old.size);
            }
            if extra != 0 {
                self.occupy(p + old.capacity, extra);
                self.used += extra;
            }
            self.blocks.insert(
                p,
                Block {
                    size,
                    capacity: old.capacity.max(cap),
                    ..old
                },
            );
            return Ok(p);
        }
        if flags & 16 != 0 {
            return Ok(0);
        }
        let replacement = self.allocate(memory, size, flags & 8 != 0, Some(p))?;
        self.free(memory, p)
            .expect("preflighted heap replacement release");
        Ok(replacement)
    }
}

fn fill(memory: &mut Memory, p: u64, size: u64) {
    let zeros = [0; PAGE as usize];
    let mut done = 0;
    while done < size {
        let n = (size - done).min(PAGE) as usize;
        memory
            .write(p + done, &zeros[..n])
            .expect("preflighted heap zero fill");
        done += n as u64;
    }
}

impl Drop for Heap {
    fn drop(&mut self) {
        self.metadata
            .used
            .set(self.metadata.used.get() - self.blocks.len() * BLOCK_METADATA);
    }
}

pub(crate) struct Heaps {
    metadata: Rc<Metadata>,
    process: u64,
    heaps: BTreeMap<u64, Heap>,
    task: Heap,
}
impl Heaps {
    pub(super) fn process_span(&self, base: u64) -> Option<u64> {
        self.heaps
            .get(&self.process)?
            .blocks
            .get(&base)
            .map(|b| b.capacity)
    }
    pub(super) fn check_process_free(&self, memory: &Memory, base: u64) -> Result<()> {
        self.heaps
            .get(&self.process)
            .ok_or(Error::Backing)?
            .check_free(memory, base)
    }
    pub(super) fn free_process(&mut self, memory: &mut Memory, base: u64) -> Result<()> {
        self.heaps
            .get_mut(&self.process)
            .ok_or(Error::Backing)?
            .free(memory, base)
    }
    pub(crate) fn new(process: u64) -> Self {
        let metadata = Rc::new(Metadata {
            used: Cell::new(0),
            limit: METADATA_BUDGET,
        });
        Self {
            process,
            heaps: BTreeMap::from([(process, Heap::new(0, false, metadata.clone()))]),
            task: Heap::new(0, false, metadata.clone()),
            metadata,
        }
    }
    pub(super) fn allocate_local(
        &mut self,
        memory: &mut Memory,
        size: u64,
        bytes: &[u8],
    ) -> Result<u64> {
        debug_assert!(bytes.len() as u64 <= size);
        let heap = self.heaps.get_mut(&self.process).ok_or(Error::Backing)?;
        let pointer = heap.allocate(memory, size, true, None)?;
        memory
            .write(pointer, bytes)
            .expect("preflighted local allocation payload");
        Ok(pointer)
    }
    pub(crate) fn check_clear(&self, memory: &Memory) -> std::result::Result<(), String> {
        for (&p, chunk) in &self.task.chunks {
            memory.check_unmap(p, chunk.span)?;
        }
        for (&base, heap) in &self.heaps {
            memory.check_unmap(base, PAGE)?;
            for (&p, chunk) in &heap.chunks {
                memory.check_unmap(p, chunk.span)?;
            }
        }
        Ok(())
    }
    pub(crate) fn clear(&mut self, memory: &mut Memory) {
        for (p, chunk) in std::mem::take(&mut self.task.chunks) {
            memory
                .unmap(p, chunk.span)
                .expect("preflighted COM task cleanup");
        }
        self.task = Heap::new(0, false, self.metadata.clone());
        for (base, mut heap) in std::mem::take(&mut self.heaps) {
            for (p, chunk) in std::mem::take(&mut heap.chunks) {
                memory
                    .unmap(p, chunk.span)
                    .expect("preflighted process heap cleanup");
            }
            memory
                .unmap(base, PAGE)
                .expect("preflighted heap identity cleanup");
        }
    }
    pub(super) fn allocate_task(
        &mut self,
        memory: &mut Memory,
        size: u64,
        payload: Option<&[u8]>,
    ) -> Result<u64> {
        let pointer = self.task.allocate(memory, size, payload.is_some(), None)?;
        if let Some(bytes) = payload {
            debug_assert!(bytes.len() as u64 <= size);
            memory
                .write(pointer, bytes)
                .expect("preflighted COM task payload");
        }
        Ok(pointer)
    }
    pub(super) fn free_task(&mut self, memory: &mut Memory, pointer: u64) -> Result<()> {
        if pointer == 0 {
            return Ok(());
        }
        self.task.free(memory, pointer)
    }
    pub(super) fn realloc_task(
        &mut self,
        memory: &mut Memory,
        pointer: u64,
        size: u64,
    ) -> Result<u64> {
        if pointer == 0 {
            return self.allocate_task(memory, size, None);
        }
        if size == 0 {
            self.free_task(memory, pointer)?;
            return Ok(0);
        }
        self.task.realloc(memory, pointer, size, 0)
    }
    pub(super) fn task_span(&self, pointer: u64) -> Option<u64> {
        self.task.blocks.get(&pointer).map(|block| block.capacity)
    }
    pub(super) fn check_task_free(&self, memory: &Memory, pointer: u64) -> Result<()> {
        let block = self.task.blocks.get(&pointer).ok_or(Error::Windows(87))?;
        let chunk = &self.task.chunks[&block.chunk];
        memory
            .check_unmap(block.chunk, chunk.span)
            .map_err(|_| Error::Backing)
    }
    fn create(
        &mut self,
        memory: &mut Memory,
        flags: u32,
        initial: u64,
        maximum: u64,
    ) -> Result<u64> {
        if flags & !0x40001 != 0 {
            return Err(Error::Windows(50));
        }
        if self.heaps.len() >= 64 || initial > MAX_BLOCK {
            return Err(Error::Windows(8));
        }
        let maximum = maximum.checked_add(PAGE - 1).ok_or(Error::Windows(8))? & !(PAGE - 1);
        if maximum != 0 && initial > maximum {
            return Err(Error::Windows(87));
        }
        let mut heap = Heap::new(maximum, flags & 0x40000 != 0, self.metadata.clone());
        let base = memory.map_heap(PAGE, false).map_err(Error::Windows)?;
        if initial != 0 {
            let span = initial.div_ceil(PAGE) * PAGE;
            match memory.map_heap(span, heap.executable) {
                Ok(p) => heap.add_chunk(p, span),
                Err(code) => {
                    memory.unmap(base, PAGE).expect("unpublished heap identity");
                    return Err(Error::Windows(code));
                }
            }
        }
        self.heaps.insert(base, heap);
        Ok(base)
    }
    fn heap_call(&mut self, id: H, a: &[u64; 16], memory: &mut Memory) -> Result<u64> {
        if id == H::Process {
            return Ok(self.process);
        }
        if id == H::Create {
            return self.create(memory, a[0] as u32, a[1], a[2]);
        }
        if id == H::Enumerate {
            let count = self.heaps.len() as u64;
            if a[0] >= count {
                let ptr = memory.mode() as usize / 8;
                let mut bytes = Vec::with_capacity(count as usize * ptr);
                for base in self.heaps.keys() {
                    bytes.extend_from_slice(&base.to_le_bytes()[..ptr]);
                }
                memory.write(a[1], &bytes)?;
            }
            return Ok(count);
        }
        if id == H::Destroy {
            if a[0] == self.process {
                return Err(Error::Windows(87));
            }
            let heap = self.heaps.get(&a[0]).ok_or(Error::Windows(6))?;
            memory.check_unmap(a[0], PAGE).map_err(|_| Error::Backing)?;
            for (&p, chunk) in &heap.chunks {
                memory
                    .check_unmap(p, chunk.span)
                    .map_err(|_| Error::Backing)?;
            }
            let mut heap = self.heaps.remove(&a[0]).unwrap();
            for (p, chunk) in std::mem::take(&mut heap.chunks) {
                memory
                    .unmap(p, chunk.span)
                    .expect("preflighted private heap destruction");
            }
            memory
                .unmap(a[0], PAGE)
                .expect("preflighted heap identity destruction");
            return Ok(1);
        }
        let allowed = match id {
            H::Alloc => 9,
            H::ReAlloc => 0x19,
            _ => 1,
        };
        if a[1] as u32 & !allowed != 0 {
            return Err(Error::Windows(50));
        }
        let heap = self.heaps.get_mut(&a[0]).ok_or(Error::Windows(6))?;
        match id {
            H::Alloc => heap.allocate(memory, a[2], a[1] & 8 != 0, None),
            H::Free => {
                if a[2] != 0 {
                    heap.free(memory, a[2])?;
                }
                Ok(1)
            }
            H::ReAlloc => heap.realloc(memory, a[2], a[3], a[1] as u32),
            H::Size => heap
                .blocks
                .get(&a[2])
                .map(|b| b.size)
                .ok_or(Error::Windows(87)),
            H::Validate => Ok(u64::from(a[2] == 0 || heap.blocks.contains_key(&a[2]))),
            _ => unreachable!(),
        }
    }
    fn query_information(&self, a: &[u64; 16], memory: &mut Memory, error: u64) -> Result<u64> {
        if a[1] != 0 {
            return Err(Error::Windows(50));
        }
        if !self.heaps.contains_key(&a[0]) {
            return Err(Error::Windows(6));
        }
        let width = memory.mode() as usize / 8;
        if a[4] != 0 {
            memory.check(a[4], width, WRITE)?;
        }
        if a[3] < 4 {
            memory.check(error, 4, WRITE)?;
        } else {
            memory.check(a[2], 4, WRITE)?;
        }
        if a[4] != 0 {
            memory.write_int(a[4], width, 4)?;
        }
        if a[3] < 4 {
            return Err(Error::Windows(122));
        }
        // The owned allocator has no lookaside or low-fragmentation mode.
        memory.write_int(a[2], 4, 0)?;
        Ok(1)
    }
    fn local_call(
        &mut self,
        id: L,
        a: &[u64; 16],
        memory: &mut Memory,
        global_unlock: bool,
    ) -> Result<u64> {
        let heap = self.heaps.get_mut(&self.process).ok_or(Error::Backing)?;
        match id {
            L::Alloc => {
                if a[0] & !0x40 != 0 {
                    return Err(Error::Windows(50));
                }
                heap.allocate(memory, a[1], a[0] & 0x40 != 0, None)
            }
            L::ReAlloc => {
                if a[2] & !0x42 != 0 {
                    return Err(Error::Windows(50));
                }
                if !heap.blocks.contains_key(&a[0]) {
                    return Err(Error::Windows(6));
                }
                heap.realloc(
                    memory,
                    a[0],
                    a[1],
                    if a[2] & 2 != 0 { 0 } else { 16 } | if a[2] & 0x40 != 0 { 8 } else { 0 },
                )
            }
            L::Free if a[0] == 0 => Ok(0),
            _ => {
                let block = *heap.blocks.get(&a[0]).ok_or(Error::Windows(6))?;
                match id {
                    L::Free => {
                        heap.free(memory, a[0])?;
                        Ok(0)
                    }
                    L::Size => Ok(block.size),
                    L::Lock => Ok(a[0]),
                    L::Unlock if global_unlock => Ok(1),
                    L::Unlock => Err(Error::Windows(158)),
                    L::Flags => Ok(0),
                    _ => unreachable!(),
                }
            }
        }
    }
}

pub(super) fn implemented(id: ApiId) -> bool {
    matches!(
        id,
        ApiId::LocalMemory(_)
            | ApiId::Heap(
                H::Process
                    | H::Create
                    | H::Destroy
                    | H::Alloc
                    | H::Free
                    | H::ReAlloc
                    | H::Size
                    | H::Validate
                    | H::Enumerate
                    | H::QueryInformation
            )
    )
}

pub(super) fn dispatch(
    contract: &Contract,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: Option<&mut crate::process::Loaded>,
) -> std::result::Result<u64, ServiceError> {
    let heaps = &mut image
        .ok_or(ServiceError::Unavailable(
            "heap services require a loaded image",
        ))?
        .heaps;
    let id = contract.id;
    let a = &call.arguments;
    let result = match id {
        ApiId::Heap(H::QueryInformation) => {
            heaps.query_information(a, memory, context::error_address(state, memory.mode()))
        }
        ApiId::Heap(id) => heaps.heap_call(id, a, memory),
        ApiId::LocalMemory(id) => heaps.local_call(id, a, memory, contract.name == "GlobalUnlock"),
        _ => unreachable!(),
    };
    match result {
        Ok(value) => Ok(value),
        Err(Error::Fault(f)) => Err(ServiceError::Fault(f)),
        Err(Error::Backing) => Err(ServiceError::Unavailable("heap backing ownership was lost")),
        Err(Error::Windows(error)) => {
            memory.write_int(
                context::error_address(state, memory.mode()),
                4,
                u64::from(error),
            )?;
            Ok(match id {
                ApiId::Heap(H::Size) => memory.mode().address(u64::MAX),
                ApiId::LocalMemory(L::Free) => a[0],
                ApiId::LocalMemory(L::Flags) => 0x8000,
                _ => 0,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::Mode;

    #[test]
    fn small_allocations_can_cross_the_old_per_heap_count_limit() {
        for mode in [Mode::X86, Mode::X64] {
            let mut memory = Memory::new(mode);
            let budget = Rc::new(Metadata {
                used: Cell::new(0),
                limit: METADATA_BUDGET,
            });
            let mut heap = Heap::new(0, false, budget.clone());
            let mut pointers = Vec::new();
            for _ in 0..70_000 {
                pointers.push(heap.allocate(&mut memory, 28, false, None).unwrap());
            }
            assert_eq!(budget.used.get(), pointers.len() * BLOCK_METADATA);
            for p in [
                pointers[0],
                pointers[65_535],
                pointers[65_536],
                pointers[69_999],
            ] {
                memory.write_int(p, 4, p).unwrap();
                assert_eq!(memory.read_int(p, 4).unwrap(), p & 0xffffffff);
                assert_eq!(heap.blocks[&p].size, 28);
            }
            drop(heap);
            assert_eq!(budget.used.get(), 0);
        }
    }

    #[test]
    fn metadata_is_shared_reclaimed_and_credited_during_moving_realloc() {
        let mut memory = Memory::new(Mode::X64);
        let budget = Rc::new(Metadata {
            used: Cell::new(0),
            limit: BLOCK_METADATA * 3,
        });
        let mut first = Heap::new(0, false, budget.clone());
        let mut second = Heap::new(0, false, budget.clone());
        let a = first.allocate(&mut memory, 16, false, None).unwrap();
        first.allocate(&mut memory, 16, false, None).unwrap();
        second.allocate(&mut memory, 16, false, None).unwrap();
        memory.write_int(a, 4, 0x12345678).unwrap();
        assert!(matches!(
            second.allocate(&mut memory, 1, false, None),
            Err(Error::Windows(8))
        ));
        let moved = first.realloc(&mut memory, a, CHUNK + 1, 0).unwrap();
        assert_ne!(moved, a);
        assert_eq!(memory.read_int(moved, 4).unwrap(), 0x12345678);
        assert_eq!(budget.used.get(), budget.limit);
        first.free(&mut memory, moved).unwrap();
        assert_eq!(budget.used.get(), BLOCK_METADATA * 2);
        second.allocate(&mut memory, 1, false, None).unwrap();
        drop(first);
        assert_eq!(budget.used.get(), BLOCK_METADATA * 2);
        drop(second);
        assert_eq!(budget.used.get(), 0);
    }
}
