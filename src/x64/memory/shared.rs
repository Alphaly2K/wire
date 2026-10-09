//! Explicit data-only host regions. Foreign storage requires a retained owner
//! and every native/guest access must hold the same reentrant execution lease.
use super::{Memory, EXECUTE, PAGE, READ, USER_END, WRITE};
use std::{
    alloc::{alloc_zeroed, dealloc, Layout},
    any::Any,
    cell::Cell,
    collections::BTreeMap,
    ptr::NonNull,
    sync::{Arc, Condvar, Mutex},
    thread::{self, ThreadId},
};

struct GateState {
    owner: Option<ThreadId>,
    depth: usize,
    revoked: bool,
}
struct RegionInner {
    pointer: NonNull<u8>,
    len: usize,
    _owner: Arc<dyn Any + Send + Sync>,
    gate: Mutex<GateState>,
    changed: Condvar,
}
// Foreign construction requires all memory access to use this region's lease.
// The retained owner is Send+Sync, and no borrowed byte slice escapes a lease.
unsafe impl Send for RegionInner {}
unsafe impl Sync for RegionInner {}
struct Lease<'a>(&'a RegionInner);
impl Drop for Lease<'_> {
    fn drop(&mut self) {
        let mut state = self.0.gate.lock().unwrap();
        assert_eq!(state.owner, Some(thread::current().id()));
        state.depth -= 1;
        if state.depth == 0 {
            state.owner = None;
            self.0.changed.notify_one();
        }
    }
}
struct OwnedBytes {
    pointer: NonNull<u8>,
    layout: Layout,
    _retained: Arc<dyn Any + Send + Sync>,
}
unsafe impl Send for OwnedBytes {}
unsafe impl Sync for OwnedBytes {}
impl Drop for OwnedBytes {
    fn drop(&mut self) {
        unsafe { dealloc(self.pointer.as_ptr(), self.layout) };
    }
}

