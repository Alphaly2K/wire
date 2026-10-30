use super::{catalog::ContextApi, ServiceError};
use crate::{
    machine::{Mode, State},
    memory::Memory,
};

pub(crate) fn error_address(state: &State, mode: Mode) -> u64 {
    match mode {
        Mode::X86 => mode.address(state.fs_base.wrapping_add(0x34)),
        Mode::X64 => state.gs_base.wrapping_add(0x68),
    }
}
pub(crate) fn locale_offset(mode: Mode) -> u64 {
    if mode == Mode::X86 {
        0xc4
    } else {
        0x108
    }
}
pub(crate) fn locale_address(state: &State, mode: Mode) -> u64 {
    let teb = if mode == Mode::X86 {
        state.fs_base
    } else {
        state.gs_base
    };
    mode.address(teb.wrapping_add(locale_offset(mode)))
}

pub(super) fn dispatch(
    id: ContextApi,
    argument: u64,
    state: &State,
    memory: &mut Memory,
) -> Result<u64, ServiceError> {
    let mode = memory.mode();
    let teb = if mode == Mode::X86 {
        state.fs_base
    } else {
        state.gs_base
    };
    let pointer = mode as usize / 8;
    Ok(match id {
        ContextApi::ProcessId => memory.read_int(
            mode.address(teb.wrapping_add(if mode == Mode::X86 { 0x20 } else { 0x40 })),
            4,
        )?,
        ContextApi::ThreadId => memory.read_int(
            mode.address(teb.wrapping_add(if mode == Mode::X86 { 0x24 } else { 0x48 })),
            4,
        )?,
        ContextApi::ProcessHandle => mode.address(u64::MAX),
        ContextApi::ThreadHandle => mode.address(u64::MAX - 1),
        ContextApi::DebuggerPresent => {
            let peb = memory.read_int(
                mode.address(teb.wrapping_add(if mode == Mode::X86 { 0x30 } else { 0x60 })),
                pointer,
            )?;
            u64::from(memory.read_int(mode.address(peb.wrapping_add(2)), 1)? != 0)
        }
        ContextApi::GetLastError => memory.read_int(error_address(state, mode), 4)?,
        ContextApi::SetLastError => {
            memory.write_int(error_address(state, mode), 4, argument)?;
            0
        }
    })
}
