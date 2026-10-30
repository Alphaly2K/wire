//! Process shutdown owns the remaining thread and loader continuations.
use super::*;
use crate::win32::ServiceError;

pub(super) struct Pending {
    pub code: u32,
    fls: bool,
}
impl Loaded {
    fn stop_process_threads(&mut self, code: u32, all: bool) {
        let ids: Vec<_> = self
            .threads
            .snapshot()
            .map(|(id, _)| id)
            .filter(|&id| all || id != self.threads.active)
            .collect();
        for id in ids {
            self.discard_thread_callbacks(id, u64::MAX);
            self.objects.abandon_thread(id);
            self.user.exit_thread(id);
        }
        self.threads.stop_process(code, all);
    }
    pub(crate) fn begin_process_exit(
        &mut self,
        code: u32,
        state: &mut State,
        mode: Mode,
    ) -> Result<(), ServiceError> {
        if !self.can_exit_thread() {
            return Err(ServiceError::Unavailable(
                "process exit from an active loader or shutdown callback is not implemented",
            ));
        }
        self.clear_all_pending();
        self.discard_callbacks(u64::MAX);
        self.stop_process_threads(code, false);
        self.exiting = Some(Pending { code, fls: false });
        self.loader_owner = Some(self.threads.active);
        state.cancel_repeat();
        state.exception = 0;
        state.ip = gateways::layout(mode).returned;
        Ok(())
    }
    pub(super) fn process_exit_pending(&self, state: &State, mode: Mode) -> bool {
        self.exiting.is_some()
            && self.frames.is_empty()
            && self.threads.current().callbacks.is_empty()
            && state.ip == gateways::layout(mode).returned
    }
    pub(super) fn advance_process_exit(
        &mut self,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), StartupError> {
        if !self.exiting.as_ref().unwrap().fls {
            self.begin_fls_exit(state, memory)?;
            self.exiting.as_mut().unwrap().fls = true;
            if !self.threads.current().callbacks.is_empty() {
                return Ok(());
            }
        }
        self.process_detach(state, memory)
    }
    pub(crate) fn finish_process_exit(&mut self, code: u32, state: &mut State, mode: Mode) {
        self.stop_process_threads(code, true);
        self.frames.clear();
        self.initializers.clear();
        self.completed = None;
        self.require_true = false;
        self.entry_started = true;
        self.loader_owner = None;
        self.exiting = None;
        self.exception_registrations = Default::default();
        self.function_tables = Default::default();
        self.locals = Default::default();
        self.srw = Default::default();
        self.objects.clear();
        self.snapshots.clear();
        self.registry.clear();
        self.tokens.clear();
        self.rng.clear();
        self.crypto.clear();
        self.etw.clear();
        self.hstrings = Default::default();
        self.bstrings32 = Default::default();
        self.bstrings64 = Default::default();
        self.safe_arrays = Default::default();
        self.security = Default::default();
        self.winsock = Default::default();
        self.periods = Default::default();
        self.graphics = Default::default();
        self.wmi = Default::default();
        self.gdiplus = Default::default();
        self.d3d9 = Default::default();
        self.audio = Default::default();
        self.crt = Default::default();
        self.resources.clear();
        self.open_files = Default::default();
        self.file_searches.clear();
        self.standard.close_all();
        self.pipes.clear();
        self.input.close();
        self.user = crate::win32::user::User::new(mode);
        self.process.finish(code);
        state.cancel_repeat();
        state.exception = 0;
        state.regs[0] = code.into();
        state.ip = gateways::layout(mode).returned;
    }
}
