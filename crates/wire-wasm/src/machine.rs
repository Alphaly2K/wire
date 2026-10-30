//! Architectural state shared by generated Wasm and Windows service adapters.
pub use crate::x87::Fpu;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Mode {
    X86 = 32,
    X64 = 64,
}

impl Mode {
    pub fn address(self, value: u64) -> u64 {
        match self {
            Self::X86 => u64::from(value as u32),
            Self::X64 => value,
        }
    }
}

/// Internal restart state for a faultable repeated comparison, including
/// its exact instruction bytes. Artificial budget yields retain this state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct Repeat {
    pub pc: u64,
    pub code: [u64; 2],
    pub flags: u64,
    pub active: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct State {
    /// RAX, RCX, RDX, RBX, RSP, RBP, RSI, RDI, then R8 through R15.
    pub regs: [u64; 16],
    pub xmm: [[u8; 16]; 16],
    pub mxcsr: u32,
    pub fpu: Fpu,
    pub ip: u64,
    pub flags: u64,
    pub fs_base: u64,
    pub gs_base: u64,
    pub retired: u64,
    /// Includes completed REP elements; an incomplete REP has not retired.
    pub work: u64,
    /// Zero when clear; otherwise the pending x86 exception vector plus one.
    pub exception: u32,
    pub(crate) repeat: Repeat,
}

impl State {
    /// Call before externally editing a suspended CPU context. Memory repairs
    /// and code-cache eviction alone do not cancel a repeated comparison.
    pub fn cancel_repeat(&mut self) {
        self.repeat.active = 0;
    }

    pub fn new(mode: Mode, ip: u64, sp: u64) -> Self {
        let mut regs = [0; 16];
        regs[4] = mode.address(sp);
        Self {
            regs,
            xmm: [[0; 16]; 16],
            mxcsr: 0x1f80,
            fpu: Fpu::default(),
            ip: mode.address(ip),
            flags: 0x202,
            fs_base: 0,
            gs_base: 0,
            retired: 0,
            work: 0,
            exception: 0,
            repeat: Repeat::default(),
        }
    }
}

#[derive(Clone, Copy)]
pub enum WaitReason {
    Retry,
    Loader,
}

/// Resolved host input supplied while guest execution is suspended.
#[derive(Clone, Copy)]
pub struct WindowInput {
    pub event: crate::WindowEvent,
    pub time: u32,
    pub point: Option<[i32; 2]>,
}

pub struct WindowPixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Owns architectural state and source memory. The caller installs the emitted
/// module into the same Wasm engine and revalidates it immediately before run.
pub struct Machine {
    pub state: State,
    pub memory: crate::memory::Memory,
    pub code_cache: crate::jit::cache::CodeCache,
    pub services: crate::win32::Services,
    pub image: Option<crate::process::Loaded>,
    pub module_files: crate::pe::linker::Files,
    current: Option<crate::jit::cache::ModuleId>,
    fetch_failure: Option<(u64, crate::memory::Fault)>,
    next_run: u64,
    run_hint: crate::jit::cache::ModuleId,
    run_pins: std::collections::BTreeSet<crate::jit::cache::ModuleId>,
    #[cfg(target_arch = "wasm32")]
    pub(crate) numeric: [u64; 2],
    #[cfg(target_arch = "wasm32")]
    pub(crate) floating: crate::floating::Output,
}

#[derive(Clone, Copy, Debug)]
pub enum BeginError {
    Busy,
    IdentityExhausted,
}
impl std::fmt::Display for BeginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Busy => "machine execution is already active",
            Self::IdentityExhausted => "execution identity exhausted",
        })
    }
}
impl std::error::Error for BeginError {}

#[derive(Debug)]
pub enum PrepareError {
    ProcessExited,
    Compile(crate::jit::CompileError),
    Cache(crate::jit::cache::CacheError),
}

