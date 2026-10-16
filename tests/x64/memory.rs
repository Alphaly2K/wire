//! Existing behavioral tests grouped by memory; expectations remain independent of API catalogs.

use super::*;

#[test]
fn local_memory_keeps_win64_aliases_errors_full_width_and_free_frame_preflight() {
    for dll in ["kernel32.dll", "kernelbase.dll"] {
        for prefix in ["Local", "Global"] {
            let (mut mem, mut cpu, mut runtime) = startup();
            let alloc = format!("{prefix}Alloc");
            let free = format!("{prefix}Free");
            let block = startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                dll,
                &alloc,
                &[0xfacefeed00000040, 23],
            );
            assert_ne!(block, 0);
            assert_eq!(mem.read_int(block, 8).unwrap(), 0);
            assert_eq!(
                startup_dll_call(
                    &mut mem,
                    &mut cpu,
                    &mut runtime,
                    dll,
                    &format!("{prefix}Size"),
                    &[block]
                ),
                23
            );
            assert_eq!(
                startup_dll_call(
                    &mut mem,
                    &mut cpu,
                    &mut runtime,
                    dll,
                    &format!("{prefix}Lock"),
                    &[block]
                ),
                block
            );
            assert_eq!(
                startup_dll_call(
                    &mut mem,
                    &mut cpu,
                    &mut runtime,
                    dll,
                    &format!("{prefix}Flags"),
                    &[block]
                ),
                0
            );
            assert_eq!(
                startup_dll_call(
                    &mut mem,
                    &mut cpu,
                    &mut runtime,
                    dll,
                    &format!("{prefix}Unlock"),
                    &[block]
                ),
                0
            );
            assert_eq!(
                mem.read_int(wire::x64::runtime::TEB + 0x68, 4).unwrap(),
                158
            );
            let invalid = 0xfacefeed12345678;
            assert_eq!(
                startup_dll_call(
                    &mut mem,
                    &mut cpu,
                    &mut runtime,
                    dll,
                    &format!("{prefix}Flags"),
                    &[invalid]
                ),
                0x8000
            );
            assert_eq!(mem.read_int(wire::x64::runtime::TEB + 0x68, 4).unwrap(), 6);
            assert_eq!(
                startup_dll_call(&mut mem, &mut cpu, &mut runtime, dll, &free, &[invalid]),
                invalid
            );
            assert_eq!(
                startup_dll_call(&mut mem, &mut cpu, &mut runtime, dll, &alloc, &[2, 23]),
                0
            );
            assert_eq!(mem.read_int(wire::x64::runtime::TEB + 0x68, 4).unwrap(), 50);
            assert_eq!(
                startup_dll_call(
                    &mut mem,
                    &mut cpu,
                    &mut runtime,
                    dll,
                    &alloc,
                    &[0, 0x100000000]
                ),
                0
            );
            assert_eq!(mem.read_int(wire::x64::runtime::TEB + 0x68, 4).unwrap(), 8);
            cpu.regs[4] = 0;
            cpu.regs[1] = block;
            let before = cpu.clone();
            let calls = runtime.api_calls;
            assert!(runtime.dispatch(dll, &free, &mut cpu, &mut mem).is_err());
            assert_eq!(cpu, before);
            assert_eq!(runtime.api_calls, calls);
            assert_eq!(
                startup_dll_call(
                    &mut mem,
                    &mut cpu,
                    &mut runtime,
                    dll,
                    "GlobalSize",
                    &[block]
                ),
                23
            );
            mem.write_int(block, 8, 0x123456789abcdef0).unwrap();
            let grown = startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                dll,
                &format!("{prefix}ReAlloc"),
                &[block, 8192, 0xfacefeed00000042],
            );
            assert_ne!(grown, 0);
            assert_ne!(grown, block);
            assert_eq!(mem.read_int(grown, 8).unwrap(), 0x123456789abcdef0);
            assert_eq!(mem.read_int(grown + 23, 1).unwrap(), 0);
            assert!(mem.read_int(block, 1).is_err());
            assert_eq!(
                startup_dll_call(
                    &mut mem,
                    &mut cpu,
                    &mut runtime,
                    dll,
                    "GlobalFree",
                    &[grown]
                ),
                0
            );
            assert_eq!(mem.read_int(wire::x64::runtime::TEB + 0x68, 4).unwrap(), 8);
        }
    }
}

