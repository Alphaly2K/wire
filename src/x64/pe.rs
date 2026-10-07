//! Checked PE32+ parsing/loading; RVA stays 32 bits, virtual addresses stay 64 bits.
use super::memory::{Memory, EXECUTE, PAGE, READ, WRITE};
use crate::pe::{DataDir, ImportDll, ImportFunc, Section};
#[derive(Debug, Clone)]
pub struct Pe64 {
    pub is_dll: bool,
    pub image_base: u64,
    pub entry_rva: u32,
    pub size_of_image: u32,
    pub size_of_headers: u32,
    pub stack_reserve: u64,
    pub stack_commit: u64,
    pub dirs: Vec<DataDir>,
    pub sections: Vec<Section>,
}
#[derive(Debug)]
pub struct Tls64 {
    pub template: Vec<u8>,
    pub zero_fill: u32,
    pub alignment: u64,
    pub index_address: u64,
    pub callbacks: Vec<u64>,
}
#[derive(Debug)]
pub struct Delay64 {
    pub dll: String,
    pub module_handle_rva: u32,
    pub functions: Vec<ImportFunc>,
}
#[derive(Debug, Default)]
pub struct Exports64 {
    pub names: std::collections::BTreeMap<String, u64>,
    pub ordinals: std::collections::BTreeMap<u16, u64>,
}
fn bytes(b: &[u8], at: usize, n: usize) -> Result<&[u8], String> {
    b.get(at..at.checked_add(n).ok_or("PE32+ offset overflow")?)
        .ok_or("truncated PE32+".into())
}
fn u16at(b: &[u8], at: usize) -> Result<u16, String> {
    Ok(u16::from_le_bytes(bytes(b, at, 2)?.try_into().unwrap()))
}
fn u32at(b: &[u8], at: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(bytes(b, at, 4)?.try_into().unwrap()))
}
fn u64at(b: &[u8], at: usize) -> Result<u64, String> {
    Ok(u64::from_le_bytes(bytes(b, at, 8)?.try_into().unwrap()))
}
impl Pe64 {
    pub fn parse(b: &[u8]) -> Result<Self, String> {
        if bytes(b, 0, 2)? != b"MZ" {
            return Err("missing MZ".into());
        }
        let nt = u32at(b, 0x3c)? as usize;
        if bytes(b, nt, 4)? != b"PE\0\0" || u16at(b, nt + 4)? != 0x8664 {
            return Err("not AMD64 PE".into());
        }
        let count = u16at(b, nt + 6)? as usize;
        let opt = nt + 24;
        let opt_size = u16at(b, nt + 20)? as usize;
        if opt_size < 112 || u16at(b, opt)? != 0x20b || count == 0 || count > 96 {
            return Err("invalid PE32+ headers".into());
        }
        bytes(b, opt, opt_size)?;
        let size_of_image = u32at(b, opt + 56)?;
        let size_of_headers = u32at(b, opt + 60)?;
        let section_alignment = u32at(b, opt + 32)?;
        if size_of_image == 0
            || size_of_image > 256 * 1024 * 1024
            || !(size_of_image as u64).is_multiple_of(PAGE)
            || size_of_headers == 0
            || size_of_headers > size_of_image
            || size_of_headers as usize > b.len()
            || section_alignment < PAGE as u32
            || !section_alignment.is_power_of_two()
        {
            return Err("unsupported PE32+ image layout".into());
        }
        let image_base = u64at(b, opt + 24)?;
        if !image_base.is_multiple_of(65536)
            || image_base
                .checked_add(size_of_image as u64)
                .is_none_or(|n| n > 1 << 47)
        {
            return Err("invalid PE32+ base".into());
        }
        let entry_rva = u32at(b, opt + 16)?;
        if entry_rva >= size_of_image {
            return Err("PE32+ entry outside image".into());
        }
        let nd = u32at(b, opt + 108)? as usize;
        if nd > 16 || 112 + nd * 8 > opt_size {
            return Err("invalid PE32+ directories".into());
        }
        let mut dirs = Vec::new();
        for i in 0..nd {
            let rva = u32at(b, opt + 112 + i * 8)?;
            let size = u32at(b, opt + 116 + i * 8)?;
            // SECURITY uses a file offset; every other directory uses an RVA.
            if i != 4 && rva.checked_add(size).is_none_or(|n| n > size_of_image) {
                return Err("PE32+ directory outside image".into());
            }
            dirs.push(DataDir { rva, size });
        }
        let mut sections = Vec::new();
        let mut ranges = vec![(0, size_of_headers.div_ceil(PAGE as u32) * PAGE as u32)];
        for i in 0..count {
            let at = opt + opt_size + i * 40;
            let s = bytes(b, at, 40)?;
            let name_end = s[..8].iter().position(|&b| b == 0).unwrap_or(8);
            let section = Section {
                name: String::from_utf8_lossy(&s[..name_end]).into_owned(),
                virtual_size: u32at(s, 8)?,
                virtual_address: u32at(s, 12)?,
                raw_size: u32at(s, 16)?,
                raw_ptr: u32at(s, 20)?,
                characteristics: u32at(s, 36)?,
            };
            let span = section.virtual_size.max(section.raw_size);
            if span == 0 {
                sections.push(section);
                continue;
            }
            let end = section
                .virtual_address
                .checked_add(span)
                .ok_or("PE32+ section overflow")?;
            if !section.virtual_address.is_multiple_of(section_alignment)
                || end > size_of_image
                || ranges
                    .iter()
                    .any(|&(a, b)| section.virtual_address < b && end > a)
            {
                return Err("overlapping/out-of-image PE32+ section".into());
            }
            bytes(b, section.raw_ptr as usize, section.raw_size as usize)?;
            ranges.push((
                section.virtual_address,
                end.div_ceil(PAGE as u32) * PAGE as u32,
            ));
            sections.push(section);
        }
        Ok(Self {
            is_dll: u16at(b, nt + 22)? & 0x2000 != 0,
            image_base,
            entry_rva,
            size_of_image,
            size_of_headers,
            stack_reserve: u64at(b, opt + 72)?,
            stack_commit: u64at(b, opt + 80)?,
            dirs,
            sections,
        })
    }
    pub fn dir(&self, i: usize) -> DataDir {
        self.dirs.get(i).copied().unwrap_or_default()
    }
    fn image(&self, b: &[u8]) -> Result<Vec<u8>, String> {
        let mut image = vec![0; self.size_of_image as usize];
        image[..self.size_of_headers as usize].copy_from_slice(bytes(
            b,
            0,
            self.size_of_headers as usize,
        )?);
        for s in &self.sections {
            if s.raw_size == 0 {
                continue;
            }
            let at = s.virtual_address as usize;
            let size = s.raw_size as usize;
            image[at..at + size].copy_from_slice(bytes(b, s.raw_ptr as usize, size)?);
        }
        Ok(image)
    }
    pub fn imports(&self, b: &[u8]) -> Result<Vec<ImportDll>, String> {
        self.image_imports(&self.image(b)?)
    }
    pub fn exports(&self, b: &[u8], base: u64) -> Result<Exports64, String> {
        let dir = self.dir(0);
        let mut out = Exports64::default();
        if dir.rva == 0 {
            return Ok(out);
        }
        if dir.size < 40 {
            return Err("truncated AMD64 exports".into());
        }
        let image = self.image(b)?;
        let at = dir.rva as usize;
        let first = u32at(&image, at + 16)?;
        let count = u32at(&image, at + 20)? as usize;
        let names = u32at(&image, at + 24)? as usize;
        let funcs = u32at(&image, at + 28)? as usize;
        let strings = u32at(&image, at + 32)? as usize;
        let ordinals = u32at(&image, at + 36)? as usize;
        if count > 65536 || names > 65536 {
            return Err("AMD64 exports exceed capacity".into());
        }
        bytes(&image, funcs, count * 4)?;
        bytes(&image, strings, names * 4)?;
        bytes(&image, ordinals, names * 2)?;
        for index in 0..count {
            let rva = u32at(&image, funcs + index * 4)?;
            if rva == 0 {
                continue;
            }
            if rva >= dir.rva && rva < dir.rva + dir.size {
                return Err("AMD64 forwarded exports are not implemented".into());
            }
            if rva >= self.size_of_image {
                return Err("AMD64 export outside image".into());
            }
            let ordinal = u16::try_from(first as u64 + index as u64)
                .map_err(|_| "AMD64 export ordinal exceeds 16 bits")?;
            out.ordinals.insert(
                ordinal,
                base.checked_add(rva as u64)
                    .ok_or("AMD64 export address overflow")?,
            );
        }
        for index in 0..names {
            let rva = u32at(&image, strings + index * 4)? as usize;
            let ord = u16at(&image, ordinals + index * 2)? as usize;
            if ord >= count {
                return Err("invalid AMD64 export name ordinal".into());
            }
            let tail = image.get(rva..).ok_or("AMD64 export name outside image")?;
            let n = tail
                .iter()
                .take(4096)
                .position(|&b| b == 0)
                .ok_or("unterminated AMD64 export name")?;
            let name = std::str::from_utf8(&tail[..n])
                .map_err(|_| "invalid AMD64 export name")?
                .to_owned();
            let ordinal = u16::try_from(first as u64 + ord as u64)
                .map_err(|_| "AMD64 export ordinal overflow")?;
            let address = *out
                .ordinals
                .get(&ordinal)
                .ok_or("AMD64 named export points to empty entry")?;
            if out.names.insert(name, address).is_some() {
                return Err("duplicate AMD64 export name".into());
            }
        }
        Ok(out)
    }
    fn image_imports(&self, image: &[u8]) -> Result<Vec<ImportDll>, String> {
        let dir = self.dir(1);
        if dir.rva == 0 {
            return Ok(Vec::new());
        }
        let string = |rva: u32| -> Result<String, String> {
            let tail = image
                .get(rva as usize..)
                .ok_or("import string outside PE32+")?;
            let n = tail
                .iter()
                .take(4096)
                .position(|&b| b == 0)
                .ok_or("unterminated PE32+ import string")?;
            std::str::from_utf8(&tail[..n])
                .map(str::to_owned)
                .map_err(|_| "non-ASCII PE32+ import name".into())
        };
        let mut out = Vec::new();
        for i in 0..512usize {
            let at = dir.rva as usize + i * 20;
            if at + 20 > dir.rva as usize + dir.size as usize {
                return Err("unterminated PE32+ import directory".into());
            }
            let desc = bytes(image, at, 20)?;
            if desc.iter().all(|&b| b == 0) {
                return Ok(out);
            }
            let ilt = u32at(desc, 0)?;
            let name = u32at(desc, 12)?;
            let iat = u32at(desc, 16)?;
            if name == 0 || iat == 0 {
                return Err("invalid PE32+ import descriptor".into());
            }
            let mut functions = Vec::new();
            let mut terminated = false;
            for j in 0..65536u32 {
                let offset = j.checked_mul(8).ok_or("thunk overflow")?;
                let thunk = (if ilt == 0 { iat } else { ilt })
                    .checked_add(offset)
                    .ok_or("thunk overflow")?;
                let value = u64at(image, thunk as usize)?;
                let iat_rva = iat.checked_add(offset).ok_or("IAT overflow")?;
                bytes(image, iat_rva as usize, 8)?;
                if value == 0 {
                    terminated = true;
                    break;
                }
                let (name, ordinal) = if value & (1 << 63) != 0 {
                    if value & 0x7fffffffffff0000 != 0 {
                        return Err("invalid AMD64 ordinal thunk".into());
                    }
                    (None, Some(value as u16))
                } else {
                    let rva = u32::try_from(value).map_err(|_| "AMD64 name RVA exceeds 32 bits")?;
                    (
                        Some(string(rva.checked_add(2).ok_or("name overflow")?)?),
                        None,
                    )
                };
                functions.push(ImportFunc {
                    iat_rva,
                    name,
                    ordinal,
                });
            }
            if !terminated {
                return Err("unterminated AMD64 thunk table".into());
            }
            out.push(ImportDll {
                dll: string(name)?,
                functions,
            });
        }
        Err("too many AMD64 imports".into())
    }
    pub fn load(
        &self,
        b: &[u8],
        base: u64,
        mut resolve: impl FnMut(&str, &ImportFunc) -> Result<u64, String>,
    ) -> Result<Memory, String> {
        if self.dir(13).rva != 0 || self.dir(9).rva != 0 {
            return Err(
                "AMD64 delay imports/TLS are not implemented by the CPU-only loader".into(),
            );
        }
        self.load_with_runtime(b, base, &mut resolve)
    }
    /// Mapping for a runtime that initializes static TLS and installs delay gateways before execution.
    pub fn load_with_runtime(
        &self,
        b: &[u8],
        base: u64,
        mut resolve: impl FnMut(&str, &ImportFunc) -> Result<u64, String>,
    ) -> Result<Memory, String> {
        if !base.is_multiple_of(65536)
            || base
                .checked_add(self.size_of_image as u64)
                .is_none_or(|n| n > 1 << 47)
        {
            return Err("invalid AMD64 load base".into());
        }
        let mut image = self.image(b)?;
        if base != self.image_base {
            let dir = self.dir(5);
            if dir.rva == 0 || dir.size == 0 {
                return Err("PE32+ relocation directory missing".into());
            }
            let delta = base.wrapping_sub(self.image_base);
            let end = dir.rva as usize + dir.size as usize;
            let mut at = dir.rva as usize;
            let mut patches = Vec::new();
            while at < end {
                if end - at < 8 {
                    return Err("truncated AMD64 relocation block".into());
                }
                let page = u32at(&image, at)?;
                let size = u32at(&image, at + 4)? as usize;
                if size < 8 || !size.is_multiple_of(2) || size > end - at {
                    return Err("invalid AMD64 relocation size".into());
                }
                for i in (8..size).step_by(2) {
                    let token = u16at(&image, at + i)?;
                    if token >> 12 == 0 {
                        continue;
                    }
                    if token >> 12 != 10 {
                        return Err(format!("unsupported AMD64 relocation {}", token >> 12));
                    }
                    let rva = page
                        .checked_add((token & 4095) as u32)
                        .ok_or("DIR64 overflow")? as usize;
                    let value = u64at(&image, rva)?.wrapping_add(delta);
                    patches.push((rva, value));
                }
                at += size;
            }
            for (at, value) in patches {
                image[at..at + 8].copy_from_slice(&value.to_le_bytes());
            }
        }
        for dll in self.image_imports(&image)? {
            for f in dll.functions {
                let target = resolve(&dll.dll, &f)?;
                image[f.iat_rva as usize..f.iat_rva as usize + 8]
                    .copy_from_slice(&target.to_le_bytes());
            }
        }
        let mut memory = Memory::default();
        memory.map_image(base, self.size_of_image as u64, READ | WRITE)?;
        memory.write(base, &image)?;
        memory.protect(base, self.size_of_image as u64, 0)?;
        let header_size = (self.size_of_headers as u64).div_ceil(PAGE) * PAGE;
        memory.protect(base, header_size, READ)?;
        for s in &self.sections {
            let span = s.virtual_size.max(s.raw_size) as u64;
            if span == 0 {
                continue;
            }
            let permissions = if s.characteristics & 0x40000000 != 0 {
                READ
            } else {
                0
            } | if s.is_writable() { WRITE } else { 0 }
                | if s.is_executable() { EXECUTE } else { 0 };
            memory.protect(
                base + s.virtual_address as u64,
                span.div_ceil(PAGE) * PAGE,
                permissions,
            )?;
        }
        Ok(memory)
    }
    pub fn tls(&self, mem: &Memory, base: u64) -> Result<Option<Tls64>, String> {
        let dir = self.dir(9);
        if dir.rva == 0 {
            return Ok(None);
        }
        if dir.size < 40 {
            return Err("truncated AMD64 TLS directory".into());
        }
        let at = base
            .checked_add(dir.rva as u64)
            .ok_or("TLS directory overflow")?;
        let start = mem.read_int(at, 8)?;
        let end = mem.read_int(at + 8, 8)?;
        let index_address = mem.read_int(at + 16, 8)?;
        let callback_array = mem.read_int(at + 24, 8)?;
        let zero_fill = mem.read_int(at + 32, 4)? as u32;
        let alignment = (mem.read_int(at + 36, 4)? >> 20) & 15;
        if alignment == 15 {
            return Err("invalid AMD64 TLS alignment".into());
        }
        let size = end
            .checked_sub(start)
            .ok_or("reversed AMD64 TLS template")?;
        if size
            .checked_add(zero_fill as u64)
            .is_none_or(|n| n > 16 * 1024 * 1024)
        {
            return Err("AMD64 TLS template exceeds capacity".into());
        }
        mem.check(index_address, 4, WRITE)?;
        let mut template = vec![0; size as usize];
        mem.read(start, &mut template)?;
        let mut callbacks = Vec::new();
        if callback_array != 0 {
            for n in 0..1024 {
                let address = mem.read_int(
                    callback_array
                        .checked_add(n * 8)
                        .ok_or("TLS callback array overflow")?,
                    8,
                )?;
                if address == 0 {
                    return Ok(Some(Tls64 {
                        template,
                        zero_fill,
                        alignment: if alignment == 0 {
                            16
                        } else {
                            1 << (alignment - 1)
                        },
                        index_address,
                        callbacks,
                    }));
                }
                mem.fetch(address, &mut [0])?;
                callbacks.push(address);
            }
            return Err("unterminated AMD64 TLS callbacks".into());
        }
        Ok(Some(Tls64 {
            template,
            zero_fill,
            alignment: if alignment == 0 {
                16
            } else {
                1 << (alignment - 1)
            },
            index_address,
            callbacks,
        }))
    }
    pub fn delay_imports(&self, b: &[u8]) -> Result<Vec<Delay64>, String> {
        let dir = self.dir(13);
        if dir.rva == 0 {
            return Ok(Vec::new());
        }
        let image = self.image(b)?;
        let string = |rva: u32| -> Result<String, String> {
            let tail = image
                .get(rva as usize..)
                .ok_or("AMD64 delay name outside image")?;
            let n = tail
                .iter()
                .take(4096)
                .position(|&b| b == 0)
                .ok_or("unterminated AMD64 delay name")?;
            std::str::from_utf8(&tail[..n])
                .map(str::to_owned)
                .map_err(|_| "invalid AMD64 delay name".into())
        };
        let mut out = Vec::new();
        for n in 0..512 {
            let at = dir.rva as usize + n * 32;
            if at + 32 > dir.rva as usize + dir.size as usize {
                return Err("unterminated AMD64 delay directory".into());
            }
            let d = bytes(&image, at, 32)?;
            if d.iter().all(|&b| b == 0) {
                return Ok(out);
            }
            if u32at(d, 0)? != 1 {
                return Err("AMD64 delay descriptor requires RVA attributes".into());
            }
            let name = u32at(d, 4)?;
            let module_handle_rva = u32at(d, 8)?;
            let iat = u32at(d, 12)?;
            let names = u32at(d, 16)?;
            if name == 0 || module_handle_rva == 0 || iat == 0 || names == 0 {
                return Err("invalid AMD64 delay descriptor".into());
            }
            bytes(&image, module_handle_rva as usize, 8)?;
            let mut functions = Vec::new();
            let mut terminated = false;
            for index in 0..65536u32 {
                let offset = index * 8;
                let value = u64at(
                    &image,
                    names
                        .checked_add(offset)
                        .ok_or("AMD64 delay thunk overflow")? as usize,
                )?;
                let iat_rva = iat.checked_add(offset).ok_or("AMD64 delay IAT overflow")?;
                bytes(&image, iat_rva as usize, 8)?;
                if value == 0 {
                    terminated = true;
                    break;
                }
                let (name, ordinal) = if value & (1 << 63) != 0 {
                    if value & 0x7fffffffffff0000 != 0 {
                        return Err("invalid AMD64 delay ordinal".into());
                    }
                    (None, Some(value as u16))
                } else {
                    let rva =
                        u32::try_from(value).map_err(|_| "AMD64 delay name exceeds RVA width")?;
                    (
                        Some(string(
                            rva.checked_add(2).ok_or("AMD64 delay name overflow")?,
                        )?),
                        None,
                    )
                };
                functions.push(ImportFunc {
                    iat_rva,
                    name,
                    ordinal,
                });
            }
            if !terminated {
                return Err("unterminated AMD64 delay thunks".into());
            }
            out.push(Delay64 {
                dll: string(name)?,
                module_handle_rva,
                functions,
            });
        }
        Err("AMD64 delay descriptor capacity exceeded".into())
    }
}
