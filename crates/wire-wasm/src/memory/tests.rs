use super::*;

#[test]
fn process_peaks_follow_committed_private_and_mapped_lifetimes() {
    let mut m = Memory::default();
    m.map_image(0x10000, 2 * PAGE, READ).unwrap();
    let dib = m.map_dib(2 * PAGE).unwrap();
    let view = m.map_file_view(0, &[1]).unwrap();
    assert_eq!(
        (m.pages.len(), m.private_pages, m.peak_private_pages),
        (5, 0, 0)
    );
    m.map(0x60000, PAGE, READ | WRITE).unwrap();
    let base = m.virtual_alloc(0x70000, 3 * PAGE, 0x2000, 4).unwrap();
    assert_eq!((m.pages.len(), m.private_pages), (6, 1));
    m.virtual_alloc(base + PAGE, PAGE, 0x1000, 4).unwrap();
    m.virtual_alloc(base + PAGE, PAGE, 0x1000, 4).unwrap();
    assert_eq!(
        (
            m.pages.len(),
            m.private_pages,
            m.peak_pages,
            m.peak_private_pages
        ),
        (7, 2, 7, 2)
    );
    m.virtual_free(base, 3 * PAGE, MEM_DECOMMIT).unwrap();
    m.virtual_free(base, 3 * PAGE, MEM_DECOMMIT).unwrap();
    m.virtual_alloc(base, 3 * PAGE, 0x1000, 4).unwrap();
    assert_eq!((m.peak_pages, m.peak_private_pages), (9, 4));
    assert!(m.virtual_free(base, PAGE, MEM_RELEASE).is_err());
    assert!(m.map(base, PAGE, READ).is_err());
    assert!(m.virtual_alloc(base, 4 * PAGE, 0x1000, 4).is_err());
    assert_eq!((m.pages.len(), m.private_pages), (9, 4));
    m.virtual_free(base, 0, MEM_RELEASE).unwrap();
    m.unmap(0x60000, PAGE).unwrap();
    m.unmap(0x10000, 2 * PAGE).unwrap();
    m.unmap(dib, 2 * PAGE).unwrap();
    m.unmap(view, PAGE).unwrap();
    assert_eq!(
        (
            m.pages.len(),
            m.private_pages,
            m.peak_pages,
            m.peak_private_pages
        ),
        (0, 0, 9, 4)
    );
}

#[test]
fn publishing_memory_counts_only_live_pages_and_preserves_failed_merge() {
    let staging = |base| {
        let mut m = Memory::default();
        m.map(base, 20 * PAGE, READ).unwrap();
        m.unmap(base, 20 * PAGE).unwrap();
        m.map_image(base, 2 * PAGE, READ).unwrap();
        m.map(base + 2 * PAGE, PAGE, READ).unwrap();
        m
    };
    let mut m = Memory::default();
    m.map(0x10000, PAGE, READ).unwrap();
    assert!(m.merge(staging(0x10000)).is_err());
    assert_eq!(
        (
            m.pages.len(),
            m.private_pages,
            m.peak_pages,
            m.peak_private_pages
        ),
        (1, 1, 1, 1)
    );
    m.merge(staging(0x20000)).unwrap();
    assert_eq!(
        (
            m.pages.len(),
            m.private_pages,
            m.peak_pages,
            m.peak_private_pages
        ),
        (4, 2, 4, 2)
    );
}

#[test]
fn dib_pages_keep_runtime_ownership_and_mapped_access_rights() {
    let mut memory = Memory::default();
    let base = memory.map_dib(4100).unwrap();
    let info = memory.virtual_query(base).unwrap();
    assert_eq!(
        (info.kind, info.protect, info.region_size),
        (MEM_MAPPED, 4, PAGE * 2)
    );
    assert_eq!(memory.virtual_free(base, 0, MEM_RELEASE), Err(487));
    assert_eq!(memory.virtual_free(base, 0, MEM_DECOMMIT), Err(487));
    memory.virtual_protect(base, PAGE, 2).unwrap();
    assert!(memory.write_int(base, 4, 1).is_err());
    memory.virtual_protect(base, PAGE, 4).unwrap();
    memory.write_int(base, 4, 1).unwrap();
    assert_eq!(memory.virtual_protect(base, PAGE, 64), Err(5));
    let view = memory.map_file_view(0, &[1, 2, 3]).unwrap();
    assert_eq!(memory.virtual_protect(view, PAGE, 4), Err(5));
    memory.unmap(base, PAGE * 2).unwrap();
    assert_eq!(memory.virtual_query(base).unwrap().state, MEM_FREE);
    assert!(memory.read_int(base, 4).is_err());
}

