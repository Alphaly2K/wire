//! Portable machine state and Windows/graphics components for the Wasm runtime.
#[path = "../../../src/codepage/portable.rs"]
pub mod codepage;
pub mod cpu;
#[allow(dead_code)]
#[path = "../../../src/win32/environment_profile.rs"]
pub(crate) mod environment;
pub mod extended;
#[allow(dead_code)]
pub(crate) mod file_handles;
#[allow(dead_code)]
pub(crate) mod file_sections;
#[allow(dead_code)]
pub mod filesystem;
pub mod floating;
pub mod integer;
pub mod jit;
#[allow(dead_code)]
#[path = "../../../src/keyboard.rs"]
mod keyboard;
pub mod machine;
pub mod memory;
pub(crate) mod paths;
pub mod pe;
pub mod process;
pub mod win32;

#[cfg(target_arch = "wasm32")]
mod exports;

pub use memory as mem;

#[path = "../../../src/graphics/gpu_types.rs"]
pub mod gpu_types;
#[path = "../../../src/graphics/operation.rs"]
pub mod graphics;

#[allow(dead_code)]
pub(crate) mod gpu;

#[allow(dead_code)]
#[path = "../../../src/numeric/x87.rs"]
pub(crate) mod x87;

pub use win32::user::records::{WindowEvent, WindowInfo};
pub use wire_simd as simd;
