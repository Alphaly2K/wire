use super::{
    callbacks::{CallFrame, Callback},
    unwind::{FunctionLookup, Request},
    *,
};
use crate::win32::{call::Call, ServiceError};

#[derive(Clone)]
pub(super) enum Finish {
    Api(Call),
    Exception,
}
#[derive(Clone)]
pub(crate) struct Frame {
    pub(super) saved: Box<State>,
    pub(super) loader_depth: usize,
    return_sp: u64,
    request: Request,
    finish: Finish,
}
impl Frame {
    pub(super) fn unwind_bridge(&self, parent: Option<&exceptions::Frame>) -> unwind::Bridge {
        let caller = match &self.finish {
            Finish::Api(call) => {
                let mut caller = self.saved.clone();
                call.clone().complete(&mut caller, 0);
                Some((caller, true))
            }
            Finish::Exception => parent.and_then(|f| f.unwind_caller()),
        };
        unwind::Bridge {
            sp: self.return_sp,
            caller,
        }
    }
}
impl Loaded {
    pub(super) fn begin_lookup(
        &mut self,
        request: Request,
        finish: Finish,
        top: u64,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), StartupError> {
        if self.threads.current().callbacks.len() >= 32 {
            return Err(unwind::invalid(
                "function-table callback nesting capacity exceeded",
            ));
        }
        let call = CallFrame::new(Mode::X64, top, &[request.pc, request.context])?;
        memory.check(call.sp, call.bytes.len(), WRITE)?;
        let frame = Frame {
            saved: Box::new(state.clone()),
            loader_depth: self.callback_depth(),
            return_sp: call.sp + 8,
            request,
            finish,
        };
        call.enter(request.target, state, memory)?;
        self.threads
            .current_mut()
            .callbacks
            .push(Callback::Function(frame));
        memory.clear_fault();
        Ok(())
    }
    pub(crate) fn lookup_function(
        &mut self,
        call: Call,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), ServiceError> {
        let result = (|| -> Result<(), StartupError> {
            memory.check(call.arguments[1], 8, WRITE)?;
            // HistoryTable is an optional cache hint; current mappings and
            // registered tables remain authoritative for every query.
            match self
                .function_tables
                .lookup(memory, &self.modules, call.arguments[0])?
            {
                FunctionLookup::Ready(function) => {
                    memory.write_int(call.arguments[1], 8, function.map_or(0, |f| f.base))?;
                    call.complete(state, function.map_or(0, |f| f.entry));
                    Ok(())
                }
                FunctionLookup::Callback(request) => {
                    self.begin_lookup(request, Finish::Api(call), state.regs[4], state, memory)
                }
            }
        })();
        result.map_err(|e| match e {
            StartupError::Fault(f) => ServiceError::Fault(f),
            _ => ServiceError::Unavailable("invalid or unsupported AMD64 function lookup"),
        })
    }
    pub(super) fn resume_function_lookup(
        &mut self,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), StartupError> {
        let Some(Callback::Function(frame)) = self.threads.current().callbacks.last() else {
            return Err(unwind::invalid("function lookup has no continuation"));
        };
        let frame = frame.clone();
        if let Finish::Api(call) = &frame.finish {
            memory.check(call.arguments[1], 8, WRITE)?;
        }
        let function = frame.request.finish(memory, state.regs[0])?;
        let parent = if matches!(frame.finish, Finish::Exception) {
            let mut parent = self
                .threads
                .current()
                .callbacks
                .iter()
                .rev()
                .nth(1)
                .and_then(|c| c.exception())
                .cloned()
                .ok_or_else(|| {
                    unwind::invalid("function lookup lost its exception continuation")
                })?;
            parent.resolve_lookup(frame.request.pc, function)?;
            Some(parent)
        } else {
            None
        };
        if let Finish::Api(call) = &frame.finish {
            memory.write_int(call.arguments[1], 8, function.map_or(0, |f| f.base))?;
        }
        let (work, retired) = (state.work, state.retired);
        *state = *frame.saved;
        state.work = work;
        state.retired = retired;
        self.threads.current_mut().callbacks.pop();
        match frame.finish {
            Finish::Api(call) => call.complete(state, function.map_or(0, |f| f.entry)),
            Finish::Exception => {
                *self.threads.current_mut().callbacks.last_mut().unwrap() =
                    Callback::Exception(Box::new(parent.unwrap()));
                state.ip = gateways::layout(Mode::X64).callback;
            }
        }
        memory.clear_fault();
        Ok(())
    }
}
