#[path = "support/x64.rs"]
mod support;
use support::{fixture, put16, put32, put64, startup, startup_call, startup_dll_call, BASE, STACK};
use wire::x64::{
    cpu::Cpu64,
    memory::{Memory, EXECUTE, READ, WRITE},
    pe::Pe64,
};
#[path = "x64/basic_contract.rs"]
mod basic_contract;
#[path = "x64/bstr_contract.rs"]
mod bstr_contract;
#[path = "x64/classes_contract.rs"]
mod classes_contract;
#[path = "x64/clocks.rs"]
mod clocks;
#[path = "x64/com.rs"]
mod com;
#[path = "x64/com_core.rs"]
mod com_core;
#[path = "x64/context.rs"]
mod context;
#[path = "x64/cpu.rs"]
mod cpu;
#[path = "x64/critical_sections.rs"]
mod critical_sections;
#[path = "x64/cursor_contract.rs"]
mod cursor_contract;
#[path = "x64/desktop_contract.rs"]
mod desktop_contract;
#[path = "x64/display_contract.rs"]
mod display_contract;
#[path = "x64/files.rs"]
mod files;
#[path = "x64/gdi_contract.rs"]
mod gdi_contract;
#[path = "x64/geometry_contract.rs"]
mod geometry_contract;
#[path = "x64/guid_allocation.rs"]
mod guid_allocation;
#[path = "x64/handles.rs"]
mod handles;
#[path = "x64/images_contract.rs"]
mod images_contract;
#[path = "x64/input_profile_contract.rs"]
mod input_profile_contract;
#[path = "x64/interlocked.rs"]
mod interlocked;
#[path = "x64/kernel.rs"]
mod kernel;
#[path = "x64/keyboard_contract.rs"]
mod keyboard_contract;
#[path = "x64/locals.rs"]
mod locals;
#[path = "x64/memory.rs"]
mod memory;
#[path = "x64/modules.rs"]
mod modules;
#[path = "x64/network.rs"]
mod network;
#[path = "x64/process.rs"]
mod process;
#[path = "x64/sections.rs"]
mod sections;
#[path = "x64/simd.rs"]
mod simd;
#[path = "x64/sync.rs"]
mod sync;
#[path = "x64/thread_control.rs"]
mod thread_control;
#[path = "x64/thread_properties.rs"]
mod thread_properties;
#[path = "x64/user.rs"]
mod user;
#[path = "x64/user_objects_contract.rs"]
mod user_objects_contract;
#[path = "x64/variant_contract.rs"]
mod variant_contract;
#[path = "x64/window_queries_contract.rs"]
mod window_queries_contract;
#[path = "x64/winrt_contract.rs"]
mod winrt_contract;

fn mono_trace_pe() -> Vec<u8> {
    let mut b = fixture();
    // mov r8,imm64; mov eax,1; add eax,2; add eax,3; ret.
    b[0x20a..0x216].copy_from_slice(&[0xb8, 1, 0, 0, 0, 0x83, 0xc0, 2, 0x83, 0xc0, 3, 0xc3]);
    // Two exports at one interior register-segment PC retain both aliases.
    put32(&mut b, 0x98 + 112, 0x1100);
    put32(&mut b, 0x98 + 116, 0x100);
    put32(&mut b, 0x300 + 16, 1);
    put32(&mut b, 0x300 + 20, 2);
    put32(&mut b, 0x300 + 24, 3);
    put32(&mut b, 0x300 + 28, 0x1140);
    put32(&mut b, 0x300 + 32, 0x1150);
    put32(&mut b, 0x300 + 36, 0x1160);
    put32(&mut b, 0x340, 0x1000);
    put32(&mut b, 0x344, 0x100f);
    for (i, (name, ordinal)) in [
        ("mono_unity_install_memory_callbacks", 0),
        ("mono_trace_probe", 1),
        ("mono_trace_alias", 1),
    ]
    .into_iter()
    .enumerate()
    {
        let at = 0x370 + i * 48;
        put32(&mut b, 0x350 + i * 4, 0x1000 + (at - 0x200) as u32);
        put16(&mut b, 0x360 + i * 2, ordinal);
        b[at..at + name.len()].copy_from_slice(name.as_bytes());
    }
    b
}

