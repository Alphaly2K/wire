//! Toolhelp snapshots expose the isolated guest process, never host applications.
use super::Memory;
use crate::win32::catalog::SnapshotApi;
use std::collections::BTreeMap;

pub(super) struct Snapshots {
    handles: BTreeMap<u64, Snapshot>,
    next: u64,
}
impl Default for Snapshots {
    fn default() -> Self {
        Self {
            handles: BTreeMap::new(),
            next: 0x7fff1000,
        }
    }
}

impl Snapshots {
    pub(super) fn create(
        &mut self,
        flags: u64,
        executable: &str,
        threads: impl Iterator<Item = (u32, i32)>,
    ) -> Result<u64, u32> {
        if flags & 6 == 0 || flags & !0x80000006 != 0 {
            return Err(50);
        }
        if self.handles.len() >= 128 || self.next >= 0x7fff2000 {
            return Err(8);
        }
        let handle = self.next;
        self.handles.insert(
            handle,
            Snapshot {
                process: flags & 2 != 0,
                enumerate_threads: flags & 4 != 0,
                threads: threads.collect(),
                name: executable
                    .rsplit('\\')
                    .next()
                    .unwrap_or(executable)
                    .to_owned(),
                process_done: false,
                thread_at: 0,
                flags: (flags >> 31) as u32,
            },
        );
        self.next += 1;
        Ok(handle)
    }

    pub(super) fn enumerate(
        &mut self,
        mem: &mut Memory,
        id: SnapshotApi,
        args: &[u64],
        last_error: u64,
    ) -> Result<u64, String> {
        let (first, thread, wide) = match id {
            SnapshotApi::ProcessFirstA => (true, false, false),
            SnapshotApi::ProcessNextA => (false, false, false),
            SnapshotApi::ProcessFirstW => (true, false, true),
            SnapshotApi::ProcessNextW => (false, false, true),
            SnapshotApi::ThreadFirst => (true, true, false),
            SnapshotApi::ThreadNext => (false, true, false),
            SnapshotApi::Create => unreachable!("creation does not enumerate a snapshot"),
        };
        let size = if thread {
            28
        } else if wide {
            568
        } else {
            304
        };
        if let Some(snapshot) = self.handles.get(&args[0]) {
            if mem.read_int(args[1], 4)? != size {
                return error(mem, last_error, 24);
            }
            let index = if first { 0 } else { snapshot.thread_at };
            let data = if thread {
                snapshot.thread_entry(index).ok_or(18)
            } else if !snapshot.process || !first && snapshot.process_done {
                Err(18)
            } else {
                snapshot.process_entry(wide)
            };
            let data = match data {
                Ok(data) => data,
                Err(code) => return error(mem, last_error, code),
            };
            // Copyout must finish before First can reset or Next can advance.
            mem.write(args[1], &data)?;
            let snapshot = self.handles.get_mut(&args[0]).unwrap();
            if thread {
                snapshot.thread_at = index + 1;
            } else {
                snapshot.process_done = true;
            }
            Ok(1)
        } else {
            error(mem, last_error, 6)
        }
    }

    pub(super) fn contains(&self, handle: u64) -> bool {
        self.handles.contains_key(&handle)
    }

    pub(super) fn flags(&self, handle: u64) -> Option<u32> {
        self.handles.get(&handle).map(|snapshot| snapshot.flags)
    }

    pub(super) fn set_flags(
        &mut self,
        handle: u64,
        mask: u64,
        value: u64,
    ) -> Option<Result<(), u32>> {
        let snapshot = self.handles.get_mut(&handle)?;
        if mask & !3 != 0 {
            return Some(Err(87));
        }
        snapshot.flags = snapshot.flags & !(mask as u32) | value as u32 & mask as u32;
        Some(Ok(()))
    }

    pub(super) fn close(&mut self, handle: u64) -> Result<(), u32> {
        if self.flags(handle).ok_or(6u32)? & 2 != 0 {
            return Err(5);
        }
        self.handles.remove(&handle);
        Ok(())
    }
}

fn error(mem: &mut Memory, last_error: u64, code: u32) -> Result<u64, String> {
    mem.write_int(last_error, 4, code as u64)?;
    Ok(0)
}

struct Snapshot {
    process: bool,
    enumerate_threads: bool,
    threads: Vec<(u32, i32)>,
    name: String,
    process_done: bool,
    thread_at: usize,
    flags: u32,
}
impl Snapshot {
    fn process_entry(&self, wide: bool) -> Result<Vec<u8>, u32> {
        let mut data = vec![0; if wide { 568 } else { 304 }];
        let size = data.len() as u32;
        data[..4].copy_from_slice(&size.to_le_bytes());
        data[8..12].copy_from_slice(&1u32.to_le_bytes());
        data[28..32].copy_from_slice(&(self.threads.len() as u32).to_le_bytes());
        data[36..40].copy_from_slice(&8i32.to_le_bytes());
        if wide {
            let name: Vec<_> = self.name.encode_utf16().collect();
            if name.len() >= 260 {
                return Err(206);
            }
            for (n, u) in name.iter().enumerate() {
                data[44 + n * 2..46 + n * 2].copy_from_slice(&u.to_le_bytes());
            }
        } else {
            let name = crate::codepage::encode(&self.name, 932)?.0;
            if name.len() >= 260 {
                return Err(206);
            }
            data[44..44 + name.len()].copy_from_slice(&name);
        }
        Ok(data)
    }
    fn thread_entry(&self, index: usize) -> Option<Vec<u8>> {
        if !self.enumerate_threads {
            return None;
        }
        let &(id, priority) = self.threads.get(index)?;
        let base = match priority {
            -15 => 1,
            15 => 15,
            _ => 8 + priority,
        };
        Some(
            [28u32, 0, id, 1, base as u32, 0, 0]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect(),
        )
    }
}