#[test]
fn heap_contract_keeps_win64_handles_size_t_flags_and_free_frame_preflight() {
    for dll in ["kernel32.dll", "kernelbase.dll"] {
        let (mut mem, mut cpu, mut runtime) = startup();
        let heap = startup_dll_call(&mut mem, &mut cpu, &mut runtime, dll, "GetProcessHeap", &[]);
        assert!(heap > u32::MAX as u64);
        let block = startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            dll,
            "HeapAlloc",
            &[heap, 0xfacefeed00000008, 23],
        );
        assert_ne!(block, 0);
        assert_eq!(mem.read_int(block, 8).unwrap(), 0);
        for args in [[heap as u32 as u64, 0, block], [heap, 2, block]] {
            cpu.regs[4] = STACK + 0x1f88;
            mem.write_int(cpu.regs[4], 8, BASE).unwrap();
            for (reg, value) in [1, 2, 8].into_iter().zip(args) {
                cpu.regs[reg] = value;
            }
            let before = cpu.clone();
            let calls = runtime.api_calls;
            assert!(runtime
                .dispatch(dll, "HeapFree", &mut cpu, &mut mem)
                .is_err());
            assert_eq!(cpu, before);
            assert_eq!(runtime.api_calls, calls);
        }
        cpu.regs[4] = 0;
        cpu.regs[1] = heap;
        cpu.regs[2] = 0;
        cpu.regs[8] = block;
        let before = cpu.clone();
        let calls = runtime.api_calls;
        assert!(runtime
            .dispatch(dll, "HeapFree", &mut cpu, &mut mem)
            .is_err());
        assert_eq!(cpu, before);
        assert_eq!(runtime.api_calls, calls);
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                dll,
                "HeapSize",
                &[heap, 0, block],
            ),
            23
        );
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                dll,
                "HeapAlloc",
                &[heap, 0, 0x100000000],
            ),
            0
        );
        assert_eq!(mem.read_int(wire::x64::runtime::TEB + 0x68, 4).unwrap(), 8);
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                dll,
                "HeapFree",
                &[heap, 0, block],
            ),
            1
        );
        assert_eq!(mem.read_int(wire::x64::runtime::TEB + 0x68, 4).unwrap(), 8);
        for name in [
            "HeapCreate",
            "HeapDestroy",
            "HeapValidate",
            "GetProcessHeaps",
            "HeapSetInformation",
            "HeapQueryInformation",
            "HeapCompact",
            "HeapWalk",
        ] {
            assert!(
                !wire::x64::runtime::import_supported(dll, name),
                "{dll}!{name}"
            );
        }
    }
}

#[test]
fn virtual_memory_keeps_win64_size_t_handles_and_saved_protect_return_alias() {
    for dll in ["kernel32.dll", "kernelbase.dll"] {
        let (mut mem, mut cpu, mut runtime) = startup();
        let high = 0xfacefeed_00000000;
        let page = startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            dll,
            "VirtualAlloc",
            &[0, 4096, high | 0x3000, high | 4],
        );
        assert_ne!(page, 0);
        let output = STACK + 0x100;
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                dll,
                "VirtualQuery",
                &[page, output, 0x100000000]
            ),
            48
        );
        assert_eq!(mem.read_int(output, 8).unwrap(), page);
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                dll,
                "VirtualProtectEx",
                &[high | u32::MAX as u64, page, 4096, 2, 0]
            ),
            0
        );
        assert_eq!(mem.read_int(wire::x64::runtime::TEB + 0x68, 4).unwrap(), 6);

        let sp = STACK + 0x1f88;
        cpu.regs[4] = sp;
        mem.write_int(sp, 8, BASE).unwrap();
        for (reg, value) in [1, 2, 8, 9].into_iter().zip([page, 4096, high | 2, sp]) {
            cpu.regs[reg] = value;
        }
        assert_eq!(
            runtime
                .dispatch(dll, "VirtualProtect", &mut cpu, &mut mem)
                .unwrap(),
            wire::x64::runtime::Control::Returned
        );
        assert_eq!(cpu.regs[0], 1);
        assert_eq!(cpu.rip, BASE); // Win64 saved the return before the output write.
        assert_eq!(cpu.regs[4], sp + 8);
        assert_eq!(mem.read_int(sp, 4).unwrap(), 4);
        assert!(mem.write_int(page, 4, 0).is_err());
        assert_eq!(mem.read_int(wire::x64::runtime::TEB + 0x68, 4).unwrap(), 6);
    }
}

