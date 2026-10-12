//! Native UnsafeUtility memory operations and engine-backed allocation owners.
//! Pure native operations follow IL's unsafe-pointer contract, including stack
//! and managed fields. Only an explicit owned export can enter guest Memory.
use super::{bindings, callbacks, managed_calls, Object};
use std::{
    alloc::{alloc_zeroed, dealloc, Layout},
    cell::{Cell, RefCell},
    collections::BTreeMap,
    marker::PhantomData,
    ptr::NonNull,
    rc::Rc,
    sync::{Mutex, OnceLock},
    thread::{self, ThreadId},
};
const CAPACITY: usize = 64 * 1024 * 1024;
const COUNT: usize = 4096;
const BINDINGS: [(&std::ffi::CStr, &std::ffi::CStr); 10] = [
    (
        c"Unity.Collections.LowLevel.Unsafe.UnsafeUtility::Malloc",
        c"Malloc",
    ),
    (
        c"Unity.Collections.LowLevel.Unsafe.UnsafeUtility::Free",
        c"Free",
    ),
    (
        c"Unity.Collections.LowLevel.Unsafe.UnsafeUtility::MemSet",
        c"MemSet",
    ),
    (
        c"Unity.Collections.LowLevel.Unsafe.UnsafeUtility::MemCpy",
        c"MemCpy",
    ),
    (
        c"Unity.Collections.LowLevel.Unsafe.UnsafeUtility::MemMove",
        c"MemMove",
    ),
    (
        c"Unity.Collections.LowLevel.Unsafe.UnsafeUtility::MallocTracked",
        c"MallocTracked",
    ),
    (
        c"Unity.Collections.LowLevel.Unsafe.UnsafeUtility::FreeTracked",
        c"FreeTracked",
    ),
    (
        c"Unity.Collections.LowLevel.Unsafe.UnsafeUtility::MemCmp",
        c"MemCmp",
    ),
    (
        c"Unity.Collections.LowLevel.Unsafe.UnsafeUtility::MemCpyReplicate",
        c"MemCpyReplicate",
    ),
    (
        c"Unity.Collections.LowLevel.Unsafe.UnsafeUtility::MemCpyStride",
        c"MemCpyStride",
    ),
];
static TARGETS: OnceLock<[u64; BINDINGS.len()]> = OnceLock::new();
// The frozen runtime does not export mono_gc_is_finalizer_thread. Its public
// profiler events bracket the actual managed finalizer invocation instead.
// A foreign caller is admitted only inside this authenticated runtime scope.
thread_local! {
    static FINALIZING: Cell<usize> = const { Cell::new(0) };
    static FINALIZER_EXECUTION: RefCell<Option<super::burst_shared::Execution>> = const { RefCell::new(None) };
    static INPUT_SPANS: RefCell<BTreeMap<usize, usize>> = const { RefCell::new(BTreeMap::new()) };
    static TEMP_DEPTH: Cell<usize> = const { Cell::new(0) };
}
pub(super) struct TemporaryScope(usize, PhantomData<Rc<()>>);
impl TemporaryScope {
    pub(super) fn enter() -> Result<Self, String> {
        TEMP_DEPTH.with(|depth| {
            let next = depth
                .get()
                .checked_add(1)
                .ok_or("temporary invocation depth overflow")?;
            depth.set(next);
            Ok(Self(next, PhantomData))
        })
    }
    pub(super) fn verify(&self) -> Result<(), String> {
        if self.0 == 1
            && ARENA.with(|arena| {
                arena
                    .borrow()
                    .live
                    .values()
                    .any(|allocation| allocation.allocator == 2)
            })
        {
            return Err("native Temp allocation survived its synchronous IL invocation; engine resume denied".into());
        }
        Ok(())
    }
}
impl Drop for TemporaryScope {
    fn drop(&mut self) {
        TEMP_DEPTH.with(|depth| {
            assert_eq!(depth.get(), self.0);
            depth.set(self.0 - 1);
        });
    }
}
/// Scoped native copy access to a privately owned input-event snapshot. This
/// capability permits no allocator/free operation and never maps host pointers.
pub(super) struct InputSpan {
    words: Vec<u64>,
    length: usize,
    _owner_only: PhantomData<Rc<()>>,
}
impl InputSpan {
    pub(super) fn new(length: usize) -> Result<Self, String> {
        if length == 0 || length > 1024 * 1024 {
            return Err("input copy span length mismatch".into());
        }
        let mut words = vec![0u64; length.div_ceil(8)];
        let base = words.as_mut_ptr() as usize;
        INPUT_SPANS.with(|spans| {
            let mut spans = spans.borrow_mut();
            if spans.len() >= 8 || spans.contains_key(&base) {
                return Err("input copy span capacity/identity mismatch".into());
            }
            spans.insert(base, length);
            Ok(Self {
                words,
                length,
                _owner_only: PhantomData,
            })
        })
    }
    pub(super) fn pointer(&self) -> usize {
        self.words.as_ptr() as usize
    }
    pub(super) fn bytes(&self, length: usize) -> Result<&[u8], String> {
        if length > self.length {
            return Err("input copy span exceeds backing".into());
        }
        Ok(unsafe { std::slice::from_raw_parts(self.words.as_ptr().cast(), length) })
    }
    pub(super) fn bytes_mut(&mut self, length: usize) -> Result<&mut [u8], String> {
        if length > self.length {
            return Err("input copy span exceeds backing".into());
        }
        Ok(unsafe { std::slice::from_raw_parts_mut(self.words.as_mut_ptr().cast(), length) })
    }
}
impl Drop for InputSpan {
    fn drop(&mut self) {
        INPUT_SPANS.with(|spans| {
            spans.borrow_mut().remove(&self.pointer());
        });
    }
}
#[cfg(test)]
fn input_span_contains(pointer: usize, size: usize) -> bool {
    INPUT_SPANS.with(|spans| {
        spans
            .borrow()
            .range(..=pointer)
            .next_back()
            .is_some_and(|(&base, &length)| {
                pointer
                    .checked_sub(base)
                    .and_then(|offset| offset.checked_add(size))
                    .is_some_and(|end| end <= length)
            })
    })
}
pub(super) fn export_input_span(pointer: usize, size: usize) -> Result<(u64, Vec<u8>), String> {
    let (_, guest) = ARENA.with(|arena| arena.borrow().range(pointer, size))?;
    let mut bytes = vec![0; size];
    unsafe { std::ptr::copy_nonoverlapping(pointer as *const u8, bytes.as_mut_ptr(), size) };
    Ok((guest, bytes))
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReleaseState {
    Live,
    Queued,
    Releasing,
    Retired,
}
struct ReleaseRecord {
    allocator: i32,
    tracked: bool,
    state: ReleaseState,
}
struct Releases {
    owner: ThreadId,
    records: BTreeMap<usize, ReleaseRecord>,
    queued: usize,
    drained: usize,
    closed: bool,
}
impl Releases {
    fn new() -> Self {
        Self {
            owner: thread::current().id(),
            records: BTreeMap::new(),
            queued: 0,
            drained: 0,
            closed: false,
        }
    }
    fn register(&mut self, pointer: usize, allocator: i32) -> Result<(), String> {
        self.register_kind(pointer, allocator, false)
    }
    fn register_kind(
        &mut self,
        pointer: usize,
        allocator: i32,
        tracked: bool,
    ) -> Result<(), String> {
        if self.owner != thread::current().id()
            || self.closed
            || self.records.len() >= COUNT
            || self.records.contains_key(&pointer)
        {
            return Err("native buffer release registry owner/capacity mismatch".into());
        }
        self.records.insert(
            pointer,
            ReleaseRecord {
                allocator,
                tracked,
                state: ReleaseState::Live,
            },
        );
        Ok(())
    }
    fn queue(&mut self, pointer: usize, allocator: i32, finalizing: bool) -> Result<(), String> {
        if self.closed || self.owner == thread::current().id() || !finalizing {
            return Err(
                "foreign native buffer Free requires an actual Mono finalizer scope".into(),
            );
        }
        let record = self
            .records
            .get_mut(&pointer)
            .filter(|r| r.allocator == allocator && r.state == ReleaseState::Live)
            .ok_or("finalizer Free requires exact live base/allocator and one release")?;
        record.state = ReleaseState::Queued;
        self.queued += 1;
        Ok(())
    }
    fn validate_family(&self, pointer: usize, allocator: i32, tracked: bool) -> Result<(), String> {
        if self
            .records
            .get(&pointer)
            .is_none_or(|r| r.allocator != allocator || r.tracked != tracked)
        {
            return Err("native Free requires matching tracked allocation family".into());
        }
        Ok(())
    }
    fn begin(&mut self, pointer: usize, allocator: i32, queued: bool) -> Result<(), String> {
        if self.owner != thread::current().id() {
            return Err("guest Free must execute on the native buffer owner".into());
        }
        let expected = if queued {
            ReleaseState::Queued
        } else {
            ReleaseState::Live
        };
        let record = self
            .records
            .get_mut(&pointer)
            .filter(|r| r.allocator == allocator && r.state == expected)
            .ok_or("native buffer Free is duplicate, pending or retired")?;
        record.state = ReleaseState::Releasing;
        Ok(())
    }
    fn complete(&mut self, pointer: usize, queued: bool) -> Result<(), String> {
        let record = self
            .records
            .get_mut(&pointer)
            .filter(|r| r.state == ReleaseState::Releasing)
            .ok_or("native buffer release completion state mismatch")?;
        record.state = ReleaseState::Retired;
        self.drained += usize::from(queued);
        Ok(())
    }
    fn live(&self, pointer: usize) -> bool {
        self.records
            .get(&pointer)
            .is_some_and(|r| r.state == ReleaseState::Live)
    }
}
static RELEASES: OnceLock<Mutex<Releases>> = OnceLock::new();
pub(super) fn actual_finalizer_scope() -> bool {
    FINALIZING.with(|scope| scope.get() != 0)
}
unsafe extern "C" fn finalizing(_: Object, object: Object) {
    checked(|| {
        FINALIZING.with(|scope| -> Result<(), String> {
            if object.is_null() || scope.replace(object as usize) != 0 {
                return Err("nested/NULL Mono finalizer profiler scope".into());
            }
            Ok(())
        })?;
        let execution = super::burst_shared::execution()?;
        FINALIZER_EXECUTION.with(|slot| {
            *slot.borrow_mut() = Some(execution);
        });
        Ok(())
    });
}
unsafe extern "C" fn finalized(_: Object, object: Object) {
    checked(|| {
        FINALIZING.with(|scope| -> Result<(), String> {
            if object.is_null() || scope.replace(0) != object as usize {
                return Err("Mono finalizer profiler scope completion mismatch".into());
            }
            Ok(())
        })?;
        FINALIZER_EXECUTION.with(|slot| {
            drop(slot.borrow_mut().take());
        });
        Ok(())
    });
}
fn ensure_live(pointer: usize) -> Result<(), String> {
    if let Some(releases) = RELEASES.get() {
        if !releases
            .lock()
            .map_err(|_| "native release registry poisoned")?
            .live(pointer)
        {
            return Err("native buffer span has a pending/retired release".into());
        }
    }
    Ok(())
}
struct Bytes {
    pointer: NonNull<u8>,
    layout: Layout,
    owner: ThreadId,
    _owner_only: PhantomData<Rc<()>>,
}
impl Bytes {
    fn new(size: usize, alignment: usize) -> Result<Self, String> {
        let layout = Layout::from_size_align(size, alignment)
            .map_err(|_| "native buffer layout overflow")?;
        let pointer = NonNull::new(unsafe { alloc_zeroed(layout) })
            .ok_or("native buffer allocation failed")?;
        Ok(Self {
            pointer,
            layout,
            owner: thread::current().id(),
            _owner_only: PhantomData,
        })
    }
    fn initialize(&self, bytes: &[u8]) -> Result<(), String> {
        if self.owner != thread::current().id() || bytes.len() != self.layout.size() {
            return Err("native buffer initialization owner/size mismatch".into());
        }
        // No slice/reference into native data escapes. Later raw IL access is
        // confined to the live synchronous owner; this backing is never mapped.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.pointer.as_ptr(), bytes.len())
        };
        Ok(())
    }
}
impl Drop for Bytes {
    fn drop(&mut self) {
        assert_eq!(self.owner, thread::current().id());
        unsafe { dealloc(self.pointer.as_ptr(), self.layout) };
    }
}
struct Allocation {
    guest: u64,
    allocator: i32,
    bytes: Bytes,
}
#[derive(Default)]
struct Arena {
    live: BTreeMap<usize, Allocation>,
    // Keep retired host addresses/storage until Mono cleanup. A stale native
    // pointer cannot accidentally match a newly allocated buffer at that base.
    retired: Vec<Bytes>,
    reserved: usize,
    pending_count: usize,
    retained: usize,
    allocations: usize,
    frees: usize,
    cleanup_frees: usize,
    memsets: usize,
    copies: usize,
    comparisons: usize,
    closed: bool,
}
impl Arena {
    fn reserve(&mut self, size: usize) -> Result<(), String> {
        if self.closed
            || self.allocations + self.pending_count >= COUNT
            || self
                .retained
                .checked_add(self.reserved)
                .and_then(|n| n.checked_add(size))
                .is_none_or(|n| n > CAPACITY)
        {
            return Err("native IL buffer owner is closed or exceeds retained capacity".into());
        }
        self.reserved += size;
        self.pending_count += 1;
        Ok(())
    }
    fn lookup(&self, pointer: usize, allocator: i32) -> Result<&Allocation, String> {
        self.live
            .get(&pointer)
            .filter(|a| a.allocator == allocator)
            .ok_or_else(|| {
                "native buffer free requires an exact live base and matching allocator".into()
            })
    }
    fn range(&self, pointer: usize, size: usize) -> Result<(usize, u64), String> {
        let (&base, allocation) = self
            .live
            .range(..=pointer)
            .next_back()
            .ok_or("native buffer span has no live owner")?;
        ensure_live(base)?;
        let offset = pointer
            .checked_sub(base)
            .ok_or("native buffer span underflow")?;
        if offset
            .checked_add(size)
            .is_none_or(|end| end > allocation.bytes.layout.size())
        {
            return Err("native buffer span crosses its exact allocation".into());
        }
        Ok((base, allocation.guest + offset as u64))
    }
    /// The native IL caller owns both valid native spans, as with cpblk/ldind.
    /// This operation never translates or publishes an address to the guest.
    unsafe fn copy(
        &mut self,
        destination: usize,
        source: usize,
        size: usize,
    ) -> Result<(), String> {
        if size == 0 {
            self.copies += 1;
            return Ok(());
        }
        native_range(destination, size)
            .map_err(|e| format!("native copy destination: {e}; size={size}"))?;
        native_range(source, size).map_err(|e| format!("native copy source: {e}; size={size}"))?;
        if size != 0 {
            // Data stays wholly native. Overlap is handled as memmove; no
            // possibly pointer-bearing bytes are ever copied into guest Memory.
            unsafe { std::ptr::copy(source as *const u8, destination as *mut u8, size) };
        }
        self.copies += 1;
        Ok(())
    }
    /// Caller guarantees both readable native spans. Managed stack/field
    /// addresses are valid here; they need no engine-allocation counterpart.
    unsafe fn compare(&mut self, left: usize, right: usize, size: usize) -> Result<i32, String> {
        let result = if size == 0 {
            0
        } else {
            native_range(left, size)?;
            native_range(right, size)?;
            unsafe { libc::memcmp(left as *const _, right as *const _, size) }
        };
        self.comparisons += 1;
        Ok(result)
    }
    /// Caller guarantees a writable native span; no guest memory is touched.
    unsafe fn fill(&mut self, pointer: usize, value: u8, size: usize) -> Result<(), String> {
        if size != 0 {
            native_range(pointer, size)?;
            unsafe { libc::memset(pointer as *mut _, value.into(), size) };
        }
        self.memsets += 1;
        Ok(())
    }
    /// Caller owns valid native spans covering the complete copy envelopes.
    unsafe fn copy_elements(
        &mut self,
        destination: usize,
        source: usize,
        pattern: CopyPattern,
    ) -> Result<(), String> {
        let (destination_size, source_size) = pattern.spans()?;
        if destination_size == 0 {
            self.copies += 1;
            return Ok(());
        }
        native_range(destination, destination_size)?;
        native_range(source, source_size)?;
        let destination_end = destination
            .checked_add(destination_size)
            .ok_or("strided destination address overflow")?;
        let source_end = source
            .checked_add(source_size)
            .ok_or("strided source address overflow")?;
        // These operations have memcpy semantics. Keep source bytes unchanged
        // across every element; reject intersecting envelopes before any write.
        if destination < source_end && source < destination_end {
            return Err("replicated/strided copy requires disjoint source and destination".into());
        }
        for index in 0..pattern.count {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    (source + index * pattern.source_stride) as *const u8,
                    (destination + index * pattern.destination_stride) as *mut u8,
                    pattern.element_size,
                );
            }
        }
        self.copies += 1;
        Ok(())
    }
    fn insert(&mut self, allocation: Allocation) -> Result<usize, String> {
        let pointer = allocation.bytes.pointer.as_ptr() as usize;
        let end = allocation
            .guest
            .checked_add(allocation.bytes.layout.size() as u64)
            .ok_or("engine allocation range overflow")?;
        if self.closed
            || self.live.contains_key(&pointer)
            || self
                .live
                .values()
                .any(|a| a.guest < end && allocation.guest < a.guest + a.bytes.layout.size() as u64)
        {
            return Err(
                "native buffer owner closed or duplicate/overlapping engine allocation".into(),
            );
        }
        self.retained += allocation.bytes.layout.size();
        self.allocations += 1;
        self.live.insert(pointer, allocation);
        Ok(pointer)
    }
    fn retire(&mut self, pointer: usize, allocator: i32) -> Result<(), String> {
        self.lookup(pointer, allocator)?;
        let allocation = self.live.remove(&pointer).unwrap();
        self.retired.push(allocation.bytes);
        self.frees += 1;
        Ok(())
    }
}
// Arithmetic validation does not infer allocation ownership. Pure native
// UnsafeUtility calls have the same unsafe-pointer contract as Mono's native
// ldind/cpblk implementation; exported guest spans still require Arena::range.
fn native_range(pointer: usize, size: usize) -> Result<(), String> {
    if pointer == 0 || pointer.checked_add(size).is_none() {
        return Err("native memory operation has a null/overflowing range".into());
    }
    Ok(())
}
#[derive(Clone, Copy)]
struct CopyPattern {
    destination_stride: usize,
    source_stride: usize,
    element_size: usize,
    count: usize,
}
impl CopyPattern {
    fn spans(self) -> Result<(usize, usize), String> {
        if self.count == 0 || self.element_size == 0 {
            return Ok((0, 0));
        }
        // Bound both accessed memory and total work, even for zero strides.
        if self
            .element_size
            .checked_mul(self.count)
            .is_none_or(|n| n > CAPACITY)
        {
            return Err("replicated/strided copy exceeds work capacity".into());
        }
        let span = |stride: usize| {
            (self.count - 1)
                .checked_mul(stride)
                .and_then(|n| n.checked_add(self.element_size))
                .filter(|&n| n <= CAPACITY)
                .ok_or_else(|| "replicated/strided copy exceeds span capacity".to_string())
        };
        Ok((span(self.destination_stride)?, span(self.source_stride)?))
    }
}
thread_local! { static ARENA: RefCell<Arena> = RefCell::new(Arena::default()); }
struct Reservation(usize);
impl Reservation {
    fn new(size: usize) -> Result<Self, String> {
        ARENA.with(|arena| arena.borrow_mut().reserve(size))?;
        Ok(Self(size))
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        ARENA.with(|arena| {
            let mut arena = arena.borrow_mut();
            arena.reserved -= self.0;
            arena.pending_count -= 1;
        });
    }
}
pub(super) fn installed() -> bool {
    TARGETS.get().is_some()
}
pub(super) fn install(
    library: Object,
    images: &[Object],
    registry: &bindings::Registry,
) -> Result<(), String> {
    macro_rules! sym {
        ($name:literal, $ty:ty) => {{
            let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
            if p.is_null() {
                return Err(concat!("missing native buffer API: ", $name).into());
            }
            unsafe { std::mem::transmute::<Object, $ty>(p) }
        }};
    }
    let find = sym!(
        "mono_class_from_name",
        unsafe extern "C" fn(Object, *const std::ffi::c_char, *const std::ffi::c_char) -> Object
    );
    let method = sym!(
        "mono_class_get_method_from_name",
        unsafe extern "C" fn(Object, *const std::ffi::c_char, i32) -> Object
    );
    let corlib = sym!("mono_get_corlib", unsafe extern "C" fn() -> Object);
    let add = sym!(
        "mono_add_internal_call",
        unsafe extern "C" fn(*const std::ffi::c_char, Object)
    );
    let unique = |namespace: &std::ffi::CStr, name: &std::ffi::CStr| -> Result<Object, String> {
        let mut classes: Vec<_> = images
            .iter()
            .map(|&image| unsafe { find(image, namespace.as_ptr(), name.as_ptr()) })
            .filter(|p| !p.is_null())
            .collect();
        classes.sort_unstable_by_key(|p| *p as usize);
        classes.dedup();
        if classes.len() != 1 {
            return Err(format!(
                "native buffer class identity ambiguous: {namespace:?}.{name:?}"
            ));
        }
        Ok(classes[0])
    };
    let owner = unique(c"Unity.Collections.LowLevel.Unsafe", c"UnsafeUtility")?;
    let allocator = unique(c"Unity.Collections", c"Allocator")?;
    let corlib = unsafe { corlib() };
    if !images.contains(&corlib) {
        return Err("native buffer corelib is not owned".into());
    }
    let system = |name: &std::ffi::CStr| unsafe { find(corlib, c"System".as_ptr(), name.as_ptr()) };
    let classes = [
        system(c"Void"),
        system(c"Int64"),
        system(c"Int32"),
        allocator,
        system(c"IntPtr"),
    ];
    let api = managed_calls::Api::load(library)?;
    let mut targets = [0; BINDINGS.len()];
    for (index, &(name, member)) in BINDINGS.iter().enumerate() {
        targets[index] = registry
            .registered_target(name.to_str().map_err(|e| e.to_string())?)?
            .ok_or("native buffer operation absent from actual Unity registration")?;
        let method = unsafe {
            method(
                owner,
                member.as_ptr(),
                match index {
                    0 | 8 => 4,
                    1 => 2,
                    5 => 5,
                    6 => 2,
                    9 => 6,
                    _ => 3,
                },
            )
        };
        if index == 2 {
            api.buffer_memset_icall(
                method,
                owner,
                &[classes[0], system(c"Byte"), classes[1]],
                images,
            )?;
        } else if matches!(index, 3 | 4 | 7 | 8 | 9) {
            let operation = match index {
                7 => managed_calls::BufferOperation::Compare,
                8 => managed_calls::BufferOperation::Replicate,
                9 => managed_calls::BufferOperation::Stride,
                _ => managed_calls::BufferOperation::Copy,
            };
            api.buffer_memory_icall(
                method,
                owner,
                operation,
                &[classes[0], classes[1], classes[2]],
                images,
            )?;
        } else if index == 5 {
            api.tracked_allocator_icall(method, owner, &classes, images)?;
        } else {
            api.allocator_icall(method, owner, index == 1 || index == 6, &classes, images)?;
        }
    }
    let create_profiler = sym!(
        "mono_profiler_create",
        unsafe extern "C" fn(Object) -> Object
    );
    let set_begin = sym!(
        "mono_profiler_set_gc_finalizing_object_callback",
        unsafe extern "C" fn(Object, unsafe extern "C" fn(Object, Object))
    );
    let set_end = sym!(
        "mono_profiler_set_gc_finalized_object_callback",
        unsafe extern "C" fn(Object, unsafe extern "C" fn(Object, Object))
    );
    RELEASES
        .set(Mutex::new(Releases::new()))
        .map_err(|_| "native buffer release registry already installed")?;
    let profiler = unsafe { create_profiler(std::ptr::null_mut()) };
    if profiler.is_null() {
        return Err("native buffer finalizer profiler creation failed".into());
    }
    unsafe {
        set_begin(profiler, finalizing);
        set_end(profiler, finalized);
    }
    TARGETS
        .set(targets)
        .map_err(|_| "native buffer bindings already installed")?;
    let adapters: [Object; BINDINGS.len()] = [
        malloc as *const () as Object,
        free as *const () as Object,
        memset as *const () as Object,
        memcpy as *const () as Object,
        memcpy as *const () as Object,
        malloc_tracked as *const () as Object,
        free_tracked as *const () as Object,
        memcmp as *const () as Object,
        memcpy_replicate as *const () as Object,
        memcpy_stride as *const () as Object,
    ];
    for ((name, _), adapter) in BINDINGS.into_iter().zip(adapters) {
        unsafe { add(name.as_ptr(), adapter) };
    }
    Ok(())
}
fn request(
    size: i64,
    alignment: i32,
    allocator: i32,
    context: usize,
) -> Result<(usize, usize), String> {
    // Temp is admitted only inside a synchronous native invocation, and must
    // be explicitly released before its outermost owner scope returns to the
    // engine scheduler. Custom/context allocators remain outside this contract.
    if !(1..=CAPACITY as i64).contains(&size)
        || alignment <= 0
        || !(alignment as usize).is_power_of_two()
        || alignment > 4096
        || !(matches!(allocator, 3 | 4)
            || (allocator == 2 && TEMP_DEPTH.with(|depth| depth.get() != 0)))
        || context != 0
    {
        return Err(format!("unsupported native buffer request: size={size}; alignment={alignment}; allocator={allocator}; context={context:#x}"));
    }
    Ok((size as usize, alignment as usize))
}
fn allocate(size: i64, alignment: i32, allocator: i32, context: usize) -> Result<Object, String> {
    allocate_kind(size, alignment, allocator, context, None)
}
fn allocate_kind(
    size: i64,
    alignment: i32,
    allocator: i32,
    context: usize,
    skip: Option<i32>,
) -> Result<Object, String> {
    callbacks::with_service(|_, _| Ok(()))?;
    let (size, alignment) = request(size, alignment, allocator, context)?;
    let targets = TARGETS
        .get()
        .ok_or("native buffer bindings not installed")?;
    let _reservation = Reservation::new(size)?;
    let bytes = Bytes::new(size, alignment)?;
    // Execute the actual Unity allocator with its complete four-argument ABI.
    // No registry borrow or native pointer is retained by guest engine code.
    let guest = if let Some(skip) = skip {
        if !(0..=1024).contains(&skip) {
            return Err("tracked allocation stack skip exceeds bound".into());
        }
        callbacks::guest_arguments(
            targets[5],
            &[
                size as u64,
                alignment as u64,
                allocator as u32 as u64,
                skip as u64,
                0,
            ],
        )?
    } else {
        callbacks::guest_arguments(
            targets[0],
            &[size as u64, alignment as u64, allocator as u32 as u64, 0],
        )?
    };
    if guest == 0 {
        return Ok(std::ptr::null_mut());
    }
    let prepared = (|| -> Result<usize, String> {
        let mut initial = vec![0; size];
        callbacks::with_service(|memory, runtime| {
            if !guest.is_multiple_of(alignment as u64) {
                return Err("original engine allocator returned a misaligned buffer".into());
            }
            memory.check(
                guest,
                size,
                crate::x64::memory::READ | crate::x64::memory::WRITE,
            )?;
            // Split at actual page boundaries to verify the complete range,
            // not only its first page, against code history and ownership.
            let mut at = guest;
            let end = guest
                .checked_add(size as u64)
                .ok_or("engine buffer overflow")?;
            while at < end {
                let width = (end - at).min(4096 - (at & 4095)) as usize;
                if !memory.owned_noncode_access(at, width)
                    || !(runtime.live_data_slot(at, width as u64)
                        || memory.guest_private_data_slot(at, width))
                {
                    return Err(
                        "original engine buffer is not wholly live owned noncode data".into(),
                    );
                }
                at += width as u64;
            }
            memory.read(guest, &mut initial)
        })?;
        bytes.initialize(&initial)?;
        let pointer = ARENA.with(|arena| {
            arena.borrow_mut().insert(Allocation {
                guest,
                allocator,
                bytes,
            })
        })?;
        let mut releases = RELEASES
            .get()
            .ok_or("native buffer release registry absent")?
            .lock()
            .map_err(|_| "native release registry poisoned")?;
        if skip.is_some() {
            releases.register_kind(pointer, allocator, true)?;
        } else {
            releases.register(pointer, allocator)?;
        }
        Ok(pointer)
    })();
    match prepared {
        Ok(pointer) => {
            eprintln!("NATIVE_MONO_UNSAFE_BUFFER_ALLOCATED: size={size}; alignment={alignment}; allocator={allocator}; original-engine-allocation={guest:#x}; native-address-private=true");
            Ok(pointer as Object)
        }
        Err(error) => {
            callbacks::guest_arguments(
                targets[if skip.is_some() { 6 } else { 1 }],
                &[guest, allocator as u32 as u64],
            )?;
            Err(error)
        }
    }
}
fn release(pointer: usize, allocator: i32) -> Result<(), String> {
    release_kind(pointer, allocator, false)
}
fn release_kind(pointer: usize, allocator: i32, tracked: bool) -> Result<(), String> {
    let releases = RELEASES
        .get()
        .ok_or("native buffer release registry absent")?;
    let owner = releases
        .lock()
        .map_err(|_| "native release registry poisoned")?
        .owner;
    if pointer == 0 && owner == thread::current().id() {
        callbacks::guest_arguments(
            TARGETS.get().ok_or("native buffer bindings absent")?[if tracked { 6 } else { 1 }],
            &[0, allocator as u32 as u64],
        )?;
        return Ok(());
    }
    releases
        .lock()
        .map_err(|_| "native release registry poisoned")?
        .validate_family(pointer, allocator, tracked)?;
    if owner != thread::current().id() {
        // The finalizer never borrows guest Memory, touches native bytes or
        // waits for the owner (Mono cleanup may be waiting for this thread).
        releases
            .lock()
            .map_err(|_| "native release registry poisoned")?
            .queue(
                pointer,
                allocator,
                FINALIZING.with(|scope| scope.get() != 0),
            )?;
        eprintln!("NATIVE_MONO_UNSAFE_BUFFER_FINALIZER_QUEUED: allocator={allocator}; runtime-finalizer-scope=true");
        return Ok(());
    }
    release_owned(pointer, allocator, false)
}
fn release_owned(pointer: usize, allocator: i32, queued: bool) -> Result<(), String> {
    callbacks::with_service(|_, _| Ok(()))?;
    let targets = TARGETS
        .get()
        .ok_or("native buffer bindings not installed")?;
    if pointer == 0 {
        callbacks::guest_arguments(targets[1], &[0, allocator as u32 as u64])?;
        return Ok(());
    }
    let guest = ARENA.with(|arena| arena.borrow().lookup(pointer, allocator).map(|a| a.guest))?;
    let releases = RELEASES
        .get()
        .ok_or("native buffer release registry absent")?;
    releases
        .lock()
        .map_err(|_| "native release registry poisoned")?
        .begin(pointer, allocator, queued)?;
    let tracked = releases
        .lock()
        .map_err(|_| "native release registry poisoned")?
        .records[&pointer]
        .tracked;
    // No arena or release lock crosses engine execution or native IL reentry.
    callbacks::guest_arguments(
        targets[if tracked { 6 } else { 1 }],
        &[guest, allocator as u32 as u64],
    )?;
    ARENA.with(|arena| arena.borrow_mut().retire(pointer, allocator))?;
    releases
        .lock()
        .map_err(|_| "native release registry poisoned")?
        .complete(pointer, queued)?;
    eprintln!("NATIVE_MONO_UNSAFE_BUFFER_FREED: allocator={allocator}; original-engine-allocation={guest:#x}; finalizer-queue={queued}; native-address-private=true");
    Ok(())
}
/// Drain at owner entry/exit, and after Mono has joined its finalizers. Bytes
/// stay allocated until VM cleanup even after logical release. No guest API is
/// executed by a foreign thread and no lock is held across the engine callback.
pub(super) fn drain_finalizers() -> Result<(), String> {
    let Some(releases) = RELEASES.get() else {
        return Ok(());
    };
    let queued = {
        let releases = releases
            .lock()
            .map_err(|_| "native release registry poisoned")?;
        if releases.owner != thread::current().id() {
            return Err("native finalizer release drain on foreign owner".into());
        }
        releases
            .records
            .iter()
            .filter(|(_, r)| r.state == ReleaseState::Queued)
            .map(|(&p, r)| (p, r.allocator))
            .collect::<Vec<_>>()
    };
    for (pointer, allocator) in queued {
        release_owned(pointer, allocator, true)?;
    }
    Ok(())
}
unsafe fn fill(pointer: usize, value: u8, size: i64) -> Result<(), String> {
    callbacks::with_service(|_, _| Ok(()))?;
    if !(0..=CAPACITY as i64).contains(&size) {
        return Err("native MemSet size exceeds the owned span contract".into());
    }
    let size = size as usize;
    ARENA.with(|arena| unsafe { arena.borrow_mut().fill(pointer, value, size) })?;
    eprintln!("NATIVE_MONO_UNSAFE_BUFFER_MEMSET_VERIFIED: size={size}; value={value:#x}; native-address-private=true");
    Ok(())
}
fn checked<T>(operation: impl FnOnce() -> Result<T, String>) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)) {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            eprintln!("NATIVE_MONO_UNSAFE_BUFFER_REJECT: {error}");
            std::process::abort();
        }
        Err(_) => {
            eprintln!("NATIVE_MONO_UNSAFE_BUFFER_REJECT: adapter panicked");
            std::process::abort();
        }
    }
}
unsafe extern "C" fn malloc(size: i64, alignment: i32, allocator: i32, context: usize) -> Object {
    checked(|| allocate(size, alignment, allocator, context))
}
unsafe extern "C" fn free(pointer: Object, allocator: i32) {
    checked(|| release(pointer as usize, allocator));
}
unsafe extern "C" fn malloc_tracked(
    size: i64,
    alignment: i32,
    allocator: i32,
    skip: i32,
    context: usize,
) -> Object {
    checked(|| allocate_kind(size, alignment, allocator, context, Some(skip)))
}
unsafe extern "C" fn free_tracked(pointer: Object, allocator: i32) {
    checked(|| release_kind(pointer as usize, allocator, true));
}
unsafe extern "C" fn memset(pointer: Object, value: u8, size: i64) {
    checked(|| unsafe { fill(pointer as usize, value, size) });
}
unsafe extern "C" fn memcpy(destination: Object, source: Object, size: i64) {
    checked(|| {
        callbacks::with_service(|_, _| Ok(()))?;
        if !(0..=CAPACITY as i64).contains(&size) {
            return Err("native buffer copy exceeds the owned span contract".into());
        }
        ARENA.with(|arena| unsafe {
            arena
                .borrow_mut()
                .copy(destination as usize, source as usize, size as usize)
        })?;
        eprintln!(
            "NATIVE_MONO_UNSAFE_BUFFER_COPY_VERIFIED: size={size}; native-address-private=true"
        );
        Ok(())
    });
}
unsafe extern "C" fn memcmp(left: Object, right: Object, size: i64) -> i32 {
    checked(|| {
        callbacks::with_service(|_, _| Ok(()))?;
        if !(0..=CAPACITY as i64).contains(&size) {
            return Err("native buffer compare exceeds the owned span contract".into());
        }
        let result = ARENA.with(|arena| unsafe {
            arena
                .borrow_mut()
                .compare(left as usize, right as usize, size as usize)
        })?;
        eprintln!("NATIVE_MONO_UNSAFE_BUFFER_COMPARE_VERIFIED: size={size}; result={result}; native-address-private=true");
        Ok(result)
    })
}
unsafe extern "C" fn memcpy_replicate(destination: Object, source: Object, size: i32, count: i32) {
    memcpy_stride(destination, size, source, 0, size, count);
}
unsafe extern "C" fn memcpy_stride(
    destination: Object,
    destination_stride: i32,
    source: Object,
    source_stride: i32,
    element_size: i32,
    count: i32,
) {
    checked(|| {
        callbacks::with_service(|_, _| Ok(()))?;
        if [destination_stride, source_stride, element_size, count]
            .iter()
            .any(|&n| n < 0)
        {
            return Err("native strided copy requires nonnegative dimensions".into());
        }
        ARENA.with(|arena| unsafe {
            arena.borrow_mut().copy_elements(
                destination as usize,
                source as usize,
                CopyPattern {
                    destination_stride: destination_stride as usize,
                    source_stride: source_stride as usize,
                    element_size: element_size as usize,
                    count: count as usize,
                },
            )
        })?;
        eprintln!("NATIVE_MONO_UNSAFE_BUFFER_STRIDED_COPY_VERIFIED: element-size={element_size}; count={count}; native-address-private=true");
        Ok(())
    });
}
/// Called only after Mono cleanup has returned: no native IL or finalizer can
/// still use these private addresses. This is finite diagnostic teardown, not
/// a replacement for the production allocator/scheduler/shared-buffer contract.
pub(super) fn retire_after_vm_cleanup() -> Result<(), String> {
    drain_finalizers()?;
    let remaining = ARENA.with(|arena| -> Result<Vec<(usize, i32)>, String> {
        let mut arena = arena.borrow_mut();
        if arena.reserved != 0 || arena.pending_count != 0 {
            return Err("native buffer allocation still pending at VM shutdown".into());
        }
        arena.closed = true;
        Ok(arena
            .live
            .iter()
            .map(|(&p, a)| (p, a.allocator))
            .collect::<Vec<_>>())
    })?;
    for (pointer, allocator) in remaining {
        let tracked = RELEASES
            .get()
            .ok_or("native release registry absent")?
            .lock()
            .map_err(|_| "native release registry poisoned")?
            .records[&pointer]
            .tracked;
        release_kind(pointer, allocator, tracked)?;
        ARENA.with(|arena| arena.borrow_mut().cleanup_frees += 1);
    }
    ARENA.with(|arena| {
        let mut arena = arena.borrow_mut();
        arena.retired.clear();
        arena.retained = 0;
    });
    if let Some(releases) = RELEASES.get() {
        let mut releases = releases
            .lock()
            .map_err(|_| "native release registry poisoned")?;
        if releases
            .records
            .values()
            .any(|r| r.state != ReleaseState::Retired)
        {
            return Err("native buffer cleanup left pending release records".into());
        }
        releases.closed = true;
    }
    Ok(())
}
pub(super) fn report() -> String {
    let queue = RELEASES.get().map(|r| {
        let r = r.lock().unwrap();
        format!("native-unsafe-buffer-finalizer-queued={}\nnative-unsafe-buffer-finalizer-drained={}\nnative-unsafe-buffer-finalizer-pending={}\n",
            r.queued, r.drained, r.records.values().filter(|r| r.state == ReleaseState::Queued).count())
    }).unwrap_or_default();
    ARENA.with(|arena| {
        let a = arena.borrow();
        format!("native-unsafe-buffer-allocations={}\nnative-unsafe-buffer-frees={}\nnative-unsafe-buffer-memsets={}\nnative-unsafe-buffer-copies={}\nnative-unsafe-buffer-comparisons={}\nnative-unsafe-buffer-cleanup-frees={}\nnative-unsafe-buffer-live={}\nnative-unsafe-buffer-retained-bytes={}\nnative-unsafe-buffer-closed={}\n", a.allocations, a.frees, a.memsets, a.copies, a.comparisons, a.cleanup_frees, a.live.len(), a.retained, a.closed) + &queue
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn temp_scope_rejects_scheduler_resume_until_explicit_release() {
        assert!(request(80, 8, 2, 0).is_err());
        let scope = TemporaryScope::enter().unwrap();
        request(80, 8, 2, 0).unwrap();
        let old = ARENA.with(|arena| std::mem::take(&mut *arena.borrow_mut()));
        let pointer = ARENA.with(|arena| {
            let mut arena = arena.borrow_mut();
            arena.reserve(80).unwrap();
            arena
                .insert(Allocation {
                    guest: 0x1000,
                    allocator: 2,
                    bytes: Bytes::new(80, 8).unwrap(),
                })
                .unwrap()
        });
        assert!(scope.verify().is_err());
        ARENA.with(|arena| arena.borrow_mut().retire(pointer, 2).unwrap());
        scope.verify().unwrap();
        ARENA.with(|arena| *arena.borrow_mut() = old);
        drop(scope);
        assert!(request(80, 8, 2, 0).is_err());
    }
    #[test]
    fn scoped_input_storage_checks_ranges_and_retires_snapshot_identity() {
        let mut source = InputSpan::new(20).unwrap();
        source.bytes_mut(20).unwrap().copy_from_slice(&[0x5a; 20]);
        let destination = InputSpan::new(20).unwrap();
        let mut arena = Arena::default();
        unsafe { arena.copy(destination.pointer(), source.pointer(), 20) }.unwrap();
        assert_eq!(destination.bytes(20).unwrap(), &[0x5a; 20]);
        assert!(source.bytes(21).is_err());
        assert_eq!(destination.bytes(20).unwrap(), &[0x5a; 20]);
        assert!(!input_span_contains(destination.pointer() + 1, 20));
        let retired = source.pointer();
        drop(source);
        assert!(!input_span_contains(retired, 20));
        assert_eq!(destination.bytes(20).unwrap(), &[0x5a; 20]);
        assert!(arena.lookup(destination.pointer(), 4).is_err());
    }
    #[test]
    fn memory_compare_handles_native_stack_values_unsigned_bytes_and_zero_length() {
        let mut arena = Arena::default();
        let left = [0u8, 0x80, 7, 5, 0, 0, 0, 0];
        let right = [0u8, 0x7f, 7, 6, 0, 0, 0, 0];
        let a = left.as_ptr() as usize;
        let b = right.as_ptr() as usize;
        assert!(arena.range(a, 8).is_err()); // Not an engine allocation.
        unsafe {
            assert_eq!(arena.compare(0, usize::MAX, 0).unwrap(), 0);
            assert!(arena.compare(a, b, 8).unwrap() > 0);
            assert!(arena.compare(b, a, 8).unwrap() < 0);
            assert_eq!(arena.compare(a + 2, b + 2, 1).unwrap(), 0);
            assert!(arena.compare(a + 3, b + 3, 1).unwrap() < 0);
            assert!(arena.compare(0, b, 1).is_err());
            assert!(arena.compare(a, usize::MAX, 1).is_err());
        }
        assert_eq!(right, [0, 0x7f, 7, 6, 0, 0, 0, 0]);
        assert_eq!(arena.comparisons, 5);
    }
    #[test]
    fn native_fill_accepts_stack_subranges_and_keeps_surrounding_bytes() {
        let mut arena = Arena::default();
        let mut value = [0xa5u8; 12];
        let pointer = value.as_mut_ptr() as usize;
        assert!(arena.range(pointer, value.len()).is_err());
        unsafe {
            arena.fill(pointer + 2, 0x80, 8).unwrap();
            assert_eq!(
                value,
                [0xa5, 0xa5, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0xa5, 0xa5]
            );
            arena.fill(pointer + 4, 0, 4).unwrap();
            assert_eq!(
                value,
                [0xa5, 0xa5, 0x80, 0x80, 0, 0, 0, 0, 0x80, 0x80, 0xa5, 0xa5]
            );
            arena.fill(0, 0xff, 0).unwrap();
            assert!(arena.fill(0, 0, 1).is_err());
            assert!(arena.fill(usize::MAX, 0, 1).is_err());
        }
        assert_eq!(arena.memsets, 3);
    }
    #[test]
    fn strided_and_replicated_native_copy_preserve_gaps_and_reject_overlap_and_overflow() {
        let mut arena = Arena::default();
        let source = [1u8, 2, 0xee, 3, 4, 0xee, 5, 6];
        let mut destination = [0xa5u8; 10];
        let src = source.as_ptr() as usize;
        let dst = destination.as_mut_ptr() as usize;
        let pattern = CopyPattern {
            destination_stride: 4,
            source_stride: 3,
            element_size: 2,
            count: 3,
        };
        unsafe { arena.copy_elements(dst, src, pattern) }.unwrap();
        assert_eq!(destination, [1, 2, 0xa5, 0xa5, 3, 4, 0xa5, 0xa5, 5, 6]);
        let before = destination;
        unsafe {
            assert!(arena.copy_elements(0, src, pattern).is_err());
            assert!(arena.copy_elements(dst, usize::MAX, pattern).is_err());
            assert!(arena
                .copy_elements(
                    dst,
                    dst + 1,
                    CopyPattern {
                        count: 2,
                        ..pattern
                    }
                )
                .is_err());
            assert!(arena
                .copy_elements(
                    dst,
                    src,
                    CopyPattern {
                        count: usize::MAX,
                        ..pattern
                    }
                )
                .is_err());
        }
        assert_eq!(destination, before);
        unsafe {
            arena
                .copy_elements(
                    dst,
                    src,
                    CopyPattern {
                        destination_stride: 2,
                        source_stride: 0,
                        element_size: 2,
                        count: 5,
                    },
                )
                .unwrap();
            assert_eq!(destination, [1, 2, 1, 2, 1, 2, 1, 2, 1, 2]);
            arena
                .copy_elements(
                    0,
                    usize::MAX,
                    CopyPattern {
                        count: 0,
                        ..pattern
                    },
                )
                .unwrap();
            arena
                .copy_elements(
                    usize::MAX,
                    0,
                    CopyPattern {
                        element_size: 0,
                        ..pattern
                    },
                )
                .unwrap();
        }
        assert!(CopyPattern {
            destination_stride: usize::MAX,
            ..pattern
        }
        .spans()
        .is_err());
        assert!(CopyPattern {
            destination_stride: 0,
            source_stride: 0,
            element_size: 1,
            count: CAPACITY + 1
        }
        .spans()
        .is_err());
        assert_eq!(arena.copies, 4);
    }
    #[test]
    fn tracked_release_identity_keeps_allocator_and_allocation_family() {
        let mut releases = Releases::new();
        releases.register_kind(0x1000, 4, true).unwrap();
        releases.register(0x2000, 4).unwrap();
        releases.validate_family(0x1000, 4, true).unwrap();
        assert!(releases.validate_family(0x1000, 4, false).is_err());
        assert!(releases.validate_family(0x1000, 3, true).is_err());
        assert!(releases.validate_family(0x2000, 4, true).is_err());
        assert_eq!(releases.records[&0x1000].state, ReleaseState::Live);
    }
    #[test]
    fn finalizer_queue_checks_origin_identity_duplicate_and_owner_drain() {
        let mut releases = Releases::new();
        releases.register(0x1000, 4).unwrap();
        assert!(releases.queue(0x1000, 4, true).is_err()); // owner is synchronous
        let releases = std::sync::Arc::new(Mutex::new(releases));
        let foreign = releases.clone();
        std::thread::spawn(move || {
            let mut r = foreign.lock().unwrap();
            assert!(r.queue(0x1000, 4, false).is_err());
            assert!(r.queue(0x1001, 4, true).is_err());
            assert!(r.queue(0x1000, 3, true).is_err());
            assert!(r.queue(0, 4, true).is_err());
            assert_eq!(r.queued, 0);
            r.queue(0x1000, 4, true).unwrap();
            assert!(r.queue(0x1000, 4, true).is_err());
            assert!(r.begin(0x1000, 4, true).is_err());
        })
        .join()
        .unwrap();
        let mut r = releases.lock().unwrap();
        assert!(!r.live(0x1000));
        assert!(r.begin(0x1000, 4, false).is_err());
        r.begin(0x1000, 4, true).unwrap();
        assert!(r.begin(0x1000, 4, true).is_err());
        r.complete(0x1000, true).unwrap();
        assert_eq!((r.queued, r.drained), (1, 1));
        assert!(r.register(0x1000, 4).is_err());
        assert!(r.begin(0x1000, 4, false).is_err());
        r.closed = true;
        assert!(r.register(0x2000, 4).is_err());
    }
    #[test]
    fn buffer_alignment_private_storage_exact_free_and_no_address_reuse() {
        let bytes = Bytes::new(33, 64).unwrap();
        assert_eq!(bytes.pointer.as_ptr() as usize % 64, 0);
        bytes.initialize(&[7; 33]).unwrap();
        let mut arena = Arena::default();
        let pointer = arena
            .insert(Allocation {
                guest: 0x100000,
                allocator: 4,
                bytes,
            })
            .unwrap();
        assert_ne!(pointer as u64, arena.lookup(pointer, 4).unwrap().guest);
        unsafe {
            (pointer as *mut u8).add(32).write(19);
        }
        assert_eq!(unsafe { (pointer as *const u8).add(32).read() }, 19);
        assert_eq!(arena.range(pointer + 32, 1).unwrap(), (pointer, 0x100020));
        assert!(arena.range(pointer + 32, 2).is_err());
        assert!(arena.range(pointer, usize::MAX).is_err());
        assert!(arena.lookup(pointer + 1, 4).is_err());
        assert!(arena.lookup(pointer, 3).is_err());
        assert!(arena.retire(pointer, 3).is_err());
        assert_eq!(arena.live.len(), 1);
        arena.retire(pointer, 4).unwrap();
        assert!(arena.lookup(pointer, 4).is_err());
        assert!(arena.retire(pointer, 4).is_err());
        let other = Bytes::new(33, 64).unwrap();
        assert_ne!(other.pointer.as_ptr() as usize, pointer);
        assert_eq!(arena.retired.len(), 1);
    }
    #[test]
    fn native_copy_preserves_overlap_and_rejects_null_or_overflow_before_writes() {
        let mut empty = Arena::default();
        unsafe {
            empty.copy(0, 0, 0).unwrap();
            empty.copy(usize::MAX, 1, 0).unwrap();
        }
        assert_eq!(empty.copies, 2);
        assert!(unsafe { empty.copy(0, 0, 1) }.is_err());
        let bytes = Bytes::new(8, 8).unwrap();
        bytes.initialize(&[1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        let mut arena = Arena::default();
        let pointer = arena
            .insert(Allocation {
                guest: 0x300000,
                allocator: 4,
                bytes,
            })
            .unwrap();
        unsafe { arena.copy(pointer + 2, pointer, 6) }.unwrap();
        let read = || unsafe { std::slice::from_raw_parts(pointer as *const u8, 8) }.to_vec();
        assert_eq!(read(), [1, 2, 1, 2, 3, 4, 5, 6]);
        unsafe {
            assert!(arena.copy(pointer, 0, 2).is_err());
            assert!(arena.copy(0, pointer, 2).is_err());
            assert!(arena.copy(pointer, pointer, usize::MAX).is_err());
        }
        assert_eq!(read(), [1, 2, 1, 2, 3, 4, 5, 6]);
        assert_eq!(arena.copies, 1);
        arena.retire(pointer, 4).unwrap();
        // Cross-domain export continues to reject a retired allocation.
        assert!(arena.range(pointer, 1).is_err());
    }
    #[test]
    fn unsupported_allocator_context_and_reentrant_capacity_fail_before_mutation() {
        assert!(request(0, 16, 4, 0).is_err());
        assert!(request(-1, 16, 4, 0).is_err());
        assert!(request(16, 3, 4, 0).is_err());
        assert!(request(16, 16, 2, 0).is_err());
        assert!(request(16, 16, 4, 17).is_err());
        let mut arena = Arena::default();
        arena.reserve(CAPACITY).unwrap();
        assert!(arena.reserve(1).is_err());
        assert_eq!(arena.pending_count, 1);
        assert_eq!(arena.reserved, CAPACITY);
        arena.reserved = 0;
        arena.pending_count = 0;
        arena.closed = true;
        assert!(arena.reserve(1).is_err());
        let bytes = Bytes::new(8, 8).unwrap();
        assert!(arena
            .insert(Allocation {
                guest: 0x200000,
                allocator: 4,
                bytes
            })
            .is_err());
        assert!(arena.live.is_empty());
    }
}
