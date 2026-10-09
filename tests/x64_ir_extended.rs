use wire::x64::memory::TranslationCacheLayout as Layout;
use wire::x64::{
    cpu::Cpu64,
    memory::{Memory, EXECUTE, READ, WRITE},
};

const CODE: u64 = 0x140000000;
const DATA: u64 = 0x200000000;

#[test]
fn diagnostics_distinguish_same_byte_epoch_reuse_from_actual_instruction_change() {
    let mut mem = Memory::default();
    mem.map_image(CODE, 4096, READ | WRITE | EXECUTE).unwrap();
    mem.map(DATA, 4096, READ | WRITE | EXECUTE).unwrap();
    mem.write(CODE, &[0x90, 0x48, 0xc1, 0xc0, 1]).unwrap();
    mem.register_code_image(CODE, "original.exe".into());
    mem.enable_execution_diagnostics();
    let mut cpu = Cpu64::new(CODE, DATA);
    cpu.step(&mut mem).unwrap();
    cpu.rip = CODE;
    cpu.step(&mut mem).unwrap(); // Hit: does not add a translation.
    assert_eq!(mem.execution_stats().unwrap().translations, 1);
    mem.write(DATA, &[1]).unwrap(); // Unrelated page generation leaves this entry valid.
    cpu.rip = CODE;
    cpu.step(&mut mem).unwrap();
    mem.write(CODE, &[0x90]).unwrap(); // Byte-identical code write.
    cpu.rip = CODE;
    cpu.step(&mut mem).unwrap();
    mem.write(CODE, &[0x50]).unwrap(); // NOP becomes PUSH RAX.
    cpu.rip = CODE;
    cpu.regs[4] = DATA + 4096;
    cpu.step(&mut mem).unwrap();
    let causes = mem.translation_diagnostics().unwrap();
    assert_eq!(causes.cold, 1);
    assert_eq!(causes.epoch_reuse, 1);
    assert_eq!(causes.byte_changes, 1);
    assert_eq!(causes.evicted, 0);
    let text = mem.execution_diagnostics_report().unwrap();
    assert!(text.contains("image:original.exe@0x140000000\t3"));
    assert!(text.contains("0x140000000\twrites=2\tchanged=1"));
    assert!(text.contains("0x200000000\twrites=2\tchanged=1"));
    assert!(!text.contains("Mono"));
}

#[test]
fn diagnostics_do_not_turn_disabled_stats_into_a_hot_path_requirement() {
    let mut mem = Memory::default();
    mem.map(CODE, 4096, READ | WRITE | EXECUTE).unwrap();
    mem.write(CODE, &[0x90]).unwrap();
    let mut cpu = Cpu64::new(CODE, DATA);
    cpu.step(&mut mem).unwrap();
    assert!(mem.execution_stats().is_none());
    assert!(mem.translation_diagnostics().is_none());
    assert!(mem.execution_diagnostics_report().is_none());
}

#[test]
fn every_cache_layout_checks_crossing_code_guard_protection_and_remapping() {
    {
        let layout = Layout::default();
        let mut mem = Memory::default();
        mem.set_translation_cache_layout(layout).unwrap();
        mem.map(CODE, 8192, READ | WRITE | EXECUTE).unwrap();
        mem.write(CODE + 4094, &[0x48, 0x89, 0xd8]).unwrap(); // mov rax,rbx crosses page.
        let mut cpu = Cpu64::new(CODE + 4094, DATA);
        cpu.regs[3] = 123;
        cpu.step(&mut mem).unwrap();
        assert_eq!(cpu.regs[0], 123);
        assert!(mem.set_translation_cache_layout(layout).is_err());
        mem.virtual_protect(CODE + 4096, 4096, 0x140).unwrap(); // EXECUTE_READ + GUARD
        cpu.rip = CODE + 4094;
        let before = cpu.clone();
        assert!(cpu.step(&mut mem).unwrap_err().contains("guard"));
        assert_eq!(cpu, before);
        cpu.step(&mut mem).unwrap(); // Guard consumed, retained bytes still valid.
        mem.protect(CODE + 4096, 4096, READ | WRITE).unwrap();
        cpu.rip = CODE + 4094;
        let before = cpu.clone();
        assert!(cpu.step(&mut mem).is_err());
        assert_eq!(cpu, before);
        mem.protect(CODE + 4096, 4096, READ | WRITE | EXECUTE)
            .unwrap();
        mem.write(CODE + 4096, &[0xc8]).unwrap(); // mov rax,rcx
        cpu.regs[1] = 456;
        cpu.step(&mut mem).unwrap();
        assert_eq!(cpu.regs[0], 456);
        mem.unmap(CODE, 8192).unwrap();
        mem.map(CODE, 8192, READ | WRITE | EXECUTE).unwrap();
        mem.write(CODE + 4094, &[0x48, 0x89, 0xd0]).unwrap(); // mov rax,rdx
        cpu.regs[2] = 789;
        cpu.rip = CODE + 4094;
        cpu.step(&mut mem).unwrap();
        assert_eq!(cpu.regs[0], 789);
    }
}

