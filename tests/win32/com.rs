//! Existing behavioral tests grouped by com; expectations remain independent of API catalogs.

use super::*;

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