#[test]
fn diagnostic_observation_preserves_guard_faults_permissions_and_atomic_output() {
    let mut memory = Memory::default();
    memory.map(BASE, 8192, READ | WRITE).unwrap();
    memory.write(BASE + 4094, &[1, 2, 3, 4]).unwrap();
    memory
        .virtual_protect(BASE + 4096, 4096, 4 | wire::mem::PAGE_GUARD)
        .unwrap();
    let mut output = [0xcc; 4];
    for _ in 0..2 {
        assert!(memory
            .observe(BASE + 4094, &mut output)
            .unwrap_err()
            .contains("guarded"));
        assert_eq!(output, [0xcc; 4]);
    }
    assert!(memory
        .read(BASE + 4094, &mut output)
        .unwrap_err()
        .contains("guard-page"));
    assert_eq!(output, [0xcc; 4]);
    memory.observe(BASE + 4094, &mut output).unwrap();
    assert_eq!(output, [1, 2, 3, 4]);
    memory.protect(BASE + 4096, 4096, EXECUTE).unwrap();
    output = [0xcc; 4];
    assert!(memory.observe(BASE + 4094, &mut output).is_err());
    assert_eq!(output, [0xcc; 4]);
    assert!(memory.observe(u64::MAX - 1, &mut output).is_err());
    assert_eq!(output, [0xcc; 4]);
}