#[test]
fn sparse_high_address_memory_is_checked_and_cross_page_writes_are_atomic() {
    let mut m = Memory::default();
    m.map(BASE, 8192, READ | WRITE).unwrap();
    m.write(BASE + 4094, &[1, 2, 3, 4]).unwrap();
    m.protect(BASE + 4096, 4096, READ).unwrap();
    assert!(m.write(BASE + 4094, &[5, 6, 7, 8]).is_err());
    let mut b = [0; 4];
    m.read(BASE + 4094, &mut b).unwrap();
    assert_eq!(b, [1, 2, 3, 4]);
    assert!(m.fetch(BASE, &mut [0]).is_err());
    assert!(m.read(1 << 47, &mut [0]).is_err());
    assert!(m.read(u64::MAX, &mut [0; 2]).is_err());
    assert!(m.map(BASE + 4096, 8192, READ).is_err());
    assert!(m.read(BASE + 8192, &mut [0]).is_err());
}

#[test]
fn amd64_scalar_memory_widths_check_guards_boundaries_and_atomic_writes() {
    let value = 0xfedcba9876543210u64;
    let mut mem = Memory::default();
    mem.map(STACK, 8192, READ | WRITE).unwrap();
    for size in [1, 2, 4, 8] {
        let expected = value & (u64::MAX >> (64 - size * 8));
        // Unaligned, end-of-page and crossing accesses exercise both paths.
        for at in [STACK + 3, STACK + 4096 - size as u64, STACK + 4095] {
            mem.write_int(at, size, value).unwrap();
            assert_eq!(mem.read_int(at, size).unwrap(), expected);
        }
        mem.protect(STACK + 4096, 4096, READ).unwrap();
        let at = STACK + 4096 - (size / 2) as u64;
        let mut before = [0; 8];
        mem.read(at, &mut before[..size]).unwrap();
        assert!(mem.write_int(at, size, !value).is_err());
        let mut after = [0; 8];
        mem.read(at, &mut after[..size]).unwrap();
        assert_eq!(after, before);
        mem.protect(STACK + 4096, 4096, READ | WRITE).unwrap();
        mem.virtual_protect(STACK + 4096, 4096, 0x104).unwrap();
        assert!(mem.read_int(at, size).unwrap_err().contains("guard"));
        assert_eq!(mem.read_int(at, size).unwrap(), u64::from_le_bytes(before));
        mem.virtual_protect(STACK + 4096, 4096, 0x104).unwrap();
        assert!(mem
            .write_int(at, size, value)
            .unwrap_err()
            .contains("guard"));
        mem.read(at, &mut after[..size]).unwrap();
        assert_eq!(after, before);
        mem.write_int(at, size, value).unwrap();
        assert_eq!(mem.read_int(at, size).unwrap(), expected);
        assert!(mem.read_int(1 << 47, size).is_err());
        assert!(mem.write_int(u64::MAX, size, value).is_err());
    }
    mem.map((1 << 47) - 4096, 4096, READ | WRITE).unwrap();
    for size in [2, 4, 8] {
        let at = (1 << 47) - size as u64;
        mem.write_int(at, size, value).unwrap();
        assert!(mem.read_int(at + 1, size).is_err());
        assert!(mem.write_int(at + 1, size, 0).is_err());
        assert_eq!(
            mem.read_int(at, size).unwrap(),
            value & (u64::MAX >> (64 - size * 8))
        );
    }
}

