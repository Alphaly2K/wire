//! Win64 keyboard adapters keep full-width HKLs and the existing input owner.
use super::{Memory, Runtime64};
use crate::win32::{
    catalog::KeyboardApi,
    user::keyboard_layout::{character, key_name, map_key, HKL},
};

impl Runtime64 {
    pub(super) fn keyboard_call(
        &mut self,
        mem: &mut Memory,
        id: KeyboardApi,
        a: &[u64],
    ) -> Result<u64, String> {
        use KeyboardApi::*;
        match id {
            AsyncState => Ok(self.user.keyboard.asynchronous(a[0] as u32) as u64),
            State => {
                let keys = self.user.keyboard.state64(self.threads.active);
                let key = keys.get(a[0] as usize).copied().unwrap_or(0);
                Ok((((key as u16 & 0x80) << 8) | (key as u16 & 1)) as i16 as i32 as u32 as u64)
            }
            StateArray => {
                mem.write(a[0], &self.user.keyboard.state64(self.threads.active))?;
                Ok(1)
            }
            Layout => {
                let tid = if a[0] == 0 { self.threads.active } else { a[0] };
                Ok(
                    if self
                        .threads
                        .entries
                        .get(&tid)
                        .is_some_and(|t| t.exit.is_none())
                    {
                        HKL
                    } else {
                        0
                    },
                )
            }
            LayoutList => {
                if (a[0] as u32 as i32) < 0 {
                    self.set_error(mem, 87)?;
                    return Ok(0);
                }
                if a[0] != 0 && a[1] != 0 {
                    mem.write_int(a[1], 8, HKL)?;
                }
                Ok(1)
            }
            LayoutNameA | LayoutNameW => {
                if id == LayoutNameW {
                    mem.write(
                        a[0],
                        &"00000409\0"
                            .encode_utf16()
                            .flat_map(u16::to_le_bytes)
                            .collect::<Vec<_>>(),
                    )?;
                } else {
                    mem.write(a[0], b"00000409\0")?;
                }
                Ok(1)
            }
            Type => Ok(match a[0] {
                0 => 4,
                1 => 0,
                2 => 12,
                _ => 0,
            }),
            MapA | MapW | MapExA | MapExW => {
                if matches!(id, MapExA | MapExW) && a[2] != 0 && a[2] != HKL {
                    self.set_error(mem, 87)?;
                    return Ok(0);
                }
                Ok(map_key(a[0] as u32, a[1] as u32) as u64)
            }
            Unicode | UnicodeEx => {
                if a[4] as u32 as i32 <= 0 {
                    return Ok(0);
                }
                if a[5] & !7 != 0 || (id == UnicodeEx && a[6] != 0 && a[6] != HKL) {
                    self.set_error(mem, 87)?;
                    return Ok(0);
                }
                let mut state = [0; 256];
                mem.read(a[2], &mut state)?;
                let text = if a[1] & 0x8000 != 0 && a[5] & 2 == 0 {
                    None
                } else {
                    character(a[0] as u32, &state)
                };
                let mut out = text.unwrap_or(0).to_le_bytes().to_vec();
                if a[4] > 1 {
                    out.extend_from_slice(&[0, 0]);
                }
                mem.write(a[3], &out)?;
                Ok(text.is_some() as u64)
            }
            KeyNameA | KeyNameW => {
                if a[2] as u32 as i32 <= 0 || a[1] == 0 {
                    return Ok(0);
                }
                let scan =
                    ((a[0] >> 16) & 255) as u32 | if a[0] & (1 << 24) != 0 { 0xe000 } else { 0 };
                let text = key_name(scan, a[0] & (1 << 25) != 0);
                let n = text.len().min(a[2] as usize - 1);
                let mut out = text.as_bytes()[..n].to_vec();
                out.push(0);
                if id == KeyNameW {
                    out = out
                        .into_iter()
                        .flat_map(|c| (c as u16).to_le_bytes())
                        .collect();
                }
                mem.write(a[1], &out)?;
                Ok(n as u64)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::x64::memory::{READ, WRITE};
    #[test]
    fn unicode_query_does_not_consume_host_resolved_text_or_change_queued_keys() {
        let mut m = Memory::default();
        const DATA: u64 = 0x700000000;
        m.map(DATA, 8192, READ | WRITE).unwrap();
        let mut r = Runtime64::new(&mut m, 0x140000000, DATA, 8192, "probe.exe").unwrap();
        let message = [0x7ffd00630000, 0x100, 65, 1, 123];
        assert!(r.user.keyboard.host64(message));
        assert!(!r.user.keyboard.host64([message[0], 0x102, 0x3042, 1, 123]));
        r.user.keyboard.consumed64(1, 0x100, 65, 1);
        let before = r.user.keyboard.state64(1);
        assert_eq!(
            r.keyboard_call(
                &mut m,
                KeyboardApi::Unicode,
                &[65, 0x1e, DATA, DATA + 512, 2, 0]
            )
            .unwrap(),
            1
        );
        assert_eq!(m.read_int(DATA + 512, 2).unwrap(), 97);
        assert_eq!(r.user.keyboard.state64(1), before);
        assert_eq!(r.user.keyboard.translate64(message), vec![0x3042]);
    }
}
