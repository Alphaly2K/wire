//! Behavioral tests for the Win32 subset, independent of a proprietary EXE.
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use wire::api::{self, ApiLogger, ApiState, ApiTable, HandleObj};
use wire::cpu::Cpu;
use wire::fs::GuestFs;
use wire::loader::{STACK_BASE, STACK_SIZE, TEB_ADDR};
use wire::mem::GuestMem;
use wire::thread::Sched;
const DATA: u32 = 0x20000;
const CODE: u32 = 0x10000;
const RET: u32 = CODE + 0x800;

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
fn cursor_show_count_is_signed_and_set_cursor_returns_previous_handle() {
    let mut v = Vm::new();
    assert_eq!(v.call("user32.dll", "ShowCursor", &[1]), 1);
    assert_eq!(v.call("user32.dll", "ShowCursor", &[0]), 0);
    assert_eq!(v.call("user32.dll", "ShowCursor", &[0]), u32::MAX);
    assert_eq!(v.call("user32.dll", "ShowCursor", &[1]), 0);
    let arrow = v.call("user32.dll", "LoadCursorA", &[0, 32512]);
    assert_ne!(arrow, 0);
    assert_eq!(v.call("user32.dll", "SetCursor", &[arrow]), 0);
    assert_eq!(v.call("user32.dll", "GetCursor", &[]), arrow);
    assert_eq!(v.call("user32.dll", "SetCursor", &[0]), arrow);
    assert_eq!(v.call("user32.dll", "GetCursor", &[]), 0);
    assert_eq!(v.call("user32.dll", "SetCursor", &[0xdeadbeef]), 0);
    assert_eq!(v.state.last_error, 6);
}

#[test]
fn monitor_callbacks_and_ansi_info_return_real_bounds_and_honor_clipping() {
    let mut v = Vm::new();
    let mut code = vec![0xff, 0x05];
    code.extend((DATA + 0x300).to_le_bytes());
    code.extend([0x8b, 0x44, 0x24, 12, 0x8b, 0x40, 8, 0xa3]);
    code.extend((DATA + 0x304).to_le_bytes());
    code.extend([0xb8, 0, 0, 0, 0, 0xc2, 16, 0]);
    v.mem.write_bytes(CODE, &code).unwrap();
    assert_eq!(
        v.call("user32.dll", "EnumDisplayMonitors", &[0, 0, CODE, 0]),
        0
    );
    assert_eq!(v.mem.read_u32(DATA + 0x300).unwrap(), 1);
    assert_eq!(v.mem.read_u32(DATA + 0x304).unwrap(), 1024);
    for (i, n) in [2000u32, 0, 2100, 100].into_iter().enumerate() {
        v.mem.write_u32(DATA + i as u32 * 4, n).unwrap();
    }
    assert_eq!(
        v.call("user32.dll", "EnumDisplayMonitors", &[0, DATA, CODE, 0]),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 0x300).unwrap(), 1);
    for (name, size) in [
        ("GetMonitorInfoA", 40),
        ("GetMonitorInfoA", 72),
        ("GetMonitorInfoW", 104),
    ] {
        v.mem.write_u32(DATA, size).unwrap();
        assert_eq!(v.call("user32.dll", name, &[0x40000001, DATA]), 1);
        assert_eq!(v.mem.read_u32(DATA + 12).unwrap(), 1024);
        assert_eq!(v.mem.read_u32(DATA + 16).unwrap(), 768);
        if size == 72 {
            assert_eq!(v.mem.read_cstr(DATA + 40, 32).unwrap(), "\\\\.\\DISPLAY1");
        }
        if size == 104 {
            assert_eq!(v.mem.read_wstr(DATA + 40, 32).unwrap(), "\\\\.\\DISPLAY1");
        }
    }
}

#[test]
fn wine_path_extensions_and_unquoting_preserve_existing_names() {
    let mut v = Vm::new();
    v.mem.write_bytes(DATA, b"dir.with.dot\\name\0").unwrap();
    v.mem.write_bytes(DATA + 0x100, b".pgd\0").unwrap();
    assert_eq!(
        v.call("shlwapi.dll", "PathAddExtensionA", &[DATA, DATA + 0x100]),
        1
    );
    assert_eq!(
        v.mem.read_cstr(DATA, 260).unwrap(),
        "dir.with.dot\\name.pgd"
    );
    assert_eq!(
        v.call("shlwapi.dll", "PathAddExtensionA", &[DATA, DATA + 0x100]),
        0
    );
    v.wide(DATA, "\"ソフト path\"");
    v.call("shlwapi.dll", "PathUnquoteSpacesW", &[DATA]);
    assert_eq!(v.mem.read_wstr(DATA, 260).unwrap(), "ソフト path");
    v.mem.write_bytes(DATA, b"\"incomplete\0").unwrap();
    v.call("shlwapi.dll", "PathUnquoteSpacesA", &[DATA]);
    assert_eq!(v.mem.read_cstr(DATA, 260).unwrap(), "\"incomplete");
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
fn guid_strings_use_com_allocations_endianness_and_checked_capacity() {
    let mut v = Vm::new();
    let bytes = [
        0xca, 0x58, 0x35, 0x97, 0x88, 0xb3, 0xd3, 0x11, 0xae, 0xf6, 0, 0x40, 0x26, 0x40, 2, 0xc6,
    ];
    v.mem.write_bytes(DATA, &bytes).unwrap();
    assert_eq!(
        v.call("ole32.dll", "StringFromCLSID", &[DATA, DATA + 0x100]),
        0
    );
    let p = v.mem.read_u32(DATA + 0x100).unwrap();
    assert_ne!(p, 0);
    let text = "{973558CA-B388-11D3-AEF6-0040264002C6}";
    assert_eq!(v.mem.read_wstr(p, 39).unwrap(), text);
    assert_eq!(
        v.call("ole32.dll", "CLSIDFromString", &[p, DATA + 0x200]),
        0
    );
    let mut round = [0; 16];
    v.mem.read_bytes(DATA + 0x200, &mut round).unwrap();
    assert_eq!(round, bytes);
    v.mem.write_u32(DATA + 0x300, 0xdeadbeef).unwrap();
    assert_eq!(
        v.call("ole32.dll", "StringFromGUID2", &[DATA, DATA + 0x300, 38]),
        0
    );
    assert_eq!(v.mem.read_u32(DATA + 0x300).unwrap(), 0xdeadbeef);
    assert_eq!(
        v.call("ole32.dll", "StringFromGUID2", &[DATA, DATA + 0x300, 39]),
        39
    );
    v.call("ole32.dll", "CoTaskMemFree", &[p]);
    assert_eq!(v.k("HeapSize", &[v.state.process_heap, 0, p]), u32::MAX);
    v.wide(DATA + 0x300, "{973558CA-B388-11D3-AEF6-0040264002CZ}");
    assert_eq!(
        v.call(
            "ole32.dll",
            "CLSIDFromString",
            &[DATA + 0x300, DATA + 0x200]
        ),
        0x800401f3
    );
}

#[test]
fn variants_deep_copy_bstr_aliases_and_reject_unsupported_managed_types() {
    let mut v = Vm::new();
    v.wide(DATA, "ソフト");
    let p = v.call("oleaut32.dll", "SysAllocString", &[DATA]);
    for at in [DATA + 0x100, DATA + 0x120] {
        v.call("oleaut32.dll", "VariantInit", &[at]);
    }
    v.mem.write_u16(DATA + 0x100, 8).unwrap();
    v.mem.write_u32(DATA + 0x108, p).unwrap();
    assert_eq!(
        v.call("oleaut32.dll", "VariantCopy", &[DATA + 0x120, DATA + 0x100]),
        0
    );
    let copy = v.mem.read_u32(DATA + 0x128).unwrap();
    assert_ne!(copy, p);
    assert_eq!(v.mem.read_wstr(copy, 10).unwrap(), "ソフト");
    assert_eq!(
        v.call(
            "oleaut32.dll",
            "VariantChangeType",
            &[DATA + 0x120, DATA + 0x120, 0, 8]
        ),
        0
    );
    assert_eq!(
        v.k("HeapSize", &[v.state.process_heap, 0, copy - 4]),
        u32::MAX
    );
    v.call("oleaut32.dll", "VariantClear", &[DATA + 0x100]);
    let still = v.mem.read_u32(DATA + 0x128).unwrap();
    assert_eq!(v.mem.read_wstr(still, 10).unwrap(), "ソフト");
    assert_eq!(v.call("oleaut32.dll", "VariantClear", &[DATA + 0x120]), 0);
    assert_eq!(v.mem.read_u16(DATA + 0x120).unwrap(), 0);
    v.mem.write_u16(DATA + 0x100, 9).unwrap(); // IDispatch requires a COM backend
    assert_eq!(
        v.call("oleaut32.dll", "VariantCopy", &[DATA + 0x120, DATA + 0x100]),
        0x80020005
    );
    assert_eq!(
        v.call("oleaut32.dll", "VariantClear", &[DATA + 0x100]),
        0x80020008
    );
}

#[test]
fn version_resources_query_fixed_translation_and_ansi_unicode_nested_strings() {
    fn block(key: &str, text: bool, value: Vec<u8>, children: Vec<Vec<u8>>) -> Vec<u8> {
        let mut b = vec![0; 6];
        b[2..4].copy_from_slice(&((value.len() / if text { 2 } else { 1 }) as u16).to_le_bytes());
        b[4..6].copy_from_slice(&u16::from(text).to_le_bytes());
        b.extend(key.encode_utf16().chain([0]).flat_map(u16::to_le_bytes));
        b.resize((b.len() + 3) & !3, 0);
        b.extend(value);
        for child in children {
            b.resize((b.len() + 3) & !3, 0);
            b.extend(child);
        }
        let len = b.len() as u16;
        b[..2].copy_from_slice(&len.to_le_bytes());
        b
    }
    let mut v = Vm::new();
    let leaf = block(
        "CompanyName",
        true,
        "ソフト"
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect(),
        vec![],
    );
    let strings = block(
        "StringFileInfo",
        true,
        vec![],
        vec![block("041103a4", true, vec![], vec![leaf])],
    );
    let vars = block(
        "VarFileInfo",
        true,
        vec![],
        vec![block(
            "Translation",
            false,
            [0x411u16.to_le_bytes(), 932u16.to_le_bytes()].concat(),
            vec![],
        )],
    );
    let mut fixed = vec![0; 52];
    fixed[..4].copy_from_slice(&0xfeef04bdu32.to_le_bytes());
    let blob = block("VS_VERSION_INFO", false, fixed, vec![strings, vars]);
    let mut exe = test_dll("kernel32.dll!GetCurrentProcessId", true, 0);
    exe[0x108..0x10c].copy_from_slice(&0x1900u32.to_le_bytes());
    exe[0x10c..0x110].copy_from_slice(&0x100u32.to_le_bytes());
    let root = 0xb00;
    for off in [0, 24, 48] {
        exe[root + off + 14..root + off + 16].copy_from_slice(&1u16.to_le_bytes());
    }
    for (off, n) in [
        (16, 16u32),
        (20, 0x80000018),
        (40, 1),
        (44, 0x80000030),
        (64, 0x411),
        (68, 72),
        (72, 0x1a00),
        (76, blob.len() as u32),
    ] {
        exe[root + off..root + off + 4].copy_from_slice(&n.to_le_bytes());
    }
    exe[0xc00..0xc00 + blob.len()].copy_from_slice(&blob);
    std::fs::write(v.dir.join("version.dll"), &exe).unwrap();
    v.mem.write_bytes(DATA, b"C:\\version.dll\0").unwrap();
    let size = v.call(
        "version.dll",
        "GetFileVersionInfoSizeA",
        &[DATA, DATA + 0x100],
    );
    assert!(size >= blob.len() as u32);
    assert_eq!(v.mem.read_u32(DATA + 0x100).unwrap(), 0);
    assert_eq!(
        v.call(
            "version.dll",
            "GetFileVersionInfoA",
            &[DATA, 0, size, DATA + 0x1000]
        ),
        1
    );
    for (path, len) in [("\\", 52), ("\\VarFileInfo\\Translation", 4)] {
        v.wide(DATA + 0x200, path);
        assert_eq!(
            v.call(
                "version.dll",
                "VerQueryValueW",
                &[DATA + 0x1000, DATA + 0x200, DATA + 0x100, DATA + 0x104]
            ),
            1
        );
        assert_eq!(v.mem.read_u32(DATA + 0x104).unwrap(), len);
    }
    let path = "\\StringFileInfo\\041103a4\\CompanyName";
    v.wide(DATA + 0x200, path);
    assert_eq!(
        v.call(
            "version.dll",
            "VerQueryValueW",
            &[DATA + 0x1000, DATA + 0x200, DATA + 0x100, DATA + 0x104]
        ),
        1
    );
    let wide = v.mem.read_u32(DATA + 0x100).unwrap();
    assert_eq!(v.mem.read_wstr(wide, 10).unwrap(), "ソフト");
    assert_eq!(v.mem.read_u32(DATA + 0x104).unwrap(), 4);
    v.mem
        .write_bytes(DATA + 0x200, format!("{path}\0").as_bytes())
        .unwrap();
    assert_eq!(
        v.call(
            "version.dll",
            "VerQueryValueA",
            &[DATA + 0x1000, DATA + 0x200, DATA + 0x100, DATA + 0x104]
        ),
        1
    );
    let ansi = v.mem.read_u32(DATA + 0x100).unwrap();
    assert_eq!(
        wire::file_api::read_name(&v.mem, ansi, false).unwrap(),
        "ソフト"
    );
    assert_eq!(v.mem.read_u32(DATA + 0x104).unwrap(), 7);
    assert_eq!(v.mem.read_wstr(wide, 10).unwrap(), "ソフト");
    v.mem.write_u16(DATA + 0x1000, 5).unwrap();
    assert_eq!(
        v.call(
            "version.dll",
            "VerQueryValueA",
            &[DATA + 0x1000, DATA + 0x200, DATA + 0x100, DATA + 0x104]
        ),
        0
    );
    assert_eq!(v.mem.read_u32(DATA + 0x104).unwrap(), 0);
}

#[test]
fn large_dll_and_resource_only_dll_preserve_heap_and_reuse_loaded_basenames() {
    let mut v = Vm::new();
    let heap = v.state.process_heap;
    let allocation = v.k("HeapAlloc", &[heap, 0, 16]);
    v.mem.write_u32(allocation, 0x1234).unwrap();
    let mut large = test_dll("kernel32.dll!GetCurrentProcessId", true, 42);
    large[0xd0..0xd4].copy_from_slice(&0x4138000u32.to_le_bytes());
    std::fs::create_dir(v.dir.join("dll")).unwrap();
    let path = v.dir.join("dll/large.dll");
    std::fs::write(&path, &large).unwrap();
    v.mem.write_bytes(DATA, b"C:\\dll\\large.dll\0").unwrap();
    let h = v.k("LoadLibraryA", &[DATA]);
    assert_ne!(h, 0);
    assert_eq!(v.mem.read_u32(allocation).unwrap(), 0x1234);
    assert_eq!(
        v.mem.virtual_query(h + 0x4137000).unwrap().allocation_base,
        h
    );
    std::fs::remove_file(&path).unwrap();
    v.mem.write_bytes(DATA, b"LARGE.DLL\0").unwrap();
    assert_eq!(v.k("LoadLibraryA", &[DATA]), h);
    assert_eq!(v.k("FreeLibrary", &[h]), 1);
    assert!(v.mem.is_committed(h));
    assert_eq!(v.k("FreeLibrary", &[h]), 1);
    assert!(!v.mem.is_committed(h));
    let mut resource = test_dll("kernel32.dll!GetCurrentProcessId", true, 0);
    resource[0xa8..0xac].fill(0);
    resource[0xf8..0x108].fill(0);
    resource[0x120..0x128].fill(0);
    resource[0x19c..0x1a0].copy_from_slice(&0x40000040u32.to_le_bytes());
    std::fs::write(v.dir.join("resource.dll"), resource).unwrap();
    v.mem.write_bytes(DATA, b"resource.dll\0").unwrap();
    let h = v.k("LoadLibraryA", &[DATA]);
    assert_ne!(h, 0);
    assert!(v.mem.read_u32(h + 0x1200).is_ok());
    assert_eq!(
        v.mem.virtual_query(h + 0x1000).unwrap().protect,
        wire::mem::PAGE_READONLY
    );
    assert_eq!(v.k("FreeLibrary", &[h]), 1);
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
fn missing_registry_installations_fail_and_dsound_ordinals_resolve() {
    let mut v = Vm::new();
    v.mem
        .write_bytes(DATA, b"Software\\not-installed\0")
        .unwrap();
    v.mem.write_u32(DATA + 0x100, 0xdeadbeef).unwrap();
    assert_eq!(
        v.call(
            "advapi32.dll",
            "RegOpenKeyA",
            &[0x80000002, DATA, DATA + 0x100]
        ),
        2
    );
    assert_eq!(v.mem.read_u32(DATA + 0x100).unwrap(), 0);
    assert_eq!(
        v.call(
            "advapi32.dll",
            "RegQueryValueExA",
            &[0, DATA, 0, 0, 0, DATA + 0x100]
        ),
        6
    );
    v.mem.write_bytes(DATA, b"dsound.dll\0").unwrap();
    let module = v.k("LoadLibraryA", &[DATA]);
    let ordinal = v.k("GetProcAddress", &[module, 1]);
    assert_ne!(ordinal, 0);
    assert_eq!(ordinal, dll_export(&mut v, module, "DirectSoundCreate"));
    assert_eq!(
        v.call("dsound.dll", "DirectSoundCreate", &[0, DATA + 0x100, 0]),
        0
    );
    let sound = v.mem.read_u32(DATA + 0x100).unwrap();
    assert_ne!(sound, 0);
    assert_eq!(d3d_com(&mut v, sound, 2, &[]), 0);
}

#[test]
fn path_remove_file_spec_preserves_roots_dbcs_and_checked_buffers() {
    let mut v = Vm::new();
    for (input, expected, changed) in [
        ("C:\\koikake.exe", "C:\\", 1),
        ("C:\\dll\\Pal.dll", "C:\\dll", 1),
        ("C:\\", "C:\\", 0),
        ("C:", "C:", 0),
        ("file.exe", "", 1),
        ("\\\\server\\share\\file.exe", "\\\\server\\share", 1),
        ("C:\\dir\\\\file.exe", "C:\\dir", 1),
        ("C:\\ソフト\\ソ", "C:\\ソフト", 1),
        ("C:\\dir/file.exe", "C:\\", 1),
        ("", "", 0),
    ] {
        for wide in [false, true] {
            if wide {
                v.wide(DATA, input);
            } else {
                let mut bytes = wire::codepage::encode(input, 932).unwrap().0;
                bytes.push(0);
                v.mem.write_bytes(DATA, &bytes).unwrap();
            }
            assert_eq!(
                v.call(
                    "shlwapi.dll",
                    if wide {
                        "PathRemoveFileSpecW"
                    } else {
                        "PathRemoveFileSpecA"
                    },
                    &[DATA]
                ),
                changed,
                "{input:?} wide {wide}"
            );
            assert_eq!(
                wire::file_api::read_name(&v.mem, DATA, wide).unwrap(),
                expected
            );
        }
    }
    v.wide(DATA, "\\\\?\\C:\\file.exe");
    assert_eq!(v.call("shlwapi.dll", "PathRemoveFileSpecW", &[DATA]), 1);
    assert_eq!(v.mem.read_wstr(DATA, 128).unwrap(), "\\\\?\\C:\\");
    assert_eq!(v.call("shlwapi.dll", "PathRemoveFileSpecA", &[0]), 0);
    assert_eq!(v.call("shlwapi.dll", "PathRemoveFileSpecW", &[0x700000]), 0);
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
fn frame_adjustment_client_geometry_and_paint_clipping_share_the_same_pixels() {
    let mut v = Vm::new();
    let def = v.api.register("user32.dll", Some("DefWindowProcW"), None);
    let hwnd = test_window(&mut v, def);
    v.call("user32.dll", "SetRect", &[DATA + 0x800, 0, 0, 200, 100]);
    assert_eq!(
        v.call(
            "user32.dll",
            "AdjustWindowRectEx",
            &[DATA + 0x800, 0xc40000, 0, 0]
        ),
        1
    );
    for (i, n) in [-4i32, -27, 204, 104].into_iter().enumerate() {
        assert_eq!(
            v.mem.read_u32(DATA + 0x800 + i as u32 * 4).unwrap() as i32,
            n
        );
    }
    v.call(
        "user32.dll",
        "SetWindowLongW",
        &[hwnd, (-16i32) as u32, 0xc40000],
    );
    v.call(
        "user32.dll",
        "SetWindowPos",
        &[hwnd, 0, 30, 40, 208, 131, 0x14],
    );
    v.call("user32.dll", "GetClientRect", &[hwnd, DATA + 0x820]);
    assert_eq!(v.mem.read_u32(DATA + 0x828).unwrap(), 200);
    assert_eq!(v.mem.read_u32(DATA + 0x82c).unwrap(), 100);
    let class = v.state.user32.windows[&hwnd].class;
    v.state.user32.classes.get_mut(&class).unwrap().fields[7] = 6;
    v.call("user32.dll", "ShowWindow", &[hwnd, 1]);
    assert_eq!(
        v.call("user32.dll", "GetUpdateRect", &[hwnd, DATA + 0x840, 0]),
        1
    );
    let dc = v.call("user32.dll", "BeginPaint", &[hwnd, DATA + 0x880]);
    assert_ne!(dc, 0);
    assert_eq!(v.mem.read_u32(DATA + 0x884).unwrap(), 0); // background was truly erased
    assert_eq!(v.call("gdi32.dll", "GetPixel", &[dc, 0, 0]), 0xffffff);
    assert_eq!(v.call("user32.dll", "GetUpdateRect", &[hwnd, 0, 0]), 0);
    assert_eq!(v.call("user32.dll", "EndPaint", &[hwnd, DATA + 0x880]), 1);
    assert_eq!(v.call("gdi32.dll", "GetPixel", &[dc, 0, 0]), u32::MAX);
    v.call("user32.dll", "SetRect", &[DATA + 0x800, 10, 20, 14, 22]);
    v.call("user32.dll", "InvalidateRect", &[hwnd, DATA + 0x800, 0]);
    let dc = v.call("user32.dll", "BeginPaint", &[hwnd, DATA + 0x880]);
    assert_eq!(v.call("gdi32.dll", "SetPixel", &[dc, 10, 20, 0xff]), 0xff);
    assert_eq!(v.call("gdi32.dll", "SetPixel", &[dc, 0, 0, 0xff]), u32::MAX);
    v.call("user32.dll", "EndPaint", &[hwnd, DATA + 0x880]);
    let (w, h, pixels) = v.state.graphics.window_snapshot(&v.mem, hwnd).unwrap();
    assert_eq!((w, h), (200, 100));
    assert_eq!(pixels[20 * 200 + 10], 0xff);
    assert_eq!(pixels[0], 0xffffff);
}

#[test]
fn logical_fonts_preserve_logfont_encoding_selection_and_14_argument_creation() {
    let mut v = Vm::new();
    let dc = v.call("gdi32.dll", "CreateCompatibleDC", &[0]);
    let stock = v.call("gdi32.dll", "GetStockObject", &[17]);
    assert_ne!(stock, 0);
    assert_eq!(v.call("gdi32.dll", "GetObjectW", &[stock, 92, DATA]), 92);
    assert_eq!(v.mem.read_u32(DATA).unwrap() as i32, -16);
    v.wide(DATA + 0x400, "日本語font");
    let font = v.call(
        "gdi32.dll",
        "CreateFontW",
        &[
            (-20i32) as u32,
            0,
            0,
            0,
            700,
            1,
            0,
            0,
            128,
            0,
            0,
            4,
            0,
            DATA + 0x400,
        ],
    );
    assert_ne!(font, 0);
    assert_eq!(v.call("gdi32.dll", "GetObjectType", &[font]), 6);
    assert_eq!(v.call("gdi32.dll", "GetObjectW", &[font, 92, DATA]), 92);
    assert_eq!(v.mem.read_u32(DATA + 16).unwrap(), 700);
    assert_eq!(v.mem.read_wstr(DATA + 28, 32).unwrap(), "日本語font");
    let old = v.call("gdi32.dll", "SelectObject", &[dc, font]);
    assert_ne!(old, 0);
    assert_eq!(v.call("gdi32.dll", "DeleteObject", &[font]), 0);
    assert_eq!(v.call("gdi32.dll", "GetCurrentObject", &[dc, 6]), font);
    v.call("gdi32.dll", "SelectObject", &[dc, old]);
    assert_eq!(v.call("gdi32.dll", "DeleteObject", &[font]), 1);
    assert_eq!(v.call("gdi32.dll", "DeleteObject", &[stock]), 1);
    assert_eq!(v.call("gdi32.dll", "GetObjectA", &[stock, 60, DATA]), 60);
    let a = v.call("gdi32.dll", "CreateFontIndirectA", &[DATA]);
    assert_ne!(a, 0);
    assert_eq!(
        v.call("gdi32.dll", "GetObjectW", &[a, 92, DATA + 0x100]),
        92
    );
    assert_eq!(
        v.mem.read_wstr(DATA + 0x100 + 28, 32).unwrap(),
        "MS UI Gothic"
    );
}

#[test]
fn dib_stretch_and_readback_use_real_bgra_rows_and_checked_scan_ranges() {
    let mut v = Vm::new();
    let screen = v.call("user32.dll", "GetDC", &[0]);
    let dc = v.call("gdi32.dll", "CreateCompatibleDC", &[screen]);
    let bitmap = v.call("gdi32.dll", "CreateCompatibleBitmap", &[screen, 4, 4]);
    let old = v.call("gdi32.dll", "SelectObject", &[dc, bitmap]);
    for (i, n) in [40, 2, (-2i32) as u32, 1 | (32 << 16), 0, 16, 0, 0, 0, 0]
        .into_iter()
        .enumerate()
    {
        v.mem.write_u32(DATA + i as u32 * 4, n).unwrap();
    }
    for (i, n) in [0xff0000, 0xff00, 0xff, 0xffffff].into_iter().enumerate() {
        v.mem.write_u32(DATA + 0x100 + i as u32 * 4, n).unwrap();
    }
    assert_eq!(
        v.call(
            "gdi32.dll",
            "StretchDIBits",
            &[dc, 0, 0, 4, 4, 0, 0, 2, 2, DATA + 0x100, DATA, 0, 0xcc0020]
        ),
        2
    );
    assert_eq!(v.call("gdi32.dll", "GetPixel", &[dc, 0, 0]), 0xff);
    assert_eq!(v.call("gdi32.dll", "GetPixel", &[dc, 3, 3]), 0xffffff);
    assert_eq!(v.call("gdi32.dll", "GetPixel", &[dc, 0, 3]), 0xff0000);
    assert_eq!(
        v.call(
            "gdi32.dll",
            "StretchDIBits",
            &[dc, 0, 0, 4, 4, 0, 0, 2, 2, 0, DATA, 0, 0xcc0020]
        ),
        0
    );
    assert_eq!(v.call("gdi32.dll", "GetPixel", &[dc, 0, 0]), 0xff);
    v.call("gdi32.dll", "SelectObject", &[dc, old]);
    assert_eq!(
        v.call("gdi32.dll", "GetDIBits", &[dc, bitmap, 0, 4, 0, DATA, 0]),
        4
    );
    assert_eq!(v.mem.read_u32(DATA + 4).unwrap(), 4);
    assert_eq!(
        v.call(
            "gdi32.dll",
            "GetDIBits",
            &[dc, bitmap, 0, 4, DATA + 0x200, DATA, 0]
        ),
        4
    );
    assert_eq!(v.mem.read_u32(DATA + 0x200).unwrap(), 0xff); // bottom row: blue
    assert_eq!(v.mem.read_u32(DATA + 0x200 + 3 * 16).unwrap(), 0xff0000); // top row: red
    assert_eq!(
        v.call(
            "gdi32.dll",
            "GetDIBits",
            &[dc, bitmap, 4, 1, DATA + 0x200, DATA, 0]
        ),
        0
    );
}

#[test]
fn thread_resource_limit_returns_win32_failure_and_does_not_create_partial_handles() {
    let mut v = Vm::new();
    while v.sched.has_thread_capacity() {
        assert_ne!(v.k("CreateThread", &[0, 0, CODE, 0, 4, 0]), 0);
    }
    let count = v.sched.threads.len();
    let handles = v.state.handles.len();
    let next_tid_output = DATA;
    v.mem.write_u32(next_tid_output, 0xdeadbeef).unwrap();
    assert_eq!(v.k("CreateThread", &[0, 0, CODE, 0, 4, next_tid_output]), 0);
    assert_eq!(v.state.last_error, 8);
    assert_eq!(v.sched.threads.len(), count);
    assert_eq!(v.state.handles.len(), handles);
    assert_eq!(v.mem.read_u32(next_tid_output).unwrap(), 0xdeadbeef);
    for t in v.sched.threads.iter().skip(1) {
        assert!(t.stack_base + wire::thread::THREAD_STACK_SIZE < 0x10000000);
        assert!(t.teb < wire::loader::PEB_ADDR);
    }
}

#[test]
fn process_and_thread_affinity_match_single_cpu_profile_and_preserve_failed_outputs() {
    let mut v = Vm::new();
    v.mem.write_u32(DATA, 0xdeadbeef).unwrap();
    assert_eq!(
        v.k("GetProcessAffinityMask", &[u32::MAX, DATA, DATA + 4]),
        1
    );
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 1);
    assert_eq!(v.mem.read_u32(DATA + 4).unwrap(), 1);
    v.mem.write_u32(DATA, 0xdeadbeef).unwrap();
    assert_eq!(v.k("GetProcessAffinityMask", &[u32::MAX, DATA, 0]), 0);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 0xdeadbeef);
    let h = v.k("OpenProcess", &[0x1600, 0, 4242]);
    assert_ne!(h, 0);
    assert_eq!(v.k("GetProcessId", &[h]), 4242);
    assert_eq!(v.k("GetProcessAffinityMask", &[h, DATA, DATA + 4]), 1);
    assert_eq!(v.k("SetProcessAffinityMask", &[h, 2]), 0);
    assert_eq!(v.k("SetProcessAffinityMask", &[h, 1]), 1);
    assert_eq!(v.k("GetExitCodeProcess", &[h, DATA]), 1);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 259);
    assert_eq!(v.k("CloseHandle", &[h]), 1);
    assert_eq!(v.k("GetProcessId", &[h]), 0);
    assert_eq!(v.state.last_error, 6);
    assert_eq!(v.k("SetThreadAffinityMask", &[0xfffffffe, 1]), 1);
    assert_eq!(v.k("SetThreadAffinityMask", &[0xfffffffe, 2]), 0);
    assert_eq!(v.k("SetThreadIdealProcessor", &[0xfffffffe, 0]), 0);
    assert_eq!(v.k("SetThreadIdealProcessor", &[0xfffffffe, 1]), u32::MAX);
    assert_eq!(v.k("GetActiveProcessorCount", &[0xffff]), 1);
}