#[test]
fn amd64_sparse_page_reuse_does_not_alias_old_addresses_or_merged_memory() {
    let mut mem = Memory::default();
    // Non-contiguous pages grow owned storage and collide in the small address
    // cache. Every lookup must still distinguish the complete guest address.
    for page in 0..128u64 {
        let address = BASE + page * 0x100000;
        mem.map(address, 4096, READ | WRITE).unwrap();
        mem.write_int(address + 7, 8, 0x1234567800000000 | page)
            .unwrap();
    }
    for page in (0..128u64).rev().chain(0..128) {
        assert_eq!(
            mem.read_int(BASE + page * 0x100000 + 7, 8).unwrap(),
            0x1234567800000000 | page
        );
    }
    let removed = BASE + 63 * 0x100000;
    mem.read_int(removed + 7, 8).unwrap();
    mem.unmap(removed, 4096).unwrap();
    // Reuse the freed storage for an address in a different cache slot.
    let replacement = BASE + 0x80001000;
    mem.map(replacement, 4096, READ | WRITE).unwrap();
    mem.write_int(replacement + 7, 8, 0xfedcba9876543210)
        .unwrap();
    assert!(mem
        .read_int(removed + 7, 8)
        .unwrap_err()
        .contains("unmapped"));
    let mut other = Memory::default();
    let merged = BASE + 0x100000000;
    other.map(merged, 8192, READ | WRITE).unwrap();
    for page in 0..2 {
        other
            .write_int(merged + page * 4096 + 7, 8, 0xabcd0000 | page)
            .unwrap();
        assert_eq!(
            other.read_int(merged + page * 4096 + 7, 8).unwrap(),
            0xabcd0000 | page
        );
    }
    mem.merge(other).unwrap();
    for page in 0..2 {
        assert_eq!(
            mem.read_int(merged + page * 4096 + 7, 8).unwrap(),
            0xabcd0000 | page
        );
    }
    assert_eq!(
        mem.read_int(replacement + 7, 8).unwrap(),
        0xfedcba9876543210
    );
    assert!(mem.read_int(removed + 7, 8).is_err());
    mem.map(removed, 4096, READ | WRITE).unwrap();
    assert_eq!(mem.read_int(removed + 7, 8).unwrap(), 0);
}

#[test]
fn amd64_decode_cache_tracks_code_writes_protection_remapping_and_guarded_page_boundaries() {
    const OTHER: u64 = BASE + 0x10000;
    let mut mem = Memory::default();
    mem.enable_execution_stats();
    mem.map(BASE, 8192, WRITE | EXECUTE).unwrap(); // Execute does not require READ.
    let mut cpu = Cpu64::new(BASE, STACK);
    let mov = |value: u64| {
        let mut code = vec![0x48, 0xb8];
        code.extend(value.to_le_bytes());
        code
    };
    mem.write(BASE, &mov(0x123456789abcdef0)).unwrap();
    for _ in 0..2 {
        cpu.rip = BASE;
        cpu.step(&mut mem).unwrap();
        assert_eq!(cpu.regs[0], 0x123456789abcdef0);
    }
    mem.write_int(BASE + 2, 8, 0x23456789abcdef01).unwrap();
    cpu.rip = BASE;
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.regs[0], 0x23456789abcdef01);
    mem.protect(BASE, 4096, READ | WRITE).unwrap();
    cpu.rip = BASE;
    let before = cpu.clone();
    assert!(cpu.step(&mut mem).is_err());
    assert_eq!(cpu, before);
    mem.write(BASE, &mov(0x3456789abcdef012)).unwrap();
    mem.protect(BASE, 4096, READ | EXECUTE).unwrap();
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.regs[0], 0x3456789abcdef012);
    // Rearming a guard on the exact cached code page invalidates the execution proof.
    mem.virtual_protect(BASE, 4096, 0x120).unwrap();
    cpu.rip = BASE;
    let before = cpu.clone();
    assert!(cpu.step(&mut mem).unwrap_err().contains("guard-page"));
    assert_eq!(cpu, before);
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.regs[0], 0x3456789abcdef012);
    mem.unmap(BASE, 8192).unwrap();
    mem.map(BASE, 8192, READ | WRITE | EXECUTE).unwrap();
    mem.write(BASE, &mov(0x456789abcdef0123)).unwrap();
    cpu.rip = BASE;
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.regs[0], 0x456789abcdef0123);
    // Cache entries are owned by Memory, never shared across address spaces.
    let mut second = Memory::default();
    second.map(BASE, 4096, READ | WRITE | EXECUTE).unwrap();
    second.write(BASE, &mov(0x56789abcdef01234)).unwrap();
    cpu.rip = BASE;
    cpu.step(&mut second).unwrap();
    assert_eq!(cpu.regs[0], 0x56789abcdef01234);
    // A short instruction at page end does not consume a following guard.
    mem.write(BASE + 4095, &[0x90]).unwrap();
    mem.virtual_protect(BASE + 4096, 4096, 0x140).unwrap();
    for _ in 0..2 {
        cpu.rip = BASE + 4095;
        cpu.step(&mut mem).unwrap();
    }
    let query = mem.virtual_query(BASE + 4096).unwrap();
    assert_eq!(u32::from_le_bytes(query[36..40].try_into().unwrap()), 0x140);
    // A cached crossing instruction must continue to check both pages.
    mem.virtual_protect(BASE + 4096, 4096, 0x40).unwrap();
    mem.write(BASE + 4095, &mov(0x6789abcdef012345)).unwrap();
    cpu.rip = BASE + 4095;
    cpu.step(&mut mem).unwrap();
    mem.virtual_protect(BASE + 4096, 4096, 0x140).unwrap();
    cpu.rip = BASE + 4095;
    let before = cpu.clone();
    assert!(cpu.step(&mut mem).unwrap_err().contains("guard-page"));
    assert_eq!(cpu, before);
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.regs[0], 0x6789abcdef012345);
    // Decommit/recommit at the same address cannot reuse the old instruction.
    let allocation = mem.virtual_alloc(OTHER, 4096, 0x3000, 0x40).unwrap();
    mem.write(allocation, &mov(0x789abcdef0123456)).unwrap();
    cpu.rip = allocation;
    cpu.step(&mut mem).unwrap();
    mem.virtual_free(allocation, 4096, 0x4000).unwrap();
    mem.virtual_alloc(allocation, 4096, 0x1000, 0x40).unwrap();
    mem.write(allocation, &mov(0x89abcdef01234567)).unwrap();
    cpu.rip = allocation;
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.regs[0], 0x89abcdef01234567);
    let stats = mem.execution_stats().unwrap();
    assert!(stats.ir_steps >= 10);
    assert_eq!(stats.fallback_steps, 0);
}

