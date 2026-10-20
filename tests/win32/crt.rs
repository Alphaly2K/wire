//! Existing behavioral tests grouped by crt; expectations remain independent of API catalogs.

use super::*;

#[test]
fn crt_pointer_state_keeps_lazy_argv_errno_and_exhaustion_order() {
    let mut v = Vm::new();
    let start = v.state.scratch_position();
    v.k("SetLastError", &[77]);
    let errno = v.call("msvcrt.dll", "_errno", &[]);
    assert_eq!(errno, start);
    v.mem.write_u32(errno, 34).unwrap();
    assert_eq!(
        v.call(
            "msvcrt.dll",
            "__getmainargs",
            &[DATA, DATA + 4, DATA + 8, 0, 0]
        ),
        0
    );
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 1);
    let argv = v.mem.read_u32(DATA + 4).unwrap();
    let envp = v.mem.read_u32(DATA + 8).unwrap();
    assert_eq!(argv, (start + 19) & !15);
    assert_eq!(envp, argv + 16);
    assert_eq!(v.mem.read_u32(argv).unwrap(), v.state.cmdline_a);
    assert_eq!(v.mem.read_u32(argv + 4).unwrap(), 0);
    assert_eq!(v.mem.read_u32(envp).unwrap(), 0);
    let top = v.state.scratch_position();
    assert_eq!(v.call("msvcrt.dll", "_errno", &[]), errno);
    assert_eq!(v.mem.read_u32(errno).unwrap(), 34);
    assert_eq!(v.state.build_argv(&mut v.mem), (1, argv, envp));
    assert_eq!(v.state.scratch_position(), top);
    assert_eq!(v.state.last_error, 77);
    assert_eq!(v.state.scratch_alloc(&mut v.mem, 1, 3), 0);
    assert_eq!(v.state.scratch_position(), top);
    assert_eq!(v.state.last_error, 8);
    assert_eq!(v.state.scratch_alloc(&mut v.mem, 0x01000000 - top, 1), top);
    assert_eq!(v.state.scratch_position(), 0x01000000);
    // Cached pointers survive arena exhaustion; new allocations fail in place.
    assert_eq!(v.call("msvcrt.dll", "_errno", &[]), errno);
    assert_eq!(v.state.build_argv(&mut v.mem), (1, argv, envp));
    assert_eq!(v.state.scratch_alloc(&mut v.mem, 0, 1), 0);
    assert_eq!(v.state.scratch_position(), 0x01000000);
}

