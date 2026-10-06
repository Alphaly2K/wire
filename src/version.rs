//! PE32 version resources and checked VS_VERSION_INFO traversal.
use crate::{
    api::{ApiState, Args},
    mem::GuestMem,
    pe::{PeFile, DIR_RESOURCE},
};
fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}
fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}
fn resource(bytes: &[u8]) -> Option<Vec<u8>> {
    let pe = PeFile::parse(bytes).ok()?;
    let offset = |rva: u32, len: u32| -> Option<usize> {
        if rva < pe.size_of_headers && rva.checked_add(len)? <= pe.size_of_headers {
            return (rva as usize + len as usize <= bytes.len()).then_some(rva as usize);
        }
        for s in &pe.sections {
            let n = rva.checked_sub(s.virtual_address);
            if let Some(n) = n.filter(|&n| n.checked_add(len).is_some_and(|v| v <= s.raw_size)) {
                let p = s.raw_ptr.checked_add(n)? as usize;
                return (p.checked_add(len as usize)? <= bytes.len()).then_some(p);
            }
        }
        None
    };
    let dir = pe.dir(DIR_RESOURCE);
    let root = offset(dir.rva, dir.size)?;
    let data = bytes.get(root..root + dir.size as usize)?;
    let entry = |at: usize, id: Option<u32>| -> Option<u32> {
        let count = u16_at(data, at + 12)? as usize + u16_at(data, at + 14)? as usize;
        data.get(at..at + 16 + count * 8)?;
        for i in 0..count {
            let p = at + 16 + i * 8;
            if id.is_none_or(|id| u32_at(data, p) == Some(id)) {
                return u32_at(data, p + 4);
            }
        }
        None
    };
    let t = entry(0, Some(16))?;
    if t & 0x80000000 == 0 {
        return None;
    }
    let n = entry((t & 0x7fffffff) as usize, None)?;
    if n & 0x80000000 == 0 {
        return None;
    }
    let l = entry((n & 0x7fffffff) as usize, None)?;
    if l & 0x80000000 != 0 {
        return None;
    }
    let at = l as usize;
    let size = u32_at(data, at + 4)?;
    if !(6..=1 << 20).contains(&size) {
        return None;
    }
    let p = offset(u32_at(data, at)?, size)?;
    let blob = bytes.get(p..p + size as usize)?;
    let length = u16_at(blob, 0)? as usize;
    (length >= 6 && length <= blob.len()).then(|| blob[..length].to_vec())
}
struct Node {
    end: usize,
    value: usize,
    length: usize,
    text: bool,
    children: usize,
    key: String,
}
fn node(b: &[u8], p: usize, end: usize) -> Option<Node> {
    let len = u16_at(b, p)? as usize;
    let stop = p.checked_add(len)?;
    if len < 6 || stop > end || stop > b.len() {
        return None;
    }
    let length = u16_at(b, p + 2)? as usize;
    let text = u16_at(b, p + 4)? == 1;
    let mut chars = Vec::new();
    let mut at = p + 6;
    loop {
        if at + 2 > stop {
            return None;
        }
        let c = u16_at(b, at)?;
        at += 2;
        if c == 0 {
            break;
        }
        chars.push(c);
    }
    let value = (at + 3) & !3;
    let value_end = value.checked_add(length.checked_mul(if text { 2 } else { 1 })?)?;
    if value_end > stop {
        return None;
    }
    Some(Node {
        end: stop,
        value,
        length,
        text,
        children: (value_end + 3) & !3,
        key: String::from_utf16(&chars).ok()?,
    })
}
pub(crate) fn dispatch(
    name: &str,
    a: &Args,
    mem: &GuestMem,
    state: &mut ApiState,
) -> Option<(u32, String)> {
    if !matches!(
        name,
        "GetFileVersionInfoA"
            | "GetFileVersionInfoW"
            | "GetFileVersionInfoSizeA"
            | "GetFileVersionInfoSizeW"
            | "VerQueryValueA"
            | "VerQueryValueW"
    ) {
        return None;
    }
    let result = (|| -> Result<u32, u32> {
        let wide = name.ends_with('W');
        if name.starts_with("GetFileVersionInfo") {
            let filename = crate::file_api::read_name(mem, a[0], wide)?;
            let path = state.fs.as_ref().ok_or(3u32)?.resolve(&filename)?;
            let bytes = std::fs::read(path).map_err(|e| crate::fs::win_error(&e))?;
            let blob = resource(&bytes).ok_or(1813u32)?;
            let size = (blob.len() * 2 + 4) as u32;
            if name.contains("Size") {
                if a[1] != 0 {
                    mem.write_u32(a[1], 0).map_err(|_| 87u32)?;
                }
                return Ok(size);
            }
            if a[2] < blob.len() as u32 {
                return Err(122);
            }
            let mut buffer = vec![0; a[2].min(size) as usize];
            buffer[..blob.len()].copy_from_slice(&blob);
            mem.write_bytes(a[3], &buffer).map_err(|_| 87u32)?;
            return Ok(1);
        }
        mem.ptr(a[2], 4, true).map_err(|_| 87u32)?;
        mem.ptr(a[3], 4, true).map_err(|_| 87u32)?;
        mem.write_u32(a[3], 0).map_err(|_| 87u32)?;
        let size = mem.read_u16(a[0]).map_err(|_| 87u32)? as usize;
        let mut blob = vec![0; size];
        mem.read_bytes(a[0], &mut blob).map_err(|_| 87u32)?;
        let path = crate::file_api::read_name(mem, a[1], wide)?;
        let mut current = node(&blob, 0, size).ok_or(13u32)?;
        if current.key != "VS_VERSION_INFO" {
            return Err(13);
        }
        for key in path.split('\\').filter(|s| !s.is_empty()) {
            let mut at = current.children;
            let mut found = None;
            while at < current.end {
                let child = node(&blob, at, current.end).ok_or(13u32)?;
                let next = (child.end + 3) & !3;
                if child.key.eq_ignore_ascii_case(key) {
                    found = Some(child);
                    break;
                }
                at = next;
            }
            current = found.ok_or(1813u32)?;
        }
        let mut p = a[0].checked_add(current.value as u32).ok_or(87u32)?;
        let mut len = current.length as u32;
        if !wide && current.text && len != 0 {
            let units = blob[current.value..current.value + current.length * 2]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes(*b))
                .collect::<Vec<_>>();
            let bytes = crate::codepage::encode(&String::from_utf16_lossy(&units), 932)?.0;
            let offset = size + 4 + current.value;
            if offset + bytes.len() > size * 2 + 4 {
                return Err(13);
            }
            p = a[0].checked_add(offset as u32).ok_or(87u32)?;
            mem.write_bytes(p, &bytes).map_err(|_| 87u32)?;
            len = bytes.len() as u32;
        }
        mem.write_u32(a[2], p).map_err(|_| 87u32)?;
        mem.write_u32(a[3], len).map_err(|_| 87u32)?;
        Ok(1)
    })();
    Some(match result {
        Ok(n) => (n, "PE version resource".into()),
        Err(e) => {
            state.last_error = e;
            (0, format!("version resource error {e}"))
        }
    })
}
