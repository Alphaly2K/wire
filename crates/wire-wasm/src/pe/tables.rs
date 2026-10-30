use super::*;

pub struct Tls {
    pub template: Vec<u8>,
    pub zero_fill: u32,
    pub alignment: u64,
    pub index: u64,
    pub callbacks: Vec<u64>,
}

impl Image {
    pub(super) fn validate_runtime_functions(&self, image: &[u8]) -> Result<(), String> {
        let dir = self.dir(3);
        if self.mode != Mode::X64 || dir.size == 0 {
            return Ok(());
        }
        if !dir.size.is_multiple_of(12) {
            return Err("invalid AMD64 runtime function directory size".into());
        }
        let mut previous = 0;
        for record in bytes(image, dir.rva as usize, dir.size as usize)?
            .as_chunks::<12>()
            .0
        {
            let start = word(record, 0, 4)? as u32;
            let end = word(record, 4, 4)? as u32;
            let unwind = word(record, 8, 4)? as u32;
            if start < previous
                || start >= end
                || end > self.size
                || !self.executable(start)
                || !self.executable(end - 1)
                || unwind == 0
                || (unwind & !1) >= self.size
            {
                return Err("invalid AMD64 runtime function directory entry".into());
            }
            previous = end;
        }
        Ok(())
    }

    pub(super) fn relocate(&self, image: &mut [u8], base: u64) -> Result<(), String> {
        if base == self.preferred_base {
            return Ok(());
        }
        let dir = self.dir(5);
        if self.relocs_stripped {
            return Err("PE relocation records were stripped".into());
        }
        if dir.size == 0 {
            return Ok(());
        }
        let mut at = dir.rva as usize;
        let end = at + dir.size as usize;
        let delta = base.wrapping_sub(self.preferred_base);
        let mut patches = Vec::new();
        while at < end {
            if end - at < 8 {
                return Err("truncated PE relocation block".into());
            }
            let page = word(image, at, 4)?;
            let size = word(image, at + 4, 4)? as usize;
            if size < 8 || !size.is_multiple_of(2) || size > end - at {
                return Err("invalid PE relocation block size".into());
            }
            for offset in (8..size).step_by(2) {
                let token = word(image, at + offset, 2)?;
                let kind = token >> 12;
                if kind == 0 {
                    continue;
                }
                let width = match (kind, self.mode) {
                    (3, _) => 4,
                    (10, Mode::X64) => 8,
                    _ => return Err("unsupported PE relocation type".into()),
                };
                let target =
                    page.checked_add(token & 4095)
                        .filter(|&rva| rva + width as u64 <= u64::from(self.size))
                        .ok_or("PE relocation target outside image")? as usize;
                patches.push((
                    target,
                    width,
                    word(image, target, width)?.wrapping_add(delta),
                ));
            }
            at += size;
        }
        for (at, size, value) in patches {
            image[at..at + size].copy_from_slice(&value.to_le_bytes()[..size]);
        }
        Ok(())
    }

