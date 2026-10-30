//! Checked PE image staging. Import resolution and publication occur only
//! after headers, backing ranges and relocation records have been validated.
use crate::{
    machine::Mode,
    memory::{Memory, EXECUTE, PAGE, READ, WRITE},
};
#[path = "../../../../src/pe/types.rs"]
mod types;
pub use types::{DataDir, ImportDll, ImportFunc, Section};
mod delay;
mod tables;
pub use delay::Delay;
pub use tables::Tls;
mod exports;
pub use exports::{Export, Exports, Symbol};
pub mod linker;
pub(crate) mod search;

pub(crate) fn bytes(b: &[u8], at: usize, size: usize) -> Result<&[u8], String> {
    b.get(at..at.checked_add(size).ok_or("PE offset overflow")?)
        .ok_or_else(|| "truncated PE data".into())
}
pub(crate) fn word(b: &[u8], at: usize, size: usize) -> Result<u64, String> {
    let mut value = [0; 8];
    value[..size].copy_from_slice(bytes(b, at, size)?);
    Ok(u64::from_le_bytes(value))
}

#[derive(Debug)]
pub struct Image {
    pub mode: Mode,
    pub is_dll: bool,
    pub dep_enabled: bool,
    pub preferred_base: u64,
    pub entry_rva: u32,
    pub size: u32,
    pub headers: u32,
    pub stack_reserve: u64,
    pub stack_commit: u64,
    pub sections: Vec<Section>,
    pub directories: Vec<DataDir>,
    relocs_stripped: bool,
}

impl Image {
    pub fn parse(b: &[u8]) -> Result<Self, String> {
        if bytes(b, 0, 2)? != b"MZ" {
            return Err("missing PE DOS signature".into());
        }
        let nt = word(b, 0x3c, 4)? as usize;
        bytes(b, nt, 24)?;
        if bytes(b, nt, 4)? != b"PE\0\0" {
            return Err("missing PE signature".into());
        }
        let mode = match word(b, nt + 4, 2)? {
            0x14c => Mode::X86,
            0x8664 => Mode::X64,
            _ => return Err("unsupported PE machine".into()),
        };
        let opt = nt + 24;
        let opt_size = word(b, nt + 20, 2)? as usize;
        bytes(b, opt, opt_size)?;
        let (magic, dir_offset, ptr) = if mode == Mode::X86 {
            (0x10b, 96, 4)
        } else {
            (0x20b, 112, 8)
        };
        if opt_size < dir_offset || word(b, opt, 2)? != magic {
            return Err("PE optional header does not match its machine".into());
        }
        let count = word(b, nt + 6, 2)? as usize;
        if count == 0 || count > 96 {
            return Err("PE section capacity exceeded".into());
        }
        let section_table = opt + opt_size;
        bytes(b, section_table, count * 40)?;
        let size = word(b, opt + 56, 4)? as u32;
        let headers = word(b, opt + 60, 4)? as u32;
        let section_align = word(b, opt + 32, 4)? as u32;
        let file_align = word(b, opt + 36, 4)? as u32;
        let base = word(b, opt + if ptr == 4 { 28 } else { 24 }, ptr)?;
        let entry = word(b, opt + 16, 4)? as u32;
        let end = base.checked_add(u64::from(size));
        if size == 0
            || size > 256 * 1024 * 1024
            || headers == 0
            || headers > size
            || (headers as usize) < section_table + count * 40
            || !section_align.is_power_of_two()
            || !file_align.is_power_of_two()
            || file_align > 65536
            || section_align < file_align
            || (section_align < PAGE as u32 && file_align != section_align)
            || (section_align >= PAGE as u32 && file_align < 512)
            || !size.is_multiple_of(section_align)
            || base < 65536
            || !base.is_multiple_of(65536)
            || end.is_none_or(|end| end > if mode == Mode::X86 { 1 << 32 } else { 1 << 47 })
            || entry >= size
        {
            return Err("invalid PE image layout".into());
        }
        bytes(b, 0, headers as usize)?;
        let nd = word(b, opt + dir_offset - 4, 4)? as usize;
        if nd > 16 || dir_offset + nd * 8 > opt_size {
            return Err("invalid PE directory count".into());
        }
        let mut directories = Vec::with_capacity(nd);
        for index in 0..nd {
            let rva = word(b, opt + dir_offset + index * 8, 4)? as u32;
            let length = word(b, opt + dir_offset + index * 8 + 4, 4)? as u32;
            if rva == 0 && length != 0 {
                return Err("PE directory has no address".into());
            }
            let limit = if index == 4 {
                b.len() as u64
            } else {
                u64::from(size)
            };
            if u64::from(rva) + u64::from(length) > limit {
                return Err("PE directory outside backing".into());
            }
            directories.push(DataDir { rva, size: length });
        }
        let mut sections = Vec::with_capacity(count);
        let mut ranges = vec![(0, u64::from(headers))];
        for index in 0..count {
            let s = bytes(b, section_table + index * 40, 40)?;
            let section = Section {
                name: String::from_utf8_lossy(
                    &s[..s[..8].iter().position(|b| *b == 0).unwrap_or(8)],
                )
                .into_owned(),
                virtual_size: word(s, 8, 4)? as u32,
                virtual_address: word(s, 12, 4)? as u32,
                raw_size: word(s, 16, 4)? as u32,
                raw_ptr: word(s, 20, 4)? as u32,
                characteristics: word(s, 36, 4)? as u32,
            };
            let start = u64::from(section.virtual_address);
            let span = u64::from(section.virtual_size.max(section.raw_size));
            if span != 0 {
                let stop = start + span;
                if !section.virtual_address.is_multiple_of(section_align)
                    || stop > u64::from(size)
                    || ranges.iter().any(|&(a, b)| start < b && stop > a)
                {
                    return Err("overlapping or out-of-image PE section".into());
                }
                if section.raw_size != 0 {
                    if !section.raw_ptr.is_multiple_of(file_align) {
                        return Err("unaligned PE raw section".into());
                    }
                    bytes(b, section.raw_ptr as usize, section.raw_size as usize)?;
                }
                ranges.push((start, stop));
            }
            sections.push(section);
        }
        let characteristics = word(b, nt + 22, 2)?;
        Ok(Self {
            mode,
            is_dll: characteristics & 0x2000 != 0,
            dep_enabled: mode == Mode::X64 || word(b, opt + 70, 2)? & 0x100 != 0,
            preferred_base: base,
            entry_rva: entry,
            size,
            headers,
            stack_reserve: word(b, opt + 72, ptr)?,
            stack_commit: word(b, opt + 72 + ptr, ptr)?,
            sections,
            directories,
            relocs_stripped: characteristics & 1 != 0,
        })
    }

