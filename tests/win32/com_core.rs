//! Preserve PE32 HRESULT/LastError, process heap sharing and ABI-only policy.
use super::*;

#[test]
fn com_core_keeps_public_nls_task_facade_scope_and_allocation_notes() {
    let mut v = Vm::new();
    let call = |name: &str, value: u32, v: &mut Vm| {
        let mut args = [0; 16];
        args[0] = value;
        wire::nls::dispatch("ole32.dll", name, &args, &mut v.mem, &mut v.state).unwrap()
    };
    let (pointer, note) = call("CoTaskMemAlloc", 17, &mut v);
    assert_ne!(pointer, 0);
    assert_eq!(note, "code-page / sandbox directory shim");
    assert_eq!(v.k("HeapSize", &[v.state.process_heap, 0, pointer]), 17);
    assert_eq!(call("CoTaskMemFree", pointer, &mut v), (0, note));
    assert_eq!(
        v.k("HeapSize", &[v.state.process_heap, 0, pointer]),
        u32::MAX
    );
    for dll in ["OLE32.dll", "kernel32.dll", "combase.dll", "user32.dll"] {
        assert!(
            wire::nls::dispatch(dll, "CoTaskMemAlloc", &[0; 16], &mut v.mem, &mut v.state)
                .is_none()
        );
    }
    for name in [
        "CoInitialize",
        "CoInitializeEx",
        "CoUninitialize",
        "CoCreateInstance",
    ] {
        assert!(
            wire::nls::dispatch("ole32.dll", name, &[0; 16], &mut v.mem, &mut v.state).is_none()
        );
    }
}

#[test]
fn com_core_keeps_pe32_apartment_error_refs_and_thread_identity() {
    let mut v = Vm::new();
    v.k("SetLastError", &[123]);
    for (name, args, expected) in [
        ("CoInitialize", vec![1], 0x80070057),
        ("CoInitializeEx", vec![0, 1], 0x80070057),
        ("CoInitialize", vec![0], 0),
        ("CoInitializeEx", vec![0, 0xe], 1),
        ("CoInitializeEx", vec![0, 0], 0x80010106),
        ("CoUninitialize", vec![], 0),
        ("CoInitializeEx", vec![0, 0], 0x80010106),
        ("CoUninitialize", vec![], 0),
        ("CoUninitialize", vec![], 0),
        ("CoInitializeEx", vec![0, 0xc], 0),
    ] {
        assert_eq!(v.call("ole32.dll", name, &args), expected);
        if expected & 0x80000000 != 0 {
            assert_eq!(v.state.last_error, expected);
        }
    }
    v.k("CreateThread", &[0, 0, CODE, 0, 4, 0]);
    v.sched.current = 1;
    assert_eq!(v.call("ole32.dll", "CoInitialize", &[0]), 0);
    assert_eq!(v.call("ole32.dll", "CoInitialize", &[0]), 1);
    v.sched.current = 0;
    assert_eq!(v.call("ole32.dll", "CoInitializeEx", &[0, 0]), 1);
    assert_eq!(v.call("ole32.dll", "CoInitialize", &[0]), 0x80010106);
}

#[test]
fn com_core_keeps_pe32_task_heap_sharing_and_optional_provider_cleanup() {
    let mut v = Vm::new();
    v.k("SetLastError", &[123]);
    let p = v.call("ole32.dll", "CoTaskMemAlloc", &[17]);
    assert_ne!(p, 0);
    assert_eq!(v.k("HeapSize", &[v.state.process_heap, 0, p]), 17);
    assert_eq!(v.k("HeapFree", &[v.state.process_heap, 0, p]), 1);
    let p = v.k("HeapAlloc", &[v.state.process_heap, 0, 31]);
    assert_eq!(v.call("ole32.dll", "CoTaskMemFree", &[p]), 0);
    assert_eq!(v.k("HeapSize", &[v.state.process_heap, 0, p]), u32::MAX);
    for p in [0, 0xdead0000] {
        assert_eq!(v.call("ole32.dll", "CoTaskMemFree", &[p]), 0);
    }
    assert_eq!(v.call("ole32.dll", "CoTaskMemAlloc", &[u32::MAX]), 0);
    assert_eq!(v.state.last_error, 123);
    for (name, bytes) in [("OleInitialize", 4), ("OleUninitialize", 0)] {
        assert_eq!(v.call("ole32.dll", name, &[0]), 0x80004001);
        assert_eq!(v.state.last_error, 120);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 252 + bytes);
    }
    for dll in [
        "kernel32.dll",
        "kernelbase.dll",
        "combase.dll",
        "api-ms-win-core-winrt-l1-1-0.dll",
        "user32.dll",
    ] {
        for name in [
            "CoInitialize",
            "CoInitializeEx",
            "CoUninitialize",
            "CoCreateInstance",
            "CoTaskMemAlloc",
            "CoTaskMemFree",
            "RoInitialize",
            "RoUninitialize",
        ] {
            assert_eq!(v.call(dll, name, &[0, 2, 0, 0, DATA]), 0, "{dll}!{name}");
            assert_eq!(v.state.last_error, 120);
            assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 252, "{dll}!{name}");
        }
    }
}
