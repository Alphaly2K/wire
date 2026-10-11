//! Fixed sample adapters preserve the original engine's disabled-marker path.
//! IntPtr is an owned guest marker identity; it is never dereferenced natively.
use super::{animation, bindings, callbacks, managed_calls, Object};
use std::{cell::RefCell, sync::OnceLock};
const BINDINGS: [(&std::ffi::CStr, &std::ffi::CStr, u64); 2] = [
    (
        c"Unity.Profiling.LowLevel.Unsafe.ProfilerUnsafeUtility::BeginSample",
        c"BeginSample",
        0x180062920,
    ),
    (
        c"Unity.Profiling.LowLevel.Unsafe.ProfilerUnsafeUtility::EndSample",
        c"EndSample",
        0x180062a20,
    ),
];
static TARGETS: OnceLock<[u64; 2]> = OnceLock::new();
#[derive(Default)]
struct Samples {
    stack: Vec<u64>,
    begins: usize,
    ends: usize,
}
impl Samples {
    fn validate(&self, marker: u64, begin: bool) -> Result<(), String> {
        if begin {
            if self.stack.len() >= 64 {
                return Err("profiler sample nesting exceeds owner bound".into());
            }
        } else if self.stack.last() != Some(&marker) {
            return Err("profiler EndSample does not match the live BeginSample".into());
        }
        Ok(())
    }
    fn record(&mut self, marker: u64, begin: bool) {
        if begin {
            self.stack.push(marker);
            self.begins += 1;
        } else {
            self.stack.pop();
            self.ends += 1;
        }
    }
}
thread_local! { static SAMPLES: RefCell<Samples> = RefCell::new(Samples::default()); }
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
                return Err(concat!("missing sample API: ", $name).into());
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
    let mut owners: Vec<_> = images
        .iter()
        .map(|&image| unsafe {
            find(
                image,
                c"Unity.Profiling.LowLevel.Unsafe".as_ptr(),
                c"ProfilerUnsafeUtility".as_ptr(),
            )
        })
        .filter(|p| !p.is_null())
        .collect();
    owners.sort_unstable_by_key(|p| *p as usize);
    owners.dedup();
    if owners.len() != 1 {
        return Err("sample owner is not a unique owned class".into());
    }
    let owner = owners[0];
    let corlib = unsafe { corlib() };
    if !images.contains(&corlib) {
        return Err("sample corelib is not owned".into());
    }
    let void = unsafe { find(corlib, c"System".as_ptr(), c"Void".as_ptr()) };
    let intptr = unsafe { find(corlib, c"System".as_ptr(), c"IntPtr".as_ptr()) };
    let api = managed_calls::Api::load(library)?;
    let mut targets = [0; 2];
    for (index, &(name, member, target)) in BINDINGS.iter().enumerate() {
        targets[index] =
            registry.exact_target(name.to_str().map_err(|e| e.to_string())?, target)?;
        let method = unsafe { method(owner, member.as_ptr(), 1) };
        api.marker_icall(method, owner, void, intptr, images)?;
    }
    TARGETS
        .set(targets)
        .map_err(|_| "samples already installed")?;
    for ((name, _, _), adapter) in BINDINGS.into_iter().zip([
        begin_sample as *const () as Object,
        end_sample as *const () as Object,
    ]) {
        unsafe { add(name.as_ptr(), adapter) };
    }
    Ok(())
}
fn sample(marker: usize, begin: bool) {
    let result = std::panic::catch_unwind(|| -> Result<(), String> {
        let targets = TARGETS.get().ok_or("samples are not installed")?;
        let marker = marker as u64;
        if !animation::owns_marker(marker) {
            return Err("profiler sample requires an actual owned CreateMarker result".into());
        }
        SAMPLES.with(|s| s.borrow().validate(marker, begin))?;
        // Release the sample registry borrow before original engine execution.
        callbacks::guest_arguments(targets[usize::from(!begin)], &[marker])?;
        SAMPLES.with(|s| {
            let mut s = s.borrow_mut();
            s.record(marker, begin);
            if s.begins + s.ends <= 2 {
                eprintln!(
                    "NATIVE_MONO_PROFILER_SAMPLE_VERIFIED: begin={begin}; marker={marker:#x}"
                );
            }
        });
        Ok(())
    });
    if !matches!(result, Ok(Ok(()))) {
        eprintln!("NATIVE_MONO_PROFILER_SAMPLE_REJECT: {result:?}");
        std::process::abort();
    }
}
unsafe extern "C" fn begin_sample(marker: usize) {
    sample(marker, true);
}
unsafe extern "C" fn end_sample(marker: usize) {
    sample(marker, false);
}
pub(super) fn verify_balanced() -> Result<(), String> {
    SAMPLES.with(|s| {
        let s = s.borrow();
        if s.stack.is_empty() && s.begins == s.ends {
            Ok(())
        } else {
            Err("native profiler samples remain unbalanced at shutdown".into())
        }
    })
}
pub(super) fn report() -> String {
    SAMPLES.with(|s| {
        let s = s.borrow();
        format!("native-profiler-sample-begins={}\nnative-profiler-sample-ends={}\nnative-profiler-sample-depth={}\n", s.begins, s.ends, s.stack.len())
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sample_order_is_checked_before_recording_even_for_null_markers() {
        let mut samples = Samples::default();
        assert!(samples.validate(0, false).is_err());
        samples.validate(0, true).unwrap();
        samples.record(0, true);
        samples.record(17, true);
        assert!(samples.validate(0, false).is_err());
        assert_eq!(samples.ends, 0);
        assert_eq!(samples.stack, [0, 17]);
        samples.validate(17, false).unwrap();
        samples.record(17, false);
        samples.validate(0, false).unwrap();
        samples.record(0, false);
        assert!(samples.stack.is_empty());
        assert_eq!(samples.begins, samples.ends);
        for _ in 0..64 {
            samples.record(0, true);
        }
        assert!(samples.validate(0, true).is_err());
    }
}
