//! Borrowed portable draw descriptions consumed by each graphics backend.
use crate::gpu::{Buffer, BufferView, ImageView, Shader};
/// Cache the same non-builtin input locations used by native pipeline validation.
/// This lets a new VS ignore unused elements in a previously bound input layout.
pub(crate) fn input_locations(words: &[u32]) -> Result<std::collections::BTreeSet<u32>, String> {
    if words.len() < 5 || words[0] != 0x07230203 {
        return Err("invalid shader input reflection header".into());
    }
    let mut locations = std::collections::BTreeMap::new();
    let mut inputs = Vec::new();
    let mut at = 5;
    while at < words.len() {
        let count = (words[at] >> 16) as usize;
        let opcode = words[at] & 65535;
        if count == 0 || count > words.len() - at {
            return Err("shader input reflection instruction bounds".into());
        }
        let w = &words[at..at + count];
        if opcode == 71 && count == 4 && w[2] == 30 {
            if locations.insert(w[1], w[3]).is_some() {
                return Err("duplicate shader input location decoration".into());
            }
        } else if opcode == 59 && count >= 4 && w[3] == 1 {
            inputs.push(w[2]);
        }
        at += count;
    }
    Ok(inputs
        .into_iter()
        .filter_map(|id| locations.get(&id).copied())
        .collect())
}
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct Attribute {
    pub location: u32,
    pub format: u32,
    pub slot: u32,
    pub offset: u32,
    pub input_class: u32,
    pub step: u32,
}
pub(crate) struct Vertex<'a> {
    pub slot: usize,
    pub buffer: &'a Buffer,
    pub stride: u32,
    pub offset: u32,
}
pub(crate) enum Resource<'a> {
    Constant(&'a Buffer),
    Image(&'a ImageView),
    Buffer(&'a BufferView),
    Sampler([u32; 13]),
}
pub(crate) struct Descriptor<'a> {
    pub set: u32,
    pub binding: u32,
    pub resource: Resource<'a>,
}
pub(crate) struct Draw<'a> {
    pub vertex: &'a Shader,
    pub pixel: &'a Shader,
    pub target: &'a ImageView,
    pub depth: Option<&'a ImageView>,
    pub index: Option<&'a Buffer>,
    pub vertices: Vec<Vertex<'a>>,
    pub attributes: Vec<Attribute>,
    pub descriptors: Vec<Descriptor<'a>>,
    pub topology: u32,
    pub index_format: u32,
    pub index_offset: u32,
    pub indexed: bool,
    pub count: u32,
    pub first: u32,
    pub base_vertex: i32,
    pub instances: u32,
    pub first_instance: u32,
    pub viewport: [f32; 6],
    pub scissor: [i32; 4],
    pub blend: [u32; 66],
    pub depth_state: [u32; 13],
    pub raster: [u32; 10],
    pub blend_factor: [f32; 4],
    pub sample_mask: u32,
    pub stencil_ref: u32,
}
