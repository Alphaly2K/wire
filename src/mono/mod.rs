//! Native Mono backend lifecycle contracts. This optional module does not load
//! a runtime or enable a native game profile. The independent finite startup
//! diagnostic provider lives under x64::process; it does not satisfy Admission.
//! The original backend remains the default until memory/ABI/service admission
//! is complete.
mod buffers;
mod roots;
mod session;
pub use buffers::{BufferHandle, NativeBuffer, SharedBuffers};
pub use roots::{GcHandleApi, GuestObjectToken, GuestRootRange, GuestRootSnapshot, GuestRoots};
pub use session::{
    Admission, Backend, BackendSession, CallLease, ResourceKind, ResourceLease, State,
};
