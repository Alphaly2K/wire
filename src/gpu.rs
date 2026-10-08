//! Optional host graphics library. Native pointers stay here, never in guest memory.
pub(crate) mod draw;
mod geometry;
pub use geometry::{GeometryBatch, GeometryInfo, GeometryIo};
use std::{
    ffi::{c_char, c_void, CString},
    path::Path,
    ptr::NonNull,
    rc::Rc,
};

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
struct Info {
    name: [u8; 256],
    vendor: u32,
    device: u32,
    api_version: u32,
    reserved: u32,
    dedicated_video: u64,
    dedicated_system: u64,
    shared_system: u64,
    uuid: [u8; 16],
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
struct View {
    words: *const u32,
    word_count: usize,
    bindings: *const Binding,
    binding_count: usize,
    stage: u32,
    push_bytes: u32,
}
type Close = unsafe extern "C" fn(*mut c_void);
type Compile =
    unsafe extern "C" fn(*mut c_void, *const u8, usize, u32, *mut c_char, usize) -> *mut c_void;
type ShaderInfo = unsafe extern "C" fn(*mut c_void, *mut View) -> i32;
type GeometryQuery = unsafe extern "C" fn(*mut c_void, *mut geometry::View) -> i32;
type GeometryDispatch = unsafe extern "C" fn(
    *mut c_void,
    *mut c_void,
    *const u32,
    usize,
    u32,
    *const BufferBinding,
    usize,
    *mut u32,
    usize,
    *mut c_char,
    usize,
) -> i32;
type GeometryDraw = unsafe extern "C" fn(
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *const u32,
    usize,
    *const u32,
    usize,
    *mut u8,
    usize,
    *mut c_char,
    usize,
) -> i32;
type BufferCreate =
    unsafe extern "C" fn(*mut c_void, *const u8, usize, u32, *mut c_char, usize) -> *mut c_void;
type BufferWrite =
    unsafe extern "C" fn(*mut c_void, usize, *const u8, usize, *mut c_char, usize) -> i32;
type BufferRead =
    unsafe extern "C" fn(*mut c_void, usize, *mut u8, usize, *mut c_char, usize) -> i32;
#[repr(C)]
struct BufferBinding {
    set: u32,
    binding: u32,
    buffer: *mut c_void,
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
type BufferViewCreate =
    unsafe extern "C" fn(*mut c_void, u32, usize, usize, *mut c_char, usize) -> *mut c_void;
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
type CopyTexture =
    unsafe extern "C" fn(*mut c_void, *mut c_void, *const TextureCopy, *mut c_char, usize) -> i32;
type TextureCreate =
    unsafe extern "C" fn(*mut c_void, *const TextureDesc, *mut c_char, usize) -> *mut c_void;
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
type ImageViewCreate =
    unsafe extern "C" fn(*mut c_void, *const ImageViewDesc, *mut c_char, usize) -> *mut c_void;
type ImageViewClear = unsafe extern "C" fn(*mut c_void, *const f32, *mut c_char, usize) -> i32;
type DepthViewClear = unsafe extern "C" fn(*mut c_void, u32, f32, u32, *mut c_char, usize) -> i32;
type TextureRead =
    unsafe extern "C" fn(*mut c_void, u32, *mut u8, usize, *mut c_char, usize) -> i32;
type TextureWrite =
    unsafe extern "C" fn(*mut c_void, u32, *const u8, usize, *mut c_char, usize) -> i32;
type SurfaceCreate =
    unsafe extern "C" fn(*mut c_void, *const c_void, *mut c_char, usize) -> *mut c_void;
type SurfacePresent =
    unsafe extern "C" fn(*mut c_void, *mut c_void, u32, u32, u32, *mut c_char, usize) -> i32;
type Pipeline = unsafe extern "C" fn(
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_char,
    usize,
) -> i32;
type Blit = unsafe extern "C" fn(
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *const u8,
    usize,
    *mut u8,
    usize,
    *mut c_char,
    usize,
) -> i32;
struct Library(NonNull<c_void>);
impl Library {
    fn load(path: &Path) -> Result<Self, String> {
        let path = CString::new(path.as_os_str().as_encoded_bytes()).map_err(|e| e.to_string())?;
        let handle = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        NonNull::new(handle).map(Self).ok_or_else(|| {
            let error = unsafe { libc::dlerror() };
            if error.is_null() {
                "cannot load native graphics library".into()
            } else {
                unsafe { std::ffi::CStr::from_ptr(error) }
                    .to_string_lossy()
                    .into_owned()
            }
        })
    }
    unsafe fn symbol<T: Copy>(&self, name: &'static [u8]) -> Result<T, String> {
        let name = std::ffi::CStr::from_bytes_with_nul(name).map_err(|e| e.to_string())?;
        let address = libc::dlsym(self.0.as_ptr(), name.as_ptr());
        if address.is_null() || std::mem::size_of::<T>() != std::mem::size_of::<*mut c_void>() {
            return Err(format!(
                "missing native graphics symbol {}",
                name.to_string_lossy()
            ));
        }
        Ok(std::mem::transmute_copy(&address))
    }
}
impl Drop for Library {
    fn drop(&mut self) {
        unsafe {
            libc::dlclose(self.0.as_ptr());
        }
    }
}
struct Inner {
    _library: Library,
    context: NonNull<c_void>,
    close: Close,
    compile: Compile,
    info_fn: ShaderInfo,
    free_shader: Close,
    pipeline: Pipeline,
    draw: draw::DrawFn,
    blit: Blit,
    geometry_query: GeometryQuery,
    geometry_dispatch: GeometryDispatch,
    geometry_draw: GeometryDraw,
    timestamp: unsafe extern "C" fn(*mut c_void, *mut u64, *mut u64, *mut c_char, usize) -> i32,
    format_support: unsafe extern "C" fn(*mut c_void, u32) -> u32,
    texture_copy: CopyTexture,
    texture_create: TextureCreate,
    texture_free: Close,
    texture_write: TextureWrite,
    texture_read: TextureRead,
    surface_create: SurfaceCreate,
    surface_free: Close,
    surface_present: SurfacePresent,
    image_view_create: ImageViewCreate,
    image_view_free: Close,
    image_view_clear: ImageViewClear,
    image_view_clear_depth: DepthViewClear,
    buffer_view_create: BufferViewCreate,
    buffer_view_free: Close,
    buffer_create: BufferCreate,
    buffer_free: Close,
    buffer_write: BufferWrite,
    buffer_read: BufferRead,
    info: AdapterInfo,
}
impl Drop for Inner {
    fn drop(&mut self) {
        unsafe {
            (self.close)(self.context.as_ptr());
        }
    }
}
#[derive(Clone)]
pub struct NativeGraphics(Rc<Inner>);
pub(crate) struct Surface {
    graphics: NativeGraphics,
    handle: NonNull<c_void>,
    _layer: crate::platform::MetalLayer,
}
impl Drop for Surface {
    fn drop(&mut self) {
        unsafe { (self.graphics.0.surface_free)(self.handle.as_ptr()) };
    }
}
impl Surface {
    pub(crate) fn present(
        &mut self,
        texture: &Texture,
        width: u32,
        height: u32,
        interval: u32,
    ) -> Result<(), String> {
        if !Rc::ptr_eq(&self.graphics.0, &texture.0.graphics.0) {
            return Err("presentation texture belongs to another GPU".into());
        }
        let mut error = [0u8; 4096];
        let ok = unsafe {
            (self.graphics.0.surface_present)(
                self.handle.as_ptr(),
                texture.0.handle.as_ptr(),
                width,
                height,
                interval,
                error.as_mut_ptr().cast(),
                error.len(),
            )
        };
        if ok == 0 {
            Err(message(&error))
        } else {
            Ok(())
        }
    }
}
#[derive(Clone)]
pub struct Buffer(Rc<BufferInner>);
struct BufferInner {
    graphics: NativeGraphics,
    handle: NonNull<c_void>,
    size: usize,
}
impl Drop for BufferInner {
    fn drop(&mut self) {
        unsafe { (self.graphics.0.buffer_free)(self.handle.as_ptr()) };
    }
}
impl Buffer {
    pub fn write(&mut self, offset: usize, bytes: &[u8]) -> Result<(), String> {
        if offset > self.0.size || bytes.len() > self.0.size - offset {
            return Err("buffer write range".into());
        }
        let mut error = [0u8; 4096];
        if unsafe {
            (self.0.graphics.0.buffer_write)(
                self.0.handle.as_ptr(),
                offset,
                bytes.as_ptr(),
                bytes.len(),
                error.as_mut_ptr().cast(),
                error.len(),
            )
        } == 0
        {
            return Err(message(&error));
        }
        Ok(())
    }
    pub fn read(&self, offset: usize, size: usize) -> Result<Vec<u8>, String> {
        if offset > self.0.size || size > self.0.size - offset {
            return Err("buffer read range".into());
        }
        let mut result = vec![0; size];
        let mut error = [0u8; 4096];
        if unsafe {
            (self.0.graphics.0.buffer_read)(
                self.0.handle.as_ptr(),
                offset,
                result.as_mut_ptr(),
                size,
                error.as_mut_ptr().cast(),
                error.len(),
            )
        } == 0
        {
            return Err(message(&error));
        }
        Ok(result)
    }
    pub fn size(&self) -> usize {
        self.0.size
    }
}
#[derive(Clone)]
pub struct Texture(Rc<TextureInner>);
struct TextureInner {
    graphics: NativeGraphics,
    handle: NonNull<c_void>,
    desc: TextureDesc,
}
impl Drop for TextureInner {
    fn drop(&mut self) {
        unsafe { (self.graphics.0.texture_free)(self.handle.as_ptr()) };
    }
}
impl Texture {
    pub fn desc(&self) -> TextureDesc {
        self.0.desc
    }
    pub fn read(&self, subresource: u32, size: usize) -> Result<Vec<u8>, String> {
        if size > 128 * 1024 * 1024 {
            return Err("texture read capacity".into());
        }
        let mut output = vec![0; size];
        let mut error = [0u8; 4096];
        if unsafe {
            (self.0.graphics.0.texture_read)(
                self.0.handle.as_ptr(),
                subresource,
                output.as_mut_ptr(),
                size,
                error.as_mut_ptr().cast(),
                error.len(),
            )
        } == 0
        {
            return Err(message(&error));
        }
        Ok(output)
    }
    pub fn view(&self, desc: ImageViewDesc) -> Result<ImageView, String> {
        let mut error = [0u8; 4096];
        let handle = NonNull::new(unsafe {
            (self.0.graphics.0.image_view_create)(
                self.0.handle.as_ptr(),
                &desc,
                error.as_mut_ptr().cast(),
                error.len(),
            )
        })
        .ok_or_else(|| message(&error))?;
        Ok(ImageView {
            texture: self.clone(),
            handle,
            desc,
        })
    }
    pub fn copy_from(&self, source: &Texture, region: TextureCopy) -> Result<(), String> {
        if !Rc::ptr_eq(&self.0.graphics.0, &source.0.graphics.0) {
            return Err("cross-context texture copy".into());
        }
        let mut error = [0u8; 4096];
        if unsafe {
            (self.0.graphics.0.texture_copy)(
                self.0.handle.as_ptr(),
                source.0.handle.as_ptr(),
                &region,
                error.as_mut_ptr().cast(),
                error.len(),
            )
        } == 0
        {
            return Err(message(&error));
        }
        Ok(())
    }
    pub fn write(&self, subresource: u32, bytes: &[u8]) -> Result<(), String> {
        let mut error = [0u8; 4096];
        if unsafe {
            (self.0.graphics.0.texture_write)(
                self.0.handle.as_ptr(),
                subresource,
                bytes.as_ptr(),
                bytes.len(),
                error.as_mut_ptr().cast(),
                error.len(),
            )
        } == 0
        {
            return Err(message(&error));
        }
        Ok(())
    }
}
pub struct ImageView {
    texture: Texture,
    handle: NonNull<c_void>,
    desc: ImageViewDesc,
}
impl Drop for ImageView {
    fn drop(&mut self) {
        unsafe { (self.texture.0.graphics.0.image_view_free)(self.handle.as_ptr()) };
    }
}
impl ImageView {
    pub fn clear_depth(&self, flags: u32, depth: f32, stencil: u32) -> Result<(), String> {
        let mut error = [0u8; 4096];
        if unsafe {
            (self.texture.0.graphics.0.image_view_clear_depth)(
                self.handle.as_ptr(),
                flags,
                depth,
                stencil,
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
    pub fn desc(&self) -> ImageViewDesc {
        self.desc
    }
    pub fn clear(&self, rgba: [f32; 4]) -> Result<(), String> {
        let mut error = [0u8; 4096];
        if unsafe {
            (self.texture.0.graphics.0.image_view_clear)(
                self.handle.as_ptr(),
                rgba.as_ptr(),
                error.as_mut_ptr().cast(),
                error.len(),
            )
        } == 0
        {
            return Err(message(&error));
        }
        Ok(())
    }
}
pub struct BufferView {
    buffer: Buffer,
    handle: NonNull<c_void>,
    range: (usize, usize, u32),
}
impl BufferView {
    pub fn range(&self) -> (usize, usize, u32) {
        self.range
    }
}
impl Drop for BufferView {
    fn drop(&mut self) {
        unsafe { (self.buffer.0.graphics.0.buffer_view_free)(self.handle.as_ptr()) };
    }
}
impl Buffer {
    pub fn view(&self, format: u32, offset: usize, size: usize) -> Result<BufferView, String> {
        let mut error = [0u8; 4096];
        let handle = NonNull::new(unsafe {
            (self.0.graphics.0.buffer_view_create)(
                self.0.handle.as_ptr(),
                format,
                offset,
                size,
                error.as_mut_ptr().cast(),
                error.len(),
            )
        })
        .ok_or_else(|| message(&error))?;
        Ok(BufferView {
            buffer: self.clone(),
            handle,
            range: (offset, size, format),
        })
    }
}
pub struct Shader {
    graphics: NativeGraphics,
    handle: NonNull<c_void>,
    pub words: Vec<u32>,
    pub bindings: Vec<Binding>,
    pub stage: u32,
    pub push_bytes: u32,
    geometry: Option<GeometryInfo>,
}
impl Shader {
    pub fn geometry(&self) -> Option<&GeometryInfo> {
        self.geometry.as_ref()
    }
}
impl Drop for Shader {
    fn drop(&mut self) {
        unsafe {
            (self.graphics.0.free_shader)(self.handle.as_ptr());
        }
    }
}
fn message(error: &[u8]) -> String {
    String::from_utf8_lossy(&error[..error.iter().position(|&b| b == 0).unwrap_or(error.len())])
        .into_owned()
}
impl NativeGraphics {
    pub fn load(backend: &Path, moltenvk: &Path) -> Result<Self, String> {
        let library = Library::load(backend)?;
        let path =
            CString::new(moltenvk.as_os_str().as_encoded_bytes()).map_err(|e| e.to_string())?;
        unsafe {
            let abi: unsafe extern "C" fn() -> u32 = library.symbol(b"wire_gpu_abi\0")?;
            if abi() != 1 {
                return Err("native graphics ABI version mismatch".into());
            }
            let open: unsafe extern "C" fn(
                *const c_char,
                *mut Info,
                *mut c_char,
                usize,
            ) -> *mut c_void = library.symbol(b"wire_gpu_open\0")?;
            let close = library.symbol(b"wire_gpu_close\0")?;
            let compile = library.symbol(b"wire_gpu_compile\0")?;
            let info_fn = library.symbol(b"wire_gpu_shader_info\0")?;
            let free_shader = library.symbol(b"wire_gpu_shader_free\0")?;
            let pipeline = library.symbol(b"wire_gpu_pipeline\0")?;
            let draw = library.symbol(b"wire_gpu_draw\0")?;
            let blit = library.symbol(b"wire_gpu_blit_rgba64\0")?;
            let geometry_query = library.symbol(b"wire_gpu_geometry_info\0")?;
            let geometry_dispatch = library.symbol(b"wire_gpu_geometry_dispatch\0")?;
            let geometry_draw = library.symbol(b"wire_gpu_geometry_draw_rgba64\0")?;
            let timestamp = library.symbol(b"wire_gpu_timestamp\0")?;
            let format_support = library.symbol(b"wire_gpu_format_support\0")?;
            let texture_copy = library.symbol(b"wire_gpu_texture_copy\0")?;
            let texture_create = library.symbol(b"wire_gpu_texture_create\0")?;
            let texture_free = library.symbol(b"wire_gpu_texture_free\0")?;
            let texture_read = library.symbol(b"wire_gpu_texture_read\0")?;
            let surface_create = library.symbol(b"wire_gpu_surface_create\0")?;
            let surface_free = library.symbol(b"wire_gpu_surface_free\0")?;
            let surface_present = library.symbol(b"wire_gpu_surface_present\0")?;
            let image_view_create = library.symbol(b"wire_gpu_image_view_create\0")?;
            let image_view_free = library.symbol(b"wire_gpu_image_view_free\0")?;
            let image_view_clear = library.symbol(b"wire_gpu_image_view_clear\0")?;
            let image_view_clear_depth = library.symbol(b"wire_gpu_image_view_clear_depth\0")?;
            let texture_write = library.symbol(b"wire_gpu_texture_write\0")?;
            let buffer_view_create = library.symbol(b"wire_gpu_buffer_view_create\0")?;
            let buffer_view_free = library.symbol(b"wire_gpu_buffer_view_free\0")?;
            let buffer_create = library.symbol(b"wire_gpu_buffer_create\0")?;
            let buffer_free = library.symbol(b"wire_gpu_buffer_free\0")?;
            let buffer_write = library.symbol(b"wire_gpu_buffer_write\0")?;
            let buffer_read = library.symbol(b"wire_gpu_buffer_read\0")?;
            let mut info: Info = std::mem::zeroed();
            let mut error = [0u8; 4096];
            let context = NonNull::new(open(
                path.as_ptr(),
                &mut info,
                error.as_mut_ptr().cast(),
                error.len(),
            ))
            .ok_or_else(|| message(&error))?;
            let info = AdapterInfo {
                name: message(&info.name),
                vendor: info.vendor,
                device: info.device,
                api_version: info.api_version,
                dedicated_video: info.dedicated_video,
                dedicated_system: info.dedicated_system,
                shared_system: info.shared_system,
                uuid: info.uuid,
            };
            Ok(Self(Rc::new(Inner {
                _library: library,
                context,
                close,
                compile,
                info_fn,
                free_shader,
                pipeline,
                draw,
                blit,
                geometry_query,
                geometry_dispatch,
                geometry_draw,
                timestamp,
                format_support,
                texture_copy,
                texture_create,
                texture_free,
                texture_write,
                texture_read,
                surface_create,
                surface_free,
                surface_present,
                image_view_create,
                image_view_free,
                image_view_clear,
                image_view_clear_depth,
                buffer_view_create,
                buffer_view_free,
                buffer_create,
                buffer_free,
                buffer_write,
                buffer_read,
                info,
            })))
        }
    }
    pub fn create_texture(&self, desc: TextureDesc) -> Result<Texture, String> {
        let mut error = [0u8; 4096];
        let handle = NonNull::new(unsafe {
            (self.0.texture_create)(
                self.0.context.as_ptr(),
                &desc,
                error.as_mut_ptr().cast(),
                error.len(),
            )
        })
        .ok_or_else(|| message(&error))?;
        Ok(Texture(Rc::new(TextureInner {
            graphics: self.clone(),
            handle,
            desc,
        })))
    }
    pub(crate) fn create_surface(
        &self,
        layer: crate::platform::MetalLayer,
    ) -> Result<Surface, String> {
        let mut error = [0u8; 4096];
        let handle = NonNull::new(unsafe {
            (self.0.surface_create)(
                self.0.context.as_ptr(),
                layer.as_ptr(),
                error.as_mut_ptr().cast(),
                error.len(),
            )
        })
        .ok_or_else(|| message(&error))?;
        Ok(Surface {
            graphics: self.clone(),
            handle,
            _layer: layer,
        })
    }
    pub fn timestamp(&self) -> Result<(u64, u64), String> {
        let mut ticks = 0;
        let mut frequency = 0;
        let mut error = [0u8; 4096];
        if unsafe {
            (self.0.timestamp)(
                self.0.context.as_ptr(),
                &mut ticks,
                &mut frequency,
                error.as_mut_ptr().cast(),
                error.len(),
            )
        } == 0
        {
            return Err(message(&error));
        }
        Ok((ticks, frequency))
    }
    pub fn format_support(&self, format: u32) -> u32 {
        unsafe { (self.0.format_support)(self.0.context.as_ptr(), format) }
    }
    pub fn adapter(&self) -> &AdapterInfo {
        &self.0.info
    }
    /// Persistent Vulkan buffer with owned copied data. Usage: CBV=1, storage=2,
    /// vertex=4, index=8. The native library checks actual device limits.
    pub fn create_buffer(&self, bytes: &[u8], usage: u32) -> Result<Buffer, String> {
        if bytes.is_empty() || bytes.len() > 64 * 1024 * 1024 || usage == 0 || usage & !15 != 0 {
            return Err("buffer size/usage limits".into());
        }
        let mut error = [0u8; 4096];
        let handle = NonNull::new(unsafe {
            (self.0.buffer_create)(
                self.0.context.as_ptr(),
                bytes.as_ptr(),
                bytes.len(),
                usage,
                error.as_mut_ptr().cast(),
                error.len(),
            )
        })
        .ok_or_else(|| message(&error))?;
        Ok(Buffer(Rc::new(BufferInner {
            graphics: self.clone(),
            handle,
            size: bytes.len(),
        })))
    }
    pub fn compile(&self, bytes: &[u8], stage: u32) -> Result<Shader, String> {
        if !(32..=1024 * 1024).contains(&bytes.len()) || !matches!(stage, 1 | 2 | 4 | 8) {
            return Err("invalid shader size/stage".into());
        }
        let mut error = [0u8; 4096];
        let handle = NonNull::new(unsafe {
            (self.0.compile)(
                self.0.context.as_ptr(),
                bytes.as_ptr(),
                bytes.len(),
                stage,
                error.as_mut_ptr().cast(),
                error.len(),
            )
        })
        .ok_or_else(|| message(&error))?;
        let mut shader = Shader {
            graphics: self.clone(),
            handle,
            words: Vec::new(),
            bindings: Vec::new(),
            stage,
            push_bytes: 0,
            geometry: None,
        };
        let mut view: View = unsafe { std::mem::zeroed() };
        if unsafe { (self.0.info_fn)(handle.as_ptr(), &mut view) } == 0
            || view.words.is_null()
            || !(5..=4 * 1024 * 1024).contains(&view.word_count)
            || view.binding_count > 512
            || (view.binding_count != 0 && view.bindings.is_null())
            || view.stage != stage
        {
            return Err("invalid native shader metadata".into());
        }
        shader.words = unsafe { std::slice::from_raw_parts(view.words, view.word_count) }.to_vec();
        if view.binding_count != 0 {
            shader.bindings =
                unsafe { std::slice::from_raw_parts(view.bindings, view.binding_count) }.to_vec();
        }
        shader.push_bytes = view.push_bytes;
        if stage == 8 {
            let mut g: geometry::View = unsafe { std::mem::zeroed() };
            if unsafe { (self.0.geometry_query)(handle.as_ptr(), &mut g) } == 0
                || g.input_count > 128
                || g.output_count > 128
                || (g.input_count != 0 && g.inputs.is_null())
                || (g.output_count != 0 && g.outputs.is_null())
                || !(1..=6).contains(&g.input_vertices)
                || !(1..=1024).contains(&g.max_vertices)
                || !(1..=32).contains(&g.instances)
                || g.input_words > 768
                || g.vertex_words > 128
                || g.max_vertices * g.vertex_words > 1024
                || g.invocation_words != 1 + (g.max_vertices + 1) * (g.vertex_words + 1)
                || !matches!(g.output_primitive, 0 | 1 | 3)
                || g.reserved != 0
            {
                return Err("invalid native geometry metadata".into());
            }
            let copy = |p, n| {
                if n == 0 {
                    Vec::new()
                } else {
                    unsafe { std::slice::from_raw_parts(p, n) }.to_vec()
                }
            };
            let inputs = copy(g.inputs, g.input_count);
            let outputs = copy(g.outputs, g.output_count);
            for (list, words) in [(&inputs, g.input_words), (&outputs, g.vertex_words)] {
                let mut end = 0;
                for io in list {
                    if io.array_size == 0
                        || io.word_offset != end
                        || !(1..=4).contains(&io.vector_size)
                        || io.array_dimensions > 2
                        || !matches!(io.scalar_type, 5 | 9 | 12)
                    {
                        return Err("invalid geometry I/O packing".into());
                    }
                    end = end
                        .checked_add(io.array_size)
                        .ok_or("geometry I/O capacity")?;
                }
                if end != words {
                    return Err("invalid geometry I/O size".into());
                }
            }
            shader.geometry = Some(GeometryInfo {
                inputs,
                outputs,
                input_vertices: g.input_vertices,
                output_primitive: g.output_primitive,
                max_vertices: g.max_vertices,
                instances: g.instances,
                input_words: g.input_words,
                vertex_words: g.vertex_words,
                invocation_words: g.invocation_words,
            });
        }
        Ok(shader)
    }
    pub fn validate_pipeline(
        &self,
        vertex: Option<&Shader>,
        pixel: Option<&Shader>,
        compute: Option<&Shader>,
    ) -> Result<(), String> {
        for shader in [vertex, pixel, compute].into_iter().flatten() {
            if !Rc::ptr_eq(&self.0, &shader.graphics.0) {
                return Err("shader belongs to another GPU".into());
            }
        }
        let pointer = |s: Option<&Shader>| s.map_or(std::ptr::null_mut(), |s| s.handle.as_ptr());
        let mut error = [0u8; 4096];
        if unsafe {
            (self.0.pipeline)(
                self.0.context.as_ptr(),
                pointer(vertex),
                pointer(pixel),
                pointer(compute),
                error.as_mut_ptr().cast(),
                error.len(),
            )
        } == 0
        {
            return Err(message(&error));
        }
        Ok(())
    }
    /// Bounded host offscreen draw/readback. This is not a guest swapchain.
    /// VS inputs are F32x4 position/F32x2 UV; PS has a 2D t0 and sampler s0.
    pub fn blit_rgba64(
        &self,
        vertex: &Shader,
        pixel: &Shader,
        rgba: &[u8],
    ) -> Result<Vec<u8>, String> {
        if rgba.len() != 64 * 64 * 4 {
            return Err("RGBA64 buffer size".into());
        }
        for shader in [vertex, pixel] {
            if !Rc::ptr_eq(&self.0, &shader.graphics.0) {
                return Err("shader belongs to another GPU".into());
            }
        }
        let mut output = vec![0; rgba.len()];
        let mut error = [0u8; 4096];
        if unsafe {
            (self.0.blit)(
                self.0.context.as_ptr(),
                vertex.handle.as_ptr(),
                pixel.handle.as_ptr(),
                rgba.as_ptr(),
                rgba.len(),
                output.as_mut_ptr(),
                output.len(),
                error.as_mut_ptr().cast(),
                error.len(),
            )
        } == 0
        {
            return Err(message(&error));
        }
        Ok(output)
    }
    /// Expand a resource-free GS on the GPU and restore list indices on the host.
    /// Input is packed using the shader's checked geometry metadata.
    pub fn expand_geometry(
        &self,
        shader: &Shader,
        input: &[u32],
        primitives: u32,
    ) -> Result<GeometryBatch, String> {
        self.expand_geometry_with_buffers(shader, input, primitives, &[])
    }
    /// Buffer bindings use GS descriptor set 3 and the reflected binding number.
    pub fn expand_geometry_with_buffers(
        &self,
        shader: &Shader,
        input: &[u32],
        primitives: u32,
        buffers: &[(u32, &Buffer)],
    ) -> Result<GeometryBatch, String> {
        if !Rc::ptr_eq(&self.0, &shader.graphics.0) {
            return Err("shader belongs to another GPU".into());
        }
        if buffers.len() > 256 {
            return Err("GS resource capacity".into());
        }
        let mut bindings = Vec::with_capacity(buffers.len());
        for (binding, buffer) in buffers {
            if !Rc::ptr_eq(&self.0, &buffer.0.graphics.0) {
                return Err("buffer belongs to another GPU".into());
            }
            bindings.push(BufferBinding {
                set: 3,
                binding: *binding,
                buffer: buffer.0.handle.as_ptr(),
            });
        }
        let g = shader
            .geometry
            .as_ref()
            .ok_or("not a geometry expansion shader")?;
        let size = u64::from(primitives) * u64::from(g.instances) * u64::from(g.invocation_words);
        if primitives == 0
            || size > 16 * 1024 * 1024
            || u64::from(primitives) * u64::from(g.input_words) != input.len() as u64
        {
            return Err("GS dispatch buffer/count limits".into());
        }
        let mut output = vec![0; size as usize];
        let mut error = [0u8; 4096];
        if unsafe {
            (self.0.geometry_dispatch)(
                self.0.context.as_ptr(),
                shader.handle.as_ptr(),
                input.as_ptr(),
                input.len(),
                primitives,
                bindings.as_ptr(),
                bindings.len(),
                output.as_mut_ptr(),
                output.len(),
                error.as_mut_ptr().cast(),
                error.len(),
            )
        } == 0
        {
            return Err(message(&error));
        }
        g.assemble(&output)
    }
    /// Rasterize expanded geometry to a 64x64 diagnostic render target using a
    /// real DXBC pixel shader. This does not present a guest window.
    pub fn draw_geometry_rgba64(
        &self,
        geometry: &Shader,
        pixel: &Shader,
        batch: &GeometryBatch,
    ) -> Result<Vec<u8>, String> {
        for shader in [geometry, pixel] {
            if !Rc::ptr_eq(&self.0, &shader.graphics.0) {
                return Err("shader belongs to another GPU".into());
            }
        }
        let g = geometry
            .geometry
            .as_ref()
            .ok_or("not a geometry expansion shader")?;
        if batch.output_primitive != g.output_primitive || batch.vertex_words != g.vertex_words {
            return Err("GS draw output layout mismatch".into());
        }
        let mut output = vec![0; 64 * 64 * 4];
        let mut error = [0u8; 4096];
        if unsafe {
            (self.0.geometry_draw)(
                self.0.context.as_ptr(),
                geometry.handle.as_ptr(),
                pixel.handle.as_ptr(),
                batch.vertices.as_ptr(),
                batch.vertices.len(),
                batch.indices.as_ptr(),
                batch.indices.len(),
                output.as_mut_ptr(),
                output.len(),
                error.as_mut_ptr().cast(),
                error.len(),
            )
        } == 0
        {
            return Err(message(&error));
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires explicit native library and MoltenVK"]
    fn native_views_keep_resources_and_device_alive_and_reject_foreign_copies() {
        let path = |n| std::path::PathBuf::from(std::env::var_os(n).expect(n));
        let graphics =
            NativeGraphics::load(&path("WIRE_NATIVE_GRAPHICS"), &path("WIRE_MOLTENVK")).unwrap();
        let desc = TextureDesc {
            width: 8,
            height: 4,
            mips: 1,
            layers: 1,
            format: 28,
            bind: 0x28,
        };
        let texture = graphics.create_texture(desc).unwrap();
        let view = texture
            .view(ImageViewDesc {
                format: 28,
                mip: 0,
                mips: 1,
                layer: 0,
                layers: 1,
                kind: 1,
            })
            .unwrap();
        let other =
            NativeGraphics::load(&path("WIRE_NATIVE_GRAPHICS"), &path("WIRE_MOLTENVK")).unwrap();
        let foreign = other.create_texture(desc).unwrap();
        let region = TextureCopy {
            dst_sub: 0,
            src_sub: 0,
            x: 0,
            y: 0,
            left: 0,
            top: 0,
            right: 8,
            bottom: 4,
        };
        assert!(texture.copy_from(&foreign, region).is_err());
        let buffer = graphics.create_buffer(&[0; 64], 2).unwrap();
        let typed = buffer.view(42, 0, 64).unwrap();
        assert!(buffer.view(42, 60, 8).is_err());
        drop(buffer);
        drop(texture);
        drop(graphics);
        view.clear([0.25, 0.5, 0.75, 1.]).unwrap();
        assert_eq!(
            view.texture.read(0, 128).unwrap(),
            [64, 128, 191, 255].repeat(32)
        );
        assert_eq!(typed.buffer.read(0, 64).unwrap(), vec![0; 64]);
    }
    #[test]
    #[ignore = "requires explicit external native library, MoltenVK and generated original GS fixtures"]
    fn geometry_gpu_resources_draw_and_lifetime() {
        let path = |n| std::path::PathBuf::from(std::env::var_os(n).expect(n));
        let graphics =
            NativeGraphics::load(&path("WIRE_NATIVE_GRAPHICS"), &path("WIRE_MOLTENVK")).unwrap();
        let fixtures = path("WIRE_GS_FIXTURES");
        let gs = graphics
            .compile(&std::fs::read(fixtures.join("triangle.dxbc")).unwrap(), 8)
            .unwrap();
        let ps = graphics
            .compile(&std::fs::read(fixtures.join("color.ps.dxbc")).unwrap(), 2)
            .unwrap();
        let screen = [-1.0f32, -1., 0., 1., 3., -1., 0., 1., -1., 3., 0., 1.].map(f32::to_bits);
        let batch = graphics.expand_geometry(&gs, &screen, 1).unwrap();
        assert_eq!(batch.indices, [0, 1, 2]);
        assert_eq!(
            graphics.draw_geometry_rgba64(&gs, &ps, &batch).unwrap(),
            [64, 128, 191, 255].repeat(4096)
        );
        let resource_gs = graphics
            .compile(
                &std::fs::read(fixtures.join("constant-buffer.dxbc")).unwrap(),
                8,
            )
            .unwrap();
        assert!(graphics.expand_geometry(&resource_gs, &screen, 1).is_err());
        assert!(graphics.create_buffer(&[0; 15], 1).is_err());
        let mut buffer = graphics.create_buffer(&[0; 16], 1).unwrap();
        buffer.write(0, &77u32.to_le_bytes()).unwrap();
        let original = buffer.read(0, 16).unwrap();
        assert!(buffer.write(14, &[1; 4]).is_err());
        assert_eq!(buffer.read(0, 16).unwrap(), original);
        assert!(buffer.read(15, 2).is_err());
        let batch = graphics
            .expand_geometry_with_buffers(&resource_gs, &screen, 1, &[(0, &buffer)])
            .unwrap();
        let g = resource_gs.geometry.as_ref().unwrap();
        let id = g.outputs.iter().find(|io| io.location == 1).unwrap();
        for v in batch.vertices.as_chunks::<6>().0 {
            assert_eq!(v[id.word_offset as usize + 1], 77);
        }
        assert!(graphics
            .expand_geometry_with_buffers(&resource_gs, &screen, 1, &[(0, &buffer), (0, &buffer)])
            .is_err());
        let other =
            NativeGraphics::load(&path("WIRE_NATIVE_GRAPHICS"), &path("WIRE_MOLTENVK")).unwrap();
        assert!(other.expand_geometry(&gs, &screen, 1).is_err());
        let foreign = other.create_buffer(&[0; 16], 1).unwrap();
        assert!(graphics
            .expand_geometry_with_buffers(&resource_gs, &screen, 1, &[(0, &foreign)])
            .is_err());
        drop(graphics);
        assert_eq!(buffer.read(0, 16).unwrap(), original);
        gs.graphics.expand_geometry(&gs, &screen, 1).unwrap();
    }
    #[test]
    #[ignore = "requires explicit external native library, MoltenVK and isolated DXBC fixtures"]
    fn native_shader_lifetime_validation_and_context_ownership() {
        let path = |n| std::path::PathBuf::from(std::env::var_os(n).expect(n));
        let backend = path("WIRE_NATIVE_GRAPHICS");
        let moltenvk = path("WIRE_MOLTENVK");
        let vertex = std::fs::read(path("WIRE_DXBC_VERTEX")).unwrap();
        let pixel = std::fs::read(path("WIRE_DXBC_PIXEL")).unwrap();
        let compute = std::fs::read(path("WIRE_DXBC_COMPUTE")).unwrap();
        let blit_vertex = std::fs::read(path("WIRE_DXBC_BLIT_VERTEX")).unwrap();
        let blit_pixel = std::fs::read(path("WIRE_DXBC_BLIT_PIXEL")).unwrap();
        let graphics = NativeGraphics::load(&backend, &moltenvk).unwrap();
        assert!(!graphics.adapter().name.is_empty());
        assert!(graphics.compile(&[0; 32], 1).is_err());
        assert!(graphics.compile(&vertex, 2).is_err());
        let vs = graphics.compile(&vertex, 1).unwrap();
        let ps = graphics.compile(&pixel, 2).unwrap();
        let cs = graphics.compile(&compute, 4).unwrap();
        for (shader, set) in [(&vs, 0), (&ps, 1), (&cs, 2)] {
            assert_eq!(shader.words[0], 0x07230203);
            assert!(shader
                .bindings
                .iter()
                .all(|b| b.set == set && b.binding < 166 && b.count != 0));
        }
        graphics
            .validate_pipeline(Some(&vs), Some(&ps), None)
            .unwrap();
        graphics.validate_pipeline(None, None, Some(&cs)).unwrap();
        let bvs = graphics.compile(&blit_vertex, 1).unwrap();
        let bps = graphics.compile(&blit_pixel, 2).unwrap();
        let rgba: Vec<u8> = (0..64)
            .flat_map(|y| (0..64).flat_map(move |x| [x * 4, y * 4, (x ^ y) * 4, 255]))
            .collect();
        assert_eq!(graphics.blit_rgba64(&bvs, &bps, &rgba).unwrap(), rgba);
        assert!(graphics
            .blit_rgba64(&bvs, &bps, &rgba[..rgba.len() - 1])
            .is_err());
        assert!(graphics
            .validate_pipeline(Some(&vs), None, Some(&cs))
            .is_err());
        let other = NativeGraphics::load(&backend, &moltenvk).unwrap();
        assert!(other
            .validate_pipeline(None, None, Some(&cs))
            .unwrap_err()
            .contains("another GPU"));
        assert!(other
            .blit_rgba64(&bvs, &bps, &rgba)
            .unwrap_err()
            .contains("another GPU"));
        drop(bvs);
        drop(bps);
        drop(other);
        drop(graphics);
        drop(vs);
        drop(ps);
        // The shader owns the library and context after the original wrapper dies.
        cs.graphics
            .validate_pipeline(None, None, Some(&cs))
            .unwrap();
        drop(cs);
    }
}
