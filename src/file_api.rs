/* PathRemoveFileSpec, PathAddExtension and PathUnquoteSpaces A/W adapted from
 * Hangover's Wine kernelbase/path.c,
 * commit 3ea3a14941b6a122b892c84141ce9662c008081e (LGPL-2.1-or-later).
 * Copyright 2018 Nikolay Sivov; 2018 Zhiyi Zhang;
 * Copyright 2021-2023 Zebediah Figura. Wire selects LGPL 2.1.
 * See third_party/hangover/path.c and COPYING.LIB for original notices.
 */
//! Public compatibility facade for PE32 file/path APIs.
use crate::api::ApiState;
use crate::mem::GuestMem;
use crate::win32::catalog::{FileApi, PathApi};
pub use crate::win32::x86::files::{read_name, write_string};

pub fn dispatch(
    dll: &str,
    name: &str,
    a: &crate::api::Args,
    mem: &GuestMem,
    state: &mut ApiState,
) -> Option<(u32, String)> {
    use crate::win32::catalog::{resolve, ApiId, Architecture, ExecutionPolicy, Resolution};
    // Preserve the public facade's exact, lowercase provider scope.
    if !matches!(dll, "kernel32.dll" | "shlwapi.dll" | "imagehlp.dll") {
        return None;
    }
    let Resolution::Bound(contract) = resolve(Architecture::X86, dll, name) else {
        return None;
    };
    if contract.execution(Architecture::X86) != ExecutionPolicy::Typed {
        return None;
    }
    match contract.id {
        ApiId::File(id) if id != FileApi::GetFileType => Some(call(id, a, mem, state)),
        ApiId::Path(id) => Some(call_path(id, a, mem, state)),
        _ => None,
    }
}

/// Bound IDs bypass the public facade's cold provider/name resolution.
pub(crate) fn call(
    id: FileApi,
    a: &crate::api::Args,
    mem: &GuestMem,
    state: &mut ApiState,
) -> (u32, String) {
    crate::win32::x86::files::call(
        id,
        a,
        mem,
        &mut state.handles,
        &mut state.fs,
        &mut state.last_error,
    )
}
pub(crate) fn call_path(
    id: PathApi,
    a: &crate::api::Args,
    mem: &GuestMem,
    state: &mut ApiState,
) -> (u32, String) {
    crate::win32::x86::files::call_path(
        id,
        a,
        mem,
        &mut state.handles,
        &mut state.fs,
        &mut state.last_error,
    )
}
