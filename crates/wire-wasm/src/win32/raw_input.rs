use super::{
    call::Call,
    catalog::RawInputApi,
    context,
    messages::Queue,
    user::{self, raw_input::Packet, raw_input_calls},
    ServiceError,
};
use crate::{
    keyboard::Keyboard,
    machine::State,
    memory::{Memory, WRITE},
    process::Loaded,
};
use std::collections::BTreeMap;

struct Backend<'a> {
    memory: &'a mut Memory,
    windows: &'a BTreeMap<u64, user::Window>,
    messages: &'a mut Queue,
    keyboard: &'a mut Keyboard,
    outputs: Vec<(u64, Vec<u8>)>,
}
impl raw_input_calls::Backend for Backend<'_> {
    type Error = ServiceError;
    fn width(&self) -> usize {
        self.memory.mode() as usize / 8
    }
    fn check_write(&self, pointer: u64, size: usize) -> Result<(), ServiceError> {
        self.memory
            .check(pointer, size, WRITE)
            .map_err(ServiceError::Fault)
    }
    fn read(&self, pointer: u64, output: &mut [u8]) -> Result<(), ServiceError> {
        self.memory
            .read(pointer, output)
            .map_err(ServiceError::Fault)
    }
    fn write(&mut self, pointer: u64, bytes: &[u8]) -> Result<(), ServiceError> {
        self.memory.check(pointer, bytes.len(), WRITE)?;
        self.outputs.push((pointer, bytes.to_vec()));
        Ok(())
    }
    fn window_exists(&self, handle: u64) -> bool {
        self.windows.contains_key(&handle)
    }
    fn consume(&mut self, packets: &[(u64, Packet)]) {
        self.messages.remove_raw(packets);
        for (_, packet) in packets {
            if packet.no_legacy {
                let [message, key, lp] = packet.input;
                self.keyboard.consumed64(packet.tid, message, key, lp);
            }
        }
    }
}
pub(super) fn dispatch(
    id: RawInputApi,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
) -> Result<u64, ServiceError> {
    let image = image.ok_or(ServiceError::Unavailable(
        "raw input requires a loaded image",
    ))?;
    let error = context::error_address(state, memory.mode());
    let u = &mut image.user;
    let mut backend = Backend {
        memory,
        windows: &u.windows,
        messages: &mut u.messages,
        keyboard: &mut u.keyboard,
        outputs: Vec::new(),
    };
    let result = raw_input_calls::call(
        &mut u.raw_input,
        &mut backend,
        id,
        &call.arguments,
        u64::from(image.threads.active),
    )?;
    if result.error.is_some() {
        backend.memory.check(error, 4, WRITE)?;
    }
    for (pointer, bytes) in backend.outputs {
        backend
            .memory
            .write(pointer, &bytes)
            .expect("preflighted raw input output");
    }
    if let Some(code) = result.error {
        backend
            .memory
            .write_int(error, 4, code.into())
            .expect("preflighted raw input error");
    }
    Ok(result.value)
}
