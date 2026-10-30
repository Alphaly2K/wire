//! Bounded resource traversal over a loaded PE image.
use super::images::Key;

pub(crate) trait Reader {
    type Error;
    fn read(&self, address: u64, output: &mut [u8]) -> Result<(), Self::Error>;
    fn invalid(&self, message: &'static str) -> Self::Error;
    fn name(&self, units: &[u16]) -> Result<String, Self::Error> {
        String::from_utf16(units).map_err(|_| self.invalid("invalid resource name UTF-16"))
    }
    fn read_int(&self, address: u64, size: usize) -> Result<u64, Self::Error> {
        let mut bytes = [0; 8];
        self.read(address, &mut bytes[..size])?;
        Ok(u64::from_le_bytes(bytes))
    }
}
#[derive(Clone, Copy)]
pub(crate) struct Directory {
    pub base: u64,
    pub image_size: u32,
    pub rva: u32,
    pub size: u32,
}
impl Directory {
    fn root<R: Reader>(self, mem: &R) -> Result<Option<u64>, R::Error> {
        if self.rva == 0 || self.size == 0 {
            return Ok(None);
        }
        if u64::from(self.rva) + u64::from(self.size) > u64::from(self.image_size) {
            return Err(mem.invalid("resource directory outside image"));
        }
        Ok(Some(self.base + u64::from(self.rva)))
    }
    pub fn find<R: Reader>(
        self,
        mem: &R,
        kind: &Key,
        key: &Key,
        language: Option<u32>,
    ) -> Result<Option<u64>, R::Error> {
        let Some(root) = self.root(mem)? else {
            return Ok(None);
        };
        let size = u64::from(self.size);
        let Some(kind) = entry(mem, root, size, 0, Some(kind))? else {
            return Ok(None);
        };
        if kind & 0x80000000 == 0 {
            return Err(mem.invalid("resource type is not a directory"));
        }
        let Some(item) = entry(mem, root, size, kind & 0x7fffffff, Some(key))? else {
            return Ok(None);
        };
        if item & 0x80000000 == 0 {
            return Err(mem.invalid("resource name is not a directory"));
        }
        let language = language.map(Key::Id);
        let selected = if let Some(language) = &language {
            entry(mem, root, size, item & 0x7fffffff, Some(language))?
        } else {
            None
        };
        let selected = match selected {
            Some(data) => Some(data),
            None => entry(mem, root, size, item & 0x7fffffff, None)?,
        };
        let Some(data) = selected else {
            return Ok(None);
        };
        if data & 0x80000000 != 0 || data + 16 > size {
            return Err(mem.invalid("invalid resource data entry"));
        }
        Ok(Some(root + data))
    }
    pub fn data<R: Reader>(self, mem: &R, descriptor: u64) -> Result<(u64, u32), R::Error> {
        let root = self
            .root(mem)?
            .ok_or_else(|| mem.invalid("missing resource directory"))?;
        if descriptor < root || descriptor - root + 16 > u64::from(self.size) {
            return Err(mem.invalid("invalid resource data entry"));
        }
        let payload = mem.read_int(descriptor, 4)?;
        let length = mem.read_int(descriptor + 4, 4)?;
        if payload + length > u64::from(self.image_size) {
            return Err(mem.invalid("resource payload outside image/capacity"));
        }
        Ok((self.base + payload, length as u32))
    }
}
pub(crate) fn string_slot<R: Reader>(
    mem: &R,
    mut pointer: u64,
    size: u32,
    id: u32,
) -> Result<Option<(u64, u32)>, R::Error> {
    let end = pointer + u64::from(size);
    for i in 0..16 {
        if pointer + 2 > end {
            return Err(mem.invalid("truncated string resource length"));
        }
        let length = mem.read_int(pointer, 2)? as u32;
        pointer += 2;
        if pointer + u64::from(length) * 2 > end {
            return Err(mem.invalid("truncated string resource payload"));
        }
        if i == id & 15 {
            return Ok(Some((pointer, length)));
        }
        pointer += u64::from(length) * 2;
    }
    Ok(None)
}
pub(crate) fn resource<R: Reader>(
    mem: &R,
    base: u64,
    image_size: u32,
    rva: u32,
    size: u32,
    kind: u32,
    key: &Key,
) -> Result<Option<Vec<u8>>, R::Error> {
    let directory = Directory {
        base,
        image_size,
        rva,
        size,
    };
    let Some(descriptor) = directory.find(mem, &Key::Id(kind), key, None)? else {
        return Ok(None);
    };
    let (payload, length) = directory.data(mem, descriptor)?;
    if length > 4 * 1024 * 1024 {
        return Err(mem.invalid("resource payload outside image/capacity"));
    }
    let mut bytes = vec![0; length as usize];
    mem.read(payload, &mut bytes)?;
    Ok(Some(bytes))
}
fn entry<R: Reader>(
    mem: &R,
    root: u64,
    size: u64,
    dir: u64,
    key: Option<&Key>,
) -> Result<Option<u64>, R::Error> {
    if dir + 16 > size {
        return Err(mem.invalid("truncated resource directory"));
    }
    let count = mem.read_int(root + dir + 12, 2)? + mem.read_int(root + dir + 14, 2)?;
    if dir + 16 + count * 8 > size {
        return Err(mem.invalid("truncated resource entries"));
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
                    return Err(mem.invalid("truncated resource name"));
                }
                let len = mem.read_int(root + offset, 2)?;
                if offset + 2 + len * 2 > size {
                    return Err(mem.invalid("truncated resource name units"));
                }
                let mut units = Vec::new();
                for n in 0..len {
                    units.push(mem.read_int(root + offset + 2 + n * 2, 2)? as u16);
                }
                mem.name(&units)?.eq_ignore_ascii_case(text)
            }
            _ => false,
        };
        if matched {
            return Ok(Some(mem.read_int(address + 4, 4)?));
        }
    }
    Ok(None)
}
