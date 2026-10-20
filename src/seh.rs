//! Compatibility facade; SEH execution belongs to Process32.
pub use crate::x86::process::exceptions::{raise, unwind};