#[test]
fn crt_math_returns_x87_values_and_integer_helpers_consume_st0() {
    use rustc_apfloat::{
        ieee::{Double, X87DoubleExtended as Ext},
        Float, FloatConvert, Round, StatusAnd,
    };
    let mut v = Vm::new();
    let extended = |value: f64| {
        let r: StatusAnd<Ext> = Double::from_bits(value.to_bits() as u128)
            .convert_r(Round::NearestTiesToEven, &mut false);
        r.value.to_bits()
    };
    let result = |v: &Vm| {
        let r: StatusAnd<Double> =
            Ext::from_bits(v.cpu.fpu.st_bits(0)).convert_r(Round::NearestTiesToEven, &mut false);
        r.value.to_bits() as u64
    };
    let words = |x: f64| [x.to_bits() as u32, (x.to_bits() >> 32) as u32];
    for input in [-3.125, 3.125, -0., 0., f64::INFINITY] {
        v.cpu.fpu = Default::default();
        v.cpu.fpu.set_st_bits(0, extended(42.));
        v.call("msvcrt.dll", "floor", &words(input));
        assert_eq!(v.cpu.fpu.top(), 7);
        assert_eq!(result(&v), input.floor().to_bits());
        assert_eq!(v.cpu.fpu.st_bits(1), extended(42.));
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 252);
    }
    let errno = v.call("msvcrt.dll", "_errno", &[]);
    for (x, y, expected, error) in [
        (2., -3., 0.125, 0),
        (-2., 3., -8., 0),
        (-2., 4., 16., 0),
        (4., 0.5, 2., 0),
        (-0., 3., -0., 0),
        (-0., -3., f64::NEG_INFINITY, 34),
        (f64::NAN, 0., 1., 0),
        (1., f64::NAN, 1., 0),
    ] {
        for intrinsic in [false, true] {
            v.cpu.fpu = Default::default();
            v.mem.write_u32(errno, 0).unwrap();
            let mut args = words(x).to_vec();
            args.extend(words(y));
            if intrinsic {
                v.cpu.fpu.set_st_bits(0, extended(y));
                v.cpu.fpu.set_st_bits(1, extended(x));
                v.call("msvcrt.dll", "_CIpow", &[]);
                assert_eq!(v.cpu.fpu.top(), 1);
            } else {
                v.call("msvcrt.dll", "pow", &args);
                assert_eq!(v.cpu.fpu.top(), 7);
            }
            assert_eq!(
                result(&v),
                expected.to_bits(),
                "{x}^{y} intrinsic={intrinsic}"
            );
            assert_eq!(v.mem.read_u32(errno).unwrap(), error);
        }
    }
    // Numerical oracle across the finite range; CRT returns a binary64 result.
    for (x, y) in [
        (0.1, 0.7),
        (123.25, -2.75),
        (1.0001, 1234.),
        (1e-200, 1.25),
        (1e200, 0.75),
    ] {
        v.cpu.fpu = Default::default();
        let mut args = words(x).to_vec();
        args.extend(words(y));
        v.call("msvcrt.dll", "pow", &args);
        assert!(result(&v).abs_diff(x.powf(y).to_bits()) <= 1, "{x}^{y}");
    }
    v.cpu.fpu = Default::default();
    let mut args = words(-2.).to_vec();
    args.extend(words(0.5));
    v.call("msvcrt.dll", "pow", &args);
    assert!(f64::from_bits(result(&v)).is_nan());
    assert_eq!(v.mem.read_u32(errno).unwrap(), 33);
    for rc in 0..4 {
        for (input, exp, expected) in [
            (1., -1074, 1),
            (1., i32::MIN, if rc == 2 { 1 } else { 0 }),
            (
                -1.,
                i32::MIN,
                0x8000000000000000 | if rc == 1 { 1 } else { 0 },
            ),
            (
                1.,
                i32::MAX,
                if rc == 1 || rc == 3 {
                    0x7fefffffffffffff
                } else {
                    0x7ff0000000000000
                },
            ),
        ] {
            v.cpu.fpu = Default::default();
            v.cpu.fpu.cw |= rc << 10;
            let mut args = words(input).to_vec();
            args.push(exp as u32);
            v.call("msvcrt.dll", "ldexp", &args);
            assert_eq!(result(&v), expected, "ldexp RC={rc} exp={exp}");
        }
    }
    for name in ["_ftol", "_ftol2", "_ftol2_sse"] {
        for input in [3.9, -3.9, 4294967296.5, -4294967296.5, 0., f64::NAN] {
            v.cpu.fpu = Default::default();
            v.cpu.fpu.cw |= 2 << 10; // conversion truncates regardless of RC
            v.cpu.fpu.set_st_bits(0, extended(input));
            v.call("msvcrt.dll", name, &[]);
            let actual = v.cpu.regs[0] as u64 | ((v.cpu.regs[2] as u64) << 32);
            assert_eq!(
                actual,
                if input.is_nan() {
                    1 << 63
                } else {
                    input.trunc() as i64 as u64
                }
            );
            assert_eq!(v.cpu.fpu.top(), 1);
            assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 252);
        }
    }
}

