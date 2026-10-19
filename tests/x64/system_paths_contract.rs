//! Directory query capacity, argument order and fault behavior, independent of catalogs.
use super::*;

#[test]
fn system_paths_preserve_units_capacity_outputs_last_error_and_saved_return_frame() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let output = STACK + 0x200;
    for (name, path, wide, temp) in [
        ("GetSystemDirectoryA", "C:\\Windows\\System32", false, false),
        ("GetSystemDirectoryW", "C:\\Windows\\System32", true, false),
        ("GetWindowsDirectoryA", "C:\\Windows", false, false),
        ("GetWindowsDirectoryW", "C:\\Windows", true, false),
        ("GetSystemWindowsDirectoryA", "C:\\Windows", false, false),
        ("GetSystemWindowsDirectoryW", "C:\\Windows", true, false),
        ("GetTempPathW", "C:\\Temp\\", true, true),
        ("GetTempPath2W", "C:\\Temp\\", true, true),
    ] {
        let expected: Vec<u8> = if wide {
            path.encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect()
        } else {
            path.bytes().chain([0]).collect()
        };
        let count = path.len() as u64;
        let args = |pointer, capacity| {
            if temp {
                [capacity, pointer]
            } else {
                [pointer, capacity]
            }
        };
        for dll in ["kernel32.dll", "kernelbase.dll"] {
            assert!(wire::x64::runtime::import_supported(dll, name));
            assert!(!wire::x64::runtime::import_supported("shell32.dll", name));
            startup_call(&mut mem, &mut cpu, &mut runtime, "SetLastError", &[77]);
            mem.write(output, &[0xa5; 64]).unwrap();
            for capacity in [0, count] {
                let pointer = if capacity == 0 { 0 } else { output };
                assert_eq!(
                    startup_dll_call(
                        &mut mem,
                        &mut cpu,
                        &mut runtime,
                        dll,
                        name,
                        &args(pointer, capacity)
                    ),
                    count + 1
                );
                assert_eq!(mem.read_int(output, 8).unwrap(), 0xa5a5a5a5a5a5a5a5);
            }
            assert_eq!(
                startup_dll_call(
                    &mut mem,
                    &mut cpu,
                    &mut runtime,
                    dll,
                    name,
                    &args(output, count + 1)
                ),
                count
            );
            let mut actual = vec![0; expected.len()];
            mem.read(output, &mut actual).unwrap();
            assert_eq!(actual, expected);
            assert_eq!(
                mem.read_int(output + expected.len() as u64, 1).unwrap(),
                0xa5
            );
            assert_eq!(
                startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
                77
            );
        }
        // Directory output may alias the original return slot after it has been saved.
        let alias = STACK + 0x1f88;
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                name,
                &args(alias, count + 1)
            ),
            count
        );
        let bad = STACK + 8192 - 4;
        mem.write(bad, &[0xa5; 4]).unwrap();
        cpu.regs[4] = STACK + 0x188;
        mem.write_int(cpu.regs[4], 8, BASE).unwrap();
        let a = args(bad, count + 1);
        cpu.regs[1] = a[0];
        cpu.regs[2] = a[1];
        let before = cpu.clone();
        let calls = runtime.api_calls;
        assert!(runtime
            .dispatch("kernel32.dll", name, &mut cpu, &mut mem)
            .is_err());
        assert_eq!(cpu, before);
        assert_eq!(runtime.api_calls, calls);
        assert_eq!(mem.read_int(bad, 4).unwrap(), 0xa5a5a5a5);
    }
}