#[test]
fn default_window_position_messages_use_actual_client_changes_and_destroy_clears_input_owners() {
    let mut v = Vm::new();
    // Record lParam indexed by message, and accept WM_NCCREATE.
    let mut proc = vec![0x8b, 0x44, 0x24, 8, 0x8b, 0x54, 0x24, 16, 0x89, 0x14, 0x85];
    proc.extend((DATA + 0x1000).to_le_bytes());
    proc.extend([
        0x3d, 0x81, 0, 0, 0, 0x0f, 0x94, 0xc0, 0x0f, 0xb6, 0xc0, 0xc2, 16, 0,
    ]);
    v.mem.write_bytes(CODE, &proc).unwrap();
    let hwnd = test_window(&mut v, CODE);
    assert_ne!(hwnd, 0);
    let p = DATA + 0x800;
    for (i, n) in [hwnd, 0, 3, 4, 640, 480, 0].into_iter().enumerate() {
        v.mem.write_u32(p + i as u32 * 4, n).unwrap();
    }
    v.call("user32.dll", "DefWindowProcW", &[hwnd, 0x47, 0, p]);
    assert_eq!(
        v.mem.read_u32(DATA + 0x1000 + 3 * 4).unwrap(),
        3 | (4 << 16)
    );
    assert_eq!(
        v.mem.read_u32(DATA + 0x1000 + 5 * 4).unwrap(),
        640 | (480 << 16)
    );
    v.mem.write_u32(DATA + 0x1000 + 5 * 4, 0xdeadbeef).unwrap();
    v.mem.write_u32(p + 24, 0x1800).unwrap();
    v.call("user32.dll", "DefWindowProcW", &[hwnd, 0x47, 0, p]);
    assert_eq!(v.mem.read_u32(DATA + 0x1000 + 5 * 4).unwrap(), 0xdeadbeef);
    v.call("user32.dll", "SetFocus", &[hwnd]);
    v.call("user32.dll", "SetActiveWindow", &[hwnd]);
    v.call("user32.dll", "SetCapture", &[hwnd]);
    assert_eq!(v.call("user32.dll", "DestroyWindow", &[hwnd]), 1);
    for name in [
        "GetFocus",
        "GetActiveWindow",
        "GetForegroundWindow",
        "GetCapture",
    ] {
        assert_eq!(v.call("user32.dll", name, &[]), 0);
    }
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

#[test]
fn common_strings_arithmetic_com_apartments_and_guid_use_checked_state() {
    let mut v = Vm::new();
    v.wide(DATA, "AbC\0tail");
    v.wide(DATA + 64, "abc\0tail");
    assert_eq!(v.k("CompareStringOrdinal", &[DATA, 3, DATA + 64, 3, 1]), 2);
    assert_eq!(v.k("CompareStringOrdinal", &[DATA, 3, DATA + 64, 3, 0]), 1);
    assert_eq!(v.k("CompareStringOrdinal", &[DATA, 0, DATA + 64, 0, 0]), 2);
    assert_eq!(v.k("CompareStringOrdinal", &[0, 0, DATA, 0, 0]), 0);
    for (n, multiplier, divisor, expected) in [
        (5, 1, 2, 3),
        (-5, 1, 2, -3),
        (5, 1, -2, -3),
        (i32::MIN, 1, 1, i32::MIN),
        (i32::MAX, 2, 1, -1),
        (1, 1, 0, -1),
    ] {
        assert_eq!(
            v.k("MulDiv", &[n as u32, multiplier as u32, divisor as u32]),
            expected as u32
        );
    }
    assert_eq!(v.k("IsBadReadPtr", &[0, 0]), 0);
    assert_eq!(v.k("IsBadReadPtr", &[0, 1]), 1);
    assert_eq!(v.k("IsBadWritePtr", &[DATA, 4]), 0);
    assert_eq!(v.call("ole32.dll", "CoInitializeEx", &[0, 2]), 0);
    assert_eq!(v.call("ole32.dll", "CoInitializeEx", &[0, 2]), 1);
    assert_eq!(v.call("ole32.dll", "CoInitializeEx", &[0, 0]), 0x80010106);
    v.call("ole32.dll", "CoUninitialize", &[]);
    assert_eq!(v.call("ole32.dll", "CoInitializeEx", &[0, 0]), 0x80010106);
    v.call("ole32.dll", "CoUninitialize", &[]);
    assert_eq!(v.call("ole32.dll", "CoInitializeEx", &[0, 0]), 0);
    assert_eq!(v.call("ole32.dll", "CoCreateGuid", &[DATA + 128]), 0);
    assert_eq!(v.mem.read_u8(DATA + 135).unwrap() & 0xf0, 0x40);
    assert_eq!(v.mem.read_u8(DATA + 136).unwrap() & 0xc0, 0x80);
    assert_eq!(v.call("ole32.dll", "CoCreateGuid", &[0]), 0x80004003);
}

#[test]
fn bstr_preserves_embedded_nuls_reallocation_aliases_and_null_contracts() {
    let mut v = Vm::new();
    v.wide(DATA, "a\0日本");
    let p = v.call("oleaut32.dll", "SysAllocStringLen", &[DATA, 4]);
    assert_ne!(p, 0);
    assert_eq!(v.mem.read_u32(p - 4).unwrap(), 8);
    assert_eq!(v.call("oleaut32.dll", "SysStringLen", &[p]), 4);
    assert_eq!(v.call("oleaut32.dll", "SysStringByteLen", &[p]), 8);
    assert_eq!(v.mem.read_u16(p + 4).unwrap(), '日' as u16);
    v.mem.write_u32(DATA + 64, p).unwrap();
    assert_eq!(
        v.call("oleaut32.dll", "SysReAllocStringLen", &[DATA + 64, 0, 6]),
        1
    );
    let grown = v.mem.read_u32(DATA + 64).unwrap();
    assert_eq!(v.call("oleaut32.dll", "SysStringLen", &[grown]), 6);
    assert_eq!(v.mem.read_u16(grown + 4).unwrap(), '日' as u16);
    assert_eq!(v.mem.read_u16(grown + 12).unwrap(), 0);
    assert_eq!(
        v.call(
            "oleaut32.dll",
            "SysReAllocStringLen",
            &[DATA + 64, grown, 3]
        ),
        1
    );
    let shrunk = v.mem.read_u32(DATA + 64).unwrap();
    assert_eq!(v.mem.read_u16(shrunk + 4).unwrap(), '日' as u16);
    assert_eq!(v.mem.read_u16(shrunk + 6).unwrap(), 0);
    assert_eq!(
        v.call(
            "oleaut32.dll",
            "SysReAllocStringLen",
            &[DATA + 64, shrunk, u32::MAX]
        ),
        0
    );
    assert_eq!(v.mem.read_u32(DATA + 64).unwrap(), shrunk);
    assert_eq!(
        v.call("oleaut32.dll", "SysReAllocString", &[DATA + 64, 0]),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 64).unwrap(), 0);
    assert_eq!(v.call("oleaut32.dll", "SysAllocString", &[0]), 0);
    let empty = v.call("oleaut32.dll", "SysAllocStringLen", &[0, 0]);
    assert_ne!(empty, 0);
    assert_eq!(v.mem.read_u32(empty - 4).unwrap(), 0);
    v.call("oleaut32.dll", "SysFreeString", &[empty]);
    v.call("oleaut32.dll", "SysFreeString", &[0]);
}

#[test]
fn spec_ordinals_share_named_stubs_and_large_abis_read_the_full_checked_stack() {
    let mut v = Vm::new();
    let named = v.api.register("oleaut32.dll", Some("SysAllocString"), None);
    assert_eq!(v.api.register("oleaut32.dll", None, Some(2)), named);
    assert_eq!(
        v.api.entries[v.api.stub_index(named).unwrap()].name,
        "SysAllocString"
    );
    v.wide(DATA, "oleaut32.dll");
    let module = v.k("LoadLibraryW", &[DATA]);
    assert_eq!(v.k("GetProcAddress", &[module, 2]), named);
    assert_eq!(v.k("GetProcAddress", &[module, 65535]), 0);
    let addr = v.api.register("gdi32.dll", Some("CreateFontW"), None);
    let index = v.api.stub_index(addr).unwrap();
    // The last of 14 arguments straddles an uncommitted page: gateway must fault.
    v.cpu.eip = addr;
    v.cpu.set_esp(DATA + 0x10000 - 56);
    v.mem.write_u32(v.cpu.esp(), RET).unwrap();
    assert!(api::dispatch(
        index,
        &mut v.cpu,
        &mut v.mem,
        &mut v.api,
        &mut v.state,
        &mut v.log,
        &mut v.sched,
        1_000_000
    )
    .is_err());
    // With all arguments committed, the known stdcall ABI restores exactly 56 bytes.
    v.call("gdi32.dll", "CreateFontW", &[0; 14]);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 4 + 56);
}