#[test]
fn amd64_data_mapping_changes_keep_ir_but_preserve_live_data_permissions_and_faults() {
    const DATA: u64 = BASE + 0x200000;
    const OTHER: u64 = BASE + 0x400000;
    let mut mem = Memory::default();
    mem.map(BASE, 4096, WRITE | EXECUTE).unwrap();
    mem.write(BASE, &[0x48, 0x8b, 0x01]).unwrap(); // MOV RAX,[RCX]
    mem.virtual_alloc(DATA, 4096, 0x3000, 4).unwrap();
    mem.write_int(DATA, 8, 0x123456789abcdef0).unwrap();
    mem.enable_execution_stats();
    let mut cpu = Cpu64::new(BASE, STACK);
    cpu.regs[1] = DATA;
    cpu.step(&mut mem).unwrap();
    assert_eq!(mem.execution_stats().unwrap().translations, 1);
    mem.map(OTHER, 4096, READ | WRITE).unwrap();
    mem.protect(OTHER, 4096, READ).unwrap();
    mem.unmap(OTHER, 4096).unwrap();
    let mut data_only = Memory::default();
    data_only.map(OTHER, 4096, READ | WRITE).unwrap();
    mem.merge(data_only).unwrap();
    cpu.rip = BASE;
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.regs[0], 0x123456789abcdef0);
    // Retaining IR must not retain data permissions or skip a freshly armed guard.
    mem.virtual_protect(DATA, 4096, 0x104).unwrap();
    cpu.rip = BASE;
    let before = cpu.clone();
    assert!(cpu.step(&mut mem).unwrap_err().contains("guard-page"));
    assert_eq!(cpu, before);
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.regs[0], 0x123456789abcdef0);
    mem.virtual_protect(DATA, 4096, 1).unwrap();
    cpu.rip = BASE;
    let before = cpu.clone();
    assert!(cpu.step(&mut mem).unwrap_err().contains("denied"));
    assert_eq!(cpu, before);
    mem.virtual_free(DATA, 4096, 0x4000).unwrap();
    assert!(cpu.step(&mut mem).unwrap_err().contains("unmapped"));
    assert_eq!(cpu, before);
    mem.virtual_alloc(DATA, 4096, 0x1000, 4).unwrap();
    mem.write_int(DATA, 8, 0xfedcba9876543210).unwrap();
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.regs[0], 0xfedcba9876543210);
    mem.virtual_free(DATA, 0, 0x8000).unwrap();
    cpu.rip = BASE;
    let before = cpu.clone();
    assert!(cpu.step(&mut mem).unwrap_err().contains("unmapped"));
    assert_eq!(cpu, before);
    let stats = mem.execution_stats().unwrap();
    assert_eq!(stats.translations, 1);
    assert_eq!(stats.invalidations, 0);
    // Executable transitions still invalidate and reject the cached instruction.
    mem.protect(BASE, 4096, READ).unwrap();
    assert!(cpu.step(&mut mem).is_err());
    assert_eq!(cpu, before);
    assert_eq!(mem.execution_stats().unwrap().invalidations, 1);
}

