//! Font installation endpoints retain their explicit unavailable policy.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontResourceApi {
    AddMemory,
    RemoveMemory,
    AddA,
    AddW,
    RemoveA,
    RemoveW,
}
const fn declaration(
    id: FontResourceApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(ApiId::FontResource(id), name, parameters, returns,
        "PE32 gdi32 only; no font resource backend, retain zero return, error 120, original unimplemented note, ignored pointers and unchanged outputs; Win64 absent; logical font creation remains separate",
        [ArchitecturePlan { dlls: &["gdi32.dll"], execution: ExecutionPolicy::Typed },
         ArchitecturePlan { dlls: &[], execution: ExecutionPolicy::Absent }])
}
use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Pointer as H, Uint32 as U};
pub static FONT_RESOURCE_CONTRACTS: [Contract; 6] = [
    declaration(
        FontResourceApi::AddMemory,
        "AddFontMemResourceEx",
        &[P, D, P, P],
        H,
    ),
    declaration(
        FontResourceApi::RemoveMemory,
        "RemoveFontMemResourceEx",
        &[P],
        B,
    ),
    declaration(FontResourceApi::AddA, "AddFontResourceExA", &[P, D, P], U),
    declaration(FontResourceApi::AddW, "AddFontResourceExW", &[P, D, P], U),
    declaration(
        FontResourceApi::RemoveA,
        "RemoveFontResourceExA",
        &[P, D, P],
        B,
    ),
    declaration(
        FontResourceApi::RemoveW,
        "RemoveFontResourceExW",
        &[P, D, P],
        B,
    ),
];
