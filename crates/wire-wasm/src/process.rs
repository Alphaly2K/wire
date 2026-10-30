//! Initial image and thread setup are staged before the live VM is changed.
use crate::{
    machine::{Mode, State},
    memory::{Fault, Memory, PAGE, READ, WRITE},
    pe::{
        linker::{self, Files, Linked, Module, ModuleTls, Phase},
        Image,
    },
    win32::{gateways, Services},
};
use std::collections::{BTreeSet, VecDeque};
mod apcs;
mod callbacks;
#[path = "../../../src/win32/kernel/command_line.rs"]
pub(crate) mod command_line;
mod contexts;
mod crt;
pub(crate) use contexts::capture_call;
pub(crate) mod current;
mod delay;
mod display;
mod exception_calls;
pub(crate) mod exceptions;
mod function_lookup;
pub(crate) mod group;
mod locals;
mod modules;
mod nls;
mod symbols;
mod termination;
pub(crate) mod threads;
mod unwind;
mod virtual_unwind;
pub(crate) use virtual_unwind::virtual_unwind_call;
#[cfg(test)]
mod tests;
mod windows;
pub(crate) use modules::LoadError;
pub(crate) use symbols::Lookup;

#[derive(Clone, Copy)]
struct Initializer {
    target: u64,
    base: u64,
    reserved: u64,
    reason: u64,
    finish: bool,
    require_true: bool,
}
#[derive(Debug)]
pub enum StartupError {
    FileInput(crate::filesystem::ReadRequest),
    Fault(Fault),
    Rejected,
    Unhandled(u32),
    Invalid(String),
}
impl From<Fault> for StartupError {
    fn from(value: Fault) -> Self {
        Self::Fault(value)
    }
}

pub struct Loaded {
    pub(crate) crt: crate::win32::crt::Crt,
    pub(crate) d3d9: crate::win32::d3d9::Owner,
    pub(crate) audio: crate::win32::dsound::Owner,
    pub(crate) gdiplus: crate::win32::gdiplus::GdiPlus,
    pub(crate) wmi: crate::win32::wmi::Wmi,
    pub(crate) graphics: crate::win32::graphics::Owner,
    pub(crate) diagnostic_imports: bool,
    pub(crate) children: VecDeque<Box<crate::machine::Machine>>,
    pub(crate) startup_info: [[u8; 104]; 2],
    pub(crate) disk_detached: bool,
    pub info: [u64; 10],
    pub(crate) threads: threads::Threads,
    pub(crate) process: current::Current,
    pub(crate) snapshots: crate::win32::toolhelp::Snapshots,
    pub(crate) objects: crate::win32::objects::Objects,
    pub(crate) heaps: crate::win32::heaps::Heaps,
    pub(crate) temp_unique: u16,
    pub(crate) locals: crate::win32::locals::Slots,
    pub(crate) srw: crate::win32::srw::SrwLocks,
    pub(crate) environment: crate::win32::environment::Environment,
    pub(crate) user: crate::win32::user::User,
    pub(crate) registry: crate::win32::registry::Registry,
    pub(crate) tokens: crate::win32::tokens::Tokens,
    pub(crate) rng: crate::win32::rng_state::Providers,
    pub(crate) crypto: crate::win32::crypto::Crypto,
    pub(crate) data_pages: crate::win32::data_pages::Pages,
    pub(crate) resources: crate::win32::resources::Views,
    pub(crate) security: crate::win32::sspi_state::Security,
    pub(crate) winsock: crate::win32::winsock_core::Winsock,
    pub(crate) periods: crate::win32::period_requests::PeriodRequests,
    pub(crate) hstrings: crate::win32::winrt::Strings,
    pub(crate) bstrings32: crate::win32::automation32::Bstrings,
    pub(crate) bstrings64: crate::win32::automation64::Bstrings,
    pub(crate) safe_arrays: crate::win32::safe_arrays::Arrays,
    pub(crate) etw: crate::win32::tracing_state::Providers,
    exception_registrations: crate::win32::exception_registrations::Registrations,
    function_tables: unwind::Tables,
    loader_owner: Option<u32>,
    initializers: VecDeque<Initializer>,
    require_true: bool,
    pub modules: Linked,
    entry_started: bool,
    sp: u64,
    completed: Option<u64>,
    initialized: Vec<u64>,
    frames: Vec<modules::Frame>,
    pub(crate) unmapped: bool,
    rejected: bool,
    exiting: Option<termination::Pending>,
    delays: delay::Registry,
    pub(crate) search: crate::pe::search::Search,
    pub(crate) open_files: crate::file_handles::Handles,
    pub(crate) file_searches: crate::win32::file_search::Searches<String>,
    pub(crate) mappings: crate::file_sections::Mappings,
    pub(crate) standard: crate::win32::standard_handles::StandardHandles,
    pub(crate) pipes: crate::win32::pipes::Pipes,
    pub(crate) streams: crate::win32::byte_streams::Streams,
    pub(crate) input: crate::win32::byte_streams::Input,
}

