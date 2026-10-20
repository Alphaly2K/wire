//! Checked Windows x64 startup services. No host pointers enter the guest.
//! This is a bring-up layer, not a complete Windows/Unity runtime.
use super::{
    cpu::Cpu64,
    memory::{Memory, PAGE, READ, WRITE},
};
use crate::win32::catalog::bstr_symbol;
use std::{
    collections::BTreeMap,
    time::{Instant, UNIX_EPOCH},
};
mod allocation;
mod apc;
mod automation;
mod bcrypt;
mod com;
mod command;
mod devices;
mod display;
mod dxgi;
mod environment;
mod events;
pub(crate) mod exception;
mod files;
mod find;
mod gateways;
mod guid;
mod heap;
mod input_profile;
mod keyboard;
mod local_memory;
mod locals;
mod mapping;
mod modules;
mod pipes;
mod process_options;
mod process_queries;
mod registry;
mod resources;
mod shell;
mod snapshot;
mod sockets;
mod sspi;
mod sync_objects;
mod threads;
mod timers;
mod token;
mod trace;
mod tracing;
pub use trace::{Trace, TraceRecord};
mod unwind;
mod user;
pub use gateways::Gateways;
pub use user::WindowCall;
pub(crate) enum WindowStep {
    Next(u64, [u64; 4]),
    Returned(u64),
}
mod winrt;

pub const TEB: u64 = 0x7ffd00000000;
pub const PEB: u64 = TEB + 2 * PAGE;
const PARAMS: u64 = TEB + 3 * PAGE;
const COMMAND_A: u64 = TEB + 4 * PAGE;
const COMMAND_W: u64 = TEB + 5 * PAGE;
const PROCESS_HEAP: u64 = TEB + 6 * PAGE;
const PID: u64 = 1;
const TID: u64 = 1;
#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub handle: u64,
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub visible: bool,
    pub topmost: bool,
}
#[derive(Debug, Clone, Copy)]
pub struct WindowEvent {
    pub window: u64,
    pub message: u32,
    pub wparam: u64,
    pub lparam: u64,
}

pub struct Runtime64 {
    api_inventory: Option<BTreeMap<(String, String, String), u64>>,
    start: Instant,
    timer_periods: crate::win32::kernel::multimedia::PeriodRequests,
    image_base: u64,
    next_alloc: u64,
    allocations: BTreeMap<u64, (usize, u64)>,
    guest_heap: heap::Heap,
    locals: crate::win32::kernel::locals64::LocalSlots64,
    modules: crate::win32::x64::modules::Modules,
    tokens: token::Tokens,
    etw: tracing::Providers,
    function_tables: unwind::FunctionTables,
    exception_registrations: exception::Registrations,
    profile: crate::runtime::environment::GuestEnvironment,
    environment: environment::Environment,
    local_memory: local_memory::LocalMemory,
    task_memory: com::TaskMemory,
    bstrings: automation::Bstrings,
    hstrings: winrt::Strings,
    devices: devices::Devices,
    rng: bcrypt::RngProviders,
    exe_path: String,
    pub gateways: Gateways,
    static_tls_count: u32,
    static_tls: Vec<(Vec<u8>, u64, u64)>,
    threads: threads::Threads,
    delay_hooks: BTreeMap<u64, (String, u64, u64)>,
    srw: crate::win32::kernel::srw::SrwLocks,
    process_options: process_options::ProcessOptions,
    snapshots: snapshot::Snapshots,
    pub api_calls: u64,
    /// Most recent calls, bounded independently of the full api_calls counter.
    pub trace: Trace,
    events: events::Events,
    pipes: pipes::Pipes,
    files: files::Files,
    mappings: mapping::Mappings,
    winsock: sockets::Winsock,
    security: sspi::Security,
    graphics: dxgi::Graphics,
    registry: registry::Registry,
    user: user::User,
    searches: find::Searches,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Control {
    Returned,
    Yielded,
    SwitchThread(String),
    Blocked,
    ThreadExited(u32),
    ThreadContext {
        dll: String,
        args: Vec<u64>,
        thread: u64,
        flags: u32,
    },
    Callbacks {
        calls: Vec<(u64, [u64; 4])>,
        release: Vec<u64>,
        stop_on_false: bool,
        return_boolean: bool,
        window_call: Option<WindowCall>,
    },
    LoadModule {
        path: String,
        search: (u64, Option<String>),
        dll: String,
        function: String,
        args: Vec<u64>,
    },
    FunctionLookup {
        dll: String,
        target: u64,
        context: u64,
        base: u64,
        args: Vec<u64>,
    },
    Present {
        dll: String,
        function: String,
        args: Vec<u64>,
    },
    Exited(u32),
    Raise {
        dll: String,
        args: Vec<u64>,
        caller: Box<Cpu64>,
    },
    Unwind {
        dll: String,
        args: Vec<u64>,
    },
}
/// Whether a Win64 import has a dispatch route and known ABI. This does not
/// certify every argument, COM method or optional backend capability.
pub fn import_supported(dll: &str, name: &str) -> bool {
    use crate::win32::catalog::{resolve, Architecture, Resolution};
    let name = normalize_symbol(dll, name);
    match resolve(Architecture::X64, dll, name) {
        Resolution::Bound(contract) => {
            return contract.execution(Architecture::X64)
                == crate::win32::catalog::ExecutionPolicy::Typed
        }
        Resolution::WrongDll(_) => return false,
        Resolution::Legacy => {}
    }
    if import_arity(name).is_none() {
        return false;
    }
    if !dll.eq_ignore_ascii_case("kernel32.dll")
        && !dll.eq_ignore_ascii_case("kernelbase.dll")
        && !(dll == dxgi::DLL && dxgi::bind_method(name).is_some())
    {
        return false;
    }
    true
}
fn kernel_export(name: &str) -> bool {
    crate::win32::catalog::by_name(name).is_some_and(|contract| {
        contract
            .lookup_dlls(crate::win32::catalog::Architecture::X64)
            .contains(&"kernel32.dll")
    })
}
fn import_arity(name: &str) -> Option<usize> {
    if let Some(contract) = crate::win32::catalog::by_name(name) {
        return Some(contract.parameters.len());
    }
    dxgi::bind_method(name).map(|method| method.arity())
}

fn normalize_symbol<'a>(dll: &str, name: &'a str) -> &'a str {
    bstr_symbol(
        dll,
        crate::win32::catalog::graphics_symbol(
            dll,
            crate::win32::catalog::winsock_symbol(dll, name),
        ),
    )
}