struct Vm {
    mem: GuestMem,
    cpu: Cpu,
    api: ApiTable,
    state: ApiState,
    log: ApiLogger,
    sched: Sched,
    dir: PathBuf,
}
impl Vm {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "wire-win32-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        let mut mem = GuestMem::new().unwrap();
        for (p, n) in [
            (CODE, 0x10000),
            (DATA, 0x10000),
            (STACK_BASE, STACK_SIZE),
            (TEB_ADDR, 0x1000),
        ] {
            mem.commit(p, n).unwrap();
        }
        mem.write_u32(TEB_ADDR, u32::MAX).unwrap();
        mem.write_u32(TEB_ADDR + 4, STACK_BASE + STACK_SIZE)
            .unwrap();
        mem.write_u32(TEB_ADDR + 8, STACK_BASE).unwrap();
        let cpu = Cpu::new(CODE, STACK_BASE + STACK_SIZE - 256, TEB_ADDR);
        let api = ApiTable::new();
        let mut state = ApiState::new(0x400000, "C:\\test.exe".into(), 0, 0);
        state.fs = Some(GuestFs::new(&dir).unwrap());
        let log = ApiLogger::new(&dir.join("api.log")).unwrap();
        let mut sched = Sched::new();
        let tid = sched.alloc_tid();
        let handle = state.alloc_handle(HandleObj::Thread { tid });
        sched.add_main(tid, handle, TEB_ADDR);
        Self {
            mem,
            cpu,
            api,
            state,
            log,
            sched,
            dir,
        }
    }
    fn call(&mut self, dll: &str, name: &str, args: &[u32]) -> u32 {
        self.try_call(dll, name, args).unwrap()
    }
    fn try_call(&mut self, dll: &str, name: &str, args: &[u32]) -> Result<u32, String> {
        let addr = self.api.register(dll, Some(name), None);
        self.cpu.eip = addr;
        self.cpu.set_esp(STACK_BASE + STACK_SIZE - 256);
        let sp = self.cpu.esp();
        self.mem.write_u32(sp, RET).unwrap();
        for i in 0..16 {
            self.mem
                .write_u32(sp + 4 + i * 4, args.get(i as usize).copied().unwrap_or(0))
                .unwrap();
        }
        let idx = self.api.stub_index(addr).unwrap();
        api::dispatch(
            idx,
            &mut self.cpu,
            &mut self.mem,
            &mut self.api,
            &mut self.state,
            &mut self.log,
            &mut self.sched,
            1_000_000,
        )?;
        Ok(self.cpu.regs[0])
    }
    fn k(&mut self, name: &str, args: &[u32]) -> u32 {
        self.call("kernel32.dll", name, args)
    }
    fn wide(&self, p: u32, text: &str) {
        for (i, c) in text.encode_utf16().chain([0]).enumerate() {
            self.mem.write_u16(p + i as u32 * 2, c).unwrap();
        }
    }
}
impl Drop for Vm {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn menu_info(v: &Vm, fields: [u32; 12]) -> u32 {
    for (i, n) in fields.into_iter().enumerate() {
        v.mem.write_u32(DATA + 0x800 + i as u32 * 4, n).unwrap();
    }
    DATA + 0x800
}

#[test]
fn menus_copy_guest_strings_find_nested_commands_and_preserve_masked_fields() {
    let mut v = Vm::new();
    let root = v.call("user32.dll", "CreateMenu", &[]);
    let child = v.call("user32.dll", "CreatePopupMenu", &[]);
    assert_ne!(root, child);
    assert_ne!(child, 0);
    v.wide(DATA + 0x900, "日本語");
    let p = menu_info(
        &v,
        [44, 0x33, 0, 8, 77, 0, 0, 0, 0x1234, DATA + 0x900, 1, 0],
    );
    assert_eq!(
        v.call("user32.dll", "InsertMenuItemW", &[child, u32::MAX, 1, p]),
        1
    );
    v.wide(DATA + 0x900, "changed"); // menu owns a copy; cch is ignored on input
    let p = menu_info(&v, [48, 6, 0, 0, 77, child, 0, 0, 0, 0, 0, 0]);
    assert_eq!(v.call("user32.dll", "InsertMenuItemW", &[root, 0, 1, p]), 1);
    assert_eq!(v.call("user32.dll", "GetSubMenu", &[root, 0]), child);
    assert_eq!(v.call("user32.dll", "GetMenuItemID", &[root, 0]), u32::MAX);
    // Descendant command 77 takes priority over popup command 77.
    let p = menu_info(&v, [48, 0x63, 0, 0, 0, 0, 0, 0, 0, DATA + 0xa00, 3, 0]);
    assert_eq!(
        v.call("user32.dll", "GetMenuItemInfoW", &[root, 77, 0, p]),
        1
    );
    assert_eq!(v.mem.read_wstr(DATA + 0xa00, 10).unwrap(), "日本");
    assert_eq!(v.mem.read_u32(p + 40).unwrap(), 2);
    assert_eq!(v.mem.read_u32(p + 32).unwrap(), 0x1234);
    assert_eq!(v.mem.read_u32(p + 12).unwrap(), 8);
    let p = menu_info(&v, [48, 1, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(
        v.call("user32.dll", "SetMenuItemInfoW", &[root, 77, 0, p]),
        1
    );
    let p = menu_info(&v, [48, 0x62, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(
        v.call("user32.dll", "GetMenuItemInfoA", &[root, 77, 0, p]),
        1
    );
    assert_eq!(v.mem.read_u32(p + 40).unwrap(), 6); // CP932 bytes
    assert_eq!(v.mem.read_u32(p + 16).unwrap(), 77);
    assert_eq!(v.mem.read_u32(p + 32).unwrap(), 0x1234);
    assert_eq!(v.call("user32.dll", "EnableMenuItem", &[root, 77, 0]), 3);
    assert_eq!(v.call("user32.dll", "CheckMenuItem", &[root, 77, 8]), 0);
    assert_eq!(v.call("user32.dll", "GetMenuState", &[root, 77, 0]), 8);
    assert_eq!(v.call("user32.dll", "RemoveMenu", &[root, 0, 0x400]), 1);
    assert_eq!(v.call("user32.dll", "GetMenuItemCount", &[root]), 0);
    assert_eq!(v.call("user32.dll", "IsMenu", &[child]), 1); // Remove does not destroy
    let p = menu_info(&v, [48, 4, 0, 0, 0, child, 0, 0, 0, 0, 0, 0]);
    assert_eq!(v.call("user32.dll", "InsertMenuItemW", &[root, 0, 1, p]), 1);
    assert_eq!(v.call("user32.dll", "DeleteMenu", &[root, 0, 0x400]), 1);
    assert_eq!(v.call("user32.dll", "IsMenu", &[child]), 0);
    assert_eq!(v.call("user32.dll", "GetMenuItemCount", &[child]), u32::MAX);
    assert_eq!(v.k("GetLastError", &[]), 1401);
}

#[test]
fn menus_reject_invalid_structs_pointers_cycles_and_deep_graphs_without_partial_mutation() {
    let mut v = Vm::new();
    let root = v.call("user32.dll", "CreateMenu", &[]);
    for fields in [
        [40, 2, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0],
        [48, 0x50, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0],
        [48, 4, 0, 0, 1, root, 0, 0, 0, 0, 0, 0],
        [48, 0x40, 0, 0, 1, 0, 0, 0, 0, 0x30000000, 0, 0],
    ] {
        let p = menu_info(&v, fields);
        assert_eq!(v.call("user32.dll", "InsertMenuItemW", &[root, 0, 1, p]), 0);
        assert_eq!(v.k("GetLastError", &[]), 87);
        assert_eq!(v.call("user32.dll", "GetMenuItemCount", &[root]), 0);
    }
    let p = menu_info(&v, [48, 2, 0x800, 0, 42, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(v.call("user32.dll", "InsertMenuItemW", &[root, 0, 1, p]), 1);
    let p = menu_info(&v, [48, 4, 0, 0, 0, root, 0, 0, 0, 0, 0, 0]);
    assert_eq!(
        v.call("user32.dll", "SetMenuItemInfoW", &[root, 0, 1, p]),
        0
    );
    assert_eq!(v.call("user32.dll", "GetMenuItemID", &[root, 0]), 42);
    let p = menu_info(&v, [48, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    v.mem
        .virtual_protect(DATA, 0x10000, wire::mem::PAGE_READONLY)
        .unwrap();
    assert_eq!(
        v.call("user32.dll", "GetMenuItemInfoW", &[root, 0, 1, p]),
        0
    );
    assert_eq!(v.mem.read_u32(p + 16).unwrap(), 0);
    v.mem
        .virtual_protect(DATA, 0x10000, wire::mem::PAGE_READWRITE)
        .unwrap();
    let mut parent = root;
    for _ in 1..30 {
        let child = v.call("user32.dll", "CreatePopupMenu", &[]);
        let p = menu_info(&v, [48, 4, 0, 0, 0, child, 0, 0, 0, 0, 0, 0]);
        assert_eq!(
            v.call("user32.dll", "InsertMenuItemW", &[parent, u32::MAX, 1, p]),
            1
        );
        parent = child;
    }
    let child = v.call("user32.dll", "CreatePopupMenu", &[]);
    let p = menu_info(&v, [48, 4, 0, 0, 0, child, 0, 0, 0, 0, 0, 0]);
    assert_eq!(
        v.call("user32.dll", "InsertMenuItemW", &[parent, 0, 1, p]),
        0
    );
    assert_eq!(v.call("user32.dll", "GetMenuItemCount", &[parent]), 0);
    assert_eq!(v.call("user32.dll", "DestroyMenu", &[root]), 1);
    assert_eq!(v.call("user32.dll", "IsMenu", &[parent]), 0);
    assert_eq!(v.call("user32.dll", "IsMenu", &[child]), 1);
}

#[test]
fn window_menu_attachment_lifetime_and_coordinate_roundtrip_are_real() {
    let mut v = Vm::new();
    let def = v.api.register("user32.dll", Some("DefWindowProcW"), None);
    let hwnd = test_window(&mut v, def);
    let menu = v.call("user32.dll", "CreateMenu", &[]);
    assert_eq!(v.call("user32.dll", "SetMenu", &[hwnd, menu]), 1);
    assert_eq!(v.call("user32.dll", "GetMenu", &[hwnd]), menu);
    assert_eq!(v.call("user32.dll", "SetMenu", &[hwnd, 123]), 0);
    assert_eq!(v.call("user32.dll", "GetMenu", &[hwnd]), menu);
    v.mem.write_u32(DATA + 0xb00, (-10i32) as u32).unwrap();
    v.mem.write_u32(DATA + 0xb04, 20).unwrap();
    assert_eq!(
        v.call("user32.dll", "ClientToScreen", &[hwnd, DATA + 0xb00]),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 0xb00).unwrap() as i32, -7);
    assert_eq!(v.mem.read_u32(DATA + 0xb04).unwrap(), 43); // client begins below the menu
    assert_eq!(
        v.call("user32.dll", "ScreenToClient", &[hwnd, DATA + 0xb00]),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 0xb00).unwrap() as i32, -10);
    assert_eq!(v.mem.read_u32(DATA + 0xb04).unwrap(), 20);
    assert_eq!(
        v.call("user32.dll", "TrackPopupMenuEx", &[menu, 0, 0, 0, hwnd, 0]),
        0
    );
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 28); // 6 DWORD stdcall
    assert_eq!(v.k("GetLastError", &[]), 120);
    assert_eq!(v.call("user32.dll", "DestroyWindow", &[hwnd]), 1);
    assert_eq!(v.call("user32.dll", "IsMenu", &[menu]), 0);
}

#[test]
fn window_text_dialog_numbers_and_geometry_use_real_control_state() {
    let mut v = Vm::new();
    let def = v.api.register("user32.dll", Some("DefWindowProcW"), None);
    let parent = test_window(&mut v, def);
    let atom = v.state.user32.windows[&parent].class as u32;
    let ctrl = v.call(
        "user32.dll",
        "CreateWindowExW",
        &[
            0, atom, 0, 0x40000000, 10, 20, 100, 30, parent, 42, 0x400000, 0,
        ],
    );
    assert_ne!(ctrl, 0);
    assert_eq!(v.call("user32.dll", "GetDlgItem", &[parent, 42]), ctrl);
    assert_eq!(v.call("user32.dll", "GetDlgCtrlID", &[ctrl]), 42);
    assert_eq!(
        v.call(
            "user32.dll",
            "SetDlgItemInt",
            &[parent, 42, (-123i32) as u32, 1]
        ),
        1
    );
    assert_eq!(
        v.call(
            "user32.dll",
            "GetDlgItemInt",
            &[parent, 42, DATA + 0xc00, 1]
        ),
        (-123i32) as u32
    );
    assert_eq!(v.mem.read_u32(DATA + 0xc00).unwrap(), 1);
    assert_eq!(v.call("user32.dll", "GetWindowTextLengthW", &[ctrl]), 4);
    assert_eq!(
        v.call(
            "user32.dll",
            "GetDlgItemTextW",
            &[parent, 42, DATA + 0xc10, 3]
        ),
        2
    );
    assert_eq!(v.mem.read_wstr(DATA + 0xc10, 10).unwrap(), "-1");
    v.wide(DATA + 0xc30, "2147483648");
    assert_eq!(
        v.call("user32.dll", "SetWindowTextW", &[ctrl, DATA + 0xc30]),
        1
    );
    assert_eq!(
        v.call(
            "user32.dll",
            "GetDlgItemInt",
            &[parent, 42, DATA + 0xc00, 1]
        ),
        0
    );
    assert_eq!(v.mem.read_u32(DATA + 0xc00).unwrap(), 0);
    assert_eq!(
        v.call(
            "user32.dll",
            "GetDlgItemInt",
            &[parent, 42, DATA + 0xc00, 0]
        ),
        0x80000000
    );
    assert_eq!(
        v.call("user32.dll", "GetClassNameW", &[ctrl, DATA + 0xc30, 32]),
        14
    );
    assert_eq!(v.mem.read_wstr(DATA + 0xc30, 32).unwrap(), "CallbackWindow");
    assert_eq!(
        v.call("user32.dll", "MoveWindow", &[ctrl, 12, 22, 120, 40, 1]),
        1
    );
    v.mem.write_bytes(DATA + 0xc60, &[0; 8]).unwrap();
    assert_eq!(
        v.call("user32.dll", "MapWindowPoints", &[ctrl, 0, DATA + 0xc60, 1]),
        15 | (26 << 16)
    );
    assert_eq!(v.mem.read_u32(DATA + 0xc60).unwrap(), 15);
    assert_eq!(v.mem.read_u32(DATA + 0xc64).unwrap(), 26);
    assert_eq!(v.call("user32.dll", "IsWindowEnabled", &[ctrl]), 1);
    assert_eq!(v.call("user32.dll", "EnableWindow", &[ctrl, 0]), 0);
    assert_eq!(v.call("user32.dll", "IsWindowEnabled", &[ctrl]), 0);
    assert_eq!(v.call("user32.dll", "SetFocus", &[ctrl]), 0);
    assert_eq!(v.call("user32.dll", "GetFocus", &[]), ctrl);
    assert_eq!(v.call("user32.dll", "SetParent", &[parent, ctrl]), 0);
    assert_eq!(v.k("GetLastError", &[]), 87);
}

#[test]
fn gdi_color_probe_dc_bitmap_lifetimes_and_pixel_failures_are_real() {
    let mut v = Vm::new();
    let screen = v.call("user32.dll", "GetDC", &[0]);
    let dc = v.call("gdi32.dll", "CreateCompatibleDC", &[screen]);
    let bitmap = v.call("gdi32.dll", "CreateCompatibleBitmap", &[screen, 1, 1]);
    assert_ne!(screen, 0);
    assert_ne!(bitmap, 0);
    let stock = v.call("gdi32.dll", "SelectObject", &[dc, bitmap]);
    assert_ne!(stock, 0);
    for component in 0..3 {
        for value in 0..256 {
            let color = value << (component * 8);
            assert_eq!(v.call("gdi32.dll", "SetPixel", &[dc, 0, 0, color]), color);
            assert_eq!(v.call("gdi32.dll", "GetPixel", &[dc, 0, 0]), color);
        }
    }
    assert_eq!(v.call("gdi32.dll", "GetPixel", &[dc, 1, 0]), u32::MAX);
    assert_eq!(v.call("gdi32.dll", "DeleteObject", &[bitmap]), 0);
    assert_eq!(v.call("gdi32.dll", "SelectObject", &[dc, stock]), bitmap);
    assert_eq!(v.call("gdi32.dll", "DeleteObject", &[bitmap]), 1);
    assert_eq!(v.call("gdi32.dll", "GetObjectType", &[bitmap]), 0);
    assert_eq!(v.call("gdi32.dll", "DeleteDC", &[screen]), 0);
    assert_eq!(v.call("user32.dll", "ReleaseDC", &[0, dc]), 0);
    assert_eq!(v.call("gdi32.dll", "DeleteDC", &[dc]), 1);
    assert_eq!(v.call("user32.dll", "ReleaseDC", &[0, screen]), 1);
}

fn dib32(v: &mut Vm, dc: u32, width: u32, height: i32) -> (u32, u32) {
    let bytes: Vec<_> = [40, width, height as u32, 0x00200001, 0, 0, 0, 0, 0, 0]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    v.mem.write_bytes(DATA + 0xd00, &bytes).unwrap();
    let h = v.call(
        "gdi32.dll",
        "CreateDIBSection",
        &[dc, DATA + 0xd00, 0, DATA + 0xd40, 0, 0],
    );
    assert_ne!(h, 0);
    (h, v.mem.read_u32(DATA + 0xd40).unwrap())
}

#[test]
fn dib_top_down_bottom_up_direct_writes_checked_permissions_and_release() {
    let mut v = Vm::new();
    let dc = v.call("gdi32.dll", "CreateCompatibleDC", &[0]);
    for height in [-2, 2] {
        let (h, bits) = dib32(&mut v, dc, 2, height);
        let stock = v.call("gdi32.dll", "SelectObject", &[dc, h]);
        assert_eq!(
            v.mem.virtual_query(bits).unwrap().kind,
            wire::mem::MEM_MAPPED
        );
        let offset = if height < 0 { 0 } else { 8 };
        v.mem.write_u32(bits + offset, 0x7f123456).unwrap(); // CPU writes BGRA directly
        assert_eq!(v.call("gdi32.dll", "GetPixel", &[dc, 0, 0]), 0x563412);
        assert_eq!(
            v.call("gdi32.dll", "SetPixel", &[dc, 1, 0, 0x332211]),
            0x332211
        );
        assert_eq!(v.mem.read_u32(bits + offset + 4).unwrap(), 0x112233);
        assert_eq!(
            v.call("gdi32.dll", "GetObjectW", &[h, 84, DATA + 0xe00]),
            84
        );
        assert_eq!(v.mem.read_u32(DATA + 0xe14).unwrap(), bits);
        assert_eq!(v.mem.read_u32(DATA + 0xe20).unwrap() as i32, height);
        assert_eq!(v.k("VirtualFree", &[bits, 0, 0x8000]), 0);
        assert_eq!(
            v.k(
                "VirtualProtect",
                &[bits, 1, wire::mem::PAGE_READONLY, DATA + 0xe80]
            ),
            1
        );
        assert_eq!(v.call("gdi32.dll", "SetPixel", &[dc, 0, 0, 0xff]), u32::MAX);
        assert_eq!(v.mem.read_u32(bits + offset).unwrap(), 0x7f123456);
        assert_eq!(v.call("gdi32.dll", "SelectObject", &[dc, stock]), h);
        assert_eq!(v.call("gdi32.dll", "DeleteObject", &[h]), 1);
        assert!(v.mem.read_u32(bits).is_err());
    }
}

#[test]
fn bitblt_overlap_clipping_origin_and_invalid_rop_preserve_expected_pixels() {
    let mut v = Vm::new();
    let dc = v.call("gdi32.dll", "CreateCompatibleDC", &[0]);
    let (h, bits) = dib32(&mut v, dc, 4, -2);
    v.call("gdi32.dll", "SelectObject", &[dc, h]);
    for i in 0..8 {
        v.mem
            .write_u32(bits + i * 4, ((i + 1) << 16) | 0xff000000)
            .unwrap();
    }
    assert_eq!(
        v.call("gdi32.dll", "BitBlt", &[dc, 1, 0, 3, 1, dc, 0, 0, 0xcc0020]),
        1
    );
    let (_, _, pixels) = v.state.graphics.snapshot(&v.mem, dc).unwrap();
    assert_eq!(pixels, [1, 1, 2, 3, 5, 6, 7, 8]);
    assert_eq!(v.call("gdi32.dll", "SetWindowOrgEx", &[dc, 1, 0, 0]), 1);
    assert_eq!(
        v.call("gdi32.dll", "BitBlt", &[dc, 0, 1, 3, 1, dc, 1, 0, 0xcc0020]),
        1
    );
    let (_, _, pixels) = v.state.graphics.snapshot(&v.mem, dc).unwrap();
    assert_eq!(pixels, [1, 1, 2, 3, 1, 2, 7, 8]);
    assert_eq!(
        v.call("gdi32.dll", "BitBlt", &[dc, 1, 0, 1, 1, dc, 1, 1, 0xdead]),
        0
    );
    assert_eq!(v.state.graphics.snapshot(&v.mem, dc).unwrap().2, pixels);
}

/// Independently authored PE32: one section, a real stdcall DllMain, code/data
/// exports, a forwarder and an ordinal hole. Absolute image pointers require
/// HIGHLOW relocation; host test result addresses are deliberately external.
fn test_dll(import: &str, attach_ok: bool, value: u32) -> Vec<u8> {
    fn w16(bytes: &mut [u8], at: usize, value: u16) {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn w32(bytes: &mut [u8], at: usize, value: u32) {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn put(bytes: &mut [u8], rva: usize, data: &[u8]) {
        bytes[0x200 + rva - 0x1000..0x200 + rva - 0x1000 + data.len()].copy_from_slice(data);
    }
    let mut bytes = vec![0; 0x2200];
    bytes[..2].copy_from_slice(b"MZ");
    w32(&mut bytes, 0x3c, 0x80);
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    w16(&mut bytes, 0x84, 0x14c);
    w16(&mut bytes, 0x86, 1);
    w16(&mut bytes, 0x94, 0xe0);
    w16(&mut bytes, 0x96, 0x2102);
    let opt = 0x98;
    w16(&mut bytes, opt, 0x10b);
    w32(&mut bytes, opt + 16, 0x1000);
    w32(&mut bytes, opt + 28, 0x10000000);
    w32(&mut bytes, opt + 32, 0x1000);
    w32(&mut bytes, opt + 36, 0x200);
    w32(&mut bytes, opt + 56, 0x4000);
    w32(&mut bytes, opt + 60, 0x200);
    w32(&mut bytes, opt + 92, 16);
    w32(&mut bytes, opt + 96, 0x1500);
    w32(&mut bytes, opt + 100, 0x200);
    w32(&mut bytes, opt + 104, 0x1300);
    w32(&mut bytes, opt + 108, 40);
    w32(&mut bytes, opt + 136, 0x1700);
    bytes[0x178..0x180].copy_from_slice(b".test\0\0\0");
    w32(&mut bytes, 0x180, 0x3000);
    w32(&mut bytes, 0x184, 0x1000);
    w32(&mut bytes, 0x188, 0x2000);
    w32(&mut bytes, 0x18c, 0x200);
    w32(&mut bytes, 0x19c, 0xe0000020);
    let mut body = vec![
        0x53, 0x8b, 0x5c, 0x24, 8, 0x8b, 0x44, 0x24, 12, 0x85, 0xc0, 0x74, 0,
    ];
    body.extend([0xff, 5]);
    body.extend((DATA + 0x700).to_le_bytes());
    body.extend([0x89, 0x1d]);
    body.extend((DATA + 0x708).to_le_bytes());
    body.extend([0xff, 0x15]);
    let iat_reloc = body.len() as u16;
    body.extend(0x10001400u32.to_le_bytes());
    body.push(0xa3);
    body.extend((DATA + 0x70c).to_le_bytes());
    body.push(0xb8);
    body.extend(u32::from(attach_ok).to_le_bytes());
    body.extend([0x5b, 0xc2, 12, 0]);
    body[12] = (body.len() - 13) as u8;
    body.extend([0xff, 5]);
    body.extend((DATA + 0x704).to_le_bytes());
    body.extend([0xb8, 1, 0, 0, 0, 0x5b, 0xc2, 12, 0]);
    put(&mut bytes, 0x1000, &body);
    put(&mut bytes, 0x1080, &[0xa1, 0, 0x12, 0, 0x10, 0xc3]);
    put(&mut bytes, 0x1200, &value.to_le_bytes());
    put(&mut bytes, 0x1210, &0x10001200u32.to_le_bytes());
    let mut imports = [0; 40];
    w32(&mut imports, 0, 0x1380);
    w32(&mut imports, 12, 0x1340);
    w32(&mut imports, 16, 0x1400);
    put(&mut bytes, 0x1300, &imports);
    let (dll, function) = import.rsplit_once('!').unwrap();
    put(&mut bytes, 0x1340, format!("{dll}\0").as_bytes());
    put(&mut bytes, 0x1380, &0x13a0u32.to_le_bytes());
    put(&mut bytes, 0x13a2, format!("{function}\0").as_bytes());
    put(&mut bytes, 0x1400, &0x13a0u32.to_le_bytes());
    let mut table = [0; 40];
    for (offset, val) in [
        (16, 7),
        (20, 5),
        (24, 4),
        (28, 0x1530),
        (32, 0x1548),
        (36, 0x1560),
    ] {
        w32(&mut table, offset, val);
    }
    put(&mut bytes, 0x1500, &table);
    for (index, rva) in [0x1080u32, 0x1200, 0x1210, 0x15e0, 0]
        .into_iter()
        .enumerate()
    {
        put(&mut bytes, 0x1530 + index * 4, &rva.to_le_bytes());
    }
    let mut at = 0x1570;
    for (index, (name, ordinal)) in [("Alias", 3u16), ("Data", 1), ("GetValue", 0), ("Ptr", 2)]
        .into_iter()
        .enumerate()
    {
        put(&mut bytes, 0x1548 + index * 4, &(at as u32).to_le_bytes());
        put(&mut bytes, 0x1560 + index * 2, &ordinal.to_le_bytes());
        let text = format!("{name}\0");
        put(&mut bytes, at, text.as_bytes());
        at += text.len();
    }
    put(&mut bytes, 0x15e0, b"kernel32.GetCurrentProcessId\0");
    let mut relocations = vec![0; 8];
    w32(&mut relocations, 0, 0x1000);
    for offset in [iat_reloc, 0x81, 0x210, 0] {
        relocations.extend((if offset == 0 { 0 } else { 0x3000 | offset }).to_le_bytes());
    }
    let reloc_size = relocations.len() as u32;
    w32(&mut relocations, 4, reloc_size);
    put(&mut bytes, 0x1700, &relocations);
    w32(&mut bytes, opt + 140, reloc_size);
    bytes
}

fn dll_export(v: &mut Vm, module: u32, name: &str) -> u32 {
    v.mem
        .write_bytes(DATA + 0x100, format!("{name}\0").as_bytes())
        .unwrap();
    v.k("GetProcAddress", &[module, DATA + 0x100])
}

fn test_tls_dll(attach_ok: bool) -> Vec<u8> {
    let mut bytes = test_dll("kernel32.dll!GetCurrentProcessId", attach_ok, 0);
    let put = |bytes: &mut [u8], rva: usize, data: &[u8]| {
        let at = 0x200 + rva - 0x1000;
        bytes[at..at + data.len()].copy_from_slice(data);
    };
    bytes[0x140..0x144].copy_from_slice(&0x1800u32.to_le_bytes());
    bytes[0x144..0x148].copy_from_slice(&24u32.to_le_bytes());
    for (i, value) in [
        0x10001900u32,
        0x10001904,
        0x10001920,
        0x10001930,
        12,
        0x500000,
    ]
    .into_iter()
    .enumerate()
    {
        put(&mut bytes, 0x1800 + i * 4, &value.to_le_bytes());
    }
    put(&mut bytes, 0x1900, &37u32.to_le_bytes());
    let mut access = vec![0x64, 0x8b, 0x0d, 0x2c, 0, 0, 0, 0xa1];
    access.extend(0x10001920u32.to_le_bytes());
    access.extend([0x8b, 0x04, 0x81, 0x8b, 0x10]);
    let mut entry = access.clone();
    entry.extend([0x89, 0x15]);
    entry.extend((DATA + 0x720).to_le_bytes());
    entry.push(0xb8);
    entry.extend(u32::from(attach_ok).to_le_bytes());
    entry.extend([0xc2, 12, 0]);
    put(&mut bytes, 0x1000, &entry);
    access.extend([0xff, 0x00, 0x89, 0xd0, 0xc3]); // return previous TLS value, increment it
    put(&mut bytes, 0x1080, &access);
    let mut reloc = Vec::from(0x1000u32.to_le_bytes());
    reloc.extend(20u32.to_le_bytes());
    for offset in [0x3008u16, 0x3088, 0x3800, 0x3804, 0x3808, 0x380c] {
        reloc.extend(offset.to_le_bytes());
    }
    put(&mut bytes, 0x1700, &reloc);
    bytes[0x124..0x128].copy_from_slice(&(reloc.len() as u32).to_le_bytes());
    bytes
}

#[test]
fn main_static_tls_is_initialized_before_entry_and_reserves_its_loader_index() {
    let mut v = Vm::new();
    let mut bytes = test_tls_dll(true);
    bytes[0x96..0x98].copy_from_slice(&0x102u16.to_le_bytes());
    bytes[0xb4..0xb8].copy_from_slice(&0x400000u32.to_le_bytes());
    for rva in [0x1008usize, 0x1088, 0x1800, 0x1804, 0x1808, 0x180c] {
        let at = 0x200 + rva - 0x1000;
        let va = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        bytes[at..at + 4].copy_from_slice(&(va - 0x10000000 + 0x400000).to_le_bytes());
    }
    let path = v.dir.join("main-tls.exe");
    std::fs::write(&path, &bytes).unwrap();
    let mut main_mem = GuestMem::new().unwrap();
    let mut main_api = ApiTable::new();
    let loaded =
        wire::loader::load_image(&mut main_mem, &mut main_api, &path, "main-tls.exe").unwrap();
    main_mem.commit(CODE, 0x10000).unwrap();
    main_mem.commit(DATA, 0x10000).unwrap();
    v.mem = main_mem;
    v.api = main_api;
    v.state
        .modules
        .register_main(
            &v.mem,
            loaded.image_base,
            path.clone(),
            "C:\\main-tls.exe".into(),
        )
        .unwrap();
    v.state
        .modules
        .setup_tls_thread(
            &mut v.mem,
            &mut v.state.heap,
            v.state.process_heap,
            loaded.teb,
        )
        .unwrap();
    let index = v.mem.read_u32(0x401920).unwrap();
    assert_eq!(index, 0);
    let vector = v.mem.read_u32(loaded.teb + 0x2c).unwrap();
    assert_ne!(vector, 0);
    let block = v.mem.read_u32(vector).unwrap();
    assert_eq!(v.mem.read_u32(block).unwrap(), 37);
    for i in 4..16 {
        assert_eq!(v.mem.read_u8(block + i).unwrap(), 0);
    }
    let f = dll_export(&mut v, loaded.image_base, "GetValue");
    assert!(matches!(
        v.cpu
            .call_guest(
                &mut v.mem,
                &mut v.api,
                &mut v.state,
                &mut v.log,
                &mut v.sched,
                f,
                &[],
                1000
            )
            .unwrap(),
        wire::cpu::GuestCallOutcome::Returned(37)
    ));
    let thread = v.k("CreateThread", &[0, 0, CODE, 0, 4, 0]);
    let teb = v.sched.threads[v.sched.thread_index_by_handle(thread).unwrap()].teb;
    let other = v.mem.read_u32(v.mem.read_u32(teb + 0x2c).unwrap()).unwrap();
    assert_ne!(other, block);
    assert_eq!(v.mem.read_u32(other).unwrap(), 37);
    std::fs::write(v.dir.join("tls.dll"), test_tls_dll(true)).unwrap();
    v.mem.write_bytes(DATA, b"tls.dll\0").unwrap();
    let dll = v.k("LoadLibraryA", &[DATA]);
    assert_ne!(dll, 0);
    assert_eq!(v.mem.read_u32(dll + 0x1920).unwrap(), 1);
    assert_eq!(v.mem.read_u32(block).unwrap(), 38);
    v.k("FreeLibrary", &[dll]);
    assert_eq!(v.mem.read_u32(vector).unwrap(), block);
    // Invalid main callback targets are rejected before registration, never ignored.
    bytes[0xb30..0xb34].copy_from_slice(&0x404000u32.to_le_bytes());
    std::fs::write(&path, &bytes).unwrap();
    let mut mem = GuestMem::new().unwrap();
    let mut api = ApiTable::new();
    wire::loader::load_image(&mut mem, &mut api, &path, "main-tls.exe").unwrap();
    assert!(wire::modules::Modules::default()
        .register_main(&mem, 0x400000, path, "C:\\main-tls.exe".into())
        .unwrap_err()
        .contains("TLS"));
}

#[test]
fn wmi_local_objects_balance_references_and_fail_queries_without_null_dereferences() {
    let mut v = Vm::new();
    let clsid = [0x4590f811u32, 0x11d01d3a, 0xaa001f89, 0x242e4b00];
    let iid = [0xdc12a687u32, 0x11cf737f, 0xaa004d88, 0x242e4b00];
    for (p, g) in [(DATA, clsid), (DATA + 0x20, iid)] {
        v.mem
            .write_bytes(
                p,
                &g.into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>(),
            )
            .unwrap();
    }
    assert_eq!(
        v.call(
            "ole32.dll",
            "CoCreateInstance",
            &[DATA, 0, 1, DATA + 0x20, DATA + 0x100]
        ),
        0
    );
    let locator = v.mem.read_u32(DATA + 0x100).unwrap();
    assert_ne!(locator, 0);
    v.mem.write_bytes(DATA + 0x40, &[0; 16]).unwrap();
    assert_eq!(
        v.call(
            "wire.wmi",
            "IWbemLocator.QueryInterface",
            &[locator, DATA + 0x40, DATA + 0x104]
        ),
        0x80004002
    );
    assert_eq!(v.mem.read_u32(DATA + 0x104).unwrap(), 0);
    assert_eq!(
        v.call(
            "wire.wmi",
            "IWbemLocator.QueryInterface",
            &[locator, DATA + 0x20, DATA + 0x104]
        ),
        0
    );
    assert_eq!(v.call("wire.wmi", "IWbemLocator.Release", &[locator]), 1);
    v.wide(DATA + 0x200, "\\\\.\\ROOT\\CIMV2");
    assert_eq!(
        v.call(
            "wire.wmi",
            "IWbemLocator.ConnectServer",
            &[locator, DATA + 0x200, 0, 0, 0, 0, 0, 0, DATA + 0x108]
        ),
        0
    );
    let service = v.mem.read_u32(DATA + 0x108).unwrap();
    assert_ne!(service, 0);
    v.wide(DATA + 0x300, "WQL");
    v.wide(DATA + 0x400, "SELECT * FROM Win32_VideoController");
    v.mem.write_u32(DATA + 0x10c, 0xdeadbeef).unwrap();
    assert_eq!(
        v.call(
            "wire.wmi",
            "IWbemServices.ExecQuery",
            &[service, DATA + 0x300, DATA + 0x400, 0x30, 0, DATA + 0x10c]
        ),
        0x80041011
    );
    assert_eq!(v.mem.read_u32(DATA + 0x10c).unwrap(), 0);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 28);
    v.wide(DATA + 0x400, "SELECT * FROM Win32_OperatingSystem");
    assert_eq!(
        v.call(
            "wire.wmi",
            "IWbemServices.ExecQuery",
            &[service, DATA + 0x300, DATA + 0x400, 0x30, 0, DATA + 0x10c]
        ),
        0
    );
    let enumerator = v.mem.read_u32(DATA + 0x10c).unwrap();
    assert_eq!(
        v.call(
            "wire.wmi",
            "IEnumWbemClassObject.Next",
            &[enumerator, u32::MAX, 1, DATA + 0x110, DATA + 0x114]
        ),
        0
    );
    assert_eq!(v.mem.read_u32(DATA + 0x114).unwrap(), 1);
    let row = v.mem.read_u32(DATA + 0x110).unwrap();
    v.wide(DATA + 0x500, "Version");
    assert_eq!(
        v.call(
            "wire.wmi",
            "IWbemClassObject.Get",
            &[row, DATA + 0x500, 0, DATA + 0x600, DATA + 0x620, 0]
        ),
        0
    );
    assert_eq!(v.mem.read_u16(DATA + 0x600).unwrap(), 8);
    let bstr = v.mem.read_u32(DATA + 0x608).unwrap();
    assert_eq!(v.mem.read_wstr(bstr, 32).unwrap(), "6.1.7601");
    assert_eq!(v.call("oleaut32.dll", "VariantClear", &[DATA + 0x600]), 0);
    assert_eq!(
        v.call(
            "wire.wmi",
            "IEnumWbemClassObject.Next",
            &[enumerator, u32::MAX, 1, DATA + 0x110, DATA + 0x114]
        ),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 0x114).unwrap(), 0);
    assert_eq!(v.call("wire.wmi", "IWbemClassObject.Release", &[row]), 0);
    assert_eq!(
        v.call("wire.wmi", "IEnumWbemClassObject.Release", &[enumerator]),
        0
    );
    assert_eq!(v.call("wire.wmi", "IWbemServices.Release", &[service]), 0);
    assert_eq!(v.call("wire.wmi", "IWbemLocator.Release", &[locator]), 0);
    assert_eq!(
        v.k("HeapSize", &[v.state.process_heap, 0, locator]),
        u32::MAX
    );
}

#[test]
fn dll_static_tls_is_ready_for_dllmain_and_isolated_for_existing_and_new_threads() {
    let mut v = Vm::new();
    let existing = v.k("CreateThread", &[0, 0, CODE, 0, 4, 0]);
    let existing_teb = v.sched.threads[v.sched.thread_index_by_handle(existing).unwrap()].teb;
    std::fs::write(v.dir.join("tls.dll"), test_tls_dll(true)).unwrap();
    v.mem.write_bytes(DATA, b"tls.dll\0").unwrap();
    let module = v.k("LoadLibraryA", &[DATA]);
    assert_eq!(module, 0x50000000);
    assert_eq!(v.mem.read_u32(DATA + 0x720).unwrap(), 37); // read by actual guest DllMain
    let function = dll_export(&mut v, module, "GetValue");
    let call = |v: &mut Vm| {
        v.cpu
            .call_guest(
                &mut v.mem,
                &mut v.api,
                &mut v.state,
                &mut v.log,
                &mut v.sched,
                function,
                &[],
                1_000_000,
            )
            .unwrap()
    };
    assert!(matches!(
        call(&mut v),
        wire::cpu::GuestCallOutcome::Returned(37)
    ));
    assert!(matches!(
        call(&mut v),
        wire::cpu::GuestCallOutcome::Returned(38)
    ));
    let new = v.k("CreateThread", &[0, 0, CODE, 0, 4, 0]);
    let new_teb = v.sched.threads[v.sched.thread_index_by_handle(new).unwrap()].teb;
    let index = v.mem.read_u32(module + 0x1920).unwrap();
    let main_vector = v.mem.read_u32(TEB_ADDR + 0x2c).unwrap();
    let main_block = v.mem.read_u32(main_vector + index * 4).unwrap();
    assert_eq!(v.mem.read_u32(main_block).unwrap(), 39);
    for teb in [existing_teb, new_teb] {
        let vector = v.mem.read_u32(teb + 0x2c).unwrap();
        let block = v.mem.read_u32(vector + index * 4).unwrap();
        assert_ne!(block, main_block);
        assert_eq!(v.mem.read_u32(block).unwrap(), 37);
        let mut zero = [1; 12];
        v.mem.read_bytes(block + 4, &mut zero).unwrap();
        assert_eq!(zero, [0; 12]);
        let saved_teb = v.cpu.fs_base;
        v.cpu.fs_base = teb;
        assert!(matches!(
            call(&mut v),
            wire::cpu::GuestCallOutcome::Returned(37)
        ));
        v.cpu.fs_base = saved_teb;
    }
    assert_eq!(v.k("FreeLibrary", &[module]), 1);
    assert_eq!(
        v.k("HeapSize", &[v.state.process_heap, 0, main_block]),
        u32::MAX
    );
    for teb in [TEB_ADDR, existing_teb, new_teb] {
        let vector = v.mem.read_u32(teb + 0x2c).unwrap();
        assert_eq!(v.mem.read_u32(vector + index * 4).unwrap(), 0);
    }
    v.mem.write_bytes(DATA, b"tls.dll\0").unwrap();
    assert_eq!(v.k("LoadLibraryA", &[DATA]), module);
    assert_eq!(v.mem.read_u32(DATA + 0x720).unwrap(), 37); // fresh template after reload
}

#[test]
fn dll_tls_failure_rolls_back_and_invalid_callback_targets_are_rejected() {
    let mut v = Vm::new();
    std::fs::write(v.dir.join("tls.dll"), test_tls_dll(false)).unwrap();
    v.mem.write_bytes(DATA, b"tls.dll\0").unwrap();
    assert_eq!(v.k("LoadLibraryA", &[DATA]), 0);
    assert_eq!(v.k("GetLastError", &[]), 1114);
    assert!(!v.mem.is_committed(0x50000000));
    let vector = v.mem.read_u32(TEB_ADDR + 0x2c).unwrap();
    assert_eq!(v.mem.read_u32(vector).unwrap(), 0);
    let mut callback = test_tls_dll(true);
    callback[0xb30..0xb34].copy_from_slice(&0x10004000u32.to_le_bytes());
    std::fs::write(v.dir.join("tls.dll"), callback).unwrap();
    assert_eq!(v.k("LoadLibraryA", &[DATA]), 0);
    assert_eq!(v.k("GetLastError", &[]), 193);
    assert!(!v.mem.is_committed(0x50000000));
    let mut bad = test_tls_dll(true);
    bad[0xa08..0xa0c].copy_from_slice(&0x10004000u32.to_le_bytes());
    std::fs::write(v.dir.join("tls.dll"), bad).unwrap();
    assert_eq!(v.k("LoadLibraryA", &[DATA]), 0);
    assert_eq!(v.k("GetLastError", &[]), 193);
    assert!(!v.mem.is_committed(0x50000000));
}

#[test]
fn pe32_dep_policy_interprets_legacy_data_but_preserves_nx_noaccess_guard_and_ir_invalidation() {
    use wire::mem::{MEM_COMMIT, MEM_RESERVE, PAGE_GUARD, PAGE_NOACCESS, PAGE_READWRITE};
    let v = Vm::new();
    for nx in [false, true] {
        let mut exe = test_dll("kernel32.dll!GetCurrentProcessId", true, 0);
        exe[0x96..0x98].copy_from_slice(&0x102u16.to_le_bytes());
        exe[0xb4..0xb8].copy_from_slice(&0x400000u32.to_le_bytes());
        exe[0xde..0xe0].copy_from_slice(&(if nx { 0x100u16 } else { 0 }).to_le_bytes());
        let path = v.dir.join("dep.exe");
        std::fs::write(&path, exe).unwrap();
        let mut mem = GuestMem::new().unwrap();
        let mut api = ApiTable::new();
        wire::loader::load_image(&mut mem, &mut api, &path, "dep.exe").unwrap();
        let p = mem
            .virtual_alloc(0x31000000, 4096, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE)
            .unwrap();
        let code = [0xb8, 42, 0, 0, 0];
        mem.write_bytes(p, &code).unwrap();
        let mut cpu = Cpu::new(p, 0, 0);
        cpu.install_ir(wire::ir::Image::from_code(p, &code).unwrap(), &mem)
            .unwrap();
        if nx {
            assert!(cpu.step(&mut mem).is_err());
        } else {
            cpu.step(&mut mem).unwrap();
            assert_eq!(cpu.regs[0], 42);
            assert_eq!(cpu.ir_steps, 1);
            cpu.eip = p;
            mem.set_dep_enabled(true);
            assert!(cpu.step(&mut mem).is_err());
            mem.set_dep_enabled(false);
        }
        mem.virtual_protect(p, 4096, PAGE_NOACCESS).unwrap();
        cpu.eip = p;
        assert!(cpu.step(&mut mem).is_err());
        mem.virtual_protect(p, 4096, PAGE_READWRITE | PAGE_GUARD)
            .unwrap();
        cpu.eip = p;
        assert!(cpu.step(&mut mem).is_err());
        assert_eq!(mem.virtual_query(p).unwrap().protect, PAGE_READWRITE);
        cpu.eip = p;
        assert_eq!(cpu.step(&mut mem).is_ok(), !nx);
    }
}

#[test]
fn dll_last_page_file_padding_is_mapped_but_cannot_extend_into_another_page() {
    let mut v = Vm::new();
    let mut bytes = test_dll("kernel32.dll!GetCurrentProcessId", true, 9);
    // Declared virtual extent ends within the last page; aligned file data
    // continues through that page, as in the real MinGW codec extension.
    bytes[0xd0..0xd4].copy_from_slice(&0x2f10u32.to_le_bytes());
    bytes[0x180..0x184].copy_from_slice(&0x1f10u32.to_le_bytes());
    bytes[0x21ff] = 0x5a;
    std::fs::write(v.dir.join("tail.dll"), &bytes).unwrap();
    v.mem.write_bytes(DATA, b"tail.dll\0").unwrap();
    let h = v.k("LoadLibraryA", &[DATA]);
    assert_ne!(h, 0);
    assert_eq!(v.mem.read_u8(h + 0x2fff).unwrap(), 0x5a);
    assert_eq!(
        v.mem.virtual_query(h + 0x2fff).unwrap().protect,
        wire::mem::PAGE_EXECUTE_READWRITE
    );
    assert!(v.mem.read_u8(h + 0x3000).is_err());
    assert_eq!(v.k("FreeLibrary", &[h]), 1);
    bytes.resize(0x2400, 0);
    bytes[0x188..0x18c].copy_from_slice(&0x2200u32.to_le_bytes());
    std::fs::write(v.dir.join("tail.dll"), &bytes).unwrap();
    assert_eq!(v.k("LoadLibraryA", &[DATA]), 0);
    assert_eq!(v.k("GetLastError", &[]), 193);
    assert!(!v.mem.is_committed(h));
}

#[test]
fn tls_callbacks_run_before_dllmain_on_the_owner_and_before_storage_is_released() {
    let mut v = Vm::new();
    v.mem.write_u32(TEB_ADDR + 0x18, TEB_ADDR).unwrap();
    let mut bytes = test_tls_dll(true);
    // callback records reason, owner TEB and the still-live TLS value.
    let mut code = vec![0x8b, 0x44, 0x24, 8, 0xff, 0x04, 0x85];
    code.extend((DATA + 0x740).to_le_bytes());
    code.extend([0x64, 0x8b, 0x0d, 0x18, 0, 0, 0, 0x89, 0x0c, 0x85]);
    code.extend((DATA + 0x760).to_le_bytes());
    code.extend([
        0x8b, 0x4c, 0x24, 4, 0x8b, 0x89, 0x20, 0x19, 0, 0, 0x64, 0x8b, 0x15, 0x2c, 0, 0, 0, 0x8b,
        0x0c, 0x8a, 0x8b, 0x11, 0x89, 0x14, 0x85,
    ]);
    code.extend((DATA + 0x780).to_le_bytes());
    code.extend([0xc2, 12, 0]);
    bytes[0x2c0..0x2c0 + code.len()].copy_from_slice(&code);
    bytes[0xb30..0xb34].copy_from_slice(&0x100010c0u32.to_le_bytes());
    // The callback table target itself also needs HIGHLOW relocation.
    bytes[0x904..0x908].copy_from_slice(&24u32.to_le_bytes());
    bytes[0x914..0x918].copy_from_slice(&[0x30, 0x39, 0, 0]);
    bytes[0x124..0x128].copy_from_slice(&24u32.to_le_bytes());
    std::fs::write(v.dir.join("tls.dll"), &bytes).unwrap();
    v.mem.write_bytes(DATA, b"tls.dll\0").unwrap();
    let h = v.k("LoadLibraryA", &[DATA]);
    assert_ne!(h, 0);
    assert_eq!(v.mem.read_u32(DATA + 0x744).unwrap(), 1);
    assert_eq!(v.mem.read_u32(DATA + 0x764).unwrap(), TEB_ADDR);
    assert_eq!(v.mem.read_u32(DATA + 0x784).unwrap(), 37);
    assert_eq!(v.mem.read_u32(DATA + 0x720).unwrap(), 37);
    assert_eq!(v.k("DisableThreadLibraryCalls", &[h]), 0);
    assert_eq!(v.k("GetLastError", &[]), 126);
    // Suspended thread receives no callback until it actually runs.
    let exit = v.api.register("kernel32.dll", Some("ExitThread"), None);
    v.mem.write_bytes(CODE, &[0x6a, 7, 0xb8]).unwrap();
    v.mem.write_u32(CODE + 3, exit).unwrap();
    v.mem.write_bytes(CODE + 7, &[0xff, 0xd0]).unwrap();
    let worker = v.k("CreateThread", &[0, 0, CODE, 0, 4, 0]);
    let teb = v.sched.threads[v.sched.thread_index_by_handle(worker).unwrap()].teb;
    assert_eq!(v.mem.read_u32(DATA + 0x748).unwrap(), 0);
    v.k("ResumeThread", &[worker]);
    v.k("Sleep", &[0]);
    // Owner resumes after the worker exits; stop before it can dispatch a
    // synthetic API frame from Vm::call's return address.
    v.sched.threads[0].ctx.eip = RET;
    v.mem.write_bytes(RET, &[0xeb, 0xfe]).unwrap();
    let outcome = v.cpu.run(
        &mut v.mem,
        &mut v.api,
        &mut v.state,
        &mut v.log,
        &mut v.sched,
        10000,
    );
    assert!(matches!(outcome, wire::cpu::RunOutcome::InstructionLimit));
    assert_eq!(v.mem.read_u32(DATA + 0x748).unwrap(), 1);
    assert_eq!(v.mem.read_u32(DATA + 0x74c).unwrap(), 1);
    assert_eq!(v.mem.read_u32(DATA + 0x768).unwrap(), teb);
    assert_eq!(v.mem.read_u32(DATA + 0x76c).unwrap(), teb);
    assert_eq!(v.mem.read_u32(DATA + 0x78c).unwrap(), 37);
    let vector = v.mem.read_u32(teb + 0x2c).unwrap();
    assert_eq!(
        v.k("HeapSize", &[v.state.process_heap, 0, vector]),
        u32::MAX
    );
    assert_eq!(v.k("FreeLibrary", &[h]), 1);
    assert_eq!(v.mem.read_u32(DATA + 0x740).unwrap(), 1);
    assert_eq!(v.mem.read_u32(DATA + 0x780).unwrap(), 37);
    // A malformed callback list never reaches guest execution.
    bytes[0xb30..0xb34].copy_from_slice(&0x10001900u32.to_le_bytes());
    bytes[0x19c..0x1a0].copy_from_slice(&0xc0000040u32.to_le_bytes());
    std::fs::write(v.dir.join("tls.dll"), &bytes).unwrap();
    assert_eq!(v.k("LoadLibraryA", &[DATA]), 0);
    assert_eq!(v.k("GetLastError", &[]), 193);
}

#[test]
fn dll_mapping_relocation_exports_initialization_and_reference_counts() {
    let mut v = Vm::new();
    let heap = v.state.process_heap;
    let allocation = v.k("HeapAlloc", &[heap, 0, 64]);
    v.mem.write_u32(allocation, 0xdead1234).unwrap();
    std::fs::write(
        v.dir.join("demo.dll"),
        test_dll("kernel32.dll!GetCurrentProcessId", true, 0x12345678),
    )
    .unwrap();
    v.wide(DATA, "C:\\demo.dll");
    let handle = v.k("LoadLibraryW", &[DATA]);
    assert_eq!(handle, 0x50000000);
    assert_eq!(v.mem.read_u32(handle + 0x1210).unwrap(), handle + 0x1200);
    assert_eq!(v.mem.read_u32(allocation).unwrap(), 0xdead1234);
    assert_eq!(v.mem.read_u32(DATA + 0x700).unwrap(), 1);
    assert_eq!(v.mem.read_u32(DATA + 0x708).unwrap(), handle);
    assert_eq!(v.mem.read_u32(DATA + 0x70c).unwrap(), 4242);
    let function = dll_export(&mut v, handle, "GetValue");
    assert_eq!(function, handle + 0x1080);
    assert_eq!(v.k("GetProcAddress", &[handle, 7]), function);
    assert_eq!(dll_export(&mut v, handle, "Data"), handle + 0x1200);
    assert_eq!(dll_export(&mut v, handle, "getvalue"), 0);
    assert_eq!(dll_export(&mut v, handle, "GetPluginInfo"), 0);
    assert_eq!(v.k("GetLastError", &[]), 127);
    assert_eq!(v.k("GetProcAddress", &[handle, 11]), 0);
    assert_eq!(
        dll_export(&mut v, handle, "Alias"),
        v.api.lookup("kernel32.dll", "GetCurrentProcessId").unwrap()
    );
    assert!(matches!(
        v.cpu
            .call_guest(
                &mut v.mem,
                &mut v.api,
                &mut v.state,
                &mut v.log,
                &mut v.sched,
                function,
                &[],
                1_000_000
            )
            .unwrap(),
        wire::cpu::GuestCallOutcome::Returned(0x12345678)
    ));
    v.mem.write_bytes(DATA, b"DEMO.dll\0").unwrap();
    assert_eq!(v.k("LoadLibraryA", &[DATA]), handle);
    assert_eq!(v.mem.read_u32(DATA + 0x700).unwrap(), 1);
    assert_eq!(v.k("GetModuleHandleA", &[DATA]), handle);
    assert_eq!(v.k("GetModuleHandleExA", &[6, function, DATA + 0x300]), 1);
    assert_eq!(v.mem.read_u32(DATA + 0x300).unwrap(), handle);
    assert_eq!(v.k("GetModuleFileNameW", &[handle, DATA + 0x400, 260]), 11);
    assert_eq!(v.mem.read_wstr(DATA + 0x400, 260).unwrap(), "C:\\demo.dll");
    assert_eq!(v.k("GetModuleFileNameW", &[handle, DATA + 0x400, 4]), 4);
    assert_eq!(v.mem.read_wstr(DATA + 0x400, 4).unwrap(), "C:\\");
    assert_eq!(v.k("FreeLibrary", &[handle]), 1);
    assert!(v.mem.is_committed(handle));
    assert_eq!(v.k("FreeLibrary", &[handle]), 1);
    assert!(!v.mem.is_committed(handle));
    assert_eq!(v.mem.read_u32(DATA + 0x704).unwrap(), 1);
    assert_eq!(v.k("GetModuleHandleA", &[DATA]), 0);
    assert_eq!(v.k("GetProcAddress", &[handle, 7]), 0);
    assert_eq!(v.k("GetLastError", &[]), 6);
    assert_eq!(v.k("LoadLibraryA", &[DATA]), handle);
    assert_eq!(v.mem.read_u32(DATA + 0x700).unwrap(), 2);
    assert_eq!(v.mem.read_u32(handle + 0x1200).unwrap(), 0x12345678);
}

#[test]
fn dll_sidecar_ir_handles_relocation_unload_reload_and_stale_source_hashes() {
    let mut v = Vm::new();
    v.cpu.profile_opcodes = false;
    v.cpu
        .install_ir(wire::ir::Image::from_code(CODE, &[]).unwrap(), &v.mem)
        .unwrap();
    let bytes = test_dll("kernel32.dll!GetCurrentProcessId", true, 0x12345678);
    std::fs::write(v.dir.join("aot.dll"), &bytes).unwrap();
    let image = wire::ir::Image::from_pe(&bytes).unwrap();
    std::fs::write(v.dir.join("aot.dll.wir"), image.encode()).unwrap();
    v.wide(DATA, "aot.dll");
    for i in 0..2 {
        let before = v.cpu.ir_steps;
        let module = v.k("LoadLibraryW", &[DATA]);
        assert_eq!(module, 0x50000000);
        assert_eq!(v.mem.read_u32(DATA + 0x700).unwrap(), i + 1);
        assert_eq!(v.mem.read_u32(DATA + 0x70c).unwrap(), 4242);
        assert!(
            v.cpu.ir_steps > before,
            "DllMain actually executes module IR"
        );
        let function = dll_export(&mut v, module, "GetValue");
        assert!(matches!(
            v.cpu
                .call_guest(
                    &mut v.mem,
                    &mut v.api,
                    &mut v.state,
                    &mut v.log,
                    &mut v.sched,
                    function,
                    &[],
                    1_000_000
                )
                .unwrap(),
            wire::cpu::GuestCallOutcome::Returned(0x12345678)
        ));
        // The relocated absolute MOV cannot use the original operand.
        assert!(v.cpu.ir_fallbacks > 0);
        assert_eq!(v.k("FreeLibrary", &[module]), 1);
        assert!(!v.mem.is_committed(module));
    }
    // A matching path is insufficient: a changed source must run as x86.
    std::fs::write(
        v.dir.join("aot.dll"),
        test_dll("kernel32.dll!GetCurrentProcessId", true, 0xcafebabe),
    )
    .unwrap();
    let before = v.cpu.ir_steps;
    let module = v.k("LoadLibraryW", &[DATA]);
    assert_ne!(module, 0);
    let function = dll_export(&mut v, module, "GetValue");
    assert!(matches!(
        v.cpu
            .call_guest(
                &mut v.mem,
                &mut v.api,
                &mut v.state,
                &mut v.log,
                &mut v.sched,
                function,
                &[],
                1_000_000
            )
            .unwrap(),
        wire::cpu::GuestCallOutcome::Returned(0xcafebabe)
    ));
    assert_eq!(v.cpu.ir_steps, before);
    assert_eq!(v.k("FreeLibrary", &[module]), 1);
}

#[test]
fn dll_dependencies_initialize_before_parent_and_failure_rolls_back() {
    let mut v = Vm::new();
    std::fs::write(
        v.dir.join("child.dll"),
        test_dll("kernel32.dll!GetCurrentProcessId", true, 0xcafe),
    )
    .unwrap();
    std::fs::write(
        v.dir.join("parent.dll"),
        test_dll("child.dll!GetValue", true, 0xbeef),
    )
    .unwrap();
    v.mem.write_bytes(DATA, b"parent.dll\0").unwrap();
    let parent = v.k("LoadLibraryA", &[DATA]);
    assert_eq!(parent, 0x50000000);
    assert_eq!(v.mem.read_u32(DATA + 0x700).unwrap(), 2);
    assert_eq!(v.mem.read_u32(DATA + 0x70c).unwrap(), 0xcafe);
    assert_eq!(v.mem.read_u32(parent + 0x1400).unwrap(), 0x50011080);
    assert_eq!(v.k("FreeLibrary", &[parent]), 1);
    assert_eq!(v.mem.read_u32(DATA + 0x704).unwrap(), 2);
    assert!(!v.mem.is_committed(0x50010000));

    std::fs::write(
        v.dir.join("bad.dll"),
        test_dll("child.dll!GetValue", false, 0xbeef),
    )
    .unwrap();
    v.mem.write_bytes(DATA, b"bad.dll\0").unwrap();
    assert_eq!(v.k("LoadLibraryA", &[DATA]), 0);
    assert_eq!(v.k("GetLastError", &[]), 1114);
    assert_eq!(v.mem.read_u32(DATA + 0x700).unwrap(), 4);
    assert_eq!(v.mem.read_u32(DATA + 0x704).unwrap(), 4); // failed parent and successful child detached
    assert!(!v.mem.is_committed(0x50000000));
    assert!(!v.mem.is_committed(0x50010000));
    assert_eq!(v.k("GetModuleHandleA", &[DATA]), 0);
}

#[test]
fn dll_guest_forwarders_deduplicate_dependencies_and_failed_load_preserves_existing_modules() {
    let mut v = Vm::new();
    let child = test_dll("kernel32.dll!GetCurrentProcessId", true, 73);
    std::fs::write(v.dir.join("child.dll"), &child).unwrap();
    v.mem.write_bytes(DATA, b"child.dll\0").unwrap();
    let child_handle = v.k("LoadLibraryA", &[DATA]);
    std::fs::write(
        v.dir.join("bad.dll"),
        test_dll("child.dll!GetValue", false, 0),
    )
    .unwrap();
    v.mem.write_bytes(DATA, b"bad.dll\0").unwrap();
    assert_eq!(v.k("LoadLibraryA", &[DATA]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x704).unwrap(), 1); // failed parent detached, existing child kept
    assert!(v.mem.is_committed(child_handle));
    assert_eq!(v.k("FreeLibrary", &[child_handle]), 1); // failed transaction released its ref
    assert!(!v.mem.is_committed(child_handle));

    let alias = |forward: &str| {
        let mut bytes = test_dll("kernel32.dll!GetCurrentProcessId", true, 81);
        bytes[0x7e0..0x800].fill(0);
        bytes[0x7e0..0x7e0 + forward.len()].copy_from_slice(forward.as_bytes());
        bytes
    };
    std::fs::write(v.dir.join("alias.dll"), alias("child.#7")).unwrap();
    v.mem.write_bytes(DATA, b"alias.dll\0").unwrap();
    let parent = v.k("LoadLibraryA", &[DATA]);
    for _ in 0..3 {
        assert_eq!(dll_export(&mut v, parent, "Alias"), 0x50011080);
    }
    assert_eq!(v.k("FreeLibrary", &[parent]), 1);
    assert!(!v.mem.is_committed(0x50010000));
    std::fs::write(v.dir.join("a.dll"), alias("b.Alias")).unwrap();
    std::fs::write(v.dir.join("b.dll"), alias("a.Alias")).unwrap();
    v.mem.write_bytes(DATA, b"a.dll\0").unwrap();
    let a = v.k("LoadLibraryA", &[DATA]);
    assert_eq!(dll_export(&mut v, a, "Alias"), 0);
    assert_eq!(v.k("GetLastError", &[]), 127);
    assert!(!v.mem.is_committed(0x50010000));
    assert!(v.mem.is_committed(a));
    assert_eq!(v.k("FreeLibrary", &[a]), 1);
    assert!(!v.mem.is_committed(a));
}

#[test]
fn dll_qualified_module_names_and_native_stub_addresses_resolve_correctly() {
    let mut v = Vm::new();
    let bytes = test_dll("kernel32.dll!GetCurrentProcessId", true, 9);
    for dir in ["one", "two"] {
        std::fs::create_dir(v.dir.join(dir)).unwrap();
        std::fs::write(v.dir.join(dir).join("same.dll"), &bytes).unwrap();
    }
    v.wide(DATA, "C:\\one\\same.dll");
    let one = v.k("LoadLibraryW", &[DATA]);
    v.wide(DATA, "C:\\two\\same.dll");
    let two = v.k("LoadLibraryW", &[DATA]);
    assert_ne!(one, two);
    assert_eq!(v.k("GetModuleHandleW", &[DATA]), two);
    v.wide(DATA, "C:\\one\\same.dll");
    assert_eq!(v.k("GetModuleHandleW", &[DATA]), one);
    v.wide(DATA, "C:\\absent\\same.dll");
    assert_eq!(v.k("GetModuleHandleW", &[DATA]), 0);
    let stub = v.api.lookup("kernel32.dll", "GetCurrentProcessId").unwrap();
    assert_eq!(v.k("GetModuleHandleExW", &[6, stub, DATA + 0x300]), 1);
    assert_eq!(
        v.mem.read_u32(DATA + 0x300).unwrap(),
        v.state.module_handles["kernel32.dll"]
    );
}

#[test]
fn main_image_registers_real_exports_and_honors_pe_stack_reserve() {
    let mut v = Vm::new();
    let mut exe = test_dll("kernel32.dll!GetCurrentProcessId", true, 19);
    exe[0x96..0x98].copy_from_slice(&0x102u16.to_le_bytes());
    exe[0xb4..0xb8].copy_from_slice(&0x400000u32.to_le_bytes());
    exe[0xe0..0xe4].copy_from_slice(&0x100000u32.to_le_bytes());
    let path = v.dir.join("test.exe");
    std::fs::write(&path, &exe).unwrap();
    let mut mem = GuestMem::new().unwrap();
    let mut table = ApiTable::new();
    let loaded = wire::loader::load_image(&mut mem, &mut table, &path, "test.exe").unwrap();
    let top = STACK_BASE + STACK_SIZE;
    assert_eq!(mem.read_u32(TEB_ADDR + 4).unwrap(), top);
    assert_eq!(mem.read_u32(TEB_ADDR + 8).unwrap(), top - 0x100000);
    assert!(mem.is_committed(top - 0x100000));
    assert!(!mem.is_committed(top - 0x100000 - 0x10000));
    assert!(mem.read_u8(0x17f000).is_ok()); // actual CRT stack probe previously faulted here
    assert_eq!(
        mem.virtual_query(0x400000).unwrap().kind,
        wire::mem::MEM_IMAGE
    );
    assert_eq!(
        mem.virtual_query(0x400000).unwrap().protect,
        wire::mem::PAGE_READONLY
    );
    assert_eq!(
        mem.virtual_query(0x401000).unwrap().protect,
        wire::mem::PAGE_EXECUTE_READWRITE
    );
    assert_eq!(
        mem.virtual_query(top - 4096).unwrap().protect,
        wire::mem::PAGE_READWRITE
    );
    assert!(mem.write_u32(0x400000, 0).is_err());
    v.mem = mem;
    v.api = table;
    v.mem.commit(DATA, 0x10000).unwrap();
    v.state
        .modules
        .register_main(
            &v.mem,
            loaded.image_base,
            path.clone(),
            "C:\\test.exe".into(),
        )
        .unwrap();
    assert_eq!(v.k("GetModuleHandleA", &[0]), 0x400000);
    assert_eq!(dll_export(&mut v, 0x400000, "GetValue"), 0x401080);
    assert_eq!(dll_export(&mut v, 0x400000, "Data"), 0x401200);
    assert_eq!(v.k("FreeLibrary", &[0x400000]), 1);
    assert!(v.mem.is_committed(0x400000));

    exe[0xe0..0xe4].copy_from_slice(&0x200000u32.to_le_bytes());
    std::fs::write(&path, &exe).unwrap();
    let mut mem = GuestMem::new().unwrap();
    assert!(
        wire::loader::load_image(&mut mem, &mut v.api, &path, "test.exe")
            .err()
            .unwrap()
            .contains("stack reserve")
    );
}

#[test]
fn dll_missing_imports_and_malformed_images_fail_without_fake_modules() {
    let mut v = Vm::new();
    let valid = test_dll("kernel32.dll!GetCurrentProcessId", true, 123);
    let mut cases = vec![
        (test_dll("missing.dll!Absent", true, 0), 126),
        (test_dll("kernel32.dll!MadeUpExport", true, 0), 127),
    ];
    let mut no_reloc = valid.clone();
    no_reloc[0x120..0x128].fill(0);
    no_reloc[0x96..0x98].copy_from_slice(&0x2103u16.to_le_bytes()); // fixed image, relocations stripped
    cases.push((no_reloc, 193));
    let mut wrong_reloc = valid.clone();
    wrong_reloc[0x908..0x90a].copy_from_slice(&0x7001u16.to_le_bytes());
    cases.push((wrong_reloc, 193));
    let mut bad_export = valid.clone();
    bad_export[0x740..0x744].copy_from_slice(&0x4000u32.to_le_bytes());
    cases.push((bad_export, 193));
    let mut bad_section = valid.clone();
    bad_section[0x188..0x18c].copy_from_slice(&0x4000u32.to_le_bytes());
    cases.push((bad_section, 193));
    for (index, (bytes, error)) in cases.into_iter().enumerate() {
        let name = format!("bad{index}.dll");
        std::fs::write(v.dir.join(&name), bytes).unwrap();
        v.mem
            .write_bytes(DATA, format!("{name}\0").as_bytes())
            .unwrap();
        assert_eq!(v.k("LoadLibraryA", &[DATA]), 0, "{name}");
        assert_eq!(v.k("GetLastError", &[]), error, "{name}");
        assert_eq!(v.k("GetModuleHandleA", &[DATA]), 0);
        assert!(!v.mem.is_committed(0x50000000));
        assert_eq!(v.mem.read_u32(DATA + 0x700).unwrap(), 0);
    }
    assert_eq!(v.k("LoadLibraryExA", &[DATA, 0, 2]), 0);
    assert_eq!(v.k("GetLastError", &[]), 87);
}

#[test]
fn heap_reuses_coalesces_and_preserves_realloc() {
    let mut v = Vm::new();
    let h = v.state.process_heap;
    let a = v.k("HeapAlloc", &[h, 0, 23]);
    let b = v.k("HeapAlloc", &[h, 0, 40]);
    v.mem.write_bytes(a, b"preserved").unwrap();
    let c = v.k("HeapReAlloc", &[h, 8, a, 100]);
    assert_ne!(c, a);
    let mut bytes = [0xff; 100];
    v.mem.read_bytes(c, &mut bytes).unwrap();
    assert_eq!(&bytes[..9], b"preserved");
    assert!(bytes[23..].iter().all(|&b| b == 0));
    assert_eq!(v.k("HeapSize", &[h, 0, c]), 100);
    assert_eq!(v.k("HeapFree", &[h, 0, b]), 1);
    assert_eq!(v.k("HeapFree", &[h, 0, c]), 1);
    let big = v.k("HeapAlloc", &[h, 8, 180]);
    assert_eq!(big, a);
    assert_eq!(v.k("HeapFree", &[h, 0, big]), 1);
    assert_eq!(v.k("HeapFree", &[h, 0, big]), 0);
    assert_eq!(v.k("HeapAlloc", &[h, 0, u32::MAX]), 0);
}

#[test]
fn heap_in_place_only_and_private_heap_ownership() {
    let mut v = Vm::new();
    let h = v.k("HeapCreate", &[0, 0, 128]);
    let a = v.k("HeapAlloc", &[h, 0, 32]);
    let b = v.k("HeapAlloc", &[h, 0, 32]);
    v.mem.write_u32(a, 0xfeed).unwrap();
    assert_eq!(v.k("HeapReAlloc", &[h, 0x10, a, 64]), 0);
    assert_eq!(v.mem.read_u32(a).unwrap(), 0xfeed);
    assert_eq!(v.k("HeapFree", &[v.state.process_heap, 0, a]), 0);
    assert_eq!(v.k("HeapFree", &[h, 0, b]), 1);
    assert_eq!(v.k("HeapReAlloc", &[h, 8, a, 64]), a);
    assert_eq!(v.k("HeapAlloc", &[h, 0, 80]), 0);
    assert_eq!(v.k("HeapDestroy", &[h]), 1);
    assert_eq!(v.k("HeapValidate", &[h, 0, 0]), 0);
}

#[test]
fn cp932_conversion_lengths_terminators_and_errors() {
    let mut v = Vm::new();
    v.mem
        .write_bytes(DATA, &[0x93, 0xfa, 0x96, 0x7b, 0])
        .unwrap(); // 日本
    assert_eq!(
        v.k("MultiByteToWideChar", &[932, 8, DATA, u32::MAX, 0, 0]),
        3
    );
    assert_eq!(
        v.k(
            "MultiByteToWideChar",
            &[932, 8, DATA, u32::MAX, DATA + 64, 3]
        ),
        3
    );
    assert_eq!(v.mem.read_wstr(DATA + 64, 3).unwrap(), "日本");
    assert_eq!(
        v.k(
            "WideCharToMultiByte",
            &[932, 0, DATA + 64, u32::MAX, DATA + 128, 5, 0, DATA + 192]
        ),
        5
    );
    let mut bytes = [0; 5];
    v.mem.read_bytes(DATA + 128, &mut bytes).unwrap();
    assert_eq!(bytes, [0x93, 0xfa, 0x96, 0x7b, 0]);
    assert_eq!(v.mem.read_u32(DATA + 192).unwrap(), 0);
    assert_eq!(
        v.k(
            "MultiByteToWideChar",
            &[932, 8, DATA, u32::MAX, DATA + 64, 2]
        ),
        0
    );
    assert_eq!(v.k("GetLastError", &[]), 122);
    v.mem.write_bytes(DATA, &[0x81, 0]).unwrap();
    assert_eq!(
        v.k("MultiByteToWideChar", &[932, 8, DATA, 1, DATA + 64, 2]),
        0
    );
    assert_eq!(v.k("GetLastError", &[]), 1113);
    assert_eq!(
        v.k("MultiByteToWideChar", &[932, 0, DATA, 1, DATA + 64, 2]),
        1
    );
    assert_eq!(v.k("GetCPInfo", &[932, DATA]), 1);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 2);
    assert_eq!(v.mem.read_u32(DATA + 6).unwrap(), 0xfce09f81);
}

#[test]
fn files_read_write_seek_eof_and_invalid_handles() {
    let mut v = Vm::new();
    v.wide(DATA, "C:\\日本.txt");
    let h = v.k("CreateFileW", &[DATA, 0xc0000000, 0, 0, 2, 0x80, 0]);
    assert_ne!(h, u32::MAX);
    v.mem.write_bytes(DATA + 256, b"abc123").unwrap();
    assert_eq!(v.k("WriteFile", &[h, DATA + 256, 6, DATA + 512, 0]), 1);
    assert_eq!(v.mem.read_u32(DATA + 512).unwrap(), 6);
    assert_eq!(v.k("GetFileType", &[h]), 1);
    assert_eq!(v.k("GetFileSizeEx", &[h, DATA + 520]), 1);
    assert_eq!(v.mem.read_u64(DATA + 520).unwrap(), 6);
    assert_eq!(v.k("SetFilePointer", &[h, 3, 0, 0]), 3);
    assert_eq!(v.k("ReadFile", &[h, DATA + 300, 10, DATA + 512, 0]), 1);
    assert_eq!(v.mem.read_u32(DATA + 512).unwrap(), 3);
    let mut b = [0; 3];
    v.mem.read_bytes(DATA + 300, &mut b).unwrap();
    assert_eq!(&b, b"123");
    assert_eq!(v.k("ReadFile", &[h, DATA + 300, 10, DATA + 512, 0]), 1);
    assert_eq!(v.mem.read_u32(DATA + 512).unwrap(), 0);
    assert_eq!(v.k("SetFilePointerEx", &[h, 7, 1, DATA + 520, 0]), 1);
    assert_eq!(v.mem.read_u64(DATA + 520).unwrap(), 0x100000007);
    assert_eq!(v.k("SetFilePointerEx", &[h, u32::MAX, u32::MAX, 0, 0]), 0);
    assert_eq!(v.k("CloseHandle", &[h]), 1);
    assert_eq!(v.k("ReadFile", &[h, DATA + 300, 1, DATA + 512, 0]), 0);
    assert_eq!(v.k("GetLastError", &[]), 6);
    assert_eq!(std::fs::read(v.dir.join("日本.txt")).unwrap(), b"abc123");
}

#[test]
fn filesystem_confines_paths_and_symlinks_and_handles_case() {
    let mut v = Vm::new();
    std::fs::create_dir(v.dir.join("Sub")).unwrap();
    std::fs::write(v.dir.join("Sub/Test.TXT"), b"ok").unwrap();
    let fs = v.state.fs.as_mut().unwrap();
    assert_eq!(
        fs.resolve("c:\\sub\\test.txt").unwrap(),
        v.dir.join("Sub/Test.TXT")
    );
    assert!(fs.resolve("../outside").is_err());
    assert!(fs.resolve("C:\\..\\outside").is_err());
    assert!(fs.resolve("\\\\server\\file").is_err());
    fs.set_cwd("Sub").unwrap();
    assert_eq!(fs.cwd(), "C:\\Sub");
    assert_eq!(
        fs.resolve("../Sub/test.txt").unwrap(),
        v.dir.join("Sub/Test.TXT")
    );
    std::os::unix::fs::symlink(std::env::temp_dir(), v.dir.join("escape")).unwrap();
    assert!(fs.resolve("C:\\escape\\new-file").is_err());
    assert!(fs.resolve("C:\\escape").is_err());
}

#[test]
fn directory_queries_and_enumeration() {
    let mut v = Vm::new();
    std::fs::write(v.dir.join("one.cf"), b"cfg").unwrap();
    std::fs::write(v.dir.join("two.cf"), b"cfg2").unwrap();
    v.wide(DATA, "C:\\*.cf");
    let h = v.k("FindFirstFileW", &[DATA, DATA + 256]);
    assert_ne!(h, u32::MAX);
    assert_eq!(v.mem.read_wstr(DATA + 256 + 44, 260).unwrap(), "one.cf");
    assert_eq!(v.k("FindNextFileW", &[h, DATA + 256]), 1);
    assert_eq!(v.mem.read_wstr(DATA + 256 + 44, 260).unwrap(), "two.cf");
    assert_eq!(v.k("FindNextFileW", &[h, DATA + 256]), 0);
    assert_eq!(v.k("GetLastError", &[]), 18);
    assert_eq!(v.k("FindClose", &[h]), 1);
    assert_eq!(v.k("GetCurrentDirectoryW", &[2, DATA + 256]), 4);
    assert_eq!(v.k("GetCurrentDirectoryW", &[4, DATA + 256]), 3);
    assert_eq!(v.mem.read_wstr(DATA + 256, 4).unwrap(), "C:\\");
    let h = v.k("FindFirstFileExW", &[DATA, 1, DATA + 256, 0, 0, 2]);
    assert_ne!(h, u32::MAX);
    assert_eq!(v.mem.read_wstr(DATA + 256 + 44, 260).unwrap(), "one.cf");
    assert_eq!(v.k("FindClose", &[h]), 1);
    assert_eq!(
        v.k("FindFirstFileExW", &[DATA, 2, DATA + 256, 0, 0, 0]),
        u32::MAX
    );
    assert_eq!(v.k("GetLastError", &[]), 87);
}

#[test]
fn x86_slist_init_and_flush_preserve_sequence_and_return_chain() {
    let mut v = Vm::new();
    v.mem.write_u64(DATA, u64::MAX).unwrap();
    v.k("InitializeSListHead", &[DATA]);
    assert_eq!(v.mem.read_u64(DATA).unwrap(), 0);
    v.mem
        .write_u64(DATA, (0xffffu64 << 48) | (2u64 << 32) | (DATA + 16) as u64)
        .unwrap();
    assert_eq!(v.k("InterlockedFlushSList", &[DATA]), DATA + 16);
    assert_eq!(v.mem.read_u64(DATA).unwrap(), 0); // sequence wrapped, depth/pointer cleared
    assert_eq!(v.k("InterlockedFlushSList", &[DATA]), 0);
    assert_eq!(v.mem.read_u64(DATA).unwrap(), 1u64 << 48);
}

#[test]
fn zero_delay_yields_finish_the_callers_frame_before_switching_threads() {
    for (name, args, cleanup, result) in [
        ("Sleep", vec![0], 8, 0),
        ("SleepEx", vec![0, 0], 12, 0),
        ("SwitchToThread", vec![], 4, 1),
    ] {
        let mut v = Vm::new();
        v.k("CreateThread", &[0, 0, CODE + 16, 0x98765432, 0, 0]);
        v.k("SetLastError", &[123]);
        let worker = v.sched.threads[1].ctx;
        let worker_teb = v.sched.threads[1].teb;
        v.mem.write_u32(worker_teb + 0x34, 456).unwrap();
        v.k(name, &args);
        assert_eq!(v.sched.current, 1, "{name}");
        assert_eq!(v.cpu.eip, worker.eip);
        assert_eq!(v.cpu.regs, worker.regs);
        assert_eq!(v.cpu.fs_base, worker_teb);
        assert_eq!(v.mem.read_u32(worker_teb + 0x34).unwrap(), 456);
        assert!(v.sched.round_robin(&mut v.cpu));
        assert_eq!(v.cpu.eip, RET, "{name} return PC");
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + cleanup);
        assert_eq!(v.cpu.regs[0], result);
        assert_eq!(v.cpu.fs_base, TEB_ADDR);
        assert_eq!(v.mem.read_u32(TEB_ADDR + 0x34).unwrap(), 123);
        // A single runnable thread still returns normally without switching.
        v.sched.threads[1].state = wire::thread::ThreadState::Suspended;
        assert_eq!(v.k(name, &args), 0);
        assert_eq!(v.sched.current, 0);
        assert_eq!(v.cpu.eip, RET);
    }
}

#[test]
fn thread_local_values_and_last_error_are_independent() {
    let mut v = Vm::new();
    let slot = v.k("TlsAlloc", &[]);
    let fls = v.k("FlsAlloc", &[0]);
    v.k("TlsSetValue", &[slot, 123]);
    v.k("FlsSetValue", &[fls, 456]);
    v.k("SetLastError", &[99]);
    let h = v.k("CreateThread", &[0, 0, CODE, 0, 0, 0]);
    assert_ne!(h, 0);
    assert!(v.sched.round_robin(&mut v.cpu));
    assert_eq!(v.k("GetLastError", &[]), 0);
    assert_eq!(v.k("TlsGetValue", &[slot]), 0);
    assert_eq!(v.k("FlsGetValue", &[fls]), 0);
    v.k("TlsSetValue", &[slot, 789]);
    v.k("SetLastError", &[88]);
    assert!(v.sched.round_robin(&mut v.cpu));
    assert_eq!(v.k("GetLastError", &[]), 99);
    assert_eq!(v.k("TlsGetValue", &[slot]), 123);
    assert_eq!(v.k("FlsGetValue", &[fls]), 456);
}

#[test]
fn interlocked_compare_exchange_uses_comparand_then_exchange() {
    let mut v = Vm::new();
    v.mem.write_u32(DATA, 17).unwrap();
    assert_eq!(v.k("InterlockedCompareExchange", &[DATA, 42, 17]), 17);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 42);
    assert_eq!(v.k("InterlockedCompareExchange", &[DATA, 10, 17]), 42);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 42);
}

#[test]
fn software_seh_continues_with_modified_context_and_rejects_noncontinuable() {
    let mut v = Vm::new();
    // mov eax,[esp+12]; mov [eax+176],0x12345678; xor eax,eax; ret
    v.mem
        .write_bytes(
            CODE,
            &[
                0x8b, 0x44, 0x24, 0x0c, 0xc7, 0x80, 0xb0, 0, 0, 0, 0x78, 0x56, 0x34, 0x12, 0x31,
                0xc0, 0xc3,
            ],
        )
        .unwrap();
    v.mem.write_u32(DATA, u32::MAX).unwrap();
    v.mem.write_u32(DATA + 4, CODE).unwrap();
    v.mem.write_u32(TEB_ADDR, DATA).unwrap();
    assert_eq!(v.k("RaiseException", &[0x1234, 0, 0, 0]), 0x12345678);
    assert_eq!(v.cpu.eip, RET);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 236);
    let error = v
        .try_call("kernel32.dll", "RaiseException", &[0x1234, 1, 0, 0])
        .unwrap_err();
    assert!(error.contains("noncontinuable"));
}

#[test]
fn unhandled_filter_is_registered_invoked_and_prior_filter_returned() {
    let mut v = Vm::new();
    // Filter returns EXCEPTION_CONTINUE_EXECUTION (-1), stdcall one argument.
    v.mem
        .write_bytes(CODE, &[0xb8, 0xff, 0xff, 0xff, 0xff, 0xc2, 4, 0])
        .unwrap();
    assert_eq!(v.k("SetUnhandledExceptionFilter", &[CODE]), 0);
    v.k("RaiseException", &[0x1234, 0, 0, 0]);
    assert_eq!(v.cpu.eip, RET);
    assert_eq!(v.k("SetUnhandledExceptionFilter", &[0]), CODE);
    let error = v
        .try_call("kernel32.dll", "RaiseException", &[0xe06d7363, 1, 0, 0])
        .unwrap_err();
    assert!(error.contains("unhandled guest exception"));
}

#[test]
fn seh_unwind_cleans_inner_frames_keeps_target_and_restores_context() {
    let mut v = Vm::new();
    let first = STACK_BASE + STACK_SIZE - 128;
    let target = first + 32;
    // Each handler records its own frame, current FS head, flags and code.
    for (i, frame) in [first, first + 16].into_iter().enumerate() {
        let handler = CODE + i as u32 * 64;
        let out = DATA + i as u32 * 16;
        let mut code = vec![0x8b, 0x54, 0x24, 8, 0x89, 0x15];
        code.extend(out.to_le_bytes());
        code.extend([0x64, 0xa1, 0, 0, 0, 0, 0xa3]);
        code.extend((out + 4).to_le_bytes());
        code.extend([0x8b, 0x54, 0x24, 4, 0x8b, 0x42, 4, 0xa3]);
        code.extend((out + 8).to_le_bytes());
        code.extend([0x8b, 0x02, 0xa3]);
        code.extend((out + 12).to_le_bytes());
        code.extend([0xb8, 1, 0, 0, 0, 0xc3]);
        v.mem.write_bytes(handler, &code).unwrap();
        v.mem.write_u32(frame, frame + 16).unwrap();
        v.mem.write_u32(frame + 4, handler).unwrap();
    }
    v.mem.write_u32(target, u32::MAX).unwrap();
    v.mem.write_u32(target + 4, CODE + 0x500).unwrap();
    v.mem.write_u8(CODE + 0x500, 0xcc).unwrap(); // target must never be invoked
    v.mem.write_u32(TEB_ADDR, first).unwrap();
    v.mem.write_u32(DATA + 64, 0x1234).unwrap();
    v.mem.write_u32(DATA + 68, 1).unwrap();
    v.cpu.regs[3] = 0xfeed_beef;
    assert_eq!(
        v.k("RtlUnwind", &[target, CODE + 0x500, DATA + 64, 0xface]),
        0xface
    );
    assert_eq!(v.cpu.regs[3], 0xfeed_beef);
    assert_eq!(v.cpu.eip, RET); // x86 TargetIp is ignored
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 236);
    assert_eq!(v.mem.read_u32(TEB_ADDR).unwrap(), target);
    for (i, frame) in [first, first + 16].into_iter().enumerate() {
        let out = DATA + i as u32 * 16;
        assert_eq!(v.mem.read_u32(out).unwrap(), frame);
        assert_eq!(v.mem.read_u32(out + 4).unwrap(), frame);
        assert_eq!(v.mem.read_u32(out + 8).unwrap(), 3);
        assert_eq!(v.mem.read_u32(out + 12).unwrap(), 0x1234);
    }
    // Null record/target generates STATUS_UNWIND and marks exit unwind.
    v.mem.write_u32(TEB_ADDR, first).unwrap();
    v.mem.write_u32(first, u32::MAX).unwrap();
    v.k("RtlUnwind", &[0, 0, 0, 7]);
    assert_eq!(v.mem.read_u32(DATA + 8).unwrap(), 6);
    assert_eq!(v.mem.read_u32(DATA + 12).unwrap(), 0xc000_0027);
    assert_eq!(v.mem.read_u32(TEB_ADDR).unwrap(), u32::MAX);
}

