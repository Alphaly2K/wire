//! Fixed class-brush colors shared by the default USER adapters.
pub(crate) fn system(index: u32) -> u32 {
    match index {
        5 => 0xffffff,
        8 | 18 => 0,
        15 => 0xf0f0f0,
        _ => 0xc0c0c0,
    }
}
