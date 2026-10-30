//! Search ownership and fixed find-data layouts, independent of guest execution.
use super::file_information::{filetime, Metadata};
use std::collections::BTreeMap;
pub(crate) struct Search<P> {
    pub(crate) entries: Vec<P>,
    pub(crate) next: usize,
    pub(crate) charge: usize,
}
pub(crate) struct Searches<P> {
    pub(crate) handles: BTreeMap<u64, Search<P>>,
    pub(crate) next: u64,
    pub(crate) end: u64,
    pub(crate) budget: Option<usize>,
    pub(crate) used: usize,
}
impl<P> Default for Searches<P> {
    fn default() -> Self {
        Self {
            handles: BTreeMap::new(),
            next: 0x7ffd00670000,
            end: 0x7ffd00680000,
            budget: None,
            used: 0,
        }
    }
}
pub(crate) fn find_data(meta: Metadata, filename: &str, wide: bool) -> Result<Vec<u8>, u32> {
    let name = if wide {
        filename
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    } else {
        crate::codepage::encode(filename, 932)?.0
    };
    if name.len() >= if wide { 520 } else { 260 } {
        return Err(206);
    }
    let mut bytes = vec![0; if wide { 592 } else { 320 }];
    bytes[..4].copy_from_slice(&meta.attributes.to_le_bytes());
    for (offset, time) in [
        (4, meta.created()),
        (12, meta.accessed()),
        (20, meta.modified()),
    ] {
        let ticks = match filetime(time) {
            Ok(ticks) => ticks,
            Err(50) => 0,
            Err(error) => return Err(error),
        };
        bytes[offset..offset + 8].copy_from_slice(&ticks.to_le_bytes());
    }
    let size = if meta.is_dir() { 0 } else { meta.len() };
    bytes[28..32].copy_from_slice(&((size >> 32) as u32).to_le_bytes());
    bytes[32..36].copy_from_slice(&(size as u32).to_le_bytes());
    bytes[44..44 + name.len()].copy_from_slice(&name);
    // The confined filesystem has no DOS 8.3 aliases or reparse-point service.
    Ok(bytes)
}
impl<P: Clone> Searches<P> {
    #[allow(dead_code)] // Native storage retains its existing unbounded profile.
    pub(crate) fn with_budget(limit: usize) -> Self {
        Self {
            budget: Some(limit),
            ..Default::default()
        }
    }
    #[cfg(test)]
    pub(crate) fn used(&self) -> usize {
        self.used
    }

    #[allow(dead_code)] // The independent Wasm owner uses a separate token range.
    pub(crate) fn with_range(first: u64, end: u64, limit: usize) -> Self {
        Self {
            next: first,
            end,
            budget: Some(limit),
            ..Default::default()
        }
    }
    #[allow(dead_code)] // Process rejection releases the independent Wasm owner.
    pub(crate) fn clear(&mut self) {
        self.handles.clear();
        self.used = 0;
    }
}
