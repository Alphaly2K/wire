//! DC identity, lifecycle and architecture-specific device capabilities.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GdiDcApi {
    Window,
    Release,
    WindowFrame,
    Compatible,
    Delete,
    Capabilities,
    PixelFormat,
}

const fn declaration(
    id: GdiDcApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    let user = matches!(
        id,
        GdiDcApi::Window | GdiDcApi::Release | GdiDcApi::WindowFrame
    );
    let win64 = !matches!(id, GdiDcApi::WindowFrame);
    Contract::new(
        ApiId::GdiDc(id), name, parameters, returns,
        "preserve PE32 Graphics backing and Win64 full-handle/thread/window/compatible DC ownership, monotonic handle bounds and allocation order; architecture-specific capability profiles and failure/LastError/return-frame ordering; PE32 GetWindowDC/GetPixelFormat remain ABI-only, Win64 GetWindowDC absent, GetPixelFormat reports no selected format; kernel provider aliases remain static-only",
        [ArchitecturePlan { dlls: if user { &["user32.dll"] } else { &["gdi32.dll"] }, execution: if matches!(id, GdiDcApi::WindowFrame | GdiDcApi::PixelFormat) { ExecutionPolicy::LegacyUnimplemented } else { ExecutionPolicy::Typed } },
         ArchitecturePlan { dlls: if !win64 { &[] } else if user { &["kernel32.dll", "kernelbase.dll", "user32.dll"] } else { &["kernel32.dll", "kernelbase.dll", "user32.dll", "gdi32.dll"] }, execution: if win64 { ExecutionPolicy::Typed } else { ExecutionPolicy::Absent } }],
    ).with_lookup_dlls([
        if user { &["user32.dll"] } else { &["gdi32.dll"] },
        if !win64 { &[] } else if user { &["user32.dll"] } else { &["user32.dll", "gdi32.dll"] },
    ])
}

use Parameter::{Int32 as I, Pointer as P};
use ReturnConvention::{Pointer as H, Uint32 as U};
pub static GDI_DC_CONTRACTS: [Contract; 7] = [
    declaration(GdiDcApi::Window, "GetDC", &[P], H),
    declaration(GdiDcApi::Release, "ReleaseDC", &[P, P], U),
    declaration(GdiDcApi::WindowFrame, "GetWindowDC", &[P], H),
    declaration(GdiDcApi::Compatible, "CreateCompatibleDC", &[P], H),
    declaration(GdiDcApi::Delete, "DeleteDC", &[P], U),
    declaration(GdiDcApi::Capabilities, "GetDeviceCaps", &[P, I], U),
    declaration(GdiDcApi::PixelFormat, "GetPixelFormat", &[P], U),
];
