//! COM and WinRT share the active thread's apartment and retain HRESULT failures.
use super::{
    apartment::InitializeError,
    call::Call,
    catalog::{ApiId, ComApi as C},
    heaps, ServiceError, Services,
};
use crate::{
    memory::{Memory, WRITE},
    process::Loaded,
};

pub(super) fn implemented(id: ApiId) -> bool {
    matches!(
        id,
        ApiId::Com(
            C::Initialize
                | C::InitializeEx
                | C::Uninitialize
                | C::WinrtInitialize
                | C::WinrtUninitialize
                | C::TaskAlloc
                | C::TaskFree
                | C::TaskRealloc
                | C::CreateInstance
                | C::PropertyClear
                | C::PropertyCopy
                | C::FreeMarshaler
                | C::ProxyBlanket
                | C::AgileReference
        )
    )
}
pub(super) fn dlls(id: C) -> &'static [&'static str] {
    match id {
        C::WinrtInitialize | C::WinrtUninitialize => {
            &["combase.dll", "api-ms-win-core-winrt-l1-1-0.dll"]
        }
        C::Initialize | C::OleInitialize | C::OleUninitialize => &["ole32.dll"],
        _ => &["ole32.dll", "combase.dll"],
    }
}
pub(super) fn allocation(result: Result<u64, heaps::Error>) -> Result<u64, ServiceError> {
    match result {
        Ok(pointer) => Ok(pointer),
        Err(heaps::Error::Windows(8)) => Ok(0),
        Err(heaps::Error::Fault(fault)) => Err(fault.into()),
        Err(_) => Err(ServiceError::Unavailable(
            "invalid COM task allocation or backing",
        )),
    }
}
pub(super) fn dispatch(
    id: C,
    call: &Call,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
    services: &mut Services,
) -> Result<u64, ServiceError> {
    let image = image.ok_or(ServiceError::Unavailable(
        "COM services require a loaded image",
    ))?;
    let a = &call.arguments;
    Ok(match id {
        C::Initialize | C::InitializeEx | C::WinrtInitialize => {
            let winrt = id == C::WinrtInitialize;
            let flags = if id == C::Initialize {
                2
            } else if winrt {
                if a[0] == 0 {
                    2
                } else {
                    0
                }
            } else {
                a[1] as u32
            };
            if (winrt && a[0] > 1) || (!winrt && (a[0] != 0 || flags & !14 != 0)) {
                0x80070057
            } else {
                match image
                    .threads
                    .current_mut()
                    .apartment
                    .initialize(flags & 2, false)
                {
                    Ok(status) => status.into(),
                    Err(InitializeError::ChangedMode) => 0x80010106,
                    Err(InitializeError::Overflow) => 0x8007000e,
                }
            }
        }
        C::Uninitialize | C::WinrtUninitialize => {
            image.threads.current_mut().apartment.uninitialize(false);
            0
        }
        C::TaskAlloc => allocation(image.heaps.allocate_task(memory, a[0], None))?,
        C::TaskRealloc => allocation(image.heaps.realloc_task(memory, a[0], a[1]))?,
        C::TaskFree => {
            allocation(image.heaps.free_task(memory, a[0]).map(|()| 0))?;
            0
        }
        C::PropertyClear | C::PropertyCopy => {
            return super::property_values::dispatch(id == C::PropertyCopy, a, memory, image);
        }
        C::ProxyBlanket => {
            if a[0] == 0 {
                0x80070057
            } else {
                0x80004001
            }
        }
        C::FreeMarshaler | C::AgileReference => {
            let output = a[if id == C::FreeMarshaler { 1 } else { 3 }];
            if output == 0 {
                return Ok(0x80004003);
            }
            let result = if id == C::AgileReference && (a[0] > 1 || a[1] == 0 || a[2] == 0) {
                0x80070057
            } else {
                0x80004001
            };
            memory.write_int(output, memory.mode() as usize / 8, 0)?;
            result
        }
        C::CreateInstance => {
            if a[4] == 0 {
                return Ok(0x80004003);
            }
            if a[0] == 0 || a[3] == 0 {
                return Ok(0x80070057);
            }
            let mut clsid = [0; 16];
            let mut iid = [0; 16];
            memory.read(a[0], &mut clsid)?;
            memory.read(a[3], &mut iid)?;
            memory.check(a[4], memory.mode() as usize / 8, WRITE)?;
            if memory.mode() == crate::machine::Mode::X86
                && image.threads.current().apartment.initialized()
                && a[2] as u32 != 0
            {
                return super::wmi::dispatch(None, a, memory, image, services);
            }
            let result = if !image.threads.current().apartment.initialized() {
                0x800401f0
            } else if a[2] as u32 == 0 {
                0x80070057
            } else {
                0x80040154
            };
            memory.write_int(a[4], memory.mode() as usize / 8, 0)?;
            result
        }
        C::OleInitialize | C::OleUninitialize => unreachable!("OLE services are not admitted"),
    })
}