#[test]
fn cache_candidates_retain_large_scalar_working_set_without_changing_execution() {
    let body = [
        0x48, 0x01, 0xd8, 0x48, 0x31, 0xd9, 0x4d, 0x89, 0xd3, 0x48, 0x8d, 0x49, 3,
    ]
    .repeat(4096);
    {
        let layout = Layout::default();
        let mut mem = Memory::default();
        mem.set_translation_cache_layout(layout).unwrap();
        mem.map(CODE, 65536, READ | WRITE | EXECUTE).unwrap();
        mem.write(CODE, &body).unwrap();
        mem.enable_execution_diagnostics();
        let mut cpu = Cpu64::new(CODE, DATA);
        cpu.regs[3] = 0x1234;
        for _ in 0..3 {
            cpu.rip = CODE;
            for _ in 0..16384 {
                cpu.step(&mut mem).unwrap();
            }
        }
        let stats = mem.execution_stats().unwrap();
        assert_eq!(stats.fallback_steps, 0);
        assert_eq!(stats.invalidations, 0);
        assert_eq!(cpu.retired, 49152);
        let causes = mem.translation_diagnostics().unwrap();
        assert_eq!(causes.cold, 16384);
        if layout == Layout::Direct16K {
            assert!(stats.translations > 26000);
        } else {
            assert!(stats.translations < 17000, "{layout:?}: {stats:?}");
        }
    }
}

#[test]
fn typed_packed_integer_lanes_high_registers_and_aliases_match_unicorn() {
    use unicorn_engine::{Arch, Mode, Prot, RegisterX86 as U, Unicorn};
    for op in [
        0xdb, 0xdf, 0xeb, 0xef, 0xfc, 0xfd, 0xfe, 0xd4, 0xf8, 0xf9, 0xfa, 0xfb, 0x74, 0x75, 0x76,
        0x64, 0x65, 0x66,
    ] {
        for alias in [false, true] {
            let code = [0x66, 0x45, 0x0f, op, if alias { 0xc0 } else { 0xc1 }];
            let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
            uc.mem_map(CODE, 4096, Prot::ALL).unwrap();
            uc.mem_write(CODE, &code).unwrap();
            let mut mem = Memory::default();
            mem.map(CODE, 4096, READ | WRITE | EXECUTE).unwrap();
            mem.write(CODE, &code).unwrap();
            mem.enable_execution_stats();
            for seed in 0u64..32 {
                let mut random = seed.wrapping_add(0x123456789abcdef0);
                let mut cpu = Cpu64::new(CODE, DATA);
                for xmm in &mut cpu.xmm[8..10] {
                    for byte in xmm {
                        random ^= random << 13;
                        random ^= random >> 7;
                        random ^= random << 17;
                        *byte = random as u8;
                    }
                }
                cpu.rflags = 0xad7;
                cpu.mxcsr = 0x9fc0;
                uc.reg_write_long(U::XMM8, &cpu.xmm[8]).unwrap();
                uc.reg_write_long(U::XMM9, &cpu.xmm[9]).unwrap();
                uc.reg_write(U::RFLAGS, cpu.rflags).unwrap();
                uc.reg_write(U::MXCSR, cpu.mxcsr as u64).unwrap();
                uc.emu_start(CODE, CODE + code.len() as u64, 0, 1).unwrap();
                cpu.step(&mut mem).unwrap();
                assert_eq!(
                    &*uc.reg_read_long(U::XMM8).unwrap(),
                    &cpu.xmm[8],
                    "op={op:x} seed={seed}"
                );
                assert_eq!(&*uc.reg_read_long(U::XMM9).unwrap(), &cpu.xmm[9]);
                assert_eq!(cpu.rflags, uc.reg_read(U::RFLAGS).unwrap());
                assert_eq!(cpu.mxcsr as u64, uc.reg_read(U::MXCSR).unwrap());
                assert_eq!(cpu.retired, 1);
                assert_eq!(cpu.work, 1);
            }
            assert_eq!(mem.execution_stats().unwrap().fallback_steps, 0);
        }
    }
}

