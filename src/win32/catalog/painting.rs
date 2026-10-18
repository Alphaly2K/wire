//! PE32 update regions, paint DC lifetime and erase-callback coordination.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintApi {
    Invalidate,
    Validate,
    UpdateRect,
    Begin,
    End,
    Redraw,
}

const fn declaration(
    id: PaintApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::Paint(id),
        name,
        parameters,
        returns,
        "retain PE32 HWND/flags/output checks, rectangular update/erase state, DC clip and release, callback/nonlocal transfer and partial commits; complex regions/nonclient paint remain unsupported; six Win64 exports absent",
        [
            ArchitecturePlan { dlls: &["user32.dll"], execution: ExecutionPolicy::Typed },
            ArchitecturePlan { dlls: &[], execution: ExecutionPolicy::Absent },
        ],
    )
}
use Parameter::{Pointer as P, Uint32 as U};
use ReturnConvention::{BoolLastError as B, Pointer as H};
pub static PAINT_CONTRACTS: [Contract; 6] = [
    declaration(PaintApi::Invalidate, "InvalidateRect", &[P, P, U], B),
    declaration(PaintApi::Validate, "ValidateRect", &[P, P], B),
    declaration(PaintApi::UpdateRect, "GetUpdateRect", &[P, P, U], B),
    declaration(PaintApi::Begin, "BeginPaint", &[P, P], H),
    declaration(PaintApi::End, "EndPaint", &[P, P], B),
    declaration(PaintApi::Redraw, "RedrawWindow", &[P, P, P, U], B),
];
