//! Captured guest identities and enumeration cursors, independent of guest memory.
use super::catalog::SnapshotApi;
use std::collections::BTreeMap;

pub(crate) struct Snapshots {
    handles: BTreeMap<u64, Snapshot>,
    next: u64,
    end: u64,
    pid: u32,
}
impl Default for Snapshots {
    fn default() -> Self {
        Self::new(0x7fff1000, 1)
    }
}
impl Snapshots {
    #[allow(dead_code)]
    pub(crate) fn selects_handles(&self, select: impl Fn(u64, u32) -> bool) -> bool {
        self.handles.iter().any(|(&h, v)| select(h, v.flags))
    }
    pub(crate) fn new(start: u64, pid: u32) -> Self {
        Self {
            handles: BTreeMap::new(),
            next: start,
            end: start + 4096,
            pid,
        }
    }
    pub(crate) fn create(
        &mut self,
        flags: u64,
        executable: &str,
        threads: impl Iterator<Item = (u32, i32)>,
    ) -> Result<u64, u32> {
        if flags & 6 == 0 || flags & !0x80000006 != 0 {
            return Err(50);
        }
        if self.handles.len() >= 128 || self.next >= self.end {
            return Err(8);
        }
        let handle = self.next;
        self.handles.insert(
            handle,
            Snapshot {
                process: flags & 2 != 0,
                enumerate_threads: flags & 4 != 0,
                threads: threads.collect(),
                pid: self.pid,
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
    pub(crate) fn kind(id: SnapshotApi) -> (bool, bool, bool) {
        match id {
            SnapshotApi::ProcessFirstA => (true, false, false),
            SnapshotApi::ProcessNextA => (false, false, false),
            SnapshotApi::ProcessFirstW => (true, false, true),
            SnapshotApi::ProcessNextW => (false, false, true),
            SnapshotApi::ThreadFirst => (true, true, false),
            SnapshotApi::ThreadNext => (false, true, false),
            SnapshotApi::Create => unreachable!("creation does not enumerate a snapshot"),
        }
    }
    pub(crate) fn size(id: SnapshotApi, width: usize) -> usize {
        let (_, thread, wide) = Self::kind(id);
        if thread {
            28
        } else {
            match (width, wide) {
                (4, false) => 296,
                (4, true) => 556,
                (8, false) => 304,
                (8, true) => 568,
                _ => unreachable!(),
            }
        }
    }
    pub(crate) fn entry(&self, handle: u64, id: SnapshotApi, width: usize) -> Result<Vec<u8>, u32> {
        let snapshot = self.handles.get(&handle).ok_or(6u32)?;
        let (first, thread, wide) = Self::kind(id);
        if thread {
            snapshot
                .thread_entry(if first { 0 } else { snapshot.thread_at })
                .ok_or(18)
        } else if !snapshot.process || !first && snapshot.process_done {
            Err(18)
        } else {
            snapshot.process_entry(wide, width, Self::size(id, width))
        }
    }
    pub(crate) fn advance(&mut self, handle: u64, id: SnapshotApi) {
        let snapshot = self.handles.get_mut(&handle).unwrap();
        let (first, thread, _) = Self::kind(id);
        if thread {
            snapshot.thread_at = if first { 1 } else { snapshot.thread_at + 1 };
        } else {
            snapshot.process_done = true;
        }
    }
    pub(crate) fn contains(&self, handle: u64) -> bool {
        self.handles.contains_key(&handle)
    }
    pub(crate) fn flags(&self, handle: u64) -> Option<u32> {
        self.handles.get(&handle).map(|s| s.flags)
    }
    pub(crate) fn set_flags(
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
    pub(crate) fn close(&mut self, handle: u64) -> Result<(), u32> {
        if self.flags(handle).ok_or(6u32)? & 2 != 0 {
            return Err(5);
        }
        self.handles.remove(&handle);
        Ok(())
    }
    pub(crate) fn clear(&mut self) {
        self.handles.clear();
    }
}
struct Snapshot {
    process: bool,
    enumerate_threads: bool,
    threads: Vec<(u32, i32)>,
    pid: u32,
    name: String,
    process_done: bool,
    thread_at: usize,
    flags: u32,
}
impl Snapshot {
    fn process_entry(&self, wide: bool, width: usize, size: usize) -> Result<Vec<u8>, u32> {
        let mut data = vec![0; size];
        data[..4].copy_from_slice(&(size as u32).to_le_bytes());
        data[8..12].copy_from_slice(&self.pid.to_le_bytes());
        let count = if width == 4 { 20 } else { 28 };
        let name_at = count + 16;
        data[count..count + 4].copy_from_slice(&(self.threads.len() as u32).to_le_bytes());
        data[count + 8..count + 12].copy_from_slice(&8i32.to_le_bytes());
        let name = if wide {
            self.name
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        } else {
            crate::codepage::encode(&self.name, 932)?.0
        };
        if name.len() >= 260 * if wide { 2 } else { 1 } {
            return Err(206);
        }
        data[name_at..name_at + name.len()].copy_from_slice(&name);
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
            [28u32, 0, id, self.pid, base as u32, 0, 0]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect(),
        )
    }
}
