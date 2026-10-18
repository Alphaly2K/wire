//! Existing stock/resource image subsets and architecture-specific failures.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageApi {
    LoadIconA,
    LoadIconW,
    LoadCursorA,
    LoadCursorW,
    LoadImageA,
    LoadImageW,
    DestroyIcon,
    DestroyCursor,
}

impl ImageApi {
    pub(crate) fn wide(self) -> bool {
        matches!(self, Self::LoadIconW | Self::LoadCursorW | Self::LoadImageW)
    }
    pub(crate) fn load_image(self) -> bool {
        matches!(self, Self::LoadImageA | Self::LoadImageW)
    }
}

const fn declaration(
    id: ImageApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::Image(id), name, parameters, returns,
        "PE32 stock arrow only; DestroyIcon retains explicit unsupported handler/trace, other five facilities remain ABI-only; Win64 logical stock icon/cursor or bounded 32-bit DIB icon resource, shared identity and owned destruction, full image handles and monotonic capacity; preserve validation/error/fault/return-frame order and static-only kernel providers",
        [ArchitecturePlan { dlls: &["user32.dll"], execution: if matches!(id, ImageApi::LoadCursorA | ImageApi::LoadCursorW | ImageApi::DestroyIcon) { ExecutionPolicy::Typed } else { ExecutionPolicy::LegacyUnimplemented } },
         ArchitecturePlan { dlls: &["kernel32.dll", "kernelbase.dll", "user32.dll"], execution: ExecutionPolicy::Typed }],
    ).with_lookup_dlls([&["user32.dll"], &["user32.dll"]])
}

use Parameter::{Int32 as I, Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Pointer as R};
pub static IMAGE_CONTRACTS: [Contract; 8] = [
    declaration(ImageApi::LoadIconA, "LoadIconA", &[P, P], R),
    declaration(ImageApi::LoadIconW, "LoadIconW", &[P, P], R),
    declaration(ImageApi::LoadCursorA, "LoadCursorA", &[P, P], R),
    declaration(ImageApi::LoadCursorW, "LoadCursorW", &[P, P], R),
    declaration(ImageApi::LoadImageA, "LoadImageA", &[P, P, D, I, I, D], R),
    declaration(ImageApi::LoadImageW, "LoadImageW", &[P, P, D, I, I, D], R),
    declaration(ImageApi::DestroyIcon, "DestroyIcon", &[P], B),
    declaration(ImageApi::DestroyCursor, "DestroyCursor", &[P], B),
];
