use super::{
    apcs, crt, display, exception_calls, exceptions, function_lookup, locals, nls, unwind, windows,
    Loaded, StartupError,
};
use crate::{
    machine::{Mode, State},
    memory::{Fault, FaultKind, Memory, WRITE},
    win32::gateways,
};

#[derive(Clone)]
pub(crate) enum Callback {
    Crt(Box<crt::Frame>),
    Exception(Box<exceptions::Frame>),
    ExceptionCall(Box<exception_calls::Frame>),
    Function(function_lookup::Frame),
    Window(Box<windows::Frame>),
    Fls(Box<locals::Frame>),
    Locale(Box<nls::Frame>),
    Monitor(Box<display::Frame>),
    Apc(Box<apcs::Frame>),
}
impl Callback {
    fn depth(&self) -> usize {
        match self {
            Self::Exception(f) => f.loader_depth,
            Self::ExceptionCall(f) => f.loader_depth,
            Self::Function(f) => f.loader_depth,
            Self::Window(f) => f.loader_depth,
            Self::Fls(f) => f.loader_depth,
            Self::Locale(f) => f.loader_depth,
            Self::Monitor(f) => f.loader_depth,
            Self::Apc(f) => f.loader_depth,
            Self::Crt(f) => f.loader_depth,
        }
    }
    pub(super) fn stack(&self) -> u64 {
        match self {
            Self::Exception(f) => f.saved.regs[4],
            Self::ExceptionCall(f) => f.saved.regs[4],
            Self::Function(f) => f.saved.regs[4],
            Self::Window(f) => f.saved.regs[4],
            Self::Fls(f) => f.saved.regs[4],
            Self::Locale(f) => f.saved.regs[4],
            Self::Monitor(f) => f.saved.regs[4],
            Self::Apc(f) => f.saved.regs[4],
            Self::Crt(f) => f.saved.regs[4],
        }
    }
    pub(super) fn exception(&self) -> Option<&exceptions::Frame> {
        if let Self::Exception(f) = self {
            Some(f)
        } else {
            None
        }
    }
}
impl Loaded {
    pub(crate) fn retire_escaped32(
        &mut self,
        state: &State,
    ) -> Result<(), crate::win32::ServiceError> {
        let escaped = |c: &&Callback| c.stack() <= state.regs[4];
        if !self.threads.current().callbacks.iter().any(|c| escaped(&c)) {
            return Ok(());
        }
        if self.loader_owner == Some(self.threads.active)
            && self
                .frames
                .last()
                .is_some_and(|f| state.regs[4] >= f.saved.regs[4])
        {
            return Err(crate::win32::ServiceError::Unavailable(
                "PE32 transfer cannot cross a loader notification",
            ));
        }
        if self
            .threads
            .current()
            .callbacks
            .iter()
            .filter(escaped)
            .any(|c| match c {
                Callback::Exception(f) => f.unwind_caller().is_none(),
                Callback::ExceptionCall(f) => f.is_unwind(),
                _ => false,
            })
        {
            return Err(crate::win32::ServiceError::Unavailable(
                "collided PE32 callback transfer is not implemented",
            ));
        }
        self.discard_callbacks(state.regs[4]);
        self.reconcile_pending(state);
        Ok(())
    }
    pub(super) fn unwind_bridges(&self) -> Vec<unwind::Bridge> {
        let callbacks = &self.threads.current().callbacks;
        callbacks
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, c)| c.depth() == self.callback_depth())
            .filter_map(|(index, c)| match c {
                Callback::Exception(f) => f.unwind_bridge(),
                Callback::ExceptionCall(f) => f.unwind_bridge(),
                Callback::Function(f) => Some(
                    f.unwind_bridge(index.checked_sub(1).and_then(|n| callbacks[n].exception())),
                ),
                Callback::Window(f) => Some(f.unwind_bridge()),
                Callback::Fls(f) => Some(f.unwind_bridge()),
                Callback::Locale(f) => Some(f.unwind_bridge()),
                Callback::Monitor(f) => Some(f.unwind_bridge()),
                Callback::Apc(f) => Some(f.unwind_bridge()),
                Callback::Crt(f) => Some(f.unwind_bridge()),
            })
            .collect()
    }
    pub(super) fn callback_depth(&self) -> usize {
        if self.loader_owner == Some(self.threads.active) {
            self.frames.len()
        } else {
            0
        }
    }
    pub(crate) fn callback_pending(&self, state: &State, mode: Mode) -> bool {
        self.threads.current().unhandled.is_some()
            || (state.ip == gateways::layout(mode).callback
                && self
                    .threads
                    .current()
                    .callbacks
                    .last()
                    .is_some_and(|f| f.depth() == self.callback_depth()))
    }
    pub(crate) fn resume_callback(
        &mut self,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), StartupError> {
        if let Some(code) = self.threads.current().unhandled {
            return Err(StartupError::Unhandled(code));
        }
        match self.threads.current().callbacks.last() {
            Some(Callback::Exception(_)) => self.resume_exception(state, memory),
            Some(Callback::ExceptionCall(_)) => self.resume_exception_call(state, memory),
            Some(Callback::Function(_)) => self.resume_function_lookup(state, memory),
            Some(Callback::Window(_)) => self.resume_window(state, memory),
            Some(Callback::Fls(_)) => self.resume_fls(state, memory),
            Some(Callback::Locale(_)) => self.resume_locales(state, memory),
            Some(Callback::Monitor(_)) => self.resume_monitors(state),
            Some(Callback::Apc(_)) => self.resume_apcs(state, memory),
            Some(Callback::Crt(_)) => self.resume_crt(state, memory),
            None => Err(StartupError::Invalid(
                "callback return has no continuation".into(),
            )),
        }
    }
    pub(super) fn discard_callbacks(&mut self, stack: u64) {
        self.discard_thread_callbacks(self.threads.active, stack);
    }
    pub(super) fn discard_thread_callbacks(&mut self, id: u32, stack: u64) {
        let mut abandoned = Vec::new();
        let mut dcs = Vec::new();
        self.threads.get_mut(id).callbacks.retain(|c| {
            let keep = c.stack() > stack;
            if !keep {
                if let Callback::Window(f) = c {
                    if let Some(dc) = f.abandoned_dc() {
                        dcs.push(dc);
                    }
                    if let Some(w) = f.abandoned_window() {
                        abandoned.push(w);
                    }
                }
            }
            keep
        });
        for dc in dcs {
            self.user.gdi.cancel_paint(dc);
        }
        for w in abandoned {
            self.user.remove(w);
        }
    }
}

