//! Resource ABI handling borrows image memory and error state explicitly.
use crate::{api::Args, mem::GuestMem, win32::catalog::ResourceApi};

pub(crate) fn call(
    id: ResourceApi,
    args: &Args,
    mem: &GuestMem,
    exe_base: u32,
    last_error: &mut u32,
) -> Result<(u32, String), String> {
    let ret;
    let mut note = String::new();
    match id {
        ResourceApi::LoadStringA | ResourceApi::LoadStringW => {
            let base = if args[0] == 0 { exe_base } else { args[0] };
            let found = crate::resource::string(mem, base, args[1] & 0xffff);
            let (ptr, len) = found.unwrap_or((0, 0));
            if id == ResourceApi::LoadStringW && args[3] == 0 {
                mem.write_u32(args[2], ptr).map_err(|e| e.to_string())?;
                ret = len;
            } else if args[3] as i32 > 0 {
                let n = len.min(args[3] - 1);
                if id == ResourceApi::LoadStringW {
                    for i in 0..n {
                        mem.write_u16(
                            args[2] + i * 2,
                            mem.read_u16(ptr + i * 2).map_err(|e| e.to_string())?,
                        )
                        .map_err(|e| e.to_string())?;
                    }
                    mem.write_u16(args[2] + n * 2, 0)
                        .map_err(|e| e.to_string())?;
                    ret = n;
                } else {
                    let mut units = Vec::new();
                    for i in 0..len {
                        units.push(mem.read_u16(ptr + i * 2).map_err(|e| e.to_string())?);
                    }
                    let bytes = crate::codepage::encode(&String::from_utf16_lossy(&units), 932)
                        .map_err(|e| format!("code-page error {e}"))?
                        .0;
                    let count = bytes.len().min(args[3] as usize - 1);
                    mem.write_bytes(args[2], &bytes[..count])
                        .map_err(|e| e.to_string())?;
                    mem.write_u8(args[2] + count as u32, 0)
                        .map_err(|e| e.to_string())?;
                    ret = count as u32;
                }
            } else {
                ret = 0;
            }
            note = format!("resource string {}: {} UTF-16 units", args[1], len);
        }
        ResourceApi::FindA | ResourceApi::FindW | ResourceApi::FindExA | ResourceApi::FindExW => {
            let base = if args[0] == 0 { exe_base } else { args[0] };
            let ex = matches!(id, ResourceApi::FindExA | ResourceApi::FindExW);
            let wide = matches!(id, ResourceApi::FindW | ResourceApi::FindExW);
            let key = |p| -> Result<crate::resource::Key, String> {
                if p <= 0xffff {
                    Ok(crate::resource::Key::Id(p))
                } else {
                    Ok(crate::resource::Key::Name(
                        crate::file_api::read_name(mem, p, wide)
                            .map_err(|e| format!("resource name: {e}"))?,
                    ))
                }
            };
            let typ = key(if ex { args[1] } else { args[2] })?;
            let resource = key(if ex { args[2] } else { args[1] })?;
            ret = crate::resource::find_key(
                mem,
                base,
                &typ,
                &resource,
                if ex { Some(args[3]) } else { None },
            )
            .unwrap_or(0);
            if ret == 0 {
                *last_error = 1814;
            }
        }
        ResourceApi::Load | ResourceApi::Size => {
            let base = if args[0] == 0 { exe_base } else { args[0] };
            let data = crate::resource::data(mem, base, args[1]);
            ret = data
                .map(|(ptr, size)| if id == ResourceApi::Load { ptr } else { size })
                .unwrap_or(0);
        }
        ResourceApi::Lock => ret = args[0],
        ResourceApi::Free => ret = 0,
    }
    Ok((ret, note))
}