#[derive(Clone)]
pub struct HostRegion(Arc<RegionInner>);
impl HostRegion {
    /// Allocate stable owned bytes. Copies of a region share this allocation.
    pub fn owned(len: usize) -> Result<Self, String> {
        Self::owned_retained(len, 16, Arc::new(()))
    }
    /// Zeroed, aligned storage which retains an additional lifecycle owner.
    /// The owner outlives every mapping/native view, including revoked aliases.
    pub fn owned_retained(
        len: usize,
        alignment: usize,
        retained: Arc<dyn Any + Send + Sync>,
    ) -> Result<Self, String> {
        if len == 0
            || len > 256 * 1024 * 1024
            || !alignment.is_power_of_two()
            || alignment > PAGE as usize
        {
            return Err("invalid shared allocation size/alignment".into());
        }
        let layout =
            Layout::from_size_align(len, alignment).map_err(|_| "shared allocation overflow")?;
        let pointer =
            NonNull::new(unsafe { alloc_zeroed(layout) }).ok_or("shared allocation failed")?;
        let owner = Arc::new(OwnedBytes {
            pointer,
            layout,
            _retained: retained,
        });
        // Owned storage is only accessed through the region's execution lease.
        unsafe { Self::foreign(pointer, len, owner) }
    }
    #[cfg(feature = "native-mono-backend")]
    pub(crate) fn same_storage(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    /// Retain a specific foreign allocation, such as a pinned Mono array.
    ///
    /// # Safety
    /// pointer..pointer+len must remain valid, initialized, and writable while
    /// owner is retained. All native accesses, including callbacks, GC writes
    /// and accesses through aliases, must use this region's execution lease.
    /// Do not hold Rust references to its bytes across a native call/reentry.
    /// The owner must safely release storage on whichever thread drops it.
    pub unsafe fn foreign(
        pointer: NonNull<u8>,
        len: usize,
        owner: Arc<dyn Any + Send + Sync>,
    ) -> Result<Self, String> {
        if len == 0
            || len > 256 * 1024 * 1024
            || (pointer.as_ptr() as usize).checked_add(len).is_none()
        {
            return Err("invalid foreign region length".into());
        }
        Ok(Self(Arc::new(RegionInner {
            pointer,
            len,
            _owner: owner,
            gate: Mutex::new(GateState {
                owner: None,
                depth: 0,
                revoked: false,
            }),
            changed: Condvar::new(),
        })))
    }
    pub fn len(&self) -> usize {
        self.0.len
    }
    pub fn is_empty(&self) -> bool {
        false
    }
    fn lease(&self) -> Result<Lease<'_>, String> {
        let me = thread::current().id();
        let mut state = self.0.gate.lock().unwrap();
        while state.owner.is_some_and(|owner| owner != me) {
            state = self.0.changed.wait(state).unwrap();
        }
        if state.revoked {
            return Err("revoked host region".into());
        }
        state.owner = Some(me);
        state.depth += 1;
        Ok(Lease(&self.0))
    }
    /// Serialize native access while permitting a callback on this same thread.
    /// The pointer is a data pointer; dereferencing it remains an unsafe act.
    pub fn with_native<T>(&self, call: impl FnOnce(NonNull<u8>) -> T) -> Result<T, String> {
        let _lease = self.lease()?;
        Ok(call(self.0.pointer))
    }
    /// Invalidate every mapping/alias before releasing a native allocation.
    /// Retained storage stays alive until the last region/mapping is dropped.
    pub fn revoke(&self) -> Result<(), String> {
        let _lease = self.lease()?;
        self.0.gate.lock().unwrap().revoked = true;
        Ok(())
    }
    fn check_live(&self) -> Result<(), String> {
        let _lease = self.lease()?;
        Ok(())
    }
    fn read(&self, offset: usize, output: &mut [u8]) -> Result<(), String> {
        let _lease = self.lease()?;
        if offset
            .checked_add(output.len())
            .is_none_or(|end| end > self.len())
        {
            return Err("shared read out of bounds".into());
        }
        unsafe {
            std::ptr::copy_nonoverlapping(
                self.0.pointer.as_ptr().add(offset),
                output.as_mut_ptr(),
                output.len(),
            );
        }
        Ok(())
    }
    fn write(&self, offset: usize, bytes: &[u8]) -> Result<(), String> {
        let _lease = self.lease()?;
        if offset
            .checked_add(bytes.len())
            .is_none_or(|end| end > self.len())
        {
            return Err("shared write out of bounds".into());
        }
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                self.0.pointer.as_ptr().add(offset),
                bytes.len(),
            );
        }
        Ok(())
    }
}

pub(super) struct SharedMapping {
    base: u64,
    end: u64,
    region: HostRegion,
    permissions: u8,
    maximum: u8,
    guards: Box<[Cell<bool>]>,
}
impl SharedMapping {
    pub(super) fn base(&self) -> u64 {
        self.base
    }
    pub(super) fn end(&self) -> u64 {
        self.end
    }
    fn check(&self, address: u64, len: usize, access: u8) -> Result<(), String> {
        if address < self.base
            || address
                .checked_add(len as u64)
                .is_none_or(|end| end > self.end)
        {
            return Err("shared access out of registered bounds".into());
        }
        for page in (address / PAGE)..=((address + len as u64 - 1) / PAGE) {
            let guard = &self.guards[(page - self.base / PAGE) as usize];
            if guard.replace(false) {
                return Err(format!(
                    "AMD64 guard-page fault at shared page {:#x}",
                    page * PAGE
                ));
            }
        }
        if self.permissions & access != access {
            return Err(format!(
                "AMD64 shared access {access} denied at {address:#x}"
            ));
        }
        self.region.check_live()
    }
}

