//! PROPVARIANT ownership follows Wine 10.0 combase/combase.c.
//! Copyright 2005 Juan Lang; 2005-2006 Robert Shearman (for CodeWeavers).
//! Adapted for checked guest storage, LGPL-2.1-or-later; see THIRD_PARTY_NOTICES.
use super::{
    automation::{self, StringOperation},
    heaps,
    safe_arrays::{self, Error},
    ServiceError,
};
use crate::{
    machine::Mode,
    memory::{Memory, WRITE},
    process::Loaded,
};
use std::collections::BTreeSet;

const INVALID: u32 = 0x80070057;
const BAD_TYPE: u32 = 0x80020008;
const STORAGE_INVALID: u32 = 0x80030057;
const OOM: u32 = 0x8007000e;
const NOT_IMPLEMENTED: u32 = 0x80004001;
const LIMIT: usize = 16 * 1024 * 1024;
const NODES: usize = 4096;
type Result<T> = std::result::Result<T, Error>;
fn fail<T>(code: u32) -> Result<T> {
    Err(Error::Hresult(code))
}
fn width(mode: Mode) -> usize {
    mode as usize / 8
}
fn size(mode: Mode) -> usize {
    if mode == Mode::X86 {
        16
    } else {
        24
    }
}
fn counted_pointer(mode: Mode) -> usize {
    if mode == Mode::X86 {
        12
    } else {
        16
    }
}
fn pointer(bytes: &[u8], at: usize, mode: Mode) -> u64 {
    let mut raw = [0; 8];
    raw[..width(mode)].copy_from_slice(&bytes[at..at + width(mode)]);
    u64::from_le_bytes(raw)
}
fn put(bytes: &mut [u8], at: usize, p: u64, mode: Mode) {
    bytes[at..at + width(mode)].copy_from_slice(&p.to_le_bytes()[..width(mode)]);
}
fn tag(bytes: &[u8]) -> u16 {
    u16::from_le_bytes(bytes[..2].try_into().unwrap())
}
fn count(bytes: &[u8]) -> usize {
    u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize
}
fn valid(vt: u16) -> bool {
    match vt & 0xf000 {
        0 => matches!(vt, 0..=11 | 13..=14 | 16..=23 | 30..=31 | 64..=73),
        0x1000 => matches!(vt & 0xfff, 2..=8 | 10..=12 | 16..=21 | 30..=31 | 64 | 71..=72),
        0x2000 => matches!(vt & 0xfff, 2..=14 | 16..=23),
        _ => false,
    }
}
fn scalar(vt: u16) -> bool {
    matches!(vt, 0..=7 | 10..=11 | 14 | 16..=23 | 64)
}
fn element(vt: u16, mode: Mode) -> usize {
    match vt {
        16 | 17 => 1,
        2 | 11 | 18 => 2,
        3 | 4 | 10 | 19 => 4,
        5 | 6 | 7 | 20 | 21 | 64 => 8,
        72 => 16,
        8 | 30 | 31 => width(mode),
        12 => size(mode),
        71 => 8 + width(mode),
        _ => unreachable!("validated vector element"),
    }
}
fn raw(memory: &Memory, p: u64, n: usize) -> Result<Vec<u8>> {
    if n > LIMIT {
        return fail(OOM);
    }
    let mut bytes = vec![0; n];
    if n != 0 {
        memory.read(p, &mut bytes)?;
    }
    Ok(bytes)
}
fn string(memory: &Memory, p: u64, wide: bool) -> Result<Vec<u8>> {
    let unit = if wide { 2 } else { 1 };
    let mut bytes = Vec::new();
    for offset in (0..LIMIT).step_by(unit) {
        let address = p
            .checked_add(offset as u64)
            .ok_or(Error::Hresult(INVALID))?;
        let value = memory.read_int(address, unit)?;
        bytes.extend_from_slice(&value.to_le_bytes()[..unit]);
        if value == 0 {
            return Ok(bytes);
        }
    }
    fail(OOM)
}
fn heap_error(error: heaps::Error) -> Error {
    match error {
        heaps::Error::Windows(8) => Error::Hresult(OOM),
        heaps::Error::Windows(_) => Error::Hresult(INVALID),
        heaps::Error::Fault(fault) => Error::Service(fault.into()),
        heaps::Error::Backing => {
            Error::Service(ServiceError::Unavailable("property task backing was lost"))
        }
    }
}
fn record(mode: Mode, vt: u16, p: u64) -> Vec<u8> {
    let mut bytes = vec![0; size(mode)];
    bytes[..2].copy_from_slice(&vt.to_le_bytes());
    put(&mut bytes, 8, p, mode);
    bytes
}

