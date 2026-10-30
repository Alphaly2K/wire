//! SAFEARRAY bounds and element ownership follow Wine 10.0 oleaut32/safearray.c.
//! Copyright 1999 Sylvain St-Germain; 2002-2003 Marcus Meissner; 2003 Jon Griffiths.
//! Adapted 2026-10-29 for checked guest storage, LGPL-2.1-or-later; see THIRD_PARTY_NOTICES.
use super::{
    automation::{self, StringOperation},
    call::Call,
    catalog::{SafeArrayApi as A, VariantApi},
    ServiceError,
};
use crate::{
    machine::Mode,
    memory::{Fault, Memory, PAGE, READ, WRITE},
    process::Loaded,
};
use std::collections::{BTreeMap, BTreeSet};

const INVALID: u32 = 0x80070057;
const OOM: u32 = 0x8007000e;
const BAD_INDEX: u32 = 0x8002000b;
const BAD_TYPE: u32 = 0x80020008;
const LOCKED: u32 = 0x8002000d;
const UNEXPECTED: u32 = 0x8000ffff;
const LIMIT: usize = 64 * 1024 * 1024;

#[derive(Debug)]
pub(super) enum Error {
    Hresult(u32),
    Service(ServiceError),
}
impl From<Fault> for Error {
    fn from(value: Fault) -> Self {
        Self::Service(value.into())
    }
}
impl From<ServiceError> for Error {
    fn from(value: ServiceError) -> Self {
        Self::Service(value)
    }
}
impl From<automation::Failure> for Error {
    fn from(value: automation::Failure) -> Self {
        match value {
            automation::Failure::Service(error) => Self::Service(error),
            automation::Failure::Windows(code) => Self::Hresult(if code == 8 {
                OOM
            } else if code & 0x80000000 != 0 {
                code
            } else {
                INVALID
            }),
        }
    }
}
type Result<T> = std::result::Result<T, Error>;
fn invalid<T>() -> Result<T> {
    Err(Error::Hresult(INVALID))
}
fn width(mode: Mode) -> usize {
    mode as usize / 8
}
fn variant_size(mode: Mode) -> usize {
    if mode == Mode::X86 {
        16
    } else {
        24
    }
}
fn header(mode: Mode) -> usize {
    if mode == Mode::X86 {
        16
    } else {
        24
    }
}
fn pointer(bytes: &[u8]) -> u64 {
    let mut value = [0; 8];
    value[..bytes.len()].copy_from_slice(bytes);
    u64::from_le_bytes(value)
}
fn put(bytes: &mut [u8], at: usize, size: usize, value: u64) {
    bytes[at..at + size].copy_from_slice(&value.to_le_bytes()[..size]);
}
fn scalar(vt: u16) -> bool {
    matches!(vt, 0..=7 | 10..=11 | 14 | 16..=23)
}
fn element(vt: u16, mode: Mode) -> Option<usize> {
    Some(match vt {
        16 | 17 => 1,
        2 | 11 | 18 => 2,
        3 | 4 | 10 | 19 | 22 | 23 => 4,
        5 | 6 | 7 | 20 | 21 => 8,
        14 => 16,
        8 | 37 | 38 => width(mode),
        12 => variant_size(mode),
        _ => return None,
    })
}

