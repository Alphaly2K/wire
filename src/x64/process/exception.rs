//! Software-exception callbacks share the process budget and thread contexts.
use super::{CallbackCall, CallbackFrame, Cpu64, Process64};
use crate::x64::runtime::exception::{self, Phase};
use std::collections::VecDeque;
impl Process64 {
    pub(super) fn raise_exception(
        &mut self,
        dll: String,
        args: Vec<u64>,
        caller: Cpu64,
    ) -> Result<(), String> {
        if self.callbacks.len() >= 32 {
            return Err("AMD64 exception/callback nesting capacity exceeded".into());
        }
        let state = self
            .runtime
            .prepare_exception(&mut self.memory, &caller, dll, args)?;
        let calls = self
            .runtime
            .vectored_handlers()
            .into_iter()
            .map(|target| CallbackCall {
                target,
                args: [state.pointers(), 0, 0, 0],
                require_true: false,
                label: Some("vectored exception handler".into()),
            })
            .collect();
        let block = state.block;
        self.callbacks.push(CallbackFrame {
            parent: caller,
            calls,
            active: None,
            release: vec![block],
            stop_on_false: false,
            loader_parent: None,
            load: None,
            lookup: None,
            exception: Some(state),
        });
        self.runtime.api_calls += 1;
        if self.callbacks.last().unwrap().calls.is_empty() {
            self.begin_language_exception()?;
        } else {
            self.begin_callback()?;
        }
        Ok(())
    }
    fn begin_language_exception(&mut self) -> Result<(), String> {
        let frame = self.callbacks.last().unwrap();
        let state = frame.exception.as_ref().unwrap();
        let parent = exception::restore(&self.memory, state.context(), &frame.parent)?;
        let handler = self
            .runtime
            .initial_exception_handler(&self.memory, &parent)?
            .ok_or_else(|| {
                format!(
                    "AMD64 exception search beyond immediate frame is not implemented: code={:#x}",
                    state.code
                )
            })?;
        // Dispatcher context uses Windows AMD64 offsets, including the ScopeIndex
        // consumed and updated by the actual guest language handler.
        let mut dispatcher = [0; 80];
        for (at, value) in [
            (0, parent.rip),
            (8, handler.base),
            (16, handler.entry),
            (24, handler.frame),
            (40, state.context()),
            (48, handler.target),
            (56, handler.data),
        ] {
            dispatcher[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        self.memory.write(state.dispatcher(), &dispatcher)?;
        let call = CallbackCall {
            target: handler.target,
            args: [
                state.block,
                handler.frame,
                state.context(),
                state.dispatcher(),
            ],
            require_true: false,
            label: Some("language exception handler".into()),
        };
        let frame = self.callbacks.last_mut().unwrap();
        frame.parent = parent;
        frame.calls = VecDeque::from([call]);
        let state = frame.exception.as_mut().unwrap();
        state.handler = Some(handler);
        state.phase = Phase::Language;
        self.begin_callback()
    }
    fn finish_exception(&mut self, mut continuation: Cpu64) -> Result<(), String> {
        let frame = self.callbacks.pop().unwrap();
        let state = frame.exception.unwrap();
        self.runtime
            .release_callback_buffers(&mut self.memory, &frame.release)?;
        self.runtime
            .complete_call(&state.dll, "RaiseException", &state.args, 0);
        continuation.retired = self.cpu.retired;
        continuation.work = self.cpu.work;
        self.cpu = continuation;
        Ok(())
    }
    pub(super) fn resume_exception(&mut self) -> Result<(), String> {
        let frame = self.callbacks.last().unwrap();
        let state = frame.exception.as_ref().unwrap();
        let disposition = self.cpu.regs[0] as u32;
        match state.phase {
            Phase::Vectored if disposition == u32::MAX => {
                if state.flags & 1 != 0 { return Err("guest continued a noncontinuable AMD64 exception".into()); }
                let continuation = exception::restore(&self.memory, state.context(), &frame.parent)?;
                self.finish_exception(continuation)
            }
            Phase::Vectored if disposition == 0 => {
                if frame.calls.is_empty() { self.begin_language_exception() } else { self.begin_callback() }
            }
            Phase::Language if disposition == 0 => {
                if state.flags & 1 != 0 { return Err("guest continued a noncontinuable AMD64 exception".into()); }
                let continuation = exception::restore(&self.memory, state.context(), &frame.parent)?;
                self.finish_exception(continuation)
            }
            Phase::Language if disposition == 1 => Err(format!(
                "AMD64 language handler continued search; outer-frame unwind is not implemented: code={:#x}", state.code)),
            Phase::Target { pc, value } if disposition == 1 => {
                let mut continuation = exception::restore(&self.memory, state.context(), &frame.parent)?;
                continuation.rip = pc;
                continuation.regs[4] = state.handler.as_ref().unwrap().frame;
                continuation.regs[0] = value;
                self.finish_exception(continuation)
            }
            _ => Err(format!("unsupported AMD64 {:?} exception disposition {disposition:#x}",
                match state.phase { Phase::Vectored => "vectored", Phase::Language => "language", Phase::Target {..} => "unwind" })),
        }
    }
    pub(super) fn unwind_exception(&mut self, dll: String, args: Vec<u64>) -> Result<(), String> {
        let frame = self
            .callbacks
            .last()
            .ok_or("RtlUnwindEx outside software exception dispatch is not implemented")?;
        let state = frame
            .exception
            .as_ref()
            .ok_or("RtlUnwindEx through nested callbacks is not implemented")?;
        let handler = state
            .handler
            .as_ref()
            .ok_or("RtlUnwindEx from vectored handler is not implemented")?;
        if !matches!(state.phase, Phase::Language)
            || args[0] != handler.frame
            || handler.frame != frame.parent.regs[4]
            || args[1] < handler.start
            || args[1] >= handler.end
            || args[2] != 0 && args[2] != state.block
            || args[4] != state.context()
            || args[5] != 0
        {
            return Err(
                "RtlUnwindEx requires unsupported outer-frame/collided/history unwind".into(),
            );
        }
        let mut continuation = exception::restore(&self.memory, state.context(), &frame.parent)?;
        continuation.rip = args[1];
        continuation.regs[4] = handler.frame;
        continuation.regs[0] = args[3];
        let target = handler.target;
        let call_args = [
            state.block,
            handler.frame,
            state.context(),
            state.dispatcher(),
        ];
        let run_unwind = handler.unwind;
        let flags = state.flags | 0x22; // EXCEPTION_UNWINDING | EXCEPTION_TARGET_UNWIND
        self.memory.write_int(state.block + 4, 4, flags as u64)?;
        self.memory.write_int(state.dispatcher() + 32, 8, args[1])?;
        self.memory.write_int(state.dispatcher() + 72, 4, 0)?;
        self.runtime.api_calls += 1;
        self.runtime
            .complete_call(&dll, "RtlUnwindEx", &args, args[3]);
        if run_unwind {
            let frame = self.callbacks.last_mut().unwrap();
            frame.exception.as_mut().unwrap().phase = Phase::Target {
                pc: args[1],
                value: args[3],
            };
            frame.calls = VecDeque::from([CallbackCall {
                target,
                args: call_args,
                require_true: false,
                label: Some("target unwind handler".into()),
            }]);
            self.begin_callback()
        } else {
            self.finish_exception(continuation)
        }
    }
}
