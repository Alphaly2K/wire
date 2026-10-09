//! Conservative roots for explicitly exposed guest objects. A complete paused
//! snapshot can shrink strong handles; no GC scans raw guest/HostRegion bytes.
use super::{Backend, BackendSession, ResourceKind, ResourceLease};
use crate::x64::memory::Memory;
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::c_void,
    marker::PhantomData,
    ptr::NonNull,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    thread::{self, ThreadId},
};
type Pointer = *mut c_void;
#[derive(Clone, Copy)]
pub struct GcHandleApi {
    pub current_thread: unsafe extern "C" fn() -> Pointer,
    pub strong: unsafe extern "C" fn(Pointer, i32) -> Pointer,
    pub weak: unsafe extern "C" fn(Pointer, i32) -> Pointer,
    pub target: unsafe extern "C" fn(Pointer) -> Pointer,
    pub free: unsafe extern "C" fn(Pointer),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuestObjectToken(u64);
static NEXT_OBJECT: AtomicU64 = AtomicU64::new(1);
#[derive(Clone, Copy, Debug)]
pub struct GuestRootRange {
    pub start: u64,
    pub bytes: usize,
}
pub struct GuestRootSnapshot {
    words: Vec<u64>,
}
impl GuestRootSnapshot {
    /// Copy paused GPRs and registered pointer-sized stack/static/heap slots.
    /// Any failed read rejects the whole snapshot, leaving existing roots in
    /// place. The provider must supply every guest root source and pause all
    /// relevant execution owners until synchronize completes.
    pub fn capture(
        registers: &[u64],
        memory: &Memory,
        ranges: &[GuestRootRange],
    ) -> Result<Self, String> {
        let total = ranges.iter().try_fold(
            registers
                .len()
                .checked_mul(8)
                .ok_or("root snapshot overflow")?,
            |total, range| {
                if !range.bytes.is_multiple_of(8) {
                    return Err("root range must cover complete pointer slots");
                }
                total
                    .checked_add(range.bytes)
                    .ok_or("root snapshot overflow")
            },
        )?;
        if total > 16 * 1024 * 1024 {
            return Err("guest root snapshot capacity exceeded".into());
        }
        let mut words = registers.to_vec();
        for range in ranges {
            let mut bytes = vec![0; range.bytes];
            memory.read(range.start, &mut bytes)?;
            words.extend(
                bytes
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .map(|bytes| u64::from_le_bytes(*bytes)),
            );
        }
        Ok(Self { words })
    }
}
struct Entry {
    object: u64,
    spans: Vec<(u64, u64)>,
    strong: usize,
    weak: usize,
}
pub struct GuestRoots {
    session: BackendSession,
    api: GcHandleApi,
    entries: BTreeMap<u64, Entry>,
    spans: BTreeMap<u64, (u64, u64)>,
    owner: ThreadId,
    _lifetime: ResourceLease,
    // Handles must be released by the attached owner before thread detach.
    _owner_only: PhantomData<Rc<()>>,
}
impl GuestRoots {
    /// # Safety
    /// APIs must be genuine v2 handle APIs of this session's sole live VM,
    /// using nonmoving BDWGC. The owner stays attached until this set drops,
    /// and this set outlives every guest exposure admitted through it.
    pub unsafe fn new(session: BackendSession, api: GcHandleApi) -> Result<Self, String> {
        if unsafe { (api.current_thread)() }.is_null() {
            return Err("guest root owner is not attached to Mono".into());
        }
        let lifetime = session.retain(Backend::NativeArm64, ResourceKind::ManagedRoot)?;
        Ok(Self {
            session,
            api,
            entries: BTreeMap::new(),
            spans: BTreeMap::new(),
            owner: thread::current().id(),
            _lifetime: lifetime,
            _owner_only: PhantomData,
        })
    }
    fn owner(&self) -> Result<(), String> {
        if thread::current().id() != self.owner || unsafe { (self.api.current_thread)() }.is_null()
        {
            return Err("guest root owner must remain attached".into());
        }
        Ok(())
    }
    pub fn strong_roots(&self) -> usize {
        self.entries
            .values()
            .filter(|entry| entry.strong != 0)
            .count()
    }
    /// # Safety
    /// object is a currently rooted real object in this VM. Each span must be
    /// an already validated guest-visible alias of exactly this object/header/
    /// payload, including interior byrefs. Spans do not admit native memory;
    /// their mappings/permissions/generation belong to the provider separately.
    pub unsafe fn admit(
        &mut self,
        object: NonNull<c_void>,
        spans: &[(u64, usize)],
    ) -> Result<GuestObjectToken, String> {
        self.owner()?;
        let _call = self.session.enter_call(Backend::NativeArm64)?;
        if spans.is_empty() {
            return Err("guest object has no validated exposure spans".into());
        }
        let mut validated = Vec::new();
        for &(start, bytes) in spans {
            let end = start
                .checked_add(bytes as u64)
                .filter(|&end| bytes != 0 && end <= (1 << 47))
                .ok_or("invalid guest object exposure span")?;
            if self
                .spans
                .range(..end)
                .next_back()
                .is_some_and(|(_, &(end_at, _))| end_at > start)
            {
                return Err("overlapping guest object exposures".into());
            }
            validated.push((start, end));
        }
        validated.sort_unstable();
        let mut spans: Vec<(u64, u64)> = Vec::new();
        for (start, end) in validated {
            if let Some(last) = spans.last_mut().filter(|last| start <= last.1) {
                last.1 = last.1.max(end);
            } else {
                spans.push((start, end));
            }
        }
        let id = NEXT_OBJECT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| "guest object token space exhausted")?;
        let strong = unsafe { (self.api.strong)(object.as_ptr(), 0) } as usize;
        if strong == 0 {
            return Err("guest exposure strong root allocation failed".into());
        }
        let weak = unsafe { (self.api.weak)(object.as_ptr(), 0) } as usize;
        if weak == 0 {
            unsafe { (self.api.free)(strong as Pointer) };
            return Err("guest exposure weak admission allocation failed".into());
        }
        for &(start, end) in &spans {
            self.spans.insert(start, (end, id));
        }
        self.entries.insert(
            id,
            Entry {
                object: object.as_ptr() as u64,
                spans,
                strong,
                weak,
            },
        );
        Ok(GuestObjectToken(id))
    }
    /// # Safety
    /// snapshot contains every live guest root source in this process, not a
    /// subset. All guest owners are paused; newly exposed objects are admitted
    /// immediately before resuming. A missing source can cause early collection.
    /// No Memory borrow/native buffer lease may survive this Mono API boundary.
    pub unsafe fn synchronize(&mut self, snapshot: GuestRootSnapshot) -> Result<(), String> {
        self.owner()?;
        let _call = self.session.enter_call(Backend::NativeArm64)?;
        let desired: BTreeSet<_> = snapshot
            .words
            .iter()
            .filter_map(|word| {
                self.spans
                    .range(..=word)
                    .next_back()
                    .and_then(|(_, &(end, id))| (*word < end).then_some(id))
            })
            .collect();
        let mut additions = Vec::new();
        for &id in &desired {
            let entry = &self.entries[&id];
            let target = unsafe { (self.api.target)(entry.weak as Pointer) } as u64;
            if target == 0 {
                continue;
            } // expired weak token never resurrects an object
            if target != entry.object {
                for (_, handle) in additions {
                    unsafe { (self.api.free)(handle as Pointer) };
                }
                return Err("guest exposure weak identity changed".into());
            }
            if entry.strong == 0 {
                let handle = unsafe { (self.api.strong)(target as Pointer, 0) } as usize;
                if handle == 0 {
                    for (_, handle) in additions {
                        unsafe { (self.api.free)(handle as Pointer) };
                    }
                    return Err(
                        "guest snapshot strong root allocation failed; old roots retained".into(),
                    );
                }
                additions.push((id, handle));
            }
        }
        for (id, handle) in additions {
            self.entries.get_mut(&id).unwrap().strong = handle;
        }
        for (&id, entry) in &mut self.entries {
            if !desired.contains(&id) && entry.strong != 0 {
                unsafe { (self.api.free)(entry.strong as Pointer) };
                entry.strong = 0;
            }
        }
        Ok(())
    }
    /// Remove only entries whose weak target is already gone. Stale tokens
    /// stay invalid; this does not retire live mappings or unroot live objects.
    pub fn forget_reclaimed(&mut self) -> Result<usize, String> {
        self.owner()?;
        let _call = self.session.enter_call(Backend::NativeArm64)?;
        let expired: Vec<_> = self
            .entries
            .iter()
            .filter_map(|(&id, entry)| {
                (entry.strong == 0 && unsafe { (self.api.target)(entry.weak as Pointer) }.is_null())
                    .then_some(id)
            })
            .collect();
        for id in &expired {
            let entry = self.entries.remove(id).unwrap();
            for &(start, _) in &entry.spans {
                self.spans.remove(&start);
            }
            unsafe { (self.api.free)(entry.weak as Pointer) };
        }
        Ok(expired.len())
    }
    pub fn token_is_rooted(&self, token: GuestObjectToken) -> bool {
        self.entries
            .get(&token.0)
            .is_some_and(|entry| entry.strong != 0)
    }
    /// # Safety
    /// Guest owners are stopped and all references/mappings from this set have
    /// been retired. This never switches VM, revokes Memory, or detaches Mono.
    pub unsafe fn retire_all(&mut self) -> Result<(), String> {
        self.owner()?;
        for entry in self.entries.values() {
            if entry.strong != 0 {
                unsafe { (self.api.free)(entry.strong as Pointer) };
            }
            unsafe { (self.api.free)(entry.weak as Pointer) };
        }
        self.entries.clear();
        self.spans.clear();
        Ok(())
    }
}
impl Drop for GuestRoots {
    fn drop(&mut self) {
        self.owner()
            .expect("guest roots dropped after owner detached");
        // The unsafe admission contract requires all guest exposure lifetimes
        // to end before the registry. Release real handles before its VM lease.
        unsafe { self.retire_all() }.expect("guest root retirement failed");
    }
}
