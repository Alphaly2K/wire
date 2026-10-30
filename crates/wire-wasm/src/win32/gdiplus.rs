//! PE32 GDI+ uses the shared matrix owner with current process allocations.
use super::{gdiplus_core, heaps, ServiceError};
use crate::{
    memory::{Fault, Memory, READ, WRITE},
    process::Loaded,
};
pub(crate) use gdiplus_core::{Export, GdiPlus};

enum Failure {
    Status(u32),
    Service(ServiceError),
}
impl From<u32> for Failure {
    fn from(status: u32) -> Self {
        Self::Status(status)
    }
}
impl From<Fault> for Failure {
    fn from(fault: Fault) -> Self {
        Self::Service(fault.into())
    }
}
fn heap_error(error: heaps::Error) -> Failure {
    match error {
        heaps::Error::Windows(8) => Failure::Status(3),
        heaps::Error::Fault(fault) => fault.into(),
        heaps::Error::Windows(_) | heaps::Error::Backing => Failure::Service(
            ServiceError::Unavailable("GDI+ process allocation was lost"),
        ),
    }
}
struct Access<'a> {
    memory: &'a mut Memory,
    heaps: &'a mut heaps::Heaps,
}
impl gdiplus_core::Backend for Access<'_> {
    type Error = Failure;
    fn check(&self, p: u32, n: usize, write: bool) -> Result<(), Failure> {
        Ok(self
            .memory
            .check(u64::from(p), n, if write { WRITE } else { READ })?)
    }
    fn read_u32(&self, p: u32) -> Result<u32, Failure> {
        Ok(self.memory.read_int(u64::from(p), 4)? as u32)
    }
    fn write_u32(&mut self, p: u32, value: u32) -> Result<(), Failure> {
        Ok(self.memory.write_int(u64::from(p), 4, u64::from(value))?)
    }
    fn write_bytes(&mut self, p: u32, bytes: &[u8]) -> Result<(), Failure> {
        Ok(self.memory.write(u64::from(p), bytes)?)
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
}
pub(super) fn dispatch(
    export: Export,
    arguments: &[u64; 16],
    memory: &mut Memory,
    image: &mut Loaded,
) -> Result<u64, ServiceError> {
    let args = arguments.map(|n| n as u32);
    let result = image.gdiplus.call(
        export.name,
        &args,
        &mut Access {
            memory,
            heaps: &mut image.heaps,
        },
    );
    match result {
        Ok(value) => Ok(u64::from(value)),
        Err(Failure::Status(value)) => Ok(if export.name == "GdipAlloc" {
            0
        } else {
            u64::from(value)
        }),
        Err(Failure::Service(error)) => Err(error),
    }
}