fn proof(memory: &mut Memory, base: u64) -> CodeProof {
    memory
        .snapshot_code_page(base, &mut [0; PAGE as usize])
        .unwrap()
}

#[test]
fn source_proofs_follow_backing_and_full_address_identity() {
    let mut memory = Memory::default();
    let low = 0x10000;
    let high = low + (1 << 32);
    memory.map(low, PAGE, READ | WRITE | EXECUTE).unwrap();
    memory.map(high, PAGE, READ | WRITE | EXECUTE).unwrap();
    let low_proof = proof(&mut memory, low);
    let high_proof = proof(&mut memory, high);
    memory.map(0x20000, PAGE, READ | WRITE).unwrap();
    memory.write_int(0x20000, 8, 1).unwrap();
    assert!(memory.code_valid(&low_proof));
    assert!(memory.code_valid(&high_proof));
    memory.write_int(high, 1, 0x90).unwrap();
    assert!(memory.code_valid(&low_proof));
    assert!(!memory.code_valid(&high_proof));

    let mut other = Memory::default();
    other.map(low, PAGE, READ | WRITE | EXECUTE).unwrap();
    let other_proof = proof(&mut other, low);
    assert!(!memory.code_valid(&other_proof));
    memory.merge(other).expect_err("overlap must fail");
    assert!(memory.code_valid(&low_proof));
    memory.unmap(low, PAGE).unwrap();
    memory.map(low, PAGE, READ | WRITE | EXECUTE).unwrap();
    let replacement = proof(&mut memory, low);
    assert!(!memory.code_valid(&low_proof));
    assert!(memory.code_valid(&replacement));
}

#[test]
fn protection_changes_nx_writes_and_version_wrap_cannot_revive_source() {
    let mut memory = Memory::default();
    memory
        .map(0x10000, PAGE * 2, READ | WRITE | EXECUTE)
        .unwrap();
    let original = proof(&mut memory, 0x10000);
    memory.protect(0x10000, PAGE, READ | WRITE).unwrap();
    memory.write_int(0x10000, 1, 0xcc).unwrap();
    memory.protect(0x10000, PAGE, READ | EXECUTE).unwrap();
    assert!(!memory.code_valid(&original));
    let current = proof(&mut memory, 0x10000);
    let adjacent = proof(&mut memory, 0x11000);
    memory
        .virtual_protect(0x10000, PAGE, PAGE_EXECUTE_READ | PAGE_GUARD)
        .unwrap();
    assert!(!memory.code_valid(&current));
    assert_eq!(
        memory.fetch(0x10000, &mut [0]),
        Err(Fault {
            address: 0x10000,
            length: 1,
            access: EXECUTE,
            kind: FaultKind::Guard,
        })
    );
    assert!(!memory.code_valid(&current));
    let before_wrap = proof(&mut memory, 0x10000);
    memory.translations.source.set(u64::MAX);
    memory.write_int(0x11000, 1, 0x90).unwrap();
    assert!(!memory.code_valid(&adjacent));
    assert!(!memory.code_valid(&before_wrap));
    assert!(!memory.code_valid(&current));
    let after_wrap = proof(&mut memory, 0x10000);
    assert!(memory.code_valid(&after_wrap));
    assert!(!memory.code_valid(&original));
}

