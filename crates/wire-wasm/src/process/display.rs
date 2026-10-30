//! Monitor enumeration lends a rectangle on the current guest stack.
use super::{
    callbacks::{CallFrame, Callback},
    unwind, *,
};
use crate::win32::{call::Call, display, user, ServiceError};

#[derive(Clone)]
pub(crate) struct Frame {
    pub(super) saved: Box<State>,
    pub(super) loader_depth: usize,
    call: Call,
    return_sp: u64,
}
impl Frame {
    pub(super) fn unwind_bridge(&self) -> unwind::Bridge {
        let mut caller = self.saved.clone();
        self.call.clone().complete(&mut caller, 1);
        unwind::Bridge {
            sp: self.return_sp,
            caller: Some((caller, true)),
        }
    }
}
impl Loaded {
    pub(crate) fn enumerate_monitors(
        &mut self,
        call: Call,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), ServiceError> {
        let a = call.arguments;
        if a[0] != 0 || a[2] == 0 {
            let result = user::error(memory, state, if a[0] != 0 { 50 } else { 87 })?;
            call.complete(state, result);
            return Ok(());
        }
        let rect = if a[1] == 0 {
            display::bounds()
        } else {
            user::geometry::intersection(display::bounds(), user::rect(memory, a[1])?).0
        };
        if user::geometry::empty(rect) {
            call.complete(state, 1);
            return Ok(());
        }
        if self.threads.current().callbacks.len() >= 32 {
            return Err(ServiceError::Unavailable(
                "monitor callback nesting capacity exceeded",
            ));
        }
        let top = memory.mode().address(state.regs[4]);
        let buffer = if memory.mode() == Mode::X86 {
            memory.mode().address(top.wrapping_sub(16))
        } else {
            top.checked_sub(16).ok_or(Fault {
                address: top,
                length: 16,
                access: WRITE,
                kind: crate::memory::FaultKind::Address,
            })?
        } & !15;
        let entry = CallFrame::new(
            memory.mode(),
            buffer,
            &[display::monitor(memory.mode()), 0, buffer, a[3]],
        )?;
        memory.check(buffer, 16, WRITE)?;
        memory.check(entry.sp, entry.bytes.len(), WRITE)?;
        let frame = Frame {
            saved: Box::new(state.clone()),
            loader_depth: self.callback_depth(),
            call,
            return_sp: entry.sp + if memory.mode() == Mode::X86 { 20 } else { 8 },
        };
        user::write_rect(memory, buffer, rect)?;
        entry.enter(a[2], state, memory)?;
        self.threads
            .current_mut()
            .callbacks
            .push(Callback::Monitor(Box::new(frame)));
        Ok(())
    }
    pub(super) fn resume_monitors(&mut self, state: &mut State) -> Result<(), StartupError> {
        let Some(Callback::Monitor(frame)) = self.threads.current_mut().callbacks.pop() else {
            return Err(unwind::invalid("monitor return has no continuation"));
        };
        let (work, retired, value) = (
            state.work,
            state.retired,
            u64::from(state.regs[0] as u32 != 0),
        );
        *state = *frame.saved;
        state.work = work;
        state.retired = retired;
        frame.call.complete(state, value);
        Ok(())
    }
}