#[derive(Default)]
struct Cleanup {
    tasks: BTreeSet<u64>,
    automation: safe_arrays::Cleanup,
    nodes: usize,
}
impl Cleanup {
    fn task(
        &mut self,
        p: u64,
        memory: &Memory,
        image: &Loaded,
        output: (u64, usize),
    ) -> Result<()> {
        if p == 0 {
            return Ok(());
        }
        if self.tasks.len() >= NODES || !self.tasks.insert(p) {
            return fail(INVALID);
        }
        image.heaps.check_task_free(memory, p).map_err(heap_error)?;
        let span = image.heaps.task_span(p).ok_or(Error::Hresult(INVALID))?;
        if output.0 < p + span && output.0.saturating_add(output.1 as u64) > p {
            return fail(INVALID);
        }
        Ok(())
    }
    fn clip(
        &mut self,
        bytes: &[u8],
        memory: &Memory,
        image: &Loaded,
        output: (u64, usize),
    ) -> Result<()> {
        self.nodes += 1;
        if self.nodes > NODES {
            return fail(INVALID);
        }
        self.task(pointer(bytes, 8, memory.mode()), memory, image, output)
    }
    fn value(
        &mut self,
        bytes: &[u8],
        memory: &mut Memory,
        image: &mut Loaded,
        output: (u64, usize),
        depth: usize,
    ) -> Result<()> {
        self.nodes += 1;
        if depth > 32 || self.nodes > NODES {
            return fail(INVALID);
        }
        let mode = memory.mode();
        let vt = tag(bytes);
        if !valid(vt) {
            return fail(BAD_TYPE);
        }
        let p = pointer(bytes, 8, mode);
        if vt & 0x1000 != 0 {
            let p = pointer(bytes, counted_pointer(mode), mode);
            self.task(p, memory, image, output)?;
            if p == 0 {
                return Ok(());
            }
            let base = vt & 0xfff;
            if matches!(base, 8 | 12 | 30 | 31 | 71) {
                let n = count(bytes)
                    .checked_mul(element(base, mode))
                    .ok_or(Error::Hresult(OOM))?;
                if n as u64 > image.heaps.task_span(p).unwrap() {
                    return fail(INVALID);
                }
                let data = raw(memory, p, n)?;
                for item in data.chunks_exact(element(base, mode)) {
                    if base == 12 {
                        self.value(item, memory, image, output, depth + 1)?;
                    } else if base == 71 {
                        self.clip(item, memory, image, output)?;
                    } else {
                        self.value(
                            &record(mode, base, pointer(item, 0, mode)),
                            memory,
                            image,
                            output,
                            depth + 1,
                        )?;
                    }
                }
            }
        } else if vt & 0x2000 != 0 || vt == 8 {
            self.automation
                .value(vt, bytes, memory, image, output, depth)?;
        } else if matches!(vt, 30 | 31 | 72) {
            self.task(p, memory, image, output)?;
        } else if matches!(vt, 65 | 70) {
            self.task(
                pointer(bytes, counted_pointer(mode), mode),
                memory,
                image,
                output,
            )?;
        } else if vt == 71 {
            self.task(p, memory, image, output)?;
            if p != 0 {
                if image.heaps.task_span(p).unwrap() < (8 + width(mode)) as u64 {
                    return fail(INVALID);
                }
                self.clip(&raw(memory, p, 8 + width(mode))?, memory, image, output)?;
            }
        } else if !scalar(vt) && p != 0 {
            return fail(NOT_IMPLEMENTED);
        }
        Ok(())
    }
    fn apply(self, memory: &mut Memory, image: &mut Loaded) -> Result<()> {
        self.automation.apply(memory, image)?;
        for p in self.tasks {
            image.heaps.free_task(memory, p).map_err(heap_error)?;
        }
        Ok(())
    }
}