#[derive(Clone)]
struct Array {
    vt: u16,
    bounds: Vec<(u32, i32)>,
    size: usize,
    bytes: usize,
    data: u64,
    blocks: Vec<u64>,
    charge: usize,
    vector: bool,
}
impl Array {
    fn descriptor(&self, mode: Mode) -> Vec<u8> {
        let mut bytes = vec![0; header(mode) + 8 * self.bounds.len()];
        let features =
            0x80 | if self.vt == 8 {
                0x100
            } else if self.vt == 12 {
                0x800
            } else {
                0
            } | if self.vector { 0x2000 } else { 0 };
        put(&mut bytes, 0, 2, self.bounds.len() as u64);
        put(&mut bytes, 2, 2, features);
        put(&mut bytes, 4, 4, self.size as u64);
        put(
            &mut bytes,
            if mode == Mode::X86 { 12 } else { 16 },
            width(mode),
            self.data,
        );
        for (n, &(count, lower)) in self.bounds.iter().rev().enumerate() {
            put(&mut bytes, header(mode) + n * 8, 4, u64::from(count));
            put(
                &mut bytes,
                header(mode) + n * 8 + 4,
                4,
                u64::from(lower as u32),
            );
        }
        bytes
    }
}
#[derive(Default)]
pub(crate) struct Arrays {
    entries: BTreeMap<u64, Array>,
    charged: usize,
}

fn view(memory: &Memory, image: &Loaded, p: u64) -> Result<(Array, u32)> {
    let value = image
        .safe_arrays
        .entries
        .get(&p)
        .ok_or(Error::Hresult(INVALID))?
        .clone();
    let mut bytes = value.descriptor(memory.mode());
    let expected = bytes.clone();
    memory.read(p, &mut bytes)?;
    let locks = pointer(&bytes[8..12]) as u32;
    bytes[8..12].fill(0);
    if bytes != expected || memory.read_int(p - 4, 4)? != u64::from(value.vt) {
        return invalid();
    }
    Ok((value, locks))
}
fn string(operation: StringOperation, memory: &mut Memory, image: &mut Loaded) -> Result<u64> {
    Ok(automation::string(operation, memory, image)?)
}
fn rounded(size: usize) -> usize {
    size.max(1).div_ceil(PAGE as usize) * PAGE as usize
}

fn create(
    memory: &mut Memory,
    image: &mut Loaded,
    vt: u16,
    bounds: Vec<(u32, i32)>,
    vector: bool,
) -> Result<u64> {
    if bounds.is_empty() || bounds.len() > 32 {
        return invalid();
    }
    let size = element(vt, memory.mode()).ok_or(Error::Hresult(BAD_TYPE))?;
    let cells = bounds
        .iter()
        .try_fold(1u64, |n, (count, _)| n.checked_mul(u64::from(*count)))
        .ok_or(Error::Hresult(OOM))?;
    let bytes = cells
        .checked_mul(size as u64)
        .filter(|n| *n <= 16 * 1024 * 1024)
        .ok_or(Error::Hresult(OOM))? as usize;
    let descriptor = 16 + header(memory.mode()) + bounds.len() * 8;
    let charge = if vector {
        rounded(descriptor + bytes)
    } else {
        rounded(descriptor) + rounded(bytes)
    };
    if image.safe_arrays.entries.len() >= 1024 || charge > LIMIT - image.safe_arrays.charged {
        return Err(Error::Hresult(OOM));
    }
    let base = image
        .data_pages
        .allocate(memory, (descriptor + if vector { bytes } else { 0 }) as u64);
    if base == 0 {
        return Err(Error::Hresult(OOM));
    }
    let data = if vector {
        base + descriptor as u64
    } else {
        image.data_pages.allocate(memory, bytes as u64)
    };
    if data == 0 {
        image.data_pages.free(memory, base)?;
        return Err(Error::Hresult(OOM));
    }
    let p = base + 16;
    let value = Array {
        vt,
        bounds,
        size,
        bytes,
        data,
        blocks: if vector { vec![base] } else { vec![base, data] },
        charge,
        vector,
    };
    memory.write_int(p - 4, 4, u64::from(vt))?;
    memory.write(p, &value.descriptor(memory.mode()))?;
    image.safe_arrays.charged += charge;
    image.safe_arrays.entries.insert(p, value);
    Ok(p)
}

