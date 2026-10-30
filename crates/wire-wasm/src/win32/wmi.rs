//! PE32 WMI reuses the portable object owner and the process automation heap.
use super::{
    automation::{Access32, Failure as AutomationFailure},
    heaps, wmi_core, ServiceError, Services,
};
use crate::{
    memory::{Fault, Memory, READ, WRITE},
    process::Loaded,
};
pub(crate) use wmi_core::{Method, Wmi};

enum Failure {
    Hresult(u32),
    Service(ServiceError),
}
impl From<u32> for Failure {
    fn from(code: u32) -> Self {
        Self::Hresult(code)
    }
}
impl From<Fault> for Failure {
    fn from(fault: Fault) -> Self {
        Self::Service(fault.into())
    }
}
fn heap_error(error: heaps::Error) -> Failure {
    match error {
        heaps::Error::Windows(8) => Failure::Hresult(0x8007000e),
        heaps::Error::Windows(_) | heaps::Error::Backing => {
            Failure::Service(ServiceError::Unavailable("WMI process allocation was lost"))
        }
        heaps::Error::Fault(fault) => fault.into(),
    }
}
struct Access<'a> {
    memory: &'a mut Memory,
    heaps: &'a mut heaps::Heaps,
    bstrs: &'a mut super::automation32::Bstrings,
    services: &'a mut Services,
}
impl wmi_core::Backend for Access<'_> {
    type Error = Failure;
    fn check(&self, p: u32, n: usize, write: bool) -> Result<(), Failure> {
        if p == 0 {
            return Err(Failure::Hresult(0x80004003));
        }
        Ok(self
            .memory
            .check(u64::from(p), n, if write { WRITE } else { READ })?)
    }
    fn read(&self, p: u32, bytes: &mut [u8]) -> Result<(), Failure> {
        if p == 0 {
            return Err(Failure::Hresult(0x80004003));
        }
        Ok(self.memory.read(u64::from(p), bytes)?)
    }
    fn write(&mut self, p: u32, bytes: &[u8]) -> Result<(), Failure> {
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
    fn check_methods(&self, kind: &'static str) -> Result<(), Failure> {
        self.services
            .check_wmi_methods(kind)
            .map_err(|_| Failure::Hresult(0x8007000e))
    }
    fn register(&mut self, method: Method) -> Result<u32, Failure> {
        self.services
            .register_wmi(method)
            .map(|p| p as u32)
            .map_err(|_| Failure::Hresult(0x8007000e))
    }
    fn bstr(&mut self, text: &[u16]) -> Result<u32, Failure> {
        self.bstrs
            .allocate(
                &mut Access32 {
                    memory: self.memory,
                    heaps: self.heaps,
                },
                text,
            )
            .map_err(|error| match error {
                AutomationFailure::Service(error) => Failure::Service(error),
                AutomationFailure::Windows(8) => Failure::Hresult(0x8007000e),
                AutomationFailure::Windows(code) => Failure::Hresult(code),
            })
    }
}
pub(super) fn dispatch(
    method: Option<Method>,
    arguments: &[u64; 16],
    memory: &mut Memory,
    image: &mut Loaded,
    services: &mut Services,
) -> Result<u64, ServiceError> {
    let args = arguments.map(|n| n as u32);
    let mut access = Access {
        memory,
        heaps: &mut image.heaps,
        bstrs: &mut image.bstrings32,
        services,
    };
    let result = if let Some(method) = method {
        image.wmi.call(method, &args, &mut access)
    } else {
        image.wmi.activate(&args, &mut access)
    };
    match result {
        Ok(value) => Ok(u64::from(value)),
        Err(Failure::Hresult(value)) => {
            if method.is_none() && args[4] != 0 {
                access.memory.write_int(u64::from(args[4]), 4, 0)?;
            }
            Ok(u64::from(value))
        }
        Err(Failure::Service(error)) => Err(error),
    }
}
