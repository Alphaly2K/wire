//! Rectangle adapters copy all input values before publishing an output.
use super::{
    call::Call,
    catalog::GeometryApi as G,
    user::{self, geometry as values},
    ServiceError,
};
use crate::{machine::State, memory::Memory};

pub(super) fn dispatch(
    id: G,
    call: &Call,
    state: &State,
    memory: &mut Memory,
) -> Result<u64, ServiceError> {
    let a = call.arguments;
    if id == G::AdjustDpi && a[4] as u32 != 96 {
        return user::error(memory, state, 50);
    }
    if a[0] == 0
        || matches!(id, G::Copy | G::Equal | G::Intersect | G::Union) && a[1] == 0
        || matches!(id, G::Intersect | G::Union) && a[2] == 0
    {
        return Ok(u64::from(id == G::IsEmpty));
    }
    let rect = match id {
        G::Set => [
            a[1] as u32 as i32,
            a[2] as u32 as i32,
            a[3] as u32 as i32,
            a[4] as u32 as i32,
        ],
        G::SetEmpty => [0; 4],
        G::Copy | G::Intersect | G::Union => user::rect(memory, a[1])?,
        _ => user::rect(memory, a[0])?,
    };
    let (output, value) = match id {
        G::Equal => return Ok(u64::from(rect == user::rect(memory, a[1])?)),
        G::IsEmpty => return Ok(u64::from(values::empty(rect))),
        G::Point => {
            return Ok(u64::from(values::point(
                rect,
                a[1] as u32 as i32,
                (a[1] >> 32) as u32 as i32,
            )))
        }
        G::Offset => (
            values::offset(rect, a[1] as u32 as i32, a[2] as u32 as i32),
            true,
        ),
        G::Inflate => (
            values::inflate(rect, a[1] as u32 as i32, a[2] as u32 as i32),
            true,
        ),
        G::Intersect => values::intersection(rect, user::rect(memory, a[2])?),
        G::Union => values::union(rect, user::rect(memory, a[2])?),
        G::Adjust | G::AdjustEx | G::AdjustDpi => (
            values::adjusted(
                rect,
                a[1] as u32,
                if id == G::Adjust { 0 } else { a[3] as u32 },
                a[2] as u32 != 0,
            ),
            true,
        ),
        G::Set | G::SetEmpty | G::Copy => (rect, true),
    };
    user::write_rect(memory, a[0], output)?;
    Ok(u64::from(value))
}
