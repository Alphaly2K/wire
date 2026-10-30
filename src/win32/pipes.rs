//! Anonymous byte pipes with process-local handles and shared endpoint storage.
use super::{byte_streams::reserve, file_rights};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, VecDeque},
    rc::Rc,
};
const BUDGET: usize = 64 * 1024 * 1024;
struct Pool {
    next: Cell<u64>,
    reserved: Cell<usize>,
    objects: Cell<usize>,
}
impl Default for Pool {
    fn default() -> Self {
        Self {
            next: Cell::new(0x10000),
            reserved: Cell::new(0),
            objects: Cell::new(0),
        }
    }
}
impl Pool {
    fn allocate(&self, count: u64) -> Result<u64, u32> {
        let next = self.next.get();
        if next + count > 0xfffff {
            return Err(8);
        }
        self.next.set(next + count);
        Ok(next)
    }
}
struct Pipe {
    data: VecDeque<u8>,
    capacity: usize,
    readers: usize,
    writers: usize,
    pool: Rc<Pool>,
}
impl Drop for Pipe {
    fn drop(&mut self) {
        self.pool
            .reserved
            .set(self.pool.reserved.get() - self.capacity.next_power_of_two());
        self.pool.objects.set(self.pool.objects.get() - 1);
    }
}
struct Handle {
    pipe: Rc<RefCell<Pipe>>,
    reader: bool,
    access: u32,
    flags: u32,
}
impl Clone for Handle {
    fn clone(&self) -> Self {
        let mut pipe = self.pipe.borrow_mut();
        if self.reader {
            pipe.readers += 1;
        } else {
            pipe.writers += 1;
        }
        Self {
            pipe: self.pipe.clone(),
            reader: self.reader,
            access: self.access,
            flags: self.flags,
        }
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        let mut pipe = self.pipe.borrow_mut();
        if self.reader {
            pipe.readers -= 1;
            if pipe.readers == 0 {
                pipe.data = VecDeque::new();
            }
        } else {
            pipe.writers -= 1;
        }
    }
}
#[derive(Default)]
pub(crate) struct Pipes {
    pool: Rc<Pool>,
    handles: BTreeMap<u64, Handle>,
}
impl Pipes {
    #[allow(dead_code)]
    pub(crate) fn fork_process(&self, select: impl Fn(u64, u32) -> bool) -> Self {
        Self {
            pool: self.pool.clone(),
            handles: self
                .handles
                .iter()
                .filter(|(h, v)| select(**h, v.flags))
                .map(|(&h, v)| (h, v.clone()))
                .collect(),
        }
    }
    pub fn create(&mut self, size: u32, inherit: bool) -> Result<(u64, u64), u32> {
        let capacity = (size as usize).max(4096);
        if size > 16 * 1024 * 1024
            || self.pool.objects.get() >= 1024
            || self.handles.len() > 4094
            || capacity.next_power_of_two() > BUDGET - self.pool.reserved.get()
        {
            return Err(8);
        }
        let read = self.pool.allocate(2)?;
        self.pool
            .reserved
            .set(self.pool.reserved.get() + capacity.next_power_of_two());
        self.pool.objects.set(self.pool.objects.get() + 1);
        let pipe = Rc::new(RefCell::new(Pipe {
            data: VecDeque::new(),
            capacity,
            readers: 1,
            writers: 1,
            pool: self.pool.clone(),
        }));
        self.handles.insert(
            read,
            Handle {
                pipe: pipe.clone(),
                reader: true,
                access: 1,
                flags: inherit as u32,
            },
        );
        self.handles.insert(
            read + 1,
            Handle {
                pipe,
                reader: false,
                access: 2,
                flags: inherit as u32,
            },
        );
        Ok((read, read + 1))
    }
    pub fn flags(&self, handle: u64) -> Option<u32> {
        self.handles.get(&handle).map(|h| h.flags)
    }
    pub fn set_flags(&mut self, handle: u64, mask: u32, flags: u32) -> Result<(), u32> {
        let h = self.handles.get_mut(&handle).ok_or(6u32)?;
        if mask & !3 != 0 {
            return Err(87);
        }
        h.flags = (h.flags & !mask) | (flags & mask);
        Ok(())
    }
    #[allow(dead_code)]
    pub fn duplicate(
        &mut self,
        handle: u64,
        desired: Option<u32>,
        inherit: bool,
    ) -> Result<u64, u32> {
        let source = self.handles.get(&handle).ok_or(6u32)?;
        let access = file_rights::duplicate_rights(source.access, desired)?;
        if self.handles.len() >= 4096 {
            return Err(8);
        }
        let alias = self.pool.allocate(1)?;
        let mut copy = source.clone();
        copy.access = access;
        copy.flags = inherit as u32;
        self.handles.insert(alias, copy);
        Ok(alias)
    }
    pub fn close(&mut self, handle: u64) -> Result<(), u32> {
        if self.flags(handle).ok_or(6u32)? & 2 != 0 {
            return Err(6);
        }
        self.handles.remove(&handle);
        Ok(())
    }
    #[allow(dead_code)]
    pub fn capacity(&self, handle: u64) -> Result<usize, u32> {
        Ok(self
            .handles
            .get(&handle)
            .ok_or(6u32)?
            .pipe
            .borrow()
            .capacity)
    }
    /// None denotes a pending read; the queue remains unchanged.
    pub fn read(&mut self, handle: u64, count: usize) -> Result<Option<Vec<u8>>, u32> {
        let h = self.handles.get(&handle).ok_or(6u32)?;
        if !h.reader || h.access & 1 == 0 {
            return Err(5);
        }
        let mut pipe = h.pipe.borrow_mut();
        if count == 0 {
            return Ok(Some(Vec::new()));
        }
        if pipe.data.is_empty() {
            return if pipe.writers != 0 {
                Ok(None)
            } else {
                Err(109)
            };
        }
        let count = count.min(pipe.data.len());
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(count).map_err(|_| 8u32)?;
        bytes.extend(pipe.data.drain(..count));
        if pipe.data.is_empty() {
            pipe.data = VecDeque::new();
        }
        Ok(Some(bytes))
    }
    pub(crate) fn peek(&self, handle: u64, count: usize) -> Result<(Vec<u8>, usize), u32> {
        let h = self.handles.get(&handle).ok_or(6u32)?;
        if !h.reader || h.access & 1 == 0 {
            return Err(5);
        }
        let pipe = h.pipe.borrow();
        if pipe.data.is_empty() && pipe.writers == 0 {
            return Err(109);
        }
        super::byte_streams::peek(&pipe.data, count)
    }
    /// False denotes backpressure; no bytes have been appended.
    pub fn write(&mut self, handle: u64, bytes: &[u8]) -> Result<bool, u32> {
        let h = self.handles.get(&handle).ok_or(6u32)?;
        if h.reader || h.access & 2 == 0 {
            return Err(5);
        }
        let mut pipe = h.pipe.borrow_mut();
        if pipe.readers == 0 {
            return Err(109);
        }
        if bytes.len() > pipe.capacity - pipe.data.len() {
            return Ok(false);
        }
        let length = pipe.data.len() + bytes.len();
        reserve(&mut pipe.data, length)?;
        pipe.data.extend(bytes);
        Ok(true)
    }
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.handles.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inherited_endpoints_share_data_and_close_independently() {
        let mut parent = Pipes::default();
        let (reader, writer) = parent.create(4096, true).unwrap();
        let mut child = parent.fork_process(|h, flags| h == writer && flags & 1 != 0);
        parent.close(writer).unwrap();
        assert_eq!(child.read(reader, 1), Err(6));
        assert_eq!(parent.read(reader, 1), Ok(None));
        assert_eq!(child.write(writer, &[1, 2, 3]), Ok(true));
        assert_eq!(parent.read(reader, 2), Ok(Some(vec![1, 2])));
        drop(child);
        assert_eq!(parent.read(reader, 2), Ok(Some(vec![3])));
        assert_eq!(parent.read(reader, 1), Err(109));
        parent.close(reader).unwrap();
        assert_eq!(parent.pool.reserved.get(), 0);
        assert_eq!(parent.pool.objects.get(), 0);
    }

    #[test]
    fn shared_pipe_budget_is_released_after_the_last_process_reference() {
        let mut parent = Pipes::default();
        for _ in 0..4 {
            parent.create(16 * 1024 * 1024, true).unwrap();
        }
        let mut child = parent.fork_process(|_, _| true);
        parent.clear();
        assert_eq!(parent.create(4096, false), Err(8));
        assert_eq!(child.create(4096, false), Err(8));
        child.clear();
        assert_eq!(parent.pool.reserved.get(), 0);
        assert!(parent.create(4096, false).is_ok());
        assert!(child.create(4096, false).is_ok());
    }
}