impl Runtime64 {
    pub fn new(
        mem: &mut Memory,
        image_base: u64,
        stack: u64,
        stack_size: u64,
        command: &str,
    ) -> Result<Self, String> {
        let ansi = command.as_bytes();
        let wide: Vec<u8> = command
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        if ansi.len() >= PAGE as usize || wide.len() > PAGE as usize {
            return Err("AMD64 command line exceeds startup buffer".into());
        }
        mem.map(TEB, 8 * PAGE, READ | WRITE)?;
        // Opaque provider handles and API gateway/sentinel addresses must not be
        // reused by guest VirtualAlloc even though they have no executable backing.
        mem.reserve_runtime(TEB + 8 * PAGE, 8 * 1024 * 1024)?;
        mem.reserve_runtime(0x7ffe00000000, 1024 * 1024)?;
        mem.write_int(
            TEB + 8,
            8,
            stack.checked_add(stack_size).ok_or("stack overflow")?,
        )?;
        mem.write_int(TEB + 16, 8, stack)?;
        mem.write_int(TEB + 0x30, 8, TEB)?;
        mem.write_int(TEB + 0x40, 8, PID)?;
        mem.write_int(TEB + 0x48, 8, TID)?;
        mem.write_int(TEB + 0x60, 8, PEB)?;
        mem.write_int(TEB + 0x58, 8, TEB + 7 * PAGE)?;
        mem.write_int(PEB + 0x10, 8, image_base)?;
        mem.write_int(PEB + 0x20, 8, PARAMS)?;
        mem.write_int(PEB + 0x30, 8, PROCESS_HEAP)?;
        // RTL_USER_PROCESS_PARAMETERS.CommandLine: UNICODE_STRING, x64 layout.
        mem.write_int(PARAMS + 0x70, 2, (wide.len() - 2) as u64)?;
        mem.write_int(PARAMS + 0x72, 2, wide.len() as u64)?;
        mem.write_int(PARAMS + 0x78, 8, COMMAND_W)?;
        mem.write(COMMAND_A, ansi)?;
        mem.write(COMMAND_W, &wide)?;
        Ok(Self {
            api_inventory: None,
            start: Instant::now(),
            timer_periods: Default::default(),
            image_base,
            next_alloc: 0x400000000,
            allocations: BTreeMap::new(),
            guest_heap: Default::default(),
            bstrings: Default::default(),
            locals: Default::default(),
            modules: crate::win32::x64::modules::Modules::new(TEB + 0x20000),
            tokens: token::Tokens::default(),
            etw: tracing::Providers::default(),
            function_tables: unwind::FunctionTables::default(),
            exception_registrations: exception::Registrations::default(),
            profile: crate::runtime::environment::GuestEnvironment::amd64(),
            environment: Default::default(),
            local_memory: local_memory::LocalMemory::default(),
            task_memory: Default::default(),
            hstrings: Default::default(),
            devices: Default::default(),
            rng: bcrypt::RngProviders::default(),
            exe_path: command.trim_matches('"').to_owned(),
            gateways: BTreeMap::new().into(),
            static_tls_count: 0,
            static_tls: Vec::new(),
            threads: threads::Threads::new(stack_size),
            delay_hooks: BTreeMap::new(),
            srw: Default::default(),
            process_options: process_options::ProcessOptions::default(),
            snapshots: snapshot::Snapshots::default(),
            api_calls: 0,
            trace: Trace::default(),
            events: events::Events::default(),
            pipes: pipes::Pipes::default(),
            files: files::Files::default(),
            mappings: mapping::Mappings::default(),
            winsock: sockets::Winsock::default(),
            security: sspi::Security::default(),
            graphics: dxgi::Graphics::default(),
            registry: registry::Registry::default(),
            user: user::User::default(),
            searches: find::Searches::default(),
        })
    }
    fn set_error(&self, mem: &mut Memory, error: u32) -> Result<(), String> {
        self.threads.set_error(mem, error)
    }
    fn remove_device_notifications(&mut self, window: u64) {
        self.devices.remove_notifications(window);
    }
    fn device_call(
        &mut self,
        mem: &mut Memory,
        id: crate::win32::catalog::DeviceApi,
        args: &[u64],
    ) -> Result<u64, String> {
        use crate::win32::catalog::DeviceApi;
        let window_exists = matches!(id, DeviceApi::RegisterA | DeviceApi::RegisterW)
            && self.user.windows.contains_key(&args[0]);
        let last_error = self.teb() + 0x68;
        self.devices.call(mem, id, args, last_error, window_exists)
    }
    pub fn set_filesystem(&mut self, fs: crate::fs::GuestFs) {
        self.files.set_filesystem(fs);
    }
    pub(crate) fn prepare_user_profile(&self) -> Result<(), String> {
        let fs = self
            .files
            .filesystem()
            .ok_or("Win64 user profile requires a filesystem")?;
        for path in self.profile.user_directories() {
            let host = fs
                .resolve(path)
                .map_err(|e| format!("Win64 user profile path error {e}"))?;
            std::fs::create_dir_all(host).map_err(|e| format!("Win64 user profile: {e}"))?;
        }
        Ok(())
    }
    fn utf16(mem: &Memory, ptr: u64) -> Result<String, String> {
        crate::win32::x64::text::utf16(mem, ptr)
    }
    fn ascii(mem: &Memory, ptr: u64) -> Result<String, String> {
        crate::win32::x64::text::ascii(mem, ptr)
    }
    fn path_name(mem: &Memory, pointer: u64, wide: bool) -> Result<String, String> {
        crate::win32::x64::text::path_name(mem, pointer, wide)
    }
    fn native_module(&mut self, name: &str, load: bool) -> Result<u64, String> {
        self.modules
            .native_module(self.files.filesystem(), name, load)
    }
    pub fn install_gateways(&mut self, gateways: BTreeMap<u64, String>) {
        self.gateways = gateways.into();
        // Kernel32 is an implicit, persistent process module.
        self.native_module("kernel32.dll", true)
            .expect("kernel32 provider");
        self.register_provider("ntdll.dll");
        let ntdll = self
            .native_module("ntdll.dll", false)
            .expect("ntdll provider");
        self.modules.pin(ntdll);
    }
    pub fn initialize_tls(
        &mut self,
        mem: &mut Memory,
        tls: &super::pe::Tls64,
    ) -> Result<(), String> {
        if self.static_tls_count >= 64 {
            return Err("AMD64 static TLS module capacity exceeded".into());
        }
        let size = tls.template.len() as u64 + tls.zero_fill as u64;
        mem.check(tls.index_address, 4, WRITE)?;
        let threads: Vec<_> = self
            .threads
            .entries
            .iter()
            .filter(|(_, thread)| thread.exit.is_none())
            .map(|(&id, thread)| (id, thread.teb))
            .collect();
        let mut slots = Vec::new();
        for &(id, teb) in &threads {
            let vector = mem.read_int(teb + 0x58, 8)?;
            let slot = vector + self.static_tls_count as u64 * 8;
            mem.check(slot, 8, WRITE)?;
            slots.push((id, slot));
        }
        if tls.alignment == 0 || !tls.alignment.is_power_of_two() || tls.alignment > 8192 {
            return Err("invalid AMD64 static TLS alignment".into());
        }
        let mut blocks = Vec::new();
        let prepared = (|| -> Result<(), String> {
            for _ in &threads {
                self.next_alloc = self.next_alloc.div_ceil(tls.alignment) * tls.alignment;
                let ptr = self.heap_alloc(mem, size)?;
                if ptr == 0 {
                    return Err("AMD64 static TLS allocation failed".into());
                }
                blocks.push(ptr);
                mem.write(ptr, &tls.template)?;
            }
            Ok(())
        })();
        if let Err(error) = prepared {
            self.release_callback_buffers(mem, &blocks)?;
            return Err(error);
        }
        mem.write_int(tls.index_address, 4, self.static_tls_count as u64)?;
        for ((id, slot), ptr) in slots.into_iter().zip(blocks) {
            mem.write_int(slot, 8, ptr)?;
            if let Some((_, _, blocks)) = &mut self.threads.entries.get_mut(&id).unwrap().resources
            {
                blocks.push(ptr);
            }
        }
        self.static_tls_count += 1;
        self.static_tls
            .push((tls.template.clone(), size, tls.alignment));
        Ok(())
    }
    pub fn install_delays(
        &mut self,
        mem: &mut Memory,
        base: u64,
        delays: &[super::pe::Delay64],
    ) -> Result<(), String> {
        for delay in delays {
            mem.check(base + delay.module_handle_rva as u64, 8, WRITE)?;
            for f in &delay.functions {
                mem.check(base + f.iat_rva as u64, 8, WRITE)?;
            }
        }
        for delay in delays {
            for f in &delay.functions {
                let address = 0x7ffe00000000 + self.gateways.len() as u64 * 16;
                let name = f
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("#{}", f.ordinal.unwrap_or(0)));
                self.gateways
                    .insert(address, format!("{}!{name}", delay.dll));
                let handle = base + delay.module_handle_rva as u64;
                let iat = base + f.iat_rva as u64;
                self.delay_hooks
                    .insert(address, (delay.dll.clone(), handle, iat));
                mem.write_int(iat, 8, address)?;
            }
        }
        Ok(())
    }
    /// Opt-in aggregate diagnostics. Disabled on normal and performance runs.
    pub fn enable_api_inventory(&mut self) {
        self.api_inventory = Some(BTreeMap::new());
    }
    fn inventory_record(&mut self, kind: &str, dll: &str, name: &str) {
        if let Some(map) = &mut self.api_inventory {
            *map.entry((kind.into(), dll.to_ascii_lowercase(), name.into()))
                .or_default() += 1;
        }
    }
    pub(crate) fn inventory_failure(&mut self, dll: &str, name: &str) {
        self.inventory_record("failed", dll, name);
    }
    pub fn api_inventory_tsv(&self) -> Option<String> {
        let escape = |s: &str| {
            s.replace('\\', "\\\\")
                .replace('\t', "\\t")
                .replace('\r', "\\r")
                .replace('\n', "\\n")
        };
        let mut output = String::from("kind\tdll\tname\tcount\n");
        for ((kind, dll, name), count) in self.api_inventory.as_ref()? {
            output.push_str(&format!(
                "{}\t{}\t{}\t{count}\n",
                escape(kind),
                escape(dll),
                escape(name)
            ));
        }
        Some(output)
    }
    pub fn register_module(&mut self, name: &str, base: u64, exports: super::pe::Exports64) {
        self.modules.register_guest(name, base, exports);
    }
    pub(crate) fn register_module_path(&mut self, base: u64, path: String) {
        self.modules.register_path(base, path);
    }
    pub(crate) fn register_image(&mut self, base: u64, size: u32, entry: u64) {
        self.modules.register_image(base, size, entry);
    }
    pub(crate) fn filesystem(&self) -> Result<crate::fs::GuestFs, String> {
        self.files
            .filesystem()
            .cloned()
            .ok_or("Win64 module load requires a filesystem".into())
    }
    pub(crate) fn complete_call(&mut self, dll: &str, function: &str, args: &[u64], base: u64) {
        self.record(dll, function, args, base);
    }
    pub fn set_main_base(&mut self, mem: &mut Memory, base: u64) -> Result<(), String> {
        mem.write_int(PEB + 0x10, 8, base)?;
        self.image_base = base;
        Ok(())
    }
    pub fn register_provider(&mut self, name: &str) {
        self.modules.register_provider(name);
    }
    pub fn register(&mut self, dll: &str, name: &str) -> u64 {
        self.gateways.register(dll, name)
    }
    fn heap_alloc(&mut self, mem: &mut Memory, size: u64) -> Result<u64, String> {
        let last_error = self.teb() + 0x68;
        allocation::Pages {
            next: &mut self.next_alloc,
            allocations: &mut self.allocations,
            last_error,
            trace: &mut self.trace,
        }
        .allocate(mem, size)
    }
    /// Validate the return frame first, then dispatch the exact Win64 arity.
    /// A bad return frame or checked output fault must not commit CPU/API state.
    fn catalog_call(
        &mut self,
        mem: &mut Memory,
        id: crate::win32::catalog::ApiId,
        args: &[u64],
    ) -> Result<u64, String> {
        use crate::win32::catalog::ApiId;
        match id {
            ApiId::WindowsPrintf(_) => unreachable!("Windows formatting has no Win64 binding"),
            ApiId::GraphicsExport(id) => self.graphics_export_call(mem, id, args),
            ApiId::ProcessControl(_) => {
                unreachable!("process controls use their bound gateway route")
            }
            ApiId::MessageFormat(id) => {
                let last_error = self.teb() + 0x68;
                let mut pages = allocation::Pages {
                    next: &mut self.next_alloc,
                    allocations: &mut self.allocations,
                    last_error,
                    trace: &mut self.trace,
                };
                self.local_memory.format_message(mem, id, args, &mut pages)
            }
            ApiId::Shell(id) => self.shell_call(mem, id, args),
            ApiId::Exception(_) => unreachable!("exception controls use their bound gateway route"),
            ApiId::Com(id) => {
                use crate::win32::catalog::ComApi;
                if matches!(id, ComApi::TaskAlloc | ComApi::TaskFree) {
                    let last_error = self.teb() + 0x68;
                    let mut pages = allocation::Pages {
                        next: &mut self.next_alloc,
                        allocations: &mut self.allocations,
                        last_error,
                        trace: &mut self.trace,
                    };
                    self.task_memory.call(mem, id, args, &mut pages)
                } else {
                    com::call(mem, id, args, self.threads.apartment_mut(), &mut self.trace)
                }
            }
            ApiId::Winrt(id) => {
                let last_error = self.teb() + 0x68;
                let mut pages = allocation::Pages {
                    next: &mut self.next_alloc,
                    allocations: &mut self.allocations,
                    last_error,
                    trace: &mut self.trace,
                };
                self.hstrings.call(mem, id, args, &mut pages)
            }
            ApiId::Bstr(id) => {
                let last_error = self.teb() + 0x68;
                let mut pages = allocation::Pages {
                    next: &mut self.next_alloc,
                    allocations: &mut self.allocations,
                    last_error,
                    trace: &mut self.trace,
                };
                self.bstrings.call(mem, id, args, &mut pages)
            }
            ApiId::Variant(_) => unreachable!("Variant helpers have no Win64 binding"),
            ApiId::VersionResource(_) => {
                unreachable!("file version resources have no Win64 binding")
            }
            ApiId::Audio(_) | ApiId::FontResource(_) => {
                unreachable!("legacy audio/font endpoints have no Win64 binding")
            }
            ApiId::ProcessQuery(id) => process_queries::call(mem, id, args, &self.threads),
            ApiId::Environment(id) => self.environment_call(mem, id, args),
            ApiId::Startup(id) => {
                crate::win32::x64::process_startup::call(id, args, mem, [COMMAND_A, COMMAND_W])
            }
            ApiId::Snapshot(id) => {
                if id == crate::win32::catalog::SnapshotApi::Create {
                    let threads = self
                        .threads
                        .entries
                        .iter()
                        .filter(|(_, t)| t.exit.is_none())
                        .map(|(&id, t)| (id as u32, t.priority));
                    match self.snapshots.create(args[0], &self.exe_path, threads) {
                        Ok(handle) => Ok(handle),
                        Err(error) => {
                            self.set_error(mem, error)?;
                            Ok(u64::MAX)
                        }
                    }
                } else {
                    let last_error = self.teb() + 0x68;
                    self.snapshots.enumerate(mem, id, args, last_error)
                }
            }
            ApiId::Token(id) => {
                use crate::win32::catalog::TokenApi;
                let last_error = self.teb() + 0x68;
                match id {
                    TokenApi::OpenProcess => self.tokens.open(mem, args, last_error),
                    TokenApi::Information => self.tokens.query(mem, args, last_error),
                    TokenApi::OpenThread => {
                        self.set_error(
                            mem,
                            if self.threads.id(args[0]).is_ok() {
                                1008
                            } else {
                                6
                            },
                        )?;
                        Ok(0)
                    }
                }
            }
            ApiId::Sid(id) => {
                let last_error = self.teb() + 0x68;
                crate::win32::x64::sid::call(mem, id, args, last_error)
            }
            ApiId::Etw(id) => self.etw.call(mem, id, args),
            ApiId::Registry(id) => self.registry.call(mem, id, args),
            ApiId::Bcrypt(id) => self.rng.call(mem, id, args),
            ApiId::Sspi(id) => {
                self.security
                    .call(mem, id, args, &mut self.next_alloc, &mut self.gateways)
            }
            ApiId::WinHttp(id) => {
                let last_error = self.teb() + 0x68;
                crate::win32::x64::winhttp::call(mem, id, args, last_error)
            }
            ApiId::Device(id) => self.device_call(mem, id, args),
            ApiId::Winsock(id) => {
                let last_error = self.teb() + 0x68;
                self.winsock
                    .call(mem, id, args, last_error, &mut self.events)
            }
            ApiId::ProcessOption(id) => {
                let last_error = self.teb() + 0x68;
                self.process_options
                    .call(mem, id, args, last_error, &self.pipes, &self.events)
            }
            ApiId::SystemPath(id) => {
                crate::win32::x64::system_paths::call(mem, id, args, &self.profile)
            }
            ApiId::Keyboard(id) => self.user.keyboard_call(mem, id, args, &self.threads),
            ApiId::Cursor(id) => {
                let last_error = self.teb() + 0x68;
                self.user
                    .cursor_call(mem, id, args, self.threads.active, last_error)
            }
            ApiId::Image(id) => {
                let last_error = self.teb() + 0x68;
                self.user
                    .image_call(mem, id, args, &self.modules, last_error)
            }
            ApiId::Display(id) => self.display_call(mem, id, args),
            ApiId::GdiDc(id) => {
                let last_error = self.teb() + 0x68;
                self.user
                    .gdi_dc_call(mem, id, args, self.threads.active, last_error)
            }
            ApiId::Desktop(id) => {
                let last_error = self.teb() + 0x68;
                self.user.desktop_call(mem, id, args, last_error)
            }
            ApiId::Class(id) => {
                let last_error = self.teb() + 0x68;
                self.user.class_call(mem, id, args, last_error)
            }
            ApiId::WindowQuery(id) => {
                let last_error = self.teb() + 0x68;
                self.user
                    .window_query_call(mem, id, args, last_error, &self.gateways)
            }
            ApiId::WindowState(id) => {
                let last_error = self.teb() + 0x68;
                self.user.window_state_call(mem, id, args, last_error)
            }
            ApiId::WindowControl(id) => self.user.window_value_call(mem, id, args, &self.threads),
            ApiId::Message(_) => unreachable!("message calls use explicit process control"),
            ApiId::Dialog(_) => unreachable!("dialog exports remain absent on Win64"),
            ApiId::Painting(_) => unreachable!("painting exports remain absent on Win64"),
            ApiId::Resource(_) => unreachable!("resource exports remain absent on Win64"),
            ApiId::Menu(_) => unreachable!("menu exports remain absent on Win64"),
            ApiId::GdiOperation(_) => {
                unreachable!("GDI object/raster exports remain absent on Win64")
            }
            ApiId::UserObject(_) => unreachable!("USER names/properties have no Win64 binding"),
            ApiId::RawInput(_) | ApiId::Ime(_) => self.input_profile_call(mem, id, args),
            ApiId::Geometry(id) => {
                let result = crate::win32::x64::geometry::call(mem, id, args)?;
                if let Some(error) = result.error {
                    self.set_error(mem, error)?;
                }
                Ok(result.value)
            }
            ApiId::PointerHelper(id) => Ok(crate::win32::x64::pointer_helpers::call(id, args)),
            ApiId::BasicString(_) => {
                unreachable!("PE32 basic helpers have no Win64 binding")
            }
            ApiId::File(_) => unreachable!("core file calls are handled by the bound gateway"),
            ApiId::Handle(_) => unreachable!("handle calls are handled by the bound gateway"),
            ApiId::Path(_) => unreachable!("path helpers have no Win64 binding"),
            ApiId::Mutex(_) => unreachable!("mutexes have no Win64 binding"),
            ApiId::Wait(_) => unreachable!("waits use the bound process control route"),
            ApiId::Delay(_) => unreachable!("delays use the bound process control route"),
            ApiId::InlineSync(_) => {
                unreachable!("inline sync uses the bound process control route")
            }
            ApiId::CriticalSection(_) => {
                unreachable!("critical sections use the bound process control route")
            }
            ApiId::AddressWait(_) => {
                unreachable!("address waits use the bound process control route")
            }
            ApiId::ThreadControl(_) => {
                unreachable!("thread control uses the bound process control route")
            }
            ApiId::Guid(id) => self.guid_call(mem, id, args),
            ApiId::Identity(id) => {
                crate::win32::x64::identity::call(mem, id, args, &self.profile, self.teb() + 0x68)
            }
            ApiId::Version(id) => {
                crate::win32::x64::version::call(mem, id, args, &self.profile, self.teb() + 0x68)
            }
            ApiId::Nls(id) => {
                crate::win32::x64::nls::call(mem, id, args, &self.profile, self.teb() + 0x68)
            }
            ApiId::Module(id) => self.scalar_module_call(mem, id, args),
            ApiId::ModuleQuery(id) => self.module_query(mem, id, args),
            ApiId::Interlocked(id) => crate::win32::x64::interlocked::call(mem, id, args),
            ApiId::Clock(id) => crate::win32::x64::clocks::call(id, args, mem, &self.start),
            ApiId::Local(id) => self.local_call(id, args, mem),
            ApiId::SyncObject(id) => self.sync_object_call(id, args, mem),
            ApiId::Timer(id) => self.timer_call(mem, id, args),
            ApiId::VirtualMemory(id) => {
                crate::win32::x64::virtual_memory::call(mem, id, args, self.teb() + 0x68)
            }
            ApiId::Heap(id) => {
                let last_error = self.teb() + 0x68;
                let mut pages = allocation::Pages {
                    next: &mut self.next_alloc,
                    allocations: &mut self.allocations,
                    last_error,
                    trace: &mut self.trace,
                };
                self.guest_heap.call(mem, id, args, &mut pages)
            }
            ApiId::LocalMemory(id) => {
                let last_error = self.teb() + 0x68;
                let mut pages = allocation::Pages {
                    next: &mut self.next_alloc,
                    allocations: &mut self.allocations,
                    last_error,
                    trace: &mut self.trace,
                };
                self.local_memory.call(mem, id, args, &mut pages)
            }
            ApiId::SystemProfile(id) => {
                crate::win32::x64::system_profile::call(mem, id, args, self.teb() + 0x68)
            }
            ApiId::Calendar(id) => {
                crate::win32::x64::calendar::call(mem, id, args, self.teb() + 0x68)
            }
            ApiId::Multimedia(id) => {
                crate::win32::x64::multimedia::call(id, args, mem, &mut self.timer_periods)
            }
            ApiId::ThreadProperty(id) => {
                let error_slot = self.teb() + 0x68;
                self.threads.property_call(mem, id, args, error_slot)
            }
            ApiId::Context(id) => {
                use crate::win32::catalog::ContextApi;
                Ok(match id {
                    ContextApi::ProcessId => PID,
                    ContextApi::ThreadId => self.threads.active,
                    ContextApi::ProcessHandle => u64::MAX,
                    ContextApi::ThreadHandle => u64::MAX - 1,
                    ContextApi::DebuggerPresent => mem.read_int(PEB + 2, 1)?,
                    ContextApi::GetLastError => mem.read_int(self.teb() + 0x68, 4)?,
                    ContextApi::SetLastError => {
                        self.set_error(mem, args[0] as u32)?;
                        0
                    }
                })
            }
        }
    }

    pub fn dispatch(
        &mut self,
        dll: &str,
        name: &str,
        cpu: &mut Cpu64,
        mem: &mut Memory,
    ) -> Result<Control, String> {
        let binding = gateways::BoundImport::from_parts(dll, name);
        self.dispatch_bound(&binding, cpu, mem)
    }
    pub fn dispatch_gateway(
        &mut self,
        cpu: &mut Cpu64,
        mem: &mut Memory,
    ) -> Result<Control, String> {
        let binding = self
            .gateways
            .binding(&cpu.rip)
            .ok_or("unregistered AMD64 gateway")?;
        if !binding.valid {
            return Err("invalid AMD64 gateway".into());
        }
        self.dispatch_bound(&binding, cpu, mem)
    }
    pub(crate) fn dispatch_bound(
        &mut self,
        binding: &gateways::BoundImport,
        cpu: &mut Cpu64,
        mem: &mut Memory,
    ) -> Result<Control, String> {
        let (dll, name) = (&*binding.dll, &*binding.name);
        if !binding.supported {
            return Err(format!("unimplemented Win64 import: {dll}!{name}"));
        }
        let arity = binding
            .arity
            .ok_or_else(|| format!("unimplemented Win64 import: {dll}!{name}"))?;
        if binding.hot != gateways::HotImport::General
            && arity <= 4
            && !self.delay_hooks.contains_key(&cpu.rip)
        {
            return self.dispatch_hot(binding, cpu, mem);
        }
        if arity > 16 {
            return Err("AMD64 gateway argument limit".into());
        }
        let mut argument_buffer = [0; 16];
        let args = &mut argument_buffer[..arity];
        cpu.arguments_into(mem, args)?;
        if name == "D3D.Context.ClearDepthStencilView" {
            // Win64's fourth positional argument is FLOAT in XMM3, not R9.
            args[3] = u32::from_le_bytes(cpu.xmm[3][..4].try_into().unwrap()) as u64;
            args[4] &= 0xff;
        }
        let dwords = binding.dwords;
        for &index in dwords {
            args[index] = args[index] as u32 as u64;
        }
        let mut returned = cpu.clone();
        returned.return_integer(mem, 0)?;
        if let Some((module, handle, iat)) = self.delay_hooks.get(&cpu.rip).cloned() {
            mem.check(handle, 8, WRITE)?;
            mem.check(iat, 8, WRITE)?;
            let module = self.native_module(&module, true)?;
            mem.write_int(handle, 8, module)?;
            mem.write_int(iat, 8, cpu.rip)?;
            self.delay_hooks.remove(&cpu.rip);
        }
        if matches!(name, "SwapChain.Present" | "SwapChain.Present1")
            && self.dxgi_present_needed(mem, name, args)?
        {
            // Keep the CPU at the call until public native presentation succeeds.
            // Native submission failures must not be recorded as guest S_OK.
            return Ok(Control::Present {
                dll: dll.into(),
                function: name.into(),
                args: args.to_vec(),
            });
        }
        use crate::win32::catalog::{ApiId, DelayApi, FileApi, HandleApi};
        let file = match binding.api {
            Some(ApiId::File(id)) => Some(id),
            _ => None,
        };
        let handle = match binding.api {
            Some(ApiId::Handle(id)) => Some(id),
            _ => None,
        };
        let ret = match name {
            _ if binding.api.is_some() && file.is_none() && handle.is_none() => {
                self.catalog_call(mem, binding.api.unwrap(), args)?
            }
            _ if matches!(binding.control, gateways::ControlImport::Module(_)) => {
                let gateways::ControlImport::Module(id) = binding.control else {
                    unreachable!();
                };
                match self.control_module_call(mem, id, args)? {
                    modules::ModuleControlResult::Value(value) => value,
                    modules::ModuleControlResult::Returned { value, unblock } => {
                        returned.regs[0] = value;
                        *cpu = returned;
                        if unblock {
                            self.unblock();
                        }
                        self.api_calls += 1;
                        self.record(dll, name, args, value);
                        return Ok(Control::Returned);
                    }
                    modules::ModuleControlResult::Load { path, flags } => {
                        *cpu = returned;
                        self.api_calls += 1;
                        return Ok(Control::LoadModule {
                            search: (
                                flags,
                                if flags & 8 != 0 {
                                    Some(
                                        path.rsplit_once('\\')
                                            .ok_or("DLL directory is missing")?
                                            .0
                                            .to_owned(),
                                    )
                                } else {
                                    None
                                },
                            ),
                            path,
                            dll: dll.into(),
                            function: name.into(),
                            args: args.to_vec(),
                        });
                    }
                }
            }
            _ if matches!(binding.control, gateways::ControlImport::Message(id) if !id.dispatch()) =>
            {
                let gateways::ControlImport::Message(id) = binding.control else {
                    unreachable!();
                };
                match self.message_call(mem, id, args)? {
                    Some(value) => value,
                    None => return Ok(Control::Blocked),
                }
            }
            _ if binding.graphics.is_some() => {
                self.graphics_method_call(mem, binding.graphics.unwrap(), args)?
            }
            _ if binding.file_search => self.find_call(mem, file.unwrap(), args)?,
            _ if matches!(binding.control, gateways::ControlImport::Window(_))
                || matches!(binding.control, gateways::ControlImport::Message(id) if id.dispatch()) =>
            {
                let result = if let gateways::ControlImport::Window(id) = binding.control {
                    self.window_start(mem, id, name, args, &returned)?
                } else {
                    let gateways::ControlImport::Message(id) = binding.control else {
                        unreachable!();
                    };
                    self.dispatch_window(mem, id, name, args, &returned)?
                };
                match result {
                    user::WindowResult::Value(value) => value,
                    user::WindowResult::Callback {
                        target,
                        parameters,
                        call,
                        release,
                    } => {
                        *cpu = returned;
                        self.unblock();
                        self.api_calls += 1;
                        return Ok(Control::Callbacks {
                            calls: vec![(target, parameters)],
                            release,
                            stop_on_false: false,
                            return_boolean: false,
                            window_call: Some(call),
                        });
                    }
                }
            }
            _ if binding.control == gateways::ControlImport::MonitorEnumeration => {
                let enumeration = self.enumerate_monitors(mem, args, &returned)?;
                if enumeration.calls.is_empty() {
                    enumeration.result
                } else {
                    returned.regs[0] = enumeration.result;
                    *cpu = returned;
                    self.unblock();
                    self.api_calls += 1;
                    self.record(dll, name, args, enumeration.result);
                    return Ok(Control::Callbacks {
                        calls: enumeration.calls,
                        release: enumeration.release,
                        stop_on_false: true,
                        return_boolean: true,
                        window_call: None,
                    });
                }
            }
            _ if matches!(binding.control, gateways::ControlImport::AddressWait(_)) => {
                use crate::win32::catalog::AddressWaitApi;
                let gateways::ControlImport::AddressWait(id) = binding.control else {
                    unreachable!();
                };
                if id == AddressWaitApi::Wait {
                    let last_error = self.teb() + 0x68;
                    match self.threads.wait_address(mem, args, last_error)? {
                        Some(value) => value,
                        None => return Ok(Control::Blocked),
                    }
                } else {
                    self.threads
                        .wake_address(args[0], id == AddressWaitApi::WakeAll);
                    0
                }
            }
            _ if matches!(binding.control, gateways::ControlImport::Exception(_)) => {
                use crate::win32::catalog::ExceptionApi;
                let gateways::ControlImport::Exception(id) = binding.control else {
                    unreachable!()
                };
                match id {
                    ExceptionApi::AddVectored
                    | ExceptionApi::RemoveVectored
                    | ExceptionApi::SetFilter => {
                        let error_slot = self.teb() + 0x68;
                        self.exception_registrations
                            .call(mem, id, args, error_slot)?
                    }
                    ExceptionApi::Raise => {
                        if args[2] > 15 {
                            return Err("Win64 RaiseException parameter count exceeds 15".into());
                        }
                        if args[2] != 0 {
                            mem.check(args[3], args[2] as usize * 8, READ)?;
                        }
                        if args[1] & !1 != 0 || !self.threads.enabled {
                            return Err(
                                "unsupported Win64 software exception flags/scheduler".into()
                            );
                        }
                        if self.exception_registrations.is_empty()
                            && self.initial_exception_handler(mem, &returned)?.is_none()
                        {
                            return Err(format!("Win64 exception search beyond immediate frame is not implemented: code={:#x} flags={:#x}", args[0], args[1]));
                        }
                        return Ok(Control::Raise {
                            dll: dll.into(),
                            args: args.to_vec(),
                            caller: Box::new(returned),
                        });
                    }
                    ExceptionApi::UnwindEx => {
                        return Ok(Control::Unwind {
                            dll: dll.into(),
                            args: args.to_vec(),
                        })
                    }
                    ExceptionApi::AddTable
                    | ExceptionApi::DeleteTable
                    | ExceptionApi::InstallCallback
                    | ExceptionApi::LookupEntry => {
                        match self
                            .function_tables
                            .call(mem, id, args, self.threads.enabled)?
                        {
                            unwind::Result64::Value(value) => value,
                            unwind::Result64::Callback {
                                target,
                                context,
                                base,
                            } => {
                                *cpu = returned;
                                self.api_calls += 1;
                                return Ok(Control::FunctionLookup {
                                    dll: dll.into(),
                                    target,
                                    context,
                                    base,
                                    args: args.to_vec(),
                                });
                            }
                        }
                    }
                    ExceptionApi::UnhandledFilter | ExceptionApi::Unwind => {
                        unreachable!("PE32-only exception controls")
                    }
                }
            }
            _ if matches!(
                file,
                Some(
                    FileApi::MoveFileExW
                        | FileApi::MoveFileExA
                        | FileApi::MoveFileW
                        | FileApi::MoveFileA
                        | FileApi::DeleteFileW
                        | FileApi::DeleteFileA
                        | FileApi::RemoveDirectoryW
                        | FileApi::RemoveDirectoryA
                )
            ) =>
            {
                self.files
                    .mutate_call(mem, file.unwrap(), args, &self.threads)?
            }
            _ if handle == Some(HandleApi::DuplicateHandle) => {
                mem.check(args[3], 8, WRITE)?;
                let copied = (|| -> Result<u64, u32> {
                    if args[0] != u64::MAX || args[2] != u64::MAX {
                        return Err(6);
                    }
                    if args[6] & !3 != 0 {
                        return Err(87);
                    }
                    let desired = if args[6] & 2 != 0 {
                        None
                    } else {
                        Some(args[4] as u32)
                    };
                    if let Some(result) = self.files.standard_mut().duplicate(
                        args[1],
                        desired,
                        args[5] != 0,
                        args[6] & 1 != 0,
                    ) {
                        result
                    } else if self.files.flags(args[1]).is_some() {
                        if args[6] & 1 != 0 && self.files.flags(args[1]).unwrap() & 2 != 0 {
                            return Err(6);
                        }
                        self.files.duplicate(args[1], desired, args[5] != 0)
                    } else if self.mappings.flags(args[1]).is_some() {
                        if args[6] & 1 != 0 && self.mappings.flags(args[1]).unwrap() & 2 != 0 {
                            return Err(6);
                        }
                        self.mappings.duplicate(args[1], desired, args[5] != 0)
                    } else if self.threads.id(args[1]).is_ok() {
                        if desired.is_some() {
                            return Err(50);
                        }
                        self.threads
                            .duplicate(args[1], args[5] != 0, args[6] & 1 != 0)
                    } else if self.events.flags(args[1]).is_some()
                        || self.pipes.flags(args[1]).is_some()
                        || self.snapshots.contains(args[1])
                    {
                        Err(50)
                    } else {
                        Err(6)
                    }
                })();
                match copied {
                    Err(error) => {
                        self.set_error(mem, error)?;
                        0
                    }
                    Ok(handle) => {
                        mem.write_int(args[3], 8, handle)?;
                        if args[6] & 1 != 0 {
                            if self.files.standard().index(args[1]).is_some() {
                                self.files.standard_mut().close(args[1]).map_err(|e| {
                                    format!("close duplicated std handle error {e}")
                                })?;
                            } else if self.mappings.flags(args[1]).is_some() {
                                self.mappings
                                    .close(args[1])
                                    .map_err(|e| format!("close duplicated mapping error {e}"))?;
                            } else if self.threads.handles.contains_key(&args[1]) {
                                self.threads.close(args[1]).map_err(|error| {
                                    format!("close duplicated thread handle error {error}")
                                })?;
                            } else if args[1] != u64::MAX - 1 {
                                self.files
                                    .close(args[1])
                                    .map_err(|e| format!("close duplicated file error {e}"))?;
                            }
                        }
                        1
                    }
                }
            }
            _ if matches!(
                file,
                Some(FileApi::SetFileAttributesW | FileApi::SetFileAttributesA)
            ) =>
            {
                self.files
                    .set_attribute_call(mem, file.unwrap(), args, &self.threads)?
            }
            _ if matches!(
                file,
                Some(FileApi::CreateDirectoryW | FileApi::CreateDirectoryA)
            ) =>
            {
                if args[1] != 0 {
                    // CreateDirectory consumes only lpSecurityDescriptor.
                    // nLength and bInheritHandle do not affect this operation.
                    let descriptor = args[1]
                        .checked_add(8)
                        .ok_or("invalid Win64 directory security attribute address")?;
                    let error = if mem.read_int(descriptor, 8)? != 0 {
                        50 // Explicit security descriptors are not implemented.
                    } else {
                        0
                    };
                    if error != 0 {
                        self.set_error(mem, error)?;
                        returned.regs[0] = 0;
                        *cpu = returned;
                        self.unblock();
                        self.api_calls += 1;
                        self.record(dll, name, args, 0);
                        return Ok(Control::Returned);
                    }
                }
                self.files
                    .create_directory_call(mem, file.unwrap(), args, &self.threads)?
            }
            _ if binding.control == gateways::ControlImport::LocaleEnumeration => {
                if args[0] == 0 || args[3] != 0 || args[1] & !0x37 != 0 {
                    self.set_error(mem, 87)?;
                    0
                } else if !self.threads.enabled {
                    return Err("Win64 locale callback requires a process executor".into());
                } else {
                    let frame = (returned.regs[4]
                        .checked_sub(48)
                        .ok_or("callback stack underflow")?
                        & !15)
                        + 8;
                    mem.check(frame, 40, WRITE)?;
                    let block = self.heap_alloc(mem, 64)?;
                    if block == 0 {
                        0
                    } else {
                        let calls = crate::win32::x64::nls::enumeration_calls(mem, block, args)?;
                        returned.regs[0] = 1;
                        *cpu = returned;
                        self.unblock();
                        self.api_calls += 1;
                        self.record(dll, name, args, 1);
                        return Ok(Control::Callbacks {
                            calls,
                            release: vec![block],
                            stop_on_false: true,
                            return_boolean: false,
                            window_call: None,
                        });
                    }
                }
            }
            _ if matches!(binding.control, gateways::ControlImport::Thread(_)) => {
                let gateways::ControlImport::Thread(id) = binding.control else {
                    unreachable!();
                };
                match self.thread_control_call(mem, id, args)? {
                    threads::ThreadControlResult::Value(value) => value,
                    threads::ThreadControlResult::Exit(code) => {
                        self.api_calls += 1;
                        self.record(dll, name, args, args[0]);
                        return Ok(Control::ThreadExited(code));
                    }
                    threads::ThreadControlResult::Context { thread, flags } => {
                        returned.regs[0] = 1;
                        *cpu = returned;
                        self.api_calls += 1;
                        return Ok(Control::ThreadContext {
                            dll: dll.into(),
                            args: args.to_vec(),
                            thread,
                            flags,
                        });
                    }
                }
            }
            _ if binding.control == gateways::ControlImport::Delay(DelayApi::SleepEx) => {
                if args[1] != 0 {
                    if let Some(control) =
                        self.deliver_apcs(mem, dll, name, args, &returned, cpu)?
                    {
                        return Ok(control);
                    }
                }
                let deadline = match self.threads.entries[&self.threads.active].block {
                    Some(threads::Block::SleepEx { deadline, .. }) => deadline,
                    _ if args[0] == u32::MAX as u64 => None,
                    _ => Some(Instant::now() + std::time::Duration::from_millis(args[0])),
                };
                if args[0] != 0 && deadline.is_none_or(|d| Instant::now() < d) {
                    self.block(threads::Block::SleepEx {
                        deadline,
                        alertable: args[1] != 0,
                    })?;
                    return Ok(Control::Blocked);
                }
                0
            }
            _ if matches!(
                binding.control,
                gateways::ControlImport::Delay(DelayApi::Sleep | DelayApi::SwitchThread)
            ) =>
            {
                if binding.control == gateways::ControlImport::Delay(DelayApi::Sleep)
                    && args[0] != 0
                {
                    let deadline = match &self.threads.entries[&self.threads.active].block {
                        Some(threads::Block::Sleep(d)) => *d,
                        _ => Instant::now() + std::time::Duration::from_millis(args[0]),
                    };
                    if Instant::now() < deadline {
                        self.block(threads::Block::Sleep(deadline))?;
                        return Ok(Control::Blocked);
                    }
                }
                self.unblock();
                returned.regs[0] = 0;
                *cpu = returned;
                self.api_calls += 1;
                if binding.control == gateways::ControlImport::Delay(DelayApi::SwitchThread) {
                    return Ok(Control::SwitchThread(dll.into()));
                }
                self.record(dll, name, args, 0);
                return Ok(Control::Yielded);
            }
            _ if matches!(
                file,
                Some(FileApi::CreateFileMappingW | FileApi::CreateFileMappingA)
            ) =>
            {
                self.mappings
                    .create_call(mem, args, &self.files, &self.threads)?
            }
            _ if matches!(
                file,
                Some(FileApi::MapViewOfFile | FileApi::MapViewOfFileEx)
            ) =>
            {
                self.mappings
                    .map_call(mem, file.unwrap(), args, &self.threads)?
            }
            _ if file == Some(FileApi::UnmapViewOfFile) => {
                self.mappings.unmap_call(mem, args, &self.threads)?
            }
            _ if file == Some(FileApi::FlushViewOfFile) => {
                self.mappings.flush_call(mem, args, &self.threads)?
            }
            _ if matches!(file, Some(FileApi::CreateFileW | FileApi::CreateFileA)) => self
                .files
                .open_call(mem, file.unwrap(), args, &self.mappings, &self.threads)?,
            _ if file == Some(FileApi::NtQueryDirectory) => {
                self.files.nt_query_directory(mem, args)?
            }
            _ if file == Some(FileApi::GetFileInformationByHandleEx)
                && matches!(args[1], 14 | 15) =>
            {
                self.full_directory_info(mem, args)?
            }
            _ if matches!(
                file,
                Some(
                    FileApi::GetFileInformationByHandle
                        | FileApi::GetFileInformationByHandleEx
                        | FileApi::GetFileTime
                        | FileApi::GetFileSizeEx
                        | FileApi::GetFileSize
                        | FileApi::FlushFileBuffers
                        | FileApi::SetFilePointerEx
                        | FileApi::GetFileAttributesW
                        | FileApi::GetFileAttributesA
                        | FileApi::GetFileAttributesExW
                        | FileApi::GetFileAttributesExA
                        | FileApi::SetCurrentDirectoryW
                        | FileApi::SetCurrentDirectoryA
                        | FileApi::GetCurrentDirectoryW
                        | FileApi::GetCurrentDirectoryA
                )
            ) =>
            {
                self.files
                    .query_call(mem, file.unwrap(), args, &self.threads)?
            }
            _ if binding.control
                == gateways::ControlImport::Process(
                    crate::win32::catalog::ProcessControlApi::Create,
                ) =>
            {
                use crate::win32::x64::process_control::{probe_child, ChildProbe};
                match probe_child(mem, args, self.files.filesystem())? {
                    ChildProbe::InvalidStartup => {
                        self.set_error(mem, 87)?;
                        returned.regs[0] = 0;
                        *cpu = returned;
                        self.api_calls += 1;
                        self.record(dll, name, args, 0);
                        return Ok(Control::Returned);
                    }
                    ChildProbe::Missing { application, error } => {
                        self.set_error(mem, error)?;
                        self.trace.push_back(format!(
                            "CreateProcessW executable={application:?} failed error={error}"
                        ));
                        if self.trace.len() > 4096 {
                            self.trace.pop_front();
                        }
                        0
                    }
                }
            }
            _ if file == Some(FileApi::CreatePipe) => {
                let error_slot = self.teb() + 0x68;
                self.pipes.create_call(mem, args, error_slot)?
            }
            _ if handle == Some(HandleApi::GetHandleInformation) => {
                if let Some(flags) = self
                    .pipes
                    .flags(args[0])
                    .or_else(|| self.events.flags(args[0]))
                    .or_else(|| self.files.flags(args[0]))
                    .or_else(|| self.mappings.flags(args[0]))
                    .or_else(|| self.threads.flags(args[0]))
                    .or_else(|| self.snapshots.flags(args[0]))
                    .or_else(|| self.files.standard().flags(args[0]))
                    .or_else(|| self.tokens.flags(args[0]))
                {
                    mem.write_int(args[1], 4, flags as u64)?;
                    1
                } else {
                    self.set_error(mem, 6)?;
                    0
                }
            }
            _ if handle == Some(HandleApi::SetHandleInformation) => {
                if let Some(result) = self.tokens.set_flags(args[0], args[1], args[2]) {
                    let result = match result {
                        Ok(()) => 1,
                        Err(error) => {
                            self.set_error(mem, error)?;
                            0
                        }
                    };
                    returned.regs[0] = result;
                    *cpu = returned;
                    self.api_calls += 1;
                    self.record(dll, name, args, result);
                    return Ok(Control::Returned);
                }
                if let Some(result) = self
                    .files
                    .standard_mut()
                    .set_flags(args[0], args[1], args[2])
                {
                    let result = match result {
                        Ok(()) => 1,
                        Err(error) => {
                            self.set_error(mem, error)?;
                            0
                        }
                    };
                    returned.regs[0] = result;
                    *cpu = returned;
                    self.api_calls += 1;
                    self.record(dll, name, args, result);
                    return Ok(Control::Returned);
                }
                if let Some(result) = self.snapshots.set_flags(args[0], args[1], args[2]) {
                    let result = match result {
                        Ok(()) => 1,
                        Err(error) => {
                            self.set_error(mem, error)?;
                            0
                        }
                    };
                    returned.regs[0] = result;
                    *cpu = returned;
                    self.api_calls += 1;
                    self.record(dll, name, args, result);
                    return Ok(Control::Returned);
                }
                if let Some(flags) = self.threads.handle_flags.get_mut(&args[0]) {
                    if args[1] & !3 != 0 {
                        self.set_error(mem, 87)?;
                        returned.regs[0] = 0;
                        *cpu = returned;
                        self.api_calls += 1;
                        self.record(dll, name, args, 0);
                        return Ok(Control::Returned);
                    }
                    *flags = *flags & !(args[1] as u32) | args[2] as u32 & args[1] as u32;
                    returned.regs[0] = 1;
                    *cpu = returned;
                    self.api_calls += 1;
                    self.record(dll, name, args, 1);
                    return Ok(Control::Returned);
                }
                if self.events.flags(args[0]).is_some() {
                    return Err("Win64 event handle flag updates not implemented".into());
                }
                let result = if self.mappings.flags(args[0]).is_some() {
                    self.mappings
                        .set_flags(args[0], args[1] as u32, args[2] as u32)
                } else if self.files.flags(args[0]).is_some() {
                    self.files
                        .set_flags(args[0], args[1] as u32, args[2] as u32)
                } else {
                    self.pipes
                        .set_flags(args[0], args[1] as u32, args[2] as u32)
                };
                match result {
                    Ok(()) => 1,
                    Err(error) => {
                        self.set_error(mem, error)?;
                        0
                    }
                }
            }
            _ if file == Some(FileApi::ReadFile) => {
                let count = args[2] as usize;
                if args[4] != 0 || count > 16 * 1024 * 1024 || args[3] == 0 {
                    return Err("unsupported Win64 ReadFile overlap/size/result pointer".into());
                }
                mem.check(args[1], count, WRITE)?;
                mem.check(args[3], 4, WRITE)?;
                if self.files.flags(args[0]).is_some() {
                    let result = self.files.read_output(mem, args, count, &self.threads)?;
                    returned.regs[0] = result;
                    *cpu = returned;
                    self.api_calls += 1;
                    self.record(dll, name, args, result);
                    return Ok(Control::Returned);
                }
                self.pipes.read_output(mem, args, count, &self.threads)?
            }
            _ if matches!(binding.control, gateways::ControlImport::Wait(_))
                || binding.control == gateways::ControlImport::WinsockWait =>
            {
                let wait = match binding.control {
                    gateways::ControlImport::Wait(id) => Some(id),
                    _ => None,
                };
                let winsock = wait.is_none();
                if winsock && !self.winsock.initialized() {
                    self.set_error(mem, 10093)?;
                    returned.regs[0] = u32::MAX as u64;
                    *cpu = returned;
                    self.api_calls += 1;
                    self.record(dll, name, args, u32::MAX as u64);
                    return Ok(Control::Returned);
                }
                let multiple =
                    winsock || wait.is_some_and(crate::win32::catalog::WaitApi::multiple);
                let (handles, all, timeout) = if multiple {
                    if args[0] == 0 || args[0] > 64 {
                        self.set_error(mem, if winsock { 10022 } else { 87 })?;
                        returned.regs[0] = u32::MAX as u64;
                        *cpu = returned;
                        self.api_calls += 1;
                        self.record(dll, name, args, u32::MAX as u64);
                        return Ok(Control::Returned);
                    }
                    let mut handles = Vec::new();
                    for n in 0..args[0] {
                        handles.push(mem.read_int(
                            args[1].checked_add(n * 8).ok_or("wait array overflow")?,
                            8,
                        )?);
                    }
                    (handles, args[2] != 0, args[3])
                } else {
                    (vec![args[0]], false, args[1])
                };
                let alertable = (winsock
                    || wait.is_some_and(crate::win32::catalog::WaitApi::alertable))
                    && args[if multiple { 4 } else { 2 }] != 0;
                if alertable && self.wait_objects(&handles, all, false).is_ok() {
                    if let Some(control) =
                        self.deliver_apcs(mem, dll, name, args, &returned, cpu)?
                    {
                        return Ok(control);
                    }
                }
                let deadline = self.wait_deadline(timeout);
                match self.wait_objects(&handles, all, true) {
                    Ok(Some(result)) => {
                        self.unblock();
                        result
                    }
                    Ok(None) if timeout == 0 || deadline.is_some_and(|d| Instant::now() >= d) => {
                        self.unblock();
                        258
                    }
                    Ok(None) => {
                        self.block(threads::Block::Wait {
                            handles,
                            all,
                            deadline,
                            alertable,
                        })?;
                        return Ok(Control::Blocked);
                    }
                    Err(error) => {
                        self.set_error(mem, error)?;
                        u32::MAX as u64
                    }
                }
            }
            _ if matches!(binding.control, gateways::ControlImport::InlineSync(_)) => {
                use crate::win32::x64::inline_sync::{call, Outcome};
                let gateways::ControlImport::InlineSync(id) = binding.control else {
                    unreachable!();
                };
                let last_error = self.teb() + 0x68;
                match call(
                    mem,
                    id,
                    args,
                    &mut self.srw,
                    self.threads.active,
                    last_error,
                )? {
                    Outcome::Value(value) => value,
                    Outcome::TryFailed => {
                        returned.regs[0] = 0;
                        *cpu = returned;
                        self.api_calls += 1;
                        self.record(dll, name, args, 0);
                        return Ok(Control::Returned);
                    }
                    Outcome::BlockSrw { pointer, shared } => {
                        self.block(threads::Block::Srw(pointer, shared))?;
                        return Ok(Control::Blocked);
                    }
                    Outcome::BlockOnce(pointer) => {
                        self.block(threads::Block::Once(pointer))?;
                        return Ok(Control::Blocked);
                    }
                }
            }
            _ if file == Some(FileApi::AreFileApisAnsi) => 1,
            _ if matches!(
                handle,
                Some(HandleApi::GetStdHandle | HandleApi::SetStdHandle)
            ) =>
            {
                self.files.standard_mut().redirect_call(
                    mem,
                    handle.unwrap(),
                    args,
                    &self.threads,
                )?
            }
            _ if file == Some(FileApi::GetFileType)
                || handle == Some(HandleApi::CloseHandle)
                || file == Some(FileApi::GetConsoleMode) =>
            {
                let index = self.files.standard().index(args[0]);
                if index.is_some() {
                    match name {
                        _ if file == Some(FileApi::GetFileType) => 3, // Redirected host pipe, not a console.
                        _ if file == Some(FileApi::GetConsoleMode) => {
                            self.set_error(mem, 6)?;
                            0
                        }
                        _ => match self.files.standard_mut().close(args[0]) {
                            Ok(()) => 1,
                            Err(error) => {
                                self.set_error(mem, error)?;
                                0
                            }
                        },
                    }
                } else if self.files.flags(args[0]).is_some() {
                    match name {
                        _ if file == Some(FileApi::GetFileType) => 1,
                        _ if handle == Some(HandleApi::CloseHandle) => {
                            match self.files.close(args[0]) {
                                Ok(()) => 1,
                                Err(error) => {
                                    self.set_error(mem, error)?;
                                    0
                                }
                            }
                        }
                        _ => {
                            self.set_error(mem, 6)?;
                            0
                        }
                    }
                } else if self.pipes.flags(args[0]).is_some() {
                    match name {
                        _ if file == Some(FileApi::GetFileType) => 3,
                        _ if handle == Some(HandleApi::CloseHandle) => {
                            match self.pipes.close(args[0]) {
                                Ok(()) => 1,
                                Err(error) => {
                                    self.set_error(mem, error)?;
                                    0
                                }
                            }
                        }
                        _ => {
                            self.set_error(mem, 6)?;
                            0
                        }
                    }
                } else if handle == Some(HandleApi::CloseHandle)
                    && self.mappings.flags(args[0]).is_some()
                {
                    match self.mappings.close(args[0]) {
                        Ok(()) => 1,
                        Err(error) => {
                            self.set_error(mem, error)?;
                            0
                        }
                    }
                } else if handle == Some(HandleApi::CloseHandle)
                    && self.tokens.flags(args[0]).is_some()
                {
                    match self.tokens.close(args[0]) {
                        Ok(()) => 1,
                        Err(error) => {
                            self.set_error(mem, error)?;
                            0
                        }
                    }
                } else if handle == Some(HandleApi::CloseHandle) && self.snapshots.contains(args[0])
                {
                    match self.snapshots.close(args[0]) {
                        Ok(()) => 1,
                        Err(error) => {
                            self.set_error(mem, error)?;
                            0
                        }
                    }
                } else if handle == Some(HandleApi::CloseHandle)
                    && self.threads.flags(args[0]).is_some()
                {
                    match self.threads.close(args[0]) {
                        Ok(()) => 1,
                        Err(error) => {
                            self.set_error(mem, error)?;
                            0
                        }
                    }
                } else if handle == Some(HandleApi::CloseHandle) && self.events.close(args[0]) {
                    1
                } else {
                    self.set_error(mem, 6)?;
                    0
                }
            }
            _ if matches!(file, Some(FileApi::WriteFile | FileApi::WriteConsoleW)) => {
                if file == Some(FileApi::WriteFile) && self.files.flags(args[0]).is_some() {
                    let result =
                        self.files
                            .write_output(mem, args, &self.mappings, &self.threads)?;
                    returned.regs[0] = result;
                    *cpu = returned;
                    self.api_calls += 1;
                    self.record(dll, name, args, result);
                    return Ok(Control::Returned);
                }
                if file == Some(FileApi::WriteFile) && self.pipes.flags(args[0]).is_some() {
                    let value = self.pipes.write_output(mem, args, &self.threads)?;
                    returned.regs[0] = value;
                    *cpu = returned;
                    self.api_calls += 1;
                    self.record(dll, name, args, value);
                    return Ok(Control::Returned);
                }
                self.files
                    .standard()
                    .write_output(mem, file.unwrap(), args, &self.threads)?
            }
            _ if matches!(binding.control, gateways::ControlImport::CriticalSection(_)) => {
                use crate::win32::x64::critical_sections::{call, Outcome};
                let gateways::ControlImport::CriticalSection(id) = binding.control else {
                    unreachable!();
                };
                match call(mem, id, args, self.threads.active)? {
                    Outcome::Value(value) => value,
                    Outcome::TryFailed => {
                        returned.regs[0] = 0;
                        *cpu = returned;
                        self.api_calls += 1;
                        self.record(dll, name, args, 0);
                        return Ok(Control::Returned);
                    }
                    Outcome::Block(pointer) => {
                        self.block(threads::Block::Critical(pointer))?;
                        return Ok(Control::Blocked);
                    }
                }
            }
            _ if binding.control
                == gateways::ControlImport::Process(
                    crate::win32::catalog::ProcessControlApi::Exit,
                ) =>
            {
                self.api_calls += 1;
                self.record(dll, name, args, args[0] as u32 as u64);
                return Ok(Control::Exited(args[0] as u32));
            }
            _ => unreachable!(),
        };
        returned.regs[0] = ret;
        *cpu = returned;
        self.unblock();
        self.api_calls += 1;
        self.record(dll, name, args, ret);
        Ok(Control::Returned)
    }
    fn dispatch_hot(
        &mut self,
        binding: &gateways::BoundImport,
        cpu: &mut Cpu64,
        mem: &mut Memory,
    ) -> Result<Control, String> {
        use gateways::HotImport as H;
        let arity = binding.arity.unwrap();
        let mut values = [0; 4];
        for n in 0..arity {
            values[n] = cpu.regs[[1, 2, 8, 9][n]];
        }
        for &n in binding.dwords {
            values[n] = values[n] as u32 as u64;
        }
        let args = &values[..arity];
        // Capture the frame before API writes: an output may alias the return
        // address. No whole CPU copy or argument Vec is needed on this route.
        let target = mem.read_int(cpu.regs[4], 8)?;
        let rsp = cpu.regs[4].checked_add(8).ok_or("AMD64 return overflow")?;
        let ret = match binding.hot {
            H::ThreadId => self.threads.active,
            H::ProcessId => PID,
            H::ThreadHandle => u64::MAX - 1,
            H::ProcessHandle => u64::MAX,
            H::Counter => {
                mem.write_int(args[0], 8, (self.start.elapsed().as_nanos() / 100) as u64)?;
                1
            }
            H::Frequency => {
                mem.write_int(args[0], 8, 10_000_000)?;
                1
            }
            H::Tick64 => self.start.elapsed().as_millis() as u64,
            H::Tick32 => self.start.elapsed().as_millis() as u32 as u64,
            H::ErrorGet => mem.read_int(self.teb() + 0x68, 4)?,
            H::ErrorSet => {
                self.set_error(mem, args[0] as u32)?;
                0
            }
            H::TlsGet | H::TlsSet | H::TlsFree => {
                let index = args[0] as u32 as usize;
                if self.locals.tls(index).is_none() {
                    self.set_error(mem, 87)?;
                    0
                } else if binding.hot == H::TlsGet {
                    let value = mem.read_int(self.teb() + 0x1480 + index as u64 * 8, 8)?;
                    self.set_error(mem, 0)?;
                    value
                } else {
                    let value = if binding.hot == H::TlsFree {
                        0
                    } else {
                        args[1]
                    };
                    mem.write_int(self.teb() + 0x1480 + index as u64 * 8, 8, value)?;
                    self.locals.commit_tls(
                        index,
                        if binding.hot == H::TlsFree {
                            None
                        } else {
                            Some(value)
                        },
                    );
                    1
                }
            }
            H::General => unreachable!(),
        };
        cpu.regs[0] = ret;
        cpu.regs[4] = rsp;
        cpu.rip = target;
        self.unblock();
        self.api_calls += 1;
        self.inventory_record("completed", &binding.dll, &binding.name);
        self.trace
            .call_bound(binding.trace_export.clone(), args, ret);
        Ok(Control::Returned)
    }
    fn record(&mut self, dll: &str, name: &str, args: &[u64], ret: u64) {
        self.inventory_record("completed", dll, name);
        self.trace.call(dll, name, args, ret);
    }
    pub(super) fn trace_note(&mut self, message: String) {
        self.trace.note(message);
    }
}