#[test]
fn seh_unwind_rejects_corrupt_or_absent_targets_before_cleanup() {
    let mut v = Vm::new();
    let first = STACK_BASE + STACK_SIZE - 128;
    v.mem.write_u32(first, u32::MAX).unwrap();
    v.mem.write_u32(first + 4, CODE).unwrap();
    v.mem.write_u8(CODE, 0xcc).unwrap(); // validation must not run this handler
    v.mem.write_u32(TEB_ADDR, first).unwrap();
    assert!(v
        .try_call("kernel32.dll", "RtlUnwind", &[first + 16, 0, 0, 0])
        .unwrap_err()
        .contains("not in the chain"));
    assert_eq!(v.mem.read_u32(TEB_ADDR).unwrap(), first);
    v.mem.write_u32(first, first).unwrap();
    assert!(v
        .try_call("ntdll.dll", "RtlUnwind", &[0, 0, 0, 0])
        .unwrap_err()
        .contains("cyclic"));
    v.mem.write_u32(TEB_ADDR, DATA).unwrap();
    assert!(v
        .try_call("kernel32.dll", "RtlUnwind", &[0, 0, 0, 0])
        .unwrap_err()
        .contains("invalid SEH unwind frame"));
}

#[test]
fn seh_catch_transfer_escapes_raise_and_initializer_host_callbacks() {
    for initializer in [false, true] {
        let mut v = Vm::new();
        let first = STACK_BASE + STACK_SIZE - 128;
        let target = first + 32;
        let unwind = v.api.register("kernel32.dll", Some("RtlUnwind"), None);
        let raise = v.api.register("kernel32.dll", Some("RaiseException"), None);
        v.mem.write_bytes(CODE, &[0xb8, 1, 0, 0, 0, 0xc3]).unwrap();
        v.mem.write_u32(first, target).unwrap();
        v.mem.write_u32(first + 4, CODE).unwrap();
        v.mem.write_u32(target, u32::MAX).unwrap();
        v.mem.write_u32(target + 4, CODE + 64).unwrap();
        v.mem.write_u32(TEB_ADDR, first).unwrap();
        // Outer search handler: global unwind, then discard the callback stack
        // and jump to a catch outside both RaiseException and _initterm.
        let mut handler = vec![0x8b, 0x44, 0x24, 4, 0x6a, 0, 0x50, 0x6a, 0, 0x68];
        handler.extend(target.to_le_bytes());
        handler.push(0xb8);
        handler.extend(unwind.to_le_bytes());
        handler.extend([0xff, 0xd0, 0xbc]);
        handler.extend((target - 16).to_le_bytes());
        handler.push(0xb8);
        handler.extend((CODE + 256).to_le_bytes());
        handler.extend([0xff, 0xe0]);
        v.mem.write_bytes(CODE + 64, &handler).unwrap();
        let mut catch = vec![0xc7, 0x05];
        catch.extend(DATA.to_le_bytes());
        catch.extend(0xcafe_beefu32.to_le_bytes());
        catch.push(0xcc);
        v.mem.write_bytes(CODE + 256, &catch).unwrap();
        if initializer {
            let mut code = vec![0x6a, 0, 0x6a, 0, 0x6a, 1, 0x68];
            code.extend(0xe06d_7363u32.to_le_bytes());
            code.push(0xb8);
            code.extend(raise.to_le_bytes());
            code.extend([0xff, 0xd0, 0xcc]); // cannot return here
            v.mem.write_bytes(CODE + 512, &code).unwrap();
            v.mem.write_u32(DATA + 64, CODE + 512).unwrap();
            v.call("msvcrt.dll", "_initterm", &[DATA + 64, DATA + 68]);
        } else {
            v.k("RaiseException", &[0xe06d_7363, 1, 0, 0]);
        }
        assert_eq!(v.cpu.esp(), target - 16);
        assert_ne!(v.cpu.eip, RET); // no stub epilogue/saved callback restoration
        let outcome = v.cpu.run(
            &mut v.mem,
            &mut v.api,
            &mut v.state,
            &mut v.log,
            &mut v.sched,
            1_000_000,
        );
        assert!(
            matches!(outcome, wire::cpu::RunOutcome::Crashed(ref e) if e.contains("breakpoint"))
        );
        assert_eq!(v.mem.read_u32(DATA).unwrap(), 0xcafe_beef);
        assert_eq!(v.mem.read_u32(TEB_ADDR).unwrap(), target);
    }
}

#[test]
fn window_classes_keep_metadata_support_atoms_and_share_ansi_unicode_names() {
    let mut v = Vm::new();
    v.wide(DATA + 128, "WireWindow");
    v.mem.write_bytes(DATA + 160, b"wirewindow\0").unwrap();
    assert_eq!(
        v.call(
            "user32.dll",
            "GetClassInfoExW",
            &[0x400000, DATA + 128, DATA + 256]
        ),
        0
    );
    assert_eq!(v.state.last_error, 1411);
    let words = [48, 3, CODE, 4, 8, 0x400000, 2, 3, 4, 0, DATA + 128, 5];
    for (i, value) in words.into_iter().enumerate() {
        v.mem.write_u32(DATA + i as u32 * 4, value).unwrap();
    }
    let atom = v.call("user32.dll", "RegisterClassExW", &[DATA]);
    assert!((0xc000..=0xffff).contains(&atom));
    assert_eq!(v.call("user32.dll", "RegisterClassExW", &[DATA]), 0);
    assert_eq!(v.state.last_error, 1410);
    assert_eq!(
        v.call(
            "user32.dll",
            "GetClassInfoExA",
            &[0x400000, DATA + 160, DATA + 256]
        ),
        1
    );
    for (i, value) in words.into_iter().enumerate().filter(|(i, _)| *i != 10) {
        assert_eq!(v.mem.read_u32(DATA + 256 + i as u32 * 4).unwrap(), value);
    }
    let name = v.mem.read_u32(DATA + 296).unwrap();
    assert_eq!(v.mem.read_cstr(name, 256).unwrap(), "WireWindow");
    assert_eq!(
        v.call("user32.dll", "GetClassInfoW", &[0x400000, atom, DATA + 320]),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 324).unwrap(), CODE);
    let name = v.mem.read_u32(DATA + 356).unwrap();
    assert_eq!(v.mem.read_wstr(name, 256).unwrap(), "WireWindow");
    assert_eq!(
        v.call(
            "user32.dll",
            "GetClassInfoExA",
            &[0x500000, atom, DATA + 256]
        ),
        0
    );
    assert_eq!(
        v.call("user32.dll", "UnregisterClassA", &[atom, 0x400000]),
        1
    );
    assert_eq!(
        v.call(
            "user32.dll",
            "GetClassInfoExW",
            &[0x400000, DATA + 128, DATA + 256]
        ),
        0
    );
    // WNDCLASS (40 bytes) has no cbSize/hIconSm and remains interoperable.
    for (i, value) in words[1..11].iter().enumerate() {
        v.mem.write_u32(DATA + i as u32 * 4, *value).unwrap();
    }
    let atom = v.call("user32.dll", "RegisterClassW", &[DATA]);
    assert_ne!(atom, 0);
    assert_eq!(
        v.call(
            "user32.dll",
            "GetClassInfoExW",
            &[0x400000, atom, DATA + 256]
        ),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 300).unwrap(), 0);
}

fn test_window(v: &mut Vm, wndproc: u32) -> u32 {
    v.wide(DATA + 128, "CallbackWindow");
    let words = [48, 0, wndproc, 0, 8, 0x400000, 0, 0, 0, 0, DATA + 128, 0];
    for (i, n) in words.into_iter().enumerate() {
        v.mem.write_u32(DATA + i as u32 * 4, n).unwrap();
    }
    let atom = v.call("user32.dll", "RegisterClassExW", &[DATA]);
    assert_ne!(atom, 0);
    v.call(
        "user32.dll",
        "CreateWindowExW",
        &[0, atom, 0, 0, 3, 4, 640, 480, 0, 0, 0x400000, 0x1234],
    )
}