#[test]
fn cross_page_faults_preserve_data_and_observation_does_not_consume_guards() {
    let mut memory = Memory::default();
    memory
        .map(0x10000, PAGE * 2, READ | WRITE | EXECUTE)
        .unwrap();
    memory.write(0x10ffe, &[1, 2, 3, 4]).unwrap();
    memory
        .virtual_protect(0x11000, PAGE, PAGE_EXECUTE_READWRITE | PAGE_GUARD)
        .unwrap();
    let mut observed = [0xa5; 4];
    assert_eq!(
        memory.observe(0x10ffe, &mut observed).unwrap_err().kind,
        FaultKind::Guard
    );
    assert_eq!(observed, [0xa5; 4]);
    assert_eq!(
        memory
            .snapshot_code_page(0x11000, &mut [0; PAGE as usize])
            .unwrap_err()
            .kind,
        FaultKind::Guard
    );
    assert_ne!(
        memory.virtual_query(0x11000).unwrap().protect & PAGE_GUARD,
        0
    );
    assert_eq!(
        memory.write(0x10ffe, &[9; 4]).unwrap_err().kind,
        FaultKind::Guard
    );
    memory.read(0x10ffe, &mut observed).unwrap();
    assert_eq!(observed, [1, 2, 3, 4]);
    memory.protect(0x11000, PAGE, EXECUTE).unwrap();
    observed = [0xa5; 4];
    assert_eq!(
        memory.read(0x10ffe, &mut observed).unwrap_err().kind,
        FaultKind::Protection
    );
    assert_eq!(observed, [0xa5; 4]);
    assert_eq!(
        memory.write(0x10ffe, &[9; 4]).unwrap_err().kind,
        FaultKind::Protection
    );
    assert_eq!(memory.read_int(0x10ffe, 2).unwrap(), 0x0201);
    memory.fetch(0x10ffe, &mut observed).unwrap();
    assert_eq!(observed, [1, 2, 3, 4]);
}

#[test]
fn windows_reservation_commit_query_and_recommit_preserve_protocol() {
    for mode in [Mode::X86, Mode::X64] {
        let mut memory = Memory::new(mode);
        let base = memory
            .virtual_alloc(0x12345, PAGE * 2, MEM_RESERVE, PAGE_READWRITE)
            .unwrap();
        assert_eq!(base, 0x10000);
        let reserved = memory.virtual_query(base + 1).unwrap();
        assert_eq!(
            (reserved.state, reserved.region_size),
            (MEM_RESERVE, PAGE * 5)
        );
        assert_eq!(
            memory.virtual_alloc(
                base + PAGE + 1,
                PAGE - 1,
                MEM_COMMIT,
                PAGE_EXECUTE_READWRITE
            ),
            Ok(base + PAGE)
        );
        let info = memory.virtual_query(base + PAGE + 7).unwrap();
        assert_eq!(
            info,
            MemoryInfo {
                base: base + PAGE,
                allocation_base: base,
                allocation_protect: PAGE_READWRITE,
                region_size: PAGE,
                state: MEM_COMMIT,
                protect: PAGE_EXECUTE_READWRITE,
                kind: MEM_PRIVATE,
            }
        );
        let bytes32 = info.encode32().unwrap();
        assert_eq!(
            u32::from_le_bytes(bytes32[12..16].try_into().unwrap()),
            PAGE as u32
        );
        let bytes64 = info.encode64();
        assert_eq!(&bytes64[20..24], &[0; 4]);
        assert_eq!(&bytes64[44..48], &[0; 4]);
        assert_eq!(
            u64::from_le_bytes(bytes64[24..32].try_into().unwrap()),
            PAGE
        );
        memory.write_int(base + PAGE, 4, 0xdeadbeef).unwrap();
        let source = proof(&mut memory, base + PAGE);
        memory
            .virtual_free(base + PAGE, PAGE, MEM_DECOMMIT)
            .unwrap();
        assert!(!memory.code_valid(&source));
        assert_eq!(memory.virtual_query(base).unwrap(), reserved);
        memory
            .virtual_alloc(base + PAGE, PAGE, MEM_COMMIT, PAGE_EXECUTE_READWRITE)
            .unwrap();
        assert_eq!(memory.read_int(base + PAGE, 4).unwrap(), 0);
        assert!(!memory.code_valid(&source));
        assert_eq!(memory.virtual_free(base + PAGE, 0, MEM_RELEASE), Err(487));
        memory.virtual_free(base, 0, MEM_RELEASE).unwrap();
        assert_eq!(memory.virtual_query(base).unwrap().state, MEM_FREE);
        assert_eq!(memory.virtual_query(memory.app_end()), Err(87));
        assert_eq!(
            memory.virtual_alloc(0, PAGE, MEM_COMMIT, PAGE_READWRITE | PAGE_GUARD | 0x200),
            Err(87)
        );
    }
}

