//! Read PE resources directly from the mapped guest image.
use crate::mem::GuestMem;

pub enum Key {
    Id(u32),
    Name(String),
}

fn entry(mem: &GuestMem, root: u32, size: u32, dir: u32, key: Option<&Key>) -> Option<u32> {
    if dir.checked_add(16)? > size {
        return None;
    }
    let p = root.checked_add(dir)?;
    let count = mem.read_u16(p + 12).ok()? as u32 + mem.read_u16(p + 14).ok()? as u32;
    if dir.checked_add(16 + count.checked_mul(8)?)? > size {
        return None;
    }
    for i in 0..count {
        let e = p + 16 + i * 8;
        let name = mem.read_u32(e).ok()?;
        let matched = match key {
            None => true,
            Some(Key::Id(id)) => *id == name,
            Some(Key::Name(text)) if name & 0x8000_0000 != 0 => {
                let offset = name & 0x7fff_ffff;
                if offset.checked_add(2)? > size {
                    return None;
                }
                let len = mem.read_u16(root + offset).ok()? as u32;
                if offset.checked_add(2 + len * 2)? > size {
                    return None;
                }
                let mut chars = Vec::new();
                for j in 0..len {
                    chars.push(mem.read_u16(root + offset + 2 + j * 2).ok()?);
                }
                String::from_utf16_lossy(&chars).eq_ignore_ascii_case(text)
            }
            _ => false,
        };
        if matched {
            return mem.read_u32(e + 4).ok();
        }
    }
    None
}

/// Return the guest address of an IMAGE_RESOURCE_DATA_ENTRY (an HRSRC).
pub fn find(mem: &GuestMem, base: u32, typ: u32, id: u32, lang: Option<u32>) -> Option<u32> {
    find_key(mem, base, &Key::Id(typ), &Key::Id(id), lang)
}

pub fn find_key(mem: &GuestMem, base: u32, typ: &Key, id: &Key, lang: Option<u32>) -> Option<u32> {
    let nt = base.checked_add(mem.read_u32(base + 0x3c).ok()?)?;
    if mem.read_u32(nt).ok()? != 0x4550 {
        return None;
    }
    let rva = mem.read_u32(nt + 24 + 112).ok()?;
    let size = mem.read_u32(nt + 24 + 116).ok()?;
    if rva == 0 {
        return None;
    }
    let root = base.checked_add(rva)?;
    let t = entry(mem, root, size, 0, Some(typ))?;
    if t & 0x8000_0000 == 0 {
        return None;
    }
    let n = entry(mem, root, size, t & 0x7fff_ffff, Some(id))?;
    if n & 0x8000_0000 == 0 {
        return None;
    }
    let language = lang.map(Key::Id);
    let l = entry(mem, root, size, n & 0x7fff_ffff, language.as_ref())
        .or_else(|| entry(mem, root, size, n & 0x7fff_ffff, None))?;
    if l & 0x8000_0000 != 0 || l.checked_add(16)? > size {
        return None;
    }
    root.checked_add(l)
}

pub fn data(mem: &GuestMem, base: u32, resource: u32) -> Option<(u32, u32)> {
    let p = base.checked_add(mem.read_u32(resource).ok()?)?;
    let size = mem.read_u32(resource + 4).ok()?;
    p.checked_add(size)?;
    Some((p, size))
}

/// RT_STRING blocks contain sixteen length-prefixed UTF-16 strings.
pub fn string(mem: &GuestMem, base: u32, id: u32) -> Option<(u32, u32)> {
    let res = find(mem, base, 6, (id >> 4) + 1, None)?;
    let (mut p, size) = data(mem, base, res)?;
    let end = p.checked_add(size)?;
    for i in 0..16 {
        if p.checked_add(2)? > end {
            return None;
        }
        let len = mem.read_u16(p).ok()? as u32;
        p += 2;
        if p.checked_add(len * 2)? > end {
            return None;
        }
        if i == id & 15 {
            return Some((p, len));
        }
        p += len * 2;
    }
    None
}
