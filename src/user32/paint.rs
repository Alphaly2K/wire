//! Window update rectangles and guest paint callbacks. CPU surfaces are the
//! pixel authority; complex regions/nonclient painting remain explicit gaps.
use crate::win32::catalog::PaintApi as P;
use crate::{
    api::{ApiLogger, ApiState, ApiTable, Args},
    cpu::{Cpu, GuestCallOutcome},
    mem::GuestMem,
    thread::Sched,
};

type ResultValue = Result<(GuestCallOutcome, String), String>;
fn value(n: u32) -> ResultValue {
    Ok((
        GuestCallOutcome::Returned(n),
        "window update/paint state".into(),
    ))
}
fn rect(mem: &GuestMem, p: u32) -> Result<[i32; 4], String> {
    let mut bytes = [0; 16];
    mem.read_bytes(p, &mut bytes).map_err(|e| e.to_string())?;
    Ok(std::array::from_fn(|i| {
        i32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
    }))
}
fn invalidate(
    state: &mut ApiState,
    hwnd: u32,
    area: Option<[i32; 4]>,
    erase: bool,
) -> Result<(), u32> {
    let w = state.user32.windows.get_mut(&hwnd).ok_or(1400u32)?;
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
#[allow(clippy::too_many_arguments)]
pub(crate) fn call(
    id: P,
    a: &Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> ResultValue {
    if id == P::Redraw && (a[2] != 0 || a[3] & !0x14f != 0) {
        state.last_error = 120;
        return value(0);
    }
    if !state.user32.windows.contains_key(&a[0]) {
        state.last_error = 1400;
        return value(0);
    }
    match id {
        P::Invalidate | P::Validate | P::Redraw => {
            let area = if a[1] == 0 {
                None
            } else {
                Some(rect(mem, a[1])?)
            };
            if id == P::Invalidate || id == P::Redraw && a[3] & 3 != 0 {
                invalidate(
                    state,
                    a[0],
                    area,
                    if id == P::Redraw {
                        a[3] & 4 != 0
                    } else {
                        a[2] != 0
                    },
                )
                .map_err(|e| format!("invalidate {e}"))?;
            }
            if id == P::Validate || id == P::Redraw && a[3] & 8 != 0 {
                let w = state.user32.windows.get_mut(&a[0]).unwrap();
                if area.is_none()
                    || w.update.zip(area).is_some_and(|(old, r)| {
                        r[0] <= old[0] && r[1] <= old[1] && r[2] >= old[2] && r[3] >= old[3]
                    })
                {
                    w.update = None;
                    w.erase = false;
                }
            }
            if id == P::Redraw && a[3] & 0x100 != 0 {
                return crate::user32::control_call(
                    crate::win32::catalog::WindowControlApi::Update,
                    a,
                    cpu,
                    mem,
                    api,
                    state,
                    log,
                    sched,
                    max_instr,
                );
            }
            value(1)
        }
        P::UpdateRect => {
            let w = &state.user32.windows[&a[0]];
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
        P::Begin => {
            mem.ptr(a[1], 64, true).map_err(|e| e.to_string())?;
            let w = &state.user32.windows[&a[0]];
            let area = w.update.unwrap_or([0; 4]);
            let erase = w.erase;
            let proc = w.wndproc;
            let [width, height] = w.client_size();
            let dc = state
                .graphics
                .window_dc(a[0], width.max(1) as u32, height.max(1) as u32)
                .map_err(|e| format!("paint DC {e}"))?;
            state
                .graphics
                .set_clip(dc, area)
                .map_err(|e| format!("paint clip {e}"))?;
            state.user32.windows.get_mut(&a[0]).unwrap().update = None;
            state.user32.windows.get_mut(&a[0]).unwrap().erase = false;
            let mut erased = !erase;
            if erase {
                let result = crate::user32::invoke(
                    cpu,
                    mem,
                    api,
                    state,
                    log,
                    sched,
                    proc,
                    &[a[0], 0x14, dc, 0],
                    max_instr,
                )?;
                match result {
                    GuestCallOutcome::Returned(n) => erased = n != 0,
                    _ => return Ok((result, "WM_ERASEBKGND transferred".into())),
                }
            }
            let mut bytes = vec![0; 64];
            for (i, n) in [
                dc,
                (!erased) as u32,
                area[0] as u32,
                area[1] as u32,
                area[2] as u32,
                area[3] as u32,
            ]
            .into_iter()
            .enumerate()
            {
                bytes[i * 4..i * 4 + 4].copy_from_slice(&n.to_le_bytes());
            }
            mem.write_bytes(a[1], &bytes).map_err(|e| e.to_string())?;
            value(dc)
        }
        P::End => {
            let mut ps = [0; 64];
            mem.read_bytes(a[1], &mut ps).map_err(|e| e.to_string())?;
            let dc = u32::from_le_bytes(ps[..4].try_into().unwrap());
            if state.graphics.release_window_dc(a[0], dc).is_err() {
                state.last_error = 6;
                return value(0);
            }
            value(1)
        }
    }
}