impl Memory {
    /// Map only this explicitly retained byte interval, including non-page-
    /// aligned objects. Neighbouring host allocations are never exposed.
    pub fn map_shared_data(
        &mut self,
        address: u64,
        region: HostRegion,
        permissions: u8,
        maximum: u8,
    ) -> Result<(), String> {
        if maximum & !(READ | WRITE) != 0 || permissions & !maximum != 0 {
            return Err("shared host regions are data-only with bounded permissions".into());
        }
        let end = address
            .checked_add(region.len() as u64)
            .ok_or("shared mapping overflow")?;
        if address >= USER_END || end > USER_END || !self.free_range(address, end) {
            return Err("invalid or overlapping shared mapping".into());
        }
        if self.committed_bytes() + region.len() as u64 > 512 * 1024 * 1024 {
            return Err("AMD64 mapping capacity exceeded".into());
        }
        region.check_live()?;
        self.shared_bytes += region.len() as u64;
        self.shared.insert(
            address,
            SharedMapping {
                base: address,
                end,
                region,
                permissions,
                maximum,
                guards: (address / PAGE..=(end - 1) / PAGE)
                    .map(|_| Cell::new(false))
                    .collect(),
            },
        );
        self.invalidate_mapping(false);
        Ok(())
    }
    /// Exact-region protection for the isolated native backend prototype.
    /// This is not an implementation of Win32 VirtualProtect on GC pages.
    pub fn protect_shared_data(
        &mut self,
        address: u64,
        permissions: u8,
        guard: bool,
    ) -> Result<(), String> {
        let mapping = self
            .shared
            .get_mut(&address)
            .ok_or("unmapped shared region")?;
        if permissions & !mapping.maximum != 0 || permissions & EXECUTE != 0 {
            return Err("shared protection exceeds native ownership".into());
        }
        mapping.region.check_live()?;
        mapping.permissions = permissions;
        for page in &mapping.guards {
            page.set(guard);
        }
        self.invalidate_mapping(false);
        Ok(())
    }
    /// Verify ownership before an allocator retires an exact mapping.
    #[cfg(feature = "native-mono-backend")]
    pub(crate) fn owns_shared_data(&self, address: u64, region: &HostRegion) -> bool {
        self.shared
            .get(&address)
            .is_some_and(|mapping| mapping.region.same_storage(region))
    }
    pub fn unmap_shared_data(&mut self, address: u64) -> Result<HostRegion, String> {
        let mapping = self
            .shared
            .remove(&address)
            .ok_or("unmapped shared region")?;
        self.shared_bytes -= mapping.region.len() as u64;
        self.invalidate_mapping(false);
        Ok(mapping.region)
    }
    pub(super) fn shared_overlaps(&self, start: u64, end: u64) -> bool {
        self.shared
            .range(..end)
            .next_back()
            .is_some_and(|(_, mapping)| mapping.end > start)
    }
    fn shared_mapping(&self, address: u64, len: usize) -> Result<Option<&SharedMapping>, String> {
        let range = Self::range(address, len)?;
        if len == 0 || self.shared.is_empty() {
            return Ok(None);
        }
        if let Some((_, mapping)) = self.shared.range(..=address).next_back() {
            if address < mapping.end {
                if range.end > mapping.end {
                    return Err("shared access crosses registered boundary".into());
                }
                return Ok(Some(mapping));
            }
        }
        if self.shared.range(address..range.end).next().is_some() {
            return Err("access crosses shared mapping boundary".into());
        }
        Ok(None)
    }
    pub(super) fn shared_check(
        &self,
        address: u64,
        len: usize,
        access: u8,
    ) -> Result<bool, String> {
        let Some(mapping) = self.shared_mapping(address, len)? else {
            return Ok(false);
        };
        mapping.check(address, len, access)?;
        Ok(true)
    }
    pub(super) fn shared_read(
        &self,
        address: u64,
        output: &mut [u8],
        access: u8,
    ) -> Result<bool, String> {
        let Some(mapping) = self.shared_mapping(address, output.len())? else {
            return Ok(false);
        };
        mapping.check(address, output.len(), access)?;
        mapping
            .region
            .read((address - mapping.base) as usize, output)?;
        Ok(true)
    }
    pub(super) fn shared_observe(&self, address: u64, output: &mut [u8]) -> Result<bool, String> {
        let Some(mapping) = self.shared_mapping(address, output.len())? else {
            return Ok(false);
        };
        for page in address / PAGE..=(address + output.len() as u64 - 1) / PAGE {
            if mapping.guards[(page - mapping.base / PAGE) as usize].get() {
                return Err(format!(
                    "guarded AMD64 shared observation at {:#x}",
                    page * PAGE
                ));
            }
        }
        if mapping.permissions & READ == 0 {
            return Err(format!(
                "unreadable AMD64 shared observation at {address:#x}"
            ));
        }
        // read() obtains a single lease, rejects revoked storage, then copies.
        mapping
            .region
            .read((address - mapping.base) as usize, output)?;
        Ok(true)
    }
    pub(super) fn shared_contains_range(&self, address: u64, len: usize) -> Result<bool, String> {
        Ok(self.shared_mapping(address, len)?.is_some())
    }
    pub(super) fn shared_modify_int(
        &self,
        address: u64,
        size: usize,
        update: impl FnOnce(u64) -> u64,
    ) -> Result<(), String> {
        let mapping = self
            .shared_mapping(address, size)?
            .ok_or("unmapped shared scalar update")?;
        mapping.check(address, size, READ)?;
        // Keep read/update/write in a single reentrant region lease. Native
        // accesses cannot observe a split RMW or race its copied old value.
        mapping.region.with_native(|pointer| {
            let at = unsafe { pointer.as_ptr().add((address - mapping.base) as usize) };
            let mut bytes = [0u8; 8];
            unsafe { std::ptr::copy_nonoverlapping(at, bytes.as_mut_ptr(), size) };
            let result = update(u64::from_le_bytes(bytes));
            mapping.check(address, size, WRITE)?;
            unsafe { std::ptr::copy_nonoverlapping(result.to_le_bytes().as_ptr(), at, size) };
            Ok::<(), String>(())
        })?
    }
    pub(super) fn shared_write(&self, address: u64, bytes: &[u8]) -> Result<bool, String> {
        let Some(mapping) = self.shared_mapping(address, bytes.len())? else {
            return Ok(false);
        };
        mapping.check(address, bytes.len(), WRITE)?;
        mapping
            .region
            .write((address - mapping.base) as usize, bytes)?;
        Ok(true)
    }
    pub(super) fn shared_merge(&mut self, other: &mut BTreeMap<u64, SharedMapping>) {
        self.shared_bytes += other.values().map(|m| m.region.len() as u64).sum::<u64>();
        self.shared.append(other);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_rmw_preserves_alias_permissions_guards_and_native_lease() {
        let mut memory = Memory::default();
        let region = HostRegion::owned(8).unwrap();
        memory
            .map_shared_data(0x57000003, region.clone(), READ | WRITE, READ | WRITE)
            .unwrap();
        memory
            .map_shared_data(0x57001003, region.clone(), READ | WRITE, READ | WRITE)
            .unwrap();
        memory.write_int(0x57000003, 8, 41).unwrap();
        memory.modify_int(0x57001003, 8, |value| value + 1).unwrap();
        assert_eq!(memory.read_int(0x57000003, 8).unwrap(), 42);
        memory.protect_shared_data(0x57001003, READ, false).unwrap();
        let mut called = false;
        assert!(memory
            .modify_int(0x57001003, 8, |_| {
                called = true;
                99
            })
            .is_err());
        assert!(called);
        assert_eq!(memory.read_int(0x57000003, 8).unwrap(), 42);
        memory
            .protect_shared_data(0x57001003, READ | WRITE, true)
            .unwrap();
        assert!(memory
            .modify_int(0x57001003, 8, |_| panic!("guard must fail before update"))
            .is_err());
        memory.modify_int(0x57001003, 8, |value| value + 1).unwrap();
        assert_eq!(memory.read_int(0x57000003, 8).unwrap(), 43);
        assert!(memory
            .modify_int(0x57001003 + 4, 8, |_| panic!(
                "bounds must fail before update"
            ))
            .is_err());
        region.revoke().unwrap();
        assert!(memory
            .modify_int(0x57000003, 8, |_| panic!(
                "retired mapping must reject update"
            ))
            .is_err());
    }
}