#[derive(Default)]
pub(super) struct Cleanup {
    arrays: Vec<(u64, Array)>,
    strings: BTreeSet<u64>,
    seen: BTreeSet<u64>,
}
impl Cleanup {
    pub(super) fn value(
        &mut self,
        vt: u16,
        bytes: &[u8],
        memory: &mut Memory,
        image: &mut Loaded,
        preserve: (u64, usize),
        depth: usize,
    ) -> Result<()> {
        if vt & 0x4000 != 0 {
            return Ok(());
        }
        if vt & 0x2000 != 0 {
            return self.array(
                pointer(&bytes[8..8 + width(memory.mode())]),
                memory,
                image,
                preserve,
                depth + 1,
            );
        }
        if vt == 8 {
            return self.bstr(
                pointer(&bytes[8..8 + width(memory.mode())]),
                memory,
                image,
                preserve,
            );
        }
        if scalar(vt) {
            Ok(())
        } else {
            Err(Error::Hresult(BAD_TYPE))
        }
    }
    fn bstr(
        &mut self,
        p: u64,
        memory: &mut Memory,
        image: &mut Loaded,
        preserve: (u64, usize),
    ) -> Result<()> {
        if p != 0 {
            if !self.strings.insert(p) {
                return invalid();
            }
            string(
                StringOperation::Check(p, preserve.0, preserve.1),
                memory,
                image,
            )?;
        }
        Ok(())
    }
    fn array(
        &mut self,
        p: u64,
        memory: &mut Memory,
        image: &mut Loaded,
        preserve: (u64, usize),
        depth: usize,
    ) -> Result<()> {
        if p == 0 {
            return Ok(());
        }
        if depth > 32 || !self.seen.insert(p) {
            return invalid();
        }
        let (value, locks) = view(memory, image, p)?;
        if locks != 0 {
            return Err(Error::Hresult(LOCKED));
        }
        for &base in &value.blocks {
            image.data_pages.check_free(memory, base)?;
            let span = image.data_pages.span(base).ok_or(Error::Hresult(INVALID))?;
            if preserve.1 != 0
                && preserve.0 < base + span
                && preserve.0.saturating_add(preserve.1 as u64) > base
            {
                return invalid();
            }
        }
        if matches!(value.vt, 8 | 12) {
            let mut bytes = vec![0; value.bytes];
            if !bytes.is_empty() {
                memory.read(value.data, &mut bytes)?;
            }
            for item in bytes.chunks_exact(value.size) {
                if value.vt == 8 {
                    self.bstr(pointer(item), memory, image, preserve)?;
                } else {
                    self.value(
                        pointer(&item[..2]) as u16,
                        item,
                        memory,
                        image,
                        preserve,
                        depth,
                    )?;
                }
            }
        }
        self.arrays.push((p, value));
        Ok(())
    }
    pub(super) fn apply(self, memory: &mut Memory, image: &mut Loaded) -> Result<()> {
        for p in self.strings {
            string(StringOperation::Free(p), memory, image)?;
        }
        for (p, value) in self.arrays {
            for base in value.blocks {
                image.data_pages.free(memory, base)?;
            }
            image.safe_arrays.charged -= value.charge;
            image.safe_arrays.entries.remove(&p);
        }
        Ok(())
    }
}
pub(super) fn destroy(p: u64, memory: &mut Memory, image: &mut Loaded) -> Result<()> {
    let mut plan = Cleanup::default();
    plan.array(p, memory, image, (0, 0), 0)?;
    plan.apply(memory, image)
}
pub(super) fn clone_value(
    bytes: &[u8],
    memory: &mut Memory,
    image: &mut Loaded,
    active: &mut BTreeSet<u64>,
) -> Result<Vec<u8>> {
    let mut bytes = bytes.to_vec();
    let vt = pointer(&bytes[..2]) as u16;
    if vt & 0x4000 != 0 {
        return Ok(bytes);
    }
    if vt & 0x2000 != 0 {
        let p = copy(
            pointer(&bytes[8..8 + width(memory.mode())]),
            memory,
            image,
            active,
        )?;
        put(&mut bytes, 8, width(memory.mode()), p);
    } else if vt == 8 {
        let p = string(
            StringOperation::Duplicate(Some(pointer(&bytes[8..8 + width(memory.mode())]))),
            memory,
            image,
        )?;
        put(&mut bytes, 8, width(memory.mode()), p);
    } else if !scalar(vt) {
        return Err(Error::Hresult(BAD_TYPE));
    }
    Ok(bytes)
}
fn copy(
    p: u64,
    memory: &mut Memory,
    image: &mut Loaded,
    active: &mut BTreeSet<u64>,
) -> Result<u64> {
    if p == 0 {
        return Ok(0);
    }
    if active.len() >= 32 || !active.insert(p) {
        return invalid();
    }
    let result = (|| {
        let (value, _) = view(memory, image, p)?;
        let mut source = vec![0; value.bytes];
        if !source.is_empty() {
            memory.read(value.data, &mut source)?;
        }
        let next = create(memory, image, value.vt, value.bounds.clone(), false)?;
        let target = image.safe_arrays.entries[&next].data;
        let result = (|| {
            if matches!(value.vt, 8 | 12) {
                for (n, item) in source.chunks_exact(value.size).enumerate() {
                    let bytes = if value.vt == 8 {
                        string(
                            StringOperation::Duplicate(Some(pointer(item))),
                            memory,
                            image,
                        )?
                        .to_le_bytes()[..value.size]
                            .to_vec()
                    } else {
                        clone_value(item, memory, image, active)?
                    };
                    memory.write(target + (n * value.size) as u64, &bytes)?;
                }
            } else if !source.is_empty() {
                memory.write(target, &source)?;
            }
            Ok(next)
        })();
        if result.is_err() {
            destroy(next, memory, image)?;
        }
        result
    })();
    active.remove(&p);
    result
}
fn index(memory: &Memory, value: &Array, indices: u64) -> Result<u64> {
    if indices == 0 {
        return invalid();
    }
    let mut bytes = vec![0; value.bounds.len() * 4];
    memory.read(indices, &mut bytes)?;
    if value.bytes == 0 {
        return Err(Error::Hresult(BAD_INDEX));
    }
    let mut offset = 0usize;
    let mut stride = 1usize;
    for (item, &(count, lower)) in bytes.as_chunks::<4>().0.iter().zip(&value.bounds) {
        let relative = i64::from(i32::from_le_bytes(*item)) - i64::from(lower);
        if relative < 0 || relative >= i64::from(count) {
            return Err(Error::Hresult(BAD_INDEX));
        }
        offset += relative as usize * stride;
        stride *= count as usize;
    }
    Ok(value.data + (offset * value.size) as u64)
}
fn raw(memory: &Memory, p: u64, size: usize) -> Result<Vec<u8>> {
    let mut bytes = vec![0; size];
    memory.read(p, &mut bytes)?;
    Ok(bytes)
}