pub(crate) struct Prepared {
    pub memory: Memory,
    pub state: State,
    pub services: Services,
    pub loaded: Loaded,
    pub files: Files,
}

pub(crate) struct Launch<'a> {
    pub base: u64,
    pub command: &'a str,
    pub executable: Option<&'a str>,
    pub diagnostic_imports: bool,
    pub process: current::Current,
}

fn round(value: u64, alignment: u64) -> Result<u64, String> {
    value
        .checked_add(alignment - 1)
        .map(|n| n & !(alignment - 1))
        .ok_or_else(|| "startup size overflow".into())
}

pub(crate) fn prepare(
    existing: &Memory,
    services: &Services,
    files: &Files,
    bytes: &[u8],
    launch: Launch<'_>,
) -> Result<Prepared, linker::Error> {
    let Launch {
        base: requested,
        command,
        executable,
        diagnostic_imports,
        process,
    } = launch;
    let image = Image::parse(bytes)?;
    let mode = image.mode;
    let pid = process.id();
    let first_thread = process.first_thread();
    if mode != existing.mode() {
        return Err("PE machine does not match the VM".into());
    }
    if process.mode() != mode {
        return Err("process handle ABI does not match the VM".into());
    }
    if image.is_dll || image.entry_rva == 0 || !image.executable(image.entry_rva) {
        return Err("main PE image requires an executable EXE entry".into());
    }
    if command.contains('\0') {
        return Err("command line contains NUL".into());
    }
    let (mut narrow, _) =
        crate::codepage::encode(command, 932).map_err(|_| "command line conversion failed")?;
    narrow.push(0);
    let wide: Vec<u8> = command
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    if narrow.len() > PAGE as usize || wide.len() > PAGE as usize {
        return Err("command line exceeds startup capacity".into());
    }
    let mut memory = Memory::new(mode);
    memory.set_dep_enabled(image.dep_enabled);
    let points = gateways::layout(mode);
    memory.reserve_runtime(
        points.services.start,
        points.services.end - points.services.start,
    )?;
    let providers = gateways::provider_area(mode);
    memory.reserve_runtime(providers.start, providers.end - providers.start)?;
    let delays = gateways::delay_area(mode);
    memory.reserve_runtime(delays.start, delays.end - delays.start)?;
    for page in [points.returned, points.callback, points.thread_returned]
        .map(|pc| pc & !(PAGE - 1))
        .into_iter()
        .collect::<BTreeSet<_>>()
    {
        if page < points.services.start || page >= points.services.end {
            memory.reserve_runtime(page, PAGE)?;
        }
    }
    let stack_top = if mode == Mode::X86 {
        0x6ff0_0000
    } else {
        0x7ff0_0000_0000
    };
    let reserve = round(
        if image.stack_reserve == 0 {
            1024 * 1024
        } else {
            image.stack_reserve.max(65536)
        },
        65536,
    )?;
    if reserve > 64 * 1024 * 1024 || image.stack_commit > reserve {
        return Err("PE stack reserve exceeds startup capacity".into());
    }
    let stack_base = stack_top - reserve;
    memory.map(stack_base, reserve, READ | WRITE)?;
    let (teb, peb, arena) = if mode == Mode::X86 {
        (0x7ffd_c000, 0x7ffd_e000, 0x7ffc_0000)
    } else {
        (0x7ffd_0000_0000, 0x7ffd_0000_2000, 0x7ffd_0000_4000)
    };
    memory.map(teb, 2 * PAGE, READ | WRITE)?;
    memory.map(peb, PAGE, READ | WRITE)?;
    memory.map(arena, 4 * PAGE, READ | WRITE)?;
    let mut services = services.clone();
    services.register_provider("ntdll.dll")?;
    services.register_provider("kernel32.dll")?;
    let executable = executable
        .map(str::to_owned)
        .or_else(|| {
            command_line::arguments(command)
                .into_iter()
                .next()
                .filter(|name| !name.is_empty())
        })
        .ok_or("command line has no executable identity")?;
    let mut files = files.clone();
    let executable = files
        .resolve(&executable)
        .map_err(|c| format!("invalid executable path (Windows error {c})"))?;
    files.register_executable(&executable, bytes)?;
    let profile = if mode == Mode::X86 {
        crate::environment::GuestEnvironment::pe32()
    } else {
        crate::environment::GuestEnvironment::amd64()
    };
    for directory in [
        profile.system_directory(),
        profile.home(),
        profile.local(),
        profile.roaming(),
        profile.temporary_directory(),
    ] {
        files
            .create_directory_all(directory)
            .map_err(|c| format!("invalid process namespace (Windows error {c})"))?;
    }
    let search = crate::pe::search::Search {
        executable: executable.clone(),
        directory: None,
    };
    let mut modules = linker::link(
        existing,
        &mut memory,
        &files,
        &mut services,
        bytes,
        requested,
        &search,
        diagnostic_imports,
    )?;
    let root = modules.root;
    let delays = delay::Registry::default().stage(&mut modules, &mut memory)?;
    modules.modules.get_mut(&root).unwrap().path = executable;
    let base = root;
    let ptr = mode as usize / 8;
    let command_a = arena;
    let command_w = arena + PAGE;
    let params = arena + 2 * PAGE;
    let tls_array = arena + 3 * PAGE;
    memory.write(command_a, &narrow)?;
    memory.write(command_w, &wide)?;
    memory.write_int(teb + crate::win32::context::locale_offset(mode), 4, 0x411)?;
    if mode == Mode::X86 {
        for (at, value) in [
            (teb, 0xffffffff),
            (teb + 4, stack_top),
            (teb + 8, stack_base),
            (teb + 0x18, teb),
            (teb + 0x20, u64::from(pid)),
            (teb + 0x24, u64::from(first_thread)),
            (teb + 0x2c, tls_array),
            (teb + 0x30, peb),
            (peb + 8, base),
            (peb + 0x10, params),
            (params + 0x44, command_w),
        ] {
            memory.write_int(at, 4, value)?;
        }
        memory.write_int(params + 0x40, 2, (wide.len() - 2) as u64)?;
        memory.write_int(params + 0x42, 2, wide.len() as u64)?;
    } else {
        for (at, value) in [
            (teb + 8, stack_top),
            (teb + 16, stack_base),
            (teb + 0x30, teb),
            (teb + 0x40, u64::from(pid)),
            (teb + 0x48, u64::from(first_thread)),
            (teb + 0x58, tls_array),
            (teb + 0x60, peb),
            (peb + 0x10, base),
            (peb + 0x20, params),
            (params + 0x78, command_w),
        ] {
            memory.write_int(at, 8, value)?;
        }
        memory.write_int(params + 0x70, 2, (wide.len() - 2) as u64)?;
        memory.write_int(params + 0x72, 2, wide.len() as u64)?;
    }
    let mut initializers = VecDeque::new();
    let mut tls_block = 0;
    let mut next_tls = 0usize;
    let initialized = Vec::new();
    for &module_base in &modules.order {
        let module = modules.modules.get_mut(&module_base).unwrap();
        module.tls = prepare_tls(&mut memory, existing, module, next_tls, first_thread)?;
        if let Some(tls) = &module.tls {
            memory.write_int(
                tls_array + next_tls as u64 * ptr as u64,
                ptr,
                tls.blocks[&first_thread],
            )?;
            next_tls += 1;
            if module_base == root {
                tls_block = tls.blocks[&first_thread];
            }
        }
        let queue = notifications(module, 1, u64::from(module_base != root));
        module.phase = Phase::Initializing;
        initializers.extend(queue);
    }
    let heap = allocate_process_data(&mut memory, existing, PAGE, &[])?;
    memory.write_int(peb + if mode == Mode::X86 { 0x18 } else { 0x30 }, ptr, heap)?;
    let environment = crate::win32::environment::Environment::new(&mut memory, existing, params)?;
    let sp = stack_top - if mode == Mode::X86 { 16 } else { 40 };
    let entry = base + u64::from(image.entry_rva);
    services.initialize_crt_data(
        &mut memory,
        [command_a, command_w],
        command_line::arguments(command).len() as u32,
        profile.version(),
    );
    let mut loaded = Loaded {
        graphics: Default::default(),
        wmi: Default::default(),
        gdiplus: Default::default(),
        d3d9: Default::default(),
        audio: Default::default(),
        crt: crate::win32::crt::Crt::new(command),
        diagnostic_imports,
        children: VecDeque::new(),
        startup_info: [[0; 104]; 2],
        disk_detached: false,
        info: [
            base,
            image.mapped_size(),
            entry,
            sp,
            teb,
            peb,
            command_a,
            command_w,
            tls_block,
            tls_array,
        ],
        threads: threads::Threads::new(mode, teb, tls_array, reserve, first_thread),
        process,
        snapshots: crate::win32::toolhelp::Snapshots::new(crate::file_handles::HANDLE_END, pid),
        objects: crate::win32::objects::new(),
        heaps: crate::win32::heaps::Heaps::new(heap),
        locals: Default::default(),
        temp_unique: 0,
        srw: Default::default(),
        environment,
        user: crate::win32::user::User::new(mode),
        registry: crate::win32::registry::new(mode),
        crypto: crate::win32::crypto::Crypto::new(mode),
        tokens: crate::win32::tokens::new(mode),
        rng: crate::win32::rng_state::Providers::new(if mode == Mode::X86 {
            0xa0020000
        } else {
            0x7ffd05000000
        }),
        exception_registrations: Default::default(),
        etw: Default::default(),
        data_pages: Default::default(),
        resources: crate::win32::resources::Views::new(mode),
        hstrings: Default::default(),
        bstrings32: Default::default(),
        bstrings64: Default::default(),
        safe_arrays: Default::default(),
        security: Default::default(),
        winsock: Default::default(),
        periods: Default::default(),
        function_tables: Default::default(),
        loader_owner: Some(first_thread),
        initializers,
        require_true: false,
        modules,
        entry_started: false,
        sp,
        completed: None,
        initialized,
        frames: Vec::new(),
        unmapped: false,
        rejected: false,
        exiting: None,
        delays,
        search,
        open_files: crate::file_handles::Handles::for_files(&files),
        mappings: Default::default(),
        standard: Default::default(),
        pipes: Default::default(),
        streams: Default::default(),
        input: Default::default(),
        file_searches: crate::win32::file_search::Searches::with_range(
            0x80000000,
            0x90000000,
            64 * 1024 * 1024,
        ),
    };
    services.own_crt_data(&mut loaded.data_pages);
    for info in &mut loaded.startup_info {
        info[..4].copy_from_slice(&(if mode == Mode::X86 { 68u32 } else { 104 }).to_le_bytes());
    }
    let std_offset = if mode == Mode::X86 { 0x18 } else { 0x20 };
    for n in 0..3 {
        memory.write_int(params + std_offset + n * ptr as u64, ptr, 0x100 + n)?;
    }
    let mut state = State::new(mode, entry, sp);
    if mode == Mode::X86 {
        state.fs_base = teb;
    } else {
        state.gs_base = teb;
    }
    loaded
        .begin(&mut state, &mut memory, &mut services, &files)
        .map_err(|error| match error {
            StartupError::FileInput(_) => "file input is required".into(),
            StartupError::Fault(fault) => fault.to_string(),
            StartupError::Rejected => "DLL process attach returned FALSE".into(),
            StartupError::Unhandled(code) => format!("unhandled Windows exception {code:#x}"),
            StartupError::Invalid(message) => message,
        })?;
    loaded.refresh_loader();
    Ok(Prepared {
        memory,
        state,
        services,
        loaded,
        files,
    })
}

