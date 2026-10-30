use super::{
    call::Call,
    catalog::{ApiId, Contract, ProcessControlApi as P},
    context, ServiceError,
};
use crate::{
    machine::State,
    memory::Memory,
    process::{current, Loaded},
};

pub(super) fn dispatch(
    contract: &Contract,
    state: &mut State,
    memory: &mut Memory,
    image: &mut Loaded,
) -> Result<(), ServiceError> {
    let a = Call::arguments(memory.mode(), state, memory, contract)?;
    match contract.id {
        ApiId::ProcessControl(P::Exit) => {
            if !image.loader_available() {
                return Err(ServiceError::LoaderBlocked);
            }
            image.begin_process_exit(a[0] as u32, state, memory.mode())?;
        }
        ApiId::ProcessControl(P::Terminate) => {
            if let Err(code) = image.process.check(a[0], current::TERMINATE) {
                let call = Call::capture(memory.mode(), state, memory, contract)?;
                memory.write_int(context::error_address(state, memory.mode()), 4, code.into())?;
                call.complete(state, 0);
            } else {
                let target = image.process.target(a[0]).expect("checked process handle");
                if target.id == image.process.id() {
                    image.finish_process_exit(a[1] as u32, state, memory.mode());
                } else {
                    let call = Call::capture(memory.mode(), state, memory, contract)?;
                    if target.exit.get().is_some() {
                        memory.write_int(context::error_address(state, memory.mode()), 4, 5)?;
                        call.complete(state, 0);
                    } else {
                        target.terminate.set(Some(a[1] as u32));
                        call.complete(state, 1);
                    }
                }
            }
        }
        _ => unreachable!("unbound process control"),
    }
    state.work = state.work.wrapping_add(1);
    Ok(())
}