#[test]
fn keyboard_host_state_and_retrieved_state_follow_different_boundaries() {
    let mut v = Vm::new();
    v.mem
        .write_bytes(CODE, &[0xb8, 1, 0, 0, 0, 0xc2, 16, 0])
        .unwrap();
    let hwnd = test_window(&mut v, CODE);
    wire::user32::host_message(&mut v.state, &mut v.sched, hwnd, 0x100, 0x41, 1);
    assert_eq!(
        v.call("user32.dll", "GetAsyncKeyState", &[0x41]),
        0xffff8001
    );
    assert_eq!(
        v.call("user32.dll", "GetAsyncKeyState", &[0x41]),
        0xffff8000
    );
    assert_eq!(v.call("user32.dll", "GetKeyState", &[0x41]), 0);
    assert_eq!(
        v.call(
            "user32.dll",
            "PeekMessageW",
            &[DATA + 0x900, hwnd, 0x100, 0x100, 0]
        ),
        1
    );
    assert_eq!(v.call("user32.dll", "GetKeyState", &[0x41]), 0);
    assert_eq!(
        v.call(
            "user32.dll",
            "PeekMessageW",
            &[DATA + 0x900, hwnd, 0x100, 0x100, 1]
        ),
        1
    );
    assert_eq!(v.call("user32.dll", "GetKeyState", &[0x41]), 0xffff8000);
    assert_eq!(v.call("user32.dll", "GetKeyboardState", &[DATA + 0xa00]), 1);
    assert_eq!(v.mem.read_u8(DATA + 0xa41).unwrap(), 0x80);
    wire::user32::host_message(&mut v.state, &mut v.sched, hwnd, 0x101, 0x41, 0xc0000001);
    assert_eq!(v.call("user32.dll", "GetAsyncKeyState", &[0x41]), 0);
    assert_eq!(v.call("user32.dll", "GetKeyState", &[0x41]), 0xffff8000);
    assert_eq!(
        v.call(
            "user32.dll",
            "GetMessageW",
            &[DATA + 0x900, hwnd, 0x101, 0x101]
        ),
        1
    );
    assert_eq!(v.call("user32.dll", "GetKeyState", &[0x41]), 0);
    assert_eq!(v.call("user32.dll", "GetAsyncKeyState", &[256]), 0);
    assert!(v
        .try_call("user32.dll", "GetKeyboardState", &[0xfffffff0])
        .is_err());
    // CapsLock toggles once per physical press, not again on auto-repeat.
    for lp in [1, 1 | (1 << 30)] {
        wire::user32::host_message(&mut v.state, &mut v.sched, hwnd, 0x100, 0x14, lp);
        v.call(
            "user32.dll",
            "GetMessageW",
            &[DATA + 0x900, hwnd, 0x100, 0x100],
        );
        assert_eq!(v.call("user32.dll", "GetKeyState", &[0x14]), 0xffff8001);
    }
    wire::user32::host_message(&mut v.state, &mut v.sched, hwnd, 0x101, 0x14, 0xc0000001);
    v.call(
        "user32.dll",
        "GetMessageW",
        &[DATA + 0x900, hwnd, 0x101, 0x101],
    );
    assert_eq!(v.call("user32.dll", "GetKeyState", &[0x14]), 1);
}

#[test]
fn keyboard_translation_defers_host_text_and_preserves_unicode_and_ansi() {
    for wide in [true, false] {
        let mut v = Vm::new();
        v.mem
            .write_bytes(CODE, &[0xb8, 1, 0, 0, 0, 0xc2, 16, 0])
            .unwrap();
        let hwnd = if wide {
            test_window(&mut v, CODE)
        } else {
            v.mem.write_bytes(DATA + 128, b"AnsiKeyboard\0").unwrap();
            for (i, n) in [48, 0, CODE, 0, 0, 0x400000, 0, 0, 0, 0, DATA + 128, 0]
                .into_iter()
                .enumerate()
            {
                v.mem.write_u32(DATA + i as u32 * 4, n).unwrap();
            }
            let atom = v.call("user32.dll", "RegisterClassExA", &[DATA]);
            v.call(
                "user32.dll",
                "CreateWindowExA",
                &[0, atom, 0, 0, 0, 0, 640, 480, 0, 0, 0x400000, 0],
            )
        };
        assert_ne!(hwnd, 0);
        // The host layout resolves text; the guest virtual key does not imply a US character.
        for (msg, key, lp, text) in [(0x100, 0x51, 1, "あ"), (0x104, 0x42, 0x20000001, "B")] {
            wire::user32::host_message(&mut v.state, &mut v.sched, hwnd, msg, key, lp);
            for c in text.encode_utf16() {
                wire::user32::host_message(&mut v.state, &mut v.sched, hwnd, 0x102, c as u32, lp);
            }
            let character = if msg == 0x104 { 0x106 } else { 0x102 };
            assert_eq!(
                v.call(
                    "user32.dll",
                    "PeekMessageW",
                    &[DATA + 0x900, hwnd, character, character, 1]
                ),
                0
            );
            v.call("user32.dll", "GetMessageW", &[DATA + 0x900, hwnd, msg, msg]);
            assert_eq!(v.call("user32.dll", "TranslateMessage", &[DATA + 0x900]), 1);
            let expected: Vec<u32> = if wide {
                text.encode_utf16().map(u32::from).collect()
            } else {
                wire::codepage::encode(text, 932)
                    .unwrap()
                    .0
                    .into_iter()
                    .map(u32::from)
                    .collect()
            };
            for c in expected {
                assert_eq!(
                    v.call(
                        "user32.dll",
                        "PeekMessageW",
                        &[DATA + 0x900, hwnd, character, character, 1]
                    ),
                    1
                );
                assert_eq!(v.mem.read_u32(DATA + 0x908).unwrap(), c);
                assert_eq!(v.mem.read_u32(DATA + 0x90c).unwrap(), lp);
            }
            assert_eq!(
                v.call(
                    "user32.dll",
                    "PeekMessageW",
                    &[DATA + 0x900, hwnd, character, character, 1]
                ),
                0
            );
        }
    }
}

#[test]
fn keyboard_posted_messages_do_not_fake_physical_key_state() {
    let mut v = Vm::new();
    v.mem
        .write_bytes(CODE, &[0xb8, 1, 0, 0, 0, 0xc2, 16, 0])
        .unwrap();
    let hwnd = test_window(&mut v, CODE);
    v.call("user32.dll", "PostMessageW", &[hwnd, 0x100, 13, 1]);
    v.call(
        "user32.dll",
        "GetMessageW",
        &[DATA + 0x900, hwnd, 0x100, 0x100],
    );
    assert_eq!(v.call("user32.dll", "GetKeyState", &[13]), 0);
    assert_eq!(v.call("user32.dll", "GetAsyncKeyState", &[13]), 0);
    assert_eq!(v.call("user32.dll", "TranslateMessage", &[DATA + 0x900]), 1);
    assert_eq!(
        v.call(
            "user32.dll",
            "PeekMessageW",
            &[DATA + 0x900, hwnd, 0x102, 0x102, 1]
        ),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 0x908).unwrap(), 13);
    assert_eq!(v.call("user32.dll", "TranslateMessage", &[DATA + 0x900]), 0);
}
#[test]
fn window_properties_default_classes_and_registered_names_have_lifetimes() {
    let mut v = Vm::new();
    v.mem.write_bytes(DATA + 0x400, b"Static\0").unwrap();
    let hwnd = v.call(
        "user32.dll",
        "CreateWindowExA",
        &[0, DATA + 0x400, 0, 0, 0, 0, 30, 20, 0, 0, 0x400000, 0],
    );
    assert_ne!(hwnd, 0);
    v.mem.write_bytes(DATA + 0x500, b"EngineContext\0").unwrap();
    assert_eq!(
        v.call("user32.dll", "SetPropA", &[hwnd, DATA + 0x500, 0x12345678]),
        1
    );
    v.wide(DATA + 0x600, "EngineContext");
    assert_eq!(
        v.call("user32.dll", "GetPropW", &[hwnd, DATA + 0x600]),
        0x12345678
    );
    assert_eq!(
        v.call("user32.dll", "RemovePropW", &[hwnd, DATA + 0x600]),
        0x12345678
    );
    assert_eq!(v.call("user32.dll", "GetPropA", &[hwnd, DATA + 0x500]), 0);
    assert_eq!(v.call("user32.dll", "SetPropW", &[hwnd, 123, 456]), 1);
    assert_eq!(v.call("user32.dll", "GetPropA", &[hwnd, 123]), 456);
    let id = v.call("user32.dll", "RegisterWindowMessageA", &[DATA + 0x500]);
    assert!(id >= 0xc000);
    assert_eq!(
        v.call("user32.dll", "RegisterWindowMessageW", &[DATA + 0x600]),
        id
    );
    let desktop = v.call("user32.dll", "GetDesktopWindow", &[]);
    assert_eq!(v.call("user32.dll", "DestroyWindow", &[desktop]), 0);
    assert_eq!(v.call("user32.dll", "IsWindow", &[desktop]), 1);
    assert_eq!(v.call("user32.dll", "DestroyWindow", &[hwnd]), 1);
    assert_eq!(v.call("user32.dll", "GetPropA", &[hwnd, 123]), 0);
}
#[test]
fn display_enumeration_has_consistent_modes_and_checked_output_layouts() {
    let mut v = Vm::new();
    for wide in [false, true] {
        let suffix = if wide { "W" } else { "A" };
        let size = if wide { 840 } else { 424 };
        v.mem.write_u32(DATA, size).unwrap();
        assert_eq!(
            v.call(
                "user32.dll",
                &format!("EnumDisplayDevices{suffix}"),
                &[0, 0, DATA, 0]
            ),
            1
        );
        assert_eq!(
            wire::file_api::read_name(&v.mem, DATA + 4, wide).unwrap(),
            "\\\\.\\DISPLAY1"
        );
        assert_eq!(
            v.call(
                "user32.dll",
                &format!("EnumDisplayDevices{suffix}"),
                &[0, 1, DATA, 0]
            ),
            0
        );
        let dm = DATA + 0x1000;
        v.mem
            .write_u16(
                dm + if wide { 68 } else { 36 },
                if wide { 220 } else { 156 },
            )
            .unwrap();
        assert_eq!(
            v.call(
                "user32.dll",
                &format!("EnumDisplaySettings{suffix}"),
                &[0, u32::MAX, dm]
            ),
            1
        );
        let bits = dm + if wide { 168 } else { 104 };
        assert_eq!(v.mem.read_u32(bits).unwrap(), 32);
        assert_eq!(v.mem.read_u32(bits + 4).unwrap(), 1024);
        assert_eq!(v.mem.read_u32(bits + 8).unwrap(), 768);
        assert_eq!(
            v.call(
                "user32.dll",
                &format!("EnumDisplaySettings{suffix}"),
                &[0, 1, dm]
            ),
            0
        );
        v.mem.write_u16(dm + if wide { 68 } else { 36 }, 8).unwrap();
        assert_eq!(
            v.call(
                "user32.dll",
                &format!("EnumDisplaySettings{suffix}"),
                &[0, 0, dm]
            ),
            0
        );
        assert_eq!(v.state.last_error, 87);
    }
}
#[test]
fn path_directory_creation_readonly_and_delete_stay_inside_guest_root() {
    let mut v = Vm::new();
    v.mem
        .write_bytes(DATA, b"C:\\nested\\child\\save.dat\0")
        .unwrap();
    assert_eq!(
        v.call("imagehlp.dll", "MakeSureDirectoryPathExists", &[DATA]),
        1
    );
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 8);
    assert!(v.dir.join("nested/child").is_dir());
    assert!(!v.dir.join("nested/child/save.dat").exists());
    std::fs::write(v.dir.join("nested/child/save.dat"), b"save").unwrap();
    assert_eq!(v.k("SetFileAttributesA", &[DATA, 1]), 1);
    assert_eq!(v.k("DeleteFileA", &[DATA]), 0);
    assert_eq!(v.state.last_error, 5);
    assert_eq!(v.k("SetFileAttributesA", &[DATA, 0x80]), 1);
    assert_eq!(v.k("DeleteFileA", &[DATA]), 1);
    assert_eq!(v.k("DeleteFileA", &[DATA]), 0);
    v.mem.write_bytes(DATA + 0x400, b".bak\0").unwrap();
    assert_eq!(
        v.call("shlwapi.dll", "PathRenameExtensionA", &[DATA, DATA + 0x400]),
        1
    );
    assert!(v.mem.read_cstr(DATA, 260).unwrap().ends_with("save.bak"));
    assert_eq!(
        v.call("winmm.dll", "mciSendCommandW", &[0, 0x803, 0, 0]),
        274
    );
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 20);
}

#[test]
fn windows_invoke_creation_forwarding_and_destruction_callbacks() {
    let mut v = Vm::new();
    // Record message/count; return TRUE only for WM_NCCREATE, stdcall 16.
    let mut proc = vec![0x8b, 0x44, 0x24, 8, 0xa3];
    proc.extend((DATA + 256).to_le_bytes());
    proc.extend([0xff, 0x05]);
    proc.extend((DATA + 260).to_le_bytes());
    proc.extend([0x8b, 0x54, 0x24, 16, 0x89, 0x15]);
    proc.extend((DATA + 264).to_le_bytes());
    proc.extend([
        0x3d, 0x81, 0, 0, 0, 0x0f, 0x94, 0xc0, 0x0f, 0xb6, 0xc0, 0xc2, 16, 0,
    ]);
    v.mem.write_bytes(CODE, &proc).unwrap();
    let hwnd = test_window(&mut v, CODE);
    assert_ne!(hwnd, 0);
    assert_eq!(v.mem.read_u32(DATA + 256).unwrap(), 1);
    assert_eq!(v.mem.read_u32(DATA + 260).unwrap(), 2);
    assert_eq!(v.call("user32.dll", "IsWindow", &[hwnd]), 1);
    assert_eq!(v.call("user32.dll", "IsWindowUnicode", &[hwnd]), 1);
    assert_eq!(
        v.call(
            "user32.dll",
            "SetWindowLongA",
            &[hwnd, (-21i32) as u32, 0xfeed]
        ),
        0
    );
    assert_eq!(
        v.call("user32.dll", "GetWindowLongW", &[hwnd, (-21i32) as u32]),
        0xfeed
    );
    assert_eq!(
        v.call("user32.dll", "SetWindowLongW", &[hwnd, 4, 0xabcd]),
        0
    );
    assert_eq!(v.call("user32.dll", "GetWindowLongW", &[hwnd, 4]), 0xabcd);
    assert_eq!(v.call("user32.dll", "GetWindowLongW", &[hwnd, 8]), 0);
    assert_eq!(v.state.last_error, 1413);
    assert_eq!(
        v.call("user32.dll", "GetClientRect", &[hwnd, DATA + 320]),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 328).unwrap(), 640);
    assert_eq!(v.mem.read_u32(DATA + 332).unwrap(), 480);
    v.mem
        .write_bytes(CODE + 128, &[0x8b, 0x44, 0x24, 12, 0x40, 0xc2, 16, 0])
        .unwrap();
    assert_eq!(
        v.call(
            "user32.dll",
            "SetWindowLongW",
            &[hwnd, (-4i32) as u32, CODE + 128]
        ),
        CODE
    );
    assert_eq!(
        v.call("user32.dll", "SendMessageW", &[hwnd, 0x401, 42, 0]),
        43
    );
    assert_eq!(
        v.call("user32.dll", "CallWindowProcW", &[CODE, hwnd, 0x402, 0, 0]),
        0
    );
    assert_eq!(v.mem.read_u32(DATA + 256).unwrap(), 0x402);
    assert_eq!(
        v.call("user32.dll", "UnregisterClassW", &[DATA + 128, 0x400000]),
        0
    );
    assert_eq!(v.state.last_error, 1412);
    v.call(
        "user32.dll",
        "SetWindowLongW",
        &[hwnd, (-4i32) as u32, CODE],
    );
    let before = v.mem.read_u32(DATA + 260).unwrap();
    assert_eq!(v.call("user32.dll", "DestroyWindow", &[hwnd]), 1);
    assert_eq!(v.mem.read_u32(DATA + 256).unwrap(), 0x82);
    assert_eq!(v.mem.read_u32(DATA + 260).unwrap(), before + 2);
    assert_eq!(v.call("user32.dll", "IsWindow", &[hwnd]), 0);
    // An ANSI caller creating a Unicode class supplies a converted CREATESTRUCT.
    v.mem.write_bytes(DATA + 160, b"mixed title\0").unwrap();
    v.mem.write_bytes(DATA + 192, b"MissingClass\0").unwrap();
    let another = v.call(
        "user32.dll",
        "CreateWindowExA",
        &[
            0,
            DATA + 160 + 32,
            DATA + 160,
            0,
            0,
            0,
            10,
            10,
            0,
            0,
            0x400000,
            0x2222,
        ],
    );
    // Unknown class is rejected rather than creating a generic placeholder.
    assert_eq!(another, 0);
    v.mem.write_bytes(DATA + 192, b"CallbackWindow\0").unwrap();
    let another = v.call(
        "user32.dll",
        "CreateWindowExA",
        &[
            0,
            DATA + 192,
            DATA + 160,
            0,
            0,
            0,
            10,
            10,
            0,
            0,
            0x400000,
            0x2222,
        ],
    );
    assert_ne!(another, 0);
    assert_eq!(v.call("user32.dll", "IsWindowUnicode", &[another]), 1);
    let cs = v.mem.read_u32(DATA + 264).unwrap();
    assert_eq!(v.mem.read_u32(cs).unwrap(), 0x2222);
    let title = v.mem.read_u32(cs + 36).unwrap();
    assert_eq!(v.mem.read_wstr(title, 64).unwrap(), "mixed title");
    v.call("user32.dll", "DestroyWindow", &[another]);
    assert_eq!(
        v.call("user32.dll", "UnregisterClassW", &[DATA + 128, 0x400000]),
        1
    );
    v.mem.write_bytes(CODE, &[0x31, 0xc0, 0xc2, 16, 0]).unwrap();
    assert_eq!(test_window(&mut v, CODE), 0);
    assert!(v.state.user32.windows.is_empty());
}

#[test]
fn message_filters_quit_and_timers_preserve_queue_and_callback_semantics() {
    let mut v = Vm::new();
    let def = v.api.register("user32.dll", Some("DefWindowProcW"), None);
    let hwnd = test_window(&mut v, def);
    let msg = DATA + 512;
    v.mem.write_u32(msg, 0xeeee).unwrap();
    assert_eq!(v.call("user32.dll", "PeekMessageW", &[msg, 0, 0, 0, 1]), 0);
    assert_eq!(v.mem.read_u32(msg).unwrap(), 0xeeee);
    v.call("user32.dll", "PostMessageW", &[hwnd, 0x400, 11, 22]);
    v.call("user32.dll", "PostMessageA", &[hwnd, 0x401, 33, 44]);
    assert_eq!(
        v.call("user32.dll", "PeekMessageW", &[msg, hwnd, 0x401, 0x401, 0]),
        1
    );
    assert_eq!(v.mem.read_u32(msg + 8).unwrap(), 33);
    assert_eq!(v.call("user32.dll", "GetMessageA", &[msg, 0, 0, 0]), 1);
    assert_eq!(v.mem.read_u32(msg + 4).unwrap(), 0x400);
    assert_eq!(v.call("user32.dll", "PeekMessageW", &[msg, 0, 0, 0, 1]), 1);
    assert_eq!(v.mem.read_u32(msg + 4).unwrap(), 0x401);
    assert_eq!(v.call("user32.dll", "SetTimer", &[hwnd, 7, 50, 0]), 7);
    assert_eq!(v.call("user32.dll", "SetTimer", &[hwnd, 1, 50, 0]), 1);
    let automatic = v.call("user32.dll", "SetTimer", &[hwnd, 0, 50, 0]);
    assert_ne!(automatic, 1);
    v.call("user32.dll", "KillTimer", &[hwnd, automatic]);
    v.call("user32.dll", "KillTimer", &[hwnd, 1]);
    v.state
        .user32
        .timers
        .get_mut(&(1, hwnd, 7))
        .unwrap()
        .deadline = std::time::Instant::now();
    assert_eq!(
        v.call("user32.dll", "PeekMessageW", &[msg, hwnd, 0, 0, 0]),
        1
    );
    assert_eq!(v.mem.read_u32(msg + 4).unwrap(), 0x113);
    assert_eq!(v.mem.read_u32(msg + 8).unwrap(), 7);
    v.call("user32.dll", "KillTimer", &[hwnd, 7]);
    assert_eq!(
        v.call("user32.dll", "PeekMessageW", &[msg, hwnd, 0, 0, 1]),
        1
    );
    assert_eq!(v.call("user32.dll", "DispatchMessageW", &[msg]), 0);
    let mut proc = vec![0x8b, 0x44, 0x24, 12, 0xa3];
    proc.extend((DATA + 400).to_le_bytes());
    proc.extend([0x8b, 0x44, 0x24, 16, 0xa3]);
    proc.extend((DATA + 404).to_le_bytes());
    proc.extend([0x31, 0xc0, 0xc2, 16, 0]);
    v.mem.write_bytes(CODE, &proc).unwrap();
    v.call("user32.dll", "SetTimer", &[hwnd, 9, 50, CODE]);
    v.state
        .user32
        .timers
        .get_mut(&(1, hwnd, 9))
        .unwrap()
        .deadline = std::time::Instant::now();
    v.call("user32.dll", "PeekMessageW", &[msg, hwnd, 0, 0, 1]);
    let time = v.mem.read_u32(msg + 16).unwrap();
    v.call("user32.dll", "DispatchMessageW", &[msg]);
    assert_eq!(v.mem.read_u32(DATA + 400).unwrap(), 9);
    assert_eq!(v.mem.read_u32(DATA + 404).unwrap(), time);
    v.call("user32.dll", "PostQuitMessage", &[17]);
    assert_eq!(
        v.call("user32.dll", "GetMessageW", &[msg, hwnd, 0x900, 0x900]),
        0
    );
    assert_eq!(v.mem.read_u32(msg + 4).unwrap(), 0x12);
    assert_eq!(v.mem.read_u32(msg + 8).unwrap(), 17);
    v.call("user32.dll", "DestroyWindow", &[hwnd]);
    assert!(v.state.user32.timers.is_empty());
    assert_eq!(
        v.call("user32.dll", "GetMessageW", &[msg, hwnd, 0, 0]),
        u32::MAX
    );
}

#[test]
fn blocked_message_thread_resumes_after_worker_posts_without_fake_quit() {
    let mut v = Vm::new();
    let post = v
        .api
        .register("user32.dll", Some("PostThreadMessageW"), None);
    let exit = v.api.register("kernel32.dll", Some("ExitThread"), None);
    let mut worker = vec![0x6a, 44, 0x6a, 33, 0x68];
    worker.extend(0x501u32.to_le_bytes());
    worker.extend([0x6a, 1, 0xb8]);
    worker.extend(post.to_le_bytes());
    worker.extend([0xff, 0xd0, 0x6a, 0, 0xb8]);
    worker.extend(exit.to_le_bytes());
    worker.extend([0xff, 0xd0, 0xcc]);
    v.mem.write_bytes(CODE, &worker).unwrap();
    v.k("CreateThread", &[0, 0, CODE, 0, 0, DATA]);
    let get = v.api.register("user32.dll", Some("GetMessageW"), None);
    let msg = DATA + 512;
    v.cpu.eip = get;
    let sp = v.cpu.esp();
    v.mem.write_u32(sp, RET).unwrap();
    for (i, n) in [msg, 0, 0, 0].into_iter().enumerate() {
        v.mem.write_u32(sp + 4 + i as u32 * 4, n).unwrap();
    }
    v.mem.write_u8(RET, 0xcc).unwrap();
    let outcome = v.cpu.run(
        &mut v.mem,
        &mut v.api,
        &mut v.state,
        &mut v.log,
        &mut v.sched,
        1_000_000,
    );
    assert!(matches!(outcome, wire::cpu::RunOutcome::Crashed(ref e) if e.contains("breakpoint")));
    assert_eq!(v.sched.current_tid(), 1);
    assert_eq!(v.cpu.regs[0], 1);
    assert_eq!(v.mem.read_u32(msg + 4).unwrap(), 0x501);
    assert_eq!(v.mem.read_u32(msg + 8).unwrap(), 33);
    assert_eq!(v.mem.read_u32(msg + 12).unwrap(), 44);
}

#[test]
fn resources_load_real_utf16_strings_and_bound_lengths() {
    let mut v = Vm::new();
    let base = v.state.exe_base;
    v.mem.commit(base, 0x10000).unwrap();
    v.mem.write_u32(base + 0x3c, 0x80).unwrap();
    v.mem.write_u32(base + 0x80, 0x4550).unwrap();
    v.mem.write_u32(base + 0x80 + 24 + 112, 0x1000).unwrap();
    v.mem.write_u32(base + 0x80 + 24 + 116, 0x1000).unwrap();
    let root = base + 0x1000;
    for off in [0, 24, 48] {
        v.mem.write_u16(root + off + 14, 1).unwrap();
    }
    for (off, value) in [
        (16, 6),
        (20, 0x80000018),
        (40, 1),
        (44, 0x80000030),
        (64, 0x411),
        (68, 72),
        (72, 0x2000),
        (76, 36),
    ] {
        v.mem.write_u32(root + off, value).unwrap();
    }
    v.mem.write_u16(base + 0x2000, 2).unwrap();
    v.wide(base + 0x2002, "日本");
    assert_eq!(v.call("user32.dll", "LoadStringW", &[base, 0, DATA, 10]), 2);
    assert_eq!(v.mem.read_wstr(DATA, 10).unwrap(), "日本");
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 236); // ret + four stdcall args
    assert_eq!(v.call("user32.dll", "LoadStringW", &[base, 0, DATA, 0]), 2);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), base + 0x2002);
    assert_eq!(v.call("user32.dll", "LoadStringA", &[base, 0, DATA, 10]), 4);
    let mut b = [0; 5];
    v.mem.read_bytes(DATA, &mut b).unwrap();
    assert_eq!(b, [0x93, 0xfa, 0x96, 0x7b, 0]);
    let res = v.k("FindResourceW", &[base, 1, 6]);
    assert_eq!(res, root + 72);
    assert_eq!(v.k("SizeofResource", &[base, res]), 36);
    assert_eq!(v.k("LoadResource", &[base, res]), base + 0x2000);
    v.mem.write_u16(base + 0x2000, 0xffff).unwrap();
    assert_eq!(v.call("user32.dll", "LoadStringW", &[base, 0, DATA, 10]), 0);
    assert_eq!(v.mem.read_u16(DATA).unwrap(), 0);
}

#[test]
fn format_message_allocation_and_local_free_share_real_heap() {
    let mut v = Vm::new();
    let count = v.k("FormatMessageW", &[0x1300, 0, 2, 0, DATA, 0, 0]);
    assert!(count > 0);
    let p = v.mem.read_u32(DATA).unwrap();
    assert_ne!(p, 0);
    assert_eq!(
        v.mem.read_wstr(p, 100).unwrap(),
        "The system cannot find the file specified.\r\n"
    );
    assert_eq!(v.k("LocalFree", &[p]), 0);
    assert_eq!(v.k("HeapValidate", &[v.state.process_heap, 0, p]), 0);
}

#[test]
fn wait_all_acquires_objects_atomically() {
    let mut v = Vm::new();
    let event = v.k("CreateEventW", &[0, 0, 1, 0]);
    let sem = v.k("CreateSemaphoreW", &[0, 0, 1, 0]);
    v.mem.write_u32(DATA, event).unwrap();
    v.mem.write_u32(DATA + 4, sem).unwrap();
    assert_eq!(v.k("WaitForMultipleObjects", &[2, DATA, 1, 0]), 0x102);
    assert!(matches!(
        v.state.handles.get(&event),
        Some(HandleObj::Event { signaled: true, .. })
    ));
    assert_eq!(v.k("ReleaseSemaphore", &[sem, 1, 0]), 1);
    assert_eq!(v.k("WaitForMultipleObjects", &[2, DATA, 1, 0]), 0);
    assert!(matches!(
        v.state.handles.get(&event),
        Some(HandleObj::Event {
            signaled: false,
            ..
        })
    ));
    assert_eq!(v.k("WaitForSingleObject", &[sem, 0]), 0x102);
    assert_eq!(v.k("SetEvent", &[event]), 1);
    assert_eq!(v.k("WaitForSingleObjectEx", &[event, 0, 1]), 0);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 240); // stdcall: return + 3 arguments
    assert_eq!(v.k("WaitForSingleObjectEx", &[event, 0, 0]), 0x102);
    assert_eq!(v.k("WaitForSingleObjectEx", &[0xdead, 0, 0]), u32::MAX);
    assert_eq!(v.k("GetLastError", &[]), 6);
}

#[test]
fn blocked_thread_resumes_after_worker_signals_and_exits() {
    fn call(code: &mut Vec<u8>, stub: u32, args: &[u32]) {
        for &a in args.iter().rev() {
            code.push(0x68);
            code.extend(a.to_le_bytes());
        }
        code.push(0xb8);
        code.extend(stub.to_le_bytes());
        code.extend([0xff, 0xd0]);
    }
    let mut v = Vm::new();
    let event = v.k("CreateEventW", &[0, 0, 0, 0]);
    let set = v.api.register("kernel32.dll", Some("SetEvent"), None);
    let mut worker = Vec::new();
    call(&mut worker, set, &[event]);
    worker.extend([0xb8, 42, 0, 0, 0, 0xc2, 4, 0]);
    v.mem.write_bytes(CODE + 0x400, &worker).unwrap();
    let h = v.k("CreateThread", &[0, 0, CODE + 0x400, 0, 0, 0]);
    let wait = v
        .api
        .register("kernel32.dll", Some("WaitForSingleObject"), None);
    let get_exit = v
        .api
        .register("kernel32.dll", Some("GetExitCodeThread"), None);
    let exit = v.api.register("kernel32.dll", Some("ExitProcess"), None);
    let mut main = Vec::new();
    call(&mut main, wait, &[event, u32::MAX]);
    main.push(0xa3);
    main.extend((DATA + 16).to_le_bytes());
    call(&mut main, get_exit, &[h, DATA + 20]);
    call(&mut main, exit, &[0]);
    v.mem.write_bytes(CODE, &main).unwrap();
    v.cpu.eip = CODE;
    let outcome = v.cpu.run(
        &mut v.mem,
        &mut v.api,
        &mut v.state,
        &mut v.log,
        &mut v.sched,
        10_000,
    );
    assert!(matches!(outcome, wire::cpu::RunOutcome::Exited(0)));
    assert_eq!(v.mem.read_u32(DATA + 16).unwrap(), 0);
    assert_eq!(v.mem.read_u32(DATA + 20).unwrap(), 42);
}
#[test]
fn cross_thread_sends_wake_when_window_is_destroyed_or_owner_exits() {
    fn call(code: &mut Vec<u8>, stub: u32, args: &[u32]) {
        for n in args.iter().rev() {
            code.push(0x68);
            code.extend(n.to_le_bytes());
        }
        code.push(0xb8);
        code.extend(stub.to_le_bytes());
        code.extend([0xff, 0xd0]);
    }
    for owner_exits in [false, true] {
        let mut v = Vm::new();
        v.mem
            .write_bytes(CODE + 0x600, &[0xb8, 1, 0, 0, 0, 0xc2, 16, 0])
            .unwrap();
        let hwnd = test_window(&mut v, CODE + 0x600);
        let send = v.api.register("user32.dll", Some("SendMessageW"), None);
        let exit = v.api.register("kernel32.dll", Some("ExitProcess"), None);
        let sleep = v.api.register("kernel32.dll", Some("Sleep"), None);
        let finish = v.api.register(
            if owner_exits {
                "kernel32.dll"
            } else {
                "user32.dll"
            },
            Some(if owner_exits {
                "ExitThread"
            } else {
                "DestroyWindow"
            }),
            None,
        );
        v.mem.write_u32(DATA + 0x7000, 0xdeadbeef).unwrap();
        let mut worker = Vec::new();
        call(&mut worker, send, &[hwnd, 0x405, 0, 0]);
        worker.push(0xa3);
        worker.extend((DATA + 0x7000).to_le_bytes());
        call(&mut worker, exit, &[0]);
        v.mem.write_bytes(CODE + 0x400, &worker).unwrap();
        v.k("CreateThread", &[0, 0, CODE + 0x400, 0, 0, 0]);
        let mut main = Vec::new();
        call(&mut main, sleep, &[1]);
        call(&mut main, finish, &[if owner_exits { 42 } else { hwnd }]);
        call(&mut main, sleep, &[100]);
        v.mem.write_bytes(CODE, &main).unwrap();
        v.cpu.eip = CODE;
        let result = v.cpu.run(
            &mut v.mem,
            &mut v.api,
            &mut v.state,
            &mut v.log,
            &mut v.sched,
            10000,
        );
        if let wire::cpu::RunOutcome::Crashed(error) = result {
            panic!("{error}");
        }
        assert!(matches!(result, wire::cpu::RunOutcome::Exited(0)));
        assert_eq!(v.mem.read_u32(DATA + 0x7000).unwrap(), 0);
        assert!(!v.state.user32.windows.contains_key(&hwnd));
    }
}

