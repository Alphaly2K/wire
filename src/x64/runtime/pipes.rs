//! Anonymous byte streams with real endpoint lifetime and bounded buffering.
use crate::x64::memory::{Memory, READ, WRITE};
use std::collections::{BTreeMap, VecDeque};
struct Pipe {
    data: VecDeque<u8>,
    capacity: usize,
    reader: bool,
    writer: bool,
}
pub(super) struct Pipes {
    next: u64,
    objects: BTreeMap<u64, Pipe>,
    handles: BTreeMap<u64, (u64, bool, u32)>,
}
impl Default for Pipes {
    fn default() -> Self {
        Self {
            next: 0x10000,
            objects: BTreeMap::new(),
            handles: BTreeMap::new(),
        }
    }
}
impl Pipes {
    pub(super) fn create_call(
        &mut self,
        mem: &mut Memory,
        args: &[u64],
        error_slot: u64,
    ) -> Result<u64, String> {
        Ok({
            mem.check(args[0], 8, WRITE)?;
            mem.check(args[1], 8, WRITE)?;
            let inherit = if args[2] == 0 {
                false
            } else {
                mem.check(args[2], 24, READ)?;
                if mem.read_int(args[2], 4)? != 24 || mem.read_int(args[2] + 8, 8)? != 0 {
                    return Err("Win64 pipe security descriptor not implemented".into());
                }
                mem.read_int(args[2] + 16, 4)? != 0
            };
            match self.create(args[3] as u32, inherit) {
                Ok((read, write)) => {
                    mem.write_int(args[0], 8, read)?;
                    mem.write_int(args[1], 8, write)?;
                    1
                }
                Err(error) => {
                    mem.write_int(error_slot, 4, error as u64)?;
                    0
                }
            }
        })
    }
    pub fn create(&mut self, size: u32, inherit: bool) -> Result<(u64, u64), u32> {
        if size > 16 * 1024 * 1024 || self.objects.len() >= 1024 || self.next >= 0xffffe {
            return Err(8);
        }
        let read = self.next;
        let write = read + 1;
        self.next += 2;
        self.objects.insert(
            read,
            Pipe {
                data: VecDeque::new(),
                capacity: (size as usize).max(4096),
                reader: true,
                writer: true,
            },
        );
        self.handles.insert(read, (read, true, inherit as u32));
        self.handles.insert(write, (read, false, inherit as u32));
        Ok((read, write))
    }
    pub fn flags(&self, handle: u64) -> Option<u32> {
        self.handles.get(&handle).map(|h| h.2)
    }
    pub fn set_flags(&mut self, handle: u64, mask: u32, flags: u32) -> Result<(), u32> {
        let h = self.handles.get_mut(&handle).ok_or(6u32)?;
        if mask & !3 != 0 {
            return Err(87);
        }
        h.2 = (h.2 & !mask) | (flags & mask);
        Ok(())
    }
    pub fn close(&mut self, handle: u64) -> Result<(), u32> {
        let &(object, reader, flags) = self.handles.get(&handle).ok_or(6u32)?;
        if flags & 2 != 0 {
            return Err(6);
        }
        self.handles.remove(&handle);
        let pipe = self.objects.get_mut(&object).unwrap();
        if reader {
            pipe.reader = false;
        } else {
            pipe.writer = false;
        }
        if !pipe.reader && !pipe.writer {
            self.objects.remove(&object);
        }
        Ok(())
    }
    /// None denotes a blocking operation; it never means success.
    pub fn read(&mut self, handle: u64, count: usize) -> Result<Option<Vec<u8>>, u32> {
        let &(object, reader, _) = self.handles.get(&handle).ok_or(6u32)?;
        if !reader {
            return Err(5);
        }
        let pipe = self.objects.get_mut(&object).unwrap();
        if count == 0 {
            return Ok(Some(Vec::new()));
        }
        if pipe.data.is_empty() {
            return if pipe.writer { Ok(None) } else { Err(109) };
        }
        let bytes = pipe.data.drain(..count.min(pipe.data.len())).collect();
        Ok(Some(bytes))
    }
    pub fn write(&mut self, handle: u64, bytes: &[u8]) -> Result<bool, u32> {
        let &(object, reader, _) = self.handles.get(&handle).ok_or(6u32)?;
        if reader {
            return Err(5);
        }
        let pipe = self.objects.get_mut(&object).unwrap();
        if !pipe.reader {
            return Err(109);
        }
        if bytes.len() > pipe.capacity - pipe.data.len() {
            return Ok(false);
        }
        pipe.data.extend(bytes);
        Ok(true)
    }
}
