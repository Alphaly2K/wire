//! Account for live compiler allocations inside the fixed Wasm arena.
#[cfg(target_arch = "wasm32")]
mod wasm {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
    static LIVE: AtomicUsize = AtomicUsize::new(0);
    static PEAK: AtomicUsize = AtomicUsize::new(0);
    static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
    struct Allocator;
    fn added(bytes: usize) {
        PEAK.fetch_max(LIVE.fetch_add(bytes, Relaxed) + bytes, Relaxed);
        ALLOCATIONS.fetch_add(1, Relaxed);
    }
    unsafe impl GlobalAlloc for Allocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let pointer = unsafe { System.alloc(layout) };
            if !pointer.is_null() {
                added(layout.size());
            }
            pointer
        }
        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
            unsafe { System.dealloc(pointer, layout) };
            LIVE.fetch_sub(layout.size(), Relaxed);
        }
        unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            let result = unsafe { System.realloc(pointer, layout, size) };
            if !result.is_null() {
                LIVE.fetch_sub(layout.size(), Relaxed);
                added(size);
            }
            result
        }
    }
    #[global_allocator]
    static ALLOCATOR: Allocator = Allocator;

    pub(super) fn stat(which: u32) -> u32 {
        match which {
            0 => LIVE.load(Relaxed) as u32,
            1 => PEAK.load(Relaxed) as u32,
            2 => ALLOCATIONS.load(Relaxed) as u32,
            _ => 0,
        }
    }
}
pub(crate) fn stat(which: u32) -> u32 {
    #[cfg(target_arch = "wasm32")]
    {
        wasm::stat(which)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = which;
        0
    }
}