#[test]
fn cross_thread_send_message_runs_on_owner_teb_and_preserves_sender_result() {
    fn call(code: &mut Vec<u8>, stub: u32, args: &[u32]) {
        for &n in args.iter().rev() {
            code.push(0x68);
            code.extend(n.to_le_bytes());
        }
        code.push(0xb8);
        code.extend(stub.to_le_bytes());
        code.extend([0xff, 0xd0]);
    }
    let mut v = Vm::new();
    let mut wndproc = vec![0x64, 0xa1, 0x24, 0, 0, 0, 0xa3];
    wndproc.extend((DATA + 0x7000).to_le_bytes());
    wndproc.extend([0xb8, 0x34, 0x12, 0, 0, 0xc2, 16, 0]);
    v.mem.write_bytes(CODE + 0x600, &wndproc).unwrap();
    let hwnd = test_window(&mut v, CODE + 0x600);
    v.mem.write_u32(DATA + 0x7000, 0).unwrap();
    let send = v.api.register("user32.dll", Some("SendMessageW"), None);
    let post = v
        .api
        .register("user32.dll", Some("PostThreadMessageW"), None);
    let main_tid = v.sched.current_tid();
    v.mem.write_u32(TEB_ADDR + 0x24, main_tid).unwrap();
    let mut worker = Vec::new();
    call(&mut worker, send, &[hwnd, 0x405, 0, 0]);
    worker.push(0xa3);
    worker.extend((DATA + 0x7004).to_le_bytes());
    call(&mut worker, post, &[main_tid, 0x406, 0, 0]);
    worker.extend([0xc2, 4, 0]);
    v.mem.write_bytes(CODE + 0x400, &worker).unwrap();
    v.k("CreateThread", &[0, 0, CODE + 0x400, 0, 0, 0]);
    let get = v.api.register("user32.dll", Some("GetMessageW"), None);
    let exit = v.api.register("kernel32.dll", Some("ExitProcess"), None);
    let mut main = Vec::new();
    call(&mut main, get, &[DATA + 0x7100, 0, 0, 0]);
    call(&mut main, exit, &[0]);
    v.mem.write_bytes(CODE, &main).unwrap();
    v.cpu.eip = CODE;
    let result = v.cpu.run(
        &mut v.mem,
        &mut v.api,
        &mut v.state,
        &mut v.log,
        &mut v.sched,
        10000,
    );
    match result {
        wire::cpu::RunOutcome::Exited(0) => {}
        wire::cpu::RunOutcome::Crashed(e) => panic!("{e}"),
        _ => panic!("did not exit"),
    }
    assert_eq!(v.mem.read_u32(DATA + 0x7000).unwrap(), main_tid);
    assert_eq!(v.mem.read_u32(DATA + 0x7004).unwrap(), 0x1234);
    assert_eq!(v.mem.read_u32(DATA + 0x7104).unwrap(), 0x406);
}

#[test]
fn timeslice_wakes_expired_waiter_even_while_other_thread_spins() {
    use wire::thread::{ThreadState, WaitTarget, Wake};
    let mut v = Vm::new();
    v.k("CreateThread", &[0, 0, CODE, 0, 0, 0]);
    v.sched.threads[1].state = ThreadState::Waiting {
        target: WaitTarget::Sleep,
        deadline: Some(std::time::Instant::now()),
    };
    assert!(v.sched.round_robin(&mut v.cpu));
    assert_eq!(v.sched.current, 1);
    assert_eq!(v.sched.take_wake(), Some(Wake::Completed(0x102)));
    assert_eq!(v.cpu.mxcsr, 0x1f80);
    assert_eq!(v.cpu.fpu.cw, 0x037f);
}

#[test]
fn worker_teb_slots_do_not_alias_main_teb_or_peb() {
    let mut v = Vm::new();
    v.mem.commit(wire::loader::PEB_ADDR, 0x1000).unwrap();
    v.mem.write_u32(wire::loader::PEB_ADDR, 0xcafe).unwrap();
    v.mem.write_u32(TEB_ADDR + 0x34, 0xbeef).unwrap();
    for _ in 0..40 {
        v.k("CreateThread", &[0, 0, CODE, 0, 4, 0]);
    }
    assert_eq!(v.mem.read_u32(wire::loader::PEB_ADDR).unwrap(), 0xcafe);
    assert_eq!(v.mem.read_u32(TEB_ADDR + 0x34).unwrap(), 0xbeef);
    for t in v.sched.threads.iter().skip(1) {
        assert_ne!(t.teb, TEB_ADDR);
        assert_ne!(t.teb, wire::loader::PEB_ADDR);
        assert_eq!(v.mem.read_u32(t.teb + 0x24).unwrap(), t.tid);
    }
}

#[test]
fn x87_stack_and_rounding_survive_thread_switches() {
    let mut v = Vm::new();
    v.k("CreateThread", &[0, 0, CODE + 0x400, 0, 0, 0]);
    v.mem.write_bytes(CODE, &[0xd9, 0xe8]).unwrap();
    v.cpu.eip = CODE;
    v.cpu.step(&mut v.mem).unwrap();
    assert!(v.sched.round_robin(&mut v.cpu));
    assert_eq!(v.cpu.fpu.tags, 0xffff);
    v.cpu.fpu.cw = 0xb7f;
    v.mem.write_bytes(CODE + 0x400, &[0xd9, 0xeb]).unwrap();
    v.cpu.step(&mut v.mem).unwrap();
    let worker = v.cpu.fpu;
    assert!(v.sched.round_robin(&mut v.cpu));
    assert_eq!(v.cpu.fpu.st_bits(0), 0x3fff_8000_0000_0000_0000);
    assert_eq!(v.cpu.fpu.cw, 0x37f);
    assert!(v.sched.round_robin(&mut v.cpu));
    assert_eq!(v.cpu.fpu.regs, worker.regs);
    assert_eq!(v.cpu.fpu.tags, worker.tags);
    assert_eq!(v.cpu.fpu.sw, worker.sw);
    assert_eq!(v.cpu.fpu.cw, worker.cw);
}
#[test]
fn seh_context_preserves_and_allows_editing_x87_stack() {
    let mut v = Vm::new();
    v.mem.write_bytes(CODE + 0x400, &[0xd9, 0xe8]).unwrap();
    v.cpu.eip = CODE + 0x400;
    v.cpu.step(&mut v.mem).unwrap();
    v.cpu.fpu.cw = 0xb7f;
    // FNINIT; ctx=[esp+12]; FLDZ; FSTP m80[ctx+56]; tag[ctx]=zero;
    // return ExceptionContinueExecution. Callback clobbers the live FP state.
    v.mem
        .write_bytes(
            CODE,
            &[
                0xdb, 0xe3, 0x8b, 0x44, 0x24, 0x0c, 0xd9, 0xee, 0xdb, 0x78, 0x38, 0xc7, 0x40, 0x24,
                0xff, 0x7f, 0, 0, 0x31, 0xc0, 0xc3,
            ],
        )
        .unwrap();
    v.mem.write_u32(DATA, u32::MAX).unwrap();
    v.mem.write_u32(DATA + 4, CODE).unwrap();
    v.mem.write_u32(TEB_ADDR, DATA).unwrap();
    v.k("RaiseException", &[0x1234, 0, 0, 0]);
    assert_eq!(v.cpu.fpu.cw, 0xb7f);
    assert_eq!(v.cpu.fpu.top(), 7);
    assert_eq!(v.cpu.fpu.tags, 0x7fff);
    assert_eq!(v.cpu.fpu.st_bits(0), 0);
}

#[test]
fn system_profile_memory_structures_and_monotonic_clocks() {
    let mut v = Vm::new();
    v.k("GetNativeSystemInfo", &[DATA]);
    assert_eq!(v.mem.read_u16(DATA).unwrap(), 0); // x86, irrespective of host
    assert_eq!(v.mem.read_u32(DATA + 4).unwrap(), 4096);
    assert_eq!(v.mem.read_u32(DATA + 16).unwrap(), 1);
    assert_eq!(v.mem.read_u32(DATA + 20).unwrap(), 1);
    v.mem.write_u32(DATA + 128, 32).unwrap();
    assert_eq!(v.k("GlobalMemoryStatusEx", &[DATA + 128]), 0);
    assert_eq!(v.k("GetLastError", &[]), 87);
    v.mem.write_u32(DATA + 128, 64).unwrap();
    assert_eq!(v.k("GlobalMemoryStatusEx", &[DATA + 128]), 1);
    let available = v.mem.read_u64(DATA + 144).unwrap();
    v.k("HeapAlloc", &[v.state.process_heap, 0, 65536]);
    v.k("GlobalMemoryStatusEx", &[DATA + 128]);
    assert!(v.mem.read_u64(DATA + 144).unwrap() <= available - 65536);
    v.k("GlobalMemoryStatus", &[DATA + 256]);
    assert_eq!(v.mem.read_u32(DATA + 256).unwrap(), 32);
    assert_eq!(
        v.mem.read_u32(DATA + 264).unwrap() as u64,
        v.mem.read_u64(DATA + 136).unwrap()
    );
    v.state.start =
        std::time::Instant::now() - std::time::Duration::from_millis((1u64 << 32) + 1000);
    let ticks = v.k("GetTickCount64", &[]);
    assert_eq!(v.cpu.regs[2], 1);
    assert!((1000..2000).contains(&ticks));
    let mm = v.call("winmm.dll", "timeGetTime", &[]);
    let ordinary = v.k("GetTickCount", &[]);
    assert!(ordinary.wrapping_sub(mm) < 100);
    assert_eq!(v.call("winmm.dll", "timeGetDevCaps", &[DATA, 7]), 97);
    assert_eq!(v.call("winmm.dll", "timeGetDevCaps", &[DATA, 8]), 0);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 1);
    assert_eq!(v.call("winmm.dll", "timeBeginPeriod", &[0]), 97);
    assert_eq!(v.call("winmm.dll", "timeBeginPeriod", &[1]), 0);
    v.k("GetSystemTime", &[DATA]);
    assert!(v.mem.read_u16(DATA).unwrap() >= 2026);
    assert!((1..=12).contains(&v.mem.read_u16(DATA + 2).unwrap()));
}
#[test]
fn nls_case_mapping_lengths_buffers_and_crt_character_types() {
    let mut v = Vm::new();
    v.wide(DATA, "aZ 9日本");
    assert_eq!(
        v.k("LCMapStringEx", &[0, 0x200, DATA, u32::MAX, 0, 0, 0, 0, 0]),
        7
    );
    assert_eq!(
        v.k(
            "LCMapStringEx",
            &[0, 0x200, DATA, u32::MAX, DATA + 64, 7, 0, 0, 0]
        ),
        7
    );
    assert_eq!(v.mem.read_wstr(DATA + 64, 7).unwrap(), "AZ 9日本");
    assert_eq!(
        v.k(
            "LCMapStringW",
            &[0x411, 0x100, DATA + 64, u32::MAX, DATA + 64, 7]
        ),
        7
    );
    assert_eq!(v.mem.read_wstr(DATA + 64, 7).unwrap(), "az 9日本");
    assert_eq!(
        v.k(
            "LCMapStringEx",
            &[0, 0x200, DATA, u32::MAX, DATA + 128, 6, 0, 0, 0]
        ),
        0
    );
    assert_eq!(v.k("GetLastError", &[]), 122);
    assert_eq!(v.k("GetStringTypeW", &[1, DATA, u32::MAX, DATA + 128]), 1);
    assert_eq!(v.mem.read_u16(DATA + 128).unwrap() & 0x182, 0x182); // lower, alpha, hex 'a'
    assert_eq!(v.mem.read_u16(DATA + 132).unwrap() & 0x48, 0x48); // blank/space
    assert_eq!(v.mem.read_u16(DATA + 134).unwrap() & 0x84, 0x84); // digit/hex
    assert_eq!(v.mem.read_u16(DATA + 136).unwrap() & 0x100, 0x100); // Japanese letter
    assert_eq!(v.mem.read_u16(DATA + 140).unwrap() & 0x20, 0x20); // explicit NUL count
    assert_eq!(v.k("GetStringTypeW", &[3, DATA, 1, DATA + 128]), 0);
    assert_eq!(v.k("GetLastError", &[]), 1004);
}

fn gdip(v: &mut Vm, name: &str, args: &[u32]) -> u32 {
    v.call("gdiplus.dll", name, args)
}
fn matrix_values(v: &mut Vm, handle: u32) -> [f32; 6] {
    assert_eq!(gdip(v, "GdipGetMatrixElements", &[handle, DATA + 0x400]), 0);
    std::array::from_fn(|i| f32::from_bits(v.mem.read_u32(DATA + 0x400 + i as u32 * 4).unwrap()))
}

#[test]
fn gdiplus_exports_startup_allocation_and_unported_status_preserve_x86_abi() {
    let mut v = Vm::new();
    v.mem.write_bytes(DATA, b"gdiplus.dll\0").unwrap();
    let module = v.k("LoadLibraryA", &[DATA]);
    assert_ne!(module, 0);
    assert_ne!(dll_export(&mut v, module, "GdipCreateMatrix2"), 0);
    assert_eq!(dll_export(&mut v, module, "GdipMadeUpExport"), 0);
    assert_eq!(wire::gdiplus::stack_bytes("GdipCreateEffect"), Some(20)); // GUID by value
    assert_eq!(
        wire::gdiplus::stack_bytes("GdipDrawImagePointsRect"),
        Some(48)
    );
    v.mem.write_bytes(DATA + 0x100, &[0; 16]).unwrap();
    v.mem.write_u32(DATA + 0x100, 1).unwrap();
    v.mem.write_u32(DATA + 0x200, 0x1234).unwrap();
    assert_eq!(
        gdip(&mut v, "GdiplusStartup", &[DATA + 0x200, DATA + 0x100, 0]),
        0
    );
    let first = v.mem.read_u32(DATA + 0x200).unwrap();
    assert_ne!(first, 0);
    assert_eq!(
        gdip(&mut v, "GdiplusStartup", &[DATA + 0x204, DATA + 0x100, 0]),
        0
    );
    assert_ne!(first, v.mem.read_u32(DATA + 0x204).unwrap());
    v.mem.write_u32(DATA + 0x100, 3).unwrap();
    assert_eq!(
        gdip(&mut v, "GdiplusStartup", &[DATA + 0x200, DATA + 0x100, 0]),
        17
    );
    assert_eq!(v.mem.read_u32(DATA + 0x200).unwrap(), first);
    v.mem.write_u32(DATA + 0x100, 1).unwrap();
    v.mem.write_u32(DATA + 0x108, 1).unwrap();
    assert_eq!(
        gdip(&mut v, "GdiplusStartup", &[DATA + 0x200, DATA + 0x100, 0]),
        2
    );
    assert_eq!(
        gdip(
            &mut v,
            "GdiplusStartup",
            &[DATA + 0x200, DATA + 0x100, DATA + 0x300]
        ),
        6
    );
    let p = gdip(&mut v, "GdipAlloc", &[31]);
    assert_ne!(p, 0);
    let mut zero = [1; 31];
    v.mem.read_bytes(p, &mut zero).unwrap();
    assert_eq!(zero, [0; 31]);
    assert_eq!(gdip(&mut v, "GdipFree", &[p]), 0);
    assert_eq!(v.k("HeapSize", &[v.state.process_heap, 0, p]), u32::MAX);
    v.mem.write_u32(DATA + 0x300, 0x76543210).unwrap();
    assert_eq!(
        gdip(&mut v, "GdipCreateSolidFill", &[0xff00aa00, DATA + 0x300]),
        6
    );
    assert_eq!(v.mem.read_u32(DATA + 0x300).unwrap(), 0x76543210);
    assert!(v.log.recent.back().unwrap().contains("UNIMPLEMENTED"));
    assert_eq!(
        gdip(
            &mut v,
            "GdipCreateMatrix2",
            &[1f32.to_bits(), 0, 0, 1f32.to_bits(), 0, 0, DATA + 0x300]
        ),
        0
    );
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 224); // return + seven DWORDs
}

#[test]
fn gdiplus_matrix_order_clone_inverse_and_object_lifetime_match_affine_geometry() {
    let mut v = Vm::new();
    assert_eq!(gdip(&mut v, "GdipCreateMatrix", &[DATA]), 0);
    let m = v.mem.read_u32(DATA).unwrap();
    assert_eq!(matrix_values(&mut v, m), [1., 0., 0., 1., 0., 0.]);
    assert_eq!(
        gdip(
            &mut v,
            "GdipTranslateMatrix",
            &[m, 3f32.to_bits(), 5f32.to_bits(), 1]
        ),
        0
    );
    assert_eq!(gdip(&mut v, "GdipCloneMatrix", &[m, DATA + 4]), 0);
    let clone = v.mem.read_u32(DATA + 4).unwrap();
    assert_eq!(
        gdip(
            &mut v,
            "GdipScaleMatrix",
            &[m, 2f32.to_bits(), 4f32.to_bits(), 1]
        ),
        0
    );
    assert_eq!(matrix_values(&mut v, m), [2., 0., 0., 4., 6., 20.]);
    assert_eq!(
        gdip(
            &mut v,
            "GdipScaleMatrix",
            &[clone, 2f32.to_bits(), 4f32.to_bits(), 0]
        ),
        0
    );
    assert_eq!(matrix_values(&mut v, clone), [2., 0., 0., 4., 3., 5.]);
    assert_eq!(gdip(&mut v, "GdipInvertMatrix", &[m]), 0);
    assert_eq!(matrix_values(&mut v, m), [0.5, 0., 0., 0.25, -3., -5.]);
    assert_eq!(gdip(&mut v, "GdipMultiplyMatrix", &[m, clone, 1]), 0);
    assert_eq!(matrix_values(&mut v, m), [1., 0., 0., 1., -3., -15.]);
    // A prepend quarter turn rotates the basis but leaves its translation.
    assert_eq!(
        gdip(&mut v, "GdipRotateMatrix", &[m, 90f32.to_bits(), 0]),
        0
    );
    let rotated = matrix_values(&mut v, m);
    for (got, expected) in rotated.into_iter().zip([0., 1., -1., 0., -3., -15.]) {
        assert!((got - expected).abs() < 1e-6);
    }
    assert_eq!(
        gdip(
            &mut v,
            "GdipSetMatrixElements",
            &[m, 1f32.to_bits(), 0, 0, 1f32.to_bits(), 0, 0]
        ),
        0
    );
    assert_eq!(
        gdip(
            &mut v,
            "GdipShearMatrix",
            &[m, 2f32.to_bits(), 3f32.to_bits(), 1]
        ),
        0
    );
    assert_eq!(matrix_values(&mut v, m), [1., 3., 2., 1., 0., 0.]);
    let old = matrix_values(&mut v, m);
    assert_eq!(
        gdip(
            &mut v,
            "GdipTranslateMatrix",
            &[m, 1f32.to_bits(), 2f32.to_bits(), 2]
        ),
        2
    );
    assert_eq!(matrix_values(&mut v, m), old);
    assert_eq!(gdip(&mut v, "GdipDeleteMatrix", &[m]), 0);
    assert_eq!(gdip(&mut v, "GdipGetMatrixElements", &[m, DATA + 0x400]), 2);
    assert_eq!(gdip(&mut v, "GdipDeleteMatrix", &[m]), 2);
    assert_eq!(
        gdip(&mut v, "GdipGetMatrixElements", &[0xfffffff0, DATA + 0x400]),
        2
    );
}

#[test]
fn gdiplus_matrix_singular_threshold_bitwise_identity_and_checked_points() {
    let mut v = Vm::new();
    assert_eq!(
        gdip(
            &mut v,
            "GdipCreateMatrix2",
            &[
                1f32.to_bits(),
                1f32.to_bits(),
                1f32.to_bits(),
                1f32.to_bits(),
                0,
                0,
                DATA
            ]
        ),
        0
    );
    let m = v.mem.read_u32(DATA).unwrap();
    assert_eq!(gdip(&mut v, "GdipIsMatrixInvertible", &[m, DATA + 4]), 0);
    assert_eq!(v.mem.read_u32(DATA + 4).unwrap(), 0);
    let old = matrix_values(&mut v, m);
    assert_eq!(gdip(&mut v, "GdipInvertMatrix", &[m]), 2);
    assert_eq!(matrix_values(&mut v, m), old);
    // 1e-5f32 is just below Wine's double threshold; the next F32 is above it.
    // The off-diagonal 1 forces the general determinant path.
    for (det_bits, expected) in [(1e-5f32.to_bits(), 0), (1e-5f32.to_bits() + 1, 1)] {
        assert_eq!(
            gdip(
                &mut v,
                "GdipSetMatrixElements",
                &[m, det_bits, 1f32.to_bits(), 0, 1f32.to_bits(), 0, 0]
            ),
            0
        );
        assert_eq!(gdip(&mut v, "GdipIsMatrixInvertible", &[m, DATA + 4]), 0);
        assert_eq!(v.mem.read_u32(DATA + 4).unwrap(), expected);
        assert_eq!(
            gdip(&mut v, "GdipInvertMatrix", &[m]),
            if expected == 0 { 2 } else { 0 }
        );
    }
    assert_eq!(
        gdip(
            &mut v,
            "GdipSetMatrixElements",
            &[
                m,
                1f32.to_bits(),
                (-0f32).to_bits(),
                0,
                1f32.to_bits(),
                0,
                0
            ]
        ),
        0
    );
    assert_eq!(gdip(&mut v, "GdipIsMatrixIdentity", &[m, DATA + 4]), 0);
    assert_eq!(v.mem.read_u32(DATA + 4).unwrap(), 0); // Wine compares the bytes, including -0
    assert_eq!(
        gdip(
            &mut v,
            "GdipSetMatrixElements",
            &[
                m,
                2f32.to_bits(),
                0,
                0,
                3f32.to_bits(),
                10f32.to_bits(),
                (-4f32).to_bits()
            ]
        ),
        0
    );
    v.mem.write_u32(DATA + 0x100, 1f32.to_bits()).unwrap();
    v.mem.write_u32(DATA + 0x104, 2f32.to_bits()).unwrap();
    assert_eq!(
        gdip(&mut v, "GdipTransformMatrixPoints", &[m, DATA + 0x100, 1]),
        0
    );
    assert_eq!(v.mem.read_u32(DATA + 0x100).unwrap(), 12f32.to_bits());
    assert_eq!(v.mem.read_u32(DATA + 0x104).unwrap(), 2f32.to_bits());
    assert_eq!(
        gdip(
            &mut v,
            "GdipVectorTransformMatrixPoints",
            &[m, DATA + 0x100, 1]
        ),
        0
    );
    assert_eq!(v.mem.read_u32(DATA + 0x100).unwrap(), 24f32.to_bits());
    assert_eq!(v.mem.read_u32(DATA + 0x104).unwrap(), 6f32.to_bits());
    v.mem.write_u32(DATA + 0xfff8, 1f32.to_bits()).unwrap();
    v.mem.write_u32(DATA + 0xfffc, 2f32.to_bits()).unwrap();
    assert_eq!(
        gdip(&mut v, "GdipTransformMatrixPoints", &[m, DATA + 0xfff8, 2]),
        2
    );
    assert_eq!(v.mem.read_u32(DATA + 0xfff8).unwrap(), 1f32.to_bits()); // no partial writes across fault
    assert_eq!(
        gdip(&mut v, "GdipTransformMatrixPoints", &[m, DATA + 0x100, 0]),
        2
    );
    assert_eq!(
        gdip(
            &mut v,
            "GdipTransformMatrixPoints",
            &[m, DATA + 0x100, u32::MAX]
        ),
        2
    );
}

#[test]
fn gdiplus_rect_mapping_and_integer_rounding_transform_points() {
    let mut v = Vm::new();
    for (i, n) in [1i32, 2, 4, 8].into_iter().enumerate() {
        v.mem
            .write_u32(DATA + 0x100 + i as u32 * 4, n as u32)
            .unwrap();
    }
    for (i, n) in [10i32, 20, 18, 20, 10, 44].into_iter().enumerate() {
        v.mem
            .write_u32(DATA + 0x200 + i as u32 * 4, n as u32)
            .unwrap();
    }
    assert_eq!(
        gdip(
            &mut v,
            "GdipCreateMatrix3I",
            &[DATA + 0x100, DATA + 0x200, DATA]
        ),
        0
    );
    let m = v.mem.read_u32(DATA).unwrap();
    assert_eq!(matrix_values(&mut v, m), [2., 0., 0., 3., 8., 14.]);
    v.mem.write_u32(DATA + 0x300, 1).unwrap();
    v.mem.write_u32(DATA + 0x304, 2).unwrap();
    assert_eq!(
        gdip(&mut v, "GdipTransformMatrixPointsI", &[m, DATA + 0x300, 1]),
        0
    );
    assert_eq!(v.mem.read_u32(DATA + 0x300).unwrap(), 10);
    assert_eq!(v.mem.read_u32(DATA + 0x304).unwrap(), 20);
    assert_eq!(
        gdip(
            &mut v,
            "GdipSetMatrixElements",
            &[
                m,
                1f32.to_bits(),
                0,
                0,
                1f32.to_bits(),
                0.5f32.to_bits(),
                (-0.5f32).to_bits()
            ]
        ),
        0
    );
    v.mem.write_u32(DATA + 0x300, 0).unwrap();
    v.mem.write_u32(DATA + 0x304, 0).unwrap();
    assert_eq!(
        gdip(&mut v, "GdipTransformMatrixPointsI", &[m, DATA + 0x300, 1]),
        0
    );
    assert_eq!(v.mem.read_u32(DATA + 0x300).unwrap(), 1);
    assert_eq!(v.mem.read_u32(DATA + 0x304).unwrap(), 0);
}

#[test]
fn window_enumeration_filters_threads_and_children_stops_on_false_and_skips_destroyed_windows() {
    let mut v = Vm::new();
    v.mem
        .write_bytes(CODE, &[0xb8, 1, 0, 0, 0, 0xc2, 16, 0])
        .unwrap();
    let first = test_window(&mut v, CODE);
    let atom = v.state.user32.windows[&first].class as u32;
    let second = v.call(
        "user32.dll",
        "CreateWindowExW",
        &[0, atom, 0, 0, 0, 0, 100, 100, 0, 0, 0x400000, 0],
    );
    let child = v.call(
        "user32.dll",
        "CreateWindowExW",
        &[0, atom, 0, 0x40000000, 0, 0, 10, 10, first, 0, 0x400000, 0],
    );
    assert_ne!(child, 0);
    let callback = CODE + 0x400;
    v.mem
        .write_bytes(
            callback,
            &[
                0x8b, 0x54, 0x24, 8, 0x8b, 0x0a, 0x8b, 0x44, 0x24, 4, 0x89, 0x44, 0x8a, 4, 0xff, 2,
                0xb8, 1, 0, 0, 0, 0xc2, 8, 0,
            ],
        )
        .unwrap();
    assert_eq!(
        v.call("user32.dll", "EnumWindows", &[callback, DATA + 0x500]),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 0x500).unwrap(), 2);
    assert_eq!(v.mem.read_u32(DATA + 0x504).unwrap(), second);
    assert_eq!(v.mem.read_u32(DATA + 0x508).unwrap(), first);
    assert_eq!(
        v.call(
            "user32.dll",
            "GetWindowThreadProcessId",
            &[first, DATA + 0x600]
        ),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 0x600).unwrap(), 4242);
    v.mem.write_u32(DATA + 0x500, 0).unwrap();
    assert_eq!(
        v.call(
            "user32.dll",
            "EnumThreadWindows",
            &[999, callback, DATA + 0x500]
        ),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 0x500).unwrap(), 0);

    let set_error = v.api.register("kernel32.dll", Some("SetLastError"), None);
    let mut stop = vec![0x68];
    stop.extend(777u32.to_le_bytes());
    stop.push(0xb8);
    stop.extend(set_error.to_le_bytes());
    stop.extend([0xff, 0xd0, 0x31, 0xc0, 0xc2, 8, 0]);
    v.mem.write_bytes(callback, &stop).unwrap();
    assert_eq!(
        v.call("user32.dll", "EnumThreadWindows", &[1, callback, 0]),
        0
    );
    assert_eq!(v.k("GetLastError", &[]), 777);
    assert_eq!(v.cpu.eip, RET);
    let destroy = v.api.register("user32.dll", Some("DestroyWindow"), None);
    let mut code = vec![0xff, 5];
    code.extend((DATA + 0x500).to_le_bytes());
    code.push(0x68);
    code.extend(first.to_le_bytes());
    code.push(0xb8);
    code.extend(destroy.to_le_bytes());
    code.extend([0xff, 0xd0, 0xb8, 1, 0, 0, 0, 0xc2, 8, 0]);
    v.mem.write_bytes(callback, &code).unwrap();
    assert_eq!(v.call("user32.dll", "EnumWindows", &[callback, 0]), 1);
    assert_eq!(v.mem.read_u32(DATA + 0x500).unwrap(), 1); // second's callback destroyed first in the snapshot
    assert!(!v.state.user32.windows.contains_key(&first));

    // An exiting callback retains its new CPU context and ends enumeration.
    assert_ne!(
        v.call(
            "user32.dll",
            "CreateWindowExW",
            &[0, atom, 0, 0, 0, 0, 100, 100, 0, 0, 0x400000, 0]
        ),
        0
    );
    v.mem.write_u32(DATA + 0x500, 0).unwrap();
    let exit = v.api.register("kernel32.dll", Some("ExitProcess"), None);
    let mut code = vec![0xff, 5];
    code.extend((DATA + 0x500).to_le_bytes());
    code.extend([0x68, 37, 0, 0, 0, 0xb8]);
    code.extend(exit.to_le_bytes());
    code.extend([0xff, 0xd0, 0xc2, 8, 0]);
    v.mem.write_bytes(callback, &code).unwrap();
    v.call("user32.dll", "EnumWindows", &[callback, 0]);
    assert_eq!(v.mem.read_u32(DATA + 0x500).unwrap(), 1);
    assert_eq!(v.cpu.exit_code, Some(37));
    assert_eq!(v.cpu.eip, exit);
}

#[test]
fn virtual_memory_reservation_commit_rounding_query_and_recommit_preserve_permissions() {
    use wire::mem::*;
    let mut v = Vm::new();
    let p = v.k(
        "VirtualAlloc",
        &[0x31000123, 0x1001, MEM_RESERVE, PAGE_NOACCESS],
    );
    assert_eq!(p, 0x31000000);
    let info = v.mem.virtual_query(p + 123).unwrap();
    assert_eq!(
        (
            info.base,
            info.allocation_base,
            info.region_size,
            info.state,
            info.protect
        ),
        (p, p, 0x2000, MEM_RESERVE, 0)
    );
    assert!(!v.mem.is_committed(p));
    assert!(v.mem.read_u8(p).is_err());
    assert_eq!(
        v.k("VirtualAlloc", &[p + 0x1001, 1, MEM_COMMIT, PAGE_READWRITE]),
        p + 0x1000
    );
    v.mem.write_u32(p + 0x1000, 0xcafefeed).unwrap();
    assert!(v.mem.read_u8(p).is_err());
    assert_eq!(v.mem.virtual_query(p).unwrap().region_size, 0x1000);
    assert_eq!(v.k("VirtualQuery", &[p + 0x1003, DATA, 28]), 28);
    let fields: Vec<_> = (0..7)
        .map(|i| v.mem.read_u32(DATA + i * 4).unwrap())
        .collect();
    assert_eq!(
        fields,
        [
            p + 0x1000,
            p,
            PAGE_NOACCESS,
            0x1000,
            MEM_COMMIT,
            PAGE_READWRITE,
            MEM_PRIVATE
        ]
    );
    // Recommit never changes existing contents or page protection.
    assert_eq!(
        v.k(
            "VirtualAlloc",
            &[p + 0x1000, 0x1000, MEM_COMMIT, PAGE_READONLY]
        ),
        p + 0x1000
    );
    assert_eq!(v.mem.read_u32(p + 0x1000).unwrap(), 0xcafefeed);
    v.mem.write_u32(p + 0x1000, 42).unwrap();
    v.mem.write_u32(DATA + 64, 0xfeed).unwrap();
    assert_eq!(
        v.k("VirtualProtect", &[p, 0x2000, PAGE_READONLY, DATA + 64]),
        0
    );
    assert_eq!(v.k("GetLastError", &[]), 487);
    assert_eq!(v.mem.read_u32(DATA + 64).unwrap(), 0xfeed);
    assert_eq!(
        v.mem.virtual_query(p + 0x1000).unwrap().protect,
        PAGE_READWRITE
    );
    assert_eq!(v.k("VirtualAlloc", &[p, 1, MEM_COMMIT, PAGE_NOACCESS]), p);
    assert!(v.mem.is_committed(p));
    assert!(v.mem.read_u8(p).is_err());
    assert_eq!(v.mem.virtual_query(p).unwrap().protect, PAGE_NOACCESS);
}

