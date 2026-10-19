//! ABI sources and explicit legacy policies; execution capabilities live in catalog.
mod pe32;
pub(crate) use pe32::{ordinal_name, stack_bytes};
