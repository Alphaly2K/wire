//! PE32 composition root; guest instruction semantics remain on Cpu.
use crate::{
    api::{self, ApiLogger, ApiState, ApiTable, HandleObj},
    cpu::{Cpu, EAX},
    fs::GuestFs,
    loader::{self, LoadedImage},
    mem::GuestMem,
    thread::Sched,
};
use std::path::{Path, PathBuf};

pub(crate) mod callbacks;
pub(crate) mod dialog;
pub(crate) mod termination;
pub(crate) mod window_state;

pub struct Config {
    pub present: bool,
    pub capture_frames: Option<PathBuf>,
    pub gpu_enabled: bool,
    pub stop_after_presents: Option<u64>,
}

/// Field order preserves the prior CLI's drop order: execution/scheduler,
/// guest services (including native resources), image/API, memory, then logs.
pub struct Process32 {
    pub sched: Sched,
    pub cpu: Cpu,
    pub state: ApiState,
    pub loaded: LoadedImage,
    pub api: ApiTable,
    pub mem: GuestMem,
    pub logger: ApiLogger,
}
impl Process32 {
    pub fn load(
        exe_path: &Path,
        display_name: &str,
        fs: GuestFs,
        logger: ApiLogger,
        config: Config,
    ) -> Result<Self, String> {
        let mut mem = GuestMem::new().map_err(|e| format!("guest memory: {e}"))?;
        let mut api = ApiTable::new();
        let loaded = loader::load_image(&mut mem, &mut api, exe_path, display_name)
            .map_err(|e| format!("load {}: {e}", exe_path.display()))?;
        let mut state = ApiState::new(
            loaded.image_base,
            format!("C:\\{display_name}"),
            loaded.cmdline_a,
            loaded.cmdline_w,
        );
        state.fs = Some(fs);
        state.stop_after_presents = config.stop_after_presents;
        state.presenter = crate::host::ui::Presenter::new(config.present, config.capture_frames)
            .map_err(|e| format!("presenter: {e}"))?;
        state.d3d9.enable_gpu = config.gpu_enabled;
        state.d3d9.prepare_gpu();
        state
            .modules
            .register_main(
                &mem,
                loaded.image_base,
                exe_path
                    .canonicalize()
                    .unwrap_or_else(|_| exe_path.to_path_buf()),
                state.exe_path.clone(),
            )
            .map_err(|e| format!("register main module: {e}"))?;
        state
            .modules
            .setup_tls_thread(&mut mem, &mut state.heap, state.process_heap, loaded.teb)
            .map_err(|e| format!("initialize main static TLS: {e}"))?;
        for dll in api.imported_dlls() {
            if crate::modules::native_module(dll) {
                state.module_handle_for(dll);
            }
        }
        let cpu = Cpu::new(loaded.entry, loaded.esp, loaded.teb);
        let mut sched = Sched::new();
        let main_tid = sched.alloc_tid();
        let main_handle = state.alloc_handle(HandleObj::Thread { tid: main_tid });
        sched.add_main(main_tid, main_handle, loaded.teb);
        Ok(Self {
            sched,
            cpu,
            state,
            loaded,
            api,
            mem,
            logger,
        })
    }
    pub fn run(&mut self, max_instr: u64) -> RunOutcome {
        run_loop(
            &mut self.cpu,
            &mut self.mem,
            &mut self.api,
            &mut self.state,
            &mut self.logger,
            &mut self.sched,
            max_instr,
            None,
        )
    }
}

pub enum RunOutcome {
    PresentationLimit(u64),
    Exited(u32),
    InstructionLimit,
    Crashed(String),
    CallbackReturned, // internal: guest callback frame completed
    CallbackUnwound,  // internal: a nonlocal transfer discarded the callback stack
}

