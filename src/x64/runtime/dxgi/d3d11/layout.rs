//! Checked D3D11 input signatures and Win64 input element descriptions.
use super::*;
#[derive(Clone)]
struct Element {
    name: String,
    index: u32,
    format: u32,
    slot: u32,
    offset: u32,
    class: u32,
    step: u32,
}
fn u32_at(bytes: &[u8], at: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(
        bytes
            .get(at..at + 4)
            .ok_or("input signature field bounds")?
            .try_into()
            .unwrap(),
    ))
}
fn signature(blob: &[u8]) -> Result<Vec<(String, u32, u32)>, String> {
    if blob.len() < 32
        || &blob[..4] != b"DXBC"
        || u32_at(blob, 20)? != 1
        || u32_at(blob, 24)? as usize != blob.len()
    {
        return Err("invalid input signature DXBC container".into());
    }
    let count = u32_at(blob, 28)? as usize;
    if count == 0 || count > 128 || blob.len() < 32 + 4 * count {
        return Err("input signature chunk count".into());
    }
    let mut ranges = Vec::<(usize, usize)>::new();
    let mut result = None;
    for i in 0..count {
        let at = u32_at(blob, 32 + i * 4)? as usize;
        if at < 32 + count * 4 || !at.is_multiple_of(4) {
            return Err("input signature chunk offset".into());
        }
        let length = u32_at(blob, at + 4)? as usize;
        let end = at
            .checked_add(8)
            .and_then(|v| v.checked_add(length))
            .ok_or("input signature length overflow")?;
        if end > blob.len() || ranges.iter().any(|&(a, b)| at < b && end > a) {
            return Err("input signature chunk bounds/overlap".into());
        }
        ranges.push((at, end));
        let kind = &blob[at..at + 4];
        if kind != b"ISGN" && kind != b"ISG1" {
            continue;
        }
        if result.is_some() {
            return Err("duplicate input signature".into());
        }
        let b = &blob[at + 8..end];
        let n = u32_at(b, 0)? as usize;
        let extended = kind == b"ISG1";
        let size = if extended { 32 } else { 24 };
        let table_end = 8 + n
            .checked_mul(size)
            .ok_or("input signature count overflow")?;
        if n > 32 || table_end > b.len() || u32_at(b, 4)? != 8 {
            return Err("input signature element table".into());
        }
        let mut entries = Vec::new();
        let mut locations = std::collections::BTreeSet::new();
        for j in 0..n {
            let p = 8 + j * size;
            if extended && u32_at(b, p)? != 0 {
                return Err("nonzero VS input stream".into());
            }
            let p = p + if extended { 4 } else { 0 };
            let name_at = u32_at(b, p)? as usize;
            if name_at < table_end || name_at >= b.len() {
                return Err("input signature semantic offset".into());
            }
            let tail = &b[name_at..];
            let len = tail
                .iter()
                .position(|&v| v == 0)
                .ok_or("unterminated input semantic")?;
            if len == 0 || len > 256 || !tail[..len].is_ascii() {
                return Err("invalid input semantic".into());
            }
            let name = String::from_utf8(tail[..len].to_vec())
                .unwrap()
                .to_ascii_uppercase();
            let index = u32_at(b, p + 4)?;
            let system = u32_at(b, p + 8)?;
            let ty = u32_at(b, p + 12)?;
            let reg = u32_at(b, p + 16)?;
            if system != 0 {
                continue;
            } // Builtin values do not consume a vertex attribute.
            if reg >= 32
                || !(1..=3).contains(&ty)
                || b[p + 20] == 0
                || b[p + 20] & !15 != 0
                || !locations.insert(reg)
            {
                return Err("unsupported input signature register/type/packing".into());
            }
            entries.push((name, index, reg));
        }
        result = Some(entries);
    }
    result.ok_or_else(|| "DXBC input signature absent".into())
}
fn format_size(format: u32) -> Option<(u32, u32)> {
    Some(match format {
        2..=4 => (16, 4),
        6..=8 => (12, 4),
        10..=14 | 16..=18 => (8, if format < 16 { 2 } else { 4 }),
        24..=26 | 28..=32 | 34..=38 | 41..=43 => {
            (4, if (34..=38).contains(&format) { 2 } else { 4 })
        }
        49..=52 => (2, 1),
        54..=59 => (2, 2),
        61..=64 => (1, 1),
        _ => return None,
    })
}
impl Runtime64 {
    pub(super) fn create_input_layout(
        &mut self,
        mem: &mut Memory,
        a: &[u64],
    ) -> Result<u64, String> {
        if a[5] != 0 {
            mem.check(a[5], 8, WRITE)?;
        }
        // Clear HRESULT failure outputs only after all checked guest reads have
        // completed. A guest fault must leave the original output unchanged.
        let status = self.create_input_layout_checked(mem, a)?;
        if status != 0 && a[5] != 0 {
            mem.write_int(a[5], 8, 0)?;
        }
        Ok(status)
    }
    fn create_input_layout_checked(&mut self, mem: &mut Memory, a: &[u64]) -> Result<u64, String> {
        if a[2] > 32 || (a[2] != 0 && a[1] == 0) || a[3] == 0 || a[4] == 0 || a[4] > 1024 * 1024 {
            return Ok(INVALID_ARG);
        }
        let mut blob = vec![0; a[4] as usize];
        mem.read(a[3], &mut blob)?;
        let inputs = match signature(&blob) {
            Ok(s) => s,
            Err(e) => {
                self.trace_note(e);
                return Ok(INVALID_ARG);
            }
        };
        let mut elements = Vec::new();
        let mut offsets = [0u32; 32];
        let mut semantics = std::collections::BTreeSet::new();
        let mut raw = vec![0; a[2] as usize * 32];
        mem.read(a[1], &mut raw)?;
        for b in raw.as_chunks::<32>().0 {
            let pointer = u64::from_le_bytes(b[..8].try_into().unwrap());
            if pointer == 0 {
                return Ok(INVALID_ARG);
            }
            let name = Self::ascii(mem, pointer)?.to_ascii_uppercase();
            let d: Vec<_> = b[8..]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|w| u32::from_le_bytes(*w))
                .collect();
            let Some((size, alignment)) = format_size(d[1]) else {
                return Ok(INVALID_ARG);
            };
            if name.is_empty()
                || name.len() > 256
                || !name.is_ascii()
                || d[2] >= 32
                || d[4] > 1
                || (d[4] == 0 && d[5] != 0)
                || !semantics.insert((name.clone(), d[0]))
                || self
                    .graphics
                    .backend
                    .as_ref()
                    .ok_or("input layout backend missing")?
                    .format_support(d[1])
                    & 2
                    == 0
            {
                return Ok(INVALID_ARG);
            }
            let offset = if d[3] == u32::MAX {
                offsets[d[2] as usize].next_multiple_of(alignment)
            } else {
                d[3]
            };
            let Some(end) = offset.checked_add(size).filter(|&v| v <= 2048) else {
                return Ok(INVALID_ARG);
            };
            offsets[d[2] as usize] = end;
            elements.push(Element {
                name,
                index: d[0],
                format: d[1],
                slot: d[2],
                offset,
                class: d[4],
                step: d[5],
            });
        }
        let mut attributes = Vec::new();
        for (name, index, reg) in inputs {
            let Some(e) = elements.iter().find(|e| e.name == name && e.index == index) else {
                return Ok(INVALID_ARG);
            };
            attributes.extend([reg, e.format, e.slot, e.offset, e.class, e.step]);
        }
        self.trace_note(format!("D3D11 input layout attributes={attributes:x?}"));
        if a[5] == 0 {
            return Ok(1);
        }
        let p = self.graphics.object(
            mem,
            &mut self.gateways,
            &mut self.next_alloc,
            Kind::Layout,
            a[0],
        )?;
        self.graphics.resources.insert(
            p,
            Resource::State(attributes.into_iter().flat_map(u32::to_le_bytes).collect()),
        );
        mem.write_int(a[5], 8, p)?;
        Ok(0)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standalone_signature_checks_chunk_ranges_names_and_register_aliases() {
        let mut b = vec![0u8; 36 + 8 + 8 + 24 + 9];
        let len = b.len() as u32;
        b[..4].copy_from_slice(b"DXBC");
        b[20..24].copy_from_slice(&1u32.to_le_bytes());
        b[24..28].copy_from_slice(&len.to_le_bytes());
        b[28..32].copy_from_slice(&1u32.to_le_bytes());
        b[32..36].copy_from_slice(&36u32.to_le_bytes());
        b[36..40].copy_from_slice(b"ISGN");
        b[40..44].copy_from_slice(&(len - 44).to_le_bytes());
        for (at, v) in [(44, 1u32), (48, 8), (52, 32), (64, 3), (68, 7)] {
            b[at..at + 4].copy_from_slice(&v.to_le_bytes());
        }
        b[72] = 15;
        b[76..].copy_from_slice(b"POSITION\0");
        assert_eq!(signature(&b).unwrap(), vec![("POSITION".into(), 0, 7)]);
        b[52..56].copy_from_slice(&8u32.to_le_bytes());
        assert!(signature(&b).is_err());
        b[52..56].copy_from_slice(&32u32.to_le_bytes());
        b[68..72].copy_from_slice(&32u32.to_le_bytes());
        assert!(signature(&b).is_err());
    }
}
