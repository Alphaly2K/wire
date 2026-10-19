//! PE32 rectangular update regions and paint DC callback boundaries.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintingApi {
    Invalidate,
    Validate,
    UpdateRect,
    Begin,
    End,
}
const fn declaration(
    id: PaintingApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::Painting(id), name, parameters, returns,
        "PE32 rectangular regions only; HWND validation precedes guest pointer access; BeginPaint checks the full output before consuming the region and invoking erase, writes only after normal callback return; handler output precedes gateway return-frame read; EndPaint retains separately invalidated regions; GetUpdateRect retains its erase behavior; Win64 exports absent; RedrawWindow remains in the menu gateway",
        [ArchitecturePlan { dlls: &["user32.dll"], execution: ExecutionPolicy::Typed },
         ArchitecturePlan { dlls: &[], execution: ExecutionPolicy::Absent }],
    )
}
use Parameter::{Pointer as P, Uint32 as U};
use ReturnConvention::{BoolLastError as B, Pointer as H};
pub static PAINTING_CONTRACTS: [Contract; 5] = [
    declaration(PaintingApi::Invalidate, "InvalidateRect", &[P, P, U], B),
    declaration(PaintingApi::Validate, "ValidateRect", &[P, P], B),
    declaration(PaintingApi::UpdateRect, "GetUpdateRect", &[P, P, U], B),
    declaration(PaintingApi::Begin, "BeginPaint", &[P, P], H),
    declaration(PaintingApi::End, "EndPaint", &[P, P], B),
];
