//! PE32 resource bytes and VS_VERSION_INFO nodes; no process or guest memory.
use crate::pe::{PeFile, DIR_RESOURCE};

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}
fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}
pub(crate) fn resource(bytes: &[u8]) -> Option<Vec<u8>> {
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
pub(crate) struct Node {
    pub(crate) end: usize,
    pub(crate) value: usize,
    pub(crate) length: usize,
    pub(crate) text: bool,
    pub(crate) children: usize,
    pub(crate) key: String,
}
pub(crate) fn node(b: &[u8], p: usize, end: usize) -> Option<Node> {
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