impl Loaded {
    pub fn startup_pending(&self, state: &State, mode: Mode) -> bool {
        if self.process.exit.is_some() {
            return false;
        }
        self.loader_available()
            && (self.process_exit_pending(state, mode)
                || (self.rejected && self.frames.is_empty())
                || ((!self.entry_started || !self.frames.is_empty())
                    && state.ip == gateways::layout(mode).callback))
    }

    pub(crate) fn begin(
        &mut self,
        state: &mut State,
        memory: &mut Memory,
        services: &mut Services,
        files: &Files,
    ) -> Result<(), StartupError> {
        if self.process_exit_pending(state, memory.mode()) {
            return self.advance_process_exit(state, memory);
        }
        if !self.frames.is_empty() {
            return self.advance_dynamic(state, memory, services, files);
        }
        if self.rejected {
            return Err(StartupError::Rejected);
        }
        if self.require_true && state.regs[0] as u32 == 0 {
            self.reject_static(state);
            return self.advance_dynamic(state, memory, services, files);
        }
        if let Some(base) = self.completed.take() {
            self.modules.modules.get_mut(&base).unwrap().phase = Phase::Ready;
            self.initialized.push(base);
        }
        let mode = memory.mode();
        let points = gateways::layout(mode);
        let callback = self.initializers.front();
        let target = callback.map_or(self.info[2], |call| call.target);
        let returned = if callback.is_some() {
            points.callback
        } else {
            points.returned
        };
        let mut frame = Vec::with_capacity(16);
        let width = mode as usize / 8;
        frame.extend_from_slice(&returned.to_le_bytes()[..width]);
        if let Some(callback) = callback.filter(|_| mode == Mode::X86) {
            for value in [callback.base, callback.reason, callback.reserved] {
                frame.extend_from_slice(&(value as u32).to_le_bytes());
            }
        }
        memory.write(self.sp, &frame)?;
        state.cancel_repeat();
        state.exception = 0;
        state.ip = target;
        state.regs[4] = self.sp;
        if let Some(callback) = callback {
            if mode == Mode::X64 {
                state.regs[1] = callback.base;
                state.regs[2] = callback.reason;
                state.regs[8] = callback.reserved;
            }
            self.require_true = callback.require_true;
            self.completed = callback.finish.then_some(callback.base);
            self.initializers.pop_front();
        } else {
            self.require_true = false;
            self.entry_started = true;
            self.loader_owner = None;
            for module in self
                .modules
                .modules
                .values_mut()
                .filter(|m| m.phase == Phase::Initializing)
            {
                module.phase = Phase::Ready;
                self.initialized.push(module.base);
            }
        }
        Ok(())
    }
}

