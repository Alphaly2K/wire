//! Materialize D3D's all-remaining mip/slice sentinel before native validation.
pub(super) fn remaining_range(first: u32, count: u32, total: u32) -> Option<u32> {
    let remaining = total.checked_sub(first)?;
    let count = if count == u32::MAX { remaining } else { count };
    (count != 0 && count <= remaining).then_some(count)
}
