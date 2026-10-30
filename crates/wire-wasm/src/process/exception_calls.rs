use super::{
    callbacks::{CallFrame, Callback},
    contexts, *,
};
use crate::win32::{call::Call, catalog::ExceptionApi as E, ServiceError};
#[path = "../../../../src/win32/kernel/unwind32.rs"]
mod chain;

#[derive(Clone)]
struct Unwind {
    frames: VecDeque<chain::Registration>,
    record: u64,
    context: u64,
    dispatcher: u64,
    active: bool,
}
#[derive(Clone)]
pub(crate) struct Frame {
    pub saved: Box<State>,
    pub loader_depth: usize,
    block: u64,
    return_sp: Option<u64>,
    unwind: Option<Unwind>,
    result: Option<u32>,
}
impl Frame {
    pub(super) fn unwind_bridge(&self) -> Option<unwind::Bridge> {
        self.return_sp.map(|sp| unwind::Bridge {
            sp,
            caller: Some((self.saved.clone(), true)),
        })
    }
    pub(super) fn is_unwind(&self) -> bool {
        self.unwind.is_some()
    }
    fn enter(&mut self, state: &mut State, memory: &mut Memory) -> Result<(), StartupError> {
        let unwind = self.unwind.as_mut().unwrap();
        let Some(registration) = unwind.frames.front() else {
            return Ok(());
        };
        let args = [
            unwind.record,
            registration.address.into(),
            unwind.context,
            unwind.dispatcher,
        ];
        let call = CallFrame::new(Mode::X86, self.block, &args)?;
        memory.check(call.sp, call.bytes.len(), WRITE)?;
        memory.check(unwind.dispatcher, 4, WRITE)?;
        memory.write_int(unwind.dispatcher, 4, 0)?;
        call.enter(registration.handler.into(), state, memory)?;
        unwind.active = true;
        self.result = None;
        Ok(())
    }
}
fn service(error: StartupError) -> ServiceError {
    match error {
        StartupError::Fault(fault) => ServiceError::Fault(fault),
        _ => ServiceError::Unavailable("invalid or unsupported PE32 exception continuation"),
    }
}
impl Loaded {
    pub(crate) fn exception_call(
        &mut self,
        id: E,
        call: Call,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), ServiceError> {
        if id == E::UnhandledFilter && self.exception_registrations.filter() == 0 {
            call.complete(state, 0);
            return Ok(());
        }
        if self.threads.current().callbacks.len() >= 32 {
            return Err(ServiceError::Unavailable(
                "exception/callback nesting capacity exceeded",
            ));
        }
        let mut saved = state.clone();
        call.clone().complete(
            &mut saved,
            if id == E::Unwind {
                call.arguments[3]
            } else {
                0
            },
        );
        let mut frame = Frame {
            saved: Box::new(saved),
            loader_depth: self.callback_depth(),
            block: state.regs[4],
            return_sp: None,
            unwind: None,
            result: None,
        };
        if id == E::UnhandledFilter {
            let target = self.exception_registrations.filter();
            let mode = memory.mode();
            let callback = CallFrame::new(mode, frame.block, &call.arguments[..1])?;
            frame.return_sp = (mode == Mode::X64).then_some(callback.sp + 8);
            callback.enter(target, state, memory)?;
        } else {
            if self
                .threads
                .current()
                .callbacks
                .iter()
                .any(|c| matches!(c, Callback::ExceptionCall(f) if f.unwind.is_some()))
            {
                return Err(ServiceError::Unavailable(
                    "nested/collided PE32 unwind is not implemented",
                ));
            }
            let a = call.arguments;
            let head = memory.read_int(state.fs_base, 4)? as u32;
            let top = memory.read_int(state.fs_base + 4, 4)? as u32;
            let limit = memory.read_int(state.fs_base + 8, 4)? as u32;
            let frames = chain::plan(head, a[0] as u32, [limit, top], |p| {
                memory.read_int(p.into(), 4).map(|v| v as u32)
            })
            .map_err(|e| match e {
                chain::Error::Read(f) => ServiceError::Fault(f),
                chain::Error::Invalid(message) => ServiceError::Unavailable(message),
            })?;
            let length = 256 + contexts::size(Mode::X86);
            frame.block = (state.regs[4] & !15)
                .checked_sub(length.div_ceil(16) as u64 * 16)
                .ok_or(ServiceError::Unavailable("PE32 unwind stack underflow"))?;
            let record = if a[2] == 0 { frame.block } else { a[2] };
            let mut bytes = [0; 80];
            if a[2] == 0 {
                bytes[..4].copy_from_slice(&0xc0000027u32.to_le_bytes());
                bytes[12..16].copy_from_slice(&(frame.saved.ip as u32).to_le_bytes());
            } else {
                memory.read(record, &mut bytes)?;
            }
            let flags = u32::from_le_bytes(bytes[4..8].try_into().unwrap())
                | 2
                | if a[0] == 0 { 4 } else { 0 };
            let context = frame.block + 256;
            memory.check(frame.block, length, WRITE)?;
            memory.check(record + 4, 4, WRITE)?;
            if !frames.is_empty() {
                let callback = CallFrame::new(Mode::X86, frame.block, &[0; 4])?;
                memory.check(callback.sp, callback.bytes.len(), WRITE)?;
            }
            if a[2] == 0 {
                memory.write(record, &bytes)?;
            }
            memory.write_int(record + 4, 4, flags.into())?;
            memory.write(context, &contexts::capture(&frame.saved, Mode::X86))?;
            frame.unwind = Some(Unwind {
                frames: frames.into(),
                record,
                context,
                dispatcher: frame.block + 80,
                active: false,
            });
            state.ip = gateways::layout(Mode::X86).callback;
            frame.enter(state, memory).map_err(service)?;
        }
        self.threads
            .current_mut()
            .callbacks
            .push(Callback::ExceptionCall(Box::new(frame)));
        memory.clear_fault();
        Ok(())
    }
    fn save_exception_call(&mut self, frame: &Frame) {
        *self.threads.current_mut().callbacks.last_mut().unwrap() =
            Callback::ExceptionCall(Box::new(frame.clone()));
    }
    pub(super) fn resume_exception_call(
        &mut self,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), StartupError> {
        let Some(Callback::ExceptionCall(frame)) = self.threads.current().callbacks.last() else {
            unreachable!()
        };
        let mut frame = *frame.clone();
        let mut continuation;
        if frame.unwind.is_none() {
            continuation = *frame.saved;
            continuation.regs[0] = u64::from(frame.result.unwrap_or(state.regs[0] as u32));
        } else {
            if frame.unwind.as_ref().unwrap().active {
                let disposition = *frame.result.get_or_insert(state.regs[0] as u32);
                self.save_exception_call(&frame);
                if disposition != 1 {
                    return Err(StartupError::Invalid(
                        "unsupported PE32 unwind disposition".into(),
                    ));
                }
                let u = frame.unwind.as_mut().unwrap();
                let registration = u.frames.front().unwrap();
                if memory.read_int(frame.saved.fs_base, 4)? != u64::from(registration.address)
                    || memory.read_int(registration.address.into(), 4)?
                        != u64::from(registration.next)
                {
                    return Err(StartupError::Invalid("SEH registration changed during unwind; collided unwind is not implemented".into()));
                }
                memory.write_int(frame.saved.fs_base, 4, registration.next.into())?;
                u.frames.pop_front();
                u.active = false;
                frame.result = None;
                self.save_exception_call(&frame);
            }
            if !frame.unwind.as_ref().unwrap().frames.is_empty() {
                frame.enter(state, memory)?;
                self.save_exception_call(&frame);
                return Ok(());
            }
            continuation =
                contexts::restore(memory, frame.unwind.as_ref().unwrap().context, &frame.saved)?;
            if self.loader_owner == Some(self.threads.active)
                && self
                    .frames
                    .last()
                    .is_some_and(|f| continuation.regs[4] >= f.saved.regs[4])
            {
                return Err(StartupError::Invalid(
                    "PE32 unwind cannot cross a loader notification".into(),
                ));
            }
        }
        continuation.work = state.work;
        continuation.retired = state.retired;
        *state = continuation;
        self.threads.current_mut().callbacks.pop();
        self.discard_callbacks(state.regs[4]);
        memory.clear_fault();
        Ok(())
    }
}
