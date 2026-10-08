//! Borrowed, typed host draw descriptions. Raw pointers never cross guest ABI.
use super::*;
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
#[repr(C)]
#[derive(Clone, Copy)]
struct RawVertex {
    buffer: *mut c_void,
    stride: u32,
    offset: u32,
}
#[repr(C)]
struct RawDescriptor {
    set: u32,
    binding: u32,
    kind: u32,
    reserved: u32,
    resource: *mut c_void,
    sampler: [u32; 13],
}
#[repr(C)]
pub(super) struct RawDraw {
    vertex: *mut c_void,
    pixel: *mut c_void,
    target: *mut c_void,
    depth: *mut c_void,
    index: *mut c_void,
    vertices: [RawVertex; 32],
    attributes: *const Attribute,
    attribute_count: usize,
    bindings: *const RawDescriptor,
    binding_count: usize,
    topology: u32,
    index_format: u32,
    index_offset: u32,
    indexed: u32,
    count: u32,
    first: u32,
    base_vertex: i32,
    instances: u32,
    first_instance: u32,
    viewport: [f32; 6],
    scissor: [i32; 4],
    blend: [u32; 66],
    depth_state: [u32; 13],
    raster: [u32; 10],
    blend_factor: [f32; 4],
    sample_mask: u32,
    stencil_ref: u32,
}
pub(super) type DrawFn =
    unsafe extern "C" fn(*mut c_void, *const RawDraw, *mut c_char, usize) -> i32;
const _: () = assert!(std::mem::size_of::<RawDraw>() == 1040);
const _: () = assert!(std::mem::offset_of!(RawDraw, viewport) == 620);
const _: () = assert!(std::mem::size_of::<RawDescriptor>() == 80);
impl NativeGraphics {
    pub(crate) fn draw(&self, d: &Draw<'_>) -> Result<(), String> {
        let same = |graphics: &NativeGraphics| -> Result<(), String> {
            if Rc::ptr_eq(&self.0, &graphics.0) {
                Ok(())
            } else {
                Err("draw resource belongs to another GPU".into())
            }
        };
        same(&d.vertex.graphics)?;
        same(&d.pixel.graphics)?;
        same(&d.target.texture.0.graphics)?;
        if let Some(v) = d.depth {
            same(&v.texture.0.graphics)?;
        }
        if let Some(b) = d.index {
            same(&b.0.graphics)?;
        }
        let mut vertices = [RawVertex {
            buffer: std::ptr::null_mut(),
            stride: 0,
            offset: 0,
        }; 32];
        let mut slots = std::collections::BTreeSet::new();
        for v in &d.vertices {
            same(&v.buffer.0.graphics)?;
            if v.slot >= 32 || !slots.insert(v.slot) {
                return Err("draw vertex slot range/duplicate".into());
            }
            vertices[v.slot] = RawVertex {
                buffer: v.buffer.0.handle.as_ptr(),
                stride: v.stride,
                offset: v.offset,
            };
        }
        let mut descriptors = Vec::new();
        for b in &d.descriptors {
            let (kind, resource, sampler) = match &b.resource {
                Resource::Constant(v) => {
                    same(&v.0.graphics)?;
                    (0, v.0.handle.as_ptr(), [0; 13])
                }
                Resource::Image(v) => {
                    same(&v.texture.0.graphics)?;
                    (1, v.handle.as_ptr(), [0; 13])
                }
                Resource::Buffer(v) => {
                    same(&v.buffer.0.graphics)?;
                    (2, v.handle.as_ptr(), [0; 13])
                }
                Resource::Sampler(s) => (3, std::ptr::null_mut(), *s),
            };
            descriptors.push(RawDescriptor {
                set: b.set,
                binding: b.binding,
                kind,
                reserved: 0,
                resource,
                sampler,
            });
        }
        let raw = RawDraw {
            vertex: d.vertex.handle.as_ptr(),
            pixel: d.pixel.handle.as_ptr(),
            target: d.target.handle.as_ptr(),
            depth: d.depth.map_or(std::ptr::null_mut(), |v| v.handle.as_ptr()),
            index: d
                .index
                .map_or(std::ptr::null_mut(), |v| v.0.handle.as_ptr()),
            vertices,
            attributes: d.attributes.as_ptr(),
            attribute_count: d.attributes.len(),
            bindings: descriptors.as_ptr(),
            binding_count: descriptors.len(),
            topology: d.topology,
            index_format: d.index_format,
            index_offset: d.index_offset,
            indexed: d.indexed as u32,
            count: d.count,
            first: d.first,
            base_vertex: d.base_vertex,
            instances: d.instances,
            first_instance: d.first_instance,
            viewport: d.viewport,
            scissor: d.scissor,
            blend: d.blend,
            depth_state: d.depth_state,
            raster: d.raster,
            blend_factor: d.blend_factor,
            sample_mask: d.sample_mask,
            stencil_ref: d.stencil_ref,
        };
        let mut error = [0u8; 4096];
        if unsafe {
            (self.0.draw)(
                self.0.context.as_ptr(),
                &raw,
                error.as_mut_ptr().cast(),
                error.len(),
            )
        } == 0
        {
            Err(message(&error))
        } else {
            Ok(())
        }
    }
}