#[test]
fn page_generations_keep_unrelated_code_and_track_writes_while_nonexecutable() {
    let mut mem = Memory::default();
    mem.map(CODE, 4096, READ | WRITE | EXECUTE).unwrap();
    mem.map(DATA, 4096, READ | WRITE | EXECUTE).unwrap();
    mem.write(CODE, &[0xb8, 1, 0, 0, 0]).unwrap();
    mem.write(DATA, &[0xb8, 2, 0, 0, 0]).unwrap();
    mem.enable_execution_stats();
    let mut cpu = Cpu64::new(CODE, DATA + 4096);
    cpu.step(&mut mem).unwrap();
    cpu.rip = DATA;
    cpu.step(&mut mem).unwrap();
    assert_eq!(mem.execution_stats().unwrap().translations, 2);
    mem.write(CODE + 1, &[3]).unwrap();
    cpu.rip = DATA;
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.regs[0], 2);
    assert_eq!(mem.execution_stats().unwrap().translations, 2);
    cpu.rip = CODE;
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.regs[0], 3);
    mem.protect(CODE, 4096, READ | WRITE).unwrap();
    mem.write(CODE + 1, &[4]).unwrap();
    cpu.rip = DATA;
    cpu.step(&mut mem).unwrap();
    assert_eq!(mem.execution_stats().unwrap().translations, 3);
    mem.protect(CODE, 4096, READ | EXECUTE).unwrap();
    cpu.rip = CODE;
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.regs[0], 4);
    assert_eq!(mem.execution_stats().unwrap().translations, 4);
}

#[test]
fn crossing_generation_tracks_second_page_and_merged_memories_have_fresh_identity() {
    let mut mem = Memory::default();
    mem.map(CODE, 8192, READ | WRITE | EXECUTE).unwrap();
    let at = CODE + 4091;
    mem.write(at, &[0x48, 0xb8, 1, 2, 3, 4, 5, 6, 7, 8])
        .unwrap();
    let mut cpu = Cpu64::new(at, DATA);
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.regs[0], 0x0807060504030201);
    mem.write(CODE + 4096, &[9, 10, 11, 12, 13]).unwrap();
    cpu.rip = at;
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.regs[0], 0x0d0c0b0a09030201);
    mem.unmap(CODE, 8192).unwrap();
    let mut incoming = Memory::default();
    incoming.map(CODE, 8192, READ | WRITE | EXECUTE).unwrap();
    incoming
        .write(at, &[0x48, 0xb8, 0x11, 0, 0, 0, 0, 0, 0, 0])
        .unwrap();
    mem.merge(incoming).unwrap();
    cpu.rip = at;
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.regs[0], 0x11);
}

#[test]
fn bounded_rep_keeps_self_modifying_code_observable_between_elements() {
    let mut mem = Memory::default();
    mem.map(CODE, 4096, READ | WRITE | EXECUTE).unwrap();
    mem.write(CODE, &[0xf3, 0xaa]).unwrap(); // REP STOSB.
    let mut cpu = Cpu64::new(CODE, DATA);
    cpu.regs[0] = 0x90;
    cpu.regs[1] = 2;
    cpu.regs[7] = CODE;
    assert_eq!(cpu.step_bounded(&mut mem, 2).unwrap(), 1);
    assert_eq!(cpu.rip, CODE);
    assert_eq!(cpu.regs[1], 1);
    assert_eq!(cpu.step_bounded(&mut mem, 1).unwrap(), 1); // Newly written NOP.
    assert_eq!(cpu.rip, CODE + 1);
    assert_eq!(cpu.regs[1], 1);
    assert_eq!(cpu.retired, 1);
    assert_eq!(mem.read_int(CODE + 1, 1).unwrap(), 0xaa);
}

