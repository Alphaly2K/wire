//! Single virtual desktop profile, shared with USER32 metrics and CPU D3D9.
use crate::{
    api::{ApiState, Args},
    mem::GuestMem,
};
pub(crate) fn dispatch(
    name: &str,
    a: &Args,
    mem: &GuestMem,
    state: &mut ApiState,
) -> Option<(u32, String)> {
    if !matches!(
        name,
        "EnumDisplayDevicesA"
            | "EnumDisplayDevicesW"
            | "EnumDisplaySettingsA"
            | "EnumDisplaySettingsW"
            | "EnumDisplaySettingsExA"
            | "EnumDisplaySettingsExW"
    ) {
        return None;
    }
    let result = (|| -> Result<u32, u32> {
        let wide = name.ends_with('W');
        let device = if a[0] == 0 {
            String::new()
        } else {
            crate::file_api::read_name(mem, a[0], wide)?
        };
        if name.starts_with("EnumDisplayDevices") {
            let size = if wide { 840 } else { 424 };
            if mem.read_u32(a[2]).map_err(|_| 87u32)? != size {
                return Err(87);
            }
            if a[3] & !1 != 0 {
                return Err(87);
            }
            if !device.is_empty() || a[1] != 0 {
                return Ok(0);
            }
            let mut b = vec![0; size as usize];
            b[..4].copy_from_slice(&size.to_le_bytes());
            let write = |b: &mut [u8], at: usize, text: &str| {
                if wide {
                    let data = text
                        .encode_utf16()
                        .flat_map(u16::to_le_bytes)
                        .collect::<Vec<_>>();
                    b[at..at + data.len()].copy_from_slice(&data);
                } else {
                    b[at..at + text.len()].copy_from_slice(text.as_bytes());
                }
            };
            write(&mut b, 4, "\\\\.\\DISPLAY1");
            write(&mut b, if wide { 68 } else { 36 }, "Wire CPU Display");
            let flags = if wide { 324 } else { 164 };
            b[flags..flags + 4].copy_from_slice(&5u32.to_le_bytes());
            mem.write_bytes(a[2], &b).map_err(|_| 87u32)?;
            return Ok(1);
        }
        if !device.is_empty() && !device.eq_ignore_ascii_case("\\\\.\\DISPLAY1") {
            return Ok(0);
        }
        if !matches!(a[1], 0 | 0xffffffff | 0xfffffffe) {
            return Ok(0);
        }
        if name.contains("Ex") && a[3] & !3 != 0 {
            return Err(87);
        }
        let total = if wide { 220 } else { 156 };
        let dm_size = if wide { 68 } else { 36 };
        let size = mem
            .read_u16(a[2].checked_add(dm_size).ok_or(87u32)?)
            .map_err(|_| 87u32)? as usize;
        if size < if wide { 188 } else { 124 } || size > total {
            return Err(87);
        }
        let mut b = vec![0; size];
        let data = if wide {
            "\\\\.\\DISPLAY1"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        } else {
            b"\\\\.\\DISPLAY1".to_vec()
        };
        b[..data.len()].copy_from_slice(&data);
        b[dm_size as usize - 4..dm_size as usize - 2].copy_from_slice(&0x401u16.to_le_bytes());
        b[dm_size as usize..dm_size as usize + 2].copy_from_slice(&(size as u16).to_le_bytes());
        let fields = if wide { 72 } else { 40 };
        b[fields..fields + 4].copy_from_slice(&0x005c0020u32.to_le_bytes());
        let bits = if wide { 168 } else { 104 };
        for (i, n) in [32u32, 1024, 768, 0, 60].into_iter().enumerate() {
            b[bits + i * 4..bits + i * 4 + 4].copy_from_slice(&n.to_le_bytes());
        }
        mem.write_bytes(a[2], &b).map_err(|_| 87u32)?;
        Ok(1)
    })();
    Some(match result {
        Ok(n) => (n, "single 1024x768 CPU display".into()),
        Err(e) => {
            state.last_error = e;
            (0, format!("display error {e}"))
        }
    })
}
