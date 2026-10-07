//! Optional host graphics library. Native pointers stay here, never in guest memory.
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
    blit: Blit,
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
pub struct Shader {
    graphics: NativeGraphics,
    handle: NonNull<c_void>,
    pub words: Vec<u32>,
    pub bindings: Vec<Binding>,
    pub stage: u32,
    pub push_bytes: u32,
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
            let blit = library.symbol(b"wire_gpu_blit_rgba64\0")?;
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
                blit,
                info,
            })))
        }
    }
    pub fn adapter(&self) -> &AdapterInfo {
        &self.0.info
    }
    pub fn compile(&self, bytes: &[u8], stage: u32) -> Result<Shader, String> {
        if !(32..=1024 * 1024).contains(&bytes.len()) || !matches!(stage, 1 | 2 | 4) {
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
}

#[cfg(test)]
mod tests {
    use super::*;
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
