//! Public graphics creation exports; COM slot ABI remains a separate family.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphicsExportApi {
    Factory,
    Factory1,
    Factory2,
    D3d11Device,
    D3d11DeviceAndSwapChain,
    D3d12Device,
    D3d12Debug,
}

const fn declaration(
    id: GraphicsExportApi,
    name: &'static str,
    parameters: &'static [Parameter],
    provider: &'static [&'static str],
    lookup: &'static [&'static str],
) -> Contract {
    Contract::new(
        ApiId::GraphicsExport(id), name, parameters, ReturnConvention::Hresult,
        "PE32 unknown ABI remains absent; Win64 retains legacy kernel32/kernelbase static aliases, exact-case internal wire.dxgi provider, dynamic lookup only in the actual graphics DLL and existing-gateway fallback; checked outputs, object references, optional backend errors and D3D12 rejection remain unchanged",
        [ArchitecturePlan { dlls: &[], execution: ExecutionPolicy::Absent },
         ArchitecturePlan { dlls: provider, execution: ExecutionPolicy::Typed }],
    ).with_lookup_dlls([&[], lookup]).with_exact_dlls(&["wire.dxgi"])
}

use Parameter::{Pointer as P, Uint32 as D};
const DXGI: &[&str] = &["dxgi.dll", "kernel32.dll", "kernelbase.dll", "wire.dxgi"];
const D3D11: &[&str] = &["d3d11.dll", "kernel32.dll", "kernelbase.dll", "wire.dxgi"];
const D3D12: &[&str] = &["d3d12.dll", "kernel32.dll", "kernelbase.dll", "wire.dxgi"];
pub static GRAPHICS_EXPORT_CONTRACTS: [Contract; 7] = [
    declaration(
        GraphicsExportApi::Factory,
        "CreateDXGIFactory",
        &[P, P],
        DXGI,
        &["dxgi.dll"],
    ),
    declaration(
        GraphicsExportApi::Factory1,
        "CreateDXGIFactory1",
        &[P, P],
        DXGI,
        &["dxgi.dll"],
    ),
    declaration(
        GraphicsExportApi::Factory2,
        "CreateDXGIFactory2",
        &[D, P, P],
        DXGI,
        &["dxgi.dll"],
    ),
    declaration(
        GraphicsExportApi::D3d11Device,
        "D3D11CreateDevice",
        &[P, D, P, D, P, D, D, P, P, P],
        D3D11,
        &["d3d11.dll"],
    ),
    declaration(
        GraphicsExportApi::D3d11DeviceAndSwapChain,
        "D3D11CreateDeviceAndSwapChain",
        &[P, D, P, D, P, D, D, P, P, P, P, P],
        D3D11,
        &["d3d11.dll"],
    ),
    declaration(
        GraphicsExportApi::D3d12Device,
        "D3D12CreateDevice",
        &[P, D, P, P],
        D3D12,
        &["d3d12.dll"],
    ),
    declaration(
        GraphicsExportApi::D3d12Debug,
        "D3D12GetDebugInterface",
        &[P, P],
        D3D12,
        &["d3d12.dll"],
    ),
];

pub(crate) fn symbol<'a>(dll: &str, name: &'a str) -> &'a str {
    if dll.eq_ignore_ascii_case("d3d12.dll") {
        // Wine 10.0 d3d12.spec, ordinal ABI data only.
        match name {
            "#101" => "D3D12CreateDevice",
            "#102" => "D3D12GetDebugInterface",
            _ => name,
        }
    } else {
        name
    }
}
