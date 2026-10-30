//! WebGPU resource adapter. Windows ABI/state stay in the shared DXGI layer.
//! The optional function-table bridge runs in this VM's WebContent environment.
pub(crate) use crate::gpu_types::*;
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};
#[path = "../../../src/graphics/gpu_draw.rs"]
pub(crate) mod draw;

pub(crate) const PENDING: &str = "resident WebGPU operation pending";
const CREATE_BUFFER: u32 = 1;
const WRITE_BUFFER: u32 = 2;
const READ_BUFFER: u32 = 3;
const CREATE_TEXTURE: u32 = 4;
const WRITE_TEXTURE: u32 = 5;
const READ_TEXTURE: u32 = 6;
const COPY_TEXTURE: u32 = 7;
const CREATE_IMAGE_VIEW: u32 = 8;
const CLEAR_COLOR: u32 = 9;
const CLEAR_DEPTH: u32 = 10;
const CREATE_BUFFER_VIEW: u32 = 11;
const COMPILE_SHADER: u32 = 12;
const DRAW: u32 = 13;
const PRESENT: u32 = 14;
const TIMESTAMP: u32 = 15;
const RELEASE: u32 = 16;
const PREPARE_BUFFER_READ: u32 = 17;
const PREPARE_TEXTURE_READ: u32 = 18;
const MAX_BYTES: usize = TEXTURE_BYTE_LIMIT;