impl std::fmt::Display for PrepareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProcessExited => f.write_str("process has exited"),
            Self::Compile(error) => error.fmt(f),
            Self::Cache(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for PrepareError {}

impl Machine {
    /// Configure the process's Windows timezone before loading or executing an image.
    pub fn configure_timezone(&mut self, bytes: [u8; 172]) -> Result<(), String> {
        if self.has_activity() || self.image.is_some() || self.state.work != 0 {
            return Err(
                "timezone configuration requires an unstarted machine without an image".into(),
            );
        }
        self.services.set_time_zone(bytes)
    }
    pub fn window_pixels(&self, window: u64) -> Result<Option<WindowPixels>, String> {
        if self.has_activity() {
            return Err("machine execution is already active".into());
        }
        self.image
            .as_ref()
            .map_or(Ok(None), |i| i.user.gdi.snapshot(window))
    }
    pub fn window_info(&self) -> Result<Vec<crate::WindowInfo>, String> {
        if self.has_activity() {
            return Err("machine execution is already active".into());
        }
        Ok(self
            .image
            .as_ref()
            .map_or_else(Vec::new, |i| i.user.window_info()))
    }

    pub fn queue_window_events(&mut self, inputs: &[WindowInput]) -> Result<(), String> {
        if self.has_activity() {
            return Err("machine execution is already active".into());
        }
        self.image
            .as_mut()
            .ok_or("window input requires a loaded image")?
            .queue_window_events(self.memory.mode(), inputs)
    }

    pub fn write_input(&mut self, bytes: &[u8], end: bool) -> Result<(), String> {
        if self.has_activity() {
            return Err("machine execution is already active".into());
        }
        self.image
            .as_mut()
            .ok_or("standard input requires a loaded image")?
            .input
            .write(bytes, end)
            .map_err(|code| format!("standard input rejected (Windows error {code})"))
    }
    pub fn read_output(&mut self, endpoint: u32, bytes: &mut [u8]) -> Result<usize, String> {
        if self.has_activity() {
            return Err("machine execution is already active".into());
        }
        self.image
            .as_mut()
            .ok_or("standard output requires a loaded image")?
            .streams
            .read(endpoint, bytes)
    }
    pub fn new(mode: Mode, ip: u64, sp: u64) -> Self {
        Self {
            state: State::new(mode, ip, sp),
            memory: crate::memory::Memory::new(mode),
            code_cache: crate::jit::cache::CodeCache::default(),
            services: crate::win32::Services::new(mode),
            image: None,
            module_files: Default::default(),
            current: None,
            fetch_failure: None,
            next_run: 0,
            run_hint: 0,
            run_pins: std::collections::BTreeSet::new(),
            #[cfg(target_arch = "wasm32")]
            numeric: [0; 2],
            #[cfg(target_arch = "wasm32")]
            floating: crate::floating::Output::default(),
        }
    }

    pub fn prepare(&mut self) -> Result<crate::jit::cache::ModuleId, PrepareError> {
        self.current = None;
        self.fetch_failure = None;
        if self.process_exit_code().is_some() {
            return Err(PrepareError::ProcessExited);
        }
        let id = if let Some(id) = self.code_cache.find(&self.memory, self.state.ip) {
            id
        } else {
            let cache = &mut self.code_cache;
            let code = crate::jit::compile_until(&mut self.memory, self.state.ip, |memory, pc| {
                cache.covers(memory, pc)
            })
            .map_err(|error| {
                if let crate::jit::CompileError::Fetch(fault) = &error {
                    self.fetch_failure = Some((self.state.ip, *fault));
                }
                PrepareError::Compile(error)
            })?;
            self.code_cache.insert(code).map_err(PrepareError::Cache)?
        };
        self.current = Some(id);
        Ok(id)
    }

    pub fn load_pe(&mut self, bytes: &[u8], base: u64, command: &str) -> Result<(), String> {
        self.load_pe_with_import_policy(bytes, base, command, false)
    }

    pub fn load_pe_diagnostic(
        &mut self,
        bytes: &[u8],
        base: u64,
        command: &str,
    ) -> Result<(), String> {
        self.load_pe_with_import_policy(bytes, base, command, true)
    }

    fn load_pe_with_import_policy(
        &mut self,
        bytes: &[u8],
        base: u64,
        command: &str,
        diagnostic_imports: bool,
    ) -> Result<(), String> {
        self.load_pe_process(
            bytes,
            crate::process::Launch {
                base,
                command,
                executable: None,
                diagnostic_imports,
                process: crate::process::current::Current::new(self.memory.mode()),
            },
        )
        .map_err(|error| error.message)
    }

    pub(crate) fn load_pe_process(
        &mut self,
        bytes: &[u8],
        launch: crate::process::Launch<'_>,
    ) -> Result<(), crate::pe::linker::Error> {
        if self.has_activity()
            || self.image.is_some()
            || self.state.work != 0
            || self.state.retired != 0
        {
            return Err("PE startup requires an idle VM that has not executed an image".into());
        }
        let prepared = crate::process::prepare(
            &self.memory,
            &self.services,
            &self.module_files,
            bytes,
            launch,
        )?;
        let dep_enabled = prepared.memory.dep_enabled();
        self.memory.merge(prepared.memory)?;
        self.memory.set_dep_enabled(dep_enabled);
        self.state = prepared.state;
        self.services = prepared.services;
        self.module_files = prepared.files;
        self.image = Some(prepared.loaded);
        self.current = None;
        self.fetch_failure = None;
        self.code_cache.clear();
        Ok(())
    }

    pub fn startup_pending(&self) -> bool {
        self.image.as_ref().is_some_and(|image| {
            image.process.exit.is_none()
                && (image.callback_pending(&self.state, self.memory.mode())
                    || image.threads.current().delivery.is_some()
                    || image.startup_pending(&self.state, self.memory.mode()))
        })
    }

    pub fn advance_startup(&mut self) -> Result<(), crate::process::StartupError> {
        if self.startup_pending() {
            self.memory.clear_fault();
            let image = self.image.as_mut().unwrap();
            let result = if image.threads.current().delivery.is_some() {
                image.deliver_pending(&mut self.state, &mut self.memory)
            } else if image.callback_pending(&self.state, self.memory.mode()) {
                image.resume_callback(&mut self.state, &mut self.memory)
            } else {
                image.begin(
                    &mut self.state,
                    &mut self.memory,
                    &mut self.services,
                    &self.module_files,
                )
            };
            if std::mem::take(&mut image.unmapped) {
                self.code_cache.prune_invalid(&self.memory);
            }
            image.refresh_loader();
            if result.is_ok() {
                image.reconcile_pending(&self.state);
            }
            self.retire_process_code();
            result?;
        }
        Ok(())
    }

    pub fn schedule(
        &mut self,
        blocked: Option<WaitReason>,
        resume: bool,
    ) -> Result<Option<crate::jit::Exit>, crate::process::StartupError> {
        if self.process_exit_code().is_some() {
            self.retire_process_code();
            return Ok(Some(crate::jit::Exit::ProcessExited));
        }
        let Some(image) = self.image.as_mut() else {
            return Ok(None);
        };
        if let Some(code) = image.process.record.terminate.take() {
            image.finish_process_exit(code, &mut self.state, self.memory.mode());
            self.retire_process_code();
            return Ok(Some(crate::jit::Exit::ProcessExited));
        }
        let suspended = image.process.record.suspended.get();
        let priority = image.process.record.priority.get();
        if let Some(primary) = image.threads.primary() {
            primary.suspended = suspended;
            primary.priority = priority;
        }
        let result = image.schedule(&mut self.state, &mut self.memory, blocked, resume);
        if std::mem::take(&mut image.unmapped) {
            self.code_cache.prune_invalid(&self.memory);
        }
        result
    }

    pub fn deliver_exception(
        &mut self,
        reason: crate::jit::Exit,
    ) -> Result<bool, crate::process::StartupError> {
        if self.has_activity() {
            return Err(crate::process::StartupError::Invalid(
                "exception delivery requires a suspended machine".into(),
            ));
        }
        let Some(image) = self.image.as_mut() else {
            return Ok(false);
        };
        image.deliver_hardware(&mut self.state, &mut self.memory, reason)
    }

    pub fn current(&self) -> Option<crate::jit::cache::ModuleId> {
        self.current
    }

    pub fn boundary(&self) -> Option<crate::jit::Exit> {
        if self.process_exit_code().is_some() {
            return Some(crate::jit::Exit::ProcessExited);
        }
        crate::win32::gateways::layout(self.memory.mode()).exit(self.state.ip)
    }

    pub fn process_exit_code(&self) -> Option<u32> {
        self.image.as_ref().and_then(|image| image.process.exit)
    }

    pub(crate) fn retire_process_code(&mut self) {
        if self.process_exit_code().is_some() {
            if let Some(image) = self.image.as_mut().filter(|image| !image.disk_detached) {
                if self.module_files.shared_processes() {
                    self.module_files = self.module_files.clone();
                }
                image.disk_detached = true;
            }
            self.code_cache.clear();
            self.current = None;
            self.fetch_failure = None;
        }
    }

    pub fn code(&self, id: crate::jit::cache::ModuleId) -> Option<&crate::jit::Compiled> {
        self.code_cache.code(id)
    }

    pub fn ready(&mut self, id: crate::jit::cache::ModuleId) -> bool {
        self.process_exit_code().is_none() && self.code_cache.ready(&self.memory, self.state.ip, id)
    }

    pub fn has_activity(&self) -> bool {
        self.code_cache.has_pins() || self.code_cache.links().run.get() != 0
    }

    pub fn begin_run(&mut self, id: crate::jit::cache::ModuleId) -> Result<bool, BeginError> {
        if self.code_cache.links().run.get() != 0 {
            return Err(BeginError::Busy);
        }
        if !self.ready(id) || self.code_cache.slot(id) == 0 {
            return Ok(false);
        }
        let nonce = self
            .next_run
            .checked_add(1)
            .ok_or(BeginError::IdentityExhausted)?;
        if !self.code_cache.pin(&self.memory, id) {
            return Ok(false);
        }
        self.next_run = nonce;
        self.run_hint = id;
        self.run_pins.insert(id);
        self.code_cache.links().run.set(nonce);
        Ok(true)
    }

    pub fn link(&mut self, pc: u64) -> u32 {
        if self.code_cache.links().run.get() == 0 {
            return 0;
        }
        let id = if self.code_cache.ready(&self.memory, pc, self.run_hint)
            && self.code_cache.slot(self.run_hint) != 0
        {
            self.run_hint
        } else if let Some(id) = self.code_cache.find(&self.memory, pc) {
            id
        } else {
            return 0;
        };
        let slot = self.code_cache.slot(id);
        if slot == 0 {
            return 0;
        }
        if !self.run_pins.contains(&id) {
            if !self.code_cache.pin(&self.memory, id) {
                return 0;
            }
            self.run_pins.insert(id);
        }
        self.run_hint = id;
        self.code_cache.links().record(
            pc,
            self.memory.mapping_epoch(),
            self.memory.source_version(),
            slot,
        );
        slot
    }

    pub fn end_run(&mut self) {
        self.code_cache.links().run.set(0);
        self.run_hint = 0;
        for id in std::mem::take(&mut self.run_pins) {
            self.code_cache.release(&self.memory, id);
        }
    }

    /// Compilation observes guards without consuming them. An attempted run
    /// consumes the failed executable access while leaving the CPU unchanged.
    pub fn resolve_fetch_fault(&mut self) -> bool {
        let Some((pc, failed)) = self
            .fetch_failure
            .take()
            .filter(|(pc, _)| *pc == self.state.ip)
        else {
            return false;
        };
        let address = if failed.address == pc & !(crate::memory::PAGE - 1) {
            pc
        } else {
            failed.address
        };
        self.memory.clear_fault();
        if crate::win32::gateways::layout(self.memory.mode())
            .exit(failed.address)
            .is_some()
        {
            self.memory.record_fault(failed);
            return true;
        }
        match self.memory.fetch(address, &mut [0]) {
            Ok(()) => false,
            Err(fault) => {
                self.memory.record_fault(fault);
                true
            }
        }
    }
}
