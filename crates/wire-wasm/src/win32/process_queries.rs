use super::{call::Call, catalog::ProcessQueryApi as P, context, modules::Failure, ServiceError};
use crate::{
    machine::State,
    memory::{Memory, WRITE},
    process::Loaded,
};

fn query(id: P, a: &[u64; 16], memory: &mut Memory, image: &mut Loaded) -> Result<u64, Failure> {
    Ok(match id {
        P::Priority | P::SetPriority => {
            if id == P::SetPriority {
                image.process.check(a[0], 0x200)
            } else {
                image.process.query(a[0])
            }
            .map_err(Failure::Windows)?;
            let record = image.process.target(a[0]).map_err(Failure::Windows)?;
            if id == P::SetPriority {
                if matches!(a[1] as u32, 0x100000 | 0x200000) {
                    return Err(Failure::Windows(50));
                }
                if !matches!(a[1] as u32, 0x20 | 0x40 | 0x80 | 0x100 | 0x4000 | 0x8000) {
                    return Err(Failure::Windows(87));
                }
                record.priority_class.set(a[1] as u32);
                1
            } else {
                record.priority_class.get().into()
            }
        }
        P::Enumerate => {
            let ids = image.process.process_ids();
            let count = ids.len().min(a[1] as u32 as usize / 4);
            memory.check(a[2], 4, WRITE)?;
            if count != 0 {
                memory.check(a[0], count * 4, WRITE)?;
            }
            for (n, id) in ids[..count].iter().enumerate() {
                memory.write_int(a[0] + n as u64 * 4, 4, (*id).into())?;
            }
            memory.write_int(a[2], 4, (count * 4) as u64)?;
            1
        }
        P::Times | P::WorkingSet | P::SetWorkingSet | P::InputIdle => {
            if id == P::SetWorkingSet {
                image.process.check(a[0], 0x100)
            } else if id == P::InputIdle {
                image
                    .process
                    .check(a[0], crate::process::current::SYNCHRONIZE)
            } else {
                image.process.query(a[0])
            }
            .map_err(Failure::Windows)?;
            return Err(Failure::Windows(50));
        }
        P::MemoryInfo => {
            let width = memory.mode() as usize / 8;
            if (a[2] as u32 as usize) < 8 + 8 * width {
                return Err(Failure::Windows(122));
            }
            image.process.query(a[0]).map_err(Failure::Windows)?;
            if image.process.target(a[0]).map_err(Failure::Windows)?.id != image.process.id() {
                return Err(Failure::Windows(50));
            }
            let bytes = memory.process_memory(a[2] as u32 as usize >= 8 + 9 * width);
            memory.write(a[1], &bytes)?;
            1
        }
        P::Open => image
            .process
            .open(a[2] as u32, a[0] as u32, a[1] as u32 != 0)
            .map_err(Failure::Windows)?,
        P::Id | P::ExitCode | P::Affinity | P::SetAffinity => {
            if id == P::SetAffinity {
                image.process.check(a[0], 0x200)
            } else {
                image.process.query(a[0])
            }
            .map_err(Failure::Windows)?;
            match id {
                P::Id => u64::from(image.process.target(a[0]).map_err(Failure::Windows)?.id),
                P::ExitCode => {
                    let record = image.process.target(a[0]).map_err(Failure::Windows)?;
                    memory.write_int(a[1], 4, record.exit.get().unwrap_or(259).into())?;
                    1
                }
                P::Affinity => {
                    let width = memory.mode() as usize / 8;
                    memory.check(a[1], width, WRITE)?;
                    memory.check(a[2], width, WRITE)?;
                    memory.write_int(a[1], width, 1)?;
                    memory.write_int(a[2], width, 1)?;
                    1
                }
                P::SetAffinity => {
                    if a[1] != 1 {
                        return Err(Failure::Windows(87));
                    }
                    1
                }
                _ => unreachable!(),
            }
        }
        P::SetIdealProcessor => {
            image.threads.target(a[0], 0x20).map_err(Failure::Windows)?;
            if a[1] as u32 != 0 && a[1] as u32 != memory.mode() as u32 {
                return Err(Failure::Windows(87));
            }
            0
        }
        P::ActiveProcessorCount | P::MaximumProcessorCount => {
            if !matches!(a[0] as u16, 0 | 0xffff) {
                return Err(Failure::Windows(87));
            }
            1
        }
        P::ActiveGroupCount | P::MaximumGroupCount => 1,
        P::PackageId => 15700,
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
        "process queries require a loaded image",
    ))?;
    match query(id, &call.arguments, memory, image) {
        Ok(value) => Ok(value),
        Err(Failure::Fault(fault)) => Err(fault.into()),
        Err(Failure::Windows(error)) => {
            memory.write_int(
                context::error_address(state, memory.mode()),
                4,
                u64::from(error),
            )?;
            Ok(if matches!(id, P::SetIdealProcessor | P::InputIdle) {
                u64::from(u32::MAX)
            } else {
                0
            })
        }
    }
}
