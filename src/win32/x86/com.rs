//! PE32 COM adapters keep HRESULT/LastError and process heap conventions.
use crate::{
    api::Args,
    heap::GuestHeap,
    mem::GuestMem,
    win32::{
        catalog::ComApi,
        com::apartment::{Apartments32, InitializeError},
    },
};

pub(crate) fn apartment_call(
    id: ComApi,
    args: &Args,
    apartments: &mut Apartments32,
    tid: u32,
    last_error: &mut u32,
) -> (u32, String) {
    let result = match id {
        ComApi::Uninitialize => {
            apartments.uninitialize(tid);
            Ok(0)
        }
        ComApi::Initialize | ComApi::InitializeEx => {
            let flags = if id == ComApi::Initialize { 2 } else { args[1] };
            if args[0] != 0 || flags & !0xe != 0 {
                Err(0x80070057)
            } else {
                apartments
                    .initialize(tid, flags & 2)
                    .map_err(|error| match error {
                        InitializeError::ChangedMode => 0x80010106,
                        InitializeError::Overflow => 0x80004005,
                    })
            }
        }
        _ => unreachable!("not a PE32 apartment call"),
    };
    match result {
        Ok(value) => (
            value,
            "common process/string/COM allocation contract".into(),
        ),
        Err(error) => {
            *last_error = error;
            (error, format!("common API error {error:#x}"))
        }
    }
}

pub(crate) fn task_call(
    id: ComApi,
    args: &Args,
    mem: &mut GuestMem,
    heap: &mut GuestHeap,
    process_heap: u32,
    last_error: &mut u32,
) -> (u32, String) {
    let result = match id {
        ComApi::TaskAlloc => heap.alloc(mem, process_heap, args[0], false).map_err(|_| 8),
        ComApi::TaskFree => {
            heap.free(process_heap, args[0]);
            Ok(0)
        }
        _ => unreachable!("not a task allocation call"),
    };
    match result {
        Ok(value) => (value, "code-page / sandbox directory shim".into()),
        Err(error) => {
            *last_error = error;
            (0, format!("Win32 error {error}"))
        }
    }
}