fn operation(id: A, a: &[u64; 16], memory: &mut Memory, image: &mut Loaded) -> Result<u64> {
    match id {
        A::Create | A::CreateEx | A::CreateVector | A::CreateVectorEx => {
            element(a[0] as u16, memory.mode()).ok_or(Error::Hresult(BAD_TYPE))?;
            let vector = matches!(id, A::CreateVector | A::CreateVectorEx);
            let bounds = if vector {
                vec![(a[2] as u32, a[1] as i32)]
            } else {
                let dimensions = a[1] as u32 as usize;
                if dimensions == 0 || dimensions > 32 || a[2] == 0 {
                    return invalid();
                }
                raw(memory, a[2], dimensions * 8)?
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .map(|b| (pointer(&b[..4]) as u32, pointer(&b[4..]) as u32 as i32))
                    .collect()
            };
            create(memory, image, a[0] as u16, bounds, vector)
        }
        A::Destroy => {
            destroy(a[0], memory, image)?;
            Ok(0)
        }
        A::Copy => {
            if a[1] == 0 {
                return invalid();
            }
            memory.check(a[1], width(memory.mode()), WRITE)?;
            match copy(a[0], memory, image, &mut BTreeSet::new()) {
                Ok(p) => {
                    memory.write_int(a[1], width(memory.mode()), p)?;
                    Ok(0)
                }
                Err(error @ Error::Hresult(_)) => {
                    memory.write_int(a[1], width(memory.mode()), 0)?;
                    Err(error)
                }
                Err(error) => Err(error),
            }
        }
        _ => {
            if matches!(id, A::Get | A::Pointer) && (a[1] == 0 || a[2] == 0)
                || id == A::Put && a[1] == 0
                || matches!(id, A::LowerBound | A::UpperBound) && a[2] == 0
                || matches!(id, A::Access | A::Vartype) && a[1] == 0
            {
                return invalid();
            }
            if a[0] == 0 && matches!(id, A::Dimensions | A::ElementSize) {
                return Ok(0);
            }
            let (value, locks) = view(memory, image, a[0])?;
            match id {
                A::Dimensions => Ok(value.bounds.len() as u64),
                A::ElementSize => Ok(value.size as u64),
                A::LowerBound | A::UpperBound => {
                    if a[2] == 0 {
                        return invalid();
                    }
                    let dimension = a[1] as u32 as usize;
                    if dimension == 0 || dimension > value.bounds.len() {
                        return Err(Error::Hresult(BAD_INDEX));
                    }
                    let (count, lower) = value.bounds[dimension - 1];
                    memory.write_int(
                        a[2],
                        4,
                        u64::from(if id == A::LowerBound {
                            lower as u32
                        } else {
                            (lower as u32).wrapping_add(count).wrapping_sub(1)
                        }),
                    )?;
                    Ok(0)
                }
                A::Vartype => {
                    if a[1] == 0 {
                        return invalid();
                    }
                    memory.write_int(a[1], 2, u64::from(value.vt))?;
                    Ok(0)
                }
                A::Lock | A::Access | A::Unlock | A::Unaccess => {
                    let lock = matches!(id, A::Lock | A::Access);
                    if id == A::Access && a[1] == 0 {
                        return invalid();
                    }
                    if (lock && locks >= 0xffff) || (!lock && (locks == 0 || locks > 0xffff)) {
                        return Err(Error::Hresult(UNEXPECTED));
                    }
                    memory.check(a[0] + 8, 4, WRITE)?;
                    if id == A::Access {
                        memory.check(a[1], width(memory.mode()), WRITE)?;
                    }
                    memory.write_int(
                        a[0] + 8,
                        4,
                        u64::from(if lock { locks + 1 } else { locks - 1 }),
                    )?;
                    if id == A::Access {
                        memory.write_int(a[1], width(memory.mode()), value.data)?;
                    }
                    Ok(0)
                }
                A::Pointer => {
                    if a[2] == 0 {
                        return invalid();
                    }
                    let p = index(memory, &value, a[1])?;
                    memory.write_int(a[2], width(memory.mode()), p)?;
                    Ok(0)
                }
                A::Get | A::Put => {
                    if locks >= 0xffff {
                        return Err(Error::Hresult(UNEXPECTED));
                    }
                    memory.check(a[0] + 8, 4, WRITE)?;
                    let p = index(memory, &value, a[1])?;
                    if id == A::Get {
                        if a[2] == 0 {
                            return invalid();
                        }
                        memory.check(a[2], value.size, WRITE)?;
                        if matches!(value.vt, 8 | 12) {
                            for &block in &value.blocks {
                                let span = image
                                    .data_pages
                                    .span(block)
                                    .ok_or(Error::Hresult(INVALID))?;
                                if a[2] < block + span
                                    && a[2].saturating_add(value.size as u64) > block
                                {
                                    return invalid();
                                }
                            }
                        }
                        let source = raw(memory, p, value.size)?;
                        let bytes = if value.vt == 8 {
                            string(
                                StringOperation::Duplicate(Some(pointer(&source))),
                                memory,
                                image,
                            )?
                            .to_le_bytes()[..value.size]
                                .to_vec()
                        } else if value.vt == 12 {
                            clone_value(&source, memory, image, &mut BTreeSet::new())?
                        } else {
                            source
                        };
                        memory.write(a[2], &bytes)?;
                    } else {
                        memory.check(p, value.size, WRITE)?;
                        let mut plan = Cleanup::default();
                        if value.vt == 8 {
                            let old = memory.read_int(p, width(memory.mode()))?;
                            plan.bstr(old, memory, image, (p, value.size))?;
                        } else if value.vt == 12 {
                            let old = raw(memory, p, value.size)?;
                            plan.value(
                                pointer(&old[..2]) as u16,
                                &old,
                                memory,
                                image,
                                (p, value.size),
                                0,
                            )?;
                        }
                        let bytes = if value.vt == 8 {
                            string(
                                StringOperation::Duplicate(if a[2] == 0 {
                                    None
                                } else {
                                    Some(a[2])
                                }),
                                memory,
                                image,
                            )?
                            .to_le_bytes()[..value.size]
                                .to_vec()
                        } else {
                            if a[2] == 0 {
                                return invalid();
                            }
                            let source = raw(memory, a[2], value.size)?;
                            if value.vt == 12 {
                                clone_value(&source, memory, image, &mut BTreeSet::new())?
                            } else {
                                source
                            }
                        };
                        plan.apply(memory, image)?;
                        memory.write(p, &bytes)?;
                    }
                    Ok(0)
                }
                _ => unreachable!(),
            }
        }
    }
}

