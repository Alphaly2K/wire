//! Exercise a compiled PE32 DLL through the actual Win32 API/callback path.
//! sh tests/dlls/build.sh
//! cargo run --release --example dll_probe -- wire-out/dll-probe/probe.dll
use wire::api::{ApiLogger, ApiState, ApiTable, HandleObj};
use wire::cpu::{Cpu, GuestCallOutcome};
use wire::fs::GuestFs;
use wire::loader::{STACK_BASE, STACK_SIZE, TEB_ADDR};
use wire::mem::GuestMem;
use wire::thread::Sched;

fn returned(result: GuestCallOutcome) -> u32 {
    match result {
        GuestCallOutcome::Returned(value) => value,
        _ => panic!("unexpected guest control transfer"),
    }
}
fn main() {
    let path = std::path::PathBuf::from(std::env::args().nth(1).expect("DLL path required"))
        .canonicalize()
        .unwrap();
    let mut mem = GuestMem::new().unwrap();
    for (base, size) in [
        (0x20000, 0x10000),
        (STACK_BASE, STACK_SIZE),
        (TEB_ADDR, 0x1000),
    ] {
        mem.commit(base, size).unwrap();
    }
    mem.write_u32(TEB_ADDR, u32::MAX).unwrap();
    mem.write_u32(TEB_ADDR + 4, STACK_BASE + STACK_SIZE)
        .unwrap();
    mem.write_u32(TEB_ADDR + 8, STACK_BASE).unwrap();
    let mut api = ApiTable::new();
    let load = api.register("kernel32.dll", Some("LoadLibraryA"), None);
    let get = api.register("kernel32.dll", Some("GetProcAddress"), None);
    let free = api.register("kernel32.dll", Some("FreeLibrary"), None);
    let mut state = ApiState::new(0x400000, "C:\\probe.exe".into(), 0, 0);
    state.fs = Some(GuestFs::new(path.parent().unwrap()).unwrap());
    let log_path = std::env::temp_dir().join(format!("wire-dll-probe-{}.log", std::process::id()));
    let mut log = ApiLogger::new(&log_path).unwrap();
    let mut sched = Sched::new();
    let tid = sched.alloc_tid();
    let handle = state.alloc_handle(HandleObj::Thread { tid });
    sched.add_main(tid, handle, TEB_ADDR);
    let mut cpu = Cpu::new(0, STACK_BASE + STACK_SIZE - 256, TEB_ADDR);
    cpu.profile_opcodes = false;
    mem.write_bytes(
        0x20000,
        format!("{}\0", path.file_name().unwrap().to_str().unwrap()).as_bytes(),
    )
    .unwrap();
    mem.write_bytes(0x20100, b"ProbeValue\0").unwrap();
    let module = returned(
        cpu.call_guest(
            &mut mem,
            &mut api,
            &mut state,
            &mut log,
            &mut sched,
            load,
            &[0x20000],
            1_000_000,
        )
        .unwrap(),
    );
    assert_eq!(module, 0x50000000, "DLL should relocate away from heap");
    let function = returned(
        cpu.call_guest(
            &mut mem,
            &mut api,
            &mut state,
            &mut log,
            &mut sched,
            get,
            &[module, 0x20100],
            1_000_000,
        )
        .unwrap(),
    );
    assert!(function >= module && function < module + 0x10000);
    let value = returned(
        cpu.call_guest(
            &mut mem,
            &mut api,
            &mut state,
            &mut log,
            &mut sched,
            function,
            &[],
            1_000_000,
        )
        .unwrap(),
    );
    assert_eq!(value, 4279); // DllMain's imported PID (4242) + exported function's 37
    assert_eq!(
        returned(
            cpu.call_guest(
                &mut mem,
                &mut api,
                &mut state,
                &mut log,
                &mut sched,
                free,
                &[module],
                1_000_000
            )
            .unwrap()
        ),
        1
    );
    assert!(!mem.is_committed(module));
    println!("compiled DLL: relocated to {module:#x}, ProbeValue={value}, detached and unmapped; {} guest instructions", cpu.instr_count);
    println!("API log: {}", log_path.display());
}
