//! Portable graphics descriptions. No guest, browser or native pointers.
/// Per-texture allocation/readback bound, including all mip levels and layers.
pub const TEXTURE_BYTE_LIMIT: usize = 256 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct AdapterInfo {
    pub name: String,
    pub vendor: u32,
    pub device: u32,
    pub api_version: u32,
    pub dedicated_video: u64,
    pub dedicated_system: u64,
    pub shared_system: u64,
    pub uuid: [u8; 16],
}
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Binding {
    pub set: u32,
    pub binding: u32,
    pub descriptor_type: u32,
    pub count: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TextureDesc {
    pub width: u32,
    pub height: u32,
    pub mips: u32,
    pub layers: u32,
    pub format: u32,
    pub bind: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TextureCopy {
    pub dst_sub: u32,
    pub src_sub: u32,
    pub x: u32,
    pub y: u32,
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ImageViewDesc {
    pub format: u32,
    pub mip: u32,
    pub mips: u32,
    pub layer: u32,
    pub layers: u32,
    pub kind: u32,
}
