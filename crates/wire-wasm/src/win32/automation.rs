use super::{
    automation32::Backend as Backend32,
    automation64::Backend as Backend64,
    call::Call,
    catalog::{ApiId, BstrApi},
    context, data_pages, heaps, ServiceError,
};
use crate::{
    machine::{Mode, State},
    memory::{Fault, Memory, READ, WRITE},
    process::Loaded,
};

pub(super) enum Failure {
    Windows(u32),
    Service(ServiceError),
}
impl From<Fault> for Failure {
    fn from(f: Fault) -> Self {
        Self::Service(f.into())
    }
}
fn heap_failure(error: heaps::Error) -> Failure {
    match error {
        heaps::Error::Windows(code) => Failure::Windows(code),
        heaps::Error::Fault(f) => f.into(),
        heaps::Error::Backing => Failure::Service(ServiceError::Unavailable(
            "automation heap backing was lost",
        )),
    }
}
pub(super) struct Access32<'a> {
    pub(super) memory: &'a mut Memory,
    pub(super) heaps: &'a mut heaps::Heaps,
}

pub(super) enum StringOperation {
    Duplicate(Option<u64>),
    Check(u64, u64, usize),
    Free(u64),
}

pub(super) fn string(
    operation: StringOperation,
    memory: &mut Memory,
    image: &mut Loaded,
) -> Result<u64, Failure> {
    if memory.mode() == Mode::X86 {
        let mut access = Access32 {
            memory,
            heaps: &mut image.heaps,
        };
        match operation {
            StringOperation::Duplicate(source) => image.bstrings32.duplicate(&mut access, source),
            StringOperation::Check(p, output, size) => image
                .bstrings32
                .check_string(&access, p as u32, output as u32, size as u32)
                .map(|_| 0),
            StringOperation::Free(p) => image
                .bstrings32
                .free_string(&mut access, p as u32)
                .map(|_| 0),
        }
    } else {
        let mut access = Access64 {
            memory,
            pages: &mut image.data_pages,
            error: None,
        };
        match operation {
            StringOperation::Duplicate(source) => image.bstrings64.duplicate(&mut access, source),
            StringOperation::Check(p, output, size) => image
                .bstrings64
                .check_string(&access, p, output, size)
                .map(|_| 0),
            StringOperation::Free(p) => image.bstrings64.free_string(&mut access, p).map(|_| 0),
        }
    }
}
impl Backend32 for Access32<'_> {
    type Error = Failure;
    fn check(&self, p: u32, n: usize, write: bool, _: u32) -> Result<(), Failure> {
        Ok(self
            .memory
            .check(u64::from(p), n, if write { WRITE } else { READ })?)
    }
    fn read(&self, p: u32, bytes: &mut [u8], _: u32) -> Result<(), Failure> {
        Ok(self.memory.read(u64::from(p), bytes)?)
    }
    fn write(&mut self, p: u32, bytes: &[u8], _: u32) -> Result<(), Failure> {
        Ok(self.memory.write(u64::from(p), bytes)?)
    }
    fn allocate(&mut self, n: u32) -> Result<u32, Failure> {
        self.heaps
            .allocate_local(self.memory, u64::from(n), &[])
            .map(|p| p as u32)
            .map_err(heap_failure)
    }
    fn check_release(&self, base: u32) -> Result<(), Failure> {
        self.heaps
            .check_process_free(self.memory, u64::from(base))
            .map_err(heap_failure)
    }
    fn free(&mut self, base: u32) -> Result<bool, Failure> {
        self.heaps
            .free_process(self.memory, u64::from(base))
            .map(|_| true)
            .map_err(heap_failure)
    }
    fn replacement(&self, base: u32, output: u32, length: u32) -> Result<(), Failure> {
        let base = u64::from(base);
        let size = self.heaps.process_span(base).ok_or(Failure::Windows(87))?;
        if u64::from(output) < base + size && u64::from(output) + u64::from(length) > base {
            return Err(Failure::Service(ServiceError::Unavailable(
                "BSTR output overlaps allocation being replaced",
            )));
        }
        Ok(())
    }
    fn windows(code: u32) -> Failure {
        Failure::Windows(code)
    }
    fn scalar(&self, vt: u16) -> bool {
        matches!(vt, 0..=7 | 10..=11 | 14 | 16..=23)
    }
}
struct Access64<'a> {
    memory: &'a mut Memory,
    pages: &'a mut data_pages::Pages,
    error: Option<u64>,
}
impl Backend64 for Access64<'_> {
    type Error = Failure;
    fn check_write(&self, p: u64, n: usize) -> Result<(), Failure> {
        Ok(self.memory.check(p, n, WRITE)?)
    }
    fn read(&self, p: u64, bytes: &mut [u8]) -> Result<(), Failure> {
        Ok(self.memory.read(p, bytes)?)
    }
    fn write(&mut self, p: u64, bytes: &[u8]) -> Result<(), Failure> {
        Ok(self.memory.write(p, bytes)?)
    }
    fn allocate(&mut self, n: u64) -> Result<u64, Failure> {
        let p = self.pages.allocate(self.memory, n);
        if p == 0 {
            if let Some(error) = self.error {
                self.memory.write_int(error, 4, 8)?;
            }
        }
        Ok(p)
    }
    fn span(&self, base: u64) -> Option<u64> {
        self.pages.span(base)
    }
    fn check_free(&self, base: u64) -> Result<(), Failure> {
        self.pages
            .check_free(self.memory, base)
            .map_err(Failure::Service)
    }
    fn free(&mut self, base: u64) -> Result<(), Failure> {
        self.pages.free(self.memory, base).map_err(Failure::Service)
    }
    fn invalid(&self, message: &'static str) -> Failure {
        Failure::Service(ServiceError::Unavailable(message))
    }
    fn windows(code: u32) -> Failure {
        Failure::Windows(code)
    }
}
pub(super) fn dlls(mode: Mode, id: ApiId) -> &'static [&'static str] {
    match id {
        ApiId::Bstr(BstrApi::AllocateBytes) if mode == Mode::X86 => &[],
        ApiId::Bstr(_) => &["oleaut32.dll"],
        ApiId::Variant(_) => &["oleaut32.dll"],
        _ => &[],
    }
}
pub(super) fn dispatch(
    id: ApiId,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
) -> Result<u64, ServiceError> {
    let image = image.ok_or(ServiceError::Unavailable(
        "automation requires a loaded image",
    ))?;
    let error = context::error_address(state, memory.mode());
    if let ApiId::Variant(id) = id {
        match super::safe_arrays::variant(id, &call.arguments, memory, image) {
            Ok(Some(value)) => return Ok(value),
            Ok(None) => {}
            Err(super::safe_arrays::Error::Service(error)) => return Err(error),
            Err(super::safe_arrays::Error::Hresult(code)) => {
                memory.write_int(error, 4, u64::from(code))?;
                return Ok(u64::from(code));
            }
        }
    }
    let result = if memory.mode() == Mode::X64 {
        let mut access = Access64 {
            memory,
            pages: &mut image.data_pages,
            error: Some(error),
        };
        match id {
            ApiId::Bstr(id) => image.bstrings64.call(&mut access, id, &call.arguments),
            ApiId::Variant(id) => image.bstrings64.variant(&mut access, id, &call.arguments),
            _ => unreachable!(),
        }
    } else {
        let args = call.arguments.map(|a| a as u32);
        let mut access = Access32 {
            memory,
            heaps: &mut image.heaps,
        };
        match id {
            ApiId::Bstr(id) => image.bstrings32.call(&mut access, id, &args),
            ApiId::Variant(id) => image.bstrings32.variant(&mut access, id, &args),
            _ => unreachable!(),
        }
        .map(u64::from)
    };
    match result {
        Ok(value) => Ok(value),
        Err(Failure::Service(error)) => Err(error),
        Err(Failure::Windows(code)) => {
            memory.write_int(error, 4, u64::from(code))?;
            Ok(if matches!(id, ApiId::Variant(_)) {
                u64::from(code)
            } else {
                0
            })
        }
    }
}