#[test]
fn virtual_memory_4k_decommit_recommit_zeroing_and_release_keep_neighbors_intact() {
    use wire::mem::*;
    let mut v = Vm::new();
    let p = v.k(
        "VirtualAlloc",
        &[0x31000000, 0x4000, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE],
    );
    assert_eq!(p, 0x31000000);
    for i in 0..4 {
        v.mem.write_u32(p + i * GUEST_PAGE, i + 100).unwrap();
    }
    assert_eq!(v.k("VirtualFree", &[p + 0x1003, 1, 0x4000]), 1);
    assert!(v.mem.read_u32(p + GUEST_PAGE).is_err());
    assert_eq!(
        v.mem.virtual_query(p + GUEST_PAGE).unwrap().state,
        MEM_RESERVE
    );
    for i in [0, 2, 3] {
        assert_eq!(v.mem.read_u32(p + i * GUEST_PAGE).unwrap(), i + 100);
    }
    assert_eq!(
        v.k(
            "VirtualAlloc",
            &[p + GUEST_PAGE, GUEST_PAGE, MEM_COMMIT, PAGE_READONLY]
        ),
        p + GUEST_PAGE
    );
    assert_eq!(v.mem.read_u32(p + GUEST_PAGE).unwrap(), 0);
    assert!(v.mem.write_u32(p + GUEST_PAGE, 77).is_err());
    assert_eq!(
        v.k("VirtualProtect", &[p + GUEST_PAGE, 1, PAGE_READWRITE, DATA]),
        1
    );
    assert_eq!(v.mem.read_u32(DATA).unwrap(), PAGE_READONLY);
    // lpflOldProtect may alias the newly readonly range; validate it at entry.
    assert_eq!(
        v.k(
            "VirtualProtect",
            &[p + GUEST_PAGE, GUEST_PAGE, PAGE_READONLY, p + GUEST_PAGE]
        ),
        1
    );
    assert_eq!(v.mem.read_u32(p + GUEST_PAGE).unwrap(), PAGE_READWRITE);
    assert_eq!(v.k("VirtualFree", &[p + GUEST_PAGE, 0, 0x8000]), 0);
    assert_eq!(v.k("VirtualFree", &[p, 0x4000, 0x8000]), 0);
    assert!(v.mem.is_committed(p));
    assert_eq!(v.k("VirtualFree", &[p, 0, 0x8000]), 1);
    assert!(v.mem.read_u32(p).is_err());
    assert_eq!(v.mem.virtual_query(p).unwrap().state, MEM_FREE);
    assert_eq!(
        v.k(
            "VirtualAlloc",
            &[p, 0x4000, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE]
        ),
        p
    );
    for i in 0..4 {
        assert_eq!(v.mem.read_u32(p + i * GUEST_PAGE).unwrap(), 0);
    }
}

#[test]
fn virtual_memory_rejects_collisions_invalid_flags_cross_allocations_and_invalid_outputs() {
    use wire::mem::*;
    let mut v = Vm::new();
    for base in [
        CODE, STACK_BASE, 0x00f00000, 0x02000000, 0x10000000, 0x50000000, 0x6fff0000, TEB_ADDR,
    ] {
        assert_eq!(
            v.k("VirtualAlloc", &[base, 4096, MEM_RESERVE, PAGE_READWRITE]),
            0
        );
        assert_eq!(v.k("GetLastError", &[]), 487);
    }
    assert_eq!(v.k("VirtualFree", &[CODE, 0, 0x8000]), 0);
    for (address, size, flags, protect, error) in [
        (0, 0, MEM_COMMIT, 4, 87),
        (0, 1, 0, 4, 87),
        (0, 1, MEM_RESERVE, 6, 87),
        (0, 1, MEM_RESERVE, 8, 87),
        (0, 1, MEM_RESERVE, PAGE_GUARD | PAGE_NOACCESS, 87),
        (0, 1, MEM_RESERVE | 0x20000000, 4, 50),
        (0xfffff000, 0x2000, MEM_RESERVE, 4, 487),
    ] {
        assert_eq!(v.k("VirtualAlloc", &[address, size, flags, protect]), 0);
        assert_eq!(v.k("GetLastError", &[]), error);
    }
    let first = v.k(
        "VirtualAlloc",
        &[
            0x31000000,
            0x10000,
            MEM_RESERVE | MEM_COMMIT,
            PAGE_READWRITE,
        ],
    );
    let second = v.k(
        "VirtualAllocEx",
        &[
            u32::MAX,
            0x31010000,
            4096,
            MEM_RESERVE | MEM_COMMIT,
            PAGE_READWRITE,
        ],
    );
    assert_ne!(first, 0);
    assert_ne!(second, 0);
    assert_eq!(
        v.k("VirtualProtect", &[first, 0x11000, PAGE_READONLY, DATA]),
        0
    );
    assert_eq!(v.mem.virtual_query(first).unwrap().protect, PAGE_READWRITE);
    assert_eq!(
        v.k("VirtualProtect", &[first, 4096, PAGE_READONLY, 0x30000000]),
        0
    );
    assert_eq!(v.mem.virtual_query(first).unwrap().protect, PAGE_READWRITE);
    assert_eq!(
        v.k(
            "VirtualAllocEx",
            &[123, 0, 4096, MEM_RESERVE, PAGE_READWRITE]
        ),
        0
    );
    assert_eq!(v.k("GetLastError", &[]), 6);
    v.mem.write_bytes(DATA, &[0x55; 32]).unwrap();
    assert_eq!(v.k("VirtualQuery", &[first, DATA, 27]), 0);
    let mut sentinel = [0; 32];
    v.mem.read_bytes(DATA, &mut sentinel).unwrap();
    assert_eq!(sentinel, [0x55; 32]);
    assert_eq!(v.k("VirtualQueryEx", &[u32::MAX, first, DATA, 32]), 28);
    assert_eq!(v.mem.read_u32(DATA + 28).unwrap(), 0x55555555);
    let top = v.k(
        "VirtualAlloc",
        &[0, 4096, MEM_RESERVE | 0x100000, PAGE_READWRITE],
    );
    assert!(top > second && top < 0x7ff00000);
    assert_eq!(top % ALLOCATION_GRANULARITY, 0);
}

fn d3d_com(v: &mut Vm, object: u32, slot: u32, args: &[u32]) -> u32 {
    let table = v.mem.read_u32(object).unwrap();
    let stub = v.mem.read_u32(table + slot * 4).unwrap();
    let mut all = vec![object];
    all.extend_from_slice(args);
    let result = v
        .cpu
        .call_guest(
            &mut v.mem,
            &mut v.api,
            &mut v.state,
            &mut v.log,
            &mut v.sched,
            stub,
            &all,
            1_000_000,
        )
        .unwrap();
    match result {
        wire::cpu::GuestCallOutcome::Returned(n) => n,
        _ => panic!("COM control transfer escaped"),
    }
}
fn d3d_device(v: &mut Vm) -> (u32, u32, u32) {
    v.mem
        .write_bytes(CODE, &[0xb8, 1, 0, 0, 0, 0xc2, 16, 0])
        .unwrap();
    let hwnd = test_window(v, CODE);
    let factory = v.call("d3d9.dll", "Direct3DCreate9", &[32]);
    assert_ne!(factory, 0);
    let p = DATA + 0x800;
    for (i, n) in [4, 4, 21, 1, 0, 0, 1, hwnd, 1, 0, 0, 0, 0, 0]
        .into_iter()
        .enumerate()
    {
        v.mem.write_u32(p + i as u32 * 4, n).unwrap();
    }
    assert_eq!(
        d3d_com(v, factory, 16, &[0, 1, hwnd, 0x20, p, DATA + 0x900]),
        0
    );
    let device = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_ne!(device, 0);
    (factory, device, hwnd)
}
#[test]
fn d3d9_target_binding_resets_extent_and_rhw_drives_perspective_sampling() {
    let mut v = Vm::new();
    let (factory, device, _) = d3d_device(&mut v);
    d3d_com(&mut v, device, 18, &[0, 0, 0, DATA + 0x900]);
    let back = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_eq!(
        d3d_com(&mut v, device, 28, &[8, 4, 21, 0, 0, 0, DATA + 0x900, 0]),
        0
    );
    let target = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_eq!(d3d_com(&mut v, device, 37, &[0, target]), 0);
    d3d_com(&mut v, device, 48, &[DATA]);
    assert_eq!(v.mem.read_u32(DATA + 8).unwrap(), 8);
    assert_eq!(d3d_com(&mut v, device, 37, &[0, back]), 0);
    d3d_com(&mut v, device, 48, &[DATA]);
    assert_eq!(v.mem.read_u32(DATA + 8).unwrap(), 4);
    assert_eq!(
        d3d_com(&mut v, device, 23, &[8, 1, 1, 0, 21, 1, DATA + 0x900, 0]),
        0
    );
    let texture = v.mem.read_u32(DATA + 0x900).unwrap();
    d3d_com(&mut v, texture, 19, &[0, DATA + 0x910, 0, 0]);
    let bits = v.mem.read_u32(DATA + 0x914).unwrap();
    for i in 0..8 {
        v.mem.write_u32(bits + i * 4, 0xff000000 + i * 10).unwrap();
    }
    d3d_com(&mut v, texture, 20, &[0]);
    d3d_com(&mut v, device, 65, &[0, texture]);
    for (i, values) in [
        [0f32, 0., 0.5, 1., 0., 0.],
        [4., 0., 0.5, 0.5, 1., 0.],
        [0., 4., 0.5, 1., 0., 0.],
    ]
    .into_iter()
    .enumerate()
    {
        for (j, n) in values.into_iter().enumerate() {
            v.mem
                .write_u32(DATA + i as u32 * 24 + j as u32 * 4, n.to_bits())
                .unwrap();
        }
    }
    d3d_com(&mut v, device, 89, &[0x104]);
    d3d_com(&mut v, device, 57, &[22, 1]);
    d3d_com(&mut v, device, 41, &[]);
    assert_eq!(d3d_com(&mut v, device, 83, &[4, 1, DATA, 24]), 0);
    d3d_com(&mut v, back, 13, &[DATA + 0x910, 0, 0x10]);
    let pixels = v.mem.read_u32(DATA + 0x914).unwrap();
    // Barycentrics=(1/2,1/4,1/4), u=(1/4*1/2)/(1/2+1/8+1/4)=1/7.
    assert_eq!(v.mem.read_u32(pixels + 5 * 4).unwrap(), 0xff00000a);
    d3d_com(&mut v, back, 14, &[]);
    d3d_com(&mut v, device, 42, &[]);
    for object in [back, target, texture, device, factory] {
        d3d_com(&mut v, object, 2, &[]);
    }
}

#[test]
fn d3d9_indexed_declarations_transforms_and_reset_preserve_real_resources() {
    let mut v = Vm::new();
    let (factory, device, _) = d3d_device(&mut v);
    assert_eq!(
        d3d_com(&mut v, device, 23, &[1, 1, 1, 0, 21, 1, DATA + 0x900, 0]),
        0
    );
    let texture = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_eq!(d3d_com(&mut v, texture, 19, &[0, DATA + 0x910, 0, 0]), 0);
    let texbits = v.mem.read_u32(DATA + 0x914).unwrap();
    v.mem.write_u32(texbits, 0xff336699).unwrap();
    d3d_com(&mut v, texture, 20, &[0]);
    d3d_com(&mut v, device, 65, &[0, texture]);
    d3d_com(&mut v, device, 26, &[140, 0, 0x144, 1, DATA + 0x900, 0]);
    let vb = v.mem.read_u32(DATA + 0x900).unwrap();
    d3d_com(&mut v, vb, 11, &[0, 0, DATA + 0x910, 0]);
    let vertices = v.mem.read_u32(DATA + 0x910).unwrap();
    for (i, [x, y]) in [[-0.5f32, -0.5], [3.5, -0.5], [-0.5, 3.5], [3.5, 3.5]]
        .into_iter()
        .enumerate()
    {
        let base = vertices + (i as u32 + 1) * 28;
        for (j, n) in [x, y, 0.25, 1.].into_iter().enumerate() {
            v.mem.write_u32(base + j as u32 * 4, n.to_bits()).unwrap();
        }
        v.mem.write_u32(base + 16, 0xffffffff).unwrap();
        v.mem.write_u32(base + 20, 0).unwrap();
        v.mem.write_u32(base + 24, 0).unwrap();
    }
    d3d_com(&mut v, vb, 12, &[]);
    d3d_com(&mut v, device, 27, &[14, 0, 101, 1, DATA + 0x900, 0]);
    let ib = v.mem.read_u32(DATA + 0x900).unwrap();
    d3d_com(&mut v, ib, 11, &[0, 0, DATA + 0x910, 0]);
    let indices = v.mem.read_u32(DATA + 0x910).unwrap();
    for (i, n) in [999u16, 2, 3, 4, 4, 3, 5].into_iter().enumerate() {
        v.mem.write_u16(indices + i as u32 * 2, n).unwrap();
    }
    d3d_com(&mut v, ib, 12, &[]);
    d3d_com(&mut v, device, 100, &[0, vb, 0, 28]);
    d3d_com(&mut v, device, 104, &[ib]);
    // POSITIONT/COLOR/TEXCOORD declaration owns an exact copy of the elements.
    let elements = [
        0, 0, 0, 0, 3, 0, 9, 0, 0, 0, 16, 0, 4, 0, 10, 0, 0, 0, 20, 0, 1, 0, 5, 0, 255, 0, 0, 0,
        17, 0, 0, 0,
    ];
    v.mem.write_bytes(DATA, &elements).unwrap();
    assert_eq!(d3d_com(&mut v, device, 86, &[DATA, DATA + 0x900]), 0);
    let declaration = v.mem.read_u32(DATA + 0x900).unwrap();
    v.mem.write_bytes(DATA, &[0; 32]).unwrap();
    assert_eq!(d3d_com(&mut v, declaration, 4, &[0, DATA + 0x910]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x910).unwrap(), 4);
    v.mem.write_u32(DATA + 0x910, 3).unwrap();
    assert_eq!(
        d3d_com(&mut v, declaration, 4, &[DATA, DATA + 0x910]),
        0x88760867
    );
    assert_eq!(d3d_com(&mut v, declaration, 4, &[DATA, DATA + 0x910]), 0);
    let mut copy = [0; 32];
    v.mem.read_bytes(DATA, &mut copy).unwrap();
    assert_eq!(copy, elements);
    d3d_com(&mut v, device, 87, &[declaration]);
    d3d_com(&mut v, device, 57, &[22, 1]);
    d3d_com(&mut v, device, 41, &[]);
    assert_eq!(
        d3d_com(&mut v, device, 82, &[4, (-1i32) as u32, 2, 4, 1, 2]),
        0
    );
    assert_eq!(d3d_com(&mut v, device, 18, &[0, 0, 0, DATA + 0x900]), 0);
    let back = v.mem.read_u32(DATA + 0x900).unwrap();
    d3d_com(&mut v, back, 13, &[DATA + 0x910, 0, 0x10]);
    let pixels = v.mem.read_u32(DATA + 0x914).unwrap();
    d3d_com(&mut v, back, 14, &[]);
    for i in 0..16 {
        assert_eq!(v.mem.read_u32(pixels + i * 4).unwrap(), 0xff336699);
    }
    v.mem.write_u16(indices + 2, 1).unwrap();
    assert_eq!(
        d3d_com(&mut v, device, 82, &[4, (-1i32) as u32, 2, 4, 1, 2]),
        0x8876086c
    );
    assert_eq!(v.mem.read_u32(pixels).unwrap(), 0xff336699);
    // Real XYZ vertices transform and clip against all homogeneous planes.
    d3d_com(&mut v, device, 89, &[0x142]);
    d3d_com(&mut v, device, 100, &[0, vb, 28, 24]);
    for (i, [x, y]) in [[-2.5f32, 2.5], [2.5, 2.5], [-2.5, -2.5], [2.5, -2.5]]
        .into_iter()
        .enumerate()
    {
        let base = vertices + 28 + i as u32 * 24;
        for (j, n) in [x, y, 0.25].into_iter().enumerate() {
            v.mem.write_u32(base + j as u32 * 4, n.to_bits()).unwrap();
        }
        v.mem.write_u32(base + 12, 0xffffffff).unwrap();
        v.mem.write_u32(base + 16, 0).unwrap();
        v.mem.write_u32(base + 20, 0).unwrap();
    }
    let matrix = std::array::from_fn::<_, 16, _>(|i| {
        if i == 0 || i == 5 {
            0.5f32
        } else if i % 5 == 0 {
            1.
        } else {
            0.
        }
    });
    for (i, n) in matrix.into_iter().enumerate() {
        v.mem
            .write_u32(DATA + 0x100 + i as u32 * 4, n.to_bits())
            .unwrap();
    }
    assert_eq!(d3d_com(&mut v, device, 44, &[256, DATA + 0x100]), 0);
    assert_eq!(d3d_com(&mut v, device, 45, &[256, DATA + 0x200]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x200).unwrap(), 0.5f32.to_bits());
    d3d_com(&mut v, device, 43, &[0, 0, 1, 0xff000000, 0, 0]);
    assert_eq!(d3d_com(&mut v, device, 81, &[5, 0, 2]), 0);
    for i in 0..16 {
        assert_eq!(v.mem.read_u32(pixels + i * 4).unwrap(), 0xff336699);
    }
    d3d_com(&mut v, device, 42, &[]);
    // A live implicit back-buffer reference prevents Reset without changing it.
    let params = [8u32, 4, 21, 1, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0];
    for (i, n) in params.into_iter().enumerate() {
        v.mem.write_u32(DATA + 0x800 + i as u32 * 4, n).unwrap();
    }
    assert_eq!(d3d_com(&mut v, device, 16, &[DATA + 0x800]), 0x8876086c);
    assert_eq!(v.mem.read_u32(pixels).unwrap(), 0xff336699);
    d3d_com(&mut v, back, 2, &[]);
    assert_eq!(d3d_com(&mut v, device, 16, &[DATA + 0x800]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x800).unwrap(), 8);
    assert_eq!(
        v.k("HeapSize", &[v.state.process_heap, 0, pixels]),
        u32::MAX
    );
    // Managed buffer/texture/declaration storage survives and bindings reset.
    assert_ne!(
        v.k("HeapSize", &[v.state.process_heap, 0, vertices]),
        u32::MAX
    );
    assert_eq!(v.mem.read_u32(texbits).unwrap(), 0xff336699);
    assert_eq!(d3d_com(&mut v, device, 88, &[DATA + 0x910]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x910).unwrap(), 0);
    assert_eq!(d3d_com(&mut v, device, 18, &[0, 0, 0, DATA + 0x900]), 0);
    let back = v.mem.read_u32(DATA + 0x900).unwrap();
    d3d_com(&mut v, back, 12, &[DATA + 0x100]);
    assert_eq!(v.mem.read_u32(DATA + 0x118).unwrap(), 8);
    for object in [back, declaration, ib, vb, texture, device, factory] {
        d3d_com(&mut v, object, 2, &[]);
    }
}

#[test]
fn d3d9_depth_stencil_alpha_and_vertex_streams_affect_real_pixels() {
    let mut v = Vm::new();
    let (factory, device, _) = d3d_device(&mut v);
    assert_eq!(d3d_com(&mut v, factory, 10, &[0, 1, 22, 2, 1, 75]), 0);
    assert_eq!(
        d3d_com(&mut v, device, 29, &[4, 4, 75, 0, 0, 0, DATA + 0x900, 0]),
        0
    );
    let depth = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_eq!(d3d_com(&mut v, device, 39, &[depth]), 0);
    assert_eq!(
        d3d_com(&mut v, depth, 13, &[DATA + 0x910, 0, 0]),
        0x8876086a
    );
    assert_eq!(d3d_com(&mut v, device, 40, &[DATA + 0x910]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x910).unwrap(), depth);
    assert_eq!(d3d_com(&mut v, depth, 2, &[]), 1);
    assert_eq!(
        d3d_com(
            &mut v,
            device,
            43,
            &[0, 0, 7, 0xff112233, 1f32.to_bits(), 3]
        ),
        0
    );
    assert_eq!(
        d3d_com(&mut v, device, 23, &[1, 1, 1, 0, 21, 1, DATA + 0x900, 0]),
        0
    );
    let texture = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_eq!(d3d_com(&mut v, texture, 19, &[0, DATA + 0x910, 0, 0]), 0);
    let texbits = v.mem.read_u32(DATA + 0x914).unwrap();
    v.mem.write_u32(texbits, 0xffff0000).unwrap();
    assert_eq!(d3d_com(&mut v, texture, 20, &[0]), 0);
    assert_eq!(d3d_com(&mut v, device, 65, &[0, texture]), 0);
    assert_eq!(
        d3d_com(&mut v, device, 26, &[96, 0, 0x104, 1, DATA + 0x900, 0]),
        0
    );
    let buffer = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_eq!(d3d_com(&mut v, buffer, 11, &[0, 0, DATA + 0x910, 0]), 0);
    let vertices = v.mem.read_u32(DATA + 0x910).unwrap();
    assert_eq!(
        d3d_com(&mut v, buffer, 11, &[0, 4, DATA + 0x910, 0]),
        0x8876086c
    );
    let points = [[-0.5f32, -0.5], [3.5, -0.5], [-0.5, 3.5], [3.5, 3.5]];
    for (i, [x, y]) in points.into_iter().enumerate() {
        for (j, n) in [x, y, 0.25, 1., 0., 0.].into_iter().enumerate() {
            v.mem
                .write_u32(vertices + i as u32 * 24 + j as u32 * 4, n.to_bits())
                .unwrap();
        }
    }
    assert_eq!(d3d_com(&mut v, buffer, 12, &[]), 0);
    assert_eq!(
        d3d_com(&mut v, buffer, 11, &[95, 2, DATA + 0x910, 0]),
        0x8876086c
    );
    assert_eq!(d3d_com(&mut v, device, 100, &[0, buffer, 0, 24]), 0);
    assert_eq!(d3d_com(&mut v, device, 89, &[0x104]), 0);
    for (state, value) in [(22, 1), (7, 1), (52, 1), (56, 3), (57, 3), (55, 4)] {
        assert_eq!(d3d_com(&mut v, device, 57, &[state, value]), 0);
    }
    assert_eq!(d3d_com(&mut v, device, 41, &[]), 0);
    assert_eq!(d3d_com(&mut v, device, 81, &[5, 0, 2]), 0);
    assert_eq!(d3d_com(&mut v, device, 18, &[0, 0, 0, DATA + 0x920]), 0);
    let back = v.mem.read_u32(DATA + 0x920).unwrap();
    assert_eq!(d3d_com(&mut v, back, 13, &[DATA + 0x930, 0, 0x10]), 0);
    let pixels = v.mem.read_u32(DATA + 0x934).unwrap();
    assert_eq!(v.mem.read_u32(pixels).unwrap(), 0xffff0000);
    assert_eq!(d3d_com(&mut v, back, 14, &[]), 0);
    // Stencil was incremented to four: the second red draw must be rejected.
    assert_eq!(d3d_com(&mut v, device, 43, &[0, 0, 1, 0xff00ff00, 0, 0]), 0);
    assert_eq!(d3d_com(&mut v, device, 81, &[5, 0, 2]), 0);
    assert_eq!(v.mem.read_u32(pixels).unwrap(), 0xff00ff00);
    // Stencil disabled; farther z fails, even though the triangle covers pixel zero.
    assert_eq!(d3d_com(&mut v, device, 57, &[52, 0]), 0);
    for i in 0..4 {
        v.mem
            .write_u32(vertices + i * 24 + 8, 0.75f32.to_bits())
            .unwrap();
    }
    assert_eq!(d3d_com(&mut v, device, 81, &[5, 0, 2]), 0);
    assert_eq!(v.mem.read_u32(pixels).unwrap(), 0xff00ff00);
    assert_eq!(d3d_com(&mut v, device, 57, &[7, 0]), 0);
    // Alpha test discards zero-alpha samples before updating any depth/stencil.
    v.mem.write_u32(texbits, 0x00ffffff).unwrap();
    for (state, value) in [(15, 1), (25, 5), (24, 0)] {
        assert_eq!(d3d_com(&mut v, device, 57, &[state, value]), 0);
    }
    assert_eq!(d3d_com(&mut v, device, 81, &[5, 0, 2]), 0);
    assert_eq!(v.mem.read_u32(pixels).unwrap(), 0xff00ff00);
    assert_eq!(d3d_com(&mut v, device, 42, &[]), 0);
    assert_eq!(d3d_com(&mut v, buffer, 2, &[]), 0);
    assert_ne!(
        v.k("HeapSize", &[v.state.process_heap, 0, vertices]),
        u32::MAX
    );
    assert_eq!(d3d_com(&mut v, back, 2, &[]), 0);
    assert_eq!(d3d_com(&mut v, depth, 2, &[]), 0);
    assert_eq!(d3d_com(&mut v, texture, 2, &[]), 0);
    assert_eq!(d3d_com(&mut v, device, 2, &[]), 0);
    assert_eq!(
        v.k("HeapSize", &[v.state.process_heap, 0, vertices]),
        u32::MAX
    );
    assert_eq!(d3d_com(&mut v, factory, 2, &[]), 0);
}
#[test]
fn d3d9_com_texture_quad_present_has_real_pixels_and_releases_bound_resources() {
    let mut v = Vm::new();
    let (factory, device, hwnd) = d3d_device(&mut v);
    assert_eq!(
        d3d_com(&mut v, device, 23, &[2, 2, 1, 0, 21, 1, DATA + 0x900, 0]),
        0
    );
    let texture = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_eq!(d3d_com(&mut v, texture, 19, &[0, DATA + 0x910, 0, 0]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x910).unwrap(), 8);
    let bits = v.mem.read_u32(DATA + 0x914).unwrap();
    for (i, n) in [0xffff0000, 0xff00ff00, 0xff0000ff, 0xffffffff]
        .into_iter()
        .enumerate()
    {
        v.mem.write_u32(bits + i as u32 * 4, n).unwrap();
    }
    assert_eq!(d3d_com(&mut v, texture, 20, &[0]), 0);
    assert_eq!(d3d_com(&mut v, device, 65, &[0, texture]), 0);
    assert_eq!(d3d_com(&mut v, device, 89, &[0x104]), 0);
    assert_eq!(d3d_com(&mut v, device, 57, &[22, 1]), 0);
    for (i, vertex) in [
        [-0.5f32, -0.5, 0.5, 1., 0., 0.],
        [3.5, -0.5, 0.5, 1., 1., 0.],
        [-0.5, 3.5, 0.5, 1., 0., 1.],
        [3.5, 3.5, 0.5, 1., 1., 1.],
    ]
    .into_iter()
    .enumerate()
    {
        for (j, n) in vertex.into_iter().enumerate() {
            v.mem
                .write_u32(DATA + 0xa00 + i as u32 * 24 + j as u32 * 4, n.to_bits())
                .unwrap();
        }
    }
    assert_eq!(d3d_com(&mut v, device, 41, &[]), 0);
    assert_eq!(d3d_com(&mut v, device, 83, &[5, 2, DATA + 0xa00, 24]), 0);
    assert_eq!(d3d_com(&mut v, device, 42, &[]), 0);
    assert_eq!(d3d_com(&mut v, device, 17, &[0, 0, 0, 0]), 0);
    let (w, h, pixels) = v.state.graphics.window_snapshot(&v.mem, hwnd).unwrap();
    assert_eq!((w, h), (640, 480));
    assert_eq!(pixels[0], 0xff);
    assert_eq!(pixels[639], 0xff00);
    assert_eq!(pixels[479 * 640], 0xff0000);
    assert_eq!(pixels[480 * 640 - 1], 0xffffff);
    assert_eq!(v.state.d3d9.frames, 1);
    assert_eq!(v.state.d3d9.draws, 1);
    // Device owns the binding after the guest drops its COM reference.
    assert_eq!(d3d_com(&mut v, texture, 2, &[]), 0);
    assert_ne!(v.k("HeapSize", &[v.state.process_heap, 0, bits]), u32::MAX);
    assert_eq!(d3d_com(&mut v, factory, 2, &[]), 0);
    assert_eq!(d3d_com(&mut v, device, 2, &[]), 0);
    assert_eq!(v.k("HeapSize", &[v.state.process_heap, 0, bits]), u32::MAX);
    assert_eq!(
        v.k("HeapSize", &[v.state.process_heap, 0, texture]),
        u32::MAX
    );
    assert_eq!(
        v.k("HeapSize", &[v.state.process_heap, 0, device]),
        u32::MAX
    );
    assert_eq!(
        v.k("HeapSize", &[v.state.process_heap, 0, factory]),
        u32::MAX
    );
}
#[test]
fn d3d9_surface_locks_and_external_child_references_preserve_com_owners() {
    let mut v = Vm::new();
    let (factory, device, _) = d3d_device(&mut v);
    assert_eq!(d3d_com(&mut v, device, 18, &[0, 0, 0, DATA + 0x900]), 0);
    let surface = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_eq!(d3d_com(&mut v, surface, 13, &[DATA + 0x910, 0, 0]), 0);
    let bits = v.mem.read_u32(DATA + 0x914).unwrap();
    assert_eq!(
        d3d_com(&mut v, surface, 13, &[DATA + 0x920, 0, 0]),
        0x8876086c
    );
    assert_eq!(
        d3d_com(&mut v, device, 43, &[0, 0, 1, 0xff123456, 0, 0]),
        0x8876086c
    );
    assert_eq!(d3d_com(&mut v, surface, 14, &[]), 0);
    assert_eq!(d3d_com(&mut v, surface, 14, &[]), 0x8876086c);
    assert_eq!(d3d_com(&mut v, device, 43, &[0, 0, 1, 0xff123456, 0, 0]), 0);
    assert_eq!(v.mem.read_u32(bits).unwrap(), 0xff123456);
    assert_eq!(d3d_com(&mut v, device, 2, &[]), 0);
    assert_eq!(d3d_com(&mut v, surface, 3, &[DATA + 0x920]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x920).unwrap(), device);
    assert_eq!(d3d_com(&mut v, device, 2, &[]), 0);
    assert_eq!(d3d_com(&mut v, surface, 2, &[]), 0);
    assert_eq!(v.k("HeapSize", &[v.state.process_heap, 0, bits]), u32::MAX);
    assert_eq!(d3d_com(&mut v, factory, 2, &[]), 0);
}
#[test]
fn d3d9_failed_outputs_formats_and_vertex_reads_leave_pixels_unchanged() {
    let mut v = Vm::new();
    let (factory, device, _) = d3d_device(&mut v);
    let available = d3d_com(&mut v, device, 4, &[]);
    assert_eq!(
        d3d_com(&mut v, device, 23, &[2, 2, 1, 0, 21, 1, 0x30000000, 0]),
        0x8876086c
    );
    assert_eq!(
        d3d_com(&mut v, device, 23, &[2, 2, 1, 0, 23, 1, DATA + 0x900, 0]),
        0x8876086a
    );
    assert_eq!(v.mem.read_u32(DATA + 0x900).unwrap(), 0);
    assert_eq!(d3d_com(&mut v, device, 4, &[]), available);
    assert_eq!(d3d_com(&mut v, device, 89, &[0x104]), 0);
    assert_eq!(d3d_com(&mut v, device, 41, &[]), 0);
    assert_eq!(
        d3d_com(&mut v, device, 83, &[5, 2, 0x30000000, 24]),
        0x8876086c
    );
    assert_eq!(v.state.d3d9.draws, 0);
    assert_eq!(d3d_com(&mut v, device, 42, &[]), 0);
    assert_eq!(d3d_com(&mut v, device, 2, &[]), 0);
    assert_eq!(d3d_com(&mut v, factory, 2, &[]), 0);
}

