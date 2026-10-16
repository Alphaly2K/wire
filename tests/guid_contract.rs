//! Independent GUID behavior fixtures, fixed before the catalog replaces routes.
#[allow(dead_code)]
#[path = "support/x64.rs"]
mod x64;
#[allow(dead_code)]
#[path = "support/x86.rs"]
mod x86;
use wire::win32::catalog::{self, Architecture, Resolution};

const GUID: [u8; 16] = [
    0x33, 0x22, 0x11, 0, 0x55, 0x44, 0x77, 0x66, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
];
const TEXT: &str = "{00112233-4455-6677-8899-AABBCCDDEEFF}";

#[test]
fn wrong_dll_keeps_pe32_legacy_policy_and_win64_rejection() {
    let mut v = x86::Vm::new();
    v.mem.write_bytes(x86::DATA, &GUID).unwrap();
    v.mem.write_bytes(x86::DATA + 0x100, &[0x5a; 78]).unwrap();
    assert_eq!(
        v.call(
            "kernel32.dll",
            "StringFromGUID2",
            &[x86::DATA, x86::DATA + 0x100, 39]
        ),
        0
    );
    assert_eq!(
        v.cpu.esp(),
        wire::loader::STACK_BASE + wire::loader::STACK_SIZE - 252
    );
    let mut untouched = [0; 78];
    v.mem.read_bytes(x86::DATA + 0x100, &mut untouched).unwrap();
    assert_eq!(untouched, [0x5a; 78]);
    let (mut mem, mut cpu, mut runtime) = x64::startup();
    let before = cpu.clone();
    assert!(!wire::x64::runtime::import_supported(
        "kernel32.dll",
        "StringFromGUID2"
    ));
    assert!(runtime
        .dispatch("kernel32.dll", "StringFromGUID2", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu, before);
    assert_eq!(runtime.api_calls, 0);
}

#[test]
fn guid_formatter_preserves_signed_capacity_and_both_architecture_frames() {
    let mut v = x86::Vm::new();
    v.mem.write_bytes(x86::DATA, &GUID).unwrap();
    for capacity in [0, 38, u32::MAX, 39] {
        assert_eq!(
            v.call(
                "ole32.dll",
                "StringFromGUID2",
                &[x86::DATA, x86::DATA + 0x100, capacity]
            ),
            if capacity == 39 { 39 } else { 0 }
        );
        assert_eq!(
            v.cpu.esp(),
            wire::loader::STACK_BASE + wire::loader::STACK_SIZE - 240
        );
    }
    assert_eq!(v.mem.read_wstr(x86::DATA + 0x100, 39).unwrap(), TEXT);
    let (mut mem, mut cpu, mut runtime) = x64::startup();
    mem.write(x64::STACK + 0x100, &GUID).unwrap();
    assert_eq!(
        x64::startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "combase.dll",
            "StringFromGUID2",
            &[x64::STACK + 0x100, x64::STACK + 0x200, 0x100000027]
        ),
        39
    );
    let mut output = [0; 78];
    mem.read(x64::STACK + 0x200, &mut output).unwrap();
    assert_eq!(
        output.to_vec(),
        TEXT.encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    );
}

#[test]
fn invalid_guid_and_output_faults_keep_existing_architecture_errors() {
    let mut v = x86::Vm::new();
    v.wide(x86::DATA, "bad");
    for name in ["CLSIDFromString", "IIDFromString"] {
        v.mem.write_bytes(x86::DATA + 0x100, &[0x5a; 16]).unwrap();
        assert_eq!(
            v.call("ole32.dll", name, &[x86::DATA, x86::DATA + 0x100]),
            0x800401f3
        );
        assert_eq!(v.call("ole32.dll", name, &[x86::DATA, 0]), 0x80004003);
    }
    v.mem.write_bytes(x86::DATA, &GUID).unwrap();
    assert_eq!(
        v.call("ole32.dll", "StringFromGUID2", &[x86::DATA, 0, 39]),
        0
    );
    let (mut mem, mut cpu, mut runtime) = x64::startup();
    mem.write(x64::STACK + 0x100, &[b'b', 0, b'a', 0, b'd', 0, 0, 0])
        .unwrap();
    assert_eq!(
        x64::startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ole32.dll",
            "IIDFromString",
            &[x64::STACK + 0x100, x64::STACK + 0x200]
        ),
        0x80070057
    );
    cpu.regs[1] = x64::STACK + 0x100;
    cpu.regs[2] = 0;
    let before = cpu.clone();
    let calls = runtime.api_calls;
    assert!(runtime
        .dispatch("ole32.dll", "IIDFromString", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu, before);
    assert_eq!(runtime.api_calls, calls);
}