#[test]
fn crt_qsort_calls_guest_comparator_preserves_records_and_callback_boundaries() {
    let mut v = Vm::new();
    let mut comparator = vec![0xff, 0x05];
    comparator.extend((DATA + 0x200).to_le_bytes());
    comparator.extend([
        0x8b, 0x44, 0x24, 4, 0x8b, 0x54, 0x24, 8, 0x8b, 0, 0x3b, 2, 0x0f, 0x9f, 0xc0, 0x0f, 0x9c,
        0xc2, 0x0f, 0xb6, 0xc0, 0x0f, 0xb6, 0xd2, 0x29, 0xd0, 0xc3,
    ]);
    v.mem.write_bytes(CODE, &comparator).unwrap();
    let values = [i32::MAX, -4, 0, 4, -4, i32::MIN, 9];
    for (i, key) in values.into_iter().enumerate() {
        v.mem.write_u32(DATA + i as u32 * 8, key as u32).unwrap();
        v.mem.write_u32(DATA + i as u32 * 8 + 4, i as u32).unwrap();
    }
    v.call("msvcrt.dll", "qsort", &[DATA, 7, 8, CODE]);
    let mut keys = Vec::new();
    for i in 0..7 {
        let key = v.mem.read_u32(DATA + i * 8).unwrap() as i32;
        let original = v.mem.read_u32(DATA + i * 8 + 4).unwrap() as usize;
        assert_eq!(key, values[original]);
        keys.push(key);
    }
    assert_eq!(keys, [i32::MIN, -4, -4, 0, 4, 9, i32::MAX]);
    assert!(v.mem.read_u32(DATA + 0x200).unwrap() > 0);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 252);
    assert!(v
        .try_call("msvcrt.dll", "qsort", &[DATA, u32::MAX, 8, CODE])
        .is_err());
    assert!(v
        .try_call("msvcrt.dll", "qsort", &[0x30000000, 2, 4, CODE])
        .is_err());
    assert!(v.try_call("msvcrt.dll", "qsort", &[DATA, 2, 4, 0]).is_err());
    v.call("msvcrt.dll", "qsort", &[0, 0, 4, 0]);
    v.mem.write_bytes(CODE, &[0xeb, 0xfe]).unwrap();
    let budget = v.cpu.instr_count;
    let stub = v.api.register("msvcrt.dll", Some("qsort"), None);
    v.cpu.eip = stub;
    v.cpu.set_esp(STACK_BASE + STACK_SIZE - 256);
    for (i, n) in [RET, DATA, 2, 4, CODE].into_iter().enumerate() {
        v.mem.write_u32(v.cpu.esp() + i as u32 * 4, n).unwrap();
    }
    let index = v.api.stub_index(stub).unwrap();
    let error = api::dispatch(
        index,
        &mut v.cpu,
        &mut v.mem,
        &mut v.api,
        &mut v.state,
        &mut v.log,
        &mut v.sched,
        budget + 8,
    )
    .unwrap_err();
    assert!(error.contains("instruction limit inside callback"));
    assert_eq!(v.cpu.instr_count, budget + 8);
    let exit = v.api.register("kernel32.dll", Some("ExitProcess"), None);
    let mut code = vec![0x6a, 7, 0xb8];
    code.extend(exit.to_le_bytes());
    code.extend([0xff, 0xd0]);
    v.mem.write_bytes(CODE, &code).unwrap();
    v.call("msvcrt.dll", "qsort", &[DATA, 2, 4, CODE]);
    assert!(v.cpu.halted);
    assert_eq!(v.cpu.exit_code, Some(7));
}

