//! PE32 RECT copy-in/out preserves legacy notes and unsupported-DPI error 120.
use crate::{
    api::Args,
    mem::GuestMem,
    win32::{
        catalog::GeometryApi,
        user::geometry::{self, Rect},
    },
};

pub(crate) struct GeometryResult {
    pub value: u32,
    pub note: &'static str,
}

fn read(mem: &GuestMem, pointer: u32) -> Result<Rect, String> {
    let mut bytes = [0; 16];
    mem.read_bytes(pointer, &mut bytes)
        .map_err(|e| e.to_string())?;
    Ok(std::array::from_fn(|i| {
        i32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
    }))
}

fn write(mem: &GuestMem, pointer: u32, rect: Rect) -> Result<(), String> {
    mem.write_bytes(
        pointer,
        &rect
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .map_err(|e| e.to_string())
}

pub(crate) fn call(
    id: GeometryApi,
    a: &Args,
    mem: &GuestMem,
    last_error: &mut u32,
) -> Result<GeometryResult, String> {
    use GeometryApi::*;
    let value = |value, note| Ok(GeometryResult { value, note });
    if matches!(id, Adjust | AdjustEx | AdjustDpi) {
        if id == AdjustDpi && a[4] != 96 {
            *last_error = 120;
            return value(0, "96 DPI window frame geometry");
        }
        let ex = if id == Adjust { 0 } else { a[3] };
        let rect = read(mem, a[0])?;
        // The old PE32 path checks writability before computing the frame.
        mem.ptr(a[0], 16, true).map_err(|e| e.to_string())?;
        write(mem, a[0], geometry::adjusted(rect, a[1], ex, a[2] != 0))?;
        return value(1, "96 DPI window frame geometry");
    }
    if matches!(id, Set | SetEmpty) {
        let rect = if id == Set {
            [a[1] as i32, a[2] as i32, a[3] as i32, a[4] as i32]
        } else {
            [0; 4]
        };
        write(mem, a[0], rect)?;
        return value(1, "rectangle");
    }
    let source = if matches!(id, Copy | Intersect | Union) {
        a[1]
    } else {
        a[0]
    };
    let rect = read(mem, source)?;
    let (rect, result) = match id {
        Copy => (rect, true),
        Equal => return value((rect == read(mem, a[1])?) as u32, "rectangle equality"),
        IsEmpty => return value(geometry::empty(rect) as u32, "empty rectangle"),
        Point => {
            return value(
                geometry::point(rect, a[1] as i32, a[2] as i32) as u32,
                "point in rectangle",
            )
        }
        Offset => (geometry::offset(rect, a[1] as i32, a[2] as i32), true),
        Inflate => (geometry::inflate(rect, a[1] as i32, a[2] as i32), true),
        Intersect => geometry::intersection(rect, read(mem, a[2])?),
        Union => geometry::union(rect, read(mem, a[2])?),
        _ => unreachable!("set and frame adjustments returned before copy-in"),
    };
    write(mem, a[0], rect)?;
    value(result as u32, "checked rectangle output")
}
