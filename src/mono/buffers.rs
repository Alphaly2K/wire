//! Engine/unsafe-IL byte buffers only. This arena does not expose the Mono heap.
//! Native views access bytes exclusively through HostRegion execution leases.
use super::{Backend, BackendSession, ResourceKind};
use crate::x64::memory::{shared::HostRegion, Memory, READ, WRITE};
use std::{
    collections::BTreeMap,
    ptr::NonNull,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
static NEXT_BUFFER: AtomicU64 = AtomicU64::new(1);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferHandle {
    id: u64,
    address: u64,
    len: usize,
    alignment: usize,
}
impl BufferHandle {
    pub fn address(self) -> u64 {
        self.address
    }
    pub fn len(self) -> usize {
        self.len
    }
    pub fn is_empty(self) -> bool {
        false
    }
    pub fn alignment(self) -> usize {
        self.alignment
    }
}
struct Allocation {
    handle: BufferHandle,
    region: HostRegion,
    aliases: Vec<u64>,
}
impl Allocation {
    fn check_owner(&self, memory: &Memory) -> Result<(), String> {
        if !std::iter::once(self.handle.address)
            .chain(self.aliases.iter().copied())
            .all(|at| memory.owns_shared_data(at, &self.region))
        {
            return Err("shared buffer mappings changed or belong to another Memory".into());
        }
        Ok(())
    }
}
#[derive(Clone)]
pub struct NativeBuffer {
    session: BackendSession,
    region: HostRegion,
}
impl NativeBuffer {
    /// The pointer cannot be dereferenced outside this callback. Do not retain
    /// Rust byte references across native calls/reentry. Views retain storage
    /// after retirement but reject access, and continue to block VM cleanup.
    pub fn with_native<T>(&self, operation: impl FnOnce(NonNull<u8>) -> T) -> Result<T, String> {
        let _call = self.session.enter_call(Backend::NativeArm64)?;
        self.region.with_native(operation)
    }
}
pub struct SharedBuffers {
    session: BackendSession,
    allocations: BTreeMap<u64, Allocation>,
    live_bytes: usize,
    capacity: usize,
}
impl SharedBuffers {
    pub fn new(session: BackendSession, capacity: usize) -> Result<Self, String> {
        if session.backend() != Backend::NativeArm64
            || capacity == 0
            || capacity > 256 * 1024 * 1024
        {
            return Err("shared buffers require a native session and bounded capacity".into());
        }
        Ok(Self {
            session,
            allocations: BTreeMap::new(),
            live_bytes: 0,
            capacity,
        })
    }
    pub fn live_bytes(&self) -> usize {
        self.live_bytes
    }
    fn allocation(&self, handle: BufferHandle) -> Result<&Allocation, String> {
        self.allocations
            .get(&handle.id)
            .filter(|entry| entry.handle == handle)
            .ok_or_else(|| "unknown or retired shared buffer handle".into())
    }
    fn prepare(
        &self,
        memory: &mut Memory,
        len: usize,
        alignment: usize,
    ) -> Result<Allocation, String> {
        let retained = self
            .session
            .retain(Backend::NativeArm64, ResourceKind::SharedBacking)?;
        let region = HostRegion::owned_retained(len, alignment, Arc::new(retained))?;
        let address = region.with_native(|pointer| pointer.as_ptr() as u64)?;
        let id = NEXT_BUFFER
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| "shared buffer token space exhausted")?;
        let handle = BufferHandle {
            id,
            address,
            len,
            alignment,
        };
        memory.map_shared_data(address, region.clone(), READ | WRITE, READ | WRITE)?;
        Ok(Allocation {
            handle,
            region,
            aliases: Vec::new(),
        })
    }
    /// Zero-filled allocation; guest/native pointer identity is the same, but
    /// no surrounding host bytes or executable permissions are registered.
    pub fn allocate(
        &mut self,
        memory: &mut Memory,
        len: usize,
        alignment: usize,
    ) -> Result<BufferHandle, String> {
        let _call = self.session.enter_call(Backend::NativeArm64)?;
        let total = self
            .live_bytes
            .checked_add(len)
            .filter(|&total| total <= self.capacity)
            .ok_or("shared buffer capacity exceeded")?;
        let allocation = self.prepare(memory, len, alignment)?;
        let handle = allocation.handle;
        self.allocations.insert(handle.id, allocation);
        self.live_bytes = total;
        Ok(handle)
    }
    pub fn native_view(&self, handle: BufferHandle) -> Result<NativeBuffer, String> {
        let _call = self.session.enter_call(Backend::NativeArm64)?;
        Ok(NativeBuffer {
            session: self.session.clone(),
            region: self.allocation(handle)?.region.clone(),
        })
    }
    /// All aliases retain the same storage/root owner and retire together.
    pub fn alias(
        &mut self,
        memory: &mut Memory,
        handle: BufferHandle,
        at: u64,
        permissions: u8,
    ) -> Result<(), String> {
        let _call = self.session.enter_call(Backend::NativeArm64)?;
        let allocation = self.allocation(handle)?;
        allocation.check_owner(memory)?;
        memory.map_shared_data(at, allocation.region.clone(), permissions, READ | WRITE)?;
        self.allocations
            .get_mut(&handle.id)
            .unwrap()
            .aliases
            .push(at);
        Ok(())
    }
    /// Allocate/copy first; an allocation/protection failure leaves the old
    /// handle and bytes valid. Success invalidates the old token and aliases.
    /// Transient old+new backing is additionally bounded by Memory's capacity.
    pub fn reallocate(
        &mut self,
        memory: &mut Memory,
        old: BufferHandle,
        len: usize,
    ) -> Result<BufferHandle, String> {
        let _call = self.session.enter_call(Backend::NativeArm64)?;
        let previous = self.allocation(old)?;
        previous.check_owner(memory)?;
        let total = (self.live_bytes - old.len)
            .checked_add(len)
            .filter(|&total| total <= self.capacity)
            .ok_or("shared buffer capacity exceeded")?;
        let mut preserved = vec![0; old.len.min(len)];
        memory.read(old.address, &mut preserved)?;
        let replacement = self.prepare(memory, len, old.alignment)?;
        // Fresh region has no other native owner; do not hold two region locks.
        replacement
            .region
            .with_native(|pointer| unsafe {
                std::ptr::copy_nonoverlapping(
                    preserved.as_ptr(),
                    pointer.as_ptr(),
                    preserved.len(),
                );
            })
            .expect("fresh shared buffer unexpectedly revoked");
        let handle = replacement.handle;
        self.free(memory, old)?;
        self.allocations.insert(handle.id, replacement);
        self.live_bytes = total;
        Ok(handle)
    }
    /// Free is allowed during Failed/Stopping so shutdown can release owners.
    pub fn free(&mut self, memory: &mut Memory, handle: BufferHandle) -> Result<(), String> {
        let allocation = self.allocation(handle)?;
        allocation.check_owner(memory)?;
        allocation.region.revoke()?;
        let allocation = self.allocations.remove(&handle.id).unwrap();
        for at in std::iter::once(handle.address).chain(allocation.aliases.iter().copied()) {
            drop(
                memory
                    .unmap_shared_data(at)
                    .expect("preflighted shared buffer mapping disappeared"),
            );
        }
        self.live_bytes -= handle.len;
        Ok(())
    }
    pub fn retire_all(&mut self, memory: &mut Memory) -> Result<(), String> {
        // Preflight the whole collection before removing any mapping.
        for allocation in self.allocations.values() {
            allocation.check_owner(memory)?;
        }
        let handles: Vec<_> = self
            .allocations
            .values()
            .map(|entry| entry.handle)
            .collect();
        for handle in handles {
            self.free(memory, handle)?;
        }
        Ok(())
    }
}
impl Drop for SharedBuffers {
    fn drop(&mut self) {
        // If the caller omitted retire_all, remaining Memory mappings keep the
        // backing/resource lease alive, but all aliases become inaccessible.
        for allocation in self.allocations.values() {
            let _ = allocation.region.revoke();
        }
    }
}
