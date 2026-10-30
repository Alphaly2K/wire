use super::*;
use crate::{machine::Machine, win32::ServiceSample};

fn put(bytes: &mut [u8], at: usize, value: u64, width: usize) {
    bytes[at..at + width].copy_from_slice(&value.to_le_bytes()[..width]);
}

fn image(mode: Mode) -> Vec<u8> {
    let mut bytes = vec![0; 0x600];
    let width = mode as usize / 8;
    let base = if width == 4 { 0x400000 } else { 0x140000000 };
    let opt = 0x98;
    let dirs = opt + if width == 4 { 96 } else { 112 };
    let section = opt + if width == 4 { 0xe0 } else { 0xf0 };
    bytes[..2].copy_from_slice(b"MZ");
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    for (at, value, size) in [
        (0x3c, 0x80, 4),
        (0x84, if width == 4 { 0x14c } else { 0x8664 }, 2),
        (0x86, 1, 2),
        (0x94, (section - opt) as u64, 2),
        (0x96, 0x103, 2),
        (opt, if width == 4 { 0x10b } else { 0x20b }, 2),
        (opt + 16, 0x1000, 4),
        (opt + if width == 4 { 28 } else { 24 }, base, width),
        (opt + 32, 0x1000, 4),
        (opt + 36, 0x200, 4),
        (opt + 56, 0x2000, 4),
        (opt + 60, 0x200, 4),
        (opt + 68, 3, 2),
        (opt + 70, 0x140, 2),
        (opt + 72, 0x100000, width),
        (opt + 72 + width, 0x1000, width),
        (dirs - 4, 16, 4),
        (section + 8, 0x1000, 4),
        (section + 12, 0x1000, 4),
        (section + 16, 0x400, 4),
        (section + 20, 0x200, 4),
        (section + 36, 0xe0000020, 4),
        (dirs + 9 * 8, 0x1100, 4),
        (dirs + 9 * 8 + 4, (width * 4 + 8) as u64, 4),
        (0x300, base + 0x1200, width),
        (0x300 + width, base + 0x1204, width),
        (0x300 + 2 * width, base + 0x1300, width),
        (0x300 + 3 * width, base + 0x1180, width),
        (0x300 + 4 * width, 4, 4),
        (0x380, base + 0x1010, width),
        (0x400, 0x11223344, 4),
    ] {
        put(&mut bytes, at, value, size);
    }
    bytes[section..section + 5].copy_from_slice(b".text");
    bytes[0x200] = 0xc3;
    bytes[0x210..0x213].copy_from_slice(if width == 4 {
        &[0xc2, 12, 0]
    } else {
        &[0xc3, 0, 0]
    });
    bytes
}

fn service(machine: &mut Machine, name: &str, args: &[u64]) -> u64 {
    let mode = machine.memory.mode();
    let width = mode as usize / 8;
    let sp = machine.image.as_ref().unwrap().info[3] - 256;
    machine.state.ip = machine.services.bind("kernel32.dll", name).unwrap();
    machine.state.regs[4] = sp;
    machine
        .memory
        .write_int(sp, width, gateways::layout(mode).returned)
        .unwrap();
    for (n, &value) in args.iter().enumerate() {
        if mode == Mode::X86 {
            machine
                .memory
                .write_int(sp + 4 + n as u64 * 4, 4, value)
                .unwrap();
        } else if n < 4 {
            machine.state.regs[[1, 2, 8, 9][n]] = value;
        } else {
            machine
                .memory
                .write_int(sp + 40 + (n - 4) as u64 * 8, 8, value)
                .unwrap();
        }
    }
    machine
        .services
        .dispatch(
            &mut machine.state,
            &mut machine.memory,
            machine.image.as_mut(),
            &mut machine.module_files,
            ServiceSample::default(),
        )
        .unwrap();
    machine.state.regs[0]
}

