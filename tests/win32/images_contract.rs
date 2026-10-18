//! Image APIs keep the PE32 stock-arrow subset and known failure/stack behavior.
use super::*;

#[test]
fn images_contract_pe32_keeps_only_stock_arrow_and_unsupported_calls() {
    let mut v = Vm::new();
    for name in ["LoadCursorA", "LoadCursorW"] {
        v.k("SetLastError", &[77]);
        assert_eq!(v.call("user32.dll", name, &[0, 32512]), 0x42007f00);
        assert_eq!(v.state.last_error, 77);
        for args in [[1, 32512], [0, 32513], [0, DATA]] {
            assert_eq!(v.call("user32.dll", name, &args), 0);
            assert_eq!(v.state.last_error, 1812);
        }
    }
    for (name, args) in [
        ("LoadIconA", vec![0, 32512]),
        ("LoadIconW", vec![0, 32512]),
        ("LoadImageA", vec![0, 32512, 2, 0, 0, 0x8000]),
        ("LoadImageW", vec![0, 32512, 2, 0, 0, 0x8000]),
        ("DestroyIcon", vec![0x42007f00]),
        ("DestroyCursor", vec![0x42007f00]),
    ] {
        v.k("SetLastError", &[77]);
        assert_eq!(v.call("user32.dll", name, &args), 0, "{name}");
        assert_eq!(v.state.last_error, 120, "{name}");
        assert_eq!(
            v.cpu.esp(),
            STACK_BASE + STACK_SIZE - 256 + 4 * (args.len() as u32 + 1),
            "{name}"
        );
        let trace = v.log.recent.back().unwrap();
        assert!(trace.contains(" [UNIMPLEMENTED]"), "{name}: {trace}");
        assert_eq!(
            trace.contains(" ; USER32 error 120"),
            name == "DestroyIcon",
            "{name}: {trace}"
        );
    }
    assert_eq!(v.call("user32.dll", "SetCursor", &[0x42007f00]), 0);
    assert_eq!(v.call("user32.dll", "GetCursor", &[]), 0x42007f00);
}
