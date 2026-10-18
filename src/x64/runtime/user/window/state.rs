//! Win64 scalar titles and ownership, with their original validation order.
use super::{Memory, Runtime64};
use crate::win32::catalog::WindowStateApi as S;

impl Runtime64 {
    pub(in crate::x64::runtime) fn window_state_call(
        &mut self,
        mem: &mut Memory,
        id: S,
        args: &[u64],
    ) -> Result<u64, String> {
        match id {
            S::Focus => return Ok(self.user.focus),
            S::Active => return Ok(self.user.active),
            S::Foreground => return Ok(self.user.foreground),
            _ => {}
        }
        let Some(w) = self.user.windows.get_mut(&args[0]) else {
            self.set_error(mem, 1400)?;
            return Ok(0);
        };
        match id {
            S::SetTextA | S::SetTextW => {
                let title = if args[1] == 0 {
                    String::new()
                } else {
                    Self::path_name(mem, args[1], id.wide())?
                };
                w.title = title;
                Ok(1)
            }
            S::TextLengthA | S::TextLengthW => Ok(if id.wide() {
                w.title.encode_utf16().count()
            } else {
                crate::codepage::encode(&w.title, 932)
                    .map_err(|e| format!("window title: {e}"))?
                    .0
                    .len()
            } as u64),
            S::TextA | S::TextW => {
                if args[2] == 0 {
                    return Ok(0);
                }
                let wide = id.wide();
                let mut bytes = if wide {
                    w.title
                        .encode_utf16()
                        .flat_map(u16::to_le_bytes)
                        .collect::<Vec<_>>()
                } else {
                    crate::codepage::encode(&w.title, 932)
                        .map_err(|e| format!("window title: {e}"))?
                        .0
                };
                let width = if wide { 2 } else { 1 };
                bytes.truncate(bytes.len().min((args[2] as usize - 1) * width));
                let length = bytes.len() / width;
                bytes.extend(vec![0; width]);
                mem.write(args[1], &bytes)?;
                Ok(length as u64)
            }
            S::SetFocus => {
                let previous = self.user.focus;
                self.user.focus = args[0];
                Ok(previous)
            }
            S::SetForeground => {
                self.user.foreground = args[0];
                self.user.active = args[0];
                Ok(1)
            }
            _ => unreachable!("Win64 scalar window-state contract"),
        }
    }
}