#[test]
fn merge_rejects_coincident_generation_numbers_from_independent_memories() {
    let mut mem = Memory::default();
    mem.map(CODE, 4096, READ | WRITE | EXECUTE).unwrap();
    mem.write(CODE, &[0xb8, 1, 0, 0, 0]).unwrap();
    let mut cpu = Cpu64::new(CODE, DATA);
    cpu.step(&mut mem).unwrap();
    mem.unmap(CODE, 4096).unwrap();
    let mut incoming = Memory::default();
    incoming.map(CODE, 4096, READ | WRITE | EXECUTE).unwrap();
    incoming.write(CODE, &[0xb8, 2, 0, 0, 0]).unwrap();
    mem.merge(incoming).unwrap();
    cpu.rip = CODE;
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.regs[0], 2);
}

#[test]
fn typed_multiply_and_implicit_sign_conversions_match_independent_oracle() {
    use unicorn_engine::{Arch, Mode, Prot, RegisterX86 as U, Unicorn};
    let fixtures = [
        (vec![0x4d, 0x0f, 0xaf, 0xc1], true),
        (vec![0x45, 0x0f, 0xaf, 0xc1], true),
        (vec![0x66, 0x45, 0x0f, 0xaf, 0xc1], true),
        (vec![0x4d, 0x6b, 0xc1, 0x81], true),
        (vec![0x45, 0x69, 0xc1, 1, 2, 3, 0x80], true),
        (vec![0x66, 0x45, 0x69, 0xc1, 1, 0x80], true),
        (vec![0x4c, 0x0f, 0xaf, 0x07], true),
        (vec![0x44, 0x6b, 0x07, 0x81], true),
        (vec![0x66, 0x98], false),
        (vec![0x98], false),
        (vec![0x48, 0x98], false),
        (vec![0x66, 0x99], false),
        (vec![0x99], false),
        (vec![0x48, 0x99], false),
        (vec![0x49, 0x0f, 0xc8], false),
        (vec![0x41, 0x0f, 0xc8], false),
    ];
    let registers = [
        U::RAX,
        U::RCX,
        U::RDX,
        U::RBX,
        U::RSP,
        U::RBP,
        U::RSI,
        U::RDI,
        U::R8,
        U::R9,
        U::R10,
        U::R11,
        U::R12,
        U::R13,
        U::R14,
        U::R15,
    ];
    let edges = [
        0,
        1,
        u64::MAX,
        0x80,
        0x8000,
        0x80000000,
        1 << 63,
        0x7fff,
        0x7fffffff,
        (1 << 63) - 1,
    ];
    for (code, multiply) in fixtures {
        let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
        uc.mem_map(CODE, 4096, Prot::ALL).unwrap();
        uc.mem_write(CODE, &code).unwrap();
        uc.mem_map(DATA, 4096, Prot::ALL).unwrap();
        let mut mem = Memory::default();
        mem.map(CODE, 4096, READ | WRITE | EXECUTE).unwrap();
        mem.map(DATA, 4096, READ | WRITE).unwrap();
        mem.write(CODE, &code).unwrap();
        mem.enable_execution_stats();
        for seed in 0..128usize {
            let mut random = (seed as u64).wrapping_add(0x123456789abcdef0);
            let mut cpu = Cpu64::new(CODE, DATA);
            for value in &mut cpu.regs {
                random ^= random << 13;
                random ^= random >> 7;
                random ^= random << 17;
                *value = random;
            }
            if seed < edges.len() * edges.len() {
                cpu.regs[0] = edges[seed % edges.len()];
                cpu.regs[8] = edges[seed % edges.len()];
                cpu.regs[9] = edges[seed / edges.len()];
            }
            cpu.regs[7] = DATA;
            cpu.rflags = 0xad7;
            let data = cpu.regs[9].to_le_bytes();
            uc.mem_write(DATA, &data).unwrap();
            mem.write(DATA, &data).unwrap();
            for (r, value) in registers.iter().zip(cpu.regs) {
                uc.reg_write(*r, value).unwrap();
            }
            uc.reg_write(U::RFLAGS, cpu.rflags).unwrap();
            uc.emu_start(CODE, CODE + code.len() as u64, 0, 1).unwrap();
            cpu.step(&mut mem).unwrap();
            for (n, r) in registers.iter().enumerate() {
                assert_eq!(
                    cpu.regs[n],
                    uc.reg_read(*r).unwrap(),
                    "code={code:x?} seed={seed} reg={n}"
                );
            }
            let defined = if multiply { 1 | 0x800 } else { u64::MAX };
            assert_eq!(
                cpu.rflags & defined,
                uc.reg_read(U::RFLAGS).unwrap() & defined,
                "code={code:x?} seed={seed}"
            );
            if multiply {
                assert_eq!(cpu.rflags & !defined, 0xad7 & !defined);
            }
            assert_eq!(cpu.retired, 1);
            assert_eq!(cpu.work, 1);
        }
        assert_eq!(mem.execution_stats().unwrap().fallback_steps, 0);
    }
}

