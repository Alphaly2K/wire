use super::{
    call::{peek_slot, Call},
    catalog::BcryptApi as B,
    modules::{name, Failure},
    ServiceError,
};
use crate::{
    machine::State,
    memory::{Memory, WRITE},
    process::Loaded,
};

pub(super) fn request(id: B, state: &State, memory: &Memory, image: Option<&Loaded>) -> u32 {
    let Some(image) = image else {
        return 0;
    };
    let args = if id == B::SystemRandom {
        [
            Some(0),
            peek_slot(state, memory, 0),
            peek_slot(state, memory, 1),
            Some(2),
        ]
    } else {
        std::array::from_fn(|n| peek_slot(state, memory, n as u64))
    };
    let [Some(handle), Some(output), Some(length), Some(flags)] = args else {
        return 0;
    };
    let length = length as u32;
    if image.rng.validate(handle, flags as u32, length).is_err()
        || length == 0
        || !memory.probe(output, length as usize, WRITE)
    {
        return 0;
    }
    length
}

fn provider_name(memory: &Memory, pointer: u64) -> Result<String, Failure> {
    name(memory, pointer, true).map_err(|error| match error {
        Failure::Windows(_) => Failure::Windows(0xc000000d),
        fault => fault,
    })
}

fn run(
    id: B,
    call: &Call,
    memory: &mut Memory,
    image: &mut Loaded,
    random: Option<&[u8]>,
) -> Result<u64, Failure> {
    let a = &call.arguments;
    Ok(match id {
        B::Generate | B::SystemRandom => {
            let system = id == B::SystemRandom;
            let (handle, output, length, flags) = if system {
                (0, a[0], a[1], 2)
            } else {
                (a[0], a[1], a[2], a[3] as u32)
            };
            if let Err(status) = image.rng.validate(handle, flags, length as u32) {
                return Ok(if system { 0 } else { status });
            }
            if length == 0 {
                return Ok(u64::from(system));
            }
            memory.check(output, length as usize, WRITE)?;
            let Some(bytes) = random.filter(|bytes| bytes.len() == length as usize) else {
                return Ok(if system { 0 } else { 0xc0000001 });
            };
            memory.write(output, bytes)?;
            u64::from(system)
        }
        B::Open => {
            let algorithm = provider_name(memory, a[1])?;
            let implementation = if a[2] == 0 {
                None
            } else {
                Some(provider_name(memory, a[2])?)
            };
            if a[3] != 0 {
                return Ok(0xc000000d);
            }
            if algorithm != "RNG"
                || implementation
                    .as_ref()
                    .is_some_and(|name| name != "Microsoft Primitive Provider")
            {
                return Ok(0xc00000bb);
            }
            let width = memory.mode() as usize / 8;
            memory.check(a[0], width, WRITE)?;
            let Some(handle) = image.rng.vacant() else {
                return Ok(0xc0000017);
            };
            memory.write_int(a[0], width, handle)?;
            image.rng.insert(handle);
            0
        }
        B::Close => {
            if a[1] != 0 {
                0xc000000d
            } else if image.rng.close(a[0]) {
                0
            } else {
                0xc0000008
            }
        }
    })
}

pub(super) fn dispatch(
    id: B,
    call: &Call,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
    random: Option<&[u8]>,
) -> Result<u64, ServiceError> {
    let image = image.ok_or(ServiceError::Unavailable(
        "RNG providers require a loaded image",
    ))?;
    match run(id, call, memory, image, random) {
        Ok(status) => Ok(status),
        Err(Failure::Fault(fault)) => Err(fault.into()),
        Err(Failure::Windows(status)) => Ok(status.into()),
    }
}