    pub fn dir(&self, index: usize) -> DataDir {
        self.directories.get(index).copied().unwrap_or_default()
    }
    pub fn mapped_size(&self) -> u64 {
        u64::from(self.size).div_ceil(PAGE) * PAGE
    }
    pub fn executable(&self, rva: u32) -> bool {
        self.sections.iter().any(|s| {
            (s.is_executable() || (!self.dep_enabled && s.characteristics & 0x40000000 != 0))
                && s.contains_rva(rva)
        })
    }

    pub fn materialize(
        &self,
        b: &[u8],
        base: u64,
        mut resolve: impl FnMut(&str, &ImportFunc) -> Result<u64, String>,
    ) -> Result<Memory, String> {
        let mut image = self.relocated(b, base)?;
        for dll in self.imports(&image[..self.size as usize])? {
            for function in dll.functions {
                let value = resolve(&dll.dll, &function)?;
                self.patch_import(&mut image, function.iat_rva, value)?;
            }
        }
        self.map_bytes(base, &image)
    }

    pub(crate) fn relocated(&self, b: &[u8], base: u64) -> Result<Vec<u8>, String> {
        if base < 65536
            || !base.is_multiple_of(65536)
            || base.checked_add(self.mapped_size()).is_none_or(|end| {
                end > if self.mode == Mode::X86 {
                    1 << 32
                } else {
                    1 << 47
                }
            })
        {
            return Err("invalid PE load base".into());
        }
        let mut image = vec![0; self.mapped_size() as usize];
        image[..self.headers as usize].copy_from_slice(bytes(b, 0, self.headers as usize)?);
        for section in &self.sections {
            if section.raw_size == 0 {
                continue;
            }
            let at = section.virtual_address as usize;
            image[at..at + section.raw_size as usize].copy_from_slice(bytes(
                b,
                section.raw_ptr as usize,
                section.raw_size as usize,
            )?);
        }
        self.relocate(&mut image, base)?;
        self.validate_runtime_functions(&image)?;
        Ok(image)
    }

    pub(crate) fn patch_import(
        &self,
        image: &mut [u8],
        rva: u32,
        value: u64,
    ) -> Result<(), String> {
        if self.mode == Mode::X86 && value > u32::MAX as u64 {
            return Err("PE32 import address exceeds 32 bits".into());
        }
        let size = self.mode as usize / 8;
        bytes(&image[..self.size as usize], rva as usize, size)?;
        image[rva as usize..rva as usize + size].copy_from_slice(&value.to_le_bytes()[..size]);
        Ok(())
    }

    pub(crate) fn map_bytes(&self, base: u64, image: &[u8]) -> Result<Memory, String> {
        let mut memory = Memory::new(self.mode);
        memory.set_dep_enabled(self.dep_enabled);
        memory.map_image(base, self.mapped_size(), READ | WRITE)?;
        memory.write(base, image)?;
        let mut permissions = vec![0; image.len() / PAGE as usize];
        permissions[..(self.headers as usize).div_ceil(PAGE as usize)].fill(READ);
        for s in &self.sections {
            let span = u64::from(s.virtual_size.max(s.raw_size));
            if span == 0 {
                continue;
            }
            let flags = if s.characteristics & 0x40000000 != 0 {
                READ
            } else {
                0
            } | if s.is_writable() { WRITE } else { 0 }
                | if s.is_executable() { EXECUTE } else { 0 };
            for page in u64::from(s.virtual_address) / PAGE
                ..(u64::from(s.virtual_address) + span).div_ceil(PAGE)
            {
                permissions[page as usize] |= flags;
            }
        }
        let mut first = 0;
        while first < permissions.len() {
            let end = (first + 1..permissions.len())
                .find(|&i| permissions[i] != permissions[first])
                .unwrap_or(permissions.len());
            memory.protect(
                base + first as u64 * PAGE,
                (end - first) as u64 * PAGE,
                permissions[first],
            )?;
            first = end;
        }
        Ok(memory)
    }
}
