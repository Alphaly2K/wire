//! Resident synchronous file handles. Aliases share a cursor and original
//! sharing reservation. Reads borrow file bytes without a transfer buffer.
pub(crate) const HANDLE_END: u64 = 0x7ffe0000;
use crate::filesystem::{Contents, FileMetadata, Files, Reservation, Sharing};
use crate::win32::{
    file_cursor::{Budget, Cursor},
    file_rights,
};
use std::{
    cell::{Cell, Ref, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

#[derive(Clone)]
struct Handle {
    reservation: Rc<Reservation>,
    path: String,
    node: Rc<RefCell<Contents>>,
    directory: Option<Rc<RefCell<Cursor>>>,
    cursor: Rc<Cell<u64>>,
    access: u32,
    share_access: u32,
    share: u32,
    flags: u32,
    identity: u64,
    write_attributes: bool,
    auto_write_time: Rc<Cell<bool>>,
}
pub(crate) enum Bytes<'a> {
    Borrowed(Ref<'a, [u8]>),
    Owned(Vec<u8>),
}
impl std::ops::Deref for Bytes<'_> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Self::Borrowed(bytes) => bytes,
            Self::Owned(bytes) => bytes,
        }
    }
}

pub(super) struct Handles {
    sharing: Option<Rc<Sharing>>,
    next: u64,
    directory_budget: Rc<Budget>,
    entries: BTreeMap<u64, Handle>,
}
impl Default for Handles {
    fn default() -> Self {
        Self {
            sharing: None,
            next: 0x100000,
            directory_budget: Budget::new(64 * 1024 * 1024),
            entries: BTreeMap::new(),
        }
    }
}
impl Handles {
    #[allow(dead_code)]
    pub(crate) fn fork_process(&self, select: impl Fn(u64, u32) -> bool) -> Self {
        Self {
            sharing: self.sharing.clone(),
            next: self.next,
            directory_budget: self.directory_budget.clone(),
            entries: self
                .entries
                .iter()
                .filter(|(h, v)| select(**h, v.flags))
                .map(|(&h, v)| (h, v.clone()))
                .collect(),
        }
    }
    pub(crate) fn for_files(files: &Files) -> Self {
        Self {
            sharing: Some(files.sharing()),
            ..Self::default()
        }
    }

    pub fn check_share(&self, path: &str, rights: u32, share: u32) -> Result<(), u32> {
        if let Some(sharing) = &self.sharing {
            return sharing.check(path, rights, share);
        }
        let key = path.to_lowercase();
        if self.entries.values().any(|file| {
            file.path == key && (rights & !file.share != 0 || file.share_access & !share != 0)
        }) {
            Err(32)
        } else {
            Ok(())
        }
    }
    #[cfg(test)]
    pub(super) fn directory_budget_for_test(&mut self, bytes: usize) {
        self.directory_budget = Budget::new(bytes);
    }
    #[cfg(test)]
    pub(super) fn directory_bytes_for_test(&self) -> usize {
        self.directory_budget.used()
    }