pub(crate) enum BridgeError {
    Pending,
    Failed(String),
}
pub(crate) trait Dispatch {
    fn call(
        &self,
        op: u32,
        args: &[u32],
        input: &[u8],
        output: &mut [u8],
    ) -> Result<u32, BridgeError>;
}
#[cfg(target_arch = "wasm32")]
struct TableBridge(u32);
#[cfg(target_arch = "wasm32")]
impl Dispatch for TableBridge {
    fn call(
        &self,
        op: u32,
        args: &[u32],
        input: &[u8],
        output: &mut [u8],
    ) -> Result<u32, BridgeError> {
        if args.len() > 16 {
            return Err(BridgeError::Failed(
                "WebGPU command argument capacity".into(),
            ));
        }
        let mut error = [0u8; 512];
        let mut header = [0u32; 22];
        header[..6].copy_from_slice(&[
            input.as_ptr() as u32,
            input.len() as u32,
            output.as_mut_ptr() as u32,
            output.len() as u32,
            error.as_mut_ptr() as u32,
            error.len() as u32,
        ]);
        header[6..6 + args.len()].copy_from_slice(args);
        // Only a trusted same-WebContent setup export installs this typed slot.
        let function: unsafe extern "C" fn(u32, u32) -> i32 =
            unsafe { std::mem::transmute(self.0 as usize) };
        match unsafe { function(op, header.as_mut_ptr() as u32) } {
            -2 => Err(BridgeError::Pending),
            value if value < 0 => Err(BridgeError::Failed(
                String::from_utf8_lossy(
                    &error[..error.iter().position(|&v| v == 0).unwrap_or(error.len())],
                )
                .into_owned(),
            )),
            value => Ok(value as u32),
        }
    }
}
struct Device {
    bridge: Rc<dyn Dispatch>,
    info: AdapterInfo,
    formats: BTreeMap<u32, u32>,
    pending: Cell<bool>,
}
#[derive(Clone)]
pub(crate) struct Backend(Rc<Device>);
impl Backend {
    pub(crate) fn profile(bytes: &[u8]) -> Result<(AdapterInfo, BTreeMap<u32, u32>), String> {
        if bytes.len() < 72 {
            return Err("truncated WebGPU profile".into());
        }
        let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let count = word(4) as usize;
        let length = word(8) as usize;
        if word(0) != 1
            || word(12) != 0
            || word(68) != 0
            || count > 128
            || length == 0
            || length > 256
            || bytes.len() != 72 + count * 8 + length
        {
            return Err("invalid WebGPU profile layout".into());
        }
        let name = std::str::from_utf8(&bytes[72 + count * 8..])
            .map_err(|_| "invalid WebGPU adapter name")?
            .to_owned();
        if name.contains('\0') {
            return Err("invalid WebGPU adapter name".into());
        }
        let mut formats = BTreeMap::new();
        for n in 0..count {
            if formats.insert(word(72 + n * 8), word(76 + n * 8)).is_some() {
                return Err("duplicate WebGPU format profile".into());
            }
        }
        let wide = |at| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
        Ok((
            AdapterInfo {
                name,
                vendor: word(56),
                device: word(60),
                api_version: word(64),
                dedicated_video: wide(32),
                dedicated_system: wide(40),
                shared_system: wide(48),
                uuid: bytes[16..32].try_into().unwrap(),
            },
            formats,
        ))
    }
    pub(crate) fn new(
        info: AdapterInfo,
        formats: BTreeMap<u32, u32>,
        bridge: Rc<dyn Dispatch>,
    ) -> Result<Self, String> {
        if info.name.is_empty()
            || info.name.len() > 256
            || formats.len() > 128
            || info.api_version != 0
        {
            return Err("invalid WebGPU adapter profile".into());
        }
        Ok(Self(Rc::new(Device {
            bridge,
            info,
            formats,
            pending: Cell::new(false),
        })))
    }
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn from_table(
        info: AdapterInfo,
        formats: BTreeMap<u32, u32>,
        slot: u32,
    ) -> Result<Self, String> {
        if slot == 0 {
            return Err("invalid WebGPU bridge slot".into());
        }
        Self::new(info, formats, Rc::new(TableBridge(slot)))
    }
    pub(crate) fn pending(&self) -> bool {
        self.0.pending.get()
    }
    pub(crate) fn clear_pending(&self) {
        self.0.pending.set(false);
    }
    pub(crate) fn adapter(&self) -> &AdapterInfo {
        &self.0.info
    }
    fn call(&self, op: u32, args: &[u32], input: &[u8], output: &mut [u8]) -> Result<u32, String> {
        match self.0.bridge.call(op, args, input, output) {
            Ok(value) => Ok(value),
            Err(BridgeError::Pending) => {
                self.0.pending.set(true);
                Err(PENDING.into())
            }
            Err(BridgeError::Failed(message)) => Err(message),
        }
    }
    fn resource(
        &self,
        op: u32,
        args: &[u32],
        input: &[u8],
        output: &mut [u8],
    ) -> Result<Rc<Handle>, String> {
        let id = self.call(op, args, input, output)?;
        if id == 0 {
            return Err("WebGPU returned a null resource".into());
        }
        Ok(Rc::new(Handle {
            backend: self.clone(),
            id,
        }))
    }
    pub(crate) fn create_buffer(&self, bytes: &[u8], usage: u32) -> Result<Buffer, String> {
        if bytes.is_empty() || bytes.len() > MAX_BYTES {
            return Err("WebGPU buffer capacity".into());
        }
        Ok(Buffer {
            handle: self.resource(CREATE_BUFFER, &[usage], bytes, &mut [])?,
            size: bytes.len(),
        })
    }
    pub(crate) fn create_texture(&self, d: TextureDesc) -> Result<Texture, String> {
        if d.width == 0
            || d.height == 0
            || d.mips == 0
            || d.layers == 0
            || !self.0.formats.contains_key(&d.format)
        {
            return Err("unsupported WebGPU texture profile".into());
        }
        Ok(Texture {
            handle: self.resource(
                CREATE_TEXTURE,
                &[d.width, d.height, d.mips, d.layers, d.format, d.bind],
                &[],
                &mut [],
            )?,
            desc: d,
        })
    }
    pub(crate) fn format_support(&self, format: u32) -> u32 {
        self.0.formats.get(&format).copied().unwrap_or(0)
    }
    pub(crate) fn timestamp(&self) -> Result<(u64, u64), String> {
        let mut output = [0u8; 16];
        self.call(TIMESTAMP, &[], &[], &mut output)?;
        Ok((
            u64::from_le_bytes(output[..8].try_into().unwrap()),
            u64::from_le_bytes(output[8..].try_into().unwrap()),
        ))
    }
    pub(crate) fn compile(&self, bytes: &[u8], stage: u32) -> Result<Shader, String> {
        if bytes.len() > 16 * 1024 * 1024 || !matches!(stage, 1 | 2 | 4 | 8) {
            return Err("WebGPU shader input capacity/stage".into());
        }
        // The compiler returns bounded reflection; WGSL and GPUShaderModule
        // stay in the WebGPU owner, never enter guest memory.
        let mut output = [0u8; 8 + 128 * 16 + 32 * 4];
        let handle = self.resource(COMPILE_SHADER, &[stage], bytes, &mut output)?;
        let word = |n: usize| u32::from_le_bytes(output[n..n + 4].try_into().unwrap());
        let count = word(0) as usize;
        let inputs = word(4) as usize;
        if count > 128 || inputs > 32 {
            return Err("invalid WebGPU shader reflection".into());
        }
        let bindings = (0..count)
            .map(|n| {
                let at = 8 + n * 16;
                Binding {
                    set: word(at),
                    binding: word(at + 4),
                    descriptor_type: word(at + 8),
                    count: word(at + 12),
                }
            })
            .collect();
        let input_locations = (0..inputs).map(|n| word(8 + 128 * 16 + n * 4)).collect();
        Ok(Shader {
            handle,
            bindings,
            stage,
            input_locations,
        })
    }
    pub(crate) fn draw(&self, d: &draw::Draw<'_>) -> Result<(), String> {
        self.same(&d.vertex.handle)?;
        self.same(&d.pixel.handle)?;
        self.same(&d.target.handle)?;
        if let Some(depth) = d.depth {
            self.same(&depth.handle)?;
        }
        if let Some(index) = d.index {
            self.same(&index.handle)?;
        }
        let mut words = vec![
            d.vertex.handle.id,
            d.pixel.handle.id,
            d.target.handle.id,
            d.depth.map_or(0, |v| v.handle.id),
            d.index.map_or(0, |v| v.handle.id),
            d.topology,
            d.index_format,
            d.index_offset,
            d.indexed as u32,
            d.count,
            d.first,
            d.base_vertex as u32,
            d.instances,
            d.first_instance,
        ];
        words.extend(d.viewport.map(f32::to_bits));
        words.extend(d.scissor.map(|v| v as u32));
        words.extend(d.blend);
        words.extend(d.depth_state);
        words.extend(d.raster);
        words.extend(d.blend_factor.map(f32::to_bits));
        words.extend([
            d.sample_mask,
            d.stencil_ref,
            d.vertices.len() as u32,
            d.attributes.len() as u32,
            d.descriptors.len() as u32,
        ]);
        for v in &d.vertices {
            self.same(&v.buffer.handle)?;
            words.extend([v.slot as u32, v.buffer.handle.id, v.stride, v.offset]);
        }
        for a in &d.attributes {
            words.extend([
                a.location,
                a.format,
                a.slot,
                a.offset,
                a.input_class,
                a.step,
            ]);
        }
        for b in &d.descriptors {
            let (kind, id, sampler) = match b.resource {
                draw::Resource::Constant(v) => {
                    self.same(&v.handle)?;
                    (0, v.handle.id, [0; 13])
                }
                draw::Resource::Image(v) => {
                    self.same(&v.handle)?;
                    (1, v.handle.id, [0; 13])
                }
                draw::Resource::Buffer(v) => {
                    self.same(&v.handle)?;
                    (2, v.handle.id, [0; 13])
                }
                draw::Resource::Sampler(v) => (3, 0, v),
            };
            words.extend([b.set, b.binding, kind, id]);
            words.extend(sampler);
        }
        let bytes: Vec<_> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        self.call(DRAW, &[], &bytes, &mut [])?;
        Ok(())
    }
    pub(crate) fn present(
        &self,
        texture: &Texture,
        window: u64,
        interval: u32,
    ) -> Result<(), String> {
        self.same(&texture.handle)?;
        self.call(
            PRESENT,
            &[
                texture.handle.id,
                window as u32,
                (window >> 32) as u32,
                interval,
            ],
            &[],
            &mut [],
        )?;
        Ok(())
    }
    fn same(&self, h: &Handle) -> Result<(), String> {
        if Rc::ptr_eq(&self.0, &h.backend.0) {
            Ok(())
        } else {
            Err("resource belongs to another WebGPU device".into())
        }
    }
}
struct Handle {
    backend: Backend,
    id: u32,
}
impl Drop for Handle {
    fn drop(&mut self) {
        let _ = self.backend.call(RELEASE, &[self.id], &[], &mut []);
    }
}
#[derive(Clone)]
pub(crate) struct Buffer {
    handle: Rc<Handle>,
    size: usize,
}
impl Buffer {
    pub(crate) fn size(&self) -> usize {
        self.size
    }
    pub(crate) fn write(&mut self, offset: usize, bytes: &[u8]) -> Result<(), String> {
        if offset > self.size || bytes.len() > self.size - offset {
            return Err("WebGPU buffer write range".into());
        }
        self.handle.backend.call(
            WRITE_BUFFER,
            &[self.handle.id, offset as u32],
            bytes,
            &mut [],
        )?;
        Ok(())
    }
    pub(crate) fn read(&self, offset: usize, size: usize) -> Result<Vec<u8>, String> {
        if offset > self.size || size > self.size - offset {
            return Err("WebGPU buffer read range".into());
        }
        // Start/await the GPU transfer before allocating a Wasm result buffer.
        self.handle.backend.call(
            PREPARE_BUFFER_READ,
            &[self.handle.id, offset as u32, size as u32],
            &[],
            &mut [],
        )?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(size)
            .map_err(|_| "WebGPU read allocation")?;
        bytes.resize(size, 0);
        self.handle.backend.call(
            READ_BUFFER,
            &[self.handle.id, offset as u32],
            &[],
            &mut bytes,
        )?;
        Ok(bytes)
    }
    pub(crate) fn view(
        &self,
        format: u32,
        offset: usize,
        size: usize,
    ) -> Result<BufferView, String> {
        if offset > self.size || size > self.size - offset {
            return Err("WebGPU buffer view range".into());
        }
        Ok(BufferView {
            handle: self.handle.backend.resource(
                CREATE_BUFFER_VIEW,
                &[self.handle.id, format, offset as u32, size as u32],
                &[],
                &mut [],
            )?,
            _buffer: self.clone(),
            range: (offset, size, format),
        })
    }
}
#[derive(Clone)]
pub(crate) struct Texture {
    handle: Rc<Handle>,
    desc: TextureDesc,
}
impl Texture {
    pub(crate) fn desc(&self) -> TextureDesc {
        self.desc
    }
    fn subresource(&self, sub: u32) -> Result<(), String> {
        let layers = if self.desc.bind & 0x200 != 0 {
            1
        } else {
            self.desc.layers
        };
        if sub
            >= self
                .desc
                .mips
                .checked_mul(layers)
                .ok_or("WebGPU subresource overflow")?
        {
            Err("WebGPU subresource range".into())
        } else {
            Ok(())
        }
    }
    pub(crate) fn write(&self, sub: u32, bytes: &[u8]) -> Result<(), String> {
        self.subresource(sub)?;
        if bytes.len() > MAX_BYTES {
            return Err("WebGPU texture transfer capacity".into());
        }
        self.handle
            .backend
            .call(WRITE_TEXTURE, &[self.handle.id, sub], bytes, &mut [])?;
        Ok(())
    }
    pub(crate) fn read(&self, sub: u32, size: usize) -> Result<Vec<u8>, String> {
        self.subresource(sub)?;
        if size > MAX_BYTES {
            return Err("WebGPU texture transfer capacity".into());
        }
        self.handle.backend.call(
            PREPARE_TEXTURE_READ,
            &[self.handle.id, sub, size as u32],
            &[],
            &mut [],
        )?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(size)
            .map_err(|_| "WebGPU read allocation")?;
        bytes.resize(size, 0);
        self.handle
            .backend
            .call(READ_TEXTURE, &[self.handle.id, sub], &[], &mut bytes)?;
        Ok(bytes)
    }
    pub(crate) fn view(&self, d: ImageViewDesc) -> Result<ImageView, String> {
        Ok(ImageView {
            handle: self.handle.backend.resource(
                CREATE_IMAGE_VIEW,
                &[
                    self.handle.id,
                    d.format,
                    d.mip,
                    d.mips,
                    d.layer,
                    d.layers,
                    d.kind,
                ],
                &[],
                &mut [],
            )?,
            _texture: self.clone(),
            desc: d,
        })
    }
    pub(crate) fn copy_from(&self, source: &Texture, r: TextureCopy) -> Result<(), String> {
        self.handle.backend.same(&source.handle)?;
        self.handle.backend.call(
            COPY_TEXTURE,
            &[
                self.handle.id,
                source.handle.id,
                r.dst_sub,
                r.src_sub,
                r.x,
                r.y,
                r.left,
                r.top,
                r.right,
                r.bottom,
            ],
            &[],
            &mut [],
        )?;
        Ok(())
    }
}
pub(crate) struct ImageView {
    handle: Rc<Handle>,
    _texture: Texture,
    desc: ImageViewDesc,
}
impl ImageView {
    pub(crate) fn desc(&self) -> ImageViewDesc {
        self.desc
    }
    pub(crate) fn clear(&self, color: [f32; 4]) -> Result<(), String> {
        self.handle.backend.call(
            CLEAR_COLOR,
            &[
                self.handle.id,
                color[0].to_bits(),
                color[1].to_bits(),
                color[2].to_bits(),
                color[3].to_bits(),
            ],
            &[],
            &mut [],
        )?;
        Ok(())
    }
    pub(crate) fn clear_depth(&self, flags: u32, depth: f32, stencil: u32) -> Result<(), String> {
        self.handle.backend.call(
            CLEAR_DEPTH,
            &[self.handle.id, flags, depth.to_bits(), stencil],
            &[],
            &mut [],
        )?;
        Ok(())
    }
}
pub(crate) struct BufferView {
    handle: Rc<Handle>,
    _buffer: Buffer,
    range: (usize, usize, u32),
}
impl BufferView {
    pub(crate) fn range(&self) -> (usize, usize, u32) {
        self.range
    }
}
pub(crate) struct Shader {
    handle: Rc<Handle>,
    pub bindings: Vec<Binding>,
    pub stage: u32,
    pub input_locations: BTreeSet<u32>,
}
