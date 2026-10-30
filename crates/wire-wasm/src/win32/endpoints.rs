use super::{byte_streams::CAPACITY, catalog::FileApi as F, files::security, modules::Failure};
use crate::{
    memory::{Memory, READ, WRITE},
    process::Loaded,
};

pub(super) fn contains(image: &Loaded, handle: u64) -> bool {
    image.standard.index(handle).is_some() || image.pipes.flags(handle).is_some()
}
pub(super) fn create(
    a: &[u64; 16],
    memory: &mut Memory,
    image: &mut Loaded,
) -> Result<u64, Failure> {
    let width = memory.mode() as usize / 8;
    memory.check(a[0], width, WRITE)?;
    memory.check(a[1], width, WRITE)?;
    let inherit = security(memory, a[2])?;
    let (reader, writer) = image
        .pipes
        .create(a[3] as u32, inherit)
        .map_err(Failure::Windows)?;
    memory.write_int(a[0], width, reader)?;
    memory.write_int(a[1], width, writer)?;
    Ok(1)
}

pub(super) fn peek(a: &[u64; 16], memory: &mut Memory, image: &Loaded) -> Result<u64, Failure> {
    let count = if a[1] == 0 { 0 } else { a[2] as u32 as usize };
    let (bytes, available) = match image.standard.index(a[0]) {
        Some(0) => image.input.peek(count),
        Some(_) => Err(5),
        None => image.pipes.peek(a[0], count),
    }
    .map_err(Failure::Windows)?;
    if !bytes.is_empty() {
        memory.check(a[1], bytes.len(), WRITE)?;
    }
    for &output in &a[3..6] {
        if output != 0 {
            memory.check(output, 4, WRITE)?;
        }
    }
    if !bytes.is_empty() {
        memory.write(a[1], &bytes)?;
    }
    for (&output, value) in a[3..6].iter().zip([bytes.len(), available, 0]) {
        if output != 0 {
            memory.write_int(output, 4, value as u64)?;
        }
    }
    Ok(1)
}

pub(super) fn cancel(id: F, image: &Loaded, handle: u64) -> Result<u64, Failure> {
    if !contains(image, handle) && image.open_files.flags(handle).is_none() {
        return Err(Failure::Windows(6));
    }
    // OVERLAPPED submissions are rejected, so there is no asynchronous request to cancel.
    if id == F::CancelIoEx {
        Err(Failure::Windows(1168))
    } else {
        Ok(1)
    }
}

pub(super) fn transfer(
    id: F,
    a: &[u64; 16],
    memory: &mut Memory,
    image: &mut Loaded,
) -> Result<Option<u64>, Failure> {
    if a[4] != 0 {
        return Err(Failure::Windows(50));
    }
    let index = image.standard.index(a[0]);
    if a[3] == 0 && !(id == F::WriteFile && index.is_some_and(|n| n != 0)) {
        return Err(Failure::Windows(87));
    }
    if a[3] != 0 {
        memory.check(a[3], 4, WRITE)?;
    }
    let count = a[2] as usize;
    if count > 16 * 1024 * 1024 {
        return Err(Failure::Windows(8));
    }
    if count != 0 {
        memory.check(a[1], count, if id == F::ReadFile { WRITE } else { READ })?;
    }
    if id == F::ReadFile {
        let data = if let Some(index) = index {
            if index != 0 {
                return Err(Failure::Windows(5));
            }
            image.input.read(count)
        } else {
            image.pipes.read(a[0], count)
        }
        .map_err(Failure::Windows)?;
        let Some(data) = data else {
            return Ok(None);
        };
        if !data.is_empty() {
            memory.write(a[1], &data)?;
        }
        memory.write_int(a[3], 4, data.len() as u64)?;
    } else if let Some(index) = index {
        if index == 0 {
            return Err(Failure::Windows(5));
        }
        if count > CAPACITY {
            return Err(Failure::Windows(8));
        }
        if count > image.streams.available(index) {
            return Ok(None);
        }
        if !image.streams.append_with(index, count, |offset, bytes| {
            memory.read(a[1] + offset as u64, bytes)
        })? {
            return Err(Failure::Windows(8));
        }
        if a[3] != 0 {
            memory.write_int(a[3], 4, count as u64)?;
        }
    } else {
        if count > image.pipes.capacity(a[0]).map_err(Failure::Windows)? {
            return Err(Failure::Windows(8));
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(count)
            .map_err(|_| Failure::Windows(8))?;
        bytes.resize(count, 0);
        if count != 0 {
            memory.read(a[1], &mut bytes)?;
        }
        if !image.pipes.write(a[0], &bytes).map_err(Failure::Windows)? {
            return Ok(None);
        }
        memory.write_int(a[3], 4, count as u64)?;
    }
    Ok(Some(1))
}
