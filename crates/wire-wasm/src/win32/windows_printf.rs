use super::{call::Call, catalog::WindowsPrintfApi, windows_printf_core, ServiceError};
use crate::{
    machine::{Mode, State},
    memory::{Fault, FaultKind, Memory, READ},
};

enum Arguments {
    Slots(u64),
    List(u64),
}
struct Adapter<'a> {
    memory: &'a mut Memory,
    state: &'a State,
    arguments: Arguments,
}
impl windows_printf_core::Memory for Adapter<'_> {
    type Error = ServiceError;
    fn add(&self, pointer: u64, offset: u64) -> Result<u64, ServiceError> {
        pointer
            .checked_add(offset)
            .filter(|&p| self.memory.mode().address(p) == p)
            .ok_or(ServiceError::Fault(Fault {
                address: pointer,
                length: offset as usize,
                access: READ,
                kind: FaultKind::Address,
            }))
    }
    fn read(&self, pointer: u64, output: &mut [u8]) -> Result<(), ServiceError> {
        self.memory
            .read(pointer, output)
            .map_err(ServiceError::Fault)
    }
    fn write(&mut self, pointer: u64, bytes: &[u8]) -> Result<(), ServiceError> {
        self.memory
            .write(pointer, bytes)
            .map_err(ServiceError::Fault)
    }
    fn argument(&mut self, wide: bool) -> Result<u64, ServiceError> {
        let mode = self.memory.mode();
        let width = if mode == Mode::X64 || wide { 8 } else { 4 };
        let (pointer, next) = match self.arguments {
            Arguments::Slots(slot) => {
                if mode == Mode::X64 && slot < 4 {
                    self.arguments = Arguments::Slots(slot + 1);
                    return Ok(self.state.regs[[1, 2, 8, 9][slot as usize]]);
                }
                let offset = if mode == Mode::X86 {
                    4 + slot * 4
                } else {
                    40 + (slot - 4) * 8
                };
                (
                    self.add(mode.address(self.state.regs[4]), offset)?,
                    Arguments::Slots(slot + if mode == Mode::X86 && wide { 2 } else { 1 }),
                )
            }
            Arguments::List(pointer) => {
                (pointer, Arguments::List(self.add(pointer, width as u64)?))
            }
        };
        let value = self.memory.read_int(pointer, width)?;
        self.arguments = next;
        Ok(value)
    }
    fn pointer_width(&self) -> usize {
        self.memory.mode() as usize / 8
    }
    fn invalid(&self, _message: String) -> ServiceError {
        ServiceError::Unavailable("Windows format conversion failed")
    }
}
pub(super) fn dispatch(
    id: WindowsPrintfApi,
    call: &Call,
    state: &State,
    memory: &mut Memory,
) -> Result<u64, ServiceError> {
    let arguments = if id.list() {
        Arguments::List(call.arguments[2])
    } else {
        Arguments::Slots(2)
    };
    windows_printf_core::format(
        &mut Adapter {
            memory,
            state,
            arguments,
        },
        call.arguments[0],
        call.arguments[1],
        id.wide(),
    )
    .map(u64::from)
}
