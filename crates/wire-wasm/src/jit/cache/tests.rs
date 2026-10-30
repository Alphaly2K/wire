use super::*;
use crate::{
    jit,
    machine::{Machine, Mode},
    memory::{EXECUTE, PAGE, READ, WRITE},
};

fn source(memory: &mut Memory, base: u64) {
    memory.map(base, PAGE, READ | WRITE | EXECUTE).unwrap();
    memory
        .write(base, &[0xb8, 7, 0, 0, 0, 0x83, 0xc0, 1, 0xcc])
        .unwrap();
}

#[test]
fn retirement_batches_preserve_pins_and_recover_after_overflow() {
    let mut memory = Memory::default();
    source(&mut memory, 0x10000);
    let mut cache = CodeCache::default();
    let first = cache
        .insert(jit::compile(&mut memory, 0x10000).unwrap())
        .unwrap();
    assert_eq!(cache.take_retired(&memory)[0], 0);
    assert!(cache.pin(&memory, first));
    memory.write(0x10001, &[9]).unwrap();
    assert_eq!(&cache.take_retired(&memory)[..2], &[1, first]);
    assert!(cache.code(first).is_some());
    assert!(cache.charged_bytes() > 0);
    cache.release(&memory, first);
    assert_eq!(cache.take_retired(&memory)[0], 0);
    assert!(cache.is_empty());
    assert_eq!(cache.charged_bytes(), 0);

    for _ in 0..=RETIRED_SLOTS {
        cache
            .insert(jit::compile(&mut memory, 0x10000).unwrap())
            .unwrap();
    }
    cache.clear();
    assert_eq!(cache.take_retired(&memory)[0], u64::MAX);
    assert_eq!(cache.take_retired(&memory)[0], 0);
    assert_eq!(cache.charged_bytes(), 0);
    cache.links.epoch.set(u64::MAX);
    assert_eq!(cache.take_retired(&memory)[0], u64::MAX);
}

#[test]
fn preparation_reuses_boundaries_and_revocation_retains_active_owners() {
    let base = 0x10000;
    let mut machine = Machine::new(Mode::X64, base, 0);
    source(&mut machine.memory, base);
    let original = machine.prepare().unwrap();
    for pc in [base, base + 5, base + 8] {
        machine.state.ip = pc;
        assert_eq!(machine.prepare().unwrap(), original);
    }
    machine.state.ip = base + 1;
    assert!(!machine.ready(original));
    assert_eq!(machine.code_cache.len(), 1);
    assert!(machine.code_cache.pin(&machine.memory, original));
    machine.memory.write(base + 1, &[8]).unwrap();
    assert!(!machine.ready(original));
    let retained = machine.code_cache.charged_bytes();
    assert!(machine.code(original).is_some());
    machine.state.ip = base;
    let replacement = machine.prepare().unwrap();
    assert_ne!(original, replacement);
    assert!(machine.code_cache.charged_bytes() > retained);
    machine.code_cache.release(&machine.memory, original);
    assert!(machine.code(original).is_none());
    assert!(machine.ready(replacement));
    assert_eq!(machine.code_cache.len(), 1);
    let mut other = Machine::new(Mode::X64, base, 0);
    source(&mut other.memory, base);
    let foreign = other.prepare().unwrap();
    assert_ne!(foreign, replacement);
    assert!(!machine.ready(foreign));
}

#[test]
fn new_regions_link_to_cached_code_without_copying_its_source() {
    for mode in [Mode::X86, Mode::X64] {
        let mut machine = Machine::new(mode, 0x10000, 0);
        for base in [0x10000, 0x20000] {
            machine
                .memory
                .map(base, PAGE, READ | WRITE | EXECUTE)
                .unwrap();
        }
        machine
            .memory
            .write(0x10000, &[0xb8, 7, 0, 0, 0, 0xc3])
            .unwrap();
        let callee = machine.prepare().unwrap();
        let displacement = (0x10000i32 - 0x20005).to_le_bytes();
        let mut caller = vec![0xe8];
        caller.extend(displacement);
        caller.push(0xcc);
        machine.memory.write(0x20000, &caller).unwrap();
        machine.state.ip = 0x20000;
        let caller = machine.prepare().unwrap();
        assert!(!machine.code(caller).unwrap().can_enter(0x10000));
        assert_eq!(
            machine.code_cache.find(&machine.memory, 0x10000),
            Some(callee)
        );
        machine.memory.write(0x10001, &[8]).unwrap();
        assert!(machine.code_cache.live(&machine.memory, caller));
        machine.state.ip = 0x10000;
        let replacement = machine.prepare().unwrap();
        assert_ne!(replacement, callee);
        assert!(!machine.code_cache.live(&machine.memory, callee));
        assert!(machine.code_cache.live(&machine.memory, caller));
    }
}