pub(super) struct CallFrame {
    pub sp: u64,
    pub bytes: Vec<u8>,
    arguments: Vec<u64>,
}
impl CallFrame {
    pub(super) fn new(mode: Mode, top: u64, arguments: &[u64]) -> Result<Self, Fault> {
        let width = mode as usize / 8;
        let length = if mode == Mode::X86 {
            (arguments.len() + 1) * 4
        } else {
            (32 + arguments.len().saturating_sub(4) * 8).div_ceil(16) * 16 + 8
        };
        let sp = if mode == Mode::X86 {
            mode.address(top.wrapping_sub(length as u64))
        } else {
            (top & !15).checked_sub(length as u64).ok_or(Fault {
                address: top,
                length,
                access: WRITE,
                kind: FaultKind::Address,
            })?
        };
        let mut bytes = vec![0; length];
        bytes[..width].copy_from_slice(&gateways::layout(mode).callback.to_le_bytes()[..width]);
        for (n, value) in arguments.iter().enumerate() {
            let offset = if mode == Mode::X86 {
                4 + n * 4
            } else if n >= 4 {
                40 + (n - 4) * 8
            } else {
                continue;
            };
            bytes[offset..offset + width].copy_from_slice(&value.to_le_bytes()[..width]);
        }
        Ok(Self {
            sp,
            bytes,
            arguments: arguments.to_vec(),
        })
    }
    pub(super) fn enter(
        self,
        target: u64,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), Fault> {
        memory.write(self.sp, &self.bytes)?;
        if memory.mode() == Mode::X64 {
            for (n, value) in self.arguments.iter().take(4).enumerate() {
                state.regs[[1, 2, 8, 9][n]] = *value;
            }
        }
        state.regs[4] = self.sp;
        state.ip = memory.mode().address(target);
        state.exception = 0;
        state.cancel_repeat();
        Ok(())
    }
}