    pub(crate) fn imports(&self, image: &[u8]) -> Result<Vec<ImportDll>, String> {
        let dir = self.dir(1);
        if dir.rva == 0 {
            return Ok(Vec::new());
        }
        let width = self.mode as usize / 8;
        let flag = 1u64 << (width * 8 - 1);
        let mut result = Vec::new();
        let mut count = 0;
        for index in 0..512 {
            let at = dir.rva as usize + index * 20;
            if at + 20 > dir.rva as usize + dir.size as usize {
                return Err("unterminated PE import directory".into());
            }
            let descriptor = bytes(image, at, 20)?;
            if descriptor.iter().all(|b| *b == 0) {
                return Ok(result);
            }
            let ilt = word(descriptor, 0, 4)? as usize;
            let name = word(descriptor, 12, 4)? as usize;
            let iat = word(descriptor, 16, 4)? as usize;
            if name == 0 || iat == 0 {
                return Err("invalid PE import descriptor".into());
            }
            let dll = string(image, name)?;
            let mut functions = Vec::new();
            for slot in 0..=65536 {
                let offset = slot * width;
                let value = word(
                    image,
                    (if ilt == 0 { iat } else { ilt })
                        .checked_add(offset)
                        .ok_or("PE thunk overflow")?,
                    width,
                )?;
                let dest = iat.checked_add(offset).ok_or("PE IAT overflow")?;
                bytes(image, dest, width)?;
                if value == 0 {
                    break;
                }
                if count == 65536 || slot == 65536 {
                    return Err("PE import capacity exceeded".into());
                }
                count += 1;
                let (name, ordinal) = if value & flag != 0 {
                    if value & !(flag | 65535) != 0 {
                        return Err("invalid PE ordinal thunk".into());
                    }
                    (None, Some(value as u16))
                } else {
                    let rva = u32::try_from(value).map_err(|_| "PE name RVA exceeds 32 bits")?;
                    bytes(image, rva as usize, 2)?;
                    (
                        Some(string(
                            image,
                            (rva as usize)
                                .checked_add(2)
                                .ok_or("PE import name overflow")?,
                        )?),
                        None,
                    )
                };
                functions.push(ImportFunc {
                    iat_rva: u32::try_from(dest).map_err(|_| "PE IAT overflow")?,
                    name,
                    ordinal,
                });
            }
            result.push(ImportDll { dll, functions });
        }
        Err("PE descriptor capacity exceeded".into())
    }

    pub fn tls(&self, memory: &Memory, base: u64) -> Result<Option<Tls>, String> {
        let dir = self.dir(9);
        if dir.rva == 0 {
            return Ok(None);
        }
        let width = self.mode as usize / 8;
        if dir.size < (4 * width + 8) as u32 {
            return Err("truncated PE TLS directory".into());
        }
        let at = base + u64::from(dir.rva);
        let start = memory.read_int(at, width)?;
        let end = memory.read_int(at + width as u64, width)?;
        let index = memory.read_int(at + 2 * width as u64, width)?;
        let array = memory.read_int(at + 3 * width as u64, width)?;
        let zero_fill = memory.read_int(at + 4 * width as u64, 4)? as u32;
        let alignment = (memory.read_int(at + 4 * width as u64 + 4, 4)? >> 20) & 15;
        if alignment == 15 {
            return Err("invalid PE TLS alignment".into());
        }
        let length = end
            .checked_sub(start)
            .filter(|&n| {
                n.checked_add(u64::from(zero_fill))
                    .is_some_and(|size| size <= 16 * 1024 * 1024)
            })
            .ok_or("PE TLS template exceeds capacity")?;
        let inside = |address: u64, length: u64| {
            address >= base
                && address
                    .checked_add(length)
                    .is_some_and(|end| end <= base + u64::from(self.size))
        };
        if !inside(index, 4) || (!(start == 0 && end == 0) && !inside(start, length)) {
            return Err("PE TLS address outside image".into());
        }
        memory.check(index, 4, WRITE)?;
        let mut template = vec![0; length as usize];
        if length != 0 {
            memory.read(start, &mut template)?;
        }
        let mut callbacks = Vec::new();
        if array != 0 {
            for slot in 0..=1024 {
                let address = array
                    .checked_add(slot * width as u64)
                    .filter(|&at| inside(at, width as u64))
                    .ok_or("PE TLS callback table outside image")?;
                let target = memory.read_int(address, width)?;
                if target == 0 {
                    break;
                }
                if slot == 1024 || !inside(target, 1) || !self.executable((target - base) as u32) {
                    return Err("invalid PE TLS callback target".into());
                }
                memory.fetch(target, &mut [0])?;
                callbacks.push(target);
            }
        }
        Ok(Some(Tls {
            template,
            zero_fill,
            alignment: if alignment == 0 {
                16
            } else {
                1 << (alignment - 1)
            },
            index,
            callbacks,
        }))
    }
}

pub(super) fn string(image: &[u8], at: usize) -> Result<String, String> {
    let tail = image.get(at..).ok_or("PE string outside image")?;
    let length = tail
        .iter()
        .take(4096)
        .position(|b| *b == 0)
        .ok_or("unterminated PE import string")?;
    if !tail[..length].is_ascii() {
        return Err("non-ASCII PE import identity".into());
    }
    Ok(std::str::from_utf8(&tail[..length]).unwrap().to_owned())
}