#[test]
fn original_mono_trace_catches_segment_interior_aliases_without_changing_execution() {
    use wire::x64::process::{Outcome, Process64};
    let dir = std::env::temp_dir().join(format!("wire-mono-entry-trace-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("mono.dll");
    std::fs::write(&exe, mono_trace_pe()).unwrap();
    let mut plain = Process64::load(&exe, false).unwrap();
    let mut traced = Process64::load(&exe, false).unwrap();
    traced.enable_mono_trace().unwrap();
    traced.enable_mono_trace().unwrap(); // Idempotent, no duplicate aliases.
    for process in [&mut plain, &mut traced] {
        process
            .memory
            .map(BASE + 0x2000, 4096, READ | WRITE)
            .unwrap();
        process
            .memory
            .virtual_protect(BASE + 0x2000, 4096, 4 | wire::mem::PAGE_GUARD)
            .unwrap();
        process.cpu.regs[1] = BASE + 0x2000; // Guarded callback table.
        process.cpu.regs[4] = STACK + 4096 - 16;
        process
            .memory
            .write_int(process.cpu.regs[4], 8, 0x7ffe10000000)
            .unwrap();
        process
            .memory
            .virtual_protect(STACK + 4096, 4096, 4 | wire::mem::PAGE_GUARD)
            .unwrap();
    }
    for budget in [1, 2, 3, 4, 5, 100] {
        let a = plain.run(budget).unwrap();
        let b = traced.run(budget).unwrap();
        assert_eq!(a, b);
        assert_eq!(plain.cpu, traced.cpu);
    }
    assert_eq!(traced.run(100).unwrap(), Outcome::Returned(6));
    assert!(plain.mono_trace_tsv().is_none());
    let trace = traced.mono_trace_tsv().unwrap();
    assert!(trace.contains("# total=2 samples=2 omitted=0"));
    assert!(trace.contains("mono.dll!mono_trace_alias|mono.dll!mono_trace_probe"));
    assert!(trace.contains("unavailable:guarded AMD64 observation"));
    assert!(trace.contains("sample\t0x14000100f"));
    for process in [&mut plain, &mut traced] {
        assert!(process
            .memory
            .read_int(BASE + 0x2000, 8)
            .unwrap_err()
            .contains("guard-page"));
        assert!(process
            .memory
            .read_int(STACK + 4096, 8)
            .unwrap_err()
            .contains("guard-page"));
    }
    // Restart with a large budget: a fused register segment cannot skip exports.
    let mut batched = Process64::load(&exe, false).unwrap();
    batched.enable_mono_trace().unwrap();
    assert_eq!(batched.run(100).unwrap(), Outcome::Returned(6));
    assert!(batched
        .mono_trace_tsv()
        .unwrap()
        .contains("# total=2 samples=2 omitted=0"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn mono_trace_registers_exports_loaded_after_observation_is_enabled() {
    use wire::x64::process::{Outcome, Process64};
    const CHILD: u64 = 0x180000000;
    let dir = std::env::temp_dir().join(format!("wire-mono-dynamic-trace-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("probe.exe");
    std::fs::write(&exe, fixture()).unwrap();
    let mut child = mono_trace_pe();
    put16(&mut child, 0x96, 0x2002); // DLL, no DllMain.
    put32(&mut child, 0x98 + 16, 0);
    put64(&mut child, 0x98 + 24, CHILD);
    std::fs::write(dir.join("mono-2.0-bdwgc.dll"), child).unwrap();
    let mut process = Process64::load(&exe, false).unwrap();
    process.enable_mono_trace().unwrap();
    process
        .memory
        .map(BASE + 0x2000, 4096, READ | WRITE)
        .unwrap();
    process
        .memory
        .write(BASE + 0x2000, b"mono-2.0-bdwgc.dll\0")
        .unwrap();
    process.cpu.regs[1] = BASE + 0x2000;
    process.cpu.rip = process.runtime.register("kernel32.dll", "LoadLibraryA");
    assert_eq!(process.run(1000).unwrap(), Outcome::Returned(CHILD));
    assert!(process
        .mono_trace_tsv()
        .unwrap()
        .contains("# total=0 samples=0 omitted=0"));
    process.cpu.regs[4] -= 8; // Reuse the consumed synthetic caller frame.
    process.cpu.rip = CHILD + 0x1000;
    assert_eq!(process.run(1000).unwrap(), Outcome::Returned(6));
    let trace = process.mono_trace_tsv().unwrap();
    assert!(trace.contains("# total=2 samples=2 omitted=0"));
    assert!(trace.contains("sample\t0x18000100f\tmono-2.0-bdwgc.dll!mono_trace_alias|mono-2.0-bdwgc.dll!mono_trace_probe"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn mono_trace_preserves_instruction_fetch_fault_when_table_points_at_guarded_code() {
    use wire::x64::process::Process64;
    let dir = std::env::temp_dir().join(format!("wire-mono-fault-trace-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("mono.dll");
    std::fs::write(&exe, mono_trace_pe()).unwrap();
    let mut plain = Process64::load(&exe, false).unwrap();
    let mut traced = Process64::load(&exe, false).unwrap();
    traced.enable_mono_trace().unwrap();
    for process in [&mut plain, &mut traced] {
        process.cpu.regs[1] = BASE + 0x1000;
        process
            .memory
            .virtual_protect(BASE + 0x1000, 4096, 0x40 | wire::mem::PAGE_GUARD)
            .unwrap();
    }
    assert_eq!(plain.run(100).unwrap_err(), traced.run(100).unwrap_err());
    assert_eq!(plain.cpu, traced.cpu);
    assert_eq!(plain.run(100).unwrap(), traced.run(100).unwrap());
    assert_eq!(plain.cpu, traced.cpu);
    let trace = traced.mono_trace_tsv().unwrap();
    assert!(trace.contains("# total=3 samples=3 omitted=0"));
    assert!(trace.contains("unavailable:guarded AMD64 observation"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn observed_allocator_callback_pc_cannot_be_skipped_after_runtime_table_installation() {
    use wire::x64::process::{Outcome, Process64};
    let dir = std::env::temp_dir().join(format!(
        "wire-mono-callback-entry-trace-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("mono.dll");
    std::fs::write(&exe, mono_trace_pe()).unwrap();
    let mut process = Process64::load(&exe, false).unwrap();
    process
        .memory
        .map(BASE + 0x2000, 4096, READ | WRITE)
        .unwrap();
    process.memory.write_int(BASE + 0x2000, 4, 1).unwrap();
    for index in 0..4 {
        // This original fixture PC is inside a segment, absent from PE exports.
        process
            .memory
            .write_int(BASE + 0x2008 + index * 8, 8, BASE + 0x100a)
            .unwrap();
    }
    process.cpu.regs[1] = BASE + 0x2000;
    process.enable_mono_trace().unwrap();
    assert_eq!(process.run(100).unwrap(), Outcome::Returned(6));
    let trace = process.mono_trace_tsv().unwrap();
    assert!(trace.contains("# total=3 samples=3 omitted=0"));
    let sample = trace
        .lines()
        .find(|line| line.starts_with("sample\t0x14000100a"))
        .unwrap();
    for name in ["malloc", "realloc", "free", "calloc"] {
        assert!(sample.contains(&format!("observed-callback!mono_allocator.{name}")));
    }
    std::fs::remove_dir_all(dir).unwrap();
}
