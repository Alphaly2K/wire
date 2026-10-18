//! Cursor state and the existing architecture-specific pointer facilities.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorApi {
    Show,
    Get,
    Set,
    GetPosition,
    SetPosition,
    Clip,
    GetClip,
    EnablePointer,
    PointerEnabled,
    Track,
}

const fn declaration(
    id: CursorApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    let pe32 = matches!(
        id,
        CursorApi::Show
            | CursorApi::Get
            | CursorApi::Set
            | CursorApi::GetPosition
            | CursorApi::SetPosition
    );
    let win64 = !matches!(id, CursorApi::SetPosition);
    Contract::new(
        ApiId::Cursor(id), name, parameters, returns,
        "existing per-thread saturating display count and cursor owner; PE32 stock arrow, unclipped signed position and five ABI-only facilities; Win64 full image handles, virtual clipping, no pointer promotion/hover delivery and absent position setter; preserve errors/output/return-frame order and static-only kernel providers",
        [ArchitecturePlan { dlls: &["user32.dll"], execution: if pe32 { ExecutionPolicy::Typed } else { ExecutionPolicy::LegacyUnimplemented } },
         ArchitecturePlan { dlls: if !win64 { &[] } else if matches!(id, CursorApi::Track) { &["user32.dll"] } else { &["kernel32.dll", "kernelbase.dll", "user32.dll"] }, execution: if win64 { ExecutionPolicy::Typed } else { ExecutionPolicy::Absent } }],
    ).with_lookup_dlls([&["user32.dll"], if win64 { &["user32.dll"] } else { &[] }])
}

use Parameter::{Int32 as I, Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Pointer as R, Uint32 as U};
pub static CURSOR_CONTRACTS: [Contract; 10] = [
    declaration(CursorApi::Show, "ShowCursor", &[D], U),
    declaration(CursorApi::Get, "GetCursor", &[], R),
    declaration(CursorApi::Set, "SetCursor", &[P], R),
    declaration(CursorApi::GetPosition, "GetCursorPos", &[P], B),
    declaration(CursorApi::SetPosition, "SetCursorPos", &[I, I], B),
    declaration(CursorApi::Clip, "ClipCursor", &[P], B),
    declaration(CursorApi::GetClip, "GetClipCursor", &[P], B),
    declaration(CursorApi::EnablePointer, "EnableMouseInPointer", &[D], B),
    declaration(CursorApi::PointerEnabled, "IsMouseInPointerEnabled", &[], B),
    declaration(CursorApi::Track, "TrackMouseEvent", &[P], B),
];