#[test]
fn crt_initializers_reload_later_entries_and_keep_first_error_while_continuing() {
    for (name, result) in [("_initterm", 0), ("_initterm_e", 7)] {
        let mut v = Vm::new();
        let second = CODE + 0x100;
        let third = CODE + 0x200;
        let mut first = vec![0xc7, 0x05];
        first.extend((DATA + 4).to_le_bytes());
        first.extend(second.to_le_bytes());
        first.extend([0xb8, 7, 0, 0, 0, 0xc3]);
        v.mem.write_bytes(CODE, &first).unwrap();
        for (procedure, counter, error) in
            [(second, DATA + 0x100, 11u32), (third, DATA + 0x104, 13u32)]
        {
            let mut code = vec![0xff, 0x05];
            code.extend(counter.to_le_bytes());
            code.push(0xb8);
            code.extend(error.to_le_bytes());
            code.push(0xc3);
            v.mem.write_bytes(procedure, &code).unwrap();
        }
        for (i, value) in [CODE, 0, third].into_iter().enumerate() {
            v.mem.write_u32(DATA + i as u32 * 4, value).unwrap();
        }
        assert_eq!(v.call("msvcrt.dll", name, &[DATA, DATA + 12]), result);
        assert_eq!(v.mem.read_u32(DATA + 0x100).unwrap(), 1);
        assert_eq!(v.mem.read_u32(DATA + 0x104).unwrap(), 1);
    }
}

#[test]
fn kernel_and_crt_strings_preserve_bytes_lengths_and_numeric_errors() {
    let mut v = Vm::new();
    v.mem.write_bytes(DATA, &[0x83, 0x5c, 0xff, 0]).unwrap();
    assert_eq!(v.k("lstrlenA", &[DATA]), 3);
    assert_eq!(v.call("msvcrt.dll", "strlen", &[DATA]), 3);
    assert_eq!(v.k("lstrcpyA", &[DATA + 0x100, DATA]), DATA + 0x100);
    let mut raw = [0; 4];
    v.mem.read_bytes(DATA + 0x100, &mut raw).unwrap();
    assert_eq!(raw, [0x83, 0x5c, 0xff, 0]);
    assert_eq!(v.k("lstrcpynA", &[DATA + 0x100, 0, 1]), DATA + 0x100);
    assert_eq!(v.mem.read_u8(DATA + 0x100).unwrap(), 0);
    assert_eq!(v.k("lstrcpynW", &[DATA + 0x100, 0, 0]), DATA + 0x100);
    assert_eq!(v.call("msvcrt.dll", "strncmp", &[0, 0, 0]), 0);
    assert_eq!(v.call("msvcrt.dll", "_mbsinc", &[DATA]), DATA + 2);
    v.mem.write_bytes(DATA, &[0x83, 0]).unwrap();
    assert_eq!(v.call("msvcrt.dll", "_mbsinc", &[DATA]), DATA + 1);
    v.mem
        .write_bytes(DATA, &[b'a', 0x83, 0x5c, b'b', 0])
        .unwrap();
    assert_eq!(
        v.call("msvcrt.dll", "_mbsnbcpy", &[DATA + 0x100, DATA, 2]),
        DATA + 0x100
    );
    assert_eq!(v.mem.read_u16(DATA + 0x100).unwrap(), b'a' as u16);
    v.call("msvcrt.dll", "_mbsnbcpy", &[DATA + 0x100, DATA, 6]);
    let mut raw = [1; 6];
    v.mem.read_bytes(DATA + 0x100, &mut raw).unwrap();
    assert_eq!(raw, [b'a', 0x83, 0x5c, b'b', 0, 0]);
    let errno = v.call("msvcrt.dll", "_errno", &[]);
    for (input, base, value, end, error) in [
        ("  -0x10rest", 0, 0xfffffff0, 7, 0),
        ("0779", 0, 63, 3, 0),
        ("0xZ", 0, 0, 1, 0),
        (" +z", 10, 0, 0, 0),
        ("4294967296x", 10, u32::MAX, 10, 34),
        ("12", 1, 0, 0, 22),
    ] {
        v.mem.write_u32(errno, 0).unwrap();
        v.mem
            .write_bytes(DATA, format!("{input}\0").as_bytes())
            .unwrap();
        assert_eq!(
            v.call("msvcrt.dll", "strtoul", &[DATA, DATA + 0x200, base]),
            value,
            "{input}"
        );
        assert_eq!(v.mem.read_u32(DATA + 0x200).unwrap(), DATA + end);
        assert_eq!(v.mem.read_u32(errno).unwrap(), error);
    }
}