pub enum GuestCallOutcome {
    Returned(u32),
    Transferred,
    Exited(u32),
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_loop(
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
    callback_exit: Option<(u32, u32, u32)>,
) -> RunOutcome {
    loop {
        if cpu.halted {
            cpu.flush_flags();
            return RunOutcome::Exited(cpu.exit_code.unwrap_or(0));
        }
        if !sched.threads[sched.current].loader_initialized {
            // Mark before callbacks, which may re-enter the interpreter.
            let main = sched.threads[sched.current].is_main;
            sched.threads[sched.current].loader_initialized = true;
            let saved = cpu.save_ctx();
            match crate::modules::notify_thread(
                main, true, cpu, mem, api, state, log, sched, max_instr,
            ) {
                Ok(GuestCallOutcome::Returned(_)) => cpu.load_ctx(&saved),
                Ok(GuestCallOutcome::Exited(code)) => return RunOutcome::Exited(code),
                Ok(GuestCallOutcome::Transferred) => continue,
                Err(error) => return RunOutcome::Crashed(error),
            }
        }
        if callback_exit.is_none()
            && state
                .stop_after_presents
                .is_some_and(|n| state.d3d9.frames >= n)
        {
            cpu.flush_flags();
            return RunOutcome::PresentationLimit(state.d3d9.frames);
        }
        if let Some((exit, owner_teb, stack_ceiling)) = callback_exit {
            if cpu.fs_base == owner_teb {
                if cpu.eip == exit && cpu.esp() <= stack_ceiling {
                    cpu.flush_flags();
                    return RunOutcome::CallbackReturned;
                }
                if cpu.esp() >= stack_ceiling {
                    cpu.flush_flags();
                    return RunOutcome::CallbackUnwound;
                }
            }
        }
        if let Some(idx) = api.stub_index(cpu.eip) {
            cpu.flush_flags();
            if let Err(e) = api::dispatch(idx, cpu, mem, api, state, log, sched, max_instr) {
                return RunOutcome::Crashed(format!("api dispatch: {e}"));
            }
            continue;
        }
        if cpu.instr_count >= max_instr {
            cpu.flush_flags();
            return RunOutcome::InstructionLimit;
        }
        // Amortize outer halt/API/budget dispatch over straight-line IR.
        // The inner loop retains per-guest fault/code-generation checks.
        // End exactly at budget or cooperative scheduling boundaries.
        let slice_remaining = 0x10_0000 - (cpu.instr_count & 0xF_FFFF);
        let limit = cpu.execution_batch_limit(max_instr - cpu.instr_count, slice_remaining);
        // The executing thread cannot change inside the compact chain.
        // Check the owner's callback stack after every guest operation;
        // other threads still stop at the exact cooperative timeslice.
        let stack_ceiling = callback_exit
            .filter(|&(_, owner_teb, _)| cpu.fs_base == owner_teb)
            .map(|(_, _, ceiling)| ceiling);
        let (retired, result) = cpu.steps_cached(mem, limit, stack_ceiling);
        cpu.instr_count += retired;
        match result {
            Ok(()) => {
                // Cooperative timeslice: a guest thread that never calls a
                // blocking API (spin loop) must not starve the others.
                if cpu.instr_count & 0xF_FFFF == 0 {
                    crate::win32::x86::host::poll(mem, state, sched);
                    sched.round_robin(cpu);
                }
            }
            Err(e) => {
                cpu.flush_flags();
                return RunOutcome::Crashed(e.to_string());
            }
        }
    }
}

/// Call a guest function from the host (e.g. CRT initializers in
/// _initterm, later window procs). Pushes a fresh frame whose return
/// address is the ExitEmulation stub and runs until the guest `ret`s
/// into it. A nonlocal stack transfer or process exit is returned separately
/// so the host must not restore a callback frame that no longer exists.
/// Nested callbacks are supported.
#[allow(clippy::too_many_arguments)]
pub(crate) fn call_guest(
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    func: u32,
    args: &[u32],
    max_instr: u64,
) -> Result<GuestCallOutcome, String> {
    let exit_stub = api
        .lookup("wire", "ExitEmulation")
        .ok_or("missing exit stub")?;
    let stack_ceiling = cpu.esp();
    for a in args.iter().rev() {
        cpu.push32(mem, *a).map_err(|e| e.to_string())?;
    }
    cpu.push32(mem, exit_stub).map_err(|e| e.to_string())?;
    let saved_eip = cpu.eip;
    cpu.eip = func;
    let r = run_loop(
        cpu,
        mem,
        api,
        state,
        log,
        sched,
        max_instr,
        Some((exit_stub, cpu.fs_base, stack_ceiling)),
    );
    match r {
        RunOutcome::CallbackReturned => {
            cpu.eip = saved_eip;
            Ok(GuestCallOutcome::Returned(cpu.regs[EAX]))
        }
        RunOutcome::CallbackUnwound => Ok(GuestCallOutcome::Transferred),
        RunOutcome::Exited(c) => Ok(GuestCallOutcome::Exited(c)),
        RunOutcome::InstructionLimit => Err("instruction limit inside callback".into()),
        RunOutcome::Crashed(e) => Err(e),
        RunOutcome::PresentationLimit(_) => {
            Err("presentation benchmark boundary escaped a guest callback".into())
        }
    }
}
