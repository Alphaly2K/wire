//! Bounded PE32+ resource tree traversal. RVAs are u32; all guest pointers are u64.
use super::{Memory, Runtime64};
#[derive(PartialEq, Eq)]
pub(super) enum Key {
    Id(u32),
    Name(String),
}
impl Runtime64 {
    pub(super) fn resource_key(mem: &Memory, pointer: u64, wide: bool) -> Result<Key, String> {
        if pointer <= 65535 {
            Ok(Key::Id(pointer as u32))
        } else {
            Ok(Key::Name(Self::path_name(mem, pointer, wide)?))
        }
    }
    pub(super) fn resource(
        &self,
        mem: &Memory,
        base: u64,
        kind: u32,
        key: &Key,
    ) -> Result<Option<Vec<u8>>, String> {
        let (image_size, _) = self
            .modules
            .image(base)
            .ok_or("invalid Win64 resource module")?;
        let nt = mem.read_int(base + 0x3c, 4)?;
        if nt
            > (image_size as u64)
                .checked_sub(160)
                .ok_or("truncated resource image")?
            || mem.read_int(base + nt, 4)? != 0x4550
            || mem.read_int(base + nt + 24, 2)? != 0x20b
        {
            return Err("invalid PE32+ resource header".into());
        }
        let directory = base + nt + 24 + 112 + 2 * 8;
        let rva = mem.read_int(directory, 4)?;
        let size = mem.read_int(directory + 4, 4)?;
        if rva == 0 || size == 0 {
            return Ok(None);
        }
        if rva + size > image_size as u64 {
            return Err("resource directory outside image".into());
        }
        let root = base + rva;
        let Some(kind) = entry(mem, root, size, 0, Some(&Key::Id(kind)))? else {
            return Ok(None);
        };
        if kind & 0x80000000 == 0 {
            return Err("resource type is not a directory".into());
        }
        let Some(item) = entry(mem, root, size, kind & 0x7fffffff, Some(key))? else {
            return Ok(None);
        };
        if item & 0x80000000 == 0 {
            return Err("resource name is not a directory".into());
        }
        let Some(data) = entry(mem, root, size, item & 0x7fffffff, None)? else {
            return Ok(None);
        };
        if data & 0x80000000 != 0 || data + 16 > size {
            return Err("invalid resource data entry".into());
        }
        let payload = mem.read_int(root + data, 4)?;
        let length = mem.read_int(root + data + 4, 4)?;
        if payload + length > image_size as u64 || length > 4 * 1024 * 1024 {
            return Err("resource payload outside image/capacity".into());
        }
        let mut bytes = vec![0; length as usize];
        mem.read(base + payload, &mut bytes)?;
        Ok(Some(bytes))
    }
}
fn entry(
    mem: &Memory,
    root: u64,
    size: u64,
    dir: u64,
    key: Option<&Key>,
) -> Result<Option<u64>, String> {
    if dir + 16 > size {
        return Err("truncated resource directory".into());
    }
    let count = mem.read_int(root + dir + 12, 2)? + mem.read_int(root + dir + 14, 2)?;
    if dir + 16 + count * 8 > size {
        return Err("truncated resource entries".into());
    }
    for i in 0..count {
        let address = root + dir + 16 + i * 8;
        let name = mem.read_int(address, 4)?;
        let matched = match key {
            None => true,
            Some(Key::Id(id)) => name == *id as u64,
            Some(Key::Name(text)) if name & 0x80000000 != 0 => {
                let offset = name & 0x7fffffff;
                if offset + 2 > size {
                    return Err("truncated resource name".into());
                }
                let len = mem.read_int(root + offset, 2)?;
                if offset + 2 + len * 2 > size {
                    return Err("truncated resource name units".into());
                }
                let mut units = Vec::new();
                for n in 0..len {
                    units.push(mem.read_int(root + offset + 2 + n * 2, 2)? as u16);
                }
                String::from_utf16(&units)
                    .map_err(|_| "invalid resource name UTF-16")?
                    .eq_ignore_ascii_case(text)
            }
            _ => false,
        };
        if matched {
            return Ok(Some(mem.read_int(address + 4, 4)?));
        }
    }
    Ok(None)
}