fn notifications(module: &Module, reason: u64, reserved: u64) -> VecDeque<Initializer> {
    let mut queue = VecDeque::new();
    if matches!(reason, 2 | 3) && module.thread_calls_disabled {
        return queue;
    }
    if let Some(tls) = &module.tls {
        queue.extend(tls.callbacks.iter().map(|&target| Initializer {
            target,
            base: module.base,
            reserved: 0,
            reason,
            finish: false,
            require_true: false,
        }));
    }
    if module.image.is_dll && module.image.entry_rva != 0 {
        queue.push_back(Initializer {
            target: module.base + u64::from(module.image.entry_rva),
            base: module.base,
            reserved,
            reason,
            finish: false,
            require_true: reason == 1,
        });
    }
    if reason == 1 {
        if let Some(last) = queue.back_mut() {
            last.finish = true;
        }
    }
    queue
}

fn prepare_tls(
    memory: &mut Memory,
    existing: &Memory,
    module: &Module,
    slot: usize,
    thread: u32,
) -> Result<Option<ModuleTls>, String> {
    let Some(tls) = module.image.tls(memory, module.base)? else {
        return Ok(None);
    };
    let size = round(
        (tls.template.len() as u64 + u64::from(tls.zero_fill)).max(1),
        PAGE,
    )?;
    let block = allocate_process_data(memory, existing, size, &tls.template)?;
    memory.write_int(tls.index, 4, slot as u64)?;
    Ok(Some(ModuleTls {
        slot,
        blocks: std::collections::BTreeMap::from([(thread, block)]),
        template: tls.template,
        size,
        callbacks: tls.callbacks,
    }))
}

pub(crate) fn allocate_process_data(
    memory: &mut Memory,
    existing: &Memory,
    size: u64,
    template: &[u8],
) -> Result<u64, String> {
    allocate_process_data_avoiding(memory, existing, size, template, &[])
}
pub(crate) fn allocate_process_data_avoiding(
    memory: &mut Memory,
    existing: &Memory,
    size: u64,
    template: &[u8],
    blocked: &[(u64, u64)],
) -> Result<u64, String> {
    let mode = memory.mode();
    let mut block = if mode == Mode::X86 {
        0x5000_0000
    } else {
        0x0005_0000_0000
    };
    while !memory.available(block, size)
        || !existing.available(block, size)
        || blocked
            .iter()
            .any(|&(base, length)| block < base + length && block + size > base)
    {
        block = block
            .checked_add(round(size, 65536)?)
            .filter(|&at| {
                at + size
                    < if mode == Mode::X86 {
                        0x6f000000
                    } else {
                        1 << 47
                    }
            })
            .ok_or("process data address space exhausted")?;
    }
    memory.map(block, size, READ | WRITE)?;
    memory.write(block, template)?;
    Ok(block)
}