enum Allocation {
    Task(u64),
    Bstr(u64),
    Array(u64),
}
#[derive(Default)]
struct Copies {
    allocations: Vec<Allocation>,
    bytes: usize,
    nodes: usize,
}
impl Copies {
    fn task(&mut self, bytes: &[u8], memory: &mut Memory, image: &mut Loaded) -> Result<u64> {
        if bytes.len() > LIMIT - self.bytes || self.allocations.len() >= NODES {
            return fail(OOM);
        }
        let p = image
            .heaps
            .allocate_task(memory, bytes.len() as u64, Some(bytes))
            .map_err(heap_error)?;
        self.bytes += bytes.len();
        self.allocations.push(Allocation::Task(p));
        Ok(p)
    }
    fn clip(&mut self, bytes: &[u8], memory: &mut Memory, image: &mut Loaded) -> Result<Vec<u8>> {
        self.nodes += 1;
        if self.nodes > NODES {
            return fail(OOM);
        }
        let mut bytes = bytes.to_vec();
        let n = u32::from_le_bytes(bytes[..4].try_into().unwrap())
            .checked_sub(4)
            .ok_or(Error::Hresult(INVALID))?;
        let p = pointer(&bytes, 8, memory.mode());
        if p != 0 {
            let data = raw(memory, p, n as usize)?;
            let next = self.task(&data, memory, image)?;
            put(&mut bytes, 8, next, memory.mode());
        }
        Ok(bytes)
    }
    fn value(
        &mut self,
        bytes: &[u8],
        memory: &mut Memory,
        image: &mut Loaded,
        depth: usize,
    ) -> Result<Vec<u8>> {
        self.nodes += 1;
        if depth > 32 || self.nodes > NODES {
            return fail(OOM);
        }
        let mode = memory.mode();
        let vt = tag(bytes);
        if !valid(vt) {
            return fail(BAD_TYPE);
        }
        let mut bytes = bytes.to_vec();
        let p = pointer(&bytes, 8, mode);
        if vt & 0x1000 != 0 {
            let p = pointer(&bytes, counted_pointer(mode), mode);
            let base = vt & 0xfff;
            let n = count(&bytes)
                .checked_mul(element(base, mode))
                .ok_or(Error::Hresult(OOM))?;
            let mut data = raw(memory, p, n)?;
            if matches!(base, 8 | 12 | 30 | 31 | 71) {
                for item in data.chunks_exact_mut(element(base, mode)) {
                    if base == 12 {
                        item.copy_from_slice(&self.value(item, memory, image, depth + 1)?);
                    } else if base == 71 {
                        item.copy_from_slice(&self.clip(item, memory, image)?);
                    } else {
                        let next = self.value(
                            &record(mode, base, pointer(item, 0, mode)),
                            memory,
                            image,
                            depth + 1,
                        )?;
                        put(item, 0, pointer(&next, 8, mode), mode);
                    }
                }
            }
            let next = if n == 0 {
                0
            } else {
                self.task(&data, memory, image)?
            };
            put(&mut bytes, counted_pointer(mode), next, mode);
        } else if vt & 0x2000 != 0 || vt == 8 {
            if self.allocations.len() >= NODES {
                return fail(OOM);
            }
            bytes = safe_arrays::clone_value(&bytes, memory, image, &mut BTreeSet::new())?;
            let p = pointer(&bytes, 8, mode);
            if p != 0 {
                self.allocations.push(if vt == 8 {
                    Allocation::Bstr(p)
                } else {
                    Allocation::Array(p)
                });
            }
        } else if matches!(vt, 30 | 31 | 65 | 70 | 71 | 72) {
            let at = if matches!(vt, 65 | 70) {
                counted_pointer(mode)
            } else {
                8
            };
            let p = pointer(&bytes, at, mode);
            if p != 0 {
                let data = match vt {
                    30 | 31 => string(memory, p, vt == 31)?,
                    72 => raw(memory, p, 16)?,
                    71 => self.clip(&raw(memory, p, 8 + width(mode))?, memory, image)?,
                    _ => raw(memory, p, count(&bytes))?,
                };
                let next = self.task(&data, memory, image)?;
                put(&mut bytes, at, next, mode);
            }
        } else if !scalar(vt) && p != 0 {
            return fail(NOT_IMPLEMENTED);
        }
        Ok(bytes)
    }
    fn rollback(self, memory: &mut Memory, image: &mut Loaded) -> Result<()> {
        for allocation in self.allocations.into_iter().rev() {
            match allocation {
                Allocation::Task(p) => image.heaps.free_task(memory, p).map_err(heap_error)?,
                Allocation::Bstr(p) => {
                    automation::string(StringOperation::Free(p), memory, image)?;
                }
                Allocation::Array(p) => safe_arrays::destroy(p, memory, image)?,
            }
        }
        Ok(())
    }
}

fn operation(copy: bool, a: &[u64; 16], memory: &mut Memory, image: &mut Loaded) -> Result<u64> {
    if !copy && a[0] == 0 {
        return Ok(0);
    }
    let size = size(memory.mode());
    memory.check(a[0], size, WRITE)?;
    let bytes = raw(memory, a[if copy { 1 } else { 0 }], size)?;
    if !valid(tag(&bytes)) {
        if !copy {
            memory.write(a[0], &vec![0; size])?;
        }
        return fail(if copy { BAD_TYPE } else { STORAGE_INVALID });
    }
    if copy {
        if a[0] == a[1] {
            return Ok(0);
        }
        let mut copies = Copies::default();
        let result = (|| {
            let bytes = copies.value(&bytes, memory, image, 0)?;
            Cleanup::default().value(&bytes, memory, image, (a[0], size), 0)?;
            memory.write(a[0], &bytes)?;
            Ok(0)
        })();
        if result.is_err() {
            copies.rollback(memory, image)?;
        }
        result
    } else {
        let mut plan = Cleanup::default();
        plan.value(&bytes, memory, image, (a[0], size), 0)?;
        plan.apply(memory, image)?;
        memory.write(a[0], &vec![0; size])?;
        Ok(0)
    }
}
pub(super) fn dispatch(
    copy: bool,
    a: &[u64; 16],
    memory: &mut Memory,
    image: &mut Loaded,
) -> std::result::Result<u64, ServiceError> {
    match operation(copy, a, memory, image) {
        Ok(value) => Ok(value),
        Err(Error::Hresult(code)) => Ok(u64::from(code)),
        Err(Error::Service(error)) => Err(error),
    }
}
