//! Existing behavioral tests grouped by memory; expectations remain independent of API catalogs.

use super::*;

#[test]
fn heap_contract_keeps_pe32_errors_enumeration_alias_and_abi_only_extensions() {
    let mut v = Vm::new();
    let process = v.k("GetProcessHeap", &[]);
    assert_eq!(process, 0x70050000);
    let private = v.k("HeapCreate", &[u32::MAX, u32::MAX, 32]);
    assert_ne!(private, process);
    v.k("SetLastError", &[123]);
    let block = v.k("HeapAlloc", &[private, u32::MAX, 23]);
    assert_ne!(block, 0);
    assert_eq!(v.mem.read_u32(block).unwrap(), 0);
    assert_eq!(v.state.last_error, 123);
    assert_eq!(v.k("HeapAlloc", &[private, 0, 16]), 0);
    assert_eq!(v.state.last_error, 8);
    assert_eq!(v.k("HeapFree", &[process, 0, block]), 0);
    assert_eq!(v.state.last_error, 87);
    assert_eq!(v.k("HeapSize", &[process, 0, block]), u32::MAX);
    assert_eq!(v.k("HeapValidate", &[private, u32::MAX, block]), 1);
    assert_eq!(v.state.last_error, 87);
    assert_eq!(v.k("GetProcessHeaps", &[1, 0]), 1); // Legacy ignores copy-out faults.
    assert_eq!(v.state.last_error, 87);
    let sp = STACK_BASE + STACK_SIZE - 256;
    assert_eq!(v.k("GetProcessHeaps", &[1, sp]), 1);
    assert_eq!(v.cpu.eip, process); // Enumeration overwrites the unsaved return slot.
    assert_eq!(v.cpu.esp(), sp + 12);
    assert_eq!(v.k("HeapDestroy", &[process]), 0);
    assert_eq!(v.state.last_error, 87);
    assert_eq!(v.k("HeapDestroy", &[private]), 1);
    assert_eq!(v.k("HeapValidate", &[private, 0, block]), 0);
    assert_eq!(v.state.last_error, 87);
    for (name, args, bytes) in [
        ("HeapSetInformation", vec![process, 0, DATA, 4], 16),
        (
            "HeapQueryInformation",
            vec![process, 0, DATA, 4, DATA + 8],
            20,
        ),
        ("HeapCompact", vec![process, 0], 8),
        ("HeapWalk", vec![process, DATA], 8),
    ] {
        v.mem.write_bytes(DATA, &[0xa5; 16]).unwrap();
        assert_eq!(v.k(name, &args), 0, "{name}");
        assert_eq!(v.state.last_error, 120, "{name}");
        assert_eq!(v.cpu.esp(), sp + 4 + bytes, "{name}");
        let mut output = [0; 16];
        v.mem.read_bytes(DATA, &mut output).unwrap();
        assert_eq!(output, [0xa5; 16], "{name}");
    }
}

#[test]
fn virtual_memory_keeps_pe32_protect_return_alias_and_remote_error_order() {
    let mut v = Vm::new();
    let page = v.k("VirtualAlloc", &[0, 4096, 0x3000, 4]);
    assert_ne!(page, 0);
    v.k("SetLastError", &[123]);
    let sp = STACK_BASE + STACK_SIZE - 256;
    let address = v.api.register("kernel32.dll", Some("VirtualProtect"), None);
    v.cpu.eip = address;
    v.cpu.set_esp(sp);
    v.mem.write_u32(sp, RET).unwrap();
    for (i, value) in [page, 4096, 2, sp].into_iter().enumerate() {
        v.mem.write_u32(sp + 4 + i as u32 * 4, value).unwrap();
    }
    let index = v.api.stub_index(address).unwrap();
    api::dispatch(
        index,
        &mut v.cpu,
        &mut v.mem,
        &mut v.api,
        &mut v.state,
        &mut v.log,
        &mut v.sched,
        1_000_000,
    )
    .unwrap();
    assert_eq!(v.cpu.regs[0], 1);
    assert_eq!(v.cpu.eip, 4); // Output overwrites the unsaved legacy return slot.
    assert_eq!(v.cpu.esp(), sp + 20);
    assert_eq!(v.mem.read_u32(sp).unwrap(), 4);
    assert_eq!(v.mem.virtual_query(page).unwrap().protect, 2);
    assert_eq!(v.state.last_error, 123);
    assert_eq!(v.k("VirtualProtectEx", &[123, page, 4096, 4, 0]), 0);
    assert_eq!(v.state.last_error, 6);
    assert_eq!(v.mem.virtual_query(page).unwrap().protect, 2);
    assert_eq!(v.k("VirtualQueryEx", &[123, page, 0, 0]), 0);
    assert_eq!(v.state.last_error, 6);
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
