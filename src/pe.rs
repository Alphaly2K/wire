//! Minimal PE32 parser (no external dependencies).

pub const IMAGE_FILE_MACHINE_I386: u16 = 0x14c;
pub const IMAGE_FILE_MACHINE_AMD64: u16 = 0x8664;
pub const PE32_MAGIC: u16 = 0x10b;

pub const DIR_EXPORT: usize = 0;
pub const DIR_IMPORT: usize = 1;
pub const DIR_RESOURCE: usize = 2;
pub const DIR_BASERELOC: usize = 5;
pub const DIR_IAT: usize = 12;

#[derive(Debug, Clone)]
pub struct Section {
    pub name: String,
    pub virtual_size: u32,
    pub virtual_address: u32,
    pub raw_size: u32,
    pub raw_ptr: u32,
    pub characteristics: u32,
}

impl Section {
    pub fn is_executable(&self) -> bool {
        self.characteristics & 0x2000_0000 != 0
    }
    pub fn is_writable(&self) -> bool {
        self.characteristics & 0x8000_0000 != 0
    }
    pub fn contains_rva(&self, rva: u32) -> bool {
        let span = self.virtual_size.max(self.raw_size);
        rva >= self.virtual_address && rva < self.virtual_address + span
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DataDir {
    pub rva: u32,
    pub size: u32,
}

#[derive(Debug)]
pub struct PeFile {
    pub machine: u16,
    pub characteristics: u16,
    pub entry_rva: u32,
    pub image_base: u32,
    pub section_alignment: u32,
    pub file_alignment: u32,
    pub size_of_image: u32,
    pub size_of_headers: u32,
    pub subsystem: u16,
    pub dll_characteristics: u16,
    pub stack_reserve: u32,
    pub stack_commit: u32,
    pub heap_reserve: u32,
    pub heap_commit: u32,
    pub dirs: Vec<DataDir>,
    pub sections: Vec<Section>,
}

#[derive(Debug)]
pub struct ImportFunc {
    pub iat_rva: u32,
    pub name: Option<String>,
    pub ordinal: Option<u16>,
}

#[derive(Debug)]
pub struct ImportDll {
    pub dll: String,
    pub functions: Vec<ImportFunc>,
}

impl PeFile {
    pub fn parse(buf: &[u8]) -> Result<PeFile, String> {
        if buf.len() < 0x40 {
            return Err("file too small for DOS header".into());
        }
        if &buf[0..2] != b"MZ" {
            return Err("missing MZ magic".into());
        }
        let lfanew = rd32(buf, 0x3c)? as usize;
        if buf.len() < lfanew + 24 {
            return Err("file too small for NT headers".into());
        }
        if &buf[lfanew..lfanew + 4] != b"PE\0\0" {
            return Err("missing PE signature".into());
        }
        let coff = lfanew + 4;
        let machine = rd16(buf, coff)?;
        if machine == IMAGE_FILE_MACHINE_AMD64 {
            return Err(
                "x86-64 (AMD64) is not supported; wire currently runs PE32 / i386 only".into(),
            );
        }
        if machine != IMAGE_FILE_MACHINE_I386 {
            return Err(format!("not a 32-bit x86 PE (machine=0x{machine:04x})"));
        }
        let num_sections = rd16(buf, coff + 2)? as usize;
        let size_opt = rd16(buf, coff + 16)? as usize;
        let characteristics = rd16(buf, coff + 18)?;
        let opt = coff + 20;
        if size_opt < 96 || buf.len() < opt + size_opt {
            return Err("truncated optional header".into());
        }
        let magic = rd16(buf, opt)?;
        if magic != PE32_MAGIC {
            return Err(format!("not PE32 (magic=0x{magic:04x})"));
        }
        let entry_rva = rd32(buf, opt + 16)?;
        let image_base = rd32(buf, opt + 28)?;
        let section_alignment = rd32(buf, opt + 32)?;
        let file_alignment = rd32(buf, opt + 36)?;
        let size_of_image = rd32(buf, opt + 56)?;
        let size_of_headers = rd32(buf, opt + 60)?;
        let subsystem = rd16(buf, opt + 68)?;
        let dll_characteristics = rd16(buf, opt + 70)?;
        let stack_reserve = rd32(buf, opt + 72)?;
        let stack_commit = rd32(buf, opt + 76)?;
        let heap_reserve = rd32(buf, opt + 80)?;
        let heap_commit = rd32(buf, opt + 84)?;
        let num_dirs = (rd32(buf, opt + 92)? as usize).min(16);
        if 96 + num_dirs * 8 > size_opt {
            return Err("data directories exceed optional header".into());
        }
        let mut dirs = Vec::with_capacity(num_dirs);
        for i in 0..num_dirs {
            dirs.push(DataDir {
                rva: rd32(buf, opt + 96 + i * 8)?,
                size: rd32(buf, opt + 96 + i * 8 + 4)?,
            });
        }
        let sec_off = opt + size_opt;
        if buf.len() < sec_off + num_sections * 40 {
            return Err("truncated section table".into());
        }
        let mut sections = Vec::with_capacity(num_sections);
        for i in 0..num_sections {
            let o = sec_off + i * 40;
            let name_bytes = &buf[o..o + 8];
            let name_len = name_bytes.iter().position(|&b| b == 0).unwrap_or(8);
            sections.push(Section {
                name: String::from_utf8_lossy(&name_bytes[..name_len]).into_owned(),
                virtual_size: rd32(buf, o + 8)?,
                virtual_address: rd32(buf, o + 12)?,
                raw_size: rd32(buf, o + 16)?,
                raw_ptr: rd32(buf, o + 20)?,
                characteristics: rd32(buf, o + 36)?,
            });
        }
        Ok(PeFile {
            machine,
            characteristics,
            entry_rva,
            image_base,
            section_alignment,
            file_alignment,
            size_of_image,
            size_of_headers,
            subsystem,
            dll_characteristics,
            stack_reserve,
            stack_commit,
            heap_reserve,
            heap_commit,
            dirs,
            sections,
        })
    }

    pub fn dir(&self, idx: usize) -> DataDir {
        self.dirs.get(idx).copied().unwrap_or_default()
    }

    /// Map an RVA to a file offset (headers + section raw data).
    pub fn rva_to_off(&self, rva: u32) -> Option<usize> {
        if rva < self.size_of_headers {
            return Some(rva as usize);
        }
        for s in &self.sections {
            if s.contains_rva(rva) {
                let delta = (rva - s.virtual_address) as usize;
                if delta < s.raw_size as usize {
                    return Some(s.raw_ptr as usize + delta);
                }
                return None; // zero-fill tail of the section
            }
        }
        None
    }

    /// Which section contains the entry point (packing heuristic aid).
    pub fn entry_section(&self) -> Option<&Section> {
        self.sections
            .iter()
            .find(|s| s.contains_rva(self.entry_rva))
    }

    pub fn imports(&self, buf: &[u8]) -> Result<Vec<ImportDll>, String> {
        let dir = self.dir(DIR_IMPORT);
        if dir.rva == 0 {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        let mut off = self
            .rva_to_off(dir.rva)
            .ok_or("import directory RVA not file-mapped")?;
        loop {
            let ilt = rd32(buf, off)?;
            let name_rva = rd32(buf, off + 12)?;
            let ft = rd32(buf, off + 16)?;
            if ilt == 0 && name_rva == 0 && ft == 0 {
                break;
            }
            let dll = {
                let o = self.rva_to_off(name_rva).ok_or("dll name RVA not mapped")?;
                rd_cstr(buf, o, 256)?
            };
            let thunk_rva = if ilt != 0 { ilt } else { ft };
            let mut t = self
                .rva_to_off(thunk_rva)
                .ok_or("thunk array RVA not mapped")?;
            let mut functions = Vec::new();
            let mut idx: u32 = 0;
            loop {
                let v = rd32(buf, t)?;
                if v == 0 {
                    break;
                }
                let iat_rva = ft.checked_add(idx * 4).ok_or("IAT RVA overflow")?;
                if v & 0x8000_0000 != 0 {
                    functions.push(ImportFunc {
                        iat_rva,
                        name: None,
                        ordinal: Some((v & 0xffff) as u16),
                    });
                } else {
                    let noff = self.rva_to_off(v).ok_or("import name RVA not mapped")?;
                    let _hint = rd16(buf, noff)?;
                    functions.push(ImportFunc {
                        iat_rva,
                        name: Some(rd_cstr(buf, noff + 2, 512)?),
                        ordinal: None,
                    });
                }
                t += 4;
                idx += 1;
                if idx > 0x10000 {
                    return Err("import thunk array runaway".into());
                }
            }
            out.push(ImportDll { dll, functions });
            off += 20;
            if out.len() > 512 {
                return Err("import descriptor runaway".into());
            }
        }
        Ok(out)
    }
}

fn rd16(b: &[u8], o: usize) -> Result<u16, String> {
    b.get(o..o + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .ok_or_else(|| format!("u16 read at file offset {o:#x} out of bounds"))
}

fn rd32(b: &[u8], o: usize) -> Result<u32, String> {
    b.get(o..o + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| format!("u32 read at file offset {o:#x} out of bounds"))
}

fn rd_cstr(b: &[u8], o: usize, max: usize) -> Result<String, String> {
    let mut s = Vec::new();
    for i in 0..max {
        let c = *b
            .get(o + i)
            .ok_or_else(|| "cstr read out of bounds".to_string())?;
        if c == 0 {
            break;
        }
        s.push(c);
    }
    Ok(String::from_utf8_lossy(&s).into_owned())
}