#[test]
fn amd64_vmm_tracks_reserve_commit_image_guard_and_allocation_ownership() {
    use wire::mem::{MEM_COMMIT, MEM_FREE, MEM_IMAGE, MEM_PRIVATE, MEM_RESERVE};
    let mut mem = Memory::default();
    mem.map_image(BASE, 8192, READ | EXECUTE).unwrap();
    let info = mem.virtual_query(BASE + 3).unwrap();
    assert_eq!(
        u32::from_le_bytes(info[40..44].try_into().unwrap()),
        MEM_IMAGE
    );
    assert!(mem.virtual_free(BASE, 0, 0x8000).is_err());
    let base = mem
        .virtual_alloc(0x400000003, 8192, MEM_RESERVE, 4)
        .unwrap();
    assert_eq!(base, 0x400000000);
    assert!(mem.read_int(base, 1).is_err());
    let info = mem.virtual_query(base).unwrap();
    assert_eq!(
        u32::from_le_bytes(info[32..36].try_into().unwrap()),
        MEM_RESERVE
    );
    assert_eq!(
        u32::from_le_bytes(info[40..44].try_into().unwrap()),
        MEM_PRIVATE
    );
    assert_eq!(u64::from_le_bytes(info[24..32].try_into().unwrap()), 12288);
    assert!(mem.map(base, 4096, READ).is_err());
    assert_eq!(
        mem.virtual_alloc(base + 4099, 1, MEM_COMMIT, 4).unwrap(),
        base + 4096
    );
    mem.write_int(base + 4096, 8, 0x12345678).unwrap();
    // Recommit preserves both payload and previous protection.
    mem.virtual_alloc(base + 4096, 4096, MEM_COMMIT, 2).unwrap();
    mem.write_int(base + 4096 + 8, 8, 99).unwrap();
    assert_eq!(mem.read_int(base + 4096, 8).unwrap(), 0x12345678);
    assert!(mem.virtual_protect(base, 8192, 2).is_err());
    assert_eq!(mem.virtual_protect(base + 4096, 4096, 0x104).unwrap(), 4);
    let info = mem.virtual_query(base + 4096).unwrap(); // Query does not consume guard.
    assert_eq!(u32::from_le_bytes(info[36..40].try_into().unwrap()), 0x104);
    assert!(mem.read_int(base + 4096, 1).unwrap_err().contains("guard"));
    assert_eq!(mem.read_int(base + 4096, 8).unwrap(), 0x12345678);
    assert_eq!(mem.virtual_protect(base + 4096, 4096, 2).unwrap(), 4);
    assert!(mem.write_int(base + 4096, 8, 0).is_err());
    mem.virtual_free(base + 4096, 4096, 0x4000).unwrap();
    mem.virtual_alloc(base + 4096, 4096, MEM_COMMIT, 4).unwrap();
    assert_eq!(mem.read_int(base + 4096, 8).unwrap(), 0);
    assert!(mem.virtual_free(base + 4096, 0, 0x8000).is_err());
    mem.virtual_free(base, 0, 0x8000).unwrap();
    let info = mem.virtual_query(base).unwrap();
    assert_eq!(
        u32::from_le_bytes(info[32..36].try_into().unwrap()),
        MEM_FREE
    );
    let top = mem.virtual_alloc(0, 4096, 0x103000, 0x20).unwrap();
    assert!(top > STACK);
    assert!(mem.read_int(top, 1).is_ok());
    assert!(mem.fetch(top, &mut [0]).is_ok());
    assert!(mem.write_int(top, 1, 0).is_err());
    mem.virtual_free(top, 0, 0x8000).unwrap();
}

