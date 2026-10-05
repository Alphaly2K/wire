use wire::ir::Image;

#[test]
fn wasm_lowering_rejects_unknown_control_flow_and_faultable_or_narrow_instructions() {
    for code in [
        &[0x90][..],                               // no backedge
        &[0x8b, 0x03, 0x49, 0x75, 0xfb][..],       // memory access
        &[0x66, 0x01, 0xd0, 0x49, 0x75, 0xfa][..], // narrow arithmetic
        &[0x90, 0x49, 0x75, 0x01][..],             // jump out of region
        &[0x75, 0, 0x49, 0x75, 0xfb][..],          // extra CFG branch
        &[0xe8, 0, 0, 0, 0, 0x49, 0x75, 0xf8][..], // call/ABI boundary
    ] {
        assert!(
            Image::from_code(0x10000, code)
                .unwrap()
                .lower_wasm_loop()
                .is_err(),
            "{code:x?}"
        );
    }
}