pub(super) fn dispatch(
    id: A,
    call: &Call,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
) -> std::result::Result<u64, ServiceError> {
    let image = image.ok_or(ServiceError::Unavailable(
        "SAFEARRAY requires a loaded image",
    ))?;
    match operation(id, &call.arguments, memory, image) {
        Ok(value) => Ok(value),
        Err(Error::Service(error)) => Err(error),
        Err(Error::Hresult(code)) => Ok(
            if matches!(
                id,
                A::Create
                    | A::CreateEx
                    | A::CreateVector
                    | A::CreateVectorEx
                    | A::Dimensions
                    | A::ElementSize
            ) {
                0
            } else {
                u64::from(code)
            },
        ),
    }
}

pub(super) fn variant(
    id: VariantApi,
    a: &[u64; 16],
    memory: &mut Memory,
    image: &mut Loaded,
) -> Result<Option<u64>> {
    let source = match id {
        VariantApi::Clear => a[0],
        VariantApi::Copy => a[1],
        _ => return Ok(None),
    };
    if !memory.probe(source, 2, READ) {
        return Ok(None);
    }
    let vt = memory.read_int(source, 2)? as u16;
    if vt & 0x2000 == 0 || (id == VariantApi::Clear && vt & 0x4000 != 0) {
        return Ok(None);
    }
    let size = variant_size(memory.mode());
    memory.check(a[0], size, WRITE)?;
    let bytes = raw(memory, source, size)?;
    let old = raw(memory, a[0], size)?;
    let mut plan = Cleanup::default();
    plan.value(
        pointer(&old[..2]) as u16,
        &old,
        memory,
        image,
        (a[0], size),
        0,
    )?;
    if id == VariantApi::Clear {
        plan.apply(memory, image)?;
        memory.write_int(a[0], 2, 0)?;
    } else {
        let bytes = clone_value(&bytes, memory, image, &mut BTreeSet::new())?;
        plan.apply(memory, image)?;
        memory.write(a[0], &bytes)?;
    }
    Ok(Some(0))
}
