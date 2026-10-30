//! FLS destructors use ordinary guest calls with a saved continuation.
use super::{
    callbacks::{CallFrame, Callback},
    unwind, *,
};
use crate::win32::{
    call::Call,
    locals::{address, clear_slots},
    ServiceError,
};

#[derive(Clone)]
pub(crate) struct Frame {
    pub(super) saved: Box<State>,
    pub(super) loader_depth: usize,
    call: Option<Call>,
    remaining: VecDeque<(u64, u64)>,
    return_sp: u64,
}
impl Frame {
    pub(super) fn unwind_bridge(&self) -> unwind::Bridge {
        unwind::Bridge {
            sp: self.return_sp,
            caller: self.call.as_ref().map(|call| {
                let mut state = self.saved.clone();
                call.clone().complete(&mut state, 1);
                (state, true)
            }),
        }
    }
}

impl Loaded {
    fn fls_frame(
        &self,
        state: &State,
        memory: &Memory,
        call: Option<Call>,
        mut calls: VecDeque<(u64, u64)>,
    ) -> Result<Option<(Frame, CallFrame, u64)>, StartupError> {
        let Some((target, value)) = calls.pop_front() else {
            return Ok(None);
        };
        if self.threads.current().callbacks.len() >= 32 {
            return Err(unwind::invalid("FLS callback nesting capacity exceeded"));
        }
        let entry = CallFrame::new(memory.mode(), state.regs[4], &[value])?;
        memory.check(entry.sp, entry.bytes.len(), WRITE)?;
        let frame = Frame {
            saved: Box::new(state.clone()),
            loader_depth: self.callback_depth(),
            // PE32 pops its one argument; Win64 pops only the return address.
            return_sp: entry.sp + 8,
            remaining: calls,
            call,
        };
        Ok(Some((frame, entry, target)))
    }
    pub(crate) fn free_fls(
        &mut self,
        call: Call,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), ServiceError> {
        let index = call.arguments[0] as u32 as usize;
        let Some((_, callback)) = self.locals.fls(index) else {
            memory.write_int(
                crate::win32::context::error_address(state, memory.mode()),
                4,
                87,
            )?;
            call.complete(state, 0);
            return Ok(());
        };
        let ptr = memory.mode() as usize / 8;
        let mut calls = VecDeque::new();
        let mut addresses = Vec::new();
        for teb in self.threads.tebs() {
            let at = address(memory.mode(), teb, true, index);
            let value = memory.read_int(at, ptr)?;
            memory.check(at, ptr, WRITE)?;
            addresses.push(at);
            if callback != 0 && value != 0 {
                calls.push_back((callback, value));
            }
        }
        let pending = self
            .fls_frame(state, memory, Some(call.clone()), calls)
            .map_err(|e| match e {
                StartupError::Fault(f) => ServiceError::Fault(f),
                _ => ServiceError::Unavailable("FLS callback nesting capacity exceeded"),
            })?;
        clear_slots(memory, &addresses)?;
        if let Some((frame, entry, target)) = pending {
            entry
                .enter(target, state, memory)
                .expect("preflighted FLS callback frame");
            self.threads
                .current_mut()
                .callbacks
                .push(Callback::Fls(Box::new(frame)));
        } else {
            call.complete(state, 1);
        }
        // A reentrant allocation can reuse this index; queued calls own the old
        // callback/value pairs and never retire a replacement allocation.
        self.locals.commit_fls(index, None);
        Ok(())
    }
    pub(super) fn begin_fls_exit(
        &mut self,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), StartupError> {
        let teb = self.threads.teb();
        let ptr = memory.mode() as usize / 8;
        let mut calls = VecDeque::new();
        let mut addresses = Vec::new();
        for (index, callback) in self.locals.active_fls().filter(|(_, c)| *c != 0) {
            let at = address(memory.mode(), teb, true, index);
            let value = memory.read_int(at, ptr)?;
            memory.check(at, ptr, WRITE)?;
            addresses.push(at);
            if value != 0 {
                calls.push_back((callback, value));
            }
        }
        let pending = self.fls_frame(state, memory, None, calls)?;
        clear_slots(memory, &addresses)?;
        if let Some((frame, entry, target)) = pending {
            entry
                .enter(target, state, memory)
                .expect("preflighted thread FLS callback frame");
            self.threads
                .current_mut()
                .callbacks
                .push(Callback::Fls(Box::new(frame)));
        }
        self.threads.current_mut().fls_cleaned = true;
        Ok(())
    }
    pub(super) fn resume_fls(
        &mut self,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), StartupError> {
        let Some(Callback::Fls(frame)) = self.threads.current().callbacks.last() else {
            return Err(unwind::invalid("FLS return has no continuation"));
        };
        let mut frame = frame.clone();
        let (work, retired) = (state.work, state.retired);
        let mut restored = *frame.saved.clone();
        restored.work = work;
        restored.retired = retired;
        if let Some((target, value)) = frame.remaining.front().copied() {
            let entry = CallFrame::new(memory.mode(), restored.regs[4], &[value])?;
            memory.check(entry.sp, entry.bytes.len(), WRITE)?;
            entry.enter(target, &mut restored, memory)?;
            frame.return_sp = restored.regs[4] + 8;
            frame.remaining.pop_front();
            *self.threads.current_mut().callbacks.last_mut().unwrap() = Callback::Fls(frame);
        } else {
            self.threads.current_mut().callbacks.pop();
            if let Some(call) = frame.call {
                call.complete(&mut restored, 1);
            }
        }
        *state = restored;
        memory.clear_fault();
        Ok(())
    }
}
