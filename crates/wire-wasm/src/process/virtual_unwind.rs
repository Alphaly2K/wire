use super::{unwind, Loaded, StartupError};
use crate::{
    machine::{Mode, State},
    memory::{Fault, FaultKind, Memory, WRITE},
    win32::{call::Call, ServiceError},
};

fn output(base: u64, offset: usize) -> Result<u64, Fault> {
    base.checked_add(offset as u64).ok_or(Fault {
        address: base,
        length: 8,
        access: WRITE,
        kind: FaultKind::Address,
    })
}

pub(crate) fn virtual_unwind_call(call: &Call, memory: &mut Memory) -> Result<u64, ServiceError> {
    let [kind, base, pc, entry, context, data, frame, pointers, ..] = call.arguments;
    let mut bytes = [0; 1232];
    memory.read(context, &mut bytes)?;
    let get = |at| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
    let mut original = State::new(Mode::X64, get(248), get(152));
    for (n, register) in original.regs.iter_mut().enumerate() {
        *register = get(120 + n * 8);
    }
    for (n, register) in original.xmm.iter_mut().enumerate() {
        register.copy_from_slice(&bytes[416 + n * 16..432 + n * 16]);
    }
    // This CONTEXT is unwind data, not an architectural resume request.
    // ContextFlags, selectors, MXCSR and unrelated payload remain untouched.
    let result = unwind::step_with_stack(
        memory,
        &original,
        (entry != 0).then_some(unwind::Function { base, entry }),
        pc,
        unwind::Stack::Supplied,
    )
    .map_err(|error| match error {
        StartupError::Fault(fault) => ServiceError::Fault(fault),
        _ => ServiceError::Unavailable("invalid or unsupported AMD64 virtual unwind metadata"),
    })?;
    for (n, register) in result.caller.regs.into_iter().enumerate() {
        bytes[120 + n * 8..128 + n * 8].copy_from_slice(&register.to_le_bytes());
    }
    bytes[248..256].copy_from_slice(&result.caller.ip.to_le_bytes());
    for (n, register) in result.caller.xmm.iter().enumerate() {
        bytes[416 + n * 16..432 + n * 16].copy_from_slice(register);
    }
    let handler = result
        .handler
        .filter(|h| u64::from(h.flags) & kind & 3 != 0);
    let handler_data = handler
        .as_ref()
        .map(|h| h.data)
        .or_else(|| (entry == 0 && kind != 0).then_some(0));
    memory.check(context, bytes.len(), WRITE)?;
    memory.check(frame, 8, WRITE)?;
    if handler_data.is_some() {
        memory.check(data, 8, WRITE)?;
    }
    let mut saved = Vec::new();
    if pointers != 0 {
        for (at, value) in result
            .saved
            .float
            .into_iter()
            .chain(result.saved.integer)
            .enumerate()
            .filter_map(|(n, value)| value.map(|value| (n * 8, value)))
        {
            let address = output(pointers, at)?;
            memory.check(address, 8, WRITE)?;
            saved.push((address, value));
        }
    }
    memory.write(context, &bytes)?;
    memory.write_int(frame, 8, result.frame)?;
    if let Some(value) = handler_data {
        memory.write_int(data, 8, value)?;
    }
    for (address, value) in saved {
        memory.write_int(address, 8, value)?;
    }
    Ok(handler.map_or(0, |h| h.target))
}

impl Loaded {
    pub(crate) fn pc_to_file_header(&self, call: &Call, memory: &mut Memory) -> Result<u64, Fault> {
        let [pc, output, ..] = call.arguments;
        let base = self
            .modules
            .modules
            .values()
            .find(|module| pc >= module.base && pc - module.base < u64::from(module.image.size))
            .map_or(0, |module| module.base);
        memory.write_int(output, memory.mode() as usize / 8, base)?;
        Ok(base)
    }
}
