//! Static Metal graphics acceleration; CPU guest interpretation is unchanged.
#[cfg(target_os = "macos")]
extern "C" {
    fn wire_metal_create(
        source: *const u8,
        size: u32,
        error: *mut u8,
        cap: u32,
    ) -> *mut std::ffi::c_void;
    fn wire_metal_destroy(ctx: *mut std::ffi::c_void);
    fn wire_metal_draw(
        ctx: *mut std::ffi::c_void,
        triangles: *const f32,
        count: u32,
        params: *const u32,
        source: *const u32,
        source_count: u32,
        dest: *mut u32,
        dest_count: u32,
        depth: *mut u32,
        depth_count: u32,
        error: *mut u8,
        cap: u32,
    ) -> i32;
}
#[derive(Default)]
pub(super) struct GpuRaster {
    #[cfg(target_os = "macos")]
    native: usize,
    #[cfg(target_os = "macos")]
    attempted: bool,
    pub draws: u64,
    pub error: Option<String>,
}
impl GpuRaster {
    pub fn prepare(&mut self) {
        #[cfg(target_os = "macos")]
        if !self.attempted {
            self.attempted = true;
            let shader = include_str!("../platform/raster.metal");
            let mut error = [0; 1024];
            self.native = unsafe {
                wire_metal_create(
                    shader.as_ptr(),
                    shader.len() as u32,
                    error.as_mut_ptr(),
                    error.len() as u32,
                )
            } as usize;
            if self.native == 0 {
                self.record_error(&error);
            }
        }
    }
    #[cfg(target_os = "macos")]
    fn record_error(&mut self, error: &[u8]) {
        if error[0] != 0 {
            let end = error.iter().position(|&b| b == 0).unwrap_or(error.len());
            self.error = Some(String::from_utf8_lossy(&error[..end]).into_owned());
        }
    }
    pub fn draw(
        &mut self,
        triangles: &[f32],
        params: &[u32; 200],
        source: &[u32],
        dest: &mut [u32],
        depth: Option<&mut [u32]>,
    ) -> bool {
        self.prepare();
        #[cfg(target_os = "macos")]
        {
            let mut error = [0; 1024];
            if self.native != 0 && triangles.len() == params[7] as usize * 36 {
                let depth = depth.unwrap_or(&mut []);
                let ok = unsafe {
                    wire_metal_draw(
                        self.native as *mut _,
                        triangles.as_ptr(),
                        params[7],
                        params.as_ptr(),
                        source.as_ptr(),
                        source.len() as u32,
                        dest.as_mut_ptr(),
                        dest.len() as u32,
                        depth.as_mut_ptr(),
                        depth.len() as u32,
                        error.as_mut_ptr(),
                        error.len() as u32,
                    )
                };
                if ok != 0 {
                    self.draws += 1;
                    return true;
                }
            }
            self.record_error(&error);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (triangles, params, source, dest, depth);
        false
    }
}
impl Drop for GpuRaster {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        if self.native != 0 {
            unsafe { wire_metal_destroy(self.native as *mut _) }
        }
    }
}
