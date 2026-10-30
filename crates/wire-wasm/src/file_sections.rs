//! Bounded read-only sections and views retain file nodes independently of handles.
use crate::filesystem::Contents;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
const PAGE: u64 = 4096;
pub(crate) trait ViewMemory {
    fn file_view_address(&self, address: u64, size: u64) -> Result<u64, u32>;
    fn map_file_view(&mut self, address: u64, bytes: &[u8]) -> Result<u64, u32>;
    fn unmap_file_view(&mut self, base: u64, span: u64) -> Result<(), String>;
    fn update_file_view(&mut self, base: u64, offset: u64, bytes: &[u8]) -> Result<(), String>;
}
pub(crate) struct Section {
    pub(crate) node: Rc<RefCell<Contents>>,
    pub(crate) identity: u64,
    pub(crate) size: u64,
}
pub(crate) struct View {
    pub(crate) _section: Rc<Section>,
    pub(crate) span: u64,
    pub(crate) offset: u64,
    pub(crate) size: u64,
}
pub(crate) struct Mappings {
    pub(crate) next: u64,
    pub(crate) handles: BTreeMap<u64, (Rc<Section>, u32)>,
    pub(crate) views: BTreeMap<u64, View>,
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
    pub(crate) fn is_empty(&self) -> bool {
        self.handles.is_empty() && self.views.is_empty()
    }
    pub fn flags(&self, handle: u64) -> Option<u32> {
        self.handles.get(&handle).map(|entry| entry.1)
    }
    pub fn set_flags(&mut self, handle: u64, mask: u32, flags: u32) -> Result<(), u32> {
        if mask & !3 != 0 {
            return Err(87);
        }
        let entry = self.handles.get_mut(&handle).ok_or(6u32)?;
        entry.1 = entry.1 & !mask | flags & mask;
        Ok(())
    }
    pub fn close(&mut self, handle: u64) -> Result<(), u32> {
        if self.flags(handle).ok_or(6u32)? & 2 != 0 {
            return Err(6);
        }
        self.handles.remove(&handle);
        Ok(())
    }
    fn insert(&mut self, section: Rc<Section>, flags: u32) -> Result<u64, u32> {
        if self.handles.len() >= 1024 || self.next >= 0x190000 {
            return Err(8);
        }
        let handle = self.next;
        self.next += 1;
        self.handles.insert(handle, (section, flags));
        Ok(handle)
    }
    pub(crate) fn create(
        &mut self,
        node: Rc<RefCell<Contents>>,
        identity: u64,
        maximum: u64,
        inherit: bool,
    ) -> Result<u64, u32> {
        let size = if maximum == 0 {
            node.borrow().metadata.size
        } else {
            maximum
        };
        if size == 0 {
            return Err(1006);
        }
        if size > node.borrow().metadata.size {
            return Err(5);
        }
        self.insert(
            Rc::new(Section {
                node,
                identity,
                size,
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
        if desired.is_some_and(|access| access != 4) {
            return Err(50);
        }
        let section = self.handles.get(&handle).ok_or(6u32)?.0.clone();
        self.insert(section, inherit as u32)
    }
    pub(crate) fn map(
        &mut self,
        mem: &mut impl ViewMemory,
        args: &[u64],
        address: u64,
    ) -> Result<u64, u32> {
        if args[1] != 4 {
            return Err(50);
        }
        let offset = args[2] << 32 | args[3];
        if !offset.is_multiple_of(65536) || !address.is_multiple_of(65536) {
            return Err(1132);
        }
        if self.views.len() >= 1024 {
            return Err(8);
        }
        let section = self.handles.get(&args[0]).ok_or(6u32)?.0.clone();
        let remaining = section
            .size
            .checked_sub(offset)
            .filter(|&size| size != 0)
            .ok_or(87u32)?;
        let size = if args[4] == 0 { remaining } else { args[4] };
        if size > remaining {
            return Err(87);
        }
        if size > 256 * 1024 * 1024 {
            return Err(8);
        }
        let address = mem.file_view_address(address, size)?;
        let base = {
            let node = section.node.borrow();
            if node.source.is_some() {
                let bytes = node
                    .read(offset, size as usize)
                    .map_err(crate::filesystem::ReadError::code)?;
                mem.map_file_view(address, &bytes)?
            } else {
                mem.map_file_view(
                    address,
                    &node.data[offset as usize..(offset + size) as usize],
                )?
            }
        };
        self.views.insert(
            base,
            View {
                _section: section,
                span: size.div_ceil(PAGE) * PAGE,
                offset,
                size,
            },
        );
        Ok(base)
    }
    pub(crate) fn unmap(&mut self, mem: &mut impl ViewMemory, base: u64) -> Result<(), String> {
        let span = self.views.get(&base).ok_or("unknown file view")?.span;
        mem.unmap_file_view(base, span)?;
        self.views.remove(&base);
        Ok(())
    }
    pub(crate) fn has_file(&self, node: &Rc<RefCell<Contents>>) -> bool {
        self.handles
            .values()
            .any(|(s, _)| Rc::ptr_eq(&s.node, node))
            || self
                .views
                .values()
                .any(|v| Rc::ptr_eq(&v._section.node, node))
    }
    pub(crate) fn update(
        &self,
        mem: &mut impl ViewMemory,
        identity: u64,
        offset: u64,
        bytes: &[u8],
    ) -> Result<(), String> {
        for (base, view_offset, source) in self.updates(identity, offset, bytes.len()) {
            mem.update_file_view(base, view_offset, &bytes[source])?;
        }
        Ok(())
    }
    pub(crate) fn updates(
        &self,
        identity: u64,
        offset: u64,
        length: usize,
    ) -> impl Iterator<Item = (u64, u64, std::ops::Range<usize>)> + '_ {
        self.views.iter().filter_map(move |(&base, view)| {
            if view._section.identity != identity {
                return None;
            }
            let start = view.offset.max(offset);
            let end = (view.offset + view.size).min(offset + length as u64);
            (start < end).then(|| {
                (
                    base,
                    start - view.offset,
                    (start - offset) as usize..(end - offset) as usize,
                )
            })
        })
    }
    pub(crate) fn regions(&self) -> impl Iterator<Item = (u64, u64)> + '_ {
        self.views.iter().map(|(&base, view)| (base, view.span))
    }
    pub(crate) fn contains_view(&self, base: u64) -> bool {
        self.views.contains_key(&base)
    }
    pub(crate) fn clear(&mut self, memory: &mut impl ViewMemory) -> Result<(), String> {
        while let Some((&base, _)) = self.views.first_key_value() {
            self.unmap(memory, base)?;
        }
        self.handles.clear();
        Ok(())
    }
    pub(crate) fn flush(&self, base: u64, size: u64) -> Result<(), u32> {
        let (&start, view) = self.views.range(..=base).next_back().ok_or(487u32)?;
        let remaining = (start + view.span)
            .checked_sub(base)
            .filter(|&n| n != 0)
            .ok_or(487u32)?;
        if size > remaining {
            return Err(487);
        }
        // Read-only views have no dirty data or host file to flush.
        Ok(())
    }
}
