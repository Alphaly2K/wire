/*
 * client_origin adapted 2026-10-06 from Hangover Wine win32u/defwnd.c, commit
 * 3ea3a14941b6a122b892c84141ce9662c008081e.
 * Copyright 1993, 1996 Alexandre Julliard
 * Copyright 1995 Alex Korobka
 * Copyright 2022 Jacek Caban for CodeWeavers
 * GNU Lesser General Public License, version 2.1 or later. No warranty.
 * Original excerpt and license: third_party/hangover/.
 */
//! PE32 scalar queries borrow USER state, checked memory and LastError only.
use crate::{api::Args, mem::GuestMem, win32::catalog::WindowQueryApi as Q};

impl super::User32 {
    pub(crate) fn client_origin(&self, mut h: u32) -> Result<[i32; 2], String> {
        let mut seen = std::collections::BTreeSet::new();
        let mut result = [0i32; 2];
        while h != 0 {
            if !seen.insert(h) {
                return Err("cyclic parent coordinates".into());
            }
            let w = self.windows.get(&h).ok_or("invalid window coordinates")?;
            let f = w.frame();
            for i in 0..2 {
                result[i] = result[i].wrapping_add(w.rect[i]).wrapping_add(f[i]);
            }
            if w.style & 0x40000000 == 0 {
                break;
            }
            h = w.parent;
        }
        Ok(result)
    }

