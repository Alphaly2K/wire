//! Read-only file sections over owned checked guest pages. Retained descriptors
//! outlive file handles; guest WriteFile updates every live view of the same inode.
use super::{files, Memory};
use std::{
    collections::BTreeMap,
    fs::{File, Metadata},
    sync::Arc,
};
struct Section {
    file: File,
    size: u64,
    identity: (u64, u64),
}
struct View {
    section: Arc<Section>,
    offset: u64,
    size: u64,
    span: u64,
}
pub(super) struct Mappings {
    next: u64,
    handles: BTreeMap<u64, (Arc<Section>, u32)>,
    views: BTreeMap<u64, View>,
}
impl Default for Mappings {
    fn default() -> Self {
        Self {
            next: 0x180000,
            handles: BTreeMap::new(),
            views: BTreeMap::new(),
        }
    }
}
impl Mappings {
    pub fn flags(&self, handle: u64) -> Option<u32> {
        self.handles.get(&handle).map(|s| s.1)
    }
    pub fn set_flags(&mut self, handle: u64, mask: u32, flags: u32) -> Result<(), u32> {
        if mask & !3 != 0 {
            return Err(87);
        }
        let section = self.handles.get_mut(&handle).ok_or(6u32)?;
        section.1 = section.1 & !mask | flags & mask;
        Ok(())
    }
    pub fn close(&mut self, handle: u64) -> Result<(), u32> {
        if self.flags(handle).ok_or(6u32)? & 2 != 0 {
            return Err(6);
        }
        self.handles.remove(&handle);
        Ok(())
    }
    fn insert(&mut self, section: Arc<Section>, flags: u32) -> Result<u64, u32> {
        if self.handles.len() >= 1024 || self.next >= 0x190000 {
            return Err(8);
        }
        let handle = self.next;
        self.next += 1;
        self.handles.insert(handle, (section, flags));
        Ok(handle)
    }
    pub fn create(&mut self, file: File, maximum: u64, inherit: bool) -> Result<u64, u32> {
        let meta = file.metadata().map_err(|e| crate::fs::win_error(&e))?;
        if !meta.is_file() {
            return Err(50);
        }
        let size = if maximum == 0 { meta.len() } else { maximum };
        if size == 0 {
            return Err(1006);
        }
        if size > meta.len() {
            return Err(5);
        } // READONLY cannot grow a file.
        let (volume, index, _, _) = files::identity(&meta)?;
        self.insert(
            Arc::new(Section {
                file,
                size,
                identity: (volume, index),
            }),
            inherit as u32,
        )
    }
    pub fn duplicate(
        &mut self,
        handle: u64,
        desired: Option<u32>,
        inherit: bool,
    ) -> Result<u64, u32> {
        if desired.is_some_and(|rights| rights != 4) {
            return Err(50);
        }
        let section = self.handles.get(&handle).ok_or(6u32)?.0.clone();
        self.insert(section, inherit as u32)
    }
    pub fn has_file(&self, meta: &Metadata) -> Result<bool, u32> {
        if self.handles.is_empty() && self.views.is_empty() {
            return Ok(false);
        }
        let (volume, index, _, _) = files::identity(meta)?;
        let matches = |section: &Section| section.identity == (volume, index);
        Ok(self.handles.values().any(|(s, _)| matches(s))
            || self.views.values().any(|v| matches(&v.section)))
    }
    pub fn map(
        &mut self,
        mem: &mut Memory,
        handle: u64,
        access: u32,
        offset: u64,
        requested: u64,
        address: u64,
    ) -> Result<u64, u32> {
        if access != 4 {
            return Err(50);
        } // Shared WRITE, COPY, EXECUTE and pagefile sections need separate backing.
        if !offset.is_multiple_of(65536) || !address.is_multiple_of(65536) {
            return Err(1132);
        }
        if self.views.len() >= 1024 {
            return Err(8);
        }
        let section = self.handles.get(&handle).ok_or(6u32)?.0.clone();
        let remaining = section
            .size
            .checked_sub(offset)
            .filter(|&s| s != 0)
            .ok_or(87u32)?;
        let size = if requested == 0 { remaining } else { requested };
        if size > remaining {
            return Err(87);
        }
        if size > 256 * 1024 * 1024 {
            return Err(8);
        }
        let mut data = vec![0; size as usize];
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileExt;
            let mut read = 0;
            while read < data.len() {
                match section
                    .file
                    .read_at(&mut data[read..], offset + read as u64)
                {
                    Ok(0) => return Err(38),
                    Ok(n) => read += n,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(e) => return Err(crate::fs::win_error(&e)),
                }
            }
        }
        #[cfg(not(unix))]
        {
            return Err(50);
        }
        let base = mem.map_file_view(address, &data)?;
        self.views.insert(
            base,
            View {
                section,
                offset,
                size,
                span: size.div_ceil(super::PAGE) * super::PAGE,
            },
        );
        Ok(base)
    }
    pub fn unmap(&mut self, mem: &mut Memory, base: u64) -> Result<(), String> {
        let span = self.views.get(&base).ok_or("Win64 unknown file view")?.span;
        mem.unmap(base, span)?;
        self.views.remove(&base);
        Ok(())
    }
    pub fn contains_view(&self, base: u64) -> bool {
        self.views.contains_key(&base)
    }
    pub fn flush(&self, base: u64, size: u64) -> Result<(), u32> {
        let (&start, view) = self.views.range(..=base).next_back().ok_or(487u32)?;
        let remaining = (start + view.span)
            .checked_sub(base)
            .filter(|&n| n != 0)
            .ok_or(487u32)?;
        if size > remaining {
            return Err(487);
        }
        // Read-only views have no dirty pages. Retain ordinary descriptor flush errors.
        view.section
            .file
            .sync_all()
            .map_err(|e| crate::fs::win_error(&e))
    }
    pub fn update(
        &self,
        mem: &mut Memory,
        meta: &Metadata,
        offset: u64,
        bytes: &[u8],
    ) -> Result<(), String> {
        if self.views.is_empty() {
            return Ok(());
        }
        let (volume, index, _, _) =
            files::identity(meta).map_err(|e| format!("mapped file identity error {e}"))?;
        for (&base, view) in &self.views {
            if view.section.identity != (volume, index) {
                continue;
            }
            let start = view.offset.max(offset);
            let end = (view.offset + view.size).min(offset + bytes.len() as u64);
            if start < end {
                mem.update_file_view(
                    base,
                    start - view.offset,
                    &bytes[(start - offset) as usize..(end - offset) as usize],
                )?;
            }
        }
        Ok(())
    }
}