#[test]
fn output_alias_return_slot_preserves_the_original_commit_order() {
    let mut v = x86::Vm::new();
    v.mem.write_bytes(x86::DATA, &GUID).unwrap();
    let sp = wire::loader::STACK_BASE + wire::loader::STACK_SIZE - 256;
    assert_eq!(
        v.call("ole32.dll", "StringFromGUID2", &[x86::DATA, sp, 39]),
        39
    );
    assert_eq!(v.cpu.eip, 0x0030007b); // PE32 reads the return PC after the output write.
    let (mut mem, mut cpu, mut runtime) = x64::startup();
    mem.write(x64::STACK + 0x100, &GUID).unwrap();
    let sp = x64::STACK + 0x1f88;
    assert_eq!(
        x64::startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ole32.dll",
            "StringFromGUID2",
            &[x64::STACK + 0x100, sp, 39]
        ),
        39
    );
    assert_eq!(cpu.rip, x64::BASE); // Win64 captures the return frame before output.
}

#[test]
fn catalog_keeps_dll_scopes_distinct_from_known_abi_and_legacy_entries() {
    for name in ["StringFromGUID2", "CLSIDFromString", "IIDFromString"] {
        assert!(matches!(
            catalog::resolve(Architecture::X86, "OLE32.dll", name),
            Resolution::Bound(_)
        ));
        assert!(matches!(
            catalog::resolve(Architecture::X64, "COMBASE.dll", name),
            Resolution::Bound(_)
        ));
        assert!(matches!(
            catalog::resolve(Architecture::X86, "combase.dll", name),
            Resolution::WrongDll(_)
        ));
        assert!(matches!(
            catalog::resolve(Architecture::X64, "kernel32.dll", name),
            Resolution::WrongDll(_)
        ));
    }
    assert!(matches!(
        catalog::resolve(Architecture::X64, "kernel32.dll", "WireUnregisteredExport"),
        Resolution::Legacy
    ));
}

#[test]
fn changed_public_entries_and_replaced_gateways_rebind_before_execution() {
    let mut v = x86::Vm::new();
    v.wide(x86::DATA, TEXT);
    // Bind a formatter first, then change the legacy public view to a parser.
    let stub = v.api.register("ole32.dll", Some("StringFromGUID2"), None);
    let index = v.api.stub_index(stub).unwrap();
    v.api.entries[index].name = "CLSIDFromString".into();
    assert_eq!(
        v.call(
            "ole32.dll",
            "StringFromGUID2",
            &[x86::DATA, x86::DATA + 0x100]
        ),
        0
    );
    let mut output = [0; 16];
    v.mem.read_bytes(x86::DATA + 0x100, &mut output).unwrap();
    assert_eq!(output, GUID);
    assert_eq!(
        v.cpu.esp(),
        wire::loader::STACK_BASE + wire::loader::STACK_SIZE - 244
    );

    let (mut mem, mut cpu, mut runtime) = x64::startup();
    let stub = runtime.register("kernel32.dll", "GetCurrentThreadId");
    runtime
        .gateways
        .insert(stub, "ole32.dll!CLSIDFromString".into());
    let sp = x64::STACK + 0x1f88;
    mem.write_int(sp, 8, x64::BASE).unwrap();
    cpu.regs[4] = sp;
    cpu.regs[1] = 0;
    cpu.regs[2] = x64::STACK + 0x100;
    cpu.rip = stub;
    runtime.dispatch_gateway(&mut cpu, &mut mem).unwrap();
    mem.read(x64::STACK + 0x100, &mut output).unwrap();
    assert_eq!(output, [0; 16]);
    assert_eq!(cpu.regs[0], 0);
    assert_eq!(cpu.rip, x64::BASE);
}