    pub(crate) fn query_call(
        &mut self,
        id: Q,
        a: &Args,
        mem: &GuestMem,
        error: &mut u32,
    ) -> Result<(u32, String), String> {
        let value = |n, note: &str| Ok((n, note.to_owned()));
        if matches!(id, Q::FindA | Q::FindW) {
            let wide = id == Q::FindW;
            let class = if a[0] > 0xffff {
                Some(
                    crate::file_api::read_name(mem, a[0], wide)
                        .map_err(|e| format!("FindWindow class: {e}"))?
                        .to_lowercase(),
                )
            } else {
                None
            };
            let title = if a[1] != 0 {
                Some(
                    crate::file_api::read_name(mem, a[1], wide)
                        .map_err(|e| format!("FindWindow title: {e}"))?
                        .to_lowercase(),
                )
            } else {
                None
            };
            let found = self
                .windows
                .iter()
                .find_map(|(&h, w)| {
                    if w.style & 0x40000000 != 0 || w.parent == 0xfffffffd {
                        return None;
                    }
                    let c = self.classes.get(&w.class)?;
                    let matches = if a[0] == 0 {
                        true
                    } else if let Some(key) = &class {
                        c.name.to_lowercase() == *key
                    } else {
                        w.class as u32 == a[0]
                    };
                    (matches
                        && title
                            .as_ref()
                            .is_none_or(|t| String::from_utf16_lossy(&w.text).to_lowercase() == *t))
                    .then_some(h)
                })
                .unwrap_or(0);
            return value(found, "process top-level window lookup");
        }
        if id == Q::Placement {
            *error = 120;
            return value(0, "USER32 error 120");
        }
        if matches!(
            id,
            Q::IsWindow | Q::Visible | Q::Unicode | Q::Iconic | Q::Zoomed | Q::Enabled
        ) {
            let result = self.windows.get(&a[0]).is_some_and(|w| match id {
                Q::Visible => w.style & 0x10000000 != 0,
                Q::Unicode => w.wide,
                Q::Iconic => w.style & 0x20000000 != 0,
                Q::Zoomed => w.style & 0x01000000 != 0,
                Q::Enabled => w.style & 0x08000000 == 0,
                _ => true,
            });
            return value(
                result as u32,
                if matches!(id, Q::Iconic | Q::Zoomed | Q::Enabled) {
                    "logical window state"
                } else {
                    ""
                },
            );
        }
        if matches!(id, Q::ClientToScreen | Q::ScreenToClient) {
            let result = (|| -> Result<u32, u32> {
                let origin = self.client_origin(a[0]).map_err(|_| 1400u32)?;
                let mut bytes = [0; 8];
                mem.read_bytes(a[1], &mut bytes).map_err(|_| 87u32)?;
                for (i, offset) in origin.into_iter().enumerate() {
                    let n = i32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
                    let n = if id == Q::ClientToScreen {
                        n.wrapping_add(offset)
                    } else {
                        n.wrapping_sub(offset)
                    };
                    bytes[i * 4..i * 4 + 4].copy_from_slice(&n.to_le_bytes());
                }
                mem.write_bytes(a[1], &bytes).map_err(|_| 87u32)?;
                Ok(1)
            })();
            return match result {
                Ok(n) => value(n, "process-owned menu / logical client coordinates"),
                Err(e) => {
                    *error = e;
                    Ok((0, format!("menu/USER32 error {e}")))
                }
            };
        }
        if id == Q::MapPoints {
            if a[3] > 4096 {
                *error = 87;
                return value(0, "USER32 error 87");
            }
            let from = self.client_origin(a[0])?;
            let to = self.client_origin(a[1])?;
            let delta = [from[0].wrapping_sub(to[0]), from[1].wrapping_sub(to[1])];
            let mut bytes = vec![0; a[3] as usize * 8];
            mem.read_bytes(a[2], &mut bytes)
                .map_err(|e| e.to_string())?;
            for (i, chunk) in bytes.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let n = i32::from_le_bytes(*chunk).wrapping_add(delta[i % 2]);
                *chunk = n.to_le_bytes();
            }
            mem.write_bytes(a[2], &bytes).map_err(|e| e.to_string())?;
            return value(
                (delta[0] as u32 & 0xffff) | ((delta[1] as u32) << 16),
                "logical coordinate mapping",
            );
        }
        let origin = if id == Q::WindowRect {
            if let Some(w) = self
                .windows
                .get(&a[0])
                .filter(|w| w.style & 0x40000000 != 0)
            {
                self.client_origin(w.parent)?
            } else {
                [0; 2]
            }
        } else {
            [0; 2]
        };
        let Some(w) = self.windows.get_mut(&a[0]) else {
            *error = 1400;
            return value(0, "invalid HWND");
        };
        match id {
            Q::LongA | Q::LongW | Q::SetLongA | Q::SetLongW => {
                let setting = id.setting();
                if setting && a[1] == (-4i32) as u32 && a[2] == 0 {
                    *error = 87;
                    return value(0, "null WndProc");
                }
                let slot = match a[1] as i32 {
                    -4 => Some(&mut w.wndproc),
                    -6 => Some(&mut w.instance),
                    -8 => Some(&mut w.parent),
                    -12 => Some(&mut w.menu),
                    -16 => Some(&mut w.style),
                    -20 => Some(&mut w.ex_style),
                    -21 => Some(&mut w.user_data),
                    _ => None,
                };
                let old = if let Some(slot) = slot {
                    let old = *slot;
                    if setting {
                        *slot = a[2];
                    }
                    old
                } else if (a[1] as i32) >= 0
                    && (a[1] as usize)
                        .checked_add(4)
                        .is_some_and(|end| end <= w.extra.len())
                {
                    let bytes = &mut w.extra[a[1] as usize..a[1] as usize + 4];
                    let old = u32::from_le_bytes(bytes.try_into().unwrap());
                    if setting {
                        bytes.copy_from_slice(&a[2].to_le_bytes());
                    }
                    old
                } else {
                    *error = 1413;
                    return value(0, "invalid window long index");
                };
                value(old, "")
            }
            Q::Parent => value(w.parent, ""),
            Q::ThreadProcess => {
                if a[1] != 0 {
                    mem.write_u32(a[1], 4242).map_err(|e| e.to_string())?;
                }
                value(w.tid, "")
            }
            Q::ClientRect | Q::WindowRect => {
                let [x, y, width, height] = w.rect;
                let rect = if id == Q::ClientRect {
                    let size = w.client_size();
                    [0, 0, size[0], size[1]]
                } else {
                    let x = x.wrapping_add(origin[0]);
                    let y = y.wrapping_add(origin[1]);
                    [x, y, x.saturating_add(width), y.saturating_add(height)]
                };
                for (i, n) in rect.into_iter().enumerate() {
                    mem.write_u32(a[1] + i as u32 * 4, n as u32)
                        .map_err(|e| e.to_string())?;
                }
                value(1, "logical window geometry")
            }
            _ => unreachable!("PE32 scalar query only"),
        }
    }
}

// Desktop construction needs the module/class coordinator only on first use.
pub(crate) fn desktop(
    mem: &mut GuestMem,
    api: &mut crate::api::ApiTable,
    state: &mut crate::api::ApiState,
) -> Result<(u32, String), String> {
    const DESKTOP: u32 = 0x4000fffc;
    if !state.user32.windows.contains_key(&DESKTOP) {
        let module = state.module_handle_for("user32.dll");
        super::register_system_class(mem, api, state, module, "Desktop", true)?;
        let (&atom, class) = state
            .user32
            .classes
            .iter()
            .find(|(_, c)| c.name == "Desktop")
            .unwrap();
        state.user32.windows.insert(
            DESKTOP,
            super::Window {
                class: atom,
                wndproc: class.fields[1],
                instance: module,
                parent: 0,
                menu: 0,
                style: 0x10000000,
                ex_style: 0,
                rect: [0, 0, 1024, 768],
                user_data: 0,
                extra: Vec::new(),
                wide: true,
                tid: 0,
                text: Vec::new(),
                destroying: false,
                update: None,
                erase: false,
                props: Default::default(),
            },
        );
    }
    Ok((DESKTOP, "virtual desktop".into()))
}
