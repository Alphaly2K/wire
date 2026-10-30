//! Runtime boundaries used by the shared D3D9 implementation.
pub(crate) trait Memory {
    type Error: From<u32>;
    fn check(&self, p: u32, n: u32, write: bool) -> Result<(), Self::Error>;
    fn read_bytes(&self, p: u32, bytes: &mut [u8]) -> Result<(), Self::Error>;
    fn write_bytes(&mut self, p: u32, bytes: &[u8]) -> Result<(), Self::Error>;
    fn read_u16(&self, p: u32) -> Result<u16, Self::Error> {
        let mut bytes = [0; 2];
        self.read_bytes(p, &mut bytes)?;
        Ok(u16::from_le_bytes(bytes))
    }
    fn read_u32(&self, p: u32) -> Result<u32, Self::Error> {
        let mut bytes = [0; 4];
        self.read_bytes(p, &mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }
}
pub(crate) trait Backend: Memory {
    fn status(error: &Self::Error) -> Option<u32>;
    fn allocate(&mut self, bytes: u32) -> Result<u32, Self::Error>;
    fn free(&mut self, p: u32) -> Result<(), Self::Error>;
    fn check_methods(&self, kind: &str) -> Result<(), Self::Error>;
    fn register(&mut self, method: super::Method) -> Result<u32, Self::Error>;
    fn window(&self, hwnd: u32) -> Option<[i32; 2]>;
    fn present(
        &mut self,
        hwnd: u32,
        width: u32,
        height: u32,
        pixels: &[u32],
    ) -> Result<(), Self::Error>;
}
pub trait RasterBackend {
    fn prepare(&mut self) {}
    fn draws(&self) -> u64 {
        0
    }
    fn error(&self) -> Option<&str> {
        None
    }
    fn draw(
        &mut self,
        triangles: &[f32],
        params: &[u32; 200],
        source: &[u32],
        dest: &mut [u32],
        depth: Option<&mut [u32]>,
        program: &[u32],
    ) -> bool;
}
#[derive(Default)]
pub struct SoftwareRaster;
impl RasterBackend for SoftwareRaster {
    fn draw(
        &mut self,
        _: &[f32],
        _: &[u32; 200],
        _: &[u32],
        _: &mut [u32],
        _: Option<&mut [u32]>,
        _: &[u32],
    ) -> bool {
        false
    }
}