#[test]
fn amd64_virtual_query_orders_sparse_commit_holes_after_merge_and_reuse() {
    use wire::mem::{MEM_COMMIT, MEM_RESERVE};
    const DATA: u64 = 0x400000000;
    let mut mem = Memory::default();
    let mut other = Memory::default();
    other.map(DATA, 4096, READ | WRITE).unwrap();
    mem.merge(other).unwrap();
    mem.unmap(DATA, 4096).unwrap();
    mem.virtual_alloc(DATA, 16 * 4096, MEM_RESERVE, 4).unwrap();
    let query = |mem: &Memory, page: u64, state: u32, pages: u64| {
        let info = mem.virtual_query(DATA + page * 4096).unwrap();
        assert_eq!(
            u64::from_le_bytes(info[..8].try_into().unwrap()),
            DATA + page * 4096
        );
        assert_eq!(u32::from_le_bytes(info[32..36].try_into().unwrap()), state);
        assert_eq!(
            u64::from_le_bytes(info[24..32].try_into().unwrap()),
            pages * 4096
        );
    };
    query(&mem, 0, MEM_RESERVE, 16);
    // Commit out of address order, preserving the next committed-page boundary.
    for page in [7, 2, 5] {
        mem.virtual_alloc(DATA + page * 4096, 4096, MEM_COMMIT, 4)
            .unwrap();
    }
    query(&mem, 0, MEM_RESERVE, 2);
    query(&mem, 2, MEM_COMMIT, 1);
    query(&mem, 3, MEM_RESERVE, 2);
    query(&mem, 6, MEM_RESERVE, 1);
    mem.virtual_free(DATA + 5 * 4096, 4096, 0x4000).unwrap();
    query(&mem, 3, MEM_RESERVE, 4);
    mem.virtual_alloc(DATA + 3 * 4096, 4096, MEM_COMMIT, 4)
        .unwrap();
    query(&mem, 2, MEM_COMMIT, 2);
    mem.virtual_protect(DATA + 3 * 4096, 4096, 2).unwrap();
    query(&mem, 2, MEM_COMMIT, 1);
    mem.virtual_free(DATA, 0, 0x8000).unwrap();
    mem.virtual_alloc(DATA, 16 * 4096, MEM_RESERVE, 4).unwrap();
    query(&mem, 0, MEM_RESERVE, 16);
    mem.virtual_alloc(DATA + 15 * 4096, 4096, MEM_COMMIT, 4)
        .unwrap();
    query(&mem, 0, MEM_RESERVE, 15);
    query(&mem, 15, MEM_COMMIT, 1);
}

#[test]
fn win64_vmm_gateway_obeys_size_t_and_prechecks_outputs_before_protection() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let base = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "VirtualAlloc",
        &[0, 8192, 0x3000, 4],
    );
    assert_ne!(base, 0);
    let pointer = STACK + 0x100;
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "VirtualQuery",
            &[base, pointer, 48]
        ),
        48
    );
    assert_eq!(mem.read_int(pointer, 8).unwrap(), base);
    assert_eq!(mem.read_int(pointer + 24, 8).unwrap(), 8192);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "VirtualQueryEx",
            &[u64::MAX, BASE, pointer, 47]
        ),
        0
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "VirtualAllocEx",
            &[0x1234, 0, 4096, 0x3000, 4]
        ),
        0
    );
    cpu.regs[1] = base;
    cpu.regs[2] = 4096;
    cpu.regs[8] = 2;
    cpu.regs[9] = 0;
    mem.write_int(cpu.regs[4], 8, BASE + 0x100).unwrap();
    let before = cpu.clone();
    assert!(runtime
        .dispatch("kernel32.dll", "VirtualProtect", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu.rip, before.rip);
    assert_eq!(cpu.regs, before.regs);
    mem.write_int(base, 8, 123).unwrap(); // Protection did not change on bad output.
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "VirtualProtect",
            &[base, 4096, 2, base]
        ),
        1
    );
    assert_eq!(mem.read_int(base, 4).unwrap(), 4); // Output may alias a page becoming read-only.
    assert!(mem.write_int(base, 4, 0).is_err());
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "VirtualFree",
            &[base, 0, 0x8000]
        ),
        1
    );
    assert!(mem.read_int(base, 1).is_err());
}
