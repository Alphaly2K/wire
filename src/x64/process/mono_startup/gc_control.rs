//! Actual frozen Mono GC exports. No guest object/pointer enters these ABIs.
use super::Object;
#[derive(Clone, Copy)]
pub(super) struct Api {
    max: unsafe extern "C" fn() -> i32,
    collect: unsafe extern "C" fn(i32),
    little: unsafe extern "C" fn() -> i32,
    start: unsafe extern "C" fn(),
    count: unsafe extern "C" fn(i32) -> i32,
    heap: unsafe extern "C" fn() -> i64,
    used: unsafe extern "C" fn() -> i64,
    slice: unsafe extern "C" fn() -> i64,
    set_slice: unsafe extern "C" fn(i64),
    incremental: unsafe extern "C" fn() -> i32,
    set_incremental: unsafe extern "C" fn(i32),
    pending: unsafe extern "C" fn() -> i32,
}
pub(super) fn handles(function: &str) -> bool {
    matches!(
        function,
        "mono_gc_max_generation"
            | "mono_gc_collect"
            | "mono_gc_collect_a_little"
            | "mono_gc_start_incremental_collection"
            | "mono_gc_collection_count"
            | "mono_gc_get_heap_size"
            | "mono_gc_get_used_size"
            | "mono_gc_get_max_time_slice_ns"
            | "mono_gc_set_max_time_slice_ns"
            | "mono_gc_is_incremental"
            | "mono_gc_set_incremental"
            | "mono_gc_pending_finalizers"
    )
}
pub(super) fn collecting(function: &str) -> bool {
    matches!(
        function,
        "mono_gc_collect"
            | "mono_gc_collect_a_little"
            | "mono_gc_start_incremental_collection"
            | "mono_gc_set_incremental"
    )
}
impl Api {
    pub(super) fn load(library: Object) -> Result<Self, String> {
        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return Err(concat!("missing GC control API: ", $name).into());
                }
                unsafe { std::mem::transmute::<Object, $ty>(p) }
            }};
        }
        Ok(Self {
            max: sym!("mono_gc_max_generation", unsafe extern "C" fn() -> i32),
            collect: sym!("mono_gc_collect", unsafe extern "C" fn(i32)),
            little: sym!("mono_gc_collect_a_little", unsafe extern "C" fn() -> i32),
            start: sym!(
                "mono_gc_start_incremental_collection",
                unsafe extern "C" fn()
            ),
            count: sym!("mono_gc_collection_count", unsafe extern "C" fn(i32) -> i32),
            heap: sym!("mono_gc_get_heap_size", unsafe extern "C" fn() -> i64),
            used: sym!("mono_gc_get_used_size", unsafe extern "C" fn() -> i64),
            slice: sym!(
                "mono_gc_get_max_time_slice_ns",
                unsafe extern "C" fn() -> i64
            ),
            set_slice: sym!("mono_gc_set_max_time_slice_ns", unsafe extern "C" fn(i64)),
            incremental: sym!("mono_gc_is_incremental", unsafe extern "C" fn() -> i32),
            set_incremental: sym!("mono_gc_set_incremental", unsafe extern "C" fn(i32)),
            pending: sym!("mono_gc_pending_finalizers", unsafe extern "C" fn() -> i32),
        })
    }
    pub(super) fn call(self, function: &str, argument: u64) -> Result<u64, String> {
        let max = unsafe { (self.max)() };
        if !(0..=16).contains(&max) {
            return Err("GC max generation outside bounded runtime contract".into());
        }
        let generation = argument as i32;
        if (function == "mono_gc_set_incremental" && argument as u32 > 1)
            || (function == "mono_gc_set_max_time_slice_ns" && (argument as i64) < 0)
        {
            return Err("GC mode/time slice argument outside actual scalar contract".into());
        }
        if matches!(function, "mono_gc_collect" | "mono_gc_collection_count")
            && !(0..=max).contains(&generation)
        {
            return Err("GC generation outside actual runtime range".into());
        }
        Ok(match function {
            "mono_gc_max_generation" => max as u64,
            "mono_gc_collect" => {
                unsafe { (self.collect)(generation) };
                0
            }
            "mono_gc_collect_a_little" => (unsafe { (self.little)() }) as u32 as u64,
            "mono_gc_start_incremental_collection" => {
                unsafe { (self.start)() };
                0
            }
            "mono_gc_collection_count" => (unsafe { (self.count)(generation) }) as u32 as u64,
            "mono_gc_get_heap_size" => (unsafe { (self.heap)() }) as u64,
            "mono_gc_get_used_size" => (unsafe { (self.used)() }) as u64,
            "mono_gc_get_max_time_slice_ns" => (unsafe { (self.slice)() }) as u64,
            "mono_gc_set_max_time_slice_ns" => {
                unsafe { (self.set_slice)(argument as i64) };
                0
            }
            "mono_gc_is_incremental" => (unsafe { (self.incremental)() }) as u32 as u64,
            "mono_gc_pending_finalizers" => (unsafe { (self.pending)() }) as u32 as u64,
            "mono_gc_set_incremental" => {
                unsafe { (self.set_incremental)(argument as i32) };
                0
            }
            _ => return Err("unadmitted GC control export".into()),
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    thread_local! { static COLLECTIONS: Cell<usize> = const { Cell::new(0) }; }
    #[test]
    fn gc_scalar_exports_preserve_wide_sizes_and_reject_generation_before_collection() {
        unsafe extern "C" fn max() -> i32 {
            0
        }
        unsafe extern "C" fn collect(_: i32) {
            COLLECTIONS.with(|n| n.set(n.get() + 1));
        }
        unsafe extern "C" fn start() {
            unsafe { collect(0) };
        }
        unsafe extern "C" fn little() -> i32 {
            unsafe { collect(0) };
            3
        }
        unsafe extern "C" fn count(_: i32) -> i32 {
            COLLECTIONS.with(|n| n.get() as i32)
        }
        unsafe extern "C" fn heap() -> i64 {
            0x1_2345_6789
        }
        unsafe extern "C" fn set_slice(_: i64) {}
        unsafe extern "C" fn set_incremental(value: i32) {
            assert!((0..=1).contains(&value));
        }
        let api = Api {
            max,
            collect,
            little,
            start,
            count,
            heap,
            used: heap,
            slice: heap,
            set_slice,
            incremental: max,
            set_incremental,
            pending: max,
        };
        COLLECTIONS.with(|n| n.set(0));
        assert_eq!(api.call("mono_gc_max_generation", 0).unwrap(), 0);
        for invalid in [1, u32::MAX as u64] {
            assert!(api.call("mono_gc_collect", invalid).is_err());
        }
        assert_eq!(COLLECTIONS.with(Cell::get), 0);
        assert!(api.call("mono_gc_set_incremental", 2).is_err());
        assert!(api.call("mono_gc_set_max_time_slice_ns", u64::MAX).is_err());
        assert_eq!(api.call("mono_gc_get_heap_size", 0).unwrap(), 0x1_2345_6789);
        assert_eq!(api.call("mono_gc_collect_a_little", 0).unwrap(), 3);
        api.call("mono_gc_collect", 0).unwrap();
        api.call("mono_gc_start_incremental_collection", 0).unwrap();
        assert_eq!(api.call("mono_gc_collection_count", 0).unwrap(), 3);
    }
}