#[test]
fn windows_varargs_formatting_reads_exact_arguments_and_preserves_cdecl_stack() {
    let mut v = Vm::new();
    v.mem.write_bytes(DATA, b"%08x|%d|%I64u|%s|%%\0").unwrap();
    v.mem.write_bytes(DATA + 0x100, b"abc\0").unwrap();
    let result = v.call(
        "user32.dll",
        "wsprintfA",
        &[
            DATA + 0x300,
            DATA,
            0x12,
            (-42i32) as u32,
            1,
            1,
            DATA + 0x100,
        ],
    );
    let expected = "00000012|-42|4294967297|abc|%";
    assert_eq!(result, expected.len() as u32);
    assert_eq!(v.mem.read_cstr(DATA + 0x300, 128).unwrap(), expected);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 4);
    v.wide(DATA, "%s|%S|%p");
    v.wide(DATA + 0x100, "ソ");
    v.mem.write_bytes(DATA + 0x200, b"raw\0").unwrap();
    v.call(
        "user32.dll",
        "wsprintfW",
        &[DATA + 0x400, DATA, DATA + 0x100, DATA + 0x200, 0x123],
    );
    assert_eq!(
        v.mem.read_wstr(DATA + 0x400, 128).unwrap(),
        "ソ|raw|00000123"
    );
    v.mem.write_bytes(DATA, b"%u/%u\0").unwrap();
    v.mem.write_u32(DATA + 0x100, 7).unwrap();
    v.mem.write_u32(DATA + 0x104, 9).unwrap();
    assert_eq!(
        v.call(
            "user32.dll",
            "wvsprintfA",
            &[DATA + 0x300, DATA, DATA + 0x100]
        ),
        3
    );
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 16);
    assert_eq!(v.mem.read_cstr(DATA + 0x300, 128).unwrap(), "7/9");
    assert!(v
        .try_call("user32.dll", "wsprintfA", &[0, DATA, 1, 2])
        .is_err());
}

#[test]
fn crt_data_imports_and_accessors_share_writable_guest_storage() {
    let mut v = Vm::new();
    v.state.cmdline_a = DATA + 0x200;
    v.state.cmdline_w = DATA + 0x400;
    v.mem.write_bytes(v.state.cmdline_a, b"test.exe\0").unwrap();
    v.wide(v.state.cmdline_w, "test.exe");
    v.mem.write_bytes(DATA, b"msvcrt.dll\0").unwrap();
    let module = v.k("LoadLibraryA", &[DATA]);
    for (global, accessor, initial) in [
        ("_acmdln", "__p__acmdln", v.state.cmdline_a),
        ("_wcmdln", "__p__wcmdln", v.state.cmdline_w),
        ("_fmode", "__p__fmode", 0),
        ("_commode", "__p__commode", 0),
        ("__argc", "__p___argc", 1),
    ] {
        let p = dll_export(&mut v, module, global);
        assert_ne!(p, 0);
        assert!(v.api.stub_index(p).is_none());
        assert_eq!(v.mem.read_u32(p).unwrap(), initial);
        assert_eq!(v.call("msvcrt.dll", accessor, &[]), p);
        v.mem.write_u32(p, 0x12345678).unwrap();
        assert_eq!(v.call("msvcrt.dll", accessor, &[]), p);
        assert_eq!(v.mem.read_u32(p).unwrap(), 0x12345678);
        assert_eq!(dll_export(&mut v, module, global), p);
    }
    let mut exe = test_dll("MSVCRT.dll!_acmdln", true, 19);
    exe[0x96..0x98].copy_from_slice(&0x102u16.to_le_bytes());
    exe[0xb4..0xb8].copy_from_slice(&0x400000u32.to_le_bytes());
    let path = v.dir.join("data-import.exe");
    std::fs::write(&path, &exe).unwrap();
    let mut mem = GuestMem::new().unwrap();
    let mut table = ApiTable::new();
    let loaded = wire::loader::load_image(&mut mem, &mut table, &path, "data-import.exe").unwrap();
    let p = mem.read_u32(0x401400).unwrap();
    assert!(table.stub_index(p).is_none());
    assert_eq!(mem.read_u32(p).unwrap(), loaded.cmdline_a);
    mem.write_u32(p, loaded.cmdline_w).unwrap();
    assert_eq!(table.lookup("msvcrt.dll", "_acmdln"), Some(p));
}

