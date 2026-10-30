//! Optional graphics interfaces expose their actual unavailable-backend protocol.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphicsProbeApi {
    CreateContext,
    CreateLayerContext,
    DeleteContext,
    CurrentContext,
    CurrentDc,
    Extension,
    MakeCurrent,
    CopyContext,
    ShareLists,
    SwapLayers,
    ChooseFormat,
    SetFormat,
    DescribeFormat,
    SwapBuffers,
    DxgiDebug,
    DxgiDebug1,
    D3d11On12,
    SetWindowAttribute,
}
impl GraphicsProbeApi {
    pub(crate) const fn dlls(self) -> &'static [&'static str] {
        use GraphicsProbeApi::*;
        match self {
            ChooseFormat | SetFormat | DescribeFormat | SwapBuffers => &["gdi32.dll"],
            DxgiDebug => &["dxgidebug.dll"],
            DxgiDebug1 => &["dxgi.dll"],
            D3d11On12 => &["d3d11.dll"],
            SetWindowAttribute => &["dwmapi.dll"],
            _ => &["opengl32.dll"],
        }
    }
}
const fn declaration(
    id: GraphicsProbeApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::GraphicsProbe(id), name, parameters, returns,
        "No OpenGL rendering, pixel formats, D3D11-on-12, DXGI debug interface or DWM compositor; current GL identities and unassigned pixel format are zero, null rendering-context release succeeds, other operations report explicit errors; full-width handles, original call frames, checked failure outputs and LastError policy",
        [ArchitecturePlan { dlls: id.dlls(), execution: ExecutionPolicy::LegacyUnimplemented }; 2],
    )
}
use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Hresult as H, Pointer as R, Uint32 as U};
pub static GRAPHICS_PROBE_CONTRACTS: [Contract; 18] = [
    declaration(GraphicsProbeApi::CreateContext, "wglCreateContext", &[P], R),
    declaration(
        GraphicsProbeApi::CreateLayerContext,
        "wglCreateLayerContext",
        &[P, D],
        R,
    ),
    declaration(GraphicsProbeApi::DeleteContext, "wglDeleteContext", &[P], B),
    declaration(
        GraphicsProbeApi::CurrentContext,
        "wglGetCurrentContext",
        &[],
        R,
    ),
    declaration(GraphicsProbeApi::CurrentDc, "wglGetCurrentDC", &[], R),
    declaration(GraphicsProbeApi::Extension, "wglGetProcAddress", &[P], R),
    declaration(GraphicsProbeApi::MakeCurrent, "wglMakeCurrent", &[P, P], B),
    declaration(
        GraphicsProbeApi::CopyContext,
        "wglCopyContext",
        &[P, P, D],
        B,
    ),
    declaration(GraphicsProbeApi::ShareLists, "wglShareLists", &[P, P], B),
    declaration(
        GraphicsProbeApi::SwapLayers,
        "wglSwapLayerBuffers",
        &[P, D],
        B,
    ),
    declaration(
        GraphicsProbeApi::ChooseFormat,
        "ChoosePixelFormat",
        &[P, P],
        U,
    ),
    declaration(GraphicsProbeApi::SetFormat, "SetPixelFormat", &[P, D, P], B),
    declaration(
        GraphicsProbeApi::DescribeFormat,
        "DescribePixelFormat",
        &[P, D, D, P],
        U,
    ),
    declaration(GraphicsProbeApi::SwapBuffers, "SwapBuffers", &[P], B),
    declaration(
        GraphicsProbeApi::DxgiDebug,
        "DXGIGetDebugInterface",
        &[P, P],
        H,
    ),
    declaration(
        GraphicsProbeApi::DxgiDebug1,
        "DXGIGetDebugInterface1",
        &[D, P, P],
        H,
    ),
    declaration(
        GraphicsProbeApi::D3d11On12,
        "D3D11On12CreateDevice",
        &[P, D, P, D, P, D, D, P, P, P],
        H,
    ),
    declaration(
        GraphicsProbeApi::SetWindowAttribute,
        "DwmSetWindowAttribute",
        &[P, D, P, D],
        H,
    ),
];
