//! Rectangle values and the current 96-DPI nonclient frame profile.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeometryApi {
    Set,
    SetEmpty,
    Copy,
    Equal,
    Inflate,
    Offset,
    Point,
    IsEmpty,
    Intersect,
    Union,
    Adjust,
    AdjustEx,
    AdjustDpi,
}

const fn declaration(
    id: GeometryApi,
    name: &'static str,
    parameters: &'static [Parameter],
) -> Contract {
    let win64 = matches!(
        id,
        GeometryApi::Copy
            | GeometryApi::Offset
            | GeometryApi::Point
            | GeometryApi::Adjust
            | GeometryApi::AdjustEx
            | GeometryApi::AdjustDpi
    );
    Contract::new(
        ApiId::Geometry(id), name, parameters, ReturnConvention::BoolLastError,
        "existing signed RECT arithmetic, wrapping coordinate changes and 96-DPI frame profile; POINT consumes two PE32 stack words and one packed Win64 carrier; preserve empty/alias/output/return-frame order and architecture-specific unsupported-DPI errors; Win64 kernel providers are static-import-only",
        [ArchitecturePlan { dlls: &["user32.dll"], execution: ExecutionPolicy::Typed },
         ArchitecturePlan { dlls: if win64 { &["kernel32.dll", "kernelbase.dll", "user32.dll"] } else { &[] }, execution: if win64 { ExecutionPolicy::Typed } else { ExecutionPolicy::Absent } }],
    ).with_lookup_dlls([&["user32.dll"], if win64 { &["user32.dll"] } else { &[] }])
}

use Parameter::{Int32 as I, Pointer as P, Uint32 as D, Uint64 as Q};
pub static GEOMETRY_CONTRACTS: [Contract; 13] = [
    declaration(GeometryApi::Set, "SetRect", &[P, I, I, I, I]),
    declaration(GeometryApi::SetEmpty, "SetRectEmpty", &[P]),
    declaration(GeometryApi::Copy, "CopyRect", &[P, P]),
    declaration(GeometryApi::Equal, "EqualRect", &[P, P]),
    declaration(GeometryApi::Inflate, "InflateRect", &[P, I, I]),
    declaration(GeometryApi::Offset, "OffsetRect", &[P, I, I]),
    // A by-value POINT occupies 8 bytes on either guest ABI.
    declaration(GeometryApi::Point, "PtInRect", &[P, Q]),
    declaration(GeometryApi::IsEmpty, "IsRectEmpty", &[P]),
    declaration(GeometryApi::Intersect, "IntersectRect", &[P, P, P]),
    declaration(GeometryApi::Union, "UnionRect", &[P, P, P]),
    declaration(GeometryApi::Adjust, "AdjustWindowRect", &[P, D, D]),
    declaration(GeometryApi::AdjustEx, "AdjustWindowRectEx", &[P, D, D, D]),
    declaration(
        GeometryApi::AdjustDpi,
        "AdjustWindowRectExForDpi",
        &[P, D, D, D, D],
    ),
];
