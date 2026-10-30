//! PE32 VMM, gateway and window adapter for shared D3D9 semantics.
use super::{d3d9_core, heaps, user, ServiceError, Services};
use crate::{
    memory::{Fault, Memory, READ, WRITE},
    process::Loaded,
};
pub(crate) use d3d9_core::Method;
pub(crate) type Owner = d3d9_core::D3D9;
enum Failure {
    Hresult(u32),
    Service(ServiceError),
}
impl From<u32> for Failure {
    fn from(n: u32) -> Self {
        Self::Hresult(n)
    }
}
impl From<Fault> for Failure {
    fn from(f: Fault) -> Self {
        Self::Service(f.into())
    }
}
fn heap_error(error: heaps::Error) -> Failure {
    match error {
        heaps::Error::Windows(8) => 0x8007000e.into(),
        heaps::Error::Fault(fault) => fault.into(),
        _ => Failure::Service(ServiceError::Unavailable(
            "D3D9 process allocation was lost",
        )),
    }
}
struct Access<'a> {
    memory: &'a mut Memory,
    heaps: &'a mut heaps::Heaps,
    services: &'a mut Services,
    user: &'a mut user::User,
}
impl d3d9_core::Memory for Access<'_> {
    type Error = Failure;
    fn check(&self, p: u32, n: u32, w: bool) -> Result<(), Failure> {
        if p == 0 {
            return Err(0x8876086c.into());
        }
        Ok(self
            .memory
            .check(u64::from(p), n as usize, if w { WRITE } else { READ })?)
    }
    fn read_bytes(&self, p: u32, b: &mut [u8]) -> Result<(), Failure> {
        self.check(p, b.len() as u32, false)?;
        Ok(self.memory.read(u64::from(p), b)?)
    }
    fn write_bytes(&mut self, p: u32, b: &[u8]) -> Result<(), Failure> {
        self.check(p, b.len() as u32, true)?;
        Ok(self.memory.write(u64::from(p), b)?)
    }
}
impl d3d9_core::Backend for Access<'_> {
    fn status(error: &Failure) -> Option<u32> {
        if let Failure::Hresult(n) = error {
            Some(*n)
        } else {
            None
        }
    }
    fn allocate(&mut self, n: u32) -> Result<u32, Failure> {
        self.heaps
            .allocate_local(self.memory, u64::from(n), &[])
            .map(|p| p as u32)
            .map_err(heap_error)
    }
    fn free(&mut self, p: u32) -> Result<(), Failure> {
        self.heaps
            .free_process(self.memory, u64::from(p))
            .map_err(heap_error)
    }
    fn check_methods(&self, kind: &str) -> Result<(), Failure> {
        self.services
            .check_d3d9_methods(kind)
            .map_err(|_| 0x8007000e.into())
    }
    fn register(&mut self, m: Method) -> Result<u32, Failure> {
        self.services
            .register_d3d9(m)
            .map(|p| p as u32)
            .map_err(|_| 0x8007000e.into())
    }
    fn window(&self, h: u32) -> Option<[i32; 2]> {
        self.user.windows.get(&u64::from(h)).map(|w| {
            [
                w.client[2].saturating_sub(w.client[0]),
                w.client[3].saturating_sub(w.client[1]),
            ]
        })
    }
    fn present(&mut self, h: u32, w: u32, height: u32, pixels: &[u32]) -> Result<(), Failure> {
        self.user
            .gdi
            .upload(u64::from(h), w, height, pixels)
            .map_err(|_| 0x8007000e.into())
    }
}
pub(super) fn dispatch(
    method: Method,
    arguments: &[u64; 16],
    memory: &mut Memory,
    image: &mut Loaded,
    services: &mut Services,
) -> Result<u64, ServiceError> {
    let args = arguments.map(|n| n as u32);
    match image.d3d9.call(
        method.name,
        &args,
        &mut Access {
            memory,
            heaps: &mut image.heaps,
            services,
            user: &mut image.user,
        },
    ) {
        Ok(value) => Ok(u64::from(value)),
        Err(Failure::Hresult(value)) => Ok(if method.name == "Direct3DCreate9" {
            0
        } else {
            u64::from(value)
        }),
        Err(Failure::Service(error)) => Err(error),
    }
}
