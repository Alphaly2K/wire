//! Compiled scalar InternalCall adapters for the exact Unity registration
//! profile. Metadata verifies every ABI; original engine code produces values.
use super::{bindings, callbacks, managed_calls, Object};
use std::{cell::RefCell, ffi::CStr, sync::OnceLock};
struct Spec {
    binding: &'static CStr,
    namespace: &'static CStr,
    owner: &'static CStr,
    method: &'static CStr,
    target: u64,
    returns: (&'static CStr, &'static CStr),
    argument: Option<(&'static CStr, &'static CStr)>,
    adapter: Object,
}
const COUNT: usize = 29;
static TARGETS: OnceLock<[u64; COUNT]> = OnceLock::new();
thread_local! { static CALLS: RefCell<[usize; COUNT]> = const { RefCell::new([0; COUNT]) }; }
pub(super) fn installed() -> bool {
    TARGETS.get().is_some()
}
pub(super) fn install(
    library: Object,
    core: Object,
    images: &[Object],
    registry: &bindings::Registry,
) -> Result<(), String> {
    macro_rules! sym {
        ($name:literal, $ty:ty) => {{
            let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
            if p.is_null() {
                return Err(concat!("missing scalar binding API: ", $name).into());
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
    let corlib = unsafe { corlib() };
    if core.is_null() || corlib.is_null() || !images.contains(&core) || !images.contains(&corlib) {
        return Err("scalar binding family has no owned core/corelib images".into());
    }
    let class = |namespace: &CStr, name: &CStr| unsafe {
        find(
            if namespace == c"System" { corlib } else { core },
            namespace.as_ptr(),
            name.as_ptr(),
        )
    };
    let specs = specs();
    let api = managed_calls::Api::load(library)?;
    let mut targets = [0; COUNT];
    for (index, spec) in specs.iter().enumerate() {
        targets[index] = registry.exact_target(
            spec.binding.to_str().map_err(|e| e.to_string())?,
            spec.target,
        )?;
        let mut owners: Vec<_> = images
            .iter()
            .map(|&image| unsafe { find(image, spec.namespace.as_ptr(), spec.owner.as_ptr()) })
            .filter(|p| !p.is_null())
            .collect();
        // Type-forwarding images can resolve to the same actual MonoClass.
        // Distinct class identities remain an ambiguous owner and are rejected.
        owners.sort_unstable_by_key(|p| *p as usize);
        owners.dedup();
        if owners.len() != 1 {
            return Err(format!(
                "scalar binding owner is not unique: {:?}",
                spec.binding
            ));
        }
        let owner = owners[0];
        let arguments: Vec<_> = spec
            .argument
            .iter()
            .map(|&(namespace, name)| class(namespace, name))
            .collect();
        let method = unsafe { method(owner, spec.method.as_ptr(), arguments.len() as i32) };
        api.scalar_icall(
            method,
            owner,
            class(spec.returns.0, spec.returns.1),
            &arguments,
            images,
        )?;
    }
    // Complete the entire signature/target validation before registering any.
    TARGETS
        .set(targets)
        .map_err(|_| "scalar bindings already installed")?;
    for spec in specs {
        unsafe {
            add(spec.binding.as_ptr(), spec.adapter);
        }
    }
    Ok(())
}
fn call<const SLOT: usize>(arguments: &[u64], floating: &[(usize, u64)]) -> (u64, u64) {
    let result = std::panic::catch_unwind(|| -> Result<(u64, u64), String> {
        let target = TARGETS.get().ok_or("scalar bindings not installed")?[SLOT];
        let spec = specs()
            .into_iter()
            .nth(SLOT)
            .ok_or("scalar binding slot absent")?;
        if spec.argument == Some((c"System", c"Boolean"))
            && arguments.first().is_none_or(|&b| b > 1)
        {
            return Err("scalar binding Boolean is not canonical".into());
        }
        let result = callbacks::guest_scalars(target, arguments, floating)?;
        if spec.returns == (c"System", c"Boolean") && result.0 & 255 > 1 {
            return Err("engine returned a noncanonical Boolean".into());
        }
        CALLS.with(|calls| {
            let mut calls = calls.borrow_mut();
            if calls[SLOT] == 0 {
                eprintln!(
                    "NATIVE_MONO_ENGINE_SCALAR_VERIFIED: {:?}; integer={:#x}; xmm0={:#x}",
                    spec.binding, result.0, result.1
                );
            }
            calls[SLOT] += 1;
        });
        Ok(result)
    });
    match result {
        Ok(Ok(value)) => value,
        other => {
            eprintln!("NATIVE_MONO_ENGINE_SCALAR_REJECT: slot={SLOT}; {other:?}");
            std::process::abort();
        }
    }
}
pub(super) fn report() -> String {
    format!(
        "native-engine-scalar-calls={}\n",
        CALLS.with(|calls| calls.borrow().iter().sum::<usize>())
    )
}
unsafe extern "C" fn adapter_0() -> u8 {
    call::<0>(&[], &[]).0 as u8
}
unsafe extern "C" fn adapter_1() -> u8 {
    call::<1>(&[], &[]).0 as u8
}
unsafe extern "C" fn adapter_2() -> u8 {
    call::<2>(&[], &[]).0 as u8
}
unsafe extern "C" fn adapter_3() -> u8 {
    call::<3>(&[], &[]).0 as u8
}
unsafe extern "C" fn adapter_4() -> i32 {
    call::<4>(&[], &[]).0 as i32
}
unsafe extern "C" fn adapter_5(value: u8) {
    call::<5>(&[u64::from(value)], &[]);
}
unsafe extern "C" fn adapter_6() -> f64 {
    f64::from_bits(call::<6>(&[], &[]).1)
}
unsafe extern "C" fn adapter_7() -> f64 {
    f64::from_bits(call::<7>(&[], &[]).1)
}
unsafe extern "C" fn adapter_8() -> i32 {
    call::<8>(&[], &[]).0 as i32
}
unsafe extern "C" fn adapter_9(value: f32) {
    call::<9>(&[0], &[(0, u64::from(value.to_bits()))]);
}
unsafe extern "C" fn adapter_10() -> f32 {
    f32::from_bits(call::<10>(&[], &[]).1 as u32)
}
unsafe extern "C" fn adapter_11() -> u64 {
    call::<11>(&[], &[]).0
}
unsafe extern "C" fn adapter_12() -> u8 {
    call::<12>(&[], &[]).0 as u8
}
unsafe extern "C" fn adapter_13(value: u8) {
    call::<13>(&[u64::from(value)], &[]);
}
unsafe extern "C" fn adapter_14() -> u8 {
    call::<14>(&[], &[]).0 as u8
}
unsafe extern "C" fn adapter_15(value: u8) {
    call::<15>(&[u64::from(value)], &[]);
}
unsafe extern "C" fn adapter_16() -> f32 {
    f32::from_bits(call::<16>(&[], &[]).1 as u32)
}
unsafe extern "C" fn adapter_17() -> i32 {
    call::<17>(&[], &[]).0 as i32
}
unsafe extern "C" fn adapter_18() -> i32 {
    call::<18>(&[], &[]).0 as i32
}
unsafe extern "C" fn adapter_19() -> i32 {
    call::<19>(&[], &[]).0 as i32
}
unsafe extern "C" fn adapter_20() -> i32 {
    call::<20>(&[], &[]).0 as i32
}
unsafe extern "C" fn adapter_21() -> i32 {
    call::<21>(&[], &[]).0 as i32
}
unsafe extern "C" fn adapter_22() -> f32 {
    f32::from_bits(call::<22>(&[], &[]).1 as u32)
}
unsafe extern "C" fn adapter_23() -> u8 {
    call::<23>(&[], &[]).0 as u8
}
unsafe extern "C" fn adapter_24(value: u8) {
    call::<24>(&[u64::from(value)], &[]);
}
unsafe extern "C" fn adapter_25() -> u8 {
    call::<25>(&[], &[]).0 as u8
}
unsafe extern "C" fn adapter_26(value: u8) {
    call::<26>(&[u64::from(value)], &[]);
}
unsafe extern "C" fn adapter_27() -> i32 {
    call::<27>(&[], &[]).0 as i32
}
unsafe extern "C" fn adapter_28() -> i32 {
    call::<28>(&[], &[]).0 as i32
}
fn specs() -> [Spec; COUNT] {
    [
    Spec { binding: c"UnityEngine.Application::get_isFocused", namespace: c"UnityEngine", owner: c"Application", method: c"get_isFocused", target: 0x18006d380, returns: (c"System", c"Boolean"), argument: None, adapter: adapter_0 as *const () as Object },
    Spec { binding: c"UnityEngine.Application::get_isPlaying", namespace: c"UnityEngine", owner: c"Application", method: c"get_isPlaying", target: 0x18005f520, returns: (c"System", c"Boolean"), argument: None, adapter: adapter_1 as *const () as Object },
    Spec { binding: c"UnityEngine.Application::get_runInBackground", namespace: c"UnityEngine", owner: c"Application", method: c"get_runInBackground", target: 0x18006dd10, returns: (c"System", c"Boolean"), argument: None, adapter: adapter_2 as *const () as Object },
    Spec { binding: c"UnityEngine.Application::get_isBatchMode", namespace: c"UnityEngine", owner: c"Application", method: c"get_isBatchMode", target: 0x18006dd50, returns: (c"System", c"Boolean"), argument: None, adapter: adapter_3 as *const () as Object },
    Spec { binding: c"UnityEngine.Application::get_platform", namespace: c"UnityEngine", owner: c"Application", method: c"get_platform", target: 0x18006fda0, returns: (c"UnityEngine", c"RuntimePlatform"), argument: None, adapter: adapter_4 as *const () as Object },
    Spec { binding: c"UnityEngineInternal.Input.NativeInputSystem::set_hasDeviceDiscoveredCallback", namespace: c"UnityEngineInternal.Input", owner: c"NativeInputSystem", method: c"set_hasDeviceDiscoveredCallback", target: 0x180132890, returns: (c"System", c"Void"), argument: Some((c"System", c"Boolean")), adapter: adapter_5 as *const () as Object },
    Spec { binding: c"UnityEngineInternal.Input.NativeInputSystem::get_currentTime", namespace: c"UnityEngineInternal.Input", owner: c"NativeInputSystem", method: c"get_currentTime", target: 0x1801328b0, returns: (c"System", c"Double"), argument: None, adapter: adapter_6 as *const () as Object },
    Spec { binding: c"UnityEngineInternal.Input.NativeInputSystem::get_currentTimeOffsetToRealtimeSinceStartup", namespace: c"UnityEngineInternal.Input", owner: c"NativeInputSystem", method: c"get_currentTimeOffsetToRealtimeSinceStartup", target: 0x1801328f0, returns: (c"System", c"Double"), argument: None, adapter: adapter_7 as *const () as Object },
    Spec { binding: c"UnityEngineInternal.Input.NativeInputSystem::AllocateDeviceId", namespace: c"UnityEngineInternal.Input", owner: c"NativeInputSystem", method: c"AllocateDeviceId", target: 0x180132900, returns: (c"System", c"Int32"), argument: None, adapter: adapter_8 as *const () as Object },
    Spec { binding: c"UnityEngineInternal.Input.NativeInputSystem::SetPollingFrequency", namespace: c"UnityEngineInternal.Input", owner: c"NativeInputSystem", method: c"SetPollingFrequency", target: 0x1801329f0, returns: (c"System", c"Void"), argument: Some((c"System", c"Single")), adapter: adapter_9 as *const () as Object },
    Spec { binding: c"UnityEngineInternal.Input.NativeInputSystem::GetPollingFrequency", namespace: c"UnityEngineInternal.Input", owner: c"NativeInputSystem", method: c"GetPollingFrequency", target: 0x180132a00, returns: (c"System", c"Single"), argument: None, adapter: adapter_10 as *const () as Object },
    Spec { binding: c"UnityEngineInternal.Input.NativeInputSystem::GetBackgroundEventBufferSize", namespace: c"UnityEngineInternal.Input", owner: c"NativeInputSystem", method: c"GetBackgroundEventBufferSize", target: 0x180132a20, returns: (c"System", c"UInt64"), argument: None, adapter: adapter_11 as *const () as Object },
    Spec { binding: c"UnityEngineInternal.Input.NativeInputSystem::get_allowInputDeviceCreationFromEvents", namespace: c"UnityEngineInternal.Input", owner: c"NativeInputSystem", method: c"get_allowInputDeviceCreationFromEvents", target: 0x180132a30, returns: (c"System", c"Boolean"), argument: None, adapter: adapter_12 as *const () as Object },
    Spec { binding: c"UnityEngineInternal.Input.NativeInputSystem::set_allowInputDeviceCreationFromEvents", namespace: c"UnityEngineInternal.Input", owner: c"NativeInputSystem", method: c"set_allowInputDeviceCreationFromEvents", target: 0x180132a40, returns: (c"System", c"Void"), argument: Some((c"System", c"Boolean")), adapter: adapter_13 as *const () as Object },
    Spec { binding: c"UnityEngineInternal.Input.NativeInputSystem::get_normalizeScrollWheelDelta", namespace: c"UnityEngineInternal.Input", owner: c"NativeInputSystem", method: c"get_normalizeScrollWheelDelta", target: 0x180132a50, returns: (c"System", c"Boolean"), argument: None, adapter: adapter_14 as *const () as Object },
    Spec { binding: c"UnityEngineInternal.Input.NativeInputSystem::set_normalizeScrollWheelDelta", namespace: c"UnityEngineInternal.Input", owner: c"NativeInputSystem", method: c"set_normalizeScrollWheelDelta", target: 0x180132a60, returns: (c"System", c"Void"), argument: Some((c"System", c"Boolean")), adapter: adapter_15 as *const () as Object },
    Spec { binding: c"UnityEngineInternal.Input.NativeInputSystem::GetScrollWheelDeltaPerTick", namespace: c"UnityEngineInternal.Input", owner: c"NativeInputSystem", method: c"GetScrollWheelDeltaPerTick", target: 0x180132a90, returns: (c"System", c"Single"), argument: None, adapter: adapter_16 as *const () as Object },
    Spec { binding: c"UnityEngine.SystemInfo::GetDeviceType", namespace: c"UnityEngine", owner: c"SystemInfo", method: c"GetDeviceType", target: 0x1800c9630, returns: (c"UnityEngine", c"DeviceType"), argument: None, adapter: adapter_17 as *const () as Object },
    Spec { binding: c"UnityEngine.SystemInfo::GetOperatingSystemFamily", namespace: c"UnityEngine", owner: c"SystemInfo", method: c"GetOperatingSystemFamily", target: 0x18006fda0, returns: (c"UnityEngine", c"OperatingSystemFamily"), argument: None, adapter: adapter_18 as *const () as Object },
    Spec { binding: c"UnityEngine.SystemInfo::GetProcessorCount", namespace: c"UnityEngine", owner: c"SystemInfo", method: c"GetProcessorCount", target: 0x1800c9480, returns: (c"System", c"Int32"), argument: None, adapter: adapter_19 as *const () as Object },
    Spec { binding: c"UnityEngine.Screen::get_width", namespace: c"UnityEngine", owner: c"Screen", method: c"get_width", target: 0x18007cbd0, returns: (c"System", c"Int32"), argument: None, adapter: adapter_20 as *const () as Object },
    Spec { binding: c"UnityEngine.Screen::get_height", namespace: c"UnityEngine", owner: c"Screen", method: c"get_height", target: 0x18007cbe0, returns: (c"System", c"Int32"), argument: None, adapter: adapter_21 as *const () as Object },
    Spec { binding: c"UnityEngine.Screen::get_dpi", namespace: c"UnityEngine", owner: c"Screen", method: c"get_dpi", target: 0x18007cbf0, returns: (c"System", c"Single"), argument: None, adapter: adapter_22 as *const () as Object },
    Spec { binding: c"Unity.Jobs.LowLevel.Unsafe.JobsUtility::get_JobCompilerEnabled", namespace: c"Unity.Jobs.LowLevel.Unsafe", owner: c"JobsUtility", method: c"get_JobCompilerEnabled", target: 0x18005fef0, returns: (c"System", c"Boolean"), argument: None, adapter: adapter_23 as *const () as Object },
    Spec { binding: c"Unity.Jobs.LowLevel.Unsafe.JobsUtility::set_JobCompilerEnabled", namespace: c"Unity.Jobs.LowLevel.Unsafe", owner: c"JobsUtility", method: c"set_JobCompilerEnabled", target: 0x18005ff00, returns: (c"System", c"Void"), argument: Some((c"System", c"Boolean")), adapter: adapter_24 as *const () as Object },
    Spec { binding: c"Unity.Jobs.LowLevel.Unsafe.JobsUtility::get_JobDebuggerEnabled", namespace: c"Unity.Jobs.LowLevel.Unsafe", owner: c"JobsUtility", method: c"get_JobDebuggerEnabled", target: 0x18005fed0, returns: (c"System", c"Boolean"), argument: None, adapter: adapter_25 as *const () as Object },
    Spec { binding: c"Unity.Jobs.LowLevel.Unsafe.JobsUtility::set_JobDebuggerEnabled", namespace: c"Unity.Jobs.LowLevel.Unsafe", owner: c"JobsUtility", method: c"set_JobDebuggerEnabled", target: 0x18005fee0, returns: (c"System", c"Void"), argument: Some((c"System", c"Boolean")), adapter: adapter_26 as *const () as Object },
    Spec { binding: c"UnityEngine.QualitySettings::get_activeColorSpace", namespace: c"UnityEngine", owner: c"QualitySettings", method: c"get_activeColorSpace", target: 0x180088390, returns: (c"UnityEngine", c"ColorSpace"), argument: None, adapter: adapter_27 as *const () as Object },
    Spec { binding: c"UnityEngine.QualitySettings::get_desiredColorSpace", namespace: c"UnityEngine", owner: c"QualitySettings", method: c"get_desiredColorSpace", target: 0x180088390, returns: (c"UnityEngine", c"ColorSpace"), argument: None, adapter: adapter_28 as *const () as Object },
]
}
