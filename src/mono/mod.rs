//! Native Mono backend lifecycle contracts. This optional module does not load
//! a runtime or enable a native game profile. The original backend remains the
//! default until the independent memory/ABI/service admission is complete.
mod buffers;
mod session;
pub use buffers::{BufferHandle, NativeBuffer, SharedBuffers};
pub use session::{
    Admission, Backend, BackendSession, CallLease, ResourceKind, ResourceLease, State,
};