#[test]
fn process_identity_reaches_tebs_tls_workers_handles_and_snapshots() {
    for mode in [Mode::X86, Mode::X64] {
        for pid in [2, 31] {
            let process = current::Current::with_id(mode, pid).unwrap();
            let first = process.first_thread();
            let mut machine = Machine::new(mode, 0, 0);
            machine
                .load_pe_process(
                    &image(mode),
                    Launch {
                        base: 0,
                        command: "\"C:\\program.exe\"",
                        executable: None,
                        diagnostic_imports: false,
                        process,
                    },
                )
                .unwrap();
            let loaded = machine.image.as_mut().unwrap();
            assert_eq!(loaded.threads.active, first);
            assert_eq!(loaded.loader_owner, Some(first));
            let tls = loaded.modules.modules[&loaded.info[0]]
                .tls
                .as_ref()
                .unwrap();
            assert_eq!(tls.blocks[&first], loaded.info[8]);
            assert_eq!(
                machine.memory.read_int(loaded.info[8], 4).unwrap(),
                0x11223344
            );
            let mut args = [0; 16];
            args[2] = loaded.info[2];
            args[4] = 4;
            let worker = loaded
                .create_thread(&mut machine.memory, &args)
                .unwrap_or_else(|_| panic!("worker creation for process {pid}"));
            assert_eq!(loaded.threads.target(worker, 0).unwrap(), first + 1);
            let width = mode as usize / 8;
            let pid_offset = if width == 4 { 0x20 } else { 0x40 };
            for teb in loaded.threads.tebs() {
                assert_eq!(
                    machine.memory.read_int(teb + pid_offset, width).unwrap(),
                    pid.into()
                );
            }
            let snap = loaded
                .snapshots
                .create(6, "program.exe", loaded.threads.snapshot())
                .unwrap();
            let entry = loaded
                .snapshots
                .entry(snap, crate::win32::catalog::SnapshotApi::ThreadFirst, width)
                .unwrap();
            assert_eq!(u32::from_le_bytes(entry[8..12].try_into().unwrap()), first);
            assert_eq!(u32::from_le_bytes(entry[12..16].try_into().unwrap()), pid);
            assert_eq!(
                service(&mut machine, "GetCurrentProcessId", &[]),
                pid.into()
            );
            assert_eq!(
                service(&mut machine, "GetCurrentThreadId", &[]),
                first.into()
            );
            let handle = service(
                &mut machine,
                "OpenProcess",
                &[current::QUERY.into(), 0, pid.into()],
            );
            assert_ne!(handle, 0);
            assert_eq!(service(&mut machine, "GetProcessId", &[handle]), pid.into());
            assert_eq!(
                service(&mut machine, "GetThreadId", &[worker]),
                (first + 1).into()
            );
            assert_eq!(
                service(&mut machine, "OpenProcess", &[current::QUERY.into(), 0, 1]),
                0
            );
            let loaded = machine.image.as_mut().unwrap();
            loaded.check_rejected_threads(&machine.memory).unwrap();
            loaded.clear_rejected_threads(&mut machine.memory);
            assert_eq!(
                loaded.threads.live().map(|(id, _)| id).collect::<Vec<_>>(),
                [first]
            );
        }
    }
}

#[test]
fn invalid_process_identity_or_abi_cannot_publish_a_loaded_image() {
    for mode in [Mode::X86, Mode::X64] {
        for pid in [0, 65536] {
            assert!(current::Current::with_id(mode, pid).is_err());
        }
        let other = if mode == Mode::X86 {
            Mode::X64
        } else {
            Mode::X86
        };
        let mut machine = Machine::new(mode, 0, 0);
        let process = current::Current::with_id(other, 2).unwrap();
        assert!(machine
            .load_pe_process(
                &image(mode),
                Launch {
                    base: 0,
                    command: "program.exe",
                    executable: None,
                    diagnostic_imports: false,
                    process
                }
            )
            .is_err());
        assert!(machine.image.is_none());
        assert_eq!(machine.state.work, 0);
        assert_eq!(machine.code_cache.len(), 0);
    }
}
