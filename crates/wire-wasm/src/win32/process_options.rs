use super::{call::Call, catalog::ProcessOptionApi as P, context, modules::Failure, ServiceError};
use crate::{
    machine::State,
    memory::{Memory, WRITE},
    process::Loaded,
};

const MAGIC: u32 = 0x5741_5454;
const HEADER: usize = 16;
const LIMIT: u32 = 32;

fn size(width: usize, count: u32) -> usize {
    HEADER + count as usize * 3 * width
}

fn list(memory: &Memory, address: u64) -> Result<(Vec<u8>, u32, u32), Failure> {
    let mut header = [0; HEADER];
    memory.read(address, &mut header)?;
    let word = |at| u32::from_le_bytes(header[at..at + 4].try_into().unwrap());
    let (capacity, count) = (word(4), word(8));
    if word(0) != MAGIC || capacity > LIMIT || count > capacity {
        return Err(Failure::Windows(87));
    }
    let mut bytes = vec![0; size(memory.mode() as usize / 8, capacity)];
    memory.read(address, &mut bytes)?;
    Ok((bytes, capacity, count))
}

fn attribute_size(key: u64, length: u64, width: usize) -> Result<(), u32> {
    let valid = match key {
        0x20000 => length == width as u64,
        0x20002 | 0x2000d => length.is_multiple_of(width as u64),
        0x30003 => length == if width == 4 { 12 } else { 16 },
        0x30005 => length == 4,
        0x20007 => matches!(length, 4 | 8 | 16),
        0x2000e => matches!(length, 4 | 8),
        0x20019 => length == 2,
        _ => return Err(50),
    };
    if valid {
        Ok(())
    } else {
        Err(24)
    }
}

pub(super) fn inherited_handles(
    memory: &Memory,
    address: u64,
) -> Result<Option<Vec<u64>>, Failure> {
    if address == 0 {
        return Ok(None);
    }
    let (bytes, _, count) = list(memory, address)?;
    let width = memory.mode() as usize / 8;
    let word = |bytes: &[u8]| {
        let mut value = [0; 8];
        value[..width].copy_from_slice(bytes);
        u64::from_le_bytes(value)
    };
    let mut result = None;
    for entry in bytes[HEADER..HEADER + count as usize * width * 3].chunks_exact(width * 3) {
        let key = word(&entry[..width]);
        let length = word(&entry[width..width * 2]);
        let pointer = word(&entry[width * 2..]);
        if key != 0x20002 {
            return Err(Failure::Windows(50));
        }
        if result.is_some()
            || length == 0
            || length > 4096 * width as u64
            || !length.is_multiple_of(width as u64)
        {
            return Err(Failure::Windows(87));
        }
        let mut handles = Vec::new();
        for n in 0..length / width as u64 {
            let at = pointer
                .checked_add(n * width as u64)
                .ok_or(Failure::Windows(87))?;
            let handle = memory.read_int(at, width)?;
            if handle == 0
                || handle == memory.mode().address(u64::MAX)
                || handle == memory.mode().address(u64::MAX - 1)
                || handles.contains(&handle)
            {
                return Err(Failure::Windows(87));
            }
            handles.push(handle);
        }
        result = Some(handles);
    }
    Ok(result)
}

fn options(
    id: P,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: &mut Loaded,
) -> Result<u64, Failure> {
    let a = &call.arguments;
    let width = memory.mode() as usize / 8;
    Ok(match id {
        P::GetErrorMode => u64::from(image.process.error_mode),
        P::SetErrorMode => {
            let value = a[0] as u32;
            if value & !0x8007 != 0 {
                return Err(Failure::Windows(87));
            }
            let previous = image.process.error_mode;
            image.process.error_mode = value | (previous & 4);
            u64::from(previous)
        }
        P::InitializeAttributes => {
            let count = a[1] as u32;
            if a[2] as u32 != 0 {
                return Err(Failure::Windows(87));
            }
            if count > LIMIT {
                return Err(Failure::Windows(50));
            }
            let needed = size(width, count);
            let capacity = if a[0] == 0 {
                0
            } else {
                memory.read_int(a[3], width)?
            };
            memory.check(a[3], width, WRITE)?;
            if a[0] == 0 || capacity < needed as u64 {
                memory.check(context::error_address(state, memory.mode()), 4, WRITE)?;
                memory.write_int(a[3], width, needed as u64)?;
                return Err(Failure::Windows(122));
            }
            memory.check(a[0], needed, WRITE)?;
            let mut bytes = vec![0; needed];
            bytes[..4].copy_from_slice(&MAGIC.to_le_bytes());
            bytes[4..8].copy_from_slice(&count.to_le_bytes());
            memory.write(a[0], &bytes)?;
            memory.write_int(a[3], width, needed as u64)?;
            1
        }
        P::UpdateAttribute => {
            if a[1] as u32 != 0 || a[5] != 0 || a[6] != 0 {
                return Err(Failure::Windows(87));
            }
            let (mut bytes, capacity, count) = list(memory, a[0])?;
            if count == capacity {
                return Err(Failure::Windows(31));
            }
            attribute_size(a[2], a[4], width).map_err(Failure::Windows)?;
            let stride = 3 * width;
            for entry in bytes[HEADER..HEADER + count as usize * stride].chunks_exact(stride) {
                if entry[..width] == a[2].to_le_bytes()[..width] {
                    return Err(Failure::Windows(698));
                }
            }
            let offset = HEADER + count as usize * stride;
            for (n, value) in [a[2], a[4], a[3]].into_iter().enumerate() {
                bytes[offset + n * width..offset + (n + 1) * width]
                    .copy_from_slice(&value.to_le_bytes()[..width]);
            }
            bytes[8..12].copy_from_slice(&(count + 1).to_le_bytes());
            memory.write(a[0], &bytes)?;
            1
        }
        P::DeleteAttributes => {
            if a[0] != 0 {
                list(memory, a[0])?;
                memory.write_int(a[0], 4, 0)?;
            }
            0
        }
    })
}

pub(super) fn dispatch(
    id: P,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
) -> Result<u64, ServiceError> {
    let image = image.ok_or(ServiceError::Unavailable(
        "process options require a loaded image",
    ))?;
    match options(id, call, state, memory, image) {
        Ok(value) => Ok(value),
        Err(Failure::Fault(fault)) => Err(fault.into()),
        Err(Failure::Windows(error)) => {
            memory.write_int(
                context::error_address(state, memory.mode()),
                4,
                u64::from(error),
            )?;
            Ok(0)
        }
    }
}