#[test]
fn d3d9_alpha_quad_shared_edges_scissor_and_diffuse_vertices_are_exact() {
    let mut v = Vm::new();
    let (factory, device, _) = d3d_device(&mut v);
    assert_eq!(
        d3d_com(&mut v, device, 23, &[1, 1, 1, 0, 21, 1, DATA + 0x900, 0]),
        0
    );
    let texture = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_eq!(
        d3d_com(&mut v, texture, 19, &[0, DATA + 0x910, 0, 0x8000]),
        0
    );
    let bits = v.mem.read_u32(DATA + 0x914).unwrap();
    v.mem.write_u32(bits, 0x80ffffff).unwrap();
    assert_eq!(d3d_com(&mut v, texture, 20, &[0]), 0);
    assert_eq!(d3d_com(&mut v, device, 65, &[0, texture]), 0);
    assert_eq!(d3d_com(&mut v, device, 89, &[0x144]), 0);
    for (n, value) in [(22, 1), (27, 1), (19, 5), (20, 6), (174, 1)] {
        assert_eq!(d3d_com(&mut v, device, 57, &[n, value]), 0);
    }
    for (i, n) in [1, 1, 3, 3].into_iter().enumerate() {
        v.mem.write_u32(DATA + 0x940 + i as u32 * 4, n).unwrap();
    }
    assert_eq!(d3d_com(&mut v, device, 75, &[DATA + 0x940]), 0);
    assert_eq!(d3d_com(&mut v, device, 43, &[0, 0, 1, 0xff0000ff, 0, 0]), 0);
    for (i, [x, y, u, t]) in [
        [-0.5f32, -0.5, 0., 0.],
        [3.5, -0.5, 1., 0.],
        [-0.5, 3.5, 0., 1.],
        [3.5, 3.5, 1., 1.],
    ]
    .into_iter()
    .enumerate()
    {
        let words = [
            x.to_bits(),
            y.to_bits(),
            1f32.to_bits(),
            1f32.to_bits(),
            0xffffffff,
            u.to_bits(),
            t.to_bits(),
        ];
        for (j, n) in words.into_iter().enumerate() {
            v.mem
                .write_u32(DATA + 0xa00 + i as u32 * 28 + j as u32 * 4, n)
                .unwrap();
        }
    }
    assert_eq!(d3d_com(&mut v, device, 41, &[]), 0);
    assert_eq!(d3d_com(&mut v, device, 83, &[5, 2, DATA + 0xa00, 28]), 0);
    assert_eq!(d3d_com(&mut v, device, 42, &[]), 0);
    assert_eq!(d3d_com(&mut v, device, 18, &[0, 0, 0, DATA + 0x900]), 0);
    let back = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_eq!(d3d_com(&mut v, back, 13, &[DATA + 0x910, 0, 0x10]), 0);
    let pixels = v.mem.read_u32(DATA + 0x914).unwrap();
    for y in 0..4 {
        for x in 0..4 {
            assert_eq!(
                v.mem.read_u32(pixels + (y * 4 + x) * 4).unwrap(),
                if (1..3).contains(&x) && (1..3).contains(&y) {
                    0xbf8080ff
                } else {
                    0xff0000ff
                }
            );
        }
    }
    d3d_com(&mut v, back, 14, &[]);
    d3d_com(&mut v, back, 2, &[]);
    d3d_com(&mut v, texture, 2, &[]);
    d3d_com(&mut v, device, 2, &[]);
    d3d_com(&mut v, factory, 2, &[]);
}
#[test]
fn unsupported_audio_returns_real_failure_and_never_a_success_with_null_object() {
    let mut v = Vm::new();
    v.mem.write_u32(DATA, 0xdeadbeef).unwrap();
    assert_eq!(
        v.call("dsound.dll", "DirectSoundCreate", &[0, DATA, 1]),
        0x80040110
    );
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 0);
    assert_eq!(v.call("winmm.dll", "waveOutGetNumDevs", &[]), 0);
    v.mem.write_u32(DATA, 0xdeadbeef).unwrap();
    assert_eq!(
        v.call(
            "winmm.dll",
            "waveOutOpen",
            &[DATA, u32::MAX, DATA + 100, 0, 0, 0]
        ),
        6
    );
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 0);
}
#[test]
fn directsound_notify_identity_keeps_buffer_alive_and_stop_wakes_event() {
    let mut v = Vm::new();
    assert_eq!(
        v.call("dsound.dll", "DirectSoundCreate", &[0, DATA + 0x900, 0]),
        0
    );
    let sound = v.mem.read_u32(DATA + 0x900).unwrap();
    for (i, n) in [20u32, 0x100, 64, 0, DATA + 0x100].into_iter().enumerate() {
        v.mem.write_u32(DATA + i as u32 * 4, n).unwrap();
    }
    v.mem
        .write_bytes(
            DATA + 0x100,
            &[
                1, 0, 1, 0, 0x44, 0xac, 0, 0, 0x88, 0x58, 1, 0, 2, 0, 16, 0, 0, 0,
            ],
        )
        .unwrap();
    assert_eq!(d3d_com(&mut v, sound, 3, &[DATA, DATA + 0x910, 0]), 0);
    let buffer = v.mem.read_u32(DATA + 0x910).unwrap();
    let notify_iid = [
        0x83, 7, 0x21, 0xb0, 0xcd, 0x89, 0xd0, 0x11, 0xaf, 8, 0, 0xa0, 0xc9, 0x25, 0xcd, 0x16,
    ];
    v.mem.write_bytes(DATA + 0x300, &notify_iid).unwrap();
    assert_eq!(d3d_com(&mut v, buffer, 0, &[DATA + 0x300, DATA + 0x910]), 0);
    let notify = v.mem.read_u32(DATA + 0x910).unwrap();
    assert_eq!(d3d_com(&mut v, buffer, 0, &[DATA + 0x300, DATA + 0x910]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x910).unwrap(), notify);
    assert_eq!(d3d_com(&mut v, notify, 2, &[]), 1);
    v.mem
        .write_bytes(
            DATA + 0x300,
            &[
                0x85, 0xfa, 0x9a, 0x27, 0x81, 0x49, 0xce, 0x11, 0xa5, 0x21, 0, 0x20, 0xaf, 0xb,
                0xe5, 0x60,
            ],
        )
        .unwrap();
    assert_eq!(d3d_com(&mut v, notify, 0, &[DATA + 0x300, DATA + 0x910]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x910).unwrap(), buffer);
    d3d_com(&mut v, buffer, 2, &[]);
    let event = v.k("CreateEventW", &[0, 0, 0, 0]);
    v.mem.write_u32(DATA + 0x200, u32::MAX).unwrap();
    v.mem.write_u32(DATA + 0x204, event).unwrap();
    assert_eq!(d3d_com(&mut v, notify, 3, &[1, DATA + 0x200]), 0);
    assert_eq!(d3d_com(&mut v, buffer, 12, &[0, 0, 1]), 0);
    assert_eq!(d3d_com(&mut v, buffer, 18, &[]), 0);
    assert_eq!(v.k("WaitForSingleObject", &[event, 0]), 0);
    assert_eq!(d3d_com(&mut v, buffer, 2, &[]), 1);
    assert_ne!(
        v.k("HeapSize", &[v.state.process_heap, 0, buffer]),
        u32::MAX
    );
    assert_eq!(d3d_com(&mut v, notify, 2, &[]), 0);
    assert_eq!(
        v.k("HeapSize", &[v.state.process_heap, 0, buffer]),
        u32::MAX
    );
    d3d_com(&mut v, sound, 2, &[]);
}

#[test]
fn directsound_pcm_buffers_check_wrapped_locks_controls_and_release_storage() {
    let mut v = Vm::new();
    assert_eq!(
        v.call("dsound.dll", "DirectSoundCreate", &[0, DATA + 0x900, 0]),
        0
    );
    let sound = v.mem.read_u32(DATA + 0x900).unwrap();
    v.mem.write_u32(DATA, 20).unwrap();
    v.mem.write_u32(DATA + 4, 0xe0).unwrap();
    v.mem.write_u32(DATA + 8, 8).unwrap();
    v.mem.write_u32(DATA + 16, DATA + 0x100).unwrap();
    v.mem
        .write_bytes(
            DATA + 0x100,
            &[
                1, 0, 1, 0, 0x44, 0xac, 0, 0, 0x88, 0x58, 1, 0, 2, 0, 16, 0, 0, 0,
            ],
        )
        .unwrap();
    assert_eq!(d3d_com(&mut v, sound, 3, &[DATA, DATA + 0x910, 0]), 0);
    let buffer = v.mem.read_u32(DATA + 0x910).unwrap();
    assert_ne!(buffer, 0);
    assert_eq!(
        d3d_com(
            &mut v,
            buffer,
            11,
            &[
                6,
                4,
                DATA + 0x920,
                DATA + 0x924,
                DATA + 0x928,
                DATA + 0x92c,
                0
            ]
        ),
        0
    );
    let first = v.mem.read_u32(DATA + 0x920).unwrap();
    let second = v.mem.read_u32(DATA + 0x928).unwrap();
    assert_eq!(first, second + 6);
    assert_eq!(v.mem.read_u32(DATA + 0x924).unwrap(), 2);
    assert_eq!(v.mem.read_u32(DATA + 0x92c).unwrap(), 2);
    assert_eq!(
        d3d_com(&mut v, buffer, 19, &[first, 3, second, 2]),
        0x80070057
    );
    v.mem.write_u16(first, 0x7fff).unwrap();
    v.mem.write_u16(second, 0x8000).unwrap();
    assert_eq!(d3d_com(&mut v, buffer, 19, &[first, 2, second, 2]), 0);
    assert_eq!(
        d3d_com(
            &mut v,
            buffer,
            11,
            &[7, 9, DATA + 0x920, DATA + 0x924, 0, 0, 0]
        ),
        0x80070057
    );
    assert_eq!(d3d_com(&mut v, buffer, 15, &[1]), 0x80070057);
    assert_eq!(d3d_com(&mut v, buffer, 15, &[(-600i32) as u32]), 0);
    assert_eq!(d3d_com(&mut v, buffer, 6, &[DATA + 0x930]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x930).unwrap() as i32, -600);
    assert_eq!(d3d_com(&mut v, buffer, 12, &[0, 0, 1]), 0);
    assert_eq!(d3d_com(&mut v, buffer, 9, &[DATA + 0x930]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x930).unwrap(), 5);
    assert_eq!(d3d_com(&mut v, buffer, 18, &[]), 0);
    assert_eq!(d3d_com(&mut v, buffer, 2, &[]), 0);
    assert_eq!(
        v.k("HeapSize", &[v.state.process_heap, 0, second]),
        u32::MAX
    );
    v.mem.write_u32(DATA + 4, 16).unwrap();
    assert_eq!(
        d3d_com(&mut v, sound, 3, &[DATA, DATA + 0x910, 0]),
        0x80004001
    );
    assert_eq!(v.mem.read_u32(DATA + 0x910).unwrap(), 0);
    assert_eq!(d3d_com(&mut v, sound, 2, &[]), 0);
}
#[test]
fn message_object_waits_preserve_queue_masks_old_input_and_atomic_acquisition() {
    let mut v = Vm::new();
    let tid = v.sched.current_tid();
    let event = v.k("CreateEventW", &[0, 0, 0, 0]);
    v.mem.write_u32(DATA, event).unwrap();
    assert_eq!(
        v.call(
            "user32.dll",
            "MsgWaitForMultipleObjects",
            &[1, DATA, 0, 0, 8]
        ),
        0x102
    );
    assert_eq!(
        v.call("user32.dll", "PostThreadMessageW", &[tid, 0x400, 0, 0]),
        1
    );
    assert_eq!(
        v.call(
            "user32.dll",
            "MsgWaitForMultipleObjects",
            &[1, DATA, 0, 0, 8]
        ),
        1
    );
    assert_eq!(
        v.call("user32.dll", "PeekMessageW", &[DATA + 0x100, 0, 0, 0, 0]),
        1
    );
    assert_eq!(
        v.call(
            "user32.dll",
            "MsgWaitForMultipleObjects",
            &[1, DATA, 0, 0, 8]
        ),
        0x102
    );
    assert_eq!(
        v.call(
            "user32.dll",
            "MsgWaitForMultipleObjectsEx",
            &[1, DATA, 0, 8, 4]
        ),
        1
    );
    assert_eq!(v.k("SetEvent", &[event]), 1);
    assert_eq!(
        v.call(
            "user32.dll",
            "MsgWaitForMultipleObjectsEx",
            &[1, DATA, 0, 8, 5]
        ),
        0
    );
    assert_eq!(v.k("WaitForSingleObject", &[event, 0]), 0x102);
    assert_eq!(
        v.call(
            "user32.dll",
            "MsgWaitForMultipleObjects",
            &[64, DATA, 0, 0, 8]
        ),
        u32::MAX
    );
    assert_eq!(v.state.last_error, 87);
    v.mem.write_u32(DATA, 0xdeadbeef).unwrap();
    assert_eq!(
        v.call(
            "user32.dll",
            "MsgWaitForMultipleObjects",
            &[1, DATA, 0, 0, 8]
        ),
        u32::MAX
    );
    assert_eq!(v.state.last_error, 6);
}
#[test]
fn validated_paint_drops_only_synthetic_messages_and_default_close_destroys_window() {
    let mut v = Vm::new();
    v.mem
        .write_bytes(CODE, &[0xb8, 1, 0, 0, 0, 0xc2, 16, 0])
        .unwrap();
    let hwnd = test_window(&mut v, CODE);
    v.call("user32.dll", "ShowWindow", &[hwnd, 5]);
    assert_eq!(
        v.call(
            "user32.dll",
            "PeekMessageW",
            &[DATA + 0x900, hwnd, 0xf, 0xf, 0]
        ),
        1
    );
    v.call("user32.dll", "ValidateRect", &[hwnd, 0]);
    assert_eq!(
        v.call(
            "user32.dll",
            "PeekMessageW",
            &[DATA + 0x900, hwnd, 0xf, 0xf, 0]
        ),
        0
    );
    v.call("user32.dll", "PostMessageW", &[hwnd, 0xf, 0, 0]);
    v.call("user32.dll", "ValidateRect", &[hwnd, 0]);
    assert_eq!(
        v.call(
            "user32.dll",
            "PeekMessageW",
            &[DATA + 0x900, hwnd, 0xf, 0xf, 1]
        ),
        1
    );
    assert_eq!(
        v.call("user32.dll", "DefWindowProcW", &[hwnd, 0x10, 0, 0]),
        1
    );
    assert_eq!(v.call("user32.dll", "IsWindow", &[hwnd]), 0);
}

#[test]
fn d3d9_pixel_shader_executes_constants_masks_and_preserves_bound_lifetime() {
    let mut v = Vm::new();
    let (factory, device, _) = d3d_device(&mut v);
    // def c0, (0.25,0.5,0.75,1); mov oC0,c0
    let tokens = [
        0xffff0200,
        0x05000051,
        0xa00f0000,
        0.25f32.to_bits(),
        0.5f32.to_bits(),
        0.75f32.to_bits(),
        1f32.to_bits(),
        0x02000001,
        0x800f0800,
        0xa0e40000,
        0xffff,
    ];
    let p = DATA + 0x200;
    v.mem
        .write_bytes(
            p,
            &tokens
                .iter()
                .flat_map(|n| n.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
    assert_eq!(d3d_com(&mut v, device, 106, &[p, DATA + 0x900]), 0);
    let shader = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_eq!(d3d_com(&mut v, shader, 4, &[0, DATA + 0x910]), 0);
    assert_eq!(
        v.mem.read_u32(DATA + 0x910).unwrap(),
        tokens.len() as u32 * 4
    );
    v.mem.write_u32(DATA + 0x910, 4).unwrap();
    assert_eq!(
        d3d_com(&mut v, shader, 4, &[DATA + 0x300, DATA + 0x910]),
        0x88760867
    );
    assert_eq!(d3d_com(&mut v, shader, 4, &[DATA + 0x300, DATA + 0x910]), 0);
    let mut copy = vec![0; tokens.len() * 4];
    v.mem.read_bytes(DATA + 0x300, &mut copy).unwrap();
    assert_eq!(
        copy,
        tokens
            .iter()
            .flat_map(|n| n.to_le_bytes())
            .collect::<Vec<_>>()
    );
    assert_eq!(d3d_com(&mut v, device, 107, &[shader]), 0);
    assert_eq!(d3d_com(&mut v, shader, 2, &[]), 0);
    assert_eq!(d3d_com(&mut v, device, 108, &[DATA + 0x900]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x900).unwrap(), shader);
    assert_eq!(d3d_com(&mut v, shader, 2, &[]), 0);
    // Constants set atomically, local def shadows device c0.
    let c = [0f32, 0., 0., 0.];
    v.mem
        .write_bytes(
            DATA + 0x400,
            &c.iter().flat_map(|n| n.to_le_bytes()).collect::<Vec<_>>(),
        )
        .unwrap();
    assert_eq!(d3d_com(&mut v, device, 109, &[0, DATA + 0x400, 1]), 0);
    assert_eq!(
        d3d_com(&mut v, device, 109, &[31, DATA + 0x400, 2]),
        0x8876086c
    );
    assert_eq!(d3d_com(&mut v, device, 110, &[0, DATA + 0x410, 1]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x410).unwrap(), 0);
    for (i, values) in [
        [0f32, 0., 0.5, 1., 0., 0.],
        [4., 0., 0.5, 1., 1., 0.],
        [0., 4., 0.5, 1., 0., 1.],
    ]
    .into_iter()
    .enumerate()
    {
        v.mem
            .write_bytes(
                DATA + i as u32 * 24,
                &values
                    .iter()
                    .flat_map(|n| n.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
    }
    assert_eq!(d3d_com(&mut v, device, 89, &[0x104]), 0);
    d3d_com(&mut v, device, 57, &[22, 1]);
    d3d_com(&mut v, device, 57, &[168, 5]); // only R and B
    d3d_com(
        &mut v,
        device,
        43,
        &[0, 0, 1, 0xff123456, 1f32.to_bits(), 0],
    );
    d3d_com(&mut v, device, 41, &[]);
    assert_eq!(d3d_com(&mut v, device, 83, &[4, 1, DATA, 24]), 0);
    d3d_com(&mut v, device, 42, &[]);
    d3d_com(&mut v, device, 18, &[0, 0, 0, DATA + 0x900]);
    let back = v.mem.read_u32(DATA + 0x900).unwrap();
    d3d_com(&mut v, back, 13, &[DATA + 0x910, 0, 0x10]);
    let bits = v.mem.read_u32(DATA + 0x914).unwrap();
    assert_eq!(v.mem.read_u32(bits).unwrap(), 0xff4034bf);
    d3d_com(&mut v, back, 14, &[]);
    assert_eq!(d3d_com(&mut v, device, 107, &[0]), 0);
    // Malformed stream cannot publish an object.
    v.mem.write_u32(p + 4, 0x04000001).unwrap();
    assert_ne!(d3d_com(&mut v, device, 106, &[p, DATA + 0x900]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x900).unwrap(), 0);
    for o in [back, device, factory] {
        d3d_com(&mut v, o, 2, &[]);
    }
}

#[test]
fn d3d9_recorded_stateblocks_isolate_live_state_capture_only_members_and_hold_resources() {
    let mut v = Vm::new();
    let (factory, device, _) = d3d_device(&mut v);
    assert_eq!(d3d_com(&mut v, device, 61, &[DATA + 0x900]), 0x8876086c);
    assert_eq!(d3d_com(&mut v, device, 60, &[]), 0);
    assert_eq!(d3d_com(&mut v, device, 60, &[]), 0x8876086c);
    assert_eq!(d3d_com(&mut v, device, 59, &[1, DATA + 0x900]), 0x8876086c);
    assert_eq!(d3d_com(&mut v, device, 57, &[22, 1]), 0);
    assert_eq!(d3d_com(&mut v, device, 57, &[168, 3]), 0);
    assert_eq!(d3d_com(&mut v, device, 57, &[168, 16]), 0x8876086a);
    assert_eq!(d3d_com(&mut v, device, 58, &[22, DATA]), 0);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 3); // getter sees live state
    assert_eq!(
        d3d_com(&mut v, device, 23, &[1, 1, 1, 0, 21, 1, DATA + 0x900, 0]),
        0
    );
    let texture = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_eq!(d3d_com(&mut v, device, 65, &[1, texture]), 0);
    assert_eq!(d3d_com(&mut v, texture, 2, &[]), 0); // recording owns the resource
    assert_eq!(d3d_com(&mut v, device, 64, &[1, DATA + 0x900]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x900).unwrap(), 0);
    assert_eq!(d3d_com(&mut v, device, 61, &[0]), 0x8876086c); // keep recording on bad output
    assert_eq!(d3d_com(&mut v, device, 61, &[DATA + 0x900]), 0);
    let block = v.mem.read_u32(DATA + 0x900).unwrap();
    d3d_com(&mut v, device, 57, &[27, 1]); // unrecorded blend-enable
    assert_eq!(d3d_com(&mut v, block, 5, &[]), 0);
    d3d_com(&mut v, device, 58, &[22, DATA]);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 1);
    d3d_com(&mut v, device, 58, &[168, DATA]);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 3);
    d3d_com(&mut v, device, 58, &[27, DATA]);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 1);
    d3d_com(&mut v, device, 64, &[1, DATA]);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), texture);
    d3d_com(&mut v, texture, 2, &[]);
    d3d_com(&mut v, device, 57, &[22, 2]);
    assert_eq!(d3d_com(&mut v, block, 4, &[]), 0);
    d3d_com(&mut v, device, 57, &[22, 3]);
    assert_eq!(d3d_com(&mut v, block, 5, &[]), 0);
    d3d_com(&mut v, device, 58, &[22, DATA]);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 2);
    assert_eq!(d3d_com(&mut v, block, 3, &[DATA]), 0);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), device);
    d3d_com(&mut v, device, 2, &[]);
    assert_eq!(d3d_com(&mut v, block, 2, &[]), 0);
    assert_eq!(d3d_com(&mut v, device, 64, &[1, DATA]), 0);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), texture); // live binding survives block release
    d3d_com(&mut v, texture, 2, &[]);
    d3d_com(&mut v, device, 65, &[1, 0]);
    assert_eq!(
        v.call("d3d9.dll", "IDirect3DTexture9.AddRef", &[texture]),
        0x8876086c
    );
    for o in [device, factory] {
        d3d_com(&mut v, o, 2, &[]);
    }
}

#[test]
fn d3d9_all_stateblocks_restore_defaults_constants_and_exclude_render_targets() {
    let mut v = Vm::new();
    let (factory, device, _) = d3d_device(&mut v);
    assert_eq!(d3d_com(&mut v, device, 59, &[1, DATA + 0x900]), 0);
    let block = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_eq!(
        d3d_com(&mut v, device, 28, &[8, 4, 21, 0, 0, 0, DATA + 0x900, 0]),
        0
    );
    let target = v.mem.read_u32(DATA + 0x900).unwrap();
    d3d_com(&mut v, device, 37, &[0, target]);
    d3d_com(&mut v, device, 57, &[22, 1]);
    d3d_com(&mut v, device, 69, &[1, 3, 3]); // address W is stored for 2D textures
    v.mem.write_bytes(DATA, &[0x55; 16]).unwrap();
    d3d_com(&mut v, device, 109, &[5, DATA, 1]);
    assert_eq!(d3d_com(&mut v, block, 5, &[]), 0);
    d3d_com(&mut v, device, 38, &[0, DATA]);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), target);
    d3d_com(&mut v, target, 2, &[]);
    d3d_com(&mut v, device, 48, &[DATA]);
    assert_eq!(v.mem.read_u32(DATA + 8).unwrap(), 4); // saved viewport, not target extent
    d3d_com(&mut v, device, 58, &[22, DATA]);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 3);
    d3d_com(&mut v, device, 68, &[1, 3, DATA]);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 1);
    d3d_com(&mut v, device, 110, &[5, DATA, 1]);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 0);
    assert_eq!(d3d_com(&mut v, device, 59, &[0, DATA]), 0x8876086c);
    for o in [block, target, device, factory] {
        d3d_com(&mut v, o, 2, &[]);
    }
}

#[cfg(target_os = "macos")]
#[test]
fn gdi_font_metrics_measure_the_selected_font_and_check_output() {
    let mut v = Vm::new();
    let dc = v.call("gdi32.dll", "CreateCompatibleDC", &[0]);
    assert_eq!(v.call("gdi32.dll", "SetMapMode", &[dc, 1]), 1);
    assert_eq!(v.call("gdi32.dll", "GetMapMode", &[dc]), 1);
    assert_eq!(v.call("gdi32.dll", "SetMapMode", &[dc, 2]), 0);
    let mut heights = Vec::new();
    for size in [12i32, 36] {
        let mut lf = [0; 92];
        lf[..4].copy_from_slice(&(-size).to_le_bytes());
        lf[16..20].copy_from_slice(&400u32.to_le_bytes());
        for (i, c) in "Helvetica".encode_utf16().enumerate() {
            lf[28 + i * 2..30 + i * 2].copy_from_slice(&c.to_le_bytes());
        }
        v.mem.write_bytes(DATA, &lf).unwrap();
        let font = v.call("gdi32.dll", "CreateFontIndirectW", &[DATA]);
        assert_ne!(font, 0);
        let old = v.call("gdi32.dll", "SelectObject", &[dc, font]);
        assert_ne!(old, 0);
        v.mem.write_bytes(DATA + 0x100, &[0x55; 64]).unwrap();
        assert_eq!(
            v.call("gdi32.dll", "GetTextMetricsW", &[dc, DATA + 0x100]),
            1
        );
        let height = v.mem.read_u32(DATA + 0x100).unwrap();
        let ascent = v.mem.read_u32(DATA + 0x104).unwrap();
        let descent = v.mem.read_u32(DATA + 0x108).unwrap();
        assert_eq!(height, ascent + descent);
        assert!(height > 0 && ascent > 0);
        assert!(v.mem.read_u32(DATA + 0x114).unwrap() > 0);
        assert_eq!(v.mem.read_u32(DATA + 0x13c).unwrap(), 0x55555555);
        heights.push(height);
        assert_eq!(v.call("gdi32.dll", "GetTextMetricsW", &[dc, 0]), 0);
        assert_eq!(v.call("gdi32.dll", "SelectObject", &[dc, old]), font);
        assert_eq!(v.call("gdi32.dll", "DeleteObject", &[font]), 1);
    }
    assert!(heights[1] > heights[0] * 2);
    assert_eq!(v.call("gdi32.dll", "GetTextMetricsW", &[0, DATA]), 0);
    v.call("gdi32.dll", "DeleteDC", &[dc]);
}

#[test]
fn d3d9_vs2_dp4_constants_clip_position_and_forward_color_to_ps2() {
    let mut v = Vm::new();
    let (factory, device, _) = d3d_device(&mut v);
    let tokens = [
        0xfffe0200u32,
        0x0200001f,
        0x80000000,
        0x900f0000,
        0x0200001f,
        0x80000005,
        0x900f0001,
        0x0200001f,
        0x8000000a,
        0x900f0002,
        0x03000009,
        0xc0010000,
        0x90e40000,
        0xa0e40000,
        0x03000009,
        0xc0020000,
        0x90e40000,
        0xa0e40001,
        0x03000009,
        0xc0040000,
        0x90e40000,
        0xa0e40002,
        0x03000009,
        0xc0080000,
        0x90e40000,
        0xa0e40003,
        0x02000001,
        0xe0030000,
        0x90e40001,
        0x02000001,
        0xd00f0000,
        0x90e40002,
        0xffff,
    ];
    let vp = [0u32, 0, 8, 8, 0, 1f32.to_bits()];
    v.mem
        .write_bytes(
            DATA + 0x800,
            &vp.iter().flat_map(|n| n.to_le_bytes()).collect::<Vec<_>>(),
        )
        .unwrap();
    assert_eq!(d3d_com(&mut v, device, 47, &[DATA + 0x800]), 0);
    v.mem.write_u32(DATA + 0x808, 0).unwrap();
    v.mem.write_u32(DATA + 0x80c, 0).unwrap();
    assert_eq!(d3d_com(&mut v, device, 47, &[DATA + 0x800]), 0);
    assert_eq!(d3d_com(&mut v, device, 48, &[DATA + 0x800]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x808).unwrap(), 0);
    v.mem.write_u32(DATA + 0x808, 4).unwrap();
    v.mem.write_u32(DATA + 0x80c, 4).unwrap();
    assert_eq!(d3d_com(&mut v, device, 47, &[DATA + 0x800]), 0);
    let p = DATA + 0x200;
    v.mem
        .write_bytes(
            p,
            &tokens
                .iter()
                .flat_map(|n| n.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
    assert_eq!(d3d_com(&mut v, device, 91, &[p, DATA + 0x900]), 0);
    let vs = v.mem.read_u32(DATA + 0x900).unwrap();
    let decl = [
        0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 16, 0, 1, 0, 5, 0, 0, 0, 24, 0, 4, 0, 10, 0, 255, 0, 0, 0,
        17, 0, 0, 0,
    ];
    v.mem.write_bytes(DATA + 0x600, &decl).unwrap();
    assert_eq!(
        d3d_com(&mut v, device, 86, &[DATA + 0x600, DATA + 0x900]),
        0
    );
    let declaration = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_eq!(d3d_com(&mut v, device, 87, &[declaration]), 0);
    assert_eq!(d3d_com(&mut v, device, 92, &[vs]), 0);
    let matrix = [
        0.5f32, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ];
    v.mem
        .write_bytes(
            DATA + 0x400,
            &matrix
                .iter()
                .flat_map(|n| n.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
    assert_eq!(d3d_com(&mut v, device, 94, &[0, DATA + 0x400, 4]), 0);
    assert_eq!(
        d3d_com(&mut v, device, 94, &[255, DATA + 0x400, 2]),
        0x8876086c
    );
    let ps = [
        0xffff0200u32,
        0x0200001f,
        0x80000000,
        0x900f0000,
        0x02000001,
        0x800f0800,
        0x90e40000,
        0xffff,
    ];
    v.mem
        .write_bytes(
            DATA + 0x700,
            &ps.iter().flat_map(|n| n.to_le_bytes()).collect::<Vec<_>>(),
        )
        .unwrap();
    assert_eq!(
        d3d_com(&mut v, device, 106, &[DATA + 0x700, DATA + 0x900]),
        0
    );
    let shader = v.mem.read_u32(DATA + 0x900).unwrap();
    d3d_com(&mut v, device, 107, &[shader]);
    for (i, point) in [
        [-1f32, 1., 0.5, 1., 0., 0.],
        [1., 1., 0.5, 1., 1., 0.],
        [-1., -1., 0.5, 1., 0., 1.],
    ]
    .into_iter()
    .enumerate()
    {
        v.mem
            .write_bytes(
                DATA + i as u32 * 28,
                &point
                    .iter()
                    .flat_map(|n| n.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        v.mem
            .write_u32(DATA + i as u32 * 28 + 24, 0xff4080bf)
            .unwrap();
    }
    d3d_com(&mut v, device, 43, &[0, 0, 1, 0, 1f32.to_bits(), 0]);
    d3d_com(&mut v, device, 41, &[]);
    assert_eq!(d3d_com(&mut v, device, 83, &[4, 1, DATA, 28]), 0);
    d3d_com(&mut v, device, 42, &[]);
    d3d_com(&mut v, device, 18, &[0, 0, 0, DATA + 0x900]);
    let back = v.mem.read_u32(DATA + 0x900).unwrap();
    d3d_com(&mut v, back, 13, &[DATA + 0x910, 0, 0x10]);
    let bits = v.mem.read_u32(DATA + 0x914).unwrap();
    assert_eq!(v.mem.read_u32(bits + 5 * 4).unwrap(), 0xff4080bf);
    assert_eq!(v.mem.read_u32(bits + 4 * 4).unwrap(), 0);
    d3d_com(&mut v, back, 14, &[]);
    // Switching to pretransformed input bypasses the still-bound VS. Its
    // matrix would collapse these screen coordinates if it were executed.
    assert_eq!(d3d_com(&mut v, device, 89, &[0x144]), 0);
    d3d_com(&mut v, device, 57, &[22, 1]);
    for (i, xy) in [[0f32, 0.], [4., 0.], [0., 4.]].into_iter().enumerate() {
        let p = DATA + i as u32 * 28;
        for (j, n) in [xy[0], xy[1], 0.5, 1.].into_iter().enumerate() {
            v.mem.write_u32(p + j as u32 * 4, n.to_bits()).unwrap();
        }
        v.mem.write_u32(p + 16, 0xffc04080).unwrap();
        v.mem.write_u32(p + 20, 0).unwrap();
        v.mem.write_u32(p + 24, 0).unwrap();
    }
    assert_eq!(d3d_com(&mut v, device, 69, &[0, 7, 1]), 0);
    assert_eq!(d3d_com(&mut v, device, 69, &[0, 7, 2]), 0);
    assert_eq!(d3d_com(&mut v, device, 68, &[0, 7, DATA + 0x920]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x920).unwrap(), 2);
    d3d_com(&mut v, device, 43, &[0, 0, 1, 0, 1f32.to_bits(), 0]);
    d3d_com(&mut v, device, 41, &[]);
    assert_eq!(d3d_com(&mut v, device, 83, &[4, 1, DATA, 28]), 0);
    d3d_com(&mut v, device, 42, &[]);
    d3d_com(&mut v, back, 13, &[DATA + 0x910, 0, 0x10]);
    assert_eq!(v.mem.read_u32(bits + 5 * 4).unwrap(), 0xffc04080);
    d3d_com(&mut v, back, 14, &[]);
    assert_eq!(d3d_com(&mut v, device, 93, &[DATA + 0x920]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x920).unwrap(), vs);
    d3d_com(&mut v, vs, 2, &[]);
    d3d_com(&mut v, device, 107, &[0]);
    // Null stage-zero texture disables the default MODULATE stage, retaining
    // diffuse RGBA instead of failing or sampling a fabricated texture.
    assert_eq!(d3d_com(&mut v, device, 69, &[0, 12, 0]), 0);
    assert_eq!(d3d_com(&mut v, device, 69, &[0, 13, 0]), 0);
    assert_eq!(d3d_com(&mut v, device, 69, &[0, 12, 1]), 0x8876086a);
    d3d_com(&mut v, device, 43, &[0, 0, 1, 0, 1f32.to_bits(), 0]);
    d3d_com(&mut v, device, 41, &[]);
    assert_eq!(d3d_com(&mut v, device, 83, &[4, 1, DATA, 28]), 0);
    d3d_com(&mut v, device, 42, &[]);
    d3d_com(&mut v, back, 13, &[DATA + 0x910, 0, 0x10]);
    assert_eq!(v.mem.read_u32(bits + 5 * 4).unwrap(), 0xffc04080);
    d3d_com(&mut v, back, 14, &[]);
    d3d_com(&mut v, device, 92, &[0]);
    d3d_com(&mut v, device, 87, &[0]);
    for o in [back, shader, vs, declaration, device, factory] {
        d3d_com(&mut v, o, 2, &[]);
    }
}

#[test]
fn gdi_bitfields_dib_reports_masks_and_keeps_checked_pixels() {
    let mut v = Vm::new();
    let dc = v.call("gdi32.dll", "CreateCompatibleDC", &[0]);
    let info = [
        40u32,
        3,
        (-2i32) as u32,
        0x200001,
        3,
        24,
        0,
        0,
        0,
        0,
        0xff0000,
        0xff00,
        0xff,
    ];
    v.mem
        .write_bytes(
            DATA,
            &info
                .iter()
                .flat_map(|n| n.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
    let bitmap = v.call(
        "gdi32.dll",
        "CreateDIBSection",
        &[dc, DATA, 0, DATA + 0x100, 0, 0],
    );
    assert_ne!(bitmap, 0);
    let bits = v.mem.read_u32(DATA + 0x100).unwrap();
    v.mem.write_u32(bits, 0x11223344).unwrap();
    let old = v.call("gdi32.dll", "SelectObject", &[dc, bitmap]);
    assert_ne!(old, 0);
    assert_eq!(v.call("gdi32.dll", "GetPixel", &[dc, 0, 0]), 0x443322);
    assert_eq!(
        v.call("gdi32.dll", "GetObjectW", &[bitmap, 84, DATA + 0x200]),
        84
    );
    assert_eq!(v.mem.read_u32(DATA + 0x228).unwrap(), 3);
    assert_eq!(v.mem.read_u32(DATA + 0x240).unwrap(), 0xff0000);
    assert_eq!(v.mem.read_u32(DATA + 0x248).unwrap(), 0xff);
    v.mem.write_u32(DATA + 40, 0xff).unwrap();
    assert_eq!(
        v.call(
            "gdi32.dll",
            "CreateDIBSection",
            &[dc, DATA, 0, DATA + 0x100, 0, 0]
        ),
        0
    );
    v.call("gdi32.dll", "SelectObject", &[dc, old]);
    v.call("gdi32.dll", "DeleteObject", &[bitmap]);
    v.call("gdi32.dll", "DeleteDC", &[dc]);
}