    fn allocate_handle(&mut self) -> u64 {
        file_rights::allocate_handle(&mut self.next)
    }
    pub fn open(
        &mut self,
        files: &mut Files,
        path: &str,
        values: [u32; 4],
        inherit: bool,
    ) -> Result<(u64, bool), u32> {
        let path = files.resolve(path)?;
        let key = path.to_lowercase();
        let [_access, share, disposition, attributes] = values;
        let rights = file_rights::open_rights(values)?;
        if self.entries.len() >= 1024 || self.next >= HANDLE_END {
            return Err(8);
        }
        let sharing = files.sharing();
        let reservation = sharing.reserve(&path, rights, share)?;
        let parent = key.rsplit_once('\\').ok_or(3u32)?.0;
        let parent = if parent == "c:" { "c:\\" } else { parent };
        if !files
            .metadata(parent)
            .is_ok_and(|m| m.attributes & 0x10 != 0)
        {
            return Err(3);
        }
        let existing = match files.metadata(&path) {
            Ok(m) => Some(m),
            Err(2) => None,
            Err(e) => return Err(e),
        };
        let directory = existing.is_some_and(|m| m.attributes & 0x10 != 0);
        if directory && (attributes & 0x02000000 == 0 || disposition != 3 || rights & 2 != 0) {
            return Err(5);
        }
        if matches!(disposition, 2 | 5) && rights & 2 == 0 {
            return Err(87);
        }
        if rights & 2 != 0 && existing.is_some_and(|m| m.attributes & 1 != 0) {
            return Err(5);
        }
        match disposition {
            1 if existing.is_some() => return Err(80),
            3 | 5 if existing.is_none() => return Err(2),
            _ => {}
        }
        // Check identity capacity before creating/truncating a namespace entry.
        files.check_identity(&path)?;
        let existed = existing.is_some();
        if !existed {
            files.create_file(&path)?;
        }
        if matches!(disposition, 2 | 5) {
            files.resize(&path, 0)?;
        }
        let opened = files.open_data(&path)?;
        let metadata = opened.metadata;
        let node = opened.node.unwrap_or_else(|| {
            Rc::new(RefCell::new(Contents {
                source: None,
                data: Rc::new(Vec::new()),
                metadata,
            }))
        });
        let identity = files.identity(&path).expect("preflighted file identity");
        let handle = self.allocate_handle();
        self.entries.insert(
            handle,
            Handle {
                reservation,
                path: opened.path,
                node,
                directory: directory.then(|| {
                    Rc::new(RefCell::new(Cursor::with_budget(
                        self.directory_budget.clone(),
                    )))
                }),
                cursor: Rc::new(Cell::new(0)),
                access: rights,
                share_access: rights,
                share,
                flags: inherit as u32,
                identity,
                write_attributes: values[0] & 0x40000100 != 0,
                auto_write_time: Rc::new(Cell::new(true)),
            },
        );
        self.sharing = Some(sharing);
        Ok((handle, existed))
    }
    pub fn flags(&self, handle: u64) -> Option<u32> {
        self.entries.get(&handle).map(|file| file.flags)
    }
    pub fn set_flags(&mut self, handle: u64, mask: u32, flags: u32) -> Result<(), u32> {
        if mask & !3 != 0 {
            return Err(87);
        }
        let file = self.entries.get_mut(&handle).ok_or(6u32)?;
        file.flags = file.flags & !mask | flags & mask;
        Ok(())
    }
    pub fn close(&mut self, handle: u64) -> Result<(), u32> {
        if self.flags(handle).ok_or(6u32)? & 2 != 0 {
            return Err(6);
        }
        self.entries.remove(&handle);
        Ok(())
    }
    pub fn duplicate(
        &mut self,
        handle: u64,
        desired: Option<u32>,
        inherit: bool,
    ) -> Result<u64, u32> {
        if self.entries.len() >= 1024 || self.next >= HANDLE_END {
            return Err(8);
        }
        let file = self.entries.get(&handle).ok_or(6u32)?;
        let access = file_rights::duplicate_rights(file.access, desired)?;
        let write_attributes = desired.map_or(file.write_attributes, |mask| mask & 0x40000100 != 0);
        if write_attributes && !file.write_attributes {
            return Err(5);
        }
        let copy = Handle {
            reservation: file.reservation.clone(),
            path: file.path.clone(),
            node: file.node.clone(),
            directory: file.directory.clone(),
            cursor: file.cursor.clone(),
            access,
            share_access: file.share_access,
            share: file.share,
            flags: inherit as u32,
            identity: file.identity,
            write_attributes,
            auto_write_time: file.auto_write_time.clone(),
        };
        let handle = self.allocate_handle();
        self.entries.insert(handle, copy);
        Ok(handle)
    }
    pub fn metadata(&self, handle: u64) -> Result<FileMetadata, u32> {
        self.entries
            .get(&handle)
            .map(|file| file.node.borrow().metadata)
            .ok_or(6)
    }
    pub fn write_attributes(&self, handle: u64) -> Result<(), u32> {
        if !self.entries.get(&handle).ok_or(6u32)?.write_attributes {
            return Err(5);
        }
        Ok(())
    }
    pub fn auto_write_time(&self, handle: u64) -> bool {
        self.entries
            .get(&handle)
            .is_some_and(|file| file.auto_write_time.get())
    }
    pub fn set_times(
        &self,
        files: &mut Files,
        handle: u64,
        times: [Option<u64>; 3],
    ) -> Result<(), u32> {
        self.write_attributes(handle)?;
        let file = &self.entries[&handle];
        let mut metadata = if file.directory.is_some() {
            files.metadata(&file.path)?
        } else {
            file.node.borrow().metadata
        };
        for (slot, time) in metadata.times.iter_mut().zip(times) {
            if let Some(time) = time.filter(|&time| time != u64::MAX) {
                *slot = time;
            }
        }
        if file.directory.is_some() && times.iter().any(|time| time.is_some_and(|n| n != u64::MAX))
        {
            files.set_metadata(&file.path, metadata).map_err(|_| 8u32)?;
        }
        file.node.borrow_mut().metadata = metadata;
        if times[2] == Some(u64::MAX) {
            file.auto_write_time.set(false);
        }
        Ok(())
    }
    pub fn identity(&self, handle: u64) -> Result<u64, u32> {
        self.entries.get(&handle).map(|file| file.identity).ok_or(6)
    }
    pub fn mapping_data(&self, handle: u64) -> Result<Rc<RefCell<Contents>>, u32> {
        let file = self.entries.get(&handle).ok_or(6u32)?;
        if file.access & 1 == 0 || file.directory.is_some() {
            return Err(5);
        }
        Ok(file.node.clone())
    }
    pub fn path(&self, handle: u64) -> Result<&str, u32> {
        self.entries.get(&handle).map(|f| f.path.as_str()).ok_or(6)
    }
    pub fn in_use(&self, path: &str) -> bool {
        if let Some(sharing) = &self.sharing {
            return sharing.in_use(path);
        }
        self.entries.values().any(|f| f.path == path)
    }
    pub fn directory(&self, handle: u64) -> Result<Rc<RefCell<Cursor>>, u32> {
        let file = self.entries.get(&handle).ok_or(6u32)?;
        let cursor = file.directory.as_ref().ok_or(267u32)?.clone();
        if file.access & 1 == 0 {
            return Err(5);
        }
        Ok(cursor)
    }
    pub fn position(&self, handle: u64) -> Result<u64, u32> {
        self.entries.get(&handle).map(|f| f.cursor.get()).ok_or(6)
    }
    pub fn writable(&self, handle: u64) -> Result<(&str, u64), u32> {
        let file = self.entries.get(&handle).ok_or(6u32)?;
        if file.access & 2 == 0 || file.directory.is_some() {
            return Err(5);
        }
        Ok((&file.path, file.cursor.get()))
    }
    pub fn advance(&self, handle: u64, count: usize) {
        let file = &self.entries[&handle];
        file.cursor.set(file.cursor.get() + count as u64);
    }
    pub fn read(&self, handle: u64, count: usize) -> Result<Ref<'_, [u8]>, u32> {
        let file = self.entries.get(&handle).ok_or(6u32)?;
        if file.access & 1 == 0 || file.directory.is_some() {
            return Err(5);
        }
        let data = file.node.borrow();
        let position = file.cursor.get();
        let start = usize::try_from(position)
            .unwrap_or(usize::MAX)
            .min(data.data.len());
        let count = count.min(data.data.len() - start);
        file.cursor.set(position + count as u64);
        Ok(Ref::map(data, |node| &node.data[start..start + count]))
    }
    pub(crate) fn read_backed(&self, handle: u64, count: usize) -> Result<Bytes<'_>, u32> {
        let file = self.entries.get(&handle).ok_or(6u32)?;
        if file.access & 1 == 0 || file.directory.is_some() {
            return Err(5);
        }
        if file.node.borrow().source.is_none() {
            return self.read(handle, count).map(Bytes::Borrowed);
        }
        let bytes = file
            .node
            .borrow()
            .read(file.cursor.get(), count)
            .map_err(crate::filesystem::ReadError::code)?;
        file.cursor.set(file.cursor.get() + bytes.len() as u64);
        Ok(Bytes::Owned(bytes))
    }
    pub fn seek(&self, handle: u64, offset: i64, method: u32) -> Result<u64, u32> {
        let position = self.seek_position(handle, offset, method)?;
        self.entries[&handle].cursor.set(position);
        Ok(position)
    }
    pub fn seek_position(&self, handle: u64, offset: i64, method: u32) -> Result<u64, u32> {
        let file = self.entries.get(&handle).ok_or(6u32)?;
        let base = match method {
            0 => 0,
            1 => file.cursor.get(),
            2 => file.node.borrow().metadata.size,
            _ => return Err(87),
        };
        let position = base as i128 + offset as i128;
        if !(0..=i64::MAX as i128).contains(&position) {
            return Err(131);
        }
        Ok(position as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn process_file_reservations_survive_aliases_and_release_on_last_close() {
        let mut files = Files::default();
        files.insert("C:\\a", vec![1, 2, 3]).unwrap();
        let mut child = files.fork_process();
        let mut parent_handles = Handles::for_files(&files);
        let mut child_handles = Handles::for_files(&child);
        let (handle, _) = parent_handles
            .open(&mut files, "C:\\a", [0x80000000, 1, 3, 0x80], false)
            .unwrap();
        let alias = parent_handles.duplicate(handle, None, false).unwrap();
        parent_handles.close(handle).unwrap();
        assert_eq!(child_handles.check_share("c:\\A", 2, 7), Err(32));
        assert_eq!(
            child_handles.open(&mut child, "C:\\a", [0x40000000, 7, 2, 0x80], false),
            Err(32)
        );
        assert_eq!(files.metadata("C:\\a").unwrap().size, 3);
        let snapshot = files.clone();
        assert_eq!(
            Handles::for_files(&snapshot).check_share("C:\\a", 2, 7),
            Ok(())
        );
        parent_handles.close(alias).unwrap();
        assert!(!child_handles.in_use("C:\\a"));
        assert!(child_handles
            .open(&mut child, "C:\\a", [0x40000000, 7, 2, 0x80], false)
            .is_ok());
        assert_eq!(files.metadata("C:\\a").unwrap().size, 0);
    }

    #[test]
    fn exhausted_file_namespace_stops_before_snapshot_handles() {
        let mut files = Files::default();
        files.insert("C:\\a", vec![1]).unwrap();
        let mut handles = Handles {
            next: HANDLE_END - 1,
            ..Default::default()
        };
        let (last, _) = handles
            .open(&mut files, "C:\\a", [0x80000000, 7, 3, 0x80], false)
            .unwrap();
        assert_eq!(last, HANDLE_END - 1);
        assert_eq!(handles.duplicate(last, None, false), Err(8));
        assert_eq!(
            handles.open(&mut files, "C:\\new", [0x40000000, 7, 1, 0x80], false),
            Err(8)
        );
        assert_eq!(files.metadata("C:\\new"), Err(2));
        assert_eq!(
            handles.mapping_data(last).unwrap().borrow().metadata.size,
            1
        );
    }
    #[test]
    fn long_lived_file_tokens_skip_the_entire_section_namespace() {
        let mut files = Files::default();
        files.insert("C:\\a", vec![1]).unwrap();
        let mut handles = Handles {
            next: 0x17ffff,
            ..Default::default()
        };
        let handle = handles
            .open(&mut files, "C:\\a", [0x80000000, 7, 3, 0x80], false)
            .unwrap();
        let handle = handle.0;
        assert_eq!(handle, 0x17ffff);
        let alias = handles.duplicate(handle, None, false).unwrap();
        assert_eq!(alias, 0x190000);
        assert!(Rc::ptr_eq(
            &handles.mapping_data(handle).unwrap(),
            &handles.mapping_data(alias).unwrap()
        ));
    }
}