#[test]
fn crt_byte_comparison_and_c_locale_case_preserve_unsigned_bytes_eof_and_cdecl() {
    let mut v = Vm::new();
    v.mem.write_bytes(DATA, &[0, 0x80, 0xff, 3]).unwrap();
    v.mem.write_bytes(DATA + 16, &[0, 0x7f, 0xff, 9]).unwrap();
    assert_eq!(v.call("msvcrt.dll", "memcmp", &[0, 0, 0]), 0);
    assert_eq!(v.call("msvcrt.dll", "memcmp", &[DATA, DATA + 16, 1]), 0);
    assert_eq!(v.call("msvcrt.dll", "memcmp", &[DATA, DATA + 16, 4]), 1);
    assert_eq!(
        v.call("msvcrt.dll", "memcmp", &[DATA + 16, DATA, 4]) as i32,
        -1
    );
    assert!(v.try_call("msvcrt.dll", "memcmp", &[DATA, 0, 1]).is_err());
    for (input, upper, lower) in [
        (b'a' as u32, b'A' as u32, b'a' as u32),
        (b'Z' as u32, b'Z' as u32, b'z' as u32),
        (0xff, 0xff, 0xff),
        (u32::MAX, u32::MAX, u32::MAX),
        (0, 0, 0),
    ] {
        assert_eq!(v.call("msvcrt.dll", "toupper", &[input]), upper);
        assert_eq!(v.call("msvcrt.dll", "tolower", &[input]), lower);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 252);
    }
}

#[test]
fn crt_windows_version_exports_are_shared_data_for_dll_imports_and_getprocaddress() {
    let mut v = Vm::new();
    std::fs::write(
        v.dir.join("version-data.dll"),
        test_dll("msvcrt.dll!_winmajor", true, 0),
    )
    .unwrap();
    // DllMain must read the imported variable, not call it as a function.
    let path = v.dir.join("version-data.dll");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[0x200..0x20c].copy_from_slice(&[0xa1, 0, 0x14, 0, 0x10, 0x8b, 0, 0xc2, 12, 0, 0, 0]);
    bytes[0x908..0x910].copy_from_slice(&[0x01, 0x30, 0x81, 0x30, 0x10, 0x32, 0, 0]);
    std::fs::write(&path, bytes).unwrap();
    v.mem.write_bytes(DATA, b"version-data.dll\0").unwrap();
    let h = v.k("LoadLibraryA", &[DATA]);
    assert_ne!(h, 0);
    let imported = v.mem.read_u32(h + 0x1400).unwrap();
    v.mem.write_bytes(DATA, b"msvcrt.dll\0").unwrap();
    let crt = v.k("LoadLibraryA", &[DATA]);
    for (name, value) in [
        ("_winmajor", 6),
        ("_winminor", 1),
        ("_winver", 0x601),
        ("_osver", 7601),
    ] {
        let address = dll_export(&mut v, crt, name);
        assert_ne!(address, 0);
        assert_eq!(v.mem.read_u32(address).unwrap(), value);
        if name == "_winmajor" {
            assert_eq!(address, imported);
            v.mem.write_u32(address, 7).unwrap();
            assert_eq!(v.mem.read_u32(imported).unwrap(), 7);
        }
    }
}
