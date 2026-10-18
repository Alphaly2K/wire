//! PE32 object, raster and logical-font bindings; Win64 retains its absence.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};
pub use crate::graphics::GdiOperation as GdiApi;

const fn declaration(
    id: GdiApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::GdiOperation(id), name, parameters, returns,
        "retain existing checked bitmap/font ownership and selection, DIB memory lifetime and pixel/transfer limits; font handling precedes bitmap handling, ANSI/Wide layouts and CP932 remain distinct; preserve error precedence, COLORREF versus BOOL errors, DIB diagnostics and handler-before-return-slot reads; Win64 absent",
        [ArchitecturePlan { dlls: &["gdi32.dll"], execution: ExecutionPolicy::Typed },
         ArchitecturePlan { dlls: &[], execution: ExecutionPolicy::Absent }],
    )
}
use Parameter::{Int32 as I, Pointer as P, Uint32 as U};
use ReturnConvention::{BoolLastError as B, Pointer as H, Uint32 as D};
pub static GDI_OPERATION_CONTRACTS: [Contract; 31] = [
    declaration(
        GdiApi::CompatibleBitmap,
        "CreateCompatibleBitmap",
        &[P, I, I],
        H,
    ),
    declaration(
        GdiApi::DibSection,
        "CreateDIBSection",
        &[P, P, U, P, P, U],
        H,
    ),
    declaration(GdiApi::Delete, "DeleteObject", &[P], B),
    declaration(GdiApi::Select, "SelectObject", &[P, P], H),
    declaration(GdiApi::ObjectA, "GetObjectA", &[P, I, P], D),
    declaration(GdiApi::ObjectW, "GetObjectW", &[P, I, P], D),
    declaration(GdiApi::ObjectType, "GetObjectType", &[P], D),
    declaration(GdiApi::CurrentObject, "GetCurrentObject", &[P, U], H),
    declaration(GdiApi::StockObject, "GetStockObject", &[I], H),
    declaration(GdiApi::SetPixel, "SetPixel", &[P, I, I, U], D),
    declaration(GdiApi::SetPixelV, "SetPixelV", &[P, I, I, U], B),
    declaration(GdiApi::GetPixel, "GetPixel", &[P, I, I], D),
    declaration(GdiApi::Blit, "BitBlt", &[P, I, I, I, I, P, I, I, U], B),
    declaration(GdiApi::SetWindowOrigin, "SetWindowOrgEx", &[P, I, I, P], B),
    declaration(
        GdiApi::SetViewportOrigin,
        "SetViewportOrgEx",
        &[P, I, I, P],
        B,
    ),
    declaration(GdiApi::GetWindowOrigin, "GetWindowOrgEx", &[P, P], B),
    declaration(GdiApi::GetViewportOrigin, "GetViewportOrgEx", &[P, P], B),
    declaration(GdiApi::SetTextColor, "SetTextColor", &[P, U], D),
    declaration(GdiApi::GetTextColor, "GetTextColor", &[P], D),
    declaration(GdiApi::SetBackgroundColor, "SetBkColor", &[P, U], D),
    declaration(GdiApi::GetBackgroundColor, "GetBkColor", &[P], D),
    declaration(GdiApi::Flush, "GdiFlush", &[], B),
    declaration(
        GdiApi::StretchDib,
        "StretchDIBits",
        &[P, I, I, I, I, I, I, I, I, P, P, U, U],
        D,
    ),
    declaration(GdiApi::GetDib, "GetDIBits", &[P, P, U, U, P, P, U], D),
    declaration(
        GdiApi::FontA,
        "CreateFontA",
        &[I, I, I, I, I, U, U, U, U, U, U, U, U, P],
        H,
    ),
    declaration(
        GdiApi::FontW,
        "CreateFontW",
        &[I, I, I, I, I, U, U, U, U, U, U, U, U, P],
        H,
    ),
    declaration(GdiApi::FontIndirectA, "CreateFontIndirectA", &[P], H),
    declaration(GdiApi::FontIndirectW, "CreateFontIndirectW", &[P], H),
    declaration(GdiApi::TextMetricsW, "GetTextMetricsW", &[P, P], B),
    declaration(GdiApi::SetMapMode, "SetMapMode", &[P, I], D),
    declaration(GdiApi::GetMapMode, "GetMapMode", &[P], D),
];
