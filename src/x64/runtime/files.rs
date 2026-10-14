//! Real confined file handles, with sharing, access and endpoint lifetime checks.
mod directory;
mod standard;
use standard::StandardHandles;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, Metadata, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
pub(super) fn filetime(time: std::io::Result<SystemTime>) -> Result<u64, u32> {
    let time = time.map_err(|e| {
        if e.kind() == std::io::ErrorKind::Unsupported {
            50
        } else {
            crate::fs::win_error(&e)
        }
    })?;
    let ticks = match time.duration_since(UNIX_EPOCH) {
        Ok(d) => 116444736000000000i128 + (d.as_nanos() / 100) as i128,
        Err(e) => 116444736000000000i128 - (e.duration().as_nanos() / 100) as i128,
    };
    u64::try_from(ticks).map_err(|_| 534)
}
pub(super) fn attributes(meta: &Metadata, not_indexed: bool) -> u32 {
    let value = (if meta.is_dir() { 0x10 } else { 0 })
        | meta.permissions().readonly() as u32
        | if not_indexed { 0x2000 } else { 0 };
    if value == 0 {
        0x80
    } else {
        value
    }
}
pub(super) fn identity(meta: &Metadata) -> Result<(u64, u64, u32, u64), u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok((
            meta.dev(),
            meta.ino(),
            u32::try_from(meta.nlink()).map_err(|_| 534u32)?,
            meta.blocks().checked_mul(512).ok_or(534u32)?,
        ))
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        Err(50)
    }
}
pub(super) fn change_time(meta: &Metadata) -> Result<u64, u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let ticks = 116444736000000000i128
            + meta.ctime() as i128 * 10000000
            + meta.ctime_nsec() as i128 / 100;
        u64::try_from(ticks).map_err(|_| 534)
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        Err(50)
    }
}
struct OpenFile {
    file: File,
    path: PathBuf,
    access: u32,
    share_access: u32,
    share: u32,
    flags: u32,
    directory: Option<std::rc::Rc<std::cell::RefCell<directory::Cursor>>>,
}
pub(super) enum PathMutation<'a> {
    Move {
        destination: Option<&'a str>,
        flags: u64,
    },
    DeleteFile,
    RemoveDirectory,
}
pub(super) struct Files {
    next: u64,
    handles: BTreeMap<u64, OpenFile>,
    fs: Option<crate::fs::GuestFs>,
    not_indexed: BTreeSet<PathBuf>,
    standard: StandardHandles,
}
impl Default for Files {
    fn default() -> Self {
        Self {
            next: 0x100000,
            handles: BTreeMap::new(),
            fs: None,
            not_indexed: BTreeSet::new(),
            standard: StandardHandles::default(),
        }
    }
}
impl Files {
    pub(super) fn set_filesystem(&mut self, fs: crate::fs::GuestFs) {
        self.fs = Some(fs);
    }
    pub(super) fn filesystem(&self) -> Option<&crate::fs::GuestFs> {
        self.fs.as_ref()
    }
    pub(super) fn filesystem_mut(&mut self) -> Option<&mut crate::fs::GuestFs> {
        self.fs.as_mut()
    }
    #[inline]
    pub(super) fn standard(&self) -> &StandardHandles {
        &self.standard
    }
    #[inline]
    pub(super) fn standard_mut(&mut self) -> &mut StandardHandles {
        &mut self.standard
    }
    pub(super) fn is_not_indexed(&self, path: &std::path::Path) -> bool {
        self.not_indexed.contains(path)
    }
    pub(super) fn set_attributes(&mut self, path: &str, flags: u64) -> Result<(), u32> {
        if flags & !0x2090 != 0 {
            return Err(50);
        }
        let path = self.fs.as_ref().ok_or(3u32)?.resolve(path)?;
        let path = path.canonicalize().map_err(|e| crate::fs::win_error(&e))?;
        // Indexing is absent; preserve the guest metadata without changing type.
        if flags & 0x2000 != 0 {
            self.not_indexed.insert(path);
        } else {
            self.not_indexed.remove(&path);
        }
        Ok(())
    }
    pub(super) fn mutate_path(
        &mut self,
        path: &str,
        mutation: PathMutation<'_>,
    ) -> Result<(), u32> {
        let (moving, destination, flags, remove_directory) = match mutation {
            PathMutation::Move { destination, flags } => (true, destination, flags, false),
            PathMutation::DeleteFile => (false, None, 0, false),
            PathMutation::RemoveDirectory => (false, None, 0, true),
        };
        if flags & !1 != 0 || moving && destination.is_none() {
            return Err(50);
        }
        let fs = self.fs.as_ref().ok_or(3u32)?;
        let source = fs.resolve(path)?;
        let current = fs.resolve(&fs.cwd())?;
        if current.starts_with(&source) {
            return Err(32);
        }
        if self.in_use(&source) {
            return Err(32);
        }
        let canonical_source = source
            .canonicalize()
            .map_err(|e| crate::fs::win_error(&e))?;
        if moving {
            let target = fs.resolve(destination.unwrap())?;
            let meta = std::fs::metadata(&source).map_err(|e| crate::fs::win_error(&e))?;
            if source == target {
                return Ok(());
            }
            if self.in_use(&target) {
                return Err(32);
            }
            if target.exists() {
                if flags & 1 == 0 {
                    return Err(183);
                }
                if target.is_dir() || meta.is_dir() {
                    return Err(5);
                }
                if target
                    .metadata()
                    .map_err(|e| crate::fs::win_error(&e))?
                    .permissions()
                    .readonly()
                {
                    return Err(5);
                }
            }
            if !target.parent().is_some_and(|p| p.is_dir()) {
                return Err(3);
            }
            std::fs::rename(&source, &target).map_err(|e| crate::fs::win_error(&e))?;
            let canonical_target = target
                .canonicalize()
                .map_err(|e| crate::fs::win_error(&e))?;
            let moved: Vec<_> = self
                .not_indexed
                .iter()
                .filter_map(|p| {
                    p.strip_prefix(&canonical_source)
                        .ok()
                        .map(|suffix| (p.clone(), canonical_target.join(suffix)))
                })
                .collect();
            self.not_indexed
                .retain(|p| !p.starts_with(&canonical_target));
            for (old, new) in moved {
                self.not_indexed.remove(&old);
                self.not_indexed.insert(new);
            }
        } else if remove_directory {
            if !source.is_dir() {
                return Err(267);
            }
            std::fs::remove_dir(source).map_err(|e| {
                if e.kind() == std::io::ErrorKind::DirectoryNotEmpty {
                    145
                } else {
                    crate::fs::win_error(&e)
                }
            })?;
            self.not_indexed.remove(&canonical_source);
        } else {
            if source.is_dir() {
                return Err(5);
            }
            if source
                .metadata()
                .map_err(|e| crate::fs::win_error(&e))?
                .permissions()
                .readonly()
            {
                return Err(5);
            }
            std::fs::remove_file(source).map_err(|e| crate::fs::win_error(&e))?;
            self.not_indexed.remove(&canonical_source);
        }
        Ok(())
    }
    pub fn in_use(&self, path: &std::path::Path) -> bool {
        self.handles.values().any(|f| f.path == path)
    }
    pub fn open(
        &mut self,
        path: PathBuf,
        access: u32,
        share: u32,
        disposition: u32,
        attributes: u32,
        inherit: bool,
    ) -> Result<(u64, bool), u32> {
        // Accept expanded FILE_GENERIC_READ/WRITE as well as generic bits.
        // Append-only, execute, delete and security access need separate semantics.
        if access & !0xc012019f != 0
            || access & 4 != 0 && access & (0x40000000 | 2) == 0
            || attributes & !0x1a000080 != 0
        {
            return Err(50);
        }
        if share & !7 != 0 || !(1..=5).contains(&disposition) {
            return Err(87);
        }
        if self.handles.len() >= 1024 || self.next >= 0x7fff0000 {
            return Err(8);
        }
        let rights =
            ((access & 0x80000001 != 0) as u32) | (((access & 0x40000002 != 0) as u32) << 1);
        if self
            .handles
            .values()
            .any(|f| f.path == path && (rights & !f.share != 0 || f.share_access & !share != 0))
        {
            return Err(32);
        }
        if !path.parent().is_some_and(|p| p.is_dir()) {
            return Err(3);
        }
        let directory = path.is_dir();
        if directory && (attributes & 0x02000000 == 0 || disposition != 3 || rights & 2 != 0) {
            return Err(5);
        }
        let existed = path.exists();
        let mut options = OpenOptions::new();
        options
            .read(rights & 1 != 0 || rights == 0)
            .write(rights & 2 != 0);
        match disposition {
            1 => {
                options.create_new(true);
            }
            2 => {
                options.create(true).truncate(true);
            }
            3 => {}
            4 => {
                options.create(true);
            }
            5 => {
                options.truncate(true);
            }
            _ => unreachable!(),
        }
        let file = options.open(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                80
            } else {
                crate::fs::win_error(&e)
            }
        })?;
        let handle = self.next;
        self.next += 1;
        self.handles.insert(
            handle,
            OpenFile {
                file,
                path,
                access: rights,
                share_access: rights,
                share,
                flags: inherit as u32,
                directory: directory.then(Default::default),
            },
        );
        Ok((handle, existed))
    }
    pub fn flags(&self, handle: u64) -> Option<u32> {
        self.handles.get(&handle).map(|f| f.flags)
    }
    pub fn duplicate(
        &mut self,
        handle: u64,
        desired: Option<u32>,
        inherit: bool,
    ) -> Result<u64, u32> {
        if self.handles.len() >= 1024 || self.next >= 0x7fff0000 {
            return Err(8);
        }
        let original = self.handles.get(&handle).ok_or(6u32)?;
        let access = if let Some(a) = desired {
            if a & !0xc012019b != 0 {
                return Err(50);
            }
            let rights = ((a & 0x80000001 != 0) as u32) | (((a & 0x40000002 != 0) as u32) << 1);
            if rights & !original.access != 0 {
                return Err(5);
            }
            rights
        } else {
            original.access
        };
        let file = original
            .file
            .try_clone()
            .map_err(|e| crate::fs::win_error(&e))?;
        let copy = OpenFile {
            file,
            path: original.path.clone(),
            access,
            share_access: original.share_access,
            share: original.share,
            flags: inherit as u32,
            directory: original.directory.clone(),
        };
        let id = self.next;
        self.next += 1;
        self.handles.insert(id, copy);
        Ok(id)
    }
    pub fn set_flags(&mut self, handle: u64, mask: u32, flags: u32) -> Result<(), u32> {
        if mask & !3 != 0 {
            return Err(87);
        }
        let file = self.handles.get_mut(&handle).ok_or(6u32)?;
        file.flags = file.flags & !mask | flags & mask;
        Ok(())
    }
    pub fn close(&mut self, handle: u64) -> Result<(), u32> {
        if self.flags(handle).ok_or(6u32)? & 2 != 0 {
            return Err(6);
        }
        self.handles.remove(&handle);
        Ok(())
    }
    pub fn read(&mut self, handle: u64, bytes: &mut [u8]) -> Result<usize, u32> {
        let file = self.handles.get_mut(&handle).ok_or(6u32)?;
        if file.access & 1 == 0 || file.directory.is_some() {
            return Err(5);
        }
        file.file.read(bytes).map_err(|e| crate::fs::win_error(&e))
    }
    pub fn write(&mut self, handle: u64, bytes: &[u8]) -> Result<usize, u32> {
        let file = self.handles.get_mut(&handle).ok_or(6u32)?;
        if file.access & 2 == 0 || file.directory.is_some() {
            return Err(5);
        }
        file.file.write(bytes).map_err(|e| crate::fs::win_error(&e))
    }
    pub fn size(&self, handle: u64) -> Result<u64, u32> {
        self.handles
            .get(&handle)
            .ok_or(6u32)?
            .file
            .metadata()
            .map(|m| m.len())
            .map_err(|e| crate::fs::win_error(&e))
    }
    pub fn metadata(&self, handle: u64) -> Result<(Metadata, &std::path::Path), u32> {
        let file = self.handles.get(&handle).ok_or(6u32)?;
        Ok((
            file.file.metadata().map_err(|e| crate::fs::win_error(&e))?,
            &file.path,
        ))
    }
    pub fn mapping_file(&self, handle: u64) -> Result<File, u32> {
        let file = self.handles.get(&handle).ok_or(6u32)?;
        if file.access & 1 == 0 || file.directory.is_some() {
            return Err(5);
        }
        file.file.try_clone().map_err(|e| crate::fs::win_error(&e))
    }
    pub fn position(&mut self, handle: u64) -> Result<u64, u32> {
        self.handles
            .get_mut(&handle)
            .ok_or(6u32)?
            .file
            .stream_position()
            .map_err(|e| crate::fs::win_error(&e))
    }
    pub fn seek(&mut self, handle: u64, offset: i64, method: u32) -> Result<u64, u32> {
        let file = self.handles.get_mut(&handle).ok_or(6u32)?;
        let from = match method {
            0 if offset < 0 => return Err(131),
            0 => SeekFrom::Start(offset as u64),
            1 => SeekFrom::Current(offset),
            2 => SeekFrom::End(offset),
            _ => return Err(87),
        };
        file.file.seek(from).map_err(|e| {
            if e.kind() == std::io::ErrorKind::InvalidInput {
                131
            } else {
                crate::fs::win_error(&e)
            }
        })
    }
    pub fn flush(&self, handle: u64) -> Result<u64, u32> {
        self.handles
            .get(&handle)
            .ok_or(6u32)?
            .file
            .sync_all()
            .map(|()| 1)
            .map_err(|e| crate::fs::win_error(&e))
    }
}
