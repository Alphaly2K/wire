//! USER owns update regions and prepares paint/DC work without executing callbacks.
use crate::{api::Args, graphics::Graphics, mem::GuestMem, win32::catalog::PaintingApi};
type ResultValue = Result<PaintRequest, String>;

// The actual RedrawWindow gateway remains MenuApi::Redraw; only its public facade uses this operation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Operation {
    Paint(PaintingApi),
    Redraw,
}

pub(crate) enum PaintRequest {
    Value(u32),
    Update,
    Begin(BeginPaint),
}
pub(crate) struct BeginPaint {
    pub procedure: u32,
    pub args: [u32; 4],
    pub erase: bool,
    area: [i32; 4],
    output: u32,
}
impl BeginPaint {
    pub(crate) fn finish(self, mem: &GuestMem, erased: bool) -> Result<u32, String> {
        let mut bytes = vec![0; 64];
        for (i, n) in [
            self.args[2],
            (!erased) as u32,
            self.area[0] as u32,
            self.area[1] as u32,
            self.area[2] as u32,
            self.area[3] as u32,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&n.to_le_bytes());
        }
        mem.write_bytes(self.output, &bytes)
            .map_err(|e| e.to_string())?;
        Ok(self.args[2])
    }
}
fn value(n: u32) -> ResultValue {
    Ok(PaintRequest::Value(n))
}
fn rect(mem: &GuestMem, p: u32) -> Result<[i32; 4], String> {
    let mut bytes = [0; 16];
    mem.read_bytes(p, &mut bytes).map_err(|e| e.to_string())?;
    Ok(std::array::from_fn(|i| {
        i32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
    }))
}

impl super::User32 {
    fn invalidate(&mut self, hwnd: u32, area: Option<[i32; 4]>, erase: bool) -> Result<(), u32> {
        let w = self.windows.get_mut(&hwnd).ok_or(1400u32)?;
        let [width, height] = w.client_size();
        let r = area.unwrap_or([0, 0, width, height]);
        let r = [r[0].max(0), r[1].max(0), r[2].min(width), r[3].min(height)];
        if r[0] >= r[2] || r[1] >= r[3] {
            return Ok(());
        }
        w.update = Some(w.update.map_or(r, |old| {
            [
                old[0].min(r[0]),
                old[1].min(r[1]),
                old[2].max(r[2]),
                old[3].max(r[3]),
            ]
        }));
        w.erase |= erase;
        Ok(())
    }
    pub(crate) fn prepare_paint(
        &mut self,
        operation: Operation,
        a: &Args,
        mem: &GuestMem,
        graphics: &mut Graphics,
        last_error: &mut u32,
    ) -> ResultValue {
        use Operation::{Paint, Redraw};
        use PaintingApi::{Begin, End, Invalidate, UpdateRect, Validate};
        if operation == Redraw && (a[2] != 0 || a[3] & !0x14f != 0) {
            *last_error = 120;
            return value(0);
        }
        if !self.windows.contains_key(&a[0]) {
            *last_error = 1400;
            return value(0);
        }
        match operation {
            Paint(Invalidate | Validate) | Redraw => {
                let area = if a[1] == 0 {
                    None
                } else {
                    Some(rect(mem, a[1])?)
                };
                if operation == Paint(Invalidate) || operation == Redraw && a[3] & 3 != 0 {
                    self.invalidate(
                        a[0],
                        area,
                        if operation == Redraw {
                            a[3] & 4 != 0
                        } else {
                            a[2] != 0
                        },
                    )
                    .map_err(|e| format!("invalidate {e}"))?;
                }
                if operation == Paint(Validate) || operation == Redraw && a[3] & 8 != 0 {
                    let w = self.windows.get_mut(&a[0]).unwrap();
                    if area.is_none()
                        || w.update.zip(area).is_some_and(|(old, r)| {
                            r[0] <= old[0] && r[1] <= old[1] && r[2] >= old[2] && r[3] >= old[3]
                        })
                    {
                        w.update = None;
                        w.erase = false;
                    }
                }
                if operation == Redraw && a[3] & 0x100 != 0 {
                    return Ok(PaintRequest::Update);
                }
                value(1)
            }
            Paint(UpdateRect) => {
                let w = &self.windows[&a[0]];
                if a[1] != 0 {
                    mem.write_bytes(
                        a[1],
                        &w.update
                            .unwrap_or([0; 4])
                            .into_iter()
                            .flat_map(i32::to_le_bytes)
                            .collect::<Vec<_>>(),
                    )
                    .map_err(|e| e.to_string())?;
                }
                value(w.update.is_some() as u32)
            }
            Paint(Begin) => {
                mem.ptr(a[1], 64, true).map_err(|e| e.to_string())?;
                let w = &self.windows[&a[0]];
                let area = w.update.unwrap_or([0; 4]);
                let erase = w.erase;
                let proc = w.wndproc;
                let [width, height] = w.client_size();
                let dc = graphics
                    .window_dc(a[0], width.max(1) as u32, height.max(1) as u32)
                    .map_err(|e| format!("paint DC {e}"))?;
                graphics
                    .set_clip(dc, area)
                    .map_err(|e| format!("paint clip {e}"))?;
                self.windows.get_mut(&a[0]).unwrap().update = None;
                self.windows.get_mut(&a[0]).unwrap().erase = false;
                Ok(PaintRequest::Begin(BeginPaint {
                    procedure: proc,
                    args: [a[0], 0x14, dc, 0],
                    erase,
                    area,
                    output: a[1],
                }))
            }
            Paint(End) => {
                let mut ps = [0; 64];
                mem.read_bytes(a[1], &mut ps).map_err(|e| e.to_string())?;
                let dc = u32::from_le_bytes(ps[..4].try_into().unwrap());
                if graphics.release_window_dc(a[0], dc).is_err() {
                    *last_error = 6;
                    return value(0);
                }
                value(1)
            }
        }
    }
}
