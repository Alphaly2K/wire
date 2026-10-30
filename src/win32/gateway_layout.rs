//! Fixed PE32 reserved gateway layout, shared by PE32 binding and address admission.
pub const STUB_BASE: u32 = 0x6fff_0000;
pub const STUB_STRIDE: u32 = 16;
pub const STUB_CAPACITY: usize = 4096;
