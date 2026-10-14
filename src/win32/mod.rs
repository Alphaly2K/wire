//! Windows contracts and architecture adapters. Existing public entry points
//! remain facades while individual API families migrate from legacy dispatch.
pub mod catalog;
pub(crate) mod com;
pub(crate) mod x64;
pub(crate) mod x86;