#[test]
fn invalid_bswap_width_keeps_cold_hot_and_segment_fault_boundaries() {
    for prefix in [vec![], vec![0x90; 4], vec![0x90; 16]] {
        for warm in [false, true] {
            let mut code = prefix.clone();
            code.extend_from_slice(&[0x66, 0x0f, 0xc8]);
            let mut mem = Memory::default();
            mem.map(CODE, 4096, READ | WRITE | EXECUTE).unwrap();
            mem.write(CODE, &code).unwrap();
            let mut cpu = Cpu64::new(CODE, DATA);
            cpu.regs[0] = 0xdeadbeef11223344;
            cpu.rflags = 0xad7;
            let fault = CODE + prefix.len() as u64;
            // Warm every entry without executing. A later rejected instruction
            // cannot cause an early error while the preceding NOPs are grouped.
            if warm {
                for n in 0..=prefix.len() {
                    cpu.rip = CODE + n as u64;
                    cpu.decode(&mem).unwrap();
                }
            }
            cpu.rip = CODE;
            while cpu.rip < fault {
                cpu.step_with_budget(&mut mem, 20).unwrap();
            }
            assert_eq!(cpu.retired, prefix.len() as u64);
            assert_eq!(cpu.work, prefix.len() as u64);
            for _ in 0..3 {
                let before = cpu.clone();
                assert!(cpu
                    .step_with_budget(&mut mem, 10)
                    .unwrap_err()
                    .contains("invalid BSWAP width"));
                assert_eq!(cpu, before);
            }
        }
    }
}

#[test]
fn register_segments_stop_before_next_page_guard_and_revalidate_changed_link_target() {
    let mut mem = Memory::default();
    mem.map(CODE, 8192, READ | WRITE | EXECUTE).unwrap();
    let start = CODE + 4088;
    mem.write(start, &[0x90; 16]).unwrap();
    let mut cpu = Cpu64::new(start, DATA);
    for n in 0..16 {
        cpu.rip = start + n;
        cpu.decode(&mem).unwrap();
    }
    mem.virtual_protect(CODE + 4096, 4096, 0x140).unwrap();
    cpu.rip = start;
    assert_eq!(cpu.step_bounded(&mut mem, 64).unwrap(), 8);
    assert_eq!(cpu.rip, CODE + 4096);
    let before = cpu.clone();
    assert!(cpu
        .step_bounded(&mut mem, 64)
        .unwrap_err()
        .contains("guard"));
    assert_eq!(cpu, before);
    assert_eq!(cpu.step_bounded(&mut mem, 8).unwrap(), 8);
    assert_eq!(cpu.retired, 16);

    let code = [0x48, 0xff, 0xc0].repeat(16);
    mem.write(CODE, &code).unwrap();
    let mut fast = Cpu64::new(CODE, DATA);
    for n in 0..16 {
        fast.rip = CODE + 3 * n;
        fast.decode(&mem).unwrap();
    }
    fast.rip = CODE;
    while fast.work < 16 {
        fast.step_with_budget(&mut mem, 16 - fast.work).unwrap();
    }
    assert_eq!(fast.regs[0], 16);
    mem.write(CODE + 24, &[0x48, 0xff, 0xc8]).unwrap(); // Change the linked segment's first INC to DEC.
    fast.rip = CODE;
    while fast.work < 32 {
        fast.step_with_budget(&mut mem, 32 - fast.work).unwrap();
    }
    assert_eq!(fast.regs[0], 30);
    assert_eq!(fast.rip, CODE + 48);
    assert_eq!(fast.retired, 32);
    assert_eq!(fast.work, 32);
}

