//! PE32 NLS ABI adapter; faults retain the legacy per-call error convention.
use crate::{
    mem::GuestMem,
    runtime::environment::GuestEnvironment,
    win32::{catalog::NlsApi, kernel::nls::ctype1},
};
pub(crate) struct Value {
    pub value: u32,
    pub note: String,
}
fn input(mem: &GuestMem, ptr: u32, len: u32, width: u32) -> Result<Vec<u8>, u32> {
    if ptr == 0 || len == 0 || (len as i32) < -1 {
        return Err(87);
    }
    let n = if len == u32::MAX {
        let mut n = 0u32;
        loop {
            let value = if width == 2 {
                mem.read_u16(ptr.checked_add(n * 2).ok_or(87u32)?)
                    .map_err(|_| 87u32)? as u32
            } else {
                mem.read_u8(ptr.checked_add(n).ok_or(87u32)?)
                    .map_err(|_| 87u32)? as u32
            };
            n += 1;
            if value == 0 {
                break n;
            }
            if n > 1 << 24 {
                return Err(87);
            }
        }
    } else {
        len
    };
    let size = n.checked_mul(width).ok_or(87u32)?;
    mem.ptr(ptr, size, false).map_err(|_| 87u32)?;
    let mut bytes = vec![0; size as usize];
    mem.read_bytes(ptr, &mut bytes).map_err(|_| 87u32)?;
    Ok(bytes)
}

pub(crate) fn call(
    id: NlsApi,
    a: &[u32; 16],
    mem: &GuestMem,
    environment: &GuestEnvironment,
    last_error: &mut u32,
) -> Result<Value, String> {
    match id {
        NlsApi::GetAcp | NlsApi::GetOemcp => {
            return Ok(Value {
                value: environment.codepage(),
                note: "fake CP932 (ja-JP)".into(),
            })
        }
        NlsApi::GetConsoleCp | NlsApi::GetConsoleOutputCp => {
            return Ok(Value {
                value: environment.codepage(),
                note: "fake CP932".into(),
            })
        }
        NlsApi::IsDbcsLeadByte | NlsApi::IsDbcsLeadByteEx => {
            let extended = id == NlsApi::IsDbcsLeadByteEx;
            let b = a[extended as usize] as u8;
            return Ok(Value {
                value: ((0x81..=0x9f).contains(&b) || (0xe0..=0xfc).contains(&b)) as u32,
                note: if extended {
                    "CP932 lead-byte ranges (codepage arg ignored)"
                } else {
                    "CP932 lead-byte ranges"
                }
                .into(),
            });
        }
        NlsApi::GetCpInfo => {
            let cp = if a[0] == 0 || a[0] == 1 { 932 } else { a[0] };
            if matches!(cp, 932 | 1252 | 65001) {
                let mut info = [0u8; 20];
                let width: u32 = match cp {
                    932 => 2,
                    65001 => 4,
                    _ => 1,
                };
                info[..4].copy_from_slice(&width.to_le_bytes());
                info[4] = b'?';
                if cp == 932 {
                    info[6..10].copy_from_slice(&[0x81, 0x9f, 0xe0, 0xfc]);
                }
                mem.write_bytes(a[1], &info).map_err(|e| e.to_string())?;
                return Ok(Value {
                    value: 1,
                    note: String::new(),
                });
            } else {
                *last_error = 87;
                return Ok(Value {
                    value: 0,
                    note: String::new(),
                });
            }
        }
        _ => {}
    }
    let result = (|| -> Result<u32, u32> {
        match id {
            NlsApi::GetStringTypeW | NlsApi::GetStringTypeExW => {
                let shift = usize::from(id == NlsApi::GetStringTypeExW);
                if a[shift] != 1 {
                    return Err(1004);
                } // CT_CTYPE1
                let bytes = input(mem, a[shift + 1], a[shift + 2], 2)?;
                let output: Vec<_> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .flat_map(|b| ctype1(u16::from_le_bytes(*b)).to_le_bytes())
                    .collect();
                mem.write_bytes(a[shift + 3], &output).map_err(|_| 87u32)?;
                Ok(1)
            }
            NlsApi::LcMapStringW | NlsApi::LcMapStringA | NlsApi::LcMapStringEx => {
                let upper = a[1] & 0x200 != 0;
                let lower = a[1] & 0x100 != 0;
                if upper == lower || a[1] & !0x0100_0300 != 0 {
                    return Err(1004);
                }
                let wide = id != NlsApi::LcMapStringA;
                let bytes = input(mem, a[2], a[3], if wide { 2 } else { 1 })?;
                let text = if wide {
                    let units: Vec<_> = bytes
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|b| u16::from_le_bytes(*b))
                        .collect();
                    String::from_utf16(&units).map_err(|_| 1113u32)?
                } else {
                    crate::codepage::decode(&bytes, 932)?
                };
                let mapped = if upper {
                    text.to_uppercase()
                } else {
                    text.to_lowercase()
                };
                let output = if wide {
                    mapped.encode_utf16().flat_map(u16::to_le_bytes).collect()
                } else {
                    crate::codepage::encode(&mapped, 932)?.0
                };
                let count = output.len() as u32 / if wide { 2 } else { 1 };
                if a[5] == 0 {
                    return Ok(count);
                }
                if a[5] < count {
                    return Err(122);
                }
                mem.write_bytes(a[4], &output).map_err(|_| 87u32)?;
                Ok(count)
            }
            NlsApi::MultiByteToWideChar => {
                let bytes = input(mem, a[2], a[3], 1)?;
                let s = if a[1] & 8 != 0 {
                    crate::codepage::decode(&bytes, a[0])?
                } else {
                    crate::codepage::decode_lossy(&bytes, a[0])?
                };
                let out: Vec<_> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
                let n = out.len() as u32 / 2;
                if a[5] == 0 {
                    return Ok(n);
                }
                if a[5] < n {
                    return Err(122);
                }
                mem.write_bytes(a[4], &out).map_err(|_| 87u32)?;
                Ok(n)
            }
            NlsApi::WideCharToMultiByte => {
                let bytes = input(mem, a[2], a[3], 2)?;
                let wide: Vec<_> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .collect();
                let s = String::from_utf16(&wide).map_err(|_| 1113u32)?;
                let (out, used) = crate::codepage::encode(&s, a[0])?;
                if a[7] != 0 {
                    mem.write_u32(a[7], used as u32).map_err(|_| 87u32)?;
                }
                let n = out.len() as u32;
                if a[5] == 0 {
                    return Ok(n);
                }
                if a[5] < n {
                    return Err(122);
                }
                mem.write_bytes(a[4], &out).map_err(|_| 87u32)?;
                Ok(n)
            }

            _ => unreachable!("ABI-only NLS call reached PE32 typed adapter"),
        }
    })();
    Ok(match result {
        Ok(value) => Value {
            value,
            note: "code-page / sandbox directory shim".into(),
        },
        Err(error) => {
            *last_error = error;
            Value {
                value: 0,
                note: format!("Win32 error {error}"),
            }
        }
    })
}
