use super::*;

#[test]
fn modern_graphics_exports_keep_pe32_unknown_abi_and_outputs() {
    let mut vm = Vm::new();
    for (name, slots) in [
        ("CreateDXGIFactory", 2),
        ("CreateDXGIFactory1", 2),
        ("CreateDXGIFactory2", 3),
        ("D3D12CreateDevice", 4),
        ("D3D12GetDebugInterface", 2),
        ("D3D11CreateDevice", 10),
        ("D3D11CreateDeviceAndSwapChain", 12),
    ] {
        for dll in [
            "kernel32.dll",
            "dxgi.dll",
            "d3d11.dll",
            "d3d12.dll",
            "wire.dxgi",
        ] {
            assert!(!api::native_export(dll, name), "{dll}!{name}");
            vm.mem.write_u32(DATA, 0xa5a5a5a5).unwrap();
            assert_eq!(vm.call(dll, name, &vec![DATA; slots]), 0);
            assert_eq!(vm.state.last_error, 120);
            assert_eq!(vm.mem.read_u32(DATA).unwrap(), 0xa5a5a5a5);
            assert_eq!(vm.cpu.esp(), STACK_BASE + STACK_SIZE - 252);
        }
    }
}