#[test]
fn owned_pages_and_section_coherence_preserve_guest_protection() {
    let mut memory = Memory::default();
    let base = memory.map_file_view(0, &[1, 2, 3, 4]).unwrap();
    assert_eq!(memory.virtual_query(base).unwrap().kind, MEM_MAPPED);
    assert_eq!(memory.virtual_protect(base, PAGE, PAGE_READWRITE), Err(5));
    memory
        .virtual_protect(base, PAGE, PAGE_READONLY | PAGE_GUARD)
        .unwrap();
    memory.update_file_view(base, 1, &[9, 8]).unwrap();
    assert_eq!(memory.read_int(base, 4).unwrap_err().kind, FaultKind::Guard);
    assert_eq!(memory.read_int(base, 4).unwrap(), 0x04080901);
    assert_eq!(
        memory.write_int(base, 1, 0).unwrap_err().kind,
        FaultKind::Protection
    );
    assert_eq!(memory.virtual_free(base, PAGE, MEM_DECOMMIT), Err(487));
    memory.seal_runtime_readonly(base).unwrap();
    assert_eq!(memory.virtual_protect(base, PAGE, PAGE_NOACCESS), Err(5));
    assert!(memory.check_unmap(base, PAGE * 2).is_err());
    memory.unmap(base, PAGE).unwrap();
    assert_eq!(
        memory.read_int(base, 1).unwrap_err().kind,
        FaultKind::Unmapped
    );
}

#[test]
fn addresses_and_mapping_capacity_are_checked_before_allocation() {
    for mode in [Mode::X86, Mode::X64] {
        let mut memory = Memory::new(mode);
        let end = memory.user_end();
        memory.map(end - PAGE, PAGE, READ | WRITE).unwrap();
        memory.write_int(end - 1, 1, 0x7f).unwrap();
        assert_eq!(memory.read_int(end - 1, 1).unwrap(), 0x7f);
        assert_eq!(
            memory.read_int(end - 1, 2).unwrap_err().kind,
            FaultKind::Address
        );
        assert_eq!(
            memory.read_int(u64::MAX, 2).unwrap_err().kind,
            FaultKind::Address
        );
        assert!(memory
            .map(0x10000, MAPPING_BYTE_LIMIT + PAGE, READ)
            .is_err());
        assert_eq!(memory.pages.len(), 1);
        assert!(memory.map(end, PAGE, READ).is_err());
        assert_eq!(
            memory.virtual_alloc(end - PAGE, PAGE, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE),
            Err(487)
        );
    }
    let mut memory = Memory::default();
    let high = (1 << 32) + 0x10000;
    memory.map(high, PAGE, READ).unwrap();
    assert_eq!(memory.virtual_query(high).unwrap().encode32(), Err(87));
    assert!(!Memory::commit_capacity(
        (COMMIT_BYTE_LIMIT / PAGE) as usize,
        1
    ));
}
#[test]
fn pe32_dep_profile_revokes_code_and_preserves_windows_protection() {
    let mut memory = Memory::new(Mode::X86);
    memory.map(0x10000, PAGE, READ | WRITE).unwrap();
    assert!(memory.fetch(0x10000, &mut [0]).is_err());
    memory.set_dep_enabled(false);
    let proof = memory
        .snapshot_code_page(0x10000, &mut [0; PAGE as usize])
        .unwrap();
    assert!(memory.code_valid(&proof));
    assert_eq!(memory.virtual_query(0x10000).unwrap().protect, 4);
    memory.set_dep_enabled(true);
    assert!(!memory.code_valid(&proof));
    memory.set_dep_enabled(false);
    assert!(!memory.code_valid(&proof));
    memory.fetch(0x10000, &mut [0]).unwrap();
    let mut x64 = Memory::new(Mode::X64);
    x64.map(0x10000, PAGE, READ | WRITE).unwrap();
    x64.set_dep_enabled(false);
    assert!(x64.fetch(0x10000, &mut [0]).is_err());
}
