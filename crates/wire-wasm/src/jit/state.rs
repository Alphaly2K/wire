//! Architectural values carried between Wasm chunks, with one load/commit at
//! the resident execution boundary. Scratch temporaries are private to chunks.
use super::*;

pub(super) const CHUNK_LOCALS: &[(u32, u8)] = &[(6, 0x7e), (6, 0x7f)];

pub(super) fn signature(out: &mut Vec<u8>) {
    let mut params = vec![0x7e; TAG as usize + 1];
    for index in [0, 2, 3, TAG as usize] {
        params[index] = 0x7f;
    }
    let mut results = vec![0x7e; (SIGN - GPR + 1) as usize];
    results.extend_from_slice(&[0x7f, 0x7f]);
    binary::signature(out, &params, &results);
}

pub(super) fn load(c: &mut Code) {
    c.get(1);
    c.i64(0);
    c.op(0x52);
    c.if_void();
    c.get(0);
    c.i32(0);
    c.memory(0x36, offset_of!(State, exception));
    c.end();
    for register in 0..16 {
        c.get(0);
        c.load64(offset_of!(State, regs) + register * 8);
        c.set(GPR + register as u32);
    }
    for (offset, local) in [
        (offset_of!(State, flags), FLAGS),
        (offset_of!(State, ip), IP),
    ] {
        c.get(0);
        c.load64(offset);
        c.set(local);
    }
}

pub(super) fn arguments(c: &mut Code) {
    for local in 0..=TAG {
        c.get(local);
    }
}

pub(super) fn results(c: &mut Code) {
    for local in GPR..=SIGN {
        c.get(local);
    }
    c.get(TAG);
    c.get(STATUS);
}

pub(super) fn receive(c: &mut Code) {
    c.set(STATUS);
    c.set(TAG);
    for local in (GPR..=SIGN).rev() {
        c.set(local);
    }
}

pub(super) fn finish(c: &mut Code, materialize: u32) {
    flags::materialize(c, materialize);
    for register in 0..16 {
        c.get(0);
        c.get(GPR + register as u32);
        c.store64(offset_of!(State, regs) + register * 8);
    }
    for (offset, local) in [
        (offset_of!(State, flags), FLAGS),
        (offset_of!(State, ip), IP),
    ] {
        c.get(0);
        c.get(local);
        c.store64(offset);
    }
    for (offset, counter) in [
        (offset_of!(State, work), WORK),
        (offset_of!(State, retired), RETIRED),
    ] {
        c.get(0);
        c.get(0);
        c.load64(offset);
        c.get(counter);
        c.op(0x7c);
        c.store64(offset);
    }
    c.get(STATUS);
}
