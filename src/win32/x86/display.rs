//! PE32 display/monitor memory policy; callbacks remain in the process facade.
use crate::{
    api::Args,
    mem::GuestMem,
    win32::{catalog::DisplayApi, user::display},
};

pub(crate) struct DisplayResult {
    pub value: u32,
    pub note: String,
}

fn value(value: u32, note: &str) -> DisplayResult {
    DisplayResult {
        value,
        note: note.into(),
    }
}

pub(crate) fn call(
    id: DisplayApi,
    a: &Args,
    mem: &GuestMem,
    last_error: &mut u32,
) -> Result<DisplayResult, String> {
    use DisplayApi::*;
    match id {
        Metrics => Ok(value(
            display::metric(a[0]),
            "logical 1024x768 desktop / 96 DPI profile",
        )),
        FromWindow | FromPoint | FromRect => Ok(value(0x40000001, "single logical monitor")),
        InfoA | InfoW => {
            let size = mem.read_u32(a[1]).map_err(|e| e.to_string())? as usize;
            let wide = id.wide();
            if a[0] != 0x40000001 || (size != 40 && size != if wide { 104 } else { 72 }) {
                *last_error = 87;
                return Ok(value(0, "USER32 error 87"));
            }
            mem.write_bytes(a[1], &display::monitor(wide, size))
                .map_err(|e| e.to_string())?;
            Ok(value(1, "logical monitor bounds"))
        }
        Monitors | ConfigSizes | ConfigQuery | ConfigDeviceInfo => {
            unreachable!("callback control and ABI-only CCD do not enter the scalar adapter")
        }
        _ => Ok(enumerate(id, a, mem, last_error)),
    }
}

fn enumerate(id: DisplayApi, a: &Args, mem: &GuestMem, last_error: &mut u32) -> DisplayResult {
    let result = (|| -> Result<u32, u32> {
        let wide = id.wide();
        let device = if a[0] == 0 {
            String::new()
        } else {
            crate::file_api::read_name(mem, a[0], wide)?
        };
        if id.devices() {
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
            mem.write_bytes(a[2], &display::device(wide, true, "Wire CPU Display"))
                .map_err(|_| 87u32)?;
            return Ok(1);
        }
        if !device.is_empty() && !device.eq_ignore_ascii_case(display::DEVICE) {
            return Ok(0);
        }
        if !matches!(a[1], 0 | 0xffffffff | 0xfffffffe) {
            return Ok(0);
        }
        if id.extended() && a[3] & !3 != 0 {
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
        mem.write_bytes(a[2], &display::mode(wide, size))
            .map_err(|_| 87u32)?;
        Ok(1)
    })();
    match result {
        Ok(n) => value(n, "single 1024x768 CPU display"),
        Err(error) => {
            *last_error = error;
            DisplayResult {
                value: 0,
                note: format!("display error {error}"),
            }
        }
    }
}

pub(crate) enum MonitorRequest {
    Value(DisplayResult),
    Callback {
        procedure: u32,
        args: [u32; 4],
        allocation: u32,
    },
}

pub(crate) fn prepare_monitors(
    a: &Args,
    mem: &mut GuestMem,
    heap: &mut crate::heap::GuestHeap,
    process_heap: u32,
    last_error: &mut u32,
) -> Result<MonitorRequest, String> {
    if a[0] != 0 || a[2] == 0 {
        *last_error = if a[0] != 0 { 50 } else { 87 };
        return Ok(MonitorRequest::Value(value(
            0,
            "unsupported DC or invalid monitor callback",
        )));
    }
    let mut rect = [0i32, 0, 1024, 768];
    if a[1] != 0 {
        let mut bytes = [0; 16];
        mem.read_bytes(a[1], &mut bytes)
            .map_err(|e| e.to_string())?;
        let clip: [i32; 4] = std::array::from_fn(|i| {
            i32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
        });
        rect = [
            rect[0].max(clip[0]),
            rect[1].max(clip[1]),
            rect[2].min(clip[2]),
            rect[3].min(clip[3]),
        ];
    }
    if rect[0] >= rect[2] || rect[1] >= rect[3] {
        return Ok(MonitorRequest::Value(value(
            1,
            "no monitor intersects the clipping rectangle",
        )));
    }
    let p = heap
        .alloc(mem, process_heap, 16, false)
        .map_err(|e| e.to_string())?;
    mem.write_bytes(
        p,
        &rect
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .map_err(|e| e.to_string())?;
    Ok(MonitorRequest::Callback {
        procedure: a[2],
        args: [0x40000001, 0, p, a[3]],
        allocation: p,
    })
}
