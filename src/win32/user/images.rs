//! Image identity, resource lifetime and pure icon validation; no guest memory.
use std::collections::BTreeMap;

#[derive(PartialEq, Eq)]
pub enum Key {
    Id(u32),
    Name(String),
}

struct Image {
    instance: Option<u64>,
    key: Key,
    kind: u32,
    shared: bool,
    _payload: Vec<u8>,
}

pub(crate) struct Images {
    records: BTreeMap<u64, Image>,
    next: u64,
    end: u64,
    byte_budget: Option<usize>,
}

impl Images {
    pub(crate) fn new(start: u64, end: u64) -> Self {
        Self::with_byte_budget(start, end, None)
    }
    pub(crate) fn with_byte_budget(start: u64, end: u64, byte_budget: Option<usize>) -> Self {
        Self {
            records: BTreeMap::new(),
            next: start,
            end,
            byte_budget,
        }
    }

    pub(crate) fn shared(&self, instance: u64, key: &Key, kind: u32) -> Option<u64> {
        self.records.iter().find_map(|(&handle, image)| {
            (image.shared
                && image.instance == Some(instance)
                && &image.key == key
                && image.kind == kind)
                .then_some(handle)
        })
    }

    pub(crate) fn is_cursor(&self, handle: u64) -> bool {
        self.records
            .get(&handle)
            .is_some_and(|image| image.kind == 2)
    }

    pub(crate) fn insert(
        &mut self,
        instance: u64,
        key: Key,
        kind: u32,
        shared: bool,
        payload: Vec<u8>,
    ) -> Result<u64, u32> {
        if self.records.len() >= 4096 || self.next >= self.end {
            return Err(8);
        }
        if let Some(budget) = self.byte_budget {
            let key_bytes = |key: &Key| match key {
                Key::Id(_) => 0,
                Key::Name(name) => name.len(),
            };
            let used: usize = self
                .records
                .values()
                .map(|image| key_bytes(&image.key) + image._payload.len())
                .sum();
            if used
                .checked_add(key_bytes(&key))
                .and_then(|n| n.checked_add(payload.len()))
                .is_none_or(|n| n > budget)
            {
                return Err(8);
            }
        }
        let handle = self.next;
        self.next += 16;
        self.records.insert(
            handle,
            Image {
                instance: Some(instance),
                key,
                kind,
                shared,
                _payload: payload,
            },
        );
        Ok(handle)
    }

    #[allow(dead_code)]
    pub(crate) fn retire_instance(&mut self, instance: u64) {
        // Loaded handles own their payload; only the module cache key expires.
        for image in self.records.values_mut() {
            if image.instance == Some(instance) {
                image.instance = None;
            }
        }
    }

    pub(crate) fn destroy(&mut self, handle: u64, kind: u32) -> Result<(), u32> {
        let Some(image) = self.records.get(&handle).filter(|image| image.kind == kind) else {
            return Err(6);
        };
        if !image.shared {
            self.records.remove(&handle);
        }
        Ok(())
    }
}

pub(crate) fn stock(kind: u32, id: u32) -> bool {
    if kind == 1 {
        matches!(id, 32512..=32518)
    } else {
        matches!(id, 32512..=32516 | 32640..=32646 | 32648..=32651 | 32671..=32672)
    }
}

pub(crate) fn icon_group(group: &[u8]) -> Result<(u16, u32), String> {
    if group.len() < 6 || group[..4] != [0, 0, 1, 0] {
        return Err("invalid icon group header".into());
    }
    let count = u16::from_le_bytes(group[4..6].try_into().unwrap()) as usize;
    if count == 0 || count > 256 || 6 + count * 14 > group.len() {
        return Err("truncated icon group".into());
    }
    let index = (0..count)
        .min_by_key(|&i| {
            let width = if group[6 + i * 14] == 0 {
                256u16
            } else {
                group[6 + i * 14] as u16
            };
            width.abs_diff(32)
        })
        .unwrap();
    let offset = 6 + index * 14;
    Ok((
        u16::from_le_bytes(group[offset + 12..offset + 14].try_into().unwrap()),
        u32::from_le_bytes(group[offset + 8..offset + 12].try_into().unwrap()),
    ))
}

// Ok(false) is the existing unsupported-DIB-format result (LastError 50).
pub(crate) fn icon_payload(payload: &[u8], length: u32) -> Result<bool, String> {
    if payload.len() != length as usize {
        return Err("icon group payload size mismatch".into());
    }
    if payload.len() < 40 {
        return Err("truncated icon DIB".into());
    }
    let word = |at| u32::from_le_bytes(payload[at..at + 4].try_into().unwrap());
    if word(0) != 40
        || word(16) != 0
        || u16::from_le_bytes(payload[14..16].try_into().unwrap()) != 32
    {
        return Ok(false);
    }
    let width = word(4);
    let doubled_height = word(8);
    if width == 0
        || width > 256
        || doubled_height == 0
        || doubled_height > 512
        || !doubled_height.is_multiple_of(2)
        || u16::from_le_bytes(payload[12..14].try_into().unwrap()) != 1
    {
        return Err("invalid icon DIB dimensions/planes".into());
    }
    let height = doubled_height / 2;
    let required = 40 + width * height * 4 + width.div_ceil(32) * 4 * height;
    if payload.len() < required as usize {
        return Err("truncated icon bitmap/mask".into());
    }
    Ok(true)
}
