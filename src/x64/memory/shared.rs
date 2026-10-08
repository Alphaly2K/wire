//! Explicit data-only host regions. Foreign storage requires a retained owner
//! and every native/guest access must hold the same reentrant execution lease.
use super::{Memory, EXECUTE, PAGE, READ, USER_END, WRITE};
use std::{
    any::Any,
    cell::{Cell, UnsafeCell},
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
struct OwnedBytes(UnsafeCell<Box<[u8]>>);
unsafe impl Send for OwnedBytes {}
unsafe impl Sync for OwnedBytes {}

#[derive(Clone)]
pub struct HostRegion(Arc<RegionInner>);
impl HostRegion {
    /// Allocate stable owned bytes. Copies of a region share this allocation.
    pub fn owned(len: usize) -> Result<Self, String> {
        if len == 0 || len > 256 * 1024 * 1024 {
            return Err("invalid shared region length".into());
        }
        let owner = Arc::new(OwnedBytes(UnsafeCell::new(vec![0; len].into_boxed_slice())));
        let pointer = unsafe { NonNull::new_unchecked((&mut *owner.0.get()).as_mut_ptr()) };
        // No other reference to these bytes exists; all later access uses a lease.
        unsafe { Self::foreign(pointer, len, owner) }
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
