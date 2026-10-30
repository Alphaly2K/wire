//! Process-owned query handles and ABI-specific virtual identity records.
use crate::win32::sid_values as sid;
use std::collections::BTreeMap;

pub(crate) const QUERY: u32 = 8;
struct Handle {
    access: u32,
    flags: u32,
    level: Option<u32>,
}
pub(crate) struct Tokens {
    handles: BTreeMap<u64, Handle>,
    next: u64,
    end: u64,
}
impl Tokens {
    #[allow(dead_code)]
    pub(crate) fn selects_handles(&self, select: impl Fn(u64, u32) -> bool) -> bool {
        self.handles.iter().any(|(&h, v)| select(h, v.flags))
    }
    pub(crate) fn new(start: u64, end: u64) -> Self {
        Self {
            handles: BTreeMap::new(),
            next: start,
            end,
        }
    }
    pub(crate) fn prepare(&self, access: u32) -> Result<u64, u32> {
        if access & !14 != 0 {
            return Err(50);
        }
        if self.handles.len() >= 128 || self.next >= self.end {
            return Err(8);
        }
        Ok(self.next)
    }
    pub(crate) fn commit(&mut self, handle: u64, access: u32, inherit: bool) {
        self.commit_level(handle, access, inherit, None);
    }
    pub(crate) fn commit_level(
        &mut self,
        handle: u64,
        access: u32,
        inherit: bool,
        level: Option<u32>,
    ) {
        assert_eq!(handle, self.next);
        self.handles.insert(
            handle,
            Handle {
                access,
                flags: u32::from(inherit),
                level,
            },
        );
        self.next += 1;
    }
    pub(crate) fn query(&self, handle: u64) -> Result<(), u32> {
        if self.handles.get(&handle).ok_or(6u32)?.access & QUERY == 0 {
            return Err(5);
        }
        Ok(())
    }
    pub(crate) fn level(&self, handle: u64) -> Result<Option<u32>, u32> {
        self.handles.get(&handle).map(|h| h.level).ok_or(6)
    }
    pub(crate) fn check(&self, handle: u64, access: u32) -> Result<(), u32> {
        if self.handles.get(&handle).ok_or(6u32)?.access & access == access {
            Ok(())
        } else {
            Err(5)
        }
    }
    pub(crate) fn duplicate(
        &mut self,
        handle: u64,
        desired: Option<u32>,
        inherit: bool,
    ) -> Result<u64, u32> {
        let access = self.handles.get(&handle).ok_or(6u32)?.access;
        let desired = desired.unwrap_or(access);
        if desired & !access != 0 {
            return Err(5);
        }
        let alias = self.prepare(desired)?;
        self.commit_level(alias, desired, inherit, self.level(handle)?);
        Ok(alias)
    }
    pub(crate) fn flags(&self, handle: u64) -> Option<u32> {
        self.handles.get(&handle).map(|h| h.flags)
    }
    pub(crate) fn set_flags(
        &mut self,
        handle: u64,
        mask: u64,
        value: u64,
    ) -> Option<Result<(), u32>> {
        let token = self.handles.get_mut(&handle)?;
        if mask & !3 != 0 {
            return Some(Err(87));
        }
        token.flags = token.flags & !(mask as u32) | value as u32 & mask as u32;
        Some(Ok(()))
    }
    pub(crate) fn close(&mut self, handle: u64) -> Result<(), u32> {
        if self.flags(handle).ok_or(6u32)? & 2 != 0 {
            return Err(6);
        }
        self.handles.remove(&handle);
        Ok(())
    }
    pub(crate) fn clear(&mut self) {
        self.handles.clear();
    }
}

pub(crate) fn information(
    class: u32,
    output: u64,
    width: usize,
    level: Option<u32>,
) -> Result<Vec<u8>, u32> {
    let (authority, parts, attributes): ([u8; 6], &[u32], u32) = match class {
        18 => return Ok(1u32.to_le_bytes().to_vec()),
        8 => {
            return Ok(if level.is_some() { 2u32 } else { 1 }
                .to_le_bytes()
                .to_vec())
        }
        9 => return level.map(|v| v.to_le_bytes().to_vec()).ok_or(87),
        20 | 29 => return Ok(0u32.to_le_bytes().to_vec()),
        1 => ([0, 0, 0, 0, 0, 5], &[21, 1, 1, 1, 1000], 0),
        25 => ([0, 0, 0, 0, 0, 16], &[8192], 0x60),
        _ => return Err(50),
    };
    let offset = width * 2;
    let pointer = output.checked_add(offset as u64).ok_or(87u32)?;
    if width == 4 && pointer > u64::from(u32::MAX) {
        return Err(87);
    }
    let mut bytes = vec![0; offset + sid::length(parts.len() as u8)];
    bytes[..width].copy_from_slice(&pointer.to_le_bytes()[..width]);
    bytes[width..width + 4].copy_from_slice(&attributes.to_le_bytes());
    let header = sid::header(parts.len() as u8, authority);
    debug_assert!(sid::valid(&header));
    bytes[offset..offset + 8].copy_from_slice(&header);
    for (n, part) in parts.iter().enumerate() {
        bytes[offset + 8 + n * 4..offset + 12 + n * 4].copy_from_slice(&part.to_le_bytes());
    }
    Ok(bytes)
}
