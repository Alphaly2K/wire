//! Win64 packed POINT and checked RECT codecs; no process/resource state.
use crate::{
    win32::{
        catalog::GeometryApi,
        user::geometry::{self, Rect},
    },
    x64::memory::Memory,
};

pub(crate) struct GeometryResult {
    pub value: u64,
    pub error: Option<u32>,
}

pub(crate) fn read_rect(mem: &Memory, pointer: u64) -> Result<Rect, String> {
    let mut bytes = [0; 16];
    mem.read(pointer, &mut bytes)?;
    Ok(std::array::from_fn(|i| {
        i32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
    }))
}

pub(crate) fn write_rect(mem: &mut Memory, pointer: u64, rect: Rect) -> Result<(), String> {
    mem.write(
        pointer,
        &rect
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>(),
    )
}

pub(crate) fn call(mem: &mut Memory, id: GeometryApi, a: &[u64]) -> Result<GeometryResult, String> {
    use GeometryApi::*;
    if id == AdjustDpi && a[4] != 96 {
        return Ok(GeometryResult {
            value: 0,
            error: Some(50),
        });
    }
    let value = match id {
        Adjust | AdjustEx | AdjustDpi => {
            let rect = read_rect(mem, a[0])?;
            let ex = if id == Adjust { 0 } else { a[3] as u32 };
            write_rect(
                mem,
                a[0],
                geometry::adjusted(rect, a[1] as u32, ex, a[2] != 0),
            )?;
            1
        }
        Point => geometry::point(
            read_rect(mem, a[0])?,
            a[1] as u32 as i32,
            (a[1] >> 32) as u32 as i32,
        ) as u64,
        Copy => {
            let rect = read_rect(mem, a[1])?;
            write_rect(mem, a[0], rect)?;
            1
        }
        Offset => {
            let rect = read_rect(mem, a[0])?;
            write_rect(
                mem,
                a[0],
                geometry::offset(rect, a[1] as u32 as i32, a[2] as u32 as i32),
            )?;
            1
        }
        _ => unreachable!("remaining rectangle helpers have no Win64 binding"),
    };
    Ok(GeometryResult { value, error: None })
}
