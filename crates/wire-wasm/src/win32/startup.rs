//! Startup queries borrow the process's existing command storage.
use super::{call::Call, catalog::StartupApi as S, ServiceError};
use crate::{machine::Mode, memory::Memory, process::Loaded};

pub(super) fn dispatch(
    id: S,
    call: &Call,
    memory: &mut Memory,
    image: Option<&Loaded>,
) -> Result<u64, ServiceError> {
    Ok(match id {
        S::CommandA | S::CommandW => {
            let image = image.ok_or(ServiceError::Unavailable(
                "command line queries require a loaded image",
            ))?;
            image.info[if id == S::CommandA { 6 } else { 7 }]
        }
        S::InfoA | S::InfoW => {
            let length = if memory.mode() == Mode::X86 { 68 } else { 104 };
            let mut info = [0; 104];
            info[..4].copy_from_slice(&(length as u32).to_le_bytes());
            let info = image.map_or(&info, |image| {
                &image.startup_info[usize::from(id == S::InfoW)]
            });
            memory.write(call.arguments[0], &info[..length])?;
            0
        }
        S::Feature => {
            let [_, _, ecx, edx] = crate::cpu::cpuid(1, 0);
            u64::from(match call.arguments[0] as u32 {
                2 => edx & (1 << 8) != 0,
                3 => edx & (1 << 23) != 0,
                6 => edx & (1 << 25) != 0,
                8 => edx & (1 << 4) != 0,
                10 => edx & (1 << 26) != 0,
                11 => true, // The numerical engine implements MXCSR.DAZ.
                12 => memory.dep_enabled(),
                13 => ecx & 1 != 0,
                14 | 15 => ecx & (1 << 13) != 0,
                32 => crate::cpu::cpuid(0x80000001, 0)[3] & (1 << 27) != 0,
                36 => ecx & (1 << 9) != 0,
                37 => ecx & (1 << 19) != 0,
                38 => ecx & (1 << 20) != 0,
                _ => false,
            })
        }
    })
}
