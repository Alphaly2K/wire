//! PE32 version resource adapter borrows memory, filesystem and LastError only.
use crate::{
    api::Args,
    fs::GuestFs,
    mem::GuestMem,
    win32::{
        catalog::VersionResourceApi,
        kernel::version_resources::{node, resource},
    },
};

pub(crate) fn call(
    id: VersionResourceApi,
    a: &Args,
    mem: &GuestMem,
    filesystem: Option<&GuestFs>,
    last_error: &mut u32,
) -> (u32, String) {
    let result = (|| -> Result<u32, u32> {
        let wide = id.wide();
        if id.fetch() {
            let filename = crate::file_api::read_name(mem, a[0], wide)?;
            let path = filesystem.ok_or(3u32)?.resolve(&filename)?;
            let bytes = std::fs::read(path).map_err(|e| crate::fs::win_error(&e))?;
            let blob = resource(&bytes).ok_or(1813u32)?;
            let size = (blob.len() * 2 + 4) as u32;
            if id.size() {
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
    match result {
        Ok(n) => (n, "PE version resource".into()),
        Err(e) => {
            *last_error = e;
            (0, format!("version resource error {e}"))
        }
    }
}
