//! PE32+ process bring-up: actual guest DLL imports, high-address exports and attach ordering.
use super::{
    cpu::Cpu64,
    memory::{Memory, PAGE, READ, WRITE},
    pe::{Exports64, Pe64},
    runtime::{Control, Runtime64},
};
use std::{
    collections::{BTreeMap, VecDeque},
    path::Path,
};
mod exception;
const STACK: u64 = 0x7ff000000000;
const RETURN: u64 = 0x7ffe10000000;
const CALLBACK_RETURN: u64 = RETURN + 16;
#[derive(Clone)]
struct Call {
    name: String,
    target: u64,
    base: u64,
    notify: bool,
    require_true: bool,
    reason: u32,
    parameter: u64,
}
struct Context {
    cpu: Cpu64,
    current: Call,
    calls: VecDeque<Call>,
    rsp: u64,
    exit_code: Option<u32>,
    started: bool,
    callbacks: Vec<CallbackFrame>,
}
#[derive(Clone)]
struct CallbackFrame {
    parent: Cpu64,
    calls: VecDeque<CallbackCall>,
    active: Option<CallbackCall>,
    release: Vec<u64>,
    stop_on_false: bool,
    return_boolean: bool,
    window_call: Option<super::runtime::WindowCall>,
    loader_parent: Option<Option<u64>>,
    load: Option<(String, String, Vec<u64>, u64)>,
    lookup: Option<(String, u64, Vec<u64>)>,
    exception: Option<super::runtime::exception::Exception>,
}
#[derive(Clone)]
struct CallbackCall {
    target: u64,
    args: [u64; 4],
    require_true: bool,
    label: Option<String>,
}
struct Image {
    pe: Pe64,
    bytes: Vec<u8>,
    base: u64,
    name: String,
    main: bool,
}
#[derive(Default)]
struct Modules {
    exports: BTreeMap<String, Exports64>,
    ranges: Vec<(u64, u64)>,
    pending: Vec<Image>,
}
fn provider(name: &str) -> bool {
    crate::modules::native_module(name)
        || matches!(
            name,
            "api-ms-win-core-synch-l1-2-0.dll"
                | "api-ms-win-core-fibers-l1-1-1.dll"
                | "api-ms-win-core-localization-l1-2-1.dll"
                | "api-ms-win-core-winrt-l1-1-0.dll"
                | "api-ms-win-core-winrt-string-l1-1-0.dll"
        )
        || matches!(
            name,
            "kernelbase.dll"
                | "setupapi.dll"
                | "opengl32.dll"
                | "iphlpapi.dll"
                | "winhttp.dll"
                | "bcrypt.dll"
                | "hid.dll"
                | "d3d11.dll"
                | "d3d12.dll"
                | "dxgi.dll"
                | "uiautomationcore.dll"
                | "crypt32.dll"
                | "dwmapi.dll"
                | "secur32.dll"
        )
}
impl Modules {
    fn load(
        &mut self,
        path: &Path,
        main: bool,
        fs: &crate::fs::GuestFs,
        mem: &mut Memory,
        runtime: &mut Runtime64,
        search: (u64, Option<&str>),
    ) -> Result<u64, String> {
        if self.exports.len() >= 64 {
            return Err("AMD64 module capacity exceeded".into());
        }
        let name = path
            .file_name()
            .ok_or("DLL has no filename")?
            .to_string_lossy()
            .to_ascii_lowercase();
        if self.exports.contains_key(&name) {
            return Err(format!("duplicate AMD64 module request: {name}"));
        }
        let bytes = std::fs::read(path).map_err(|e| format!("load {}: {e}", path.display()))?;
        let pe = Pe64::parse(&bytes)?;
        if !main && !pe.is_dll {
            return Err(format!("AMD64 dependency is not a DLL: {name}"));
        }
        let size = pe.size_of_image as u64;
        let available = |base| {
            mem.available(base, size)
                && self
                    .ranges
                    .iter()
                    .all(|&(a, b)| base + size <= a || base >= b)
        };
        let mut base = pe.image_base;
        if !available(base) {
            base = 0x180000000;
            while !available(base) {
                base = base
                    .checked_add(size.div_ceil(65536) * 65536)
                    .filter(|&a| a + size < 1 << 47)
                    .ok_or("AMD64 module address space exhausted")?;
            }
        }
        self.ranges.push((base, base + size));
        self.exports.insert(name.clone(), pe.exports(&bytes, base)?);
        let mapped = pe.load_with_runtime(&bytes, base, |dll, function| {
            let dll = dll.to_ascii_lowercase();
            if provider(&dll) {
                runtime.register_provider(&dll);
                return Ok(runtime.register(
                    &dll,
                    &function
                        .name
                        .clone()
                        .unwrap_or_else(|| format!("#{}", function.ordinal.unwrap_or(0))),
                ));
            }
            if !self.exports.contains_key(&dll) {
                let path = runtime
                    .module_path_with_search(fs, &dll, search.0, search.1)
                    .map_err(|e| format!("AMD64 DLL path error {e}: {dll}"))?;
                self.load(&path, false, fs, mem, runtime, search)?;
            }
            let exports = &self.exports[&dll];
            let address = if let Some(name) = &function.name {
                exports.names.get(name)
            } else {
                function.ordinal.and_then(|o| exports.ordinals.get(&o))
            };
            address
                .copied()
                .ok_or_else(|| format!("AMD64 export missing: {dll}!{:?}", function.name))
        })?;
        mem.merge(mapped)?;
        runtime.register_module(&name, base, pe.exports(&bytes, base)?);
        runtime.register_module_path(base, fs.guest_path(path));
        runtime.register_image(
            base,
            pe.size_of_image,
            if pe.entry_rva == 0 {
                0
            } else {
                base + pe.entry_rva as u64
            },
        );
        if let Some(dir) = pe.dirs.get(3) {
            runtime.register_static_functions(mem, base, dir.rva, dir.size)?;
        }
        self.pending.push(Image {
            pe,
            bytes,
            base,
            name,
            main,
        });
        Ok(base)
    }
}
pub struct Process64 {
    pub memory: Memory,
    pub cpu: Cpu64,
    pub runtime: Runtime64,
    pub modules: Vec<(String, u64)>,
    pub stages: Vec<(String, u64)>,
    calls: VecDeque<Call>,
    current: Call,
    rsp: u64,
    contexts: BTreeMap<u64, Context>,
    notifications: Vec<(String, u64, Vec<u64>, u64)>,
    loader_owner: Option<u64>,
    exit_code: Option<u32>,
    quantum_start: u64,
    terminated: Option<u32>,
    scheduling: bool,
    callbacks: Vec<CallbackFrame>,
    loader: Modules,
    presenter: Option<super::presenter::Presenter64>,
    next_ui_work: u64,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Returned(u64),
    Exited(u32),
    Budget,
}
impl Process64 {
    pub fn load(path: &Path, dll_only: bool) -> Result<Self, String> {
        let path = path.canonicalize().map_err(|e| e.to_string())?;
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        let pe = Pe64::parse(&bytes)?;
        if pe.is_dll != dll_only {
            return Err("DLL startup requires --dll; EXE startup does not accept --dll".into());
        }
        let reserve = pe
            .stack_reserve
            .max(PAGE)
            .checked_add(PAGE - 1)
            .ok_or("stack reserve overflow")?
            / PAGE
            * PAGE;
        if reserve > 64 * 1024 * 1024 {
            return Err("AMD64 stack exceeds 64 MiB".into());
        }
        let rsp = STACK + reserve - 160 + 8; // Shadow space and up to sixteen stack/register arguments.
        let mut memory = Memory::default();
        memory.map(STACK, reserve, READ | WRITE)?;
        memory.reserve_runtime(RETURN, PAGE)?;
        let display = path
            .file_name()
            .ok_or("missing EXE name")?
            .to_string_lossy();
        let mut runtime = Runtime64::new(
            &mut memory,
            pe.image_base,
            STACK,
            reserve,
            &format!("\"C:\\{display}\""),
        )?;
        runtime.install_gateways(Default::default());
        let fs = crate::fs::GuestFs::new(path.parent().ok_or("missing game directory")?)
            .map_err(|e| e.to_string())?;
        let mut loaded = Modules::default();
        let main_base = loaded.load(&path, true, &fs, &mut memory, &mut runtime, (0, None))?;
        runtime.set_main_base(&mut memory, main_base)?;
        let mut calls = VecDeque::new();
        let mut modules = Vec::new();
        let mut notifications = Vec::new();
        // Every module's TLS data exists before any callback/entry executes.
        let images = std::mem::take(&mut loaded.pending);
        for image in &images {
            let tls = image.pe.tls(&memory, image.base)?;
            runtime.install_delays(
                &mut memory,
                image.base,
                &image.pe.delay_imports(&image.bytes)?,
            )?;
            if let Some(tls) = tls {
                runtime.initialize_tls(&mut memory, &tls)?;
            }
        }
        for image in &images {
            modules.push((image.name.clone(), image.base));
            let mut callbacks = Vec::new();
            if let Some(tls) = image.pe.tls(&memory, image.base)? {
                for target in tls.callbacks {
                    callbacks.push(target);
                    calls.push_back(Call {
                        name: format!("{} TLS attach", image.name),
                        target,
                        base: image.base,
                        notify: true,
                        require_true: false,
                        reason: 1,
                        parameter: 0,
                    });
                }
            }
            if image.pe.entry_rva != 0 {
                calls.push_back(Call {
                    name: format!("{} entry", image.name),
                    target: image.base + image.pe.entry_rva as u64,
                    base: image.base,
                    notify: !image.main || dll_only,
                    require_true: !image.main || dll_only,
                    reason: 1,
                    parameter: 0,
                });
            } else if image.main && !dll_only {
                return Err("AMD64 executable has no entry point".into());
            }
            notifications.push((
                image.name.clone(),
                image.base,
                callbacks,
                if image.main && !dll_only || image.pe.entry_rva == 0 {
                    0
                } else {
                    image.base + image.pe.entry_rva as u64
                },
            ));
        }
        let current = calls.pop_front().unwrap_or(Call {
            name: "no DLL entry".into(),
            target: RETURN,
            base: pe.image_base,
            notify: false,
            require_true: false,
            reason: 1,
            parameter: 0,
        });
        runtime.set_filesystem(fs);
        runtime.prepare_user_profile()?;
        runtime.enable_scheduler();
        let mut process = Self {
            memory,
            cpu: Cpu64::new(current.target, rsp),
            runtime,
            modules,
            stages: Vec::new(),
            calls,
            current,
            rsp,
            contexts: BTreeMap::new(),
            notifications,
            loader_owner: None,
            exit_code: None,
            quantum_start: 0,
            terminated: None,
            scheduling: false,
            callbacks: Vec::new(),
            loader: loaded,
            presenter: None,
            next_ui_work: 0,
        };
        process.begin()?;
        Ok(process)
    }
    fn begin(&mut self) -> Result<(), String> {
        self.stages
            .push((self.current.name.clone(), self.current.target));
        let retired = self.cpu.retired;
        let work = self.cpu.work;
        let mxcsr = self.cpu.mxcsr;
        let fpu = self.cpu.fpu;
        self.cpu = Cpu64::new(self.current.target, self.rsp);
        self.cpu.retired = retired;
        self.cpu.work = work;
        self.cpu.mxcsr = mxcsr;
        self.cpu.fpu = fpu;
        self.cpu.gs_base = self.runtime.teb();
        self.memory.write_int(self.rsp, 8, RETURN)?;
        if self.current.notify {
            self.cpu.regs[1] = self.current.base;
            self.cpu.regs[2] = self.current.reason as u64;
            self.cpu.regs[8] = 0;
            self.loader_owner = Some(self.runtime.active_thread());
        } else {
            self.cpu.regs[1] = self.current.parameter;
            if self.loader_owner == Some(self.runtime.active_thread()) {
                self.loader_owner = None;
            }
        }
        if self.current.target == RETURN {
            self.cpu.regs[0] = 1;
        }
        Ok(())
    }
    pub fn enable_window(&mut self) -> Result<(), String> {
        if self.presenter.is_none() {
            self.presenter = Some(super::presenter::Presenter64::new()?);
        }
        Ok(())
    }
    pub fn poll_window(&mut self, force: bool) -> Result<(), String> {
        if let Some(presenter) = self.presenter.as_mut() {
            presenter.poll(&mut self.runtime, force)?;
        }
        Ok(())
    }
    pub fn phase(&self) -> &str {
        &self.current.name
    }
    fn thread_notifications(&self, reason: u32) -> VecDeque<Call> {
        let mut calls = VecDeque::new();
        let mut images: Vec<_> = self.notifications.iter().collect();
        if reason == 3 {
            images.reverse();
        }
        for (name, base, callbacks, entry) in images {
            for &target in callbacks {
                calls.push_back(Call {
                    name: format!("{name} TLS thread {reason}"),
                    target,
                    base: *base,
                    notify: true,
                    require_true: false,
                    reason,
                    parameter: 0,
                });
            }
            if *entry != 0 {
                calls.push_back(Call {
                    name: format!("{name} thread {reason}"),
                    target: *entry,
                    base: *base,
                    notify: true,
                    require_true: false,
                    reason,
                    parameter: 0,
                });
            }
        }
        calls
    }
    fn accept_threads(&mut self) -> Result<(), String> {
        for spawn in self.runtime.take_threads() {
            let current = Call {
                name: format!("thread {} entry", spawn.id),
                target: spawn.cpu.rip,
                base: 0,
                notify: false,
                require_true: false,
                reason: 0,
                parameter: spawn.parameter,
            };
            let rsp = spawn.cpu.regs[4];
            // begin() will set the callback arguments and return frame when first scheduled.
            self.contexts.insert(
                spawn.id,
                Context {
                    cpu: spawn.cpu,
                    current,
                    calls: VecDeque::new(),
                    rsp,
                    exit_code: None,
                    started: false,
                    callbacks: Vec::new(),
                },
            );
        }
        Ok(())
    }
    fn schedule(&mut self, finished: bool, budget: u64) -> Result<(), String> {
        self.accept_threads()?;
        let id = self.runtime.active_thread();
        let retired = self.cpu.retired;
        if !finished && !self.scheduling {
            self.contexts.insert(
                id,
                Context {
                    cpu: self.cpu.clone(),
                    current: self.current.clone(),
                    calls: std::mem::take(&mut self.calls),
                    rsp: self.rsp,
                    exit_code: self.exit_code,
                    started: true,
                    callbacks: std::mem::take(&mut self.callbacks),
                },
            );
        }
        self.scheduling = true;
        loop {
            self.poll_window(false)?;
            // Stable round robin, including a single runnable thread.
            let ids: Vec<_> = self
                .contexts
                .range((id + 1)..)
                .map(|(&id, _)| id)
                .chain(self.contexts.range(..=id).map(|(&id, _)| id))
                .collect();
            for next in ids {
                if (self.contexts[&next].current.notify
                    || !self.contexts[&next].started
                    || self.contexts[&next]
                        .callbacks
                        .iter()
                        .any(|frame| frame.loader_parent.is_some()))
                    && self.loader_owner.is_some_and(|owner| owner != next)
                {
                    continue;
                }
                if !self.runtime.thread_runnable(next, &self.memory)? {
                    continue;
                }
                let context = self.contexts.remove(&next).unwrap();
                self.runtime.activate_thread(next);
                let work = self.cpu.work;
                self.cpu = context.cpu;
                self.cpu.retired = retired;
                self.cpu.work = work;
                self.current = context.current;
                self.calls = context.calls;
                self.rsp = context.rsp;
                self.exit_code = context.exit_code;
                self.callbacks = context.callbacks;
                // New contexts have no return frame yet. Their RIP is the user entry,
                // while current may be a TLS/DllMain notification preceding it.
                if !context.started {
                    let mut calls = self.thread_notifications(2);
                    calls.push_back(self.current.clone());
                    self.current = calls.pop_front().unwrap();
                    self.calls = calls;
                    self.begin()?;
                }
                self.quantum_start = work;
                self.scheduling = false;
                return Ok(());
            }
            if let Some(delay) = self.runtime.idle_delay() {
                if self.cpu.work >= budget {
                    return Ok(());
                }
                std::thread::sleep(delay);
                self.cpu.work += 1;
            } else {
                return Err("AMD64 guest deadlock: no runnable thread or timed wait".into());
            }
        }
    }
    fn exit_thread(&mut self, code: u32, budget: u64) -> Result<(), String> {
        // ExitThread does not return through a native callback's suspended API frame.
        for frame in std::mem::take(&mut self.callbacks) {
            self.runtime
                .release_callback_buffers(&mut self.memory, &frame.release)?;
        }
        self.exit_code = Some(code);
        self.calls = self
            .runtime
            .fls_destructors(&mut self.memory)?
            .into_iter()
            .map(|(target, parameter)| Call {
                name: "thread FLS destructor".into(),
                target,
                base: 0,
                notify: false,
                require_true: false,
                reason: 0,
                parameter,
            })
            .collect();
        self.calls.extend(self.thread_notifications(3));
        if let Some(call) = self.calls.pop_front() {
            self.current = call;
            self.begin()?;
        } else {
            self.runtime.finish_thread(&mut self.memory, code)?;
            if self.contexts.is_empty() {
                self.terminated = Some(code);
            } else {
                self.schedule(true, budget)?;
            }
        }
        Ok(())
    }
    fn begin_callback(&mut self) -> Result<(), String> {
        let frame = self.callbacks.last_mut().ok_or("missing callback frame")?;
        let call = frame.calls.pop_front().ok_or("missing callback call")?;
        let rsp = (frame.parent.regs[4]
            .checked_sub(48)
            .ok_or("callback stack underflow")?
            & !15)
            + 8;
        self.memory.check(rsp, 40, WRITE)?;
        self.memory.write_int(rsp, 8, CALLBACK_RETURN)?;
        let retired = self.cpu.retired;
        let work = self.cpu.work;
        self.cpu = frame.parent.clone();
        self.cpu.retired = retired;
        self.cpu.work = work;
        self.cpu.rip = call.target;
        self.cpu.regs[4] = rsp;
        for (reg, value) in [1, 2, 8, 9].into_iter().zip(call.args) {
            self.cpu.regs[reg] = value;
        }
        if let Some(label) = &call.label {
            self.stages.push((label.clone(), call.target));
        }
        frame.active = Some(call);
        Ok(())
    }
    fn load_module(
        &mut self,
        path: &str,
        search: (u64, Option<String>),
        dll: String,
        function: String,
        args: Vec<u64>,
    ) -> Result<(), String> {
        if self.callbacks.len() >= 32 {
            return Err("AMD64 native callback nesting capacity exceeded".into());
        }
        let rsp = (self.cpu.regs[4]
            .checked_sub(48)
            .ok_or("callback stack underflow")?
            & !15)
            + 8;
        self.memory.check(rsp, 40, WRITE)?;
        let fs = self.runtime.filesystem()?;
        let path = fs
            .resolve(path)
            .map_err(|error| format!("AMD64 DLL path error {error}: {path}"))?;
        let base = self.loader.load(
            &path,
            false,
            &fs,
            &mut self.memory,
            &mut self.runtime,
            (search.0, search.1.as_deref()),
        )?;
        let images = std::mem::take(&mut self.loader.pending);
        for image in &images {
            self.runtime.install_delays(
                &mut self.memory,
                image.base,
                &image.pe.delay_imports(&image.bytes)?,
            )?;
            if let Some(tls) = image.pe.tls(&self.memory, image.base)? {
                self.runtime.initialize_tls(&mut self.memory, &tls)?;
            }
        }
        let mut calls = VecDeque::new();
        for image in images {
            self.modules.push((image.name.clone(), image.base));
            let callbacks = image
                .pe
                .tls(&self.memory, image.base)?
                .map(|tls| tls.callbacks)
                .unwrap_or_default();
            for &target in &callbacks {
                calls.push_back(CallbackCall {
                    target,
                    args: [image.base, 1, 0, 0],
                    require_true: false,
                    label: Some(format!("{} dynamic TLS attach", image.name)),
                });
            }
            let entry = if image.pe.entry_rva == 0 {
                0
            } else {
                image.base + image.pe.entry_rva as u64
            };
            if entry != 0 {
                calls.push_back(CallbackCall {
                    target: entry,
                    args: [image.base, 1, 0, 0],
                    require_true: true,
                    label: Some(format!("{} dynamic entry", image.name)),
                });
            }
            self.notifications
                .push((image.name, image.base, callbacks, entry));
        }
        self.cpu.regs[0] = base;
        if calls.is_empty() {
            self.runtime.complete_call(&dll, &function, &args, base);
        } else {
            let owner = self.loader_owner.replace(self.runtime.active_thread());
            self.callbacks.push(CallbackFrame {
                parent: self.cpu.clone(),
                calls,
                active: None,
                release: Vec::new(),
                stop_on_false: false,
                return_boolean: false,
                window_call: None,
                loader_parent: Some(owner),
                load: Some((dll, function, args, base)),
                lookup: None,
                exception: None,
            });
            self.begin_callback()?;
        }
        Ok(())
    }
    pub fn run(&mut self, budget: u64) -> Result<Outcome, String> {
        while self.cpu.work < budget {
            if self.presenter.is_some() && self.cpu.work >= self.next_ui_work {
                self.poll_window(false)?;
                self.next_ui_work = self.cpu.work.saturating_add(65536);
            }
            if let Some(code) = self.terminated {
                return Ok(Outcome::Exited(code));
            }
            if self.scheduling {
                self.schedule(false, budget)?;
                if self.scheduling {
                    return Ok(Outcome::Budget);
                }
                continue;
            }
            if self.cpu.rip == CALLBACK_RETURN {
                if self
                    .callbacks
                    .last()
                    .is_some_and(|frame| frame.exception.is_some())
                {
                    self.resume_exception()?;
                    continue;
                }
                let frame = self.callbacks.last().ok_or("unexpected callback return")?;
                if frame.active.as_ref().is_some_and(|call| call.require_true)
                    && self.cpu.regs[0] as u32 == 0
                {
                    return Err(format!("{} DLL_PROCESS_ATTACH returned FALSE; dynamic load rollback is not implemented",
                        frame.active.as_ref().and_then(|call| call.label.as_deref()).unwrap_or("AMD64 dynamic DLL")));
                }
                let mut window_value = None;
                if let Some(call) = self.callbacks.last_mut().unwrap().window_call.as_mut() {
                    match self
                        .runtime
                        .window_resume(&mut self.memory, call, self.cpu.regs[0])?
                    {
                        super::runtime::WindowStep::Next(target, args) => {
                            self.callbacks
                                .last_mut()
                                .unwrap()
                                .calls
                                .push_front(CallbackCall {
                                    target,
                                    args,
                                    require_true: false,
                                    label: None,
                                });
                            self.begin_callback()?;
                            continue;
                        }
                        super::runtime::WindowStep::Returned(value) => window_value = Some(value),
                    }
                }
                let frame = self.callbacks.last().unwrap();
                if !frame.calls.is_empty() && !(frame.stop_on_false && self.cpu.regs[0] as u32 == 0)
                {
                    self.begin_callback()?;
                } else {
                    let lookup_value = if let Some((dll, base, args)) = &frame.lookup {
                        let value = self.runtime.finish_function_lookup(
                            &mut self.memory,
                            self.cpu.regs[0],
                            *base,
                            args[0],
                            args[1],
                        )?;
                        self.runtime
                            .complete_call(dll, "RtlLookupFunctionEntry", args, value);
                        Some(value)
                    } else if window_value.is_some() {
                        window_value
                    } else {
                        frame
                            .return_boolean
                            .then_some((self.cpu.regs[0] as u32 != 0) as u64)
                    };
                    let frame = self.callbacks.pop().unwrap();
                    if let Some(owner) = frame.loader_parent {
                        self.loader_owner = owner;
                    }
                    if let Some((dll, function, args, base)) = frame.load {
                        self.runtime.complete_call(&dll, &function, &args, base);
                    }
                    self.runtime
                        .release_callback_buffers(&mut self.memory, &frame.release)?;
                    let retired = self.cpu.retired;
                    let work = self.cpu.work;
                    self.cpu = frame.parent;
                    if let Some(value) = lookup_value {
                        self.cpu.regs[0] = value;
                    }
                    self.cpu.retired = retired;
                    self.cpu.work = work;
                }
                continue;
            }
            if self.cpu.rip == RETURN {
                if self.current.require_true && self.cpu.regs[0] as u32 == 0 {
                    return Err(format!(
                        "{} DLL_PROCESS_ATTACH returned FALSE",
                        self.phase()
                    ));
                }
                if let Some(call) = self.calls.pop_front() {
                    self.current = call;
                    self.begin()?;
                    continue;
                }
                if self.runtime.active_thread() != 1 || self.exit_code.is_some() {
                    if let Some(code) = self.exit_code {
                        self.runtime.finish_thread(&mut self.memory, code)?;
                        if self.loader_owner == Some(self.runtime.active_thread()) {
                            self.loader_owner = None;
                        }
                        if self.contexts.is_empty() {
                            self.terminated = Some(code);
                        } else {
                            self.schedule(true, budget)?;
                        }
                    } else {
                        self.exit_thread(self.cpu.regs[0] as u32, budget)?;
                    }
                    continue;
                }
                return Ok(Outcome::Returned(self.cpu.regs[0]));
            }
            if let Some(name) = self.runtime.gateways.get(&self.cpu.rip).cloned() {
                let (dll, function) = name.split_once('!').ok_or("invalid AMD64 gateway")?;
                let control = self
                    .runtime
                    .dispatch(dll, function, &mut self.cpu, &mut self.memory)
                    .map_err(|error| format!("{name}: {error}"))?;
                self.cpu.work += 1;
                match control {
                    Control::Returned => {
                        self.accept_threads()?;
                    }
                    Control::Yielded | Control::Blocked => {
                        self.schedule(false, budget)?;
                    }
                    Control::SwitchThread(dll) => {
                        self.accept_threads()?;
                        let active = self.runtime.active_thread();
                        let mut switched = false;
                        for (&id, context) in &self.contexts {
                            if id == active
                                || (context.current.notify
                                    || !context.started
                                    || context
                                        .callbacks
                                        .iter()
                                        .any(|frame| frame.loader_parent.is_some()))
                                    && self.loader_owner.is_some_and(|owner| owner != id)
                            {
                                continue;
                            }
                            if self.runtime.thread_runnable(id, &self.memory)? {
                                switched = true;
                                break;
                            }
                        }
                        self.cpu.regs[0] = switched as u64;
                        self.runtime
                            .complete_call(&dll, "SwitchToThread", &[], switched as u64);
                        if switched {
                            self.schedule(false, budget)?;
                        }
                    }
                    Control::ThreadExited(code) => {
                        self.exit_thread(code, budget)?;
                    }
                    Control::ThreadContext {
                        dll,
                        args,
                        thread,
                        flags,
                    } => {
                        self.accept_threads()?;
                        let context = self
                            .contexts
                            .get(&thread)
                            .ok_or("suspended guest thread has no CPU context")?;
                        let data = super::runtime::exception::capture(&context.cpu);
                        // Only requested groups are written; home slots and
                        // unrequested components keep the caller's bytes.
                        for (bit, ranges) in [
                            (1, &[(56, 58), (66, 72), (152, 160), (248, 256)][..]),
                            (2, &[(120, 152), (160, 248)][..]),
                            (4, &[(58, 66)][..]),
                            (8, &[(52, 56), (256, 768)][..]),
                            (16, &[(72, 120)][..]),
                        ] {
                            if flags & bit != 0 {
                                for &(start, end) in ranges {
                                    self.memory
                                        .write(args[1] + start as u64, &data[start..end])?;
                                }
                            }
                        }
                        self.runtime
                            .complete_call(&dll, "GetThreadContext", &args, 1);
                    }
                    Control::LoadModule {
                        path,
                        search,
                        dll,
                        function,
                        args,
                    } => {
                        self.load_module(&path, search, dll, function, args)?;
                    }
                    Control::FunctionLookup {
                        dll,
                        target,
                        context,
                        base,
                        args,
                    } => {
                        if self.callbacks.len() >= 32 {
                            return Err("AMD64 native callback nesting capacity exceeded".into());
                        }
                        self.callbacks.push(CallbackFrame {
                            parent: self.cpu.clone(),
                            calls: VecDeque::from([CallbackCall {
                                target,
                                args: [args[0], context, 0, 0],
                                require_true: false,
                                label: None,
                            }]),
                            active: None,
                            release: Vec::new(),
                            stop_on_false: false,
                            return_boolean: false,
                            window_call: None,
                            loader_parent: None,
                            load: None,
                            lookup: Some((dll, base, args)),
                            exception: None,
                        });
                        self.begin_callback()?;
                    }
                    Control::Callbacks {
                        calls,
                        release,
                        stop_on_false,
                        return_boolean,
                        window_call,
                    } => {
                        if calls.is_empty() {
                            self.runtime
                                .release_callback_buffers(&mut self.memory, &release)?;
                        } else {
                            if self.callbacks.len() >= 32 {
                                return Err(
                                    "AMD64 native callback nesting capacity exceeded".into()
                                );
                            }
                            self.callbacks.push(CallbackFrame {
                                parent: self.cpu.clone(),
                                calls: calls
                                    .into_iter()
                                    .map(|(target, args)| CallbackCall {
                                        target,
                                        args,
                                        require_true: false,
                                        label: None,
                                    })
                                    .collect(),
                                active: None,
                                release,
                                stop_on_false,
                                return_boolean,
                                window_call,
                                loader_parent: None,
                                load: None,
                                lookup: None,
                                exception: None,
                            });
                            self.begin_callback()?;
                        }
                    }
                    Control::Raise { dll, args, caller } => {
                        self.raise_exception(dll, args, *caller)?
                    }
                    Control::Unwind { dll, args } => self.unwind_exception(dll, args)?,
                    Control::Exited(code) => return Ok(Outcome::Exited(code)),
                }
            } else {
                self.cpu.step(&mut self.memory)?;
            }
            if !self.scheduling
                && (self.runtime.active_suspended()
                    || self.cpu.work - self.quantum_start >= 10000 && !self.contexts.is_empty())
            {
                self.schedule(false, budget)?;
            }
        }
        Ok(Outcome::Budget)
    }
}
