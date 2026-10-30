//! Resident dispatch keeps architectural locals across installed Wasm chunks.
use super::*;
use crate::memory::TranslationCache;
use links::{Link, LinkCache, LINK_SLOTS};
use std::mem::size_of;

const LINKS: u32 = RESUME + 1;
const SLOT: u32 = LINKS + 1;

fn lookup(c: &mut Code) {
    c.get(LINKS);
    c.i32(offset_of!(LinkCache, entries) as u32);
    c.op(0x6a);
    c.get(IP);
    for shift in [16, 32] {
        c.get(IP);
        c.i64(shift);
        c.op(0x88);
        c.op(0x85);
    }
    c.op(0xa7);
    c.i32(LINK_SLOTS as u32 - 1);
    c.op(0x71);
    c.i32(size_of::<Link>() as u32);
    c.op(0x6c);
    c.op(0x6a);
    c.set(TRANSLATION);
    c.get(TRANSLATION);
    c.load64(offset_of!(Link, pc));
    c.get(IP);
    c.op(0x51);
    for (field, base, authority) in [
        (offset_of!(Link, run), LINKS, offset_of!(LinkCache, run)),
        (
            offset_of!(Link, code_epoch),
            LINKS,
            offset_of!(LinkCache, epoch),
        ),
        (
            offset_of!(Link, mapping_epoch),
            2,
            offset_of!(TranslationCache, epoch),
        ),
        (
            offset_of!(Link, source_epoch),
            2,
            offset_of!(TranslationCache, source),
        ),
    ] {
        c.get(TRANSLATION);
        c.load64(field);
        c.get(base);
        c.load64(authority);
        c.op(0x51);
        c.op(0x71);
    }
    for (base, field) in [
        (LINKS, offset_of!(LinkCache, epoch)),
        (2, offset_of!(TranslationCache, epoch)),
    ] {
        c.get(base);
        c.load64(field);
        c.i64(u64::MAX);
        c.op(0x52);
        c.op(0x71);
    }
    c.get(LINKS);
    c.load64(offset_of!(LinkCache, run));
    c.i64(0);
    c.op(0x52);
    c.op(0x71);
    c.if_i32();
    c.get(TRANSLATION);
    c.memory(0x28, offset_of!(Link, slot));
    c.otherwise();
    c.get(3);
    c.get(IP);
    c.call(1);
    c.end();
    c.set(SLOT);
}

pub(super) fn module() -> Vec<u8> {
    let mut c = Code::default();
    state::load(&mut c);
    c.get(3);
    c.call(0);
    c.set(LINKS);
    c.get(LINKS);
    c.op(0x45);
    c.if_void();
    c.i32(Exit::Dispatch as u32);
    c.op(0x0f);
    c.end();
    c.call(4);
    let out = c.block();
    let again = c.open(0x03, 0x40);
    c.get(WORK);
    c.get(1);
    c.op(0x5a);
    c.if_void();
    c.i32(Exit::Budget as u32);
    c.set(STATUS);
    c.branch(out);
    c.end();
    lookup(&mut c);
    c.get(SLOT);
    c.op(0x45);
    c.if_void();
    c.branch(out);
    c.end();
    c.get(WORK);
    c.set(A);
    c.get(IP);
    c.set(B);
    state::arguments(&mut c);
    c.get(SLOT);
    c.indirect(1);
    state::receive(&mut c);
    c.get(STATUS);
    c.i32(Exit::Dispatch as u32);
    c.op(0x47);
    c.get(STATUS);
    c.i32(Exit::SourceChanged as u32);
    c.op(0x47);
    c.op(0x71);
    c.if_void();
    c.branch(out);
    c.end();
    c.get(WORK);
    c.get(A);
    c.op(0x51);
    c.get(IP);
    c.get(B);
    c.op(0x51);
    c.op(0x71);
    c.if_void();
    c.branch(out);
    c.end();
    c.branch(again);
    c.end();
    c.end();
    c.get(0);
    c.call(5);
    state::finish(&mut c, 6);
    let mut module = b"\0asm\x01\0\0\0".to_vec();
    let mut types = vec![7];
    binary::signature(&mut types, &[0x7f, 0x7e, 0x7f, 0x7f], &[0x7f]);
    state::signature(&mut types);
    binary::signature(&mut types, &[0x7f], &[0x7f]);
    binary::signature(&mut types, &[0x7f, 0x7e], &[0x7f]);
    binary::signature(&mut types, &[], &[]);
    binary::signature(&mut types, &[0x7f], &[]);
    flags::signature(&mut types);
    binary::section(&mut module, 1, &types);
    let mut imports = vec![4];
    binary::name(&mut imports, "core");
    binary::name(&mut imports, "memory");
    imports.extend_from_slice(&[2, 0, 1]);
    binary::name(&mut imports, "jit");
    binary::name(&mut imports, "functions");
    imports.extend_from_slice(&[1, 0x70, 0, 1]);
    for (name, kind) in [("wire_links", 2), ("wire_link", 3)] {
        binary::name(&mut imports, "core");
        binary::name(&mut imports, name);
        imports.extend_from_slice(&[0, kind]);
    }
    binary::section(&mut module, 2, &imports);
    binary::section(&mut module, 3, &[5, 0, 0, 4, 5, 6]);
    vector::bank::declare(&mut module);
    let mut exports = vec![3 + vector::bank::GLOBALS];
    binary::name(&mut exports, "run");
    exports.extend_from_slice(&[0, 2]);
    binary::name(&mut exports, "lookup");
    exports.extend_from_slice(&[0, 3]);
    binary::name(&mut exports, "materialize");
    exports.extend_from_slice(&[0, 6]);
    vector::bank::exports(&mut exports);
    binary::section(&mut module, 7, &exports);
    let mut functions = vec![5];
    functions.extend(c.function_body(&[(24, 0x7e), (1, 0x7f), (6, 0x7e), (8, 0x7f)]));
    functions.extend(access::lookup_function().function_body(&[(1, 0x7f)]));
    let mut reset = Code::default();
    vector::bank::reset(&mut reset);
    functions.extend(reset.function_body(&[]));
    let mut commit = Code::default();
    vector::bank::commit(&mut commit);
    functions.extend(commit.function_body(&[]));
    functions.extend(flags::function().function_body(&[(22, 0x7e), (1, 0x7f), (3, 0x7e)]));
    binary::section(&mut module, 10, &functions);
    module
}
