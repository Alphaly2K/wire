//! Compatibility methods for callers migrating to the Process32 boundary.
use super::Cpu;
use crate::{
    api::{ApiLogger, ApiState, ApiTable},
    mem::GuestMem,
    thread::Sched,
    x86::process::{self, GuestCallOutcome, RunOutcome},
};
impl Cpu {
    pub fn run(
        &mut self,
        mem: &mut GuestMem,
        api: &mut ApiTable,
        state: &mut ApiState,
        log: &mut ApiLogger,
        sched: &mut Sched,
        max_instr: u64,
    ) -> RunOutcome {
        process::run_loop(self, mem, api, state, log, sched, max_instr, None)
    }
    /// Call a guest function from the host (e.g. CRT initializers in
    /// _initterm, later window procs). Pushes a fresh frame whose return
    /// address is the ExitEmulation stub and runs until the guest `ret`s
    /// into it. A nonlocal stack transfer or process exit is returned separately
    /// so the host must not restore a callback frame that no longer exists.
    /// Nested callbacks are supported.
    #[allow(clippy::too_many_arguments)]
    pub fn call_guest(
        &mut self,
        mem: &mut GuestMem,
        api: &mut ApiTable,
        state: &mut ApiState,
        log: &mut ApiLogger,
        sched: &mut Sched,
        func: u32,
        args: &[u32],
        max_instr: u64,
    ) -> Result<GuestCallOutcome, String> {
        process::call_guest(self, mem, api, state, log, sched, func, args, max_instr)
    }
}
