//! Alertable waits lend the active guest thread to bounded user APC callbacks.
use super::{
    callbacks::{CallFrame, Callback},
    unwind, *,
};
use crate::win32::{apcs, call::Call, ServiceError};
#[derive(Clone)]
pub(crate) struct Frame {
    pub(super) saved: Box<State>,
    pub(super) loader_depth: usize,
    call: Call,
    remaining: VecDeque<apcs::Call>,
    return_sp: u64,
}
impl Frame {
    pub(super) fn unwind_bridge(&self) -> unwind::Bridge {
        let mut caller = self.saved.clone();
        self.call.clone().complete(&mut caller, 0xc0);
        unwind::Bridge {
            sp: self.return_sp,
            caller: Some((caller, true)),
        }
    }
}
fn target(memory: &Memory, services: &Services, address: u64) -> Result<(), Fault> {
    if services.is_bound(address) {
        Ok(())
    } else {
        memory.check(address, 1, READ | crate::memory::EXECUTE)
    }
}
impl Loaded {
    pub(crate) fn queue_apc(
        &mut self,
        a: &[u64; 16],
        memory: &Memory,
        services: &Services,
    ) -> Result<u64, LoadError> {
        let id = self.threads.target(a[1], 0x10).and_then(|id| {
            if self.threads.get(id).is_none_or(|t| t.exit.is_some()) {
                Err(31)
            } else {
                Ok(id)
            }
        });
        let queue = id.map(|id| &mut self.threads.get_mut(id).apcs);
        let queue = apcs::capacity(queue).map_err(LoadError::Windows)?;
        apcs::enqueue(queue, (a[0], a[2]), |address| {
            target(memory, services, address)
        })?;
        self.threads.changed();
        Ok(1)
    }
    pub(crate) fn deliver_apcs(
        &mut self,
        call: Call,
        state: &mut State,
        memory: &mut Memory,
        services: &Services,
    ) -> Result<(), ServiceError> {
        if self.threads.current().callbacks.len() >= 32 {
            return Err(ServiceError::Unavailable(
                "APC callback nesting capacity exceeded",
            ));
        }
        let first =
            self.threads
                .current()
                .apcs
                .front()
                .copied()
                .ok_or(ServiceError::Unavailable(
                    "APC delivery has no queued callback",
                ))?;
        let args = [first.1, 0, 0, 0];
        let entry = CallFrame::new(
            memory.mode(),
            state.regs[4],
            &args[..if memory.mode() == Mode::X86 { 1 } else { 4 }],
        )?;
        memory.check(entry.sp, entry.bytes.len(), WRITE)?;
        let calls = apcs::take(&mut self.threads.current_mut().apcs, |address| {
            target(memory, services, address)
        })?;
        let frame = Frame {
            saved: Box::new(state.clone()),
            loader_depth: self.callback_depth(),
            call,
            remaining: calls.into_iter().skip(1).collect(),
            return_sp: entry.sp + 8,
        };
        self.clear_pending();
        entry
            .enter(first.0, state, memory)
            .expect("preflighted APC callback frame");
        self.threads
            .current_mut()
            .callbacks
            .push(Callback::Apc(Box::new(frame)));
        Ok(())
    }
    pub(super) fn resume_apcs(
        &mut self,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), StartupError> {
        let Some(Callback::Apc(frame)) = self.threads.current().callbacks.last() else {
            return Err(unwind::invalid("APC return has no continuation"));
        };
        let mut restored = *frame.saved.clone();
        restored.work = state.work;
        restored.retired = state.retired;
        if let Some((target, args)) = frame.remaining.front().copied() {
            let entry = CallFrame::new(
                memory.mode(),
                restored.regs[4],
                &args[..if memory.mode() == Mode::X86 { 1 } else { 4 }],
            )?;
            entry.enter(target, &mut restored, memory)?;
            let Some(Callback::Apc(frame)) = self.threads.current_mut().callbacks.last_mut() else {
                unreachable!()
            };
            frame.remaining.pop_front();
            frame.return_sp = restored.regs[4] + 8;
        } else {
            let Some(Callback::Apc(frame)) = self.threads.current_mut().callbacks.pop() else {
                unreachable!()
            };
            frame.call.complete(&mut restored, 0xc0);
        }
        *state = restored;
        memory.clear_fault();
        Ok(())
    }
}