#[test]
fn typed_bit_tests_keep_signed_offsets_immediate_masks_and_fault_commit_boundaries() {
    use unicorn_engine::unicorn_const::{Arch, Mode, Prot};
    use unicorn_engine::{RegisterX86 as U, Unicorn};
    for bits in [16u32, 32, 64] {
        for kind in 0..4 {
            for memory in [false, true] {
                for immediate in [false, true] {
                    for locked in [false, true] {
                        if locked && (!memory || kind == 0) {
                            continue;
                        }
                        let mut code = Vec::new();
                        if locked {
                            code.push(0xf0);
                        }
                        if bits == 16 {
                            code.push(0x66);
                        }
                        if bits == 64 {
                            code.push(0x48);
                        }
                        if immediate {
                            code.extend([
                                0x0f,
                                0xba,
                                if memory { 0x23 } else { 0xe0 } | (kind << 3),
                                0xff,
                            ]);
                        } else {
                            code.extend([
                                0x0f,
                                [0xa3, 0xab, 0xb3, 0xbb][kind as usize],
                                if memory { 0x0b } else { 0xc8 },
                            ]);
                        }
                        let mut mem = Memory::default();
                        mem.map(CODE, 4096, READ | WRITE | EXECUTE).unwrap();
                        mem.map(DATA, 8192, READ | WRITE).unwrap();
                        mem.write(CODE, &code).unwrap();
                        mem.enable_execution_stats();
                        let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
                        uc.mem_map(CODE, 4096, Prot::ALL).unwrap();
                        uc.mem_map(DATA, 8192, Prot::ALL).unwrap();
                        uc.mem_write(CODE, &code).unwrap();
                        for offset in [-129i64, -65, -33, -17, -1, 0, 15, 16, 31, 32, 63, 64, 129] {
                            let data: Vec<u8> =
                                (0..8192).map(|n| ((n * 13 + 7) ^ (n >> 3)) as u8).collect();
                            mem.write(DATA, &data).unwrap();
                            uc.mem_write(DATA, &data).unwrap();
                            let mut cpu = Cpu64::new(CODE, DATA + 8192);
                            cpu.regs[0] = 0xa5f00f5a88776655;
                            cpu.regs[1] = offset as u64;
                            cpu.regs[3] = DATA + 4096;
                            cpu.rflags = 0xad7;
                            for (r, v) in [
                                (U::RAX, cpu.regs[0]),
                                (U::RCX, cpu.regs[1]),
                                (U::RBX, cpu.regs[3]),
                                (U::RFLAGS, cpu.rflags),
                            ] {
                                uc.reg_write(r, v).unwrap();
                            }
                            uc.emu_start(CODE, CODE + code.len() as u64, 0, 1).unwrap();
                            cpu.step(&mut mem).unwrap();
                            assert_eq!(
                                cpu.regs[0],
                                uc.reg_read(U::RAX).unwrap(),
                                "code={code:x?} offset={offset}"
                            );
                            assert_eq!(
                                cpu.rflags & 1,
                                uc.reg_read(U::RFLAGS).unwrap() & 1,
                                "code={code:x?} offset={offset}"
                            );
                            assert_eq!(cpu.rflags & !1, 0xad7 & !1);
                            let mut actual = vec![0; 8192];
                            mem.read(DATA, &mut actual).unwrap();
                            let reference = uc.mem_read_as_vec(DATA, 8192).unwrap();
                            assert_eq!(actual, reference, "code={code:x?} offset={offset}");
                        }
                        assert_eq!(mem.execution_stats().unwrap().fallback_steps, 0);
                        if memory {
                            for protection in [0x02, 0x104, 0x01] {
                                mem.virtual_protect(DATA, 8192, protection).unwrap();
                                let mut cpu = Cpu64::new(CODE, 0);
                                cpu.regs[3] = DATA + 4096;
                                cpu.regs[1] = 0;
                                cpu.rflags = 0xad7;
                                let before = cpu.clone();
                                let result = cpu.step(&mut mem);
                                if kind == 0 && protection == 0x02 {
                                    assert!(result.is_ok());
                                } else {
                                    assert!(result.is_err());
                                    assert_eq!(cpu, before);
                                }
                                mem.virtual_protect(DATA, 8192, 0x04).unwrap();
                            }
                            let mut cpu = Cpu64::new(CODE, 0);
                            cpu.regs[3] = DATA + 8191;
                            cpu.regs[1] = 0;
                            let before = cpu.clone();
                            assert!(cpu.step(&mut mem).is_err());
                            assert_eq!(cpu, before);
                        }
                    }
                }
            }
        }
    }
}