#[test]
fn byte_budget_respects_pins_and_closing_reclaims_after_release() {
    let mut memory = Memory::default();
    for base in [0x10000, 0x20000, 0x30000] {
        source(&mut memory, base);
    }
    let first = jit::compile(&mut memory, 0x10000).unwrap();
    let second = jit::compile(&mut memory, 0x20000).unwrap();
    let third = jit::compile(&mut memory, 0x30000).unwrap();
    let first_charge = first.storage_charge();
    let limit = first_charge + second.storage_charge().max(third.storage_charge());
    let mut cache = CodeCache::new(limit);
    let first = cache.insert(first).unwrap();
    let second = cache.insert(second).unwrap();
    assert!(cache.pin(&memory, first));
    let third = cache.insert(third).unwrap();
    assert!(cache.live(&memory, first));
    assert!(!cache.live(&memory, second));
    assert!(cache.live(&memory, third));
    assert!(cache.charged_bytes() <= limit);
    assert_eq!(cache.set_limit(first_charge - 1), Err(CacheError::Capacity));
    assert_eq!(cache.limit(), limit);
    cache.release(&memory, first);
    assert!(cache.pin(&memory, third));
    cache.clear();
    assert!(!cache.live(&memory, first));
    assert!(!cache.live(&memory, third));
    assert_eq!(cache.len(), 1);
    assert!(cache.has_pins());
    cache.release(&memory, third);
    assert!(cache.is_empty());
    assert!(!cache.has_pins());
    assert_eq!(cache.charged_bytes(), 0);
}

#[test]
fn evicting_overlapping_code_preserves_new_entry_aliases() {
    let mut memory = Memory::default();
    source(&mut memory, 0x10000);
    let mut cache = CodeCache::default();
    let first = cache
        .insert(jit::compile(&mut memory, 0x10000).unwrap())
        .unwrap();
    let second_code = jit::compile(&mut memory, 0x10005).unwrap();
    let charge = second_code.storage_charge();
    let second = cache.insert(second_code).unwrap();
    assert_eq!(cache.find(&memory, 0x10005), Some(second));
    cache.set_limit(charge).unwrap();
    assert!(!cache.live(&memory, first));
    assert_eq!(cache.find(&memory, 0x10005), Some(second));
    assert_eq!(cache.find(&memory, 0x10000), None);
    assert!(!cache.pin(&memory, first));
}

#[test]
fn resident_links_hold_unique_leases_and_revoke_before_releasing_storage() {
    let mut machine = Machine::new(Mode::X64, 0x10000, 0);
    for base in [0x10000, 0x20000] {
        source(&mut machine.memory, base);
    }
    let first = machine.prepare().unwrap();
    assert!(!machine.begin_run(first).unwrap());
    assert!(!machine.code_cache.publish(&machine.memory, first, 0));
    assert!(machine.code_cache.publish(&machine.memory, first, 1));
    machine.state.ip = 0x20000;
    let second = machine.prepare().unwrap();
    assert!(machine.code_cache.publish(&machine.memory, second, 2));
    machine.state.ip = 0x10000;
    assert_eq!(machine.link(0x10000), 0);
    assert!(machine.begin_run(first).unwrap());
    let nonce = machine.code_cache.links().run.get();
    assert!(matches!(
        machine.begin_run(first),
        Err(crate::machine::BeginError::Busy)
    ));
    for _ in 0..3 {
        assert_eq!(machine.link(0x10000), 1);
        assert_eq!(machine.link(0x20000), 2);
    }
    assert_eq!(machine.code_cache.pins, 2);
    let epoch = machine.code_cache.epoch();
    assert!(machine.code_cache.publish(&machine.memory, second, 3));
    assert!(machine.code_cache.epoch() > epoch);
    assert_eq!(machine.link(0x20000), 3);
    machine.memory.write(0x20001, &[9]).unwrap();
    assert_eq!(machine.link(0x20000), 0);
    assert!(!machine.code_cache.publish(&machine.memory, second, 4));
    assert!(machine.code(second).is_some());
    assert_eq!(machine.code_cache.set_limit(0), Err(CacheError::Capacity));
    machine.end_run();
    assert_eq!(machine.code_cache.links().run.get(), 0);
    assert!(machine.code(second).is_none());
    assert!(!machine.has_activity());
    assert!(machine.begin_run(first).unwrap());
    assert!(machine.code_cache.links().run.get() > nonce);
    machine.code_cache.clear();
    assert!(machine.has_activity());
    assert_eq!(machine.link(0x10000), 0);
    assert!(machine.code(first).is_some());
    machine.end_run();
    machine.end_run();
    assert!(!machine.has_activity());
    assert_eq!(machine.code_cache.charged_bytes(), 0);
}
